#!/bin/bash
# 从 panel 源码全量构建 app.tgz（替代"定档 app.tgz + sync-apptgz.py 逐条目替换"链路）
#
# 产物: fnos-native/app.new.tgz —— 不覆盖定档 app.tgz，保留回归路径。
#       build.sh --from-source 检测到 app.new.tgz 时用它拼 fpk。
#
# 组成:
# - config/ ui/       ← fnos-native/app-static/（fnos-native 特有、panel 源码没有的静态件）
# - server/           ← panel/server/ 源码 + package-lock + proxy.py + tsconfig
#                       + node_modules（npm ci --omit=dev；tsx 为 dependency，esbuild 随其进入）
# - web-dist/         ← panel/web `npm run build` 产物 dist/
#
# 确定性约定（对齐 sync-apptgz.py）:
# - 条目名 ./xxx 前缀；同级按不区分大小写字典序，目录头在其子项之前（深度优先）
# - uid/gid=0，uname/gname 空，mtime 统一归一，mode 目录/文件均 0755（软链 0777 保留 link）
# - GNU_FORMAT，gzip mtime=0，compresslevel=6
# - npm ci 失败即中止，禁止降级为 npm install 自由解析
set -euo pipefail

NATIVE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PANEL_DIR="$(cd "$NATIVE_DIR/../panel" && pwd)"

# --- 0. 工具链 ---
if ! command -v node >/dev/null 2>&1; then
  # fnOS 宿主机 PATH 无 node，用 nodejs_v24 应用带的（现役 WOC 面板同款运行时）
  export PATH="/vol1/@appcenter/nodejs_v24/bin:$PATH"
fi
command -v node >/dev/null 2>&1 || { echo "ERROR: 找不到 node（PATH 与 /vol1/@appcenter/nodejs_v24/bin 均无）" >&2; exit 1; }
echo "node: $(node -v)  npm: $(npm -v)  registry: $(npm config get registry)"

# --- 0b. registry 连通性: npm ping 不可靠（元数据通但 tarball 302 到 CDN 会挂），
#     用真实小 tarball 探测；不通则走本机代理 7890（仍不通即中止，禁止降级 npm install）
PROBE_URL="$(npm config get registry)bcryptjs/-/bcryptjs-2.4.3.tgz"
if ! timeout 20 curl -sL -o /dev/null "$PROBE_URL"; then
  echo "registry tarball 直连失败，回退 https_proxy=http://127.0.0.1:7890"
  export https_proxy=http://127.0.0.1:7890 http_proxy=http://127.0.0.1:7890 NODE_USE_ENV_PROXY=1
  # ~/.npmrc 有 https-proxy=false，env 变量会被其屏蔽 → 用 npm_config_* 强制覆盖
  export npm_config_https_proxy=http://127.0.0.1:7890 npm_config_proxy=http://127.0.0.1:7890
  timeout 20 npm ping >/dev/null 2>&1 || { echo "ERROR: registry 直连与代理均不通，中止（禁止降级 npm install）" >&2; exit 1; }
fi

# --- 1. server 依赖（仅 prod；tsx/esbuild 为 dependency 会装）---
# 注: 必须显式写 --include=dev/--omit=dev，用户级 ~/.npmrc 有 omit=dev 会隐式裁剪 devDeps
echo "==> npm ci (server, --omit=dev)"
( cd "$PANEL_DIR/server" && npm ci --omit=dev --no-audit --no-fund )

# --- 2. web 依赖（devDeps 含 vite，必须 --include=dev）+ 构建 ---
echo "==> npm ci (web, --include=dev)"
( cd "$PANEL_DIR/web" && npm ci --include=dev --no-audit --no-fund )
echo "==> npm run build (web)"
( cd "$PANEL_DIR/web" && npm run build )

# --- 3. 组装 app.new.tgz ---
echo "==> 组装 app.new.tgz"
OUT="$NATIVE_DIR/app.new.tgz"
python3 - "$NATIVE_DIR" "$PANEL_DIR" "$OUT" <<'PYEOF'
import io, gzip, os, stat, sys, tarfile, time, calendar

native, panel, out = sys.argv[1], sys.argv[2], sys.argv[3]
MTIME = int(calendar.timegm(time.strptime("2026-09-01", "%Y-%m-%d")))

SKIP_SUFFIX = (".bak_", ".pyc")
SKIP_NAMES = {"__pycache__", ".DS_Store"}

def skipped(name):
    return name.endswith(SKIP_SUFFIX) or name in SKIP_NAMES

def norm(ti, name, mode=0o755):
    ti.name = name
    ti.mtime = MTIME
    ti.uid = ti.gid = 0
    ti.uname = ti.gname = ""
    ti.mode = mode

def emit_tree(tf, base, prefix):
    """深度优先：目录头在前，同级按不区分大小写字典序；软链保留"""
    def walk(rel, arc):
        d = os.path.join(base, rel) if rel else base
        for n in sorted(os.listdir(d), key=str.lower):
            if skipped(n):
                continue
            p = os.path.join(d, n)
            child = f"{arc}/{n}"
            if os.path.islink(p):
                ti = tarfile.TarInfo(child); norm(ti, child, 0o777)
                ti.type = tarfile.SYMTYPE; ti.linkname = os.readlink(p)
                tf.addfile(ti)
            elif os.path.isdir(p):
                ti = tarfile.TarInfo(child); norm(ti, child)
                ti.type = tarfile.DIRTYPE
                tf.addfile(ti)
                walk(os.path.join(rel, n) if rel else n, child)
            else:
                ti = tarfile.TarInfo(child); norm(ti, child)
                ti.type = tarfile.REGTYPE
                ti.size = os.path.getsize(p)
                with open(p, "rb") as fh:
                    tf.addfile(ti, fh)
    ti = tarfile.TarInfo(prefix); norm(ti, prefix)
    ti.type = tarfile.DIRTYPE
    tf.addfile(ti)
    walk("", prefix)

srcs = {
    "./config":    os.path.join(native, "app-static", "config"),
    "./server":    os.path.join(panel, "server"),
    "./ui":        os.path.join(native, "app-static", "ui"),
    "./web-dist":  os.path.join(panel, "web", "dist"),
}
for k, v in srcs.items():
    assert os.path.isdir(v), f"缺少源目录: {k} -> {v}"
assert os.path.isdir(os.path.join(panel, "server", "node_modules")), \
    "server/node_modules 缺失，npm ci 未生效？"

buf = io.BytesIO()
with tarfile.open(fileobj=buf, mode="w", format=tarfile.GNU_FORMAT) as tf:
    ti = tarfile.TarInfo("./"); norm(ti, "./"); ti.type = tarfile.DIRTYPE
    tf.addfile(ti)
    for k in sorted(srcs, key=str.lower):
        emit_tree(tf, srcs[k], k)

with gzip.GzipFile(filename="", mode="wb", compresslevel=6, mtime=0,
                   fileobj=open(out + ".tmp", "wb")) as gz:
    gz.write(buf.getvalue())
os.replace(out + ".tmp", out)
print(f"app.new.tgz: {os.path.getsize(out)} bytes, md5={__import__('hashlib').md5(open(out,'rb').read()).hexdigest()}")
PYEOF

echo "done: $OUT"
