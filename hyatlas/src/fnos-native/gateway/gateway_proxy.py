#!/usr/bin/env python3
"""HyAtlas v4 (Go) fnOS gateway sidecar — 剥前缀 HTTP 反代。

手机端 /app/hyatlas 必须经 socket 型入口（trim_sac 只收录自注册 unix socket）。应用监听
TCP 19528 (loopback)，本 sidecar 把 APPDEST/app.sock → 127.0.0.1:19528。

v4 特有改写（照搬自 octopus v8 验证逻辑，参数化到 /app/hyatlas）:
- 剥前缀: /app/hyatlas/<x> → /<x>; 无尾斜杠精确 /app/hyatlas → 301 带斜杠
- Origin → http://127.0.0.1:19528 (模块脚本 crossorigin CORS 校验)
- 转发请求剥 Accept-Encoding (强制后端返明文 + CL)
- JS 响应体: `/api/ 绝对路径 → /app/hyatlas/api/ (fnOS 网关根会被 SPA fallback 吞)
- dashboard 引用相对 assets(/dashboard/assets/...)，反代后无需改前缀；HTML/JS 一律 no-cache
- SSE 流式: event-stream 走原始 socket 增量泵
"""
import json
import os
import re
import signal
import socket
import sys
import threading
import http.client

APP_DIR = os.environ.get("TRIM_APPDEST", "/vol1/@appcenter/hyatlas")
SOCK_PATH = os.environ.get("GATEWAY_SOCK_PATH", os.path.join(APP_DIR, "app.sock"))
BACKEND_PORT = int(os.environ.get("GATEWAY_BACKEND_PORT", "19528"))
BACKEND_HOST = os.environ.get("GATEWAY_BACKEND_HOST", "127.0.0.1")
PID_FILE = os.environ.get("GATEWAY_PID_FILE", "")

_PREFIX = os.environ.get("GATEWAY_PREFIX", "/app/hyatlas")
IO_TIMEOUT = 600

_JS_API_RE = re.compile(rb"['\"`](/api/[A-Za-z0-9_/?=&.:-]+)")


def _api_repl(m):
    return m.group(0)[:1] + _PREFIX.encode() + m.group(0)[1:]

def _strip_prefix(path):
    if path == _PREFIX:
        return "/"
    if path.startswith(_PREFIX + "/"):
        return path[len(_PREFIX):]
    return path

def _status_text(code):
    d = {200: "OK", 201: "Created", 204: "No Content", 301: "Moved Permanently",
         302: "Found", 304: "Not Modified", 400: "Bad Request", 401: "Unauthorized",
         403: "Forbidden", 404: "Not Found", 405: "Method Not Allowed",
         408: "Request Timeout", 413: "Request Entity Too Large", 429: "Too Many Requests",
         500: "Internal Server Error", 502: "Bad Gateway", 503: "Service Unavailable",
         504: "Gateway Timeout"}
    return d.get(code, "Unknown")

def error_response(status, message):
    body = json.dumps({"error": message, "status": status}, ensure_ascii=False).encode("utf-8")
    return ("HTTP/1.1 %d %s\r\n" % (status, _status_text(status))
            + "Content-Type: application/json\r\n"
            + "Content-Length: %d\r\n" % len(body)
            + "Connection: close\r\n\r\n").encode("ascii") + body

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
    if not base_path.endswith("/"):
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
        try:
            parts = headers[0].split(" ")
            if len(parts) >= 2:
                method, target = parts[0], parts[1]
            else:
                return
        except ValueError:
            sock.sendall(error_response(400, "bad request line"))
            return

        target = target.split("#", 1)[0]
        split_q = target.split("?", 1)
        path = split_q[0]
        query = "?" + split_q[1] if len(split_q) > 1 else ""

        if path == _PREFIX:
            body = ("<html><body><a href='%s/'>Go to %s/</a></body></html>"
                    % (_PREFIX, _PREFIX)).encode("utf-8")
            resp = ("HTTP/1.1 301 Moved Permanently\r\n"
                    + "Location: %s/\r\n" % _PREFIX
                    + "Content-Type: text/html; charset=utf-8\r\n"
                    + "Content-Length: %d\r\n" % len(body)
                    + "Connection: close\r\n\r\n").encode("ascii") + body
            sock.sendall(resp)
            return

        fwd_path = _strip_prefix(path)
        # v4 Go 后端把 dashboard 挂在 /dashboard/，根路径 / 是 404 page not found，
        # 且 dashboard HTML 用相对引用 (app.js/js/l5.js) —— 必须让浏览器停在
        # /app/hyatlas/dashboard/ 相对路径才解析正确（内部改写会把 app.js 打到根 404）。
        # 走客户端 302：Location 交给下方 Location 回写逻辑补前缀。
        root_redirect = fwd_path == "/"

        if root_redirect:
            body = ("<html><body><a href='%s/dashboard/'>HyAtlas Dashboard</a></body></html>"
                    % _PREFIX).encode("utf-8")
            resp = ("HTTP/1.1 302 Found\r\n"
                    + "Location: %s/dashboard/\r\n" % _PREFIX
                    + "Content-Type: text/html; charset=utf-8\r\n"
                    + "Content-Length: %d\r\n" % len(body)
                    + "Cache-Control: no-cache\r\n"
                    + "Connection: close\r\n\r\n").encode("ascii") + body
            sock.sendall(resp)
            return
        # dashboard HTML 以绝对路径 /assets/... 引 favicon/logo，后端实体在 /dashboard/assets/
        if fwd_path.startswith("/assets/"):
            fwd_path = "/dashboard" + fwd_path

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
                # 后端绝对路径 Location（如 GET /dashboard 307 → Location: /dashboard/）：
                # 不回写前缀时浏览器会跳到网关根，被 fnOS SPA 吞掉。统一收回 /app/hyatlas 内。
                elif v.startswith("/"):
                    v = _PREFIX + v
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
            modified = False
            if "javascript" in ctype:
                # API 绝对路径 → /app/hyatlas 前缀（fnOS 网关根 SPA fallback 吞 /api/*）
                n = len(_JS_API_RE.findall(body_bytes))
                if n:
                    body_bytes = _JS_API_RE.sub(_api_repl, body_bytes)
                    modified = True
            if modified:
                out = [h for h in out if not h.lower().startswith("content-length:")]
                out.append("Content-Length: %d" % len(body_bytes))
            head_out = ("HTTP/1.1 %d %s\r\n" % (status, reason)
                        + "".join(o + "\r\n" for o in out)
                        + "Connection: close\r\n\r\n")
            sock.sendall(head_out.encode("latin-1", "replace"))
            sock.sendall(body_bytes)
            return

        head_out = ("HTTP/1.1 %d %s\r\n" % (status, reason)
                    + "".join(o + "\r\n" for o in out)
                    + "Connection: close\r\n\r\n")
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
    except Exception as e:
        try:
            sock.sendall(error_response(502, str(e)))
        except OSError:
            pass
    finally:
        if conn:
            conn.close()
        try:
            sock.close()
        except OSError:
            pass

def run():
    if os.path.exists(SOCK_PATH):
        try:
            os.unlink(SOCK_PATH)
        except OSError:
            pass
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.bind(SOCK_PATH)
    os.chmod(SOCK_PATH, 0o666)
    s.listen(128)
    if PID_FILE:
        open(PID_FILE, "w").write(str(os.getpid()))

    def term(*_a):
        try:
            s.close()
            os.unlink(SOCK_PATH)
        except OSError:
            pass
        sys.exit(0)

    signal.signal(signal.SIGTERM, term)
    signal.signal(signal.SIGINT, term)
    while True:
        try:
            c, _ = s.accept()
        except OSError:
            break
        threading.Thread(target=handle, args=(c,), daemon=True).start()

if __name__ == "__main__":
    run()