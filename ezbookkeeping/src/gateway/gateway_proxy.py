#!/usr/bin/env python3
"""EZBookkeeping gateway sidecar — fnOS 统一网关 unix socket -> 后端 HTTP 反代.

对照 EasyTier/gateway-proxy 行为：入站 /app/ezbookkeeping/* 会剥掉前缀后再转发给后端，
并对等重写 Location/刷新头里的路径前缀,使浏览器后续请求（相对绝对路径均可）回落本网关.
无尾斜杠的精确前缀请求 301 到带斜杠（前端相对资源才能正确解析）.

前身是“裸 TCP 盲转发”（v4）——把 /app/ezbookkeeping 原样透传给只认根路径的后端，
导致 EZ 对未知路径回 100001 api not found。本版改为 HTTP 层前缀代理（应用侧改动，系统零接触）.
"""
import json
import os
import re
import signal
import socket
import sys
import threading
import urllib.error
import urllib.request

APP_DIR = os.environ.get("TRIM_APPDEST", "/vol2/@appcenter/ezbookkeeping")
SOCK_PATH = os.environ.get("GATEWAY_SOCK_PATH", os.path.join(APP_DIR, "app.sock"))
BACKEND_PORT = int(os.environ.get("GATEWAY_BACKEND_PORT", "8580"))
BACKEND_HOST = os.environ.get("GATEWAY_BACKEND_HOST", "127.0.0.1")
PID_FILE = os.environ.get("GATEWAY_PID_FILE", "")

_PREFIX = "/app/ezbookkeeping"
_BACKEND_BASE = "http://%s:%d" % (BACKEND_HOST, BACKEND_PORT)
_RE_LOC_BASE = re.compile(r"\b(https?://[^/]*)?(/?)" + re.escape(_PREFIX))


def _strip_prefix(path):
    if path == _PREFIX:
        return "/"
    if path.startswith(_PREFIX + "/"):
        return path[len(_PREFIX):]
    return path





def _status_text(code):
    d = {200: "OK", 301: "Moved Permanently", 302: "Found", 303: "See Other",
         304: "Not Modified", 307: "Temporary Redirect", 308: "Permanent Redirect",
         400: "Bad Request", 401: "Unauthorized", 403: "Forbidden",
         404: "Not Found", 405: "Method Not Allowed", 413: "Request Entity Too Large",
         429: "Too Many Requests", 500: "Internal Server Error", 502: "Bad Gateway",
         503: "Service Unavailable", 504: "Gateway Timeout"}
    return d.get(code, "Unknown")


def _degenerate_relative_path(base, ref):
    """RFC3986 相对引用解析（网关侧对 Location/Set-Cookie path 做前缀回溯）. ref 若无尾斜杠则视为文件"""
    if not ref:
        return base
    if ref.startswith("/"):
        return ref
    scheme_host, sep, base_path = base.partition("/")
    if not base_path:
        return ref
    if base_path.endswith("/") is False:
        base_path = base_path.rsplit("/", 1)[0] or "/"
    merged = (base_path + "/" + ref) if base_path.endswith("/") else (base_path + ref)
    parts = []
    for seg in merged.split("/"):
        if seg in ("", "."):
            continue
        if seg == "..":
            if parts:
                parts.pop()
        else:
            parts.append(seg)
    return "/" + "/".join(parts)


def _bad_request(reason):
    return error_response(400, reason)


def error_response(status, message):
    body = json.dumps({"error": message, "status": status}, ensure_ascii=False)
    bodyb = body.encode("utf-8")
    return (
        "HTTP/1.1 %d %s\r\n" % (status, _status_text(status))
        + "Content-Type: application/json\r\n"
        + "Content-Length: %d\r\n" % len(bodyb)
        + "Connection: close\r\n\r\n"
    ).encode("ascii") + bodyb


def handle(sock):
    try:
        raw = b""
        while b"\r\n\r\n" not in raw and b"\n\n" not in raw:
            chunk = sock.recv(16384)
            if not chunk:
                sock.close()
                return
            raw += chunk
            if len(raw) > 128 * 1024:
                sock.sendall(_bad_request("request header too large"))
                sock.close()
                return

        head, _, rest = raw.partition(b"\r\n\r\n")
        if not rest and b"\n\n" in head:
            head, _, rest = head.partition(b"\n\n")
        lines = head.decode("latin-1", "replace").split("\r\n")
        if not lines or not lines[0]:
            sock.sendall(_bad_request("empty request"))
            sock.close()
            return
        try:
            method, target, httpver = lines[0].split(" ", 2)
        except ValueError:
            method, target, httpver = lines[0].split(" ", 1) + ("HTTP/1.1",)

        target = target.split("#", 1)[0]
        split_q = target.split("?", 1)
        path = split_q[0]
        query = "?" + split_q[1] if len(split_q) > 1 else ""

        # —— 无尾斜杠精确前缀 → 301 到带斜杠 ——
        if path == _PREFIX:
            body = ("<!doctype html><html><body><a href='%s/'>Go to %s/</a></body></html>"
                    % (_PREFIX, _PREFIX)).encode("utf-8")
            resp = (
                "HTTP/1.1 301 Moved Permanently\r\n"
                + "Location: %s/\r\n" % _PREFIX
                + "Content-Type: text/html; charset=utf-8\r\n"
                + "Content-Length: %d\r\n" % len(body)
                + "Connection: close\r\n\r\n"
            ).encode("ascii") + body
            sock.sendall(resp)
            sock.close()
            return

        fwd_path = _strip_prefix(path)

        # 转发头：Host 改后端，Host 头重写为 back-host
        hop = {"connection", "keep-alive", "proxy-authenticate", "proxy-authorization",
               "te", "trailer", "transfer-encoding", "upgrade"}
        hdrs = []
        for ln in lines[1:]:
            if not ln or ":" not in ln:
                continue
            k, _, v = ln.partition(":")
            kk = k.strip().lower()
            if kk in hop or kk in ("host", "content-length"):
                continue
            vv = v.strip()
            # 前端绝对前缀引用回写网关
            if kk in ("referer", "origin"):
                if _PREFIX in vv:
                    vv = re.sub(r"(https?://[^/]*)?(/" + re.escape(_PREFIX.lstrip("/")) + r")",
                                _PREFIX, vv)
            hdrs.append("%s: %s" % (k.strip(), vv))
        hdrs.append("Host: %s:%d" % (BACKEND_HOST, BACKEND_PORT))

        body = rest
        clen = 0
        for ln in lines[1:]:
            if ln.lower().startswith("content-length:"):
                try:
                    clen = int(ln.split(":", 1)[1].strip())
                except ValueError:
                    clen = 0
        # 流式收集请求体
        if body:
            while len(body) < clen:
                more = sock.recv(16384)
                if not more:
                    break
                body += more

        req = urllib.request.Request(
            _BACKEND_BASE + fwd_path + query,
            data=body if method in ("POST", "PUT", "PATCH", "DELETE") else None,
            headers={h.split(":", 1)[0].strip(): h.split(":", 1)[1].strip() for h in hdrs},
            method=method,
        )
        try:
            resp = urllib.request.urlopen(req, timeout=120)
            status = resp.status
            resp_head = resp.headers
            resp_body = resp.read()
        except urllib.error.HTTPError as e:
            status = e.code
            resp_head = e.headers or []
            try:
                resp_body = e.read()
            except Exception:
                resp_body = b""
        except Exception as e:
            sock.sendall(error_response(502, str(e)))
            sock.close()
            return

        # 过滤 hop-by-hop 头
        out = []
        ban = set(hop) | {"content-length", "transfer-encoding", "keep-alive"}
        hvals = dict(resp_head.items()) if hasattr(resp_head, "items") else {}
        for k0, v0 in (hvals.items() if isinstance(hvals, dict) else []):
            k0l = k0.lower()
            if k0l in ban:
                continue
            v = v0
            if k0l in ("location", "content-location"):
                v = _degenerate_relative_path(_PREFIX + "/", v)
                # 若后端回绝对后端地址则改成网关
                if _BACKEND_BASE in v:
                    v = v.replace(_BACKEND_BASE, _PREFIX)
            out.append("%s: %s" % (k0, v))

        reason = _status_text(status)
        resp_bytes = resp_body if isinstance(resp_body, bytes) else resp_body.encode("utf-8", "replace")
        head_out = (
            "HTTP/1.1 %d %s\r\n" % (status, reason)
            + "".join(o + "\r\n" for o in out)
            + "Content-Length: %d\r\n" % len(resp_bytes)
            + "Connection: close\r\n\r\n"
        )
        sock.sendall(head_out.encode("ascii", "replace") + resp_bytes)
    except Exception:
        try:
            sock.sendall(error_response(500, "internal gateway error"))
        except Exception:
            pass
    finally:
        try:
            sock.close()
        except Exception:
            pass


def run():
    if os.path.exists(SOCK_PATH):
        try:
            os.unlink(SOCK_PATH)
        except OSError:
            pass
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(SOCK_PATH)
    os.chmod(SOCK_PATH, 0o666)
    server.listen(128)
    if PID_FILE:
        with open(PID_FILE, "w") as f:
            f.write(str(os.getpid()))

    def handle_sigterm(sig, frame):
        try:
            server.close()
            os.unlink(SOCK_PATH)
        except OSError:
            pass
        sys.exit(0)

    signal.signal(signal.SIGTERM, handle_sigterm)
    signal.signal(signal.SIGINT, handle_sigterm)
    while True:
        try:
            client, _ = server.accept()
        except OSError:
            break
        threading.Thread(target=handle, args=(client,), daemon=True).start()


if __name__ == "__main__":
    run()