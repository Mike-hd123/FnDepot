#!/usr/bin/env python3
"""把 src/panel 的最新源码同步进 fnos-native/app.tgz（定档逐条目重打）。

替换条目：
- ./server/src/*.ts        ← panel/server/src/（全量，含新增文件如 trim-api.ts）
- ./web-dist/**            ← panel/web/dist/（委托 update-webdist.py 同款逻辑）

其余条目（node_modules/config/ui/server 非 src 文件）原字节透传。
manifest 同步 version/changelog/checksum=md5(新app.tgz)。原 app.tgz 硬链备份。

用法: python3 sync-apptgz.py <新版本号> <changelog一句话>
"""
import hashlib, io, os, re, sys, tarfile, time, calendar

here = os.path.dirname(os.path.abspath(__file__))
APP = os.path.join(here, "app.tgz")
MAN = os.path.join(here, "manifest")
SRC_PANEL = os.path.abspath(os.path.join(here, "..", "panel"))
SERVER_SRC = os.path.join(SRC_PANEL, "server", "src")
WEB_DIST = os.path.join(SRC_PANEL, "web", "dist")

MTIME = int(calendar.timegm(time.strptime(time.strftime("%Y-%m-%d"), "%Y-%m-%d")))

def make_entries(prefix, base, recursive=True):
    """生成 (TarInfo, path|None) 列表：目录在前按名排序，同 update-webdist 风格"""
    out = []
    def walk(rel):
        d = os.path.join(base, rel) if rel else base
        name = f"./{prefix}" + (f"/{rel}" if rel else "")
        if rel:
            ti = tarfile.TarInfo(name); ti.type = tarfile.DIRTYPE
            ti.mode = 0o755; ti.mtime = MTIME; ti.uid = ti.gid = 0
            ti.uname = ti.gname = ""
            out.append((ti, None))
        for n in sorted(os.listdir(d)):
            p = os.path.join(d, n)
            if os.path.isdir(p) and recursive:
                walk(f"{rel}/{n}" if rel else n)
            elif not os.path.isdir(p):
                ti = tarfile.TarInfo(f"{name}/{n}" if rel or True else n)
                ti.type = tarfile.REGTYPE; ti.size = os.path.getsize(p)
                ti.mode = 0o755; ti.mtime = MTIME; ti.uid = ti.gid = 0
                ti.uname = ti.gname = ""
                out.append((ti, p))
    walk("")
    return out

def main():
    newver, note = sys.argv[1], sys.argv[2]
    assert os.path.isdir(SERVER_SRC) and os.path.isdir(WEB_DIST)

    ts = time.strftime("%m%d_%H%M")
    bak = APP + f".bak_{ts}"
    os.link(APP, bak)
    print(f"备份: {bak}")

    # 源文件映射：name -> 内容文件
    server_new = {}   # "./server/src/<f>" -> path （含目录条目 "./server/src/"）
    for root, dirs, files in os.walk(SERVER_SRC):
        rel = os.path.relpath(root, SERVER_SRC)
        if rel == ".":
            server_new["./server/src/"] = None
            for f in sorted(files):
                server_new[f"./server/src/{f}"] = os.path.join(root, f)
    web_new = make_entries("web-dist", WEB_DIST)
    web_map = {ti.name: p for ti, p in web_new}

    out = io.BytesIO()
    with tarfile.open(fileobj=out, mode="w", format=tarfile.GNU_FORMAT) as tfw:
        with tarfile.open(APP, "r:gz") as tfr:
            emitted_web = False
            for m in tfr:
                if m.name.startswith("./web-dist"):
                    if not emitted_web:
                        for ti, p in web_new:
                            tfw.addfile(ti) if p is None else tfw.addfile(ti, open(p, "rb"))
                        emitted_web = True
                    continue
                if m.name.startswith("./server/src/") and not m.isdir():
                    srcp = server_new.pop(m.name, "")
                    if srcp:  # 同名源文件存在 → 用最新源码
                        with open(srcp, "rb") as fh:
                            ti = tarfile.TarInfo(m.name); ti.type = tarfile.REGTYPE
                            ti.size = os.path.getsize(srcp); ti.mode = 0o755
                            ti.mtime = MTIME; ti.uid = ti.gid = 0; ti.uname = ti.gname = ""
                            tfw.addfile(ti, fh)
                    else:
                        print(f"  删除包内多余: {m.name}")
                    continue
                f = tfr.extractfile(m)
                tfw.addfile(m, f if f is not None and m.isfile() else None)
            # server/src 源目录新增文件（包内没有的，如 trim-api.ts）→ 追加
            for name, p in sorted(server_new.items()):
                if p is None: continue
                ti = tarfile.TarInfo(name); ti.type = tarfile.REGTYPE
                ti.size = os.path.getsize(p); ti.mode = 0o755; ti.mtime = MTIME
                ti.uid = ti.gid = 0; ti.uname = ti.gname = ""
                tfw.addfile(ti, open(p, "rb"))
                print(f"  新增包内: {name}")
            if not emitted_web:
                for ti, p in web_new:
                    tfw.addfile(ti) if p is None else tfw.addfile(ti, open(p, "rb"))

    raw = out.getvalue()
    with gzip_open(APP + ".tmp") as gz:
        gz.write(raw)
    os.replace(APP + ".tmp", APP)
    md5 = hashlib.md5(open(APP, "rb").read()).hexdigest()
    print(f"新 app.tgz: {os.path.getsize(APP)} bytes, md5={md5}")

    txt = open(MAN, encoding="utf-8").read()
    txt = re.sub(r"(?m)^version\s*=.*$", f"version                    = {newver}", txt)
    txt = re.sub(r"(?m)^changelog\s*=.*$", f"changelog                  = {note}", txt)
    txt = re.sub(r"(?m)^checksum\s*=.*$", f"checksum                    = {md5}", txt)
    open(MAN, "w", encoding="utf-8").write(txt)
    print(f"manifest: version={newver}, checksum={md5}")

class gzip_open:
    def __init__(self, path): self.path = path
    def __enter__(self):
        import gzip
        self.g = gzip.GzipFile(filename="", mode="wb", compresslevel=6, mtime=0,
                               fileobj=open(self.path, "wb"))
        return self.g
    def __exit__(self, *a): self.g.close()

if __name__ == "__main__":
    main()
