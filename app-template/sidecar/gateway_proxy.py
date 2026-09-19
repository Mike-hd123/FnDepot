#!/usr/bin/env python3
"""sidecar gateway_proxy.py — fnOS 统一网关 unix socket -> 后端 HTTP 反代（通用骨架）。

版本演进（EZBookkeeping 实证，直接复用）：
  v4  裸 TCP 盲转发（弃）：路径原样透传，只认根路径的后端回 100001 api not found
  v5  HTTP 层前缀代理：剥 /app/<appname> 前缀转发，重写 Location/相对路径,
      无尾斜杠精确前缀 301 到带斜杠（前端相对资源才能正确解析）
  v7  SW 自杀 + 全资源 no-cache：防已装旧 SW / 代理中间层缓存旧版
  v8  gzip 压缩静态资源：仅客户端声明支持 gzip、且响应体可压缩时压缩，
      流式/已压缩(图片/音视频)跳过；手机远程加载大 JS 显著提速
  v9  缓存策略分流：哈希文件名资源(js/css) -> public, max-age=31536000, immutable；
      HTML/API -> no-cache, max-age=0, must-revalidate
  v12 后端 gzip 已启用时先解压再走管线（防二次 gzip 白屏）+ 静态页兜底白名单

按 app 改：APP_DIR / _PREFIX / BACKEND_PORT 三处即可。
"""
import gzip
import json
import os
import re
import signal
import socket
import sys
import threading
import urllib.error
import urllib.request

APP_DIR = os.environ.get("TRIM_APPDEST", "/usr/local/apps/@appcenter/<appname>")
SOCK_PATH = os.environ.get("GATEWAY_SOCK_PATH", os.path.join(APP_DIR, "app.sock"))
BACKEND_HOST = os.environ.get("GATEWAY_BACKEND_HOST", "127.0.0.1")
BACKEND_PORT = int(os.environ.get("GATEWAY_BACKEND_PORT", "<PORT>"))
PID_FILE = os.environ.get("GATEWAY_PID_FILE", "")

_PREFIX = "/app/<appname>"
_SW_REG_RE = re.compile(r"serviceWorker\.register\(")
_BACKEND_BASE = "http://%s:%d" % (BACKEND_HOST, BACKEND_PORT)
# 静态兜底页白名单（仅当后端不 serve 非 '/' 路径且前端 UA 跳转器需要时启用）
_FALLBACKS = {}   # {"desktop.html": os.path.join(APP_DIR, "server", "public", "desktop.html")}


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
    """RFC3986 相对引用解析（Location/Set-Cookie path 前缀回溯）。ref 无尾斜杠视为文件。"""
    if not ref:
        return base
    if ref.startswith("/"):
        return ref
    scheme_host, _, base_path = base.partition("/")
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


def error_response(status, message):
    body = json.dumps({"error": message, "status": status}, ensure_ascii=False).encode("utf-8")
    return ("HTTP/1.1 %d %s\r\n" % (status, _status_text(status))
            + "Content-Type: application/json\r\n"
            + "Content-Length: %d\r\n" % len(body)
            + "Connection: close\r\n\r\n").encode("ascii") + body


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
                sock.sendall(error_response(400, "request header too large"))
                sock.close()
                return
        head, _, rest = raw.partition(b"\r\n\r\n")
        lines = head.decode("latin-1", "replace").split("\r\n")
        if not lines or not lines[0]:
            sock.sendall(error_response(400, "empty request"))
            sock.close()
            return
        method, target, _ = (lines[0].split(" ", 2) + ["HTTP/1.1"])[:3][:3]
        if " " in target:
            method, target = target, target  # no-op guard
        target = target.split("#", 1)[0]
        path, _, query = target.partition("?")
        query = ("?" + query) if query else ""

        # 无尾斜杠精确前缀 -> 301 带斜杠
        if path == _PREFIX:
            body = ("<!doctype html><html><body><a href='%s/'>Go to %s/</a></body></html>"
                    % (_PREFIX, _PREFIX)).encode("utf-8")
            sock.sendall(("HTTP/1.1 301 Moved Permanently\r\n"
                          + "Location: %s/\r\n" % _PREFIX
                          + "Content-Type: text/html; charset=utf-8\r\n"
                          + "Content-Length: %d\r\n" % len(body)
                          + "Connection: close\r\n\r\n").encode("ascii") + body)
            sock.close()
            return

        fwd_path = _strip_prefix(path)

        # v12 静态兜底（仅当 _FALLBACKS 配了对应白名单文件）
        _fb = _FALLBACKS.get(fwd_path.lstrip("/"))
        if _fb and os.path.isfile(_fb):
            fb_body = open(_fb, "rb").read()
            out = ["Content-Type: text/html; charset=utf-8",
                   "Cache-Control: no-cache, max-age=0, must-revalidate"]
            gz_ok = any("gzip" in ln.lower() for ln in lines[1:] if ":" in ln)
            if gz_ok and len(fb_body) > 256:
                gz = gzip.compress(fb_body)
                if len(gz) < len(fb_body):
                    fb_body, out = gz, out + ["Content-Encoding: gzip", "Vary: Accept-Encoding"]
            sock.sendall(("HTTP/1.1 200 OK\r\n" + "".join(o + "\r\n" for o in out)
                          + "Content-Length: %d\r\n" % len(fb_body)
                          + "Connection: close\r\n\r\n").encode("ascii") + fb_body)
            sock.close()
            return

        hop = {"connection", "keep-alive", "proxy-authenticate", "proxy-authorization",
               "te", "trailer", "transfer-encoding", "upgrade"}
        hdrs, clen = [], 0
        for ln in lines[1:]:
            if not ln or ":" not in ln:
                continue
            k, _, v = ln.partition(":")
            kk = k.strip().lower()
            if kk in hop or kk in ("host", "content-length"):
                continue
            vv = v.strip()
            if kk in ("referer", "origin") and _PREFIX in vv:
                vv = re.sub(r"(https?://[^/]*)?(/" + re.escape(_PREFIX.lstrip("/")) + r")", _PREFIX, vv)
            hdrs.append("%s: %s" % (k.strip(), vv))
            if kk == "content-length":
                try:
                    clen = int(vv)
                except ValueError:
                    clen = 0
        hdrs.append("Host: %s:%d" % (BACKEND_HOST, BACKEND_PORT))
        body = rest
        while len(body) < clen:
            more = sock.recv(16384)
            if not more:
                break
            body += more

        req = urllib.request.Request(_BACKEND_BASE + fwd_path + query,
                                     data=body if method in ("POST", "PUT", "PATCH", "DELETE") else None,
                                     headers={h.split(":", 1)[0].strip(): h.split(":", 1)[1].strip() for h in hdrs},
                                     method=method)
        try:
            resp = urllib.request.urlopen(req, timeout=120)
            status, resp_head, resp_body = resp.status, resp.headers, resp.read()
        except urllib.error.HTTPError as e:
            status, resp_head = e.code, e.headers or []
            try:
                resp_body = e.read()
            except Exception:
                resp_body = b""
        except Exception as e:
            sock.sendall(error_response(502, str(e)))
            sock.close()
            return

        ban = set(hop) | {"content-length", "transfer-encoding", "keep-alive",
                          "cache-control", "etag", "last-modified", "expires", "age"}
        hvals = dict(resp_head.items()) if hasattr(resp_head, "items") else {}
        ctype, resp_enc = "", ""
        out = []
        for k0, v0 in hvals.items():
            k0l = k0.lower()
            if k0l == "content-type":
                ctype = v0
            if k0l == "content-encoding":
                resp_enc = v0.strip().lower()
            if k0l in ban:
                continue
            v = v0
            if k0l in ("location", "content-location"):
                v = _degenerate_relative_path(_PREFIX + "/", v)
                if _BACKEND_BASE in str(v):
                    v = v.replace(_BACKEND_BASE, _PREFIX)
            out.append("%s: %s" % (k0, v))

        resp_bytes = resp_body if isinstance(resp_body, bytes) else resp_body.encode("utf-8", "replace")

        # v12: 后端已 gzip -> 先解压成明文（防二次 gzip 白屏），剥掉 Content-Encoding 再走管线
        if resp_enc == "gzip":
            try:
                resp_bytes = gzip.decompress(resp_bytes)
                out = [o for o in out if not o.lower().startswith("content-encoding:")]
            except Exception:
                pass

        # v7: SW 自杀 + HTML no-cache
        if "html" in ctype.lower():
            try:
                text = resp_bytes.decode("utf-8", "replace")
                if _SW_REG_RE.search(text):
                    resp_bytes = _SW_REG_RE.sub(
                        "navigator.serviceWorker.getRegistrations().then(r=>r.forEach(x=>x.unregister()))",
                        text).encode("utf-8", "replace")
            except Exception:
                pass

        # v8: gzip 静态资源
        client_gzip = any("gzip" in (ln.partition(":")[2] or "").lower() for ln in lines[1:])
        if client_gzip and 256 < len(resp_bytes) < 8 * 1024 * 1024 and any(
                t in ctype.lower() for t in ("javascript", "css", "html", "json", "svg", "xml")):
            try:
                gz = gzip.compress(resp_bytes)
                if len(gz) < len(resp_bytes):
                    resp_bytes, out = gz, out + ["Content-Encoding: gzip", "Vary: Accept-Encoding"]
            except Exception:
                pass

        # v9: 缓存分流
        if any(t in ctype.lower() for t in ("javascript", "css")):
            cc = "Cache-Control: public, max-age=31536000, immutable"
        else:
            cc = "Cache-Control: no-cache, max-age=0, must-revalidate"
        out.append(cc)

        head_out = ("HTTP/1.1 %d %s\r\n" % (status, _status_text(status))
                    + "".join(o + "\r\n" for o in out)
                    + "Content-Length: %d\r\n" % len(resp_bytes)
                    + "Connection: close\r\n\r\n")
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
    os.chmod(SOCK_PATH, 0o666)   # fnOS nginx 非 root 读
    server.listen(128)
    if PID_FILE:
        with open(PID_FILE, "w") as f:
            f.write(str(os.getpid()))

    def _term(sig, frame):
        try:
            server.close()
            os.unlink(SOCK_PATH)
        except OSError:
            pass
        sys.exit(0)

    signal.signal(signal.SIGTERM, _term)
    signal.signal(signal.SIGINT, _term)
    while True:
        try:
            client, _ = server.accept()
        except OSError:
            break
        threading.Thread(target=handle, args=(client,), daemon=True).start()


if __name__ == "__main__":
    run()