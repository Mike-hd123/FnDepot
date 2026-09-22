#!/usr/bin/env python3
"""
云微面板网关代理 — 将飞牛统一网关 Unix Socket 请求转发到面板
纯 TCP 层代理，兼容 HTTP 和 WebSocket
"""
import socket
import os
import sys
import signal
import threading

SOCK_PATH = os.environ.get("TRIM_APPDEST", "/usr/local/apps/@appcenter/wechat-on-cloud") + "/app.sock"
BACKEND_PORT = os.environ.get("PORT", "8080")
PID_FILE = os.environ.get("TRIM_PKGVAR", "/var/apps/wechat-on-cloud/var") + "/proxy.pid"


def proxy(client: socket.socket, backend_host: str, backend_port: int):
    backend = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        backend.connect((backend_host, backend_port))
    except ConnectionRefusedError:
        client.close()
        return

    def forward(src, dst):
        try:
            while True:
                data = src.recv(65536)
                if not data:
                    break
                dst.sendall(data)
        except (BrokenPipeError, ConnectionResetError, OSError):
            pass
        finally:
            try: src.close()
            except: pass
            try: dst.close()
            except: pass

    t1 = threading.Thread(target=forward, args=(client, backend), daemon=True)
    t2 = threading.Thread(target=forward, args=(backend, client), daemon=True)
    t1.start(); t2.start()
    t1.join(); t2.join()


def run():
    if os.path.exists(SOCK_PATH):
        os.unlink(SOCK_PATH)

    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(SOCK_PATH)
    os.chmod(SOCK_PATH, 0o666)
    server.listen(128)

    with open(PID_FILE, "w") as f:
        f.write(str(os.getpid()))

    print(f"云微面板代理: unix:{SOCK_PATH} -> tcp:localhost:{BACKEND_PORT}")

    def handle_sigterm(sig, frame):
        server.close()
        os.unlink(SOCK_PATH)
        sys.exit(0)
    signal.signal(signal.SIGTERM, handle_sigterm)

    while True:
        try:
            client, _ = server.accept()
            threading.Thread(target=proxy, args=(client, "127.0.0.1", int(BACKEND_PORT)), daemon=True).start()
        except OSError:
            break


if __name__ == "__main__":
    run()
