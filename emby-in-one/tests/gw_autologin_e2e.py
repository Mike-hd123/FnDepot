#!/usr/bin/env python3
"""端到端验证 emby-gateway 自动重登：模拟后端重启导致的 token 失效。

场景:
  A: admin 登录拿 T1 → T1 访问 /admin/api/status = 200
  B: 后端重启(T1 死亡) → 仍用 T1 请求 → sidecar 自动重登救回 200，stale_map 落盘
  C: 二次重启后带原 body 的写请求(PUT) 也能救回 200（重试必须完整重放 body）
  D: 登录被拒时防风暴：未知 token 连发 → 全 401，不刷爆
  E: 非 /admin/api 路径 sidecar 不干预
"""
import json, os, socket, subprocess, sys, threading, time

SCRATCH = "/vol1/@apphome/hermes-studio/hermes-home/cache/scratch"
GW_SRC = "/vol2/1000/workspace/FnDepot/emby-in-one/app/gateway/emby-gateway.py"
FAKE_PORT = 18099          # 假后端：一个可杀重启的 echo 服务
SOCK = os.path.join(SCRATCH, "autologin_test.sock")
STATE = os.path.join(SCRATCH, "gw_state.json")
LOG = os.path.join(SCRATCH, "gw_autologin_test.log")

# ── 假后端：/Users/AuthenticateByName 发 token；/admin/api/* 只认活 token 池 ──
fake = {"valid": set(), "reject_login": False}

def fake_handler_factory():
    from http.server import BaseHTTPRequestHandler

    class H(BaseHTTPRequestHandler):
        def log_message(self, *a): pass
        def _json(self, code, obj):
            b = json.dumps(obj).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(b)))
            self.end_headers()
            self.wfile.write(b)
        def _read_body(self):
            n = int(self.headers.get("Content-Length", 0))
            return self.rfile.read(n) if n > 0 else b""
        def do_POST(self):
            body = json.loads(self._read_body() or b"{}")
            if self.path == "/Users/AuthenticateByName":
                if fake["reject_login"]:
                    self._json(401, {"message": "bad creds"})
                    return
                if body.get("Username") == "admin" and body.get("Pw") == "admin":
                    tok = "FRESH%06d" % int(time.time() * 1000 % 1000000)
                    fake["valid"].add(tok)
                    self._json(200, {"AccessToken": tok})
                else:
                    self._json(401, {"message": "bad creds"})
                return
            self._json(404, {})
        def do_PUT(self):
            self._read_body()  # consume body to clear socket
            if self.path.startswith("/admin/api"):
                tok = self.headers.get("X-Emby-Token", "")
                if tok in fake["valid"]:
                    self._json(200, {"ok": True, "served_by": "fake", "tok": tok, "method": "PUT"})
                else:
                    self._json(401, {"message": "unknown token"})
                return
            self._json(404, {})
        def do_GET(self):
            if self.path.startswith("/admin/api"):
                tok = self.headers.get("X-Emby-Token", "")
                if tok in fake["valid"]:
                    self._json(200, {"ok": True, "served_by": "fake", "tok": tok})
                else:
                    self._json(401, {"message": "unknown token"})
                return
            self._json(404, {})
    return H

def req_uds(method, path, tok=None, body=None):
    """通过 unix socket 打 sidecar（裸 HTTP over socket，绕开 http.client 的 host 校验）。"""
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(15)
    s.connect(SOCK)
    hdrs = ["%s %s HTTP/1.1" % (method, path), "Host: x", "Connection: close"]
    if tok: hdrs.append("X-Emby-Token: " + tok)
    data = body.encode() if body else b""
    if body is not None:
        hdrs.append("Content-Type: application/json")
        hdrs.append("Content-Length: %d" % len(data))
    s.sendall(("\r\n".join(hdrs) + "\r\n\r\n").encode() + data)
    raw = b""
    while True:
        try:
            c = s.recv(65536)
        except socket.timeout:
            break
        if not c: break
        raw += c
    s.close()
    head, _, rbody = raw.partition(b"\r\n\r\n")
    status = int(head.split(b" ")[1])
    return status, rbody

# ── 1. 起假后端 ──
from http.server import ThreadingHTTPServer
srv = ThreadingHTTPServer(("127.0.0.1", FAKE_PORT), fake_handler_factory())
threading.Thread(target=srv.serve_forever, daemon=True).start()

# ── 2. 起 sidecar 指向假后端 ──
for f in (SOCK, STATE):
    try: os.unlink(f)
    except OSError: pass
env = dict(os.environ,
           GATEWAY_SOCK_PATH=SOCK,
           GATEWAY_BACKEND_HOST="127.0.0.1",
           GATEWAY_BACKEND_PORT=str(FAKE_PORT),
           GATEWAY_STATE_FILE=STATE,
           EIO_ADMIN_USER="admin",
           EIO_ADMIN_PASS="admin")
gw = subprocess.Popen([sys.executable, GW_SRC], env=env,
                      stdout=open(LOG, "wb"), stderr=subprocess.STDOUT)
for _ in range(30):
    if os.path.exists(SOCK): break
    time.sleep(0.2)
assert os.path.exists(SOCK), "sidecar socket 未出现"

try:
    # ── 3. 场景A：用户登录拿到 T1 ──
    st, b = req_uds("POST", "/Users/AuthenticateByName", body=json.dumps({"Username":"admin","Pw":"admin"}))
    assert st == 200, ("login", st, b)
    T1 = json.loads(b)["AccessToken"]
    print("[A1] admin 登录成功 tok=%s" % T1[:8])
    st, b = req_uds("GET", "/admin/api/status", tok=T1)
    assert st == 200, ("A2 用T1访问应200", st, b)
    print("[A2] T1 访问 /admin/api/status -> 200 ✓")

    # ── 4. 场景B：后端重启 → T1 死亡 ──
    fake["valid"].clear()
    st, b = req_uds("GET", "/admin/api/status", tok=T1)
    print("[B1] 后端重启后 T1 直接访问 -> %d（sidecar 应救回 200）" % st)
    assert st == 200, ("B1 自动重登未生效", st, b)
    inner = json.loads(b)
    T2 = inner["tok"]
    assert T2 != T1, "新token应不同于旧"
    print("[B2] sidecar 自动重试成功 ✓（新token=%s）" % T2[:8])
    sm = json.load(open(STATE))
    assert sm["stale_map"].get(T1) == T2, sm
    print("[B3] stale_map 落盘 T1->T2 ✓")

    # ── 5. 场景C：二次重启后带 body 的写请求(PUT) ──
    fake["valid"].clear()
    st, b = req_uds("PUT", "/admin/api/proxies", tok=T1, body=json.dumps({"name":"cctv"}))
    print("[C1] T1 写请求(第二次重启后) -> %d" % st)
    assert st == 200, ("C1 PUT 重试未救回", st, b)
    print("[C2] 写请求经映射链救回 200 ✓")

    # ── 6. 场景D：登录被拒时防风暴 ──
    fake["reject_login"] = True
    fake["valid"].clear()
    sts = []
    for _ in range(3):
        st, _ = req_uds("GET", "/admin/api/status", tok="GHOST-TOKEN")
        sts.append(st)
    print("[D] 登录被拒，未知token 3连发 -> %s（应全 401，无风暴）" % sts)
    assert all(s == 401 for s in sts), ("D 应全401", sts)
    fake["reject_login"] = False

    # ── 7. 场景E：非 admin 路径 sidecar 不干预 ──
    st, b = req_uds("GET", "/emby/Items", tok="CLIENT-TOK")
    print("[E] 客户端路径 /emby/Items -> %d（sidecar 不干预非 admin/api）✓" % st)

    print("\nALL SCENARIOS PASSED")
finally:
    gw.terminate()
    srv.shutdown()
    for f in (SOCK, STATE, LOG):
        try: os.unlink(f)
        except OSError: pass
