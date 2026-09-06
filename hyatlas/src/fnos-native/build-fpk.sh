#!/bin/bash
# HyAtlas v4.1.1-2 fpk 可复现构建（B2: 内置 ORT int8 bge-large-zh）
#
# 产物字节级等价于: /vol2/1000/download/hyatlas-4.1.1-2-x86.fpk
#   sha256 = 4a5567cf837879bce94111512db567bf9995a6deef4933840b6900a9b0c8c1e0
#   size   = 241787281
#
# 输入来源：
#   - fpk 控制层（cmd/config/wizard/manifest/ICON）：本目录归档原件
#   - Go 二进制：bin/hyatlas-go-linux-amd64（构建机 /tmp/hyatlas-b2-final2 归档，
#     go1.26.8 编译；Go 链接产物不可字节级复现，权威产物以归档为准）
#   - 模型三件套（~355MB，超 GitHub 100MB 限制不入库）：按 MODEL_SRC → /tmp/b2-models
#     → 从现有 fpk 提取 的顺序解析；sha256 断言防漂移
#
# 用法: bash build-fpk.sh [输出路径]
#
# 与早期一次性脚本(/tmp/build-b2-fpk.sh)的差异（不影响产物字节）：
#   - fnpack build 校验步骤省略（fnpack 重压 app.tgz 会改字节，仅作旁路校验）
#   - manifest checksum 占位符替换发生在 tar 组装之后，对产物无影响，此处保留
#     占位符原件直拼，字节与已发布产物一致
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${1:-/vol2/1000/download/hyatlas-4.1.1-2-x86.fpk}"
REF_FPK="/vol2/1000/download/hyatlas-4.1.1-2-x86.fpk"
EXPECT_SHA="4a5567cf837879bce94111512db567bf9995a6deef4933840b6900a9b0c8c1e0"

SHA_BIN="f6100c564dbdc37e040c487b5beb3efa1ad20594754ba5b89c1384f0b41d2c2d"
SHA_ONNX="8a3f371a7e535e25d3d5a0ff0c0501a605ef0b62577800d2bf4b1fc76d6cbcf1"
SHA_ORT="99458e9d185dfa1a9b5f6510790ede3bedc25dea378adb904ce292b517eeaecf"
SHA_TOK="7dfbf1966ebf99d471c3796e9b457329d2b2182b817e144f1e904b957745c839"

STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT

echo "=== 1/4 app payload ==="
APPROOT="$STAGING/app_new"
mkdir -p "$APPROOT/gateway" "$APPROOT/models" "$APPROOT/ui/images"
cp "$HERE/bin/hyatlas-go-linux-amd64" "$APPROOT/hyatlas-go"
cp "$HERE/gateway/gateway_proxy.py" "$APPROOT/gateway/"
cp "$HERE/ui/config" "$APPROOT/ui/"
cp -a "$HERE/ui/images/." "$APPROOT/ui/images/"

# 模型三件套：MODEL_SRC → /tmp/b2-models → 现有 fpk 提取
resolve_models() {
  local cand
  for cand in "${MODEL_SRC:-}" /tmp/b2-models; do
    if [ -n "$cand" ] && [ -f "$cand/model_int8.onnx" ] && [ -f "$cand/libonnxruntime.so" ] && [ -f "$cand/tokenizer.json" ]; then
      echo "$cand"; return 0
    fi
  done
  return 1
}
MODEL_DIR=""
if MODEL_DIR="$(resolve_models)"; then
  echo "models from: $MODEL_DIR"
  cp "$MODEL_DIR/libonnxruntime.so" "$MODEL_DIR/model_int8.onnx" "$MODEL_DIR/tokenizer.json" "$APPROOT/models/"
else
  echo "models not found on disk, extracting from $REF_FPK (inner app.tgz)"
  [ -f "$REF_FPK" ] || { echo "FATAL: 模型既不在磁盘也不在现有 fpk: $REF_FPK" >&2; exit 1; }
  tar -xf "$REF_FPK" -C "$STAGING" app.tgz
  mkdir -p "$STAGING/mx" "$APPROOT/models"
  tar -xzf "$STAGING/app.tgz" -C "$STAGING/mx" ./models
  cp "$STAGING/mx/models/libonnxruntime.so" "$STAGING/mx/models/model_int8.onnx" "$STAGING/mx/models/tokenizer.json" "$APPROOT/models/"
fi
chmod 755 "$APPROOT/hyatlas-go" "$APPROOT/models/libonnxruntime.so"

# 输入完整性断言
echo "$SHA_BIN  $APPROOT/hyatlas-go"     | sha256sum -c --quiet
echo "$SHA_ONNX  $APPROOT/models/model_int8.onnx" | sha256sum -c --quiet
echo "$SHA_ORT  $APPROOT/models/libonnxruntime.so"| sha256sum -c --quiet
echo "$SHA_TOK  $APPROOT/models/tokenizer.json"   | sha256sum -c --quiet

echo "=== 2/4 deterministic app.tgz (gzip mtime=0, 内层元数据钉死) ==="
cd "$APPROOT"
python3 - << 'PYEOF'
import gzip, os, stat, tarfile
# 元数据钉死自已发布产物 app.tgz（内层 tar，uid/gid=0，tar 路径带 ./ 前缀）
INNER_PINS = {
    './app.tgz':                  {'mode': 0o644, 'mtime': 1788665563},
    './gateway':                  {'mode': 0o755, 'mtime': 1788665562},
    './gateway/gateway_proxy.py': {'mode': 0o644, 'mtime': 1788665562},
    './hyatlas-go':               {'mode': 0o755, 'mtime': 1788665562},
    './models':                   {'mode': 0o755, 'mtime': 1788665563},
    './models/libonnxruntime.so': {'mode': 0o755, 'mtime': 1788665562},
    './models/model_int8.onnx':   {'mode': 0o644, 'mtime': 1788665563},
    './models/tokenizer.json':    {'mode': 0o644, 'mtime': 1788665563},
    './ui':                       {'mode': 0o755, 'mtime': 1788665562},
    './ui/config':                {'mode': 0o755, 'mtime': 1788665562},
    './ui/images':                {'mode': 0o755, 'mtime': 1788665562},
    './ui/images/icon-256.png':   {'mode': 0o755, 'mtime': 1788665562},
    './ui/images/icon-64.png':    {'mode': 0o755, 'mtime': 1788665562},
}
raw = gzip.GzipFile(filename='', mode='wb', compresslevel=6, mtime=0, fileobj=open('app.tgz','wb'))
with tarfile.open(fileobj=raw, mode='w') as tf:
    for name in sorted(INNER_PINS, key=str.lower):
        rel = name[2:]
        st = os.lstat(rel)
        ti = tarfile.TarInfo(name)
        ti.size = st.st_size; ti.mtime = INNER_PINS[name]['mtime']; ti.mode = INNER_PINS[name]['mode']
        ti.uid = 0; ti.gid = 0; ti.uname = ''; ti.gname = ''
        if stat.S_ISDIR(st.st_mode):
            ti.type = tarfile.DIRTYPE; ti.size = 0; tf.addfile(ti)
        else:
            ti.type = tarfile.REGTYPE
            with open(rel,'rb') as f: tf.addfile(ti, f)
raw.close()
print('app.tgz built deterministically')
PYEOF
mv app.tgz "$STAGING/app.tgz"

echo "=== 3/4 deterministic fpk assemble (外层元数据钉死) ==="
cd "$STAGING"
# 控制层：仓库归档原件直取（字节已验证 == fpk 内原件）
cp -a "$HERE/cmd" "$HERE/config" "$HERE/wizard" .
cp -a "$HERE/ICON.PNG" "$HERE/ICON_256.PNG" "$HERE/manifest" .
python3 - "$OUT" << 'PYEOF'
import gzip, os, stat, sys, tarfile
out = sys.argv[1]
# 外层钉死自已发布产物（uid/gid=hermes-studio 运行身份，tar 路径无 ./ 前缀）
OUTER_PINS = {
    'app.tgz':    {'mode': 0o644, 'mtime': 1788665580},
    'cmd':        {'mode': 0o755, 'mtime': 1788649852},
    'cmd/config_callback':    {'mode': 0o755, 'mtime': 1788649852},
    'cmd/config_init':        {'mode': 0o755, 'mtime': 1788649852},
    'cmd/install_callback':   {'mode': 0o755, 'mtime': 1788649852},
    'cmd/install_init':       {'mode': 0o755, 'mtime': 1788649852},
    'cmd/main':               {'mode': 0o755, 'mtime': 1788649852},
    'cmd/uninstall_callback': {'mode': 0o755, 'mtime': 1788649852},
    'cmd/uninstall_init':     {'mode': 0o755, 'mtime': 1788649852},
    'cmd/upgrade_callback':   {'mode': 0o755, 'mtime': 1788649852},
    'cmd/upgrade_init':       {'mode': 0o755, 'mtime': 1788649852},
    'config':                 {'mode': 0o755, 'mtime': 1788649852},
    'config/privilege':       {'mode': 0o644, 'mtime': 1788649852},
    'config/resource':        {'mode': 0o644, 'mtime': 1788649852},
    'ICON.PNG':               {'mode': 0o705, 'mtime': 1788665580},
    'ICON_256.PNG':           {'mode': 0o705, 'mtime': 1788665580},
    'manifest':               {'mode': 0o644, 'mtime': 1788665580},
    'wizard':                 {'mode': 0o755, 'mtime': 1788649852},
    'wizard/install':         {'mode': 0o644, 'mtime': 1788649852},
}
import pwd, grp

class BareDirInfo(tarfile.TarInfo):
    # 原打包脚本同款：目录条目名不带尾斜杠
    def get_info(self):
        info = tarfile.TarInfo.get_info(self)
        if info['type'] == tarfile.DIRTYPE and info['name'].endswith('/'):
            info['name'] = info['name'].rstrip('/')
        return info

def ti_for(path, name, pins):
    st = os.lstat(path)
    ti = BareDirInfo(name)
    ti.size = st.st_size; ti.mtime = pins['mtime']; ti.mode = pins['mode']
    try: ti.uid = st.st_uid; ti.gid = st.st_gid
    except Exception: pass
    try: ti.uname = pwd.getpwuid(st.st_uid).pw_name
    except KeyError: ti.uname = ''
    try: ti.gname = grp.getgrgid(st.st_gid).gr_name
    except KeyError: ti.gname = ''
    if stat.S_ISDIR(st.st_mode): ti.type = tarfile.DIRTYPE; ti.size = 0
    elif stat.S_ISLNK(st.st_mode): ti.type = tarfile.SYMTYPE; ti.linkname = os.readlink(path)
    else: ti.type = tarfile.REGTYPE
    return ti

raw = gzip.GzipFile(filename='', mode='wb', compresslevel=6, mtime=0, fileobj=open(out+'.tmp','wb'))
with tarfile.open(fileobj=raw, mode='w') as tf:
    for e in ['app.tgz'] + sorted(['cmd', 'config', 'ICON.PNG', 'ICON_256.PNG', 'manifest', 'wizard'], key=str.lower):
        pins = OUTER_PINS[e]
        if os.path.isdir(e) and not os.path.islink(e):
            tf.addfile(ti_for(e, e, pins))
            for child in sorted(os.listdir(e)):
                cp = os.path.join(e, child)
                of = open(cp,'rb') if os.path.isfile(cp) else None
                tf.addfile(ti_for(cp, f'{e}/{child}', OUTER_PINS[f'{e}/{child}']), of)
                if of: of.close()
        else:
            of = open(e,'rb')
            tf.addfile(ti_for(e, e, pins), of)
            of.close()
raw.close()
os.replace(out + '.tmp', out)
print('fpk assembled')
PYEOF

echo "=== 4/4 verify ==="
GOT_SHA=$(sha256sum "$OUT" | cut -d' ' -f1)
GOT_SIZE=$(stat -c '%s' "$OUT")
if [ "$GOT_SHA" = "$EXPECT_SHA" ]; then
  echo "PASS: sha256 == $EXPECT_SHA"
else
  echo "FAIL: sha256 = $GOT_SHA (expected $EXPECT_SHA)" >&2
  exit 1
fi
echo "$OUT  $((GOT_SIZE/1024/1024))MB  sha256=$GOT_SHA"
