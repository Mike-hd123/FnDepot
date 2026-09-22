#!/usr/bin/env python3
"""用前端新构建的 dist 替换 app.tgz 内的 web-dist/，并同步 manifest。

用法: python3 update-webdist.py <dist目录> <新版本号> <changelog一句话>
- app.tgz 逐条目流式重打：非 web-dist 条目原字节原样透传(含 mtime/uid/mode)，
  老 web-dist 条目丢弃，原位注入新 web-dist 块(mtime 定档今天 00:00 UTC+8)。
- manifest: version / changelog / checksum=md5(新app.tgz) 三处同步。
- 原 app.tgz 备份为 app.tgz.bak_<MMDD_HHMM>。
"""
import gzip, hashlib, io, os, re, sys, tarfile

here = os.path.dirname(os.path.abspath(__file__))
APP = os.path.join(here, "app.tgz")
MAN = os.path.join(here, "manifest")

def main():
    dist, newver, note = sys.argv[1], sys.argv[2], sys.argv[3]
    dist = os.path.abspath(dist)
    assert os.path.isdir(os.path.join(dist, "assets")), f"{dist} 不是 vite dist"

    ts = __import__("time").strftime("%m%d_%H%M")
    bak = APP + f".bak_{ts}"
    os.link(APP, bak)  # 硬链备份，零拷贝
    print(f"备份: {bak}")

    # 收集新 web-dist 条目(名称/路径)，顺序: 目录在前、按文件名排序，与原包一致
    new_entries = []
    mtime = int(__import__("calendar").timegm(__import__("time").strptime(__import__("time").strftime("%Y-%m-%d"), "%Y-%m-%d")) )
    def walk(rel):
        d = os.path.join(dist, rel) if rel else dist
        base = "./web-dist" + (f"/{rel}" if rel else "")
        if rel:
            ti = tarfile.TarInfo(base + "/")
            ti.type = tarfile.DIRTYPE; ti.mode = 0o755; ti.mtime = mtime
            ti.uid = ti.gid = 0; ti.uname = ti.gname = ""
            new_entries.append((ti, None))
        names = sorted(os.listdir(d))
        for n in names:
            p = os.path.join(d, n)
            if os.path.isdir(p):
                walk(f"{rel}/{n}" if rel else n)
            else:
                ti = tarfile.TarInfo(base + "/" + n)
                ti.type = tarfile.REGTYPE; ti.size = os.path.getsize(p)
                ti.mode = 0o755; ti.mtime = mtime; ti.uid = ti.gid = 0
                new_entries.append((ti, p))
    walk("")

    # 流式重打
    out = io.BytesIO()
    with tarfile.open(fileobj=out, mode="w", format=tarfile.GNU_FORMAT) as tfw:
        with tarfile.open(APP, "r:gz") as tfr:
            injected = False
            for m in tfr:
                if m.name.startswith("./web-dist"):
                    if not injected:
                        for ti, p in new_entries:
                            if p is None:
                                tfw.addfile(ti)
                            else:
                                tfw.addfile(ti, open(p, "rb"))
                        injected = True
                    continue
                f = tfr.extractfile(m)
                tfw.addfile(m, f if f is not None and m.isfile() else None)
        if not injected:  # 原包没有 web-dist 段(理论不该发生)则尾部追加
            for ti, p in new_entries:
                tfw.addfile(ti) if p is None else tfw.addfile(ti, open(p, "rb"))

    raw = out.getvalue()
    with gzip.GzipFile(filename="", mode="wb", compresslevel=6, mtime=0,
                       fileobj=open(APP + ".tmp", "wb")) as gz:
        gz.write(raw)
    os.replace(APP + ".tmp", APP)
    md5 = hashlib.md5(open(APP, "rb").read()).hexdigest()
    print(f"新 app.tgz: {os.path.getsize(APP)} bytes, md5={md5}")

    # manifest 同步
    txt = open(MAN, encoding="utf-8").read()
    txt = re.sub(r"(?m)^version\s*=.*$", f"version                    = {newver}", txt)
    txt = re.sub(r"(?m)^changelog\s*=.*$", f"changelog                  = {note}", txt)
    txt = re.sub(r"(?m)^checksum\s*=.*$", f"checksum                    = {md5}", txt)
    open(MAN, "w", encoding="utf-8").write(txt)
    print(f"manifest: version={newver}, checksum={md5}")

if __name__ == "__main__":
    main()
