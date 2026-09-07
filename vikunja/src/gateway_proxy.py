#!/usr/bin/env python3
"""Vikunja gateway sidecar v9 — 剥前缀 HTTP 反代（octopus v8 模式适配）。

手机端 /app/vikunja/* → 127.0.0.1:<port>/*（剥前缀）。
桌面 iframe 直连 :3456 不经本进程（publicurl 注入的绝对 API_URL 原样有效）。

体级改写（仅网关流量）：
- HTML: 绝对 href/src="/..." → "/app/vikunja/..."（assets/favicon/manifest/icons）
- HTML: window.API_URL = '<绝对URL>' → '/api/v1'（publicurl 注入撞网关根，改相对同源）
- HTML: SW 自杀注入（网关路径下注销 PWA service worker，防缓存串味；octopus v7 教训）
- JS:   backtick 内 `/api/v1`、`/api/v2/` 绝对引用 → 前缀化（config/fetcher chunk 实测仅 2 处）

其余沿用 octopus v8 成熟件：301 无尾斜杠、Origin 重写、Accept-Encoding 剥离、
非流式保留 Content-Length、SSE 原始 socket 增量泵、no-cache。
"""
import json
import os
import re
import signal
import socket
import sys
import threading
import http.client

APP_DIR = os.environ.get("TRIM_APPDEST", "/vol1/@appcenter/vikunja")
SOCK_PATH = os.environ.get("GATEWAY_SOCK_PATH", os.path.join(APP_DIR, "app.sock"))
BACKEND_PORT = int(os.environ.get("GATEWAY_BACKEND_PORT", "3456"))
BACKEND_HOST = os.environ.get("GATEWAY_BACKEND_HOST", "127.0.0.1")
PID_FILE = os.environ.get("GATEWAY_PID_FILE", "")

_PREFIX = "/app/vikunja"
IO_TIMEOUT = 600

# HTML 绝对资源引用（src|href="/..."，不含已是 // 或相对）
_HTML_ABS_RE = re.compile(rb'((?:src|href)=")/(?=[^/])')
# HTML window.API_URL 注入行 → 重写为带网关前缀路径。
# 关键：前端的「leading-slash 自动补 host」逻辑会把 '/app/vikunja/api/v1' 拼成
# 'http://<当前host>/app/vikunja/api/v1'（远程 fnos.net 域名也自适应），v2 推导
# replace(/\/api\/v1\/$/,...) 只动尾部、前缀保留，Q() endsWith('/api/v1') 校验通过。
# （勿改 JS chunk 常量：fetcher 的 v2 替换串若前缀化会产生双重前缀。）
_API_URL_RE = re.compile(r"(window\.API_URL\s*=\s*)['\"][^'\"]*['\"]")
# JS backtick 内绝对 api 引用（保留定义供诊断，运行时不再改写 JS）
_JS_API_RE = re.compile(rb"(`)/(api/v[12]/?)")
_SW_KILL = (b"<script>if(navigator.serviceWorker){navigator.serviceWorker.getRegistrations()"
            b".then(function(r){r.forEach(function(x){x.unregister()})})}</script>")


def _strip_prefix(path):
    if path == _PREFIX:
        return "/"
    if path.startswith(_PREFIX + "/"):
        return path[len(_PREFIX):]
    return path


def _status_text(code):
    d = {200: "OK", 201: "Created", 204: "No Content", 301: "Moved Permanently",
         302: "Found", 303: "See Other", 304: "Not Modified", 307: "Temporary Redirect",
         308: "Permanent Redirect", 400: "Bad Request", 401: "Unauthorized",
         403: "Forbidden", 404: "Not Found", 405: "Method Not Allowed",
         408: "Request Timeout", 429: "Too Many Requests", 500: "Internal Server Error",
         502: "Bad Gateway", 503: "Service Unavailable", 504: "Gateway Timeout"}
    return d.get(code, "Unknown")


def error_response(status, message):
    body = json.dumps({"error": message, "status": status}, ensure_ascii=False).encode("utf-8")
    return (
        "HTTP/1.1 %d %s\r\n" % (status, _status_text(status))
        + "Content-Type: application/json\r\n"
        + "Content-Length: %d\r\n" % len(body)
        + "Connection: close\r\n\r\n"
    ).encode("ascii") + body


HOP = {"connection", "keep-alive", "proxy-authenticate", "proxy-authorization",
       "te", "trailer", "transfer-encoding", "upgrade"}


def _degenerate_relative_path(base, ref):
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


def _read_headers(sock):
    raw = b""
    while b"\r\n\r\n" not in raw:
        chunk = sock.recv(16384)
        if not chunk:
            return None, b""
        raw += chunk
        if len(raw) > 256 * 1024:
            sock.sendall(error_response(400, "request header too large"))
            return None, b""
    head, _, rest = raw.partition(b"\r\n\r\n")
    return head.decode("latin-1", "replace").split("\r\n"), rest


def _read_body(sock, headers, leftover):
    clen = None
    chunked = False
    for ln in headers[1:]:
        k, _, v = ln.partition(":")
        kk = k.strip().lower()
        if kk == "content-length":
            try:
                clen = int(v.strip())
            except ValueError:
                clen = None
        elif kk == "transfer-encoding" and "chunked" in v.lower():
            chunked = True
    body = leftover
    if chunked:
        buf = body
        out = bytearray()
        while True:
            line_end = buf.find(b"\r\n")
            while line_end < 0:
                more = sock.recv(65536)
                if not more:
                    return bytes(out)
                buf += more
                line_end = buf.find(b"\r\n")
            size_str = buf[:line_end].split(b";")[0].strip()
            try:
                size = int(size_str, 16)
            except ValueError:
                return bytes(out)
            buf = buf[line_end + 2:]
            if size == 0:
                return bytes(out)
            while len(buf) < size + 2:
                more = sock.recv(65536)
                if not more:
                    return bytes(out)
                buf += more
            out += buf[:size]
            buf = buf[size + 2:]
    if clen is not None:
        while len(body) < clen:
            more = sock.recv(65536)
            if not more:
                break
            body += more
        return body[:clen]
    return body


def _rewrite_html(body_bytes):
    new = body_bytes
    # 1) window.API_URL → 带前缀同源路径（手机/远程网关路径通用）
    try:
        text = new.decode("utf-8", "replace")
        text2 = _API_URL_RE.sub("\\1'" + _PREFIX + "/api/v1'", text)
        if text2 != text:
            new = text2.encode("utf-8")
    except Exception:
        pass
    # 2) 绝对资源引用加网关前缀
    new = _HTML_ABS_RE.sub(rb"\1" + _PREFIX.encode() + rb"/", new)
    # 3) SW 自杀注入 + 子路径 SPA shim（<head> 后，顺序无关）
    if b"<head>" in new:
        new = new.replace(b"<head>", b"<head>" + _SW_KILL, 1)
    return new


def handle(sock):
    conn = None
    try:
        sock.settimeout(IO_TIMEOUT)
        headers, leftover = _read_headers(sock)
        if headers is None or not headers or not headers[0]:
            try:
                sock.sendall(error_response(400, "empty request"))
            except OSError:
                pass
            return
        parts = headers[0].split(" ")
        if len(parts) < 2:
            return
        method, target = parts[0], parts[1]

        target = target.split("#", 1)[0]
        split_q = target.split("?", 1)
        path = split_q[0]
        query = "?" + split_q[1] if len(split_q) > 1 else ""

        # 无尾斜杠精确前缀 → 301 带斜杠
        if path == _PREFIX:
            body = ("<html><body><a href='%s/'>Go to %s/</a></body></html>"
                    % (_PREFIX, _PREFIX)).encode("utf-8")
            resp = (
                "HTTP/1.1 301 Moved Permanently\r\n"
                + "Location: %s/\r\n" % _PREFIX
                + "Content-Type: text/html; charset=utf-8\r\n"
                + "Content-Length: %d\r\n" % len(body)
                + "Connection: close\r\n\r\n"
            ).encode("ascii") + body
            sock.sendall(resp)
            return

        fwd_path = _strip_prefix(path)

        # sw.js 网关路径下打空：应用注册到的是 /app/vikunja/sw.js，空实现 = 零拦截零缓存；
        # 配合 HTML 自杀脚本（注销旧注册）双保险，PWA 缓存在网关路径永不生效。
        if fwd_path == "/sw.js":
            no_sw = b"/* disabled behind gateway prefix */"
            resp = (
                "HTTP/1.1 200 OK\r\n"
                + "Content-Type: text/javascript; charset=utf-8\r\n"
                + "Cache-Control: no-store\r\n"
                + "Content-Length: %d\r\n" % len(no_sw)
                + "Connection: close\r\n\r\n"
            ).encode("ascii") + no_sw
            sock.sendall(resp)
            return

        fwd_hdrs = []
        for ln in headers[1:]:
            if not ln or ":" not in ln:
                continue
            k, _, v = ln.partition(":")
            kk = k.strip().lower()
            if kk in HOP or kk in ("host", "content-length", "expect", "accept-encoding"):
                continue
            v = v.strip()
            if kk == "origin":
                v = "http://%s:%d" % (BACKEND_HOST, BACKEND_PORT)
            fwd_hdrs.append((k.strip(), v))

        body = _read_body(sock, headers, leftover)

        conn = http.client.HTTPConnection(BACKEND_HOST, BACKEND_PORT, timeout=IO_TIMEOUT)
        conn.putrequest(method, fwd_path + query, skip_host=True, skip_accept_encoding=True)
        conn.putheader("Host", "%s:%d" % (BACKEND_HOST, BACKEND_PORT))
        sent_names = set()
        for k, v in fwd_hdrs:
            if k.lower() in sent_names:
                continue
            sent_names.add(k.lower())
            conn.putheader(k, v)
        if body:
            conn.putheader("Content-Length", str(len(body)))
        elif method in ("POST", "PUT", "PATCH"):
            conn.putheader("Content-Length", "0")
        conn.endheaders(body if body else None)
        resp = conn.getresponse()

        status = resp.status
        reason = resp.reason or _status_text(status)
        out = []
        ban = HOP | {"content-length", "cache-control", "etag", "last-modified", "expires", "age"}
        for k0, v0 in resp.getheaders():
            k0l = k0.lower()
            if k0l in ban:
                continue
            v = v0
            if k0l in ("location", "content-location"):
                v = _degenerate_relative_path(_PREFIX + "/", v)
                backend_base = "http://%s:%d" % (BACKEND_HOST, BACKEND_PORT)
                if backend_base in v:
                    v = v.replace(backend_base, _PREFIX)
            out.append("%s: %s" % (k0, v))

        clen_hdr = resp.headers.get("Content-Length")
        ctype = (resp.headers.get("Content-Type") or "").lower()
        is_stream = "event-stream" in ctype
        out.append("Cache-Control: no-cache, max-age=0, must-revalidate")
        if not is_stream and clen_hdr:
            out.append("Content-Length: %s" % clen_hdr)

        if not is_stream:
            if clen_hdr:
                body_bytes = resp.read(int(clen_hdr))
            else:
                body_bytes = resp.read()
            original = body_bytes
            if "html" in ctype:
                new_b = _rewrite_html(body_bytes)
                if new_b != original:
                    body_bytes = new_b
            # JS 改写（v9）：vue-router history 工厂 base 硬编码 `/`，网关路径下
            # 必须注入真实前缀，否则站内跳转产生裸路径 → 刷新掉回 fnOS 根页面。
            # 仅网关流量（带 /app/vikunja 前缀）改写：桌面直连形态（无前缀）原样透传。
            # 锚点唯一（router chunk 工厂调用处），上游重构建改名也能匹配（\w+ 容忍）。
            if "javascript" in ctype and path.startswith(_PREFIX):
                body_bytes, n_sub = re.subn(rb"history:(\w+)\(`/`\)",
                                            rb"history:\1(`/app/vikunja/`)", body_bytes)
                if n_sub:
                    print("router base patched x%d: %s" % (n_sub, fwd_path), flush=True)
                # v10: vite preload-helper base hardcodes root abs `/assets` (dynamic-chunk URL loses prefix on gateway)
                body_bytes, n2 = re.subn(rb"return`/`\+e", rb"return`../`+e", body_bytes)
                if n2:
                    print("preload base patched x%d: %s (JS)" % (n2, fwd_path), flush=True)
            if "css" in ctype and path.startswith(_PREFIX):
                # v10: CSS abs url(/assets/...) fonts/backgrounds -> prefix (static assets, gateway 301 loses prefix)
                body_bytes, n3 = re.subn(rb"url\(/assets/", rb"url(" + _PREFIX.encode() + rb"/assets/", body_bytes)
                if n3:
                    print("css url prefixed x%d: %s" % (n3, fwd_path), flush=True)
            # API base 不改 JS：全部经 HTML 的 window.API_URL 注入。
            if body_bytes != original:
                out = [h for h in out if not h.lower().startswith("content-length:")]
                out.append("Content-Length: %d" % len(body_bytes))
            head_out = (
                "HTTP/1.1 %d %s\r\n" % (status, reason)
                + "".join(o + "\r\n" for o in out)
                + "Connection: close\r\n\r\n"
            )
            sock.sendall(head_out.encode("latin-1", "replace"))
            sock.sendall(body_bytes)
            return

        head_out = (
            "HTTP/1.1 %d %s\r\n" % (status, reason)
            + "".join(o + "\r\n" for o in out)
            + "Connection: close\r\n\r\n"
        )
        sock.sendall(head_out.encode("latin-1", "replace"))

        if is_stream:
            raw_fp = resp.fp.raw if hasattr(resp.fp, "raw") else resp.fp
            raw_sock = getattr(raw_fp, "_sock", None) or raw_fp
            if hasattr(raw_sock, "settimeout"):
                raw_sock.settimeout(IO_TIMEOUT)
            try:
                while True:
                    chunk = raw_fp.read1(65536) if hasattr(raw_fp, "read1") else raw_fp.read(65536)
                    if not chunk:
                        break
                    sock.sendall(chunk)
            except (socket.timeout, OSError):
                pass
        else:
            remaining = int(clen_hdr)
            while remaining > 0:
                chunk = resp.read(min(65536, remaining))
                if not chunk:
                    break
                sock.sendall(chunk)
                remaining -= len(chunk)
    except (BrokenPipeError, ConnectionResetError):
        pass
    except socket.timeout:
        try:
            sock.sendall(error_response(504, "gateway timeout"))
        except OSError:
            pass
    except Exception as e:
        try:
            sock.sendall(error_response(502, str(e)))
        except OSError:
            pass
    finally:
        if conn is not None:
            try:
                conn.close()
            except Exception:
                pass
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
