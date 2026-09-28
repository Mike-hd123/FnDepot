#!/usr/bin/env python3
"""Emby-In-One fnOS gateway sidecar — 剥前缀 HTTP 反代。

emby-in-one (Go) 监听 TCP 18096 (loopback)，本 sidecar 把 APPDEST/app.sock → 127.0.0.1:18096。
fnOS 桌面走 /app/emby/ 前缀，sidecar 剥前缀后转发后端。

路径重写规则（emby admin SPA 不支持 base URL，JS 里全是绝对路径）:
- 剥前缀: /app/emby/<x> → /<x>; 无尾斜杠精确 /app/emby → 301 带斜杠
- 根路径 / → 302 → /app/emby/admin/（admin 后台才是桌面入口）
- JS 响应体: 绝对路径重写
    /admin/api/  → /app/emby/admin/api/
    /admin/      → /app/emby/admin/  (非 /admin/api 的其他 admin 引用)
    /Users/      → /app/emby/Users/  (Emby 客户端登录 API)
    /libraries   → /app/emby/libraries
    /reconnect   → /app/emby/reconnect
- HTML 响应体: href="/admin" → href="/app/emby/admin"（/emby/ 占位页里的跳转链接）
- Location 头: 后端绝对路径 → 补 /app/emby 前缀
- Origin → http://127.0.0.1:18096 (CORS)
- 转发剥 Accept-Encoding (强制后端返明文 + CL)
- SSE 流式: event-stream 走原始 socket 增量泵
"""
import json
import os
import re
import signal
import socket
import sys
import threading
import time
import http.client

APP_DIR = os.environ.get("TRIM_APPDEST", "/vol1/@appcenter/emby-in-one")
SOCK_PATH = os.environ.get("GATEWAY_SOCK_PATH", os.path.join(APP_DIR, "app.sock"))
BACKEND_PORT = int(os.environ.get("GATEWAY_BACKEND_PORT", "18096"))
BACKEND_HOST = os.environ.get("GATEWAY_BACKEND_HOST", "127.0.0.1")
PID_FILE = os.environ.get("GATEWAY_PID_FILE", "")

_PREFIX = os.environ.get("GATEWAY_PREFIX", "/app/emby")
IO_TIMEOUT = 600

# ── 管理员自动重登（后端重启内存 token 作废 → 网关无感续命）──
# 机制: /admin/api 请求带 token 且后端 401 → 查 stale_map 旧→新映射; 无则用
#   EIO_ADMIN_USER/PASS(默认 admin/admin) 登录拿新 token 重试(≤2发,5s 防抖),
#   并记录映射; 同时捕获 admin UI 手动登录的 token 入册。不带 token 透传保登录页;
#   非 /admin/api(客户端播放)绝不注入 —— TV 链路零影响。
ADMIN_USER = os.environ.get("EIO_ADMIN_USER", "admin")
ADMIN_PASS = os.environ.get("EIO_ADMIN_PASS", "admin")
STATE_FILE = os.environ.get("GATEWAY_STATE_FILE", "")
_tok_lock = threading.Lock()
_admin_tok = {"t": None, "at": 0.0}   # 当前有效 admin token + 获取时间
_known_tokens = set()                  # 曾捕获的 admin token（供参考/调试）
_stale_map = {}                        # 旧token -> 新token（免重复登录）

# ── JS 绝对路径重写（按最长前缀优先排序，避免 /admin/ 误吞 /admin/api/）──
# 注意：只重写 JS 字符串里的绝对路径，不碰相对路径（vendor/, admin.js 等）
_JS_PATTERNS = [
    # /admin/api/ → prefix + /admin/api/
    (re.compile(rb"(['\"`])(/admin/api/[A-Za-z0-9_/?=&.:-]+)"), lambda m: m.group(1) + _PREFIX.encode() + m.group(2)),
    # /Users/ → prefix + /Users/  (Emby 认证 API)
    (re.compile(rb"(['\"`])(/Users/[A-Za-z0-9_/?=&.:-]+)"), lambda m: m.group(1) + _PREFIX.encode() + m.group(2)),
    # /admin/ (非 api 子路径的 admin 引用，如 fetch('/admin/api/logs/download') 已被上面捕获)
    # 只匹配 /admin/ 开头但不含 /api/ 的 — 实际上 logs/download 也是 /admin/api/ 前缀，已覆盖
    # 安全起见：/admin/ 后非 api 的路径
    (re.compile(rb"(['\"`])(/admin/(?!api/)[A-Za-z0-9_/?=&.:-]*)"), lambda m: m.group(1) + _PREFIX.encode() + m.group(2)),
    # /libraries (emby 客户端 API)
    (re.compile(rb"(['\"`])(/libraries\b)"), lambda m: m.group(1) + _PREFIX.encode() + m.group(2)),
    # /reconnect
    (re.compile(rb"(['\"`])(/reconnect\b)"), lambda m: m.group(1) + _PREFIX.encode() + m.group(2)),
]

# ── HTML 绝对路径重写 ──
_HTML_PATTERNS = [
    # href="/admin" → href="/app/emby/admin"
    (re.compile(rb'href="(/admin)"'), lambda m: b'href="' + _PREFIX.encode() + m.group(1) + b'"'),
    # href="/admin/..." → href="/app/emby/admin/..."
    (re.compile(rb'href="(/admin/)"'), lambda m: b'href="' + _PREFIX.encode() + m.group(1) + b'"'),
]


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

def _rewrite_js(body_bytes):
    """重写 JS 响应体里的绝对路径，返回 (modified_bytes, changed)。"""
    modified = False
    for pat, repl in _JS_PATTERNS:
        new, n = pat.subn(repl, body_bytes)
        if n:
            body_bytes = new
            modified = True
    return body_bytes, modified

def _rewrite_html(body_bytes):
    """重写 HTML 响应体里的绝对路径，返回 (modified_bytes, changed)。"""
    modified = False
    for pat, repl in _HTML_PATTERNS:
        new, n = pat.subn(repl, body_bytes)
        if n:
            body_bytes = new
            modified = True
    return body_bytes, modified

# ── 管理员自动重登实现 ──

def _state_load():
    """sidecar 重启后从落盘状态恢复 known tokens（避免把用户打回登录页）。"""
    if not STATE_FILE:
        return
    try:
        with open(STATE_FILE, "r", encoding="utf-8") as f:
            d = json.load(f)
        for t in d.get("known", []):
            _known_tokens.add(t)
        for k, v in (d.get("stale_map") or {}).items():
            _stale_map[k] = v
    except (OSError, ValueError):
        pass

def _state_save():
    if not STATE_FILE:
        return
    try:
        tmp = STATE_FILE + ".tmp"
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump({"known": list(_known_tokens), "stale_map": _stale_map}, f)
        os.replace(tmp, STATE_FILE)
    except OSError:
        pass

def _admin_login():
    """用 EIO_ADMIN_USER/PASS 直连后端登录，返回新 AccessToken 或 None。"""
    try:
        payload = json.dumps({"Username": ADMIN_USER, "Pw": ADMIN_PASS}).encode()
        c = http.client.HTTPConnection(BACKEND_HOST, BACKEND_PORT, timeout=10)
        c.request("POST", "/Users/AuthenticateByName", body=payload,
                  headers={"Content-Type": "application/json",
                           "X-Emby-Client": "EIO-Gateway",
                           "X-Emby-Client-Version": "1.0",
                           "X-Emby-Device-Name": "gateway",
                           "X-Emby-Device-Id": "gateway-sidecar"})
        r = c.getresponse()
        data = r.read()
        c.close()
        if r.status == 200:
            tok = (json.loads(data) or {}).get("AccessToken")
            return tok or None
    except Exception:
        pass
    return None

_login_backoff = [0.0]         # 上次登录失败时间戳（失败退避 30s，防错密码刷爆）

def _admin_login_throttled():
    """带失败退避的登录：距上次失败<30s 直接返回 None。"""
    with _tok_lock:
        if time.time() - _login_backoff[0] < 30:
            return None
    tok = _admin_login()
    with _tok_lock:
        if tok:
            _login_backoff[0] = 0.0
            _admin_tok["t"] = tok
            _admin_tok["at"] = time.time()
            _known_tokens.add(tok)
        else:
            _login_backoff[0] = time.time()
    return tok

def _ensure_live_admin_tok(reject):
    """返回一个我们最新持有的 admin token 用于替换死 token。
    缓存活 token != reject 时先赌它可用；reject 正是缓存(说明后端二次重启它也死了)
    或缓存为空 → 强制重新登录。"""
    with _tok_lock:
        cur = _admin_tok["t"]
    if cur and cur != reject:
        return cur
    return _admin_login_throttled()

def _invalidate_admin_tok(tok):
    with _tok_lock:
        if _admin_tok["t"] == tok:
            _admin_tok["t"] = None
            _admin_tok["at"] = 0.0
            for k in [k for k, v in _stale_map.items() if v == tok]:
                del _stale_map[k]   # 映射指向死 token → 一并清理
            _state_save()

def _extract_token(hdrs):
    """从待转发 header 列表里找 admin UI 的 token（X-Emby-Token 头 或 api 访问）。"""
    for k, v in hdrs:
        if k.lower() == "x-emby-token":
            return v
    return None

def _replace_token_hdr(hdrs, tok):
    out = [(k, v) for (k, v) in hdrs if k.lower() != "x-emby-token"]
    out.append(("X-Emby-Token", tok))
    return out

def _do_forward(method, fwd_path, query, fwd_hdrs, body):
    """向后端发一次请求，返回 (conn, resp)；调用方负责 close。"""
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
    return conn, conn.getresponse()

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

        # 精确 /app/emby → 301 → /app/emby/
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

        # 根路径 / → 302 → /app/emby/admin/（admin 后台是桌面入口）
        if fwd_path == "/":
            body = ("<html><body><a href='%s/admin/'>Emby Admin Panel</a></body></html>"
                    % _PREFIX).encode("utf-8")
            resp = ("HTTP/1.1 302 Found\r\n"
                    + "Location: %s/admin/\r\n" % _PREFIX
                    + "Content-Type: text/html; charset=utf-8\r\n"
                    + "Content-Length: %d\r\n" % len(body)
                    + "Cache-Control: no-cache\r\n"
                    + "Connection: close\r\n\r\n").encode("ascii") + body
            sock.sendall(resp)
            return

        # 转发请求头
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

        is_admin_api = fwd_path.startswith("/admin/api")
        hdr_names = {k.lower() for k, _ in fwd_hdrs}
        # admin UI 登录不带 X-Emby-Client；TV/手机客户端必带 → 以此区分捕获
        capture_login = (method == "POST"
                         and fwd_path.lower().rstrip("/") == "/users/authenticatebyname"
                         and "x-emby-client" not in hdr_names)
        tok = _extract_token(fwd_hdrs)
        # 无 token 的 admin/api 请求不注入（保持登录页流程，安全边界给客户端/游客）

        conn, resp = _do_forward(method, fwd_path, query, fwd_hdrs, body)
        status = resp.status

        # ── admin 401 自动重登重试（只救 /admin/api，客户端从不碰此路径）──
        # 不带 token 的请求不注入（保持登录页流程）；带任意 token 的 admin 请求失败即续命。
        if status == 401 and is_admin_api and tok:
            retried = set()
            cur = tok
            for _attempt in range(2):
                nt = _stale_map.get(cur)
                if not nt:
                    nt = _ensure_live_admin_tok(cur)
                if not nt or nt == cur or nt in retried:
                    break
                conn.close()
                retried.add(nt)
                conn, resp = _do_forward(method, fwd_path, query,
                                         _replace_token_hdr(fwd_hdrs, nt), body)
                status = resp.status
                if status != 401:
                    _stale_map[cur] = nt
                    _state_save()
                    break
                # 映射的/缓存的 token 也失效 → 下一轮强制真登录（reject 机制）
                with _tok_lock:
                    if _admin_tok["t"] == nt:
                        _admin_tok["t"] = None
                cur = nt

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
            # 捕获 admin UI 登录成功 → token 入册，日后重启可无感续命
            if capture_login and status == 200:
                try:
                    nt = (json.loads(body_bytes) or {}).get("AccessToken")
                    if nt:
                        with _tok_lock:
                            _known_tokens.add(nt)
                            _admin_tok["t"] = nt
                            _admin_tok["at"] = time.time()
                        _state_save()
                except (ValueError, AttributeError):
                    pass
            modified = False
            if "javascript" in ctype:
                body_bytes, modified = _rewrite_js(body_bytes)
            elif "html" in ctype:
                body_bytes, modified = _rewrite_html(body_bytes)
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
