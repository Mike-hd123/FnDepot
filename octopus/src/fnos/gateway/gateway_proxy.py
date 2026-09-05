#!/usr/bin/env python3
"""Octopus gateway sidecar v8 — 修复 v7 cache-bust 引发的 ES 模块双实例白屏。

根因（2026-09-05 CDP 隔离实验闭环实锤，t_7cc29fd6）：
- v7 只给 HTML 里 3 个 ./assets/ 引用注入 ?v=v7，但 ES 模块图内部的相对引用
  （静态 import "./chunk.js"、动态 import(`./page.js`)、__vite__mapDeps 预载表）
  由 JS 模块解析器按"引用所在模块的 URL"解析，不继承 HTML 注入的 query；
- 于是 entry(index) 以 index.js?v=v7 执行，其内部 import 的 chunk 全是裸 URL
  → 同一份代码被浏览器当成两个不同模块各执行一次（双实例）；
- dist(React 运行时)双实例 → ThemeProvider 的 Context 对象出现两份 →
  home 组件 useContext(theme) 拿到 null → 抛 "useTheme must be used within ThemeProvider"
  → React 卸载失败连锁 NotFoundError: removeChild → 整页白屏（桌面/手机都白，与宽度无关）。

验证方式（隔离实验）：
- 纯透传代理(不改动任何字节)走同源 LAN IP → 桌面/手机均正常渲染；
- 同一后端经 v7 sidecar（本地 unix socket bridge）→ 桌面/手机均白屏、同样异常；
- socket 层仅剥掉 ?v=v7（单实例）→ 完全恢复；
- v8 改写（全模块图统一 ?v=v8）→ 完全恢复，资源链单一 URL。

v8 相对 v7：
- HTML：./assets/ 引用注入 ?v=v8；残留的旧 ?v=v7 一并升级为 ?v=v8（cache-bust 递进）；
- JS：引号内相对 chunk 引用（"./xxx.js" / `./xxx.js` / './xxx.js'，含 js/css）
  统一注入同一版本参数 → 全模块图单一 URL 空间 → 单实例；
- 其余（前缀剥离、Origin 重写、`/api/v1/` 前缀改写、no-cache 降级、流式泵、SW 自杀注入）与 v7 一致。
"""
import json
import os
import re
import signal
import socket
import sys
import threading
import http.client

APP_DIR = os.environ.get("TRIM_APPDEST", "/vol1/@appcenter/octopus")
SOCK_PATH = os.environ.get("GATEWAY_SOCK_PATH", os.path.join(APP_DIR, "app.sock"))
BACKEND_PORT = int(os.environ.get("GATEWAY_BACKEND_PORT", "8081"))
BACKEND_HOST = os.environ.get("GATEWAY_BACKEND_HOST", "127.0.0.1")
PID_FILE = os.environ.get("GATEWAY_PID_FILE", "")

_PREFIX = "/app/octopus"
IO_TIMEOUT = 600
_ASSET_VER = "v8"  # cache-bust：HTML 与 JS 模块图统一使用，保证单实例

_ASSET_RE = re.compile(r'(src|href)="(\./assets/[^"?]+)"')
# v8：JS 模块图内部相对引用（仅当 "./ 开头且 .js/.css 结尾，避免命中已带参数与正文文本）
_JS_ASSET_RE = re.compile(rb'((?:\./)[\w\-./]+\.(?:js|css))(?=["\'`])')
_SW_REG_RE = re.compile(r"serviceWorker\.register\(`/sw\.js`,?\{scope:`/`\}")

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
         402: "Payment Required", 403: "Forbidden", 404: "Not Found",
         405: "Method Not Allowed", 408: "Request Timeout",
         413: "Request Entity Too Large", 429: "Too Many Requests",
         500: "Internal Server Error", 502: "Bad Gateway", 503: "Service Unavailable",
         504: "Gateway Timeout"}
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
                return bytes(out) if not size_str else bytes(out)
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

        # —— 无尾斜杠精确前缀 → 301 到带斜杠 ——
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

        # —— 后端请求 ——
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

        # —— 响应头回写 ——
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
        # HTML/JS 是完整可读资源，不能当流跳过 —— 只有 event-stream 才是真流。
        is_stream = "event-stream" in ctype

        # 所有静态资源/HTML/JS 一律 no-cache（浏览器每次重新校验）
        out.append("Cache-Control: no-cache, max-age=0, must-revalidate")

        if not is_stream and clen_hdr:
            out.append("Content-Length: %s" % clen_hdr)

        # —— 体级重写：HTML 注入/升级 ?v= 版本参数；JS 重写 API 路径 + 统一模块图版本 ——
        if not is_stream:
            # 后端可能 chunked(无 CL) —— 读全量 body（HTML/JS 均完整可读）
            if clen_hdr:
                body_bytes = resp.read(int(clen_hdr))
            else:
                body_bytes = resp.read()
            original = body_bytes
            modified = False
            if "html" in ctype:
                # 1) SW 注册改造成"自杀式"（激活即清缓存自注销），兜底清除旧 SW 缓存
                new = _SW_REG_RE.sub(
                    "navigator.serviceWorker.getRegistrations().then(r=>r.forEach(x=>x.unregister()))",
                    body_bytes.decode("utf-8", "replace"))
                # 2) 旧版本参数递进升级（v7 → v8），防浏览器沿用旧缓存 URL
                new = new.replace("?v=v7", "?v=" + _ASSET_VER)
                # 3) asset URL 注入当前版本参数
                new = _ASSET_RE.sub(r'\1="\2?v=' + _ASSET_VER + '"', new)
                new_b = new.encode("utf-8")
                if new_b != original:
                    body_bytes = new_b
                    modified = True
            elif "javascript" in ctype:
                # 3a) API 路径前缀改写（继承 v6/v7）
                n_sub = body_bytes.count(b"`/api/v1/")
                if n_sub:
                    body_bytes = body_bytes.replace(b"`/api/v1/", b"`" + _PREFIX.encode() + b"/api/v1/")
                # 3b) v8 核心：模块图内相对 chunk 引用统一注入版本参数（单实例）
                new_b = _JS_ASSET_RE.sub(rb"\1?v=" + _ASSET_VER.encode(), body_bytes)
                if new_b != original:
                    body_bytes = new_b
                    modified = True
            if modified:
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
