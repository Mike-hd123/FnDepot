#!/bin/bash
# HyAtlas v4.5.0-1 fpk 构建（同步上游 v4.4.0 + v4.5.0：BM25+向量 hybrid 搜索、owner 分组 consolidation、compact_raw/dedupe_facts 维护端点、B2 内置 ORT int8 bge-large-zh）
#
# 输入来源：
#   - fpk 控制层（cmd/config/wizard/manifest/ICON）：本目录归档原件
#   - Go 二进制：bin/hyatlas-go-linux-amd64（本分支源码 go1.26.5 本地编译，
#     CGO_ENABLED=1 -trimpath GOAMD64=v1，非上游 CI 产物；含 dashboard go:embed 内嵌前端，
#     故 dashboard 有改动必须重编后重新打包装入 SHA_BIN，只改 manifest 不算交付）
#   - 模型三件套（~355MB，超 GitHub 100MB 限制不入库）：按 MODEL_SRC → /tmp/b2-models
#     → 从 REF_FPK 内层 app.tgz 提取的顺序解析；sha256 断言防漂移。
#     REF_FPK 原指 4.2.5-1，该 fpk 已从 download 目录消失（兜底路径断裂），
#     现改指现存的 4.3.3-1 —— 两者内层模型 sha 实测逐件相同，不引入漂移。
#
# 用法: bash build-fpk.sh [输出路径]
# ⚠ OUT 默认值是 /vol02/1000-1-13b246aa/… = fuse.rclone WebDAV 挂载（见 mount），
#   脚本要写 out+'.tmp' 再 os.replace，走该挂载有大文件重命名风险；
#   打包请显式传本地路径：bash build-fpk.sh /vol2/1000/download/hyatlas-4.5.0-1-x86.fpk
#
# 与 4.1.1-2 时代的字节级等价构建差异：
#   - 4.1.1-5 控制层与二进制均有变更，不再做 REF 产物 sha 等价断言；
#     末尾改为结构校验（manifest 版本、shares 自愈回调在位、app payload 完整）
#   - fnpack build 校验步骤省略（fnpack 重压 app.tgz 会改字节）
#   - manifest checksum 占位符替换发生在 tar 组装之后，对产物无影响，此处保留
#     占位符原件直拼
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${1:-/vol02/1000-1-13b246aa/download/hyatlas-4.5.0-1-x86.fpk}"
REF_FPK="/vol2/1000/download/hyatlas-4.3.3-1-x86.fpk"   # 241,916,249B sha ce362d5d… 实测存在；其内层模型三件套 sha 与 SHA_ONNX/SHA_ORT/SHA_TOK 逐件相同

# SHA_BIN must be recomputed against the freshly built bin/hyatlas-go-linux-amd64 for 4.5.0-1 (fork-src @ hyatlas-v450-sync).
# Do NOT ship fpk with the placeholder below; run sha256sum on the binary and replace.
SHA_BIN="TBD-4.5.0-1-recompute-before-packing"
SHA_ONNX="8a3f371a7e535e25d3d5a0ff0c0501a605ef0b62577800d2bf4b1fc76d6cbcf1"
SHA_ORT="99458e9d185dfa1a9b5f6510790ede3bedc25dea378adb904ce292b517eeaecf"
SHA_TOK="7dfbf1966ebf99d471c3796e9b457329d2b2182b817e144f1e904b957745c839"

# ── SHA_BIN 自检闸（防"只改 manifest 不动二进制"）────────────────────────────
# sha256sum -c 只能证明"bin/ 里的文件 == SHA_BIN 写的值"，两者一起写旧值照样全绿。
# 所以在组装前硬性拒绝：仍是占位符 / 不是 64 位十六进制 / 等于历史已出货二进制的 sha。
case "$SHA_BIN" in
  TBD*|*recompute*|*[!0-9a-fA-F]*)
    echo "FATAL: SHA_BIN 非法（占位符或不是 64 位十六进制 sha256）: '$SHA_BIN'" >&2
    echo "       对 bin/hyatlas-go-linux-amd64 跑 sha256sum 后填入实际值" >&2; exit 1;;
esac
[ "${#SHA_BIN}" = 64 ] || { echo "FATAL: SHA_BIN 长度 ${#SHA_BIN} != 64" >&2; exit 1; }
[ -f "$HERE/bin/hyatlas-go-linux-amd64" ] || { echo "FATAL: 缺 $HERE/bin/hyatlas-go-linux-amd64" >&2; exit 1; }
ACTUAL_BIN_SHA="$(sha256sum "$HERE/bin/hyatlas-go-linux-amd64" | cut -d' ' -f1)"
[ "$ACTUAL_BIN_SHA" = "$SHA_BIN" ] || { echo "FATAL: SHA_BIN($SHA_BIN) != bin/ 实际 sha($ACTUAL_BIN_SHA)" >&2; exit 1; }
case "$ACTUAL_BIN_SHA" in
  01bec1798c50456672b2a2980aa931b994d10e55f6c97caebea60b3ce73abdce)
    echo "FATAL: bin/ 仍是 4.3.3-1 出货二进制（01bec179），不能以 4.5.0-1 名义打包" >&2; exit 1;;
  83740365a1235a2dc0086be35c1d7667dd7eb7817c0ff09d1b229e3df18d2b4d)
    echo "FATAL: bin/ 仍是 4.3.0-1 出货二进制（83740365），不能以 4.5.0-1 名义打包" >&2; exit 1;;
  f9acdf9f4917df1f73a609ce975d4651684a6d0ccdd533f071a7ee66a8c47c2d)
    echo "WARN: bin/ 是 dashboard 汉化修复前的 f9acdf9f —— 若 i18n 补全已合入则本值应已变化" >&2;;
esac
echo "SHA_BIN 自检通过: $ACTUAL_BIN_SHA ($(stat -c '%s' "$HERE/bin/hyatlas-go-linux-amd64") B)"

STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT

echo "=== 1/4 app payload ==="
APPROOT="$STAGING/app_new"
mkdir -p "$APPROOT/gateway" "$APPROOT/models" "$APPROOT/ui/images"
cp "$HERE/bin/hyatlas-go-linux-amd64" "$APPROOT/hyatlas-go"
cp "$HERE/gateway/gateway_proxy.py" "$APPROOT/gateway/"
cp "$HERE/config/env.example" "$APPROOT/env.example"
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
    './gateway':                  {'mode': 0o755, 'mtime': 1788777600},
    './gateway/gateway_proxy.py': {'mode': 0o644, 'mtime': 1788777600},
    './hyatlas-go':               {'mode': 0o755, 'mtime': 1788777600},
    './models':                   {'mode': 0o755, 'mtime': 1788665563},
    './models/libonnxruntime.so': {'mode': 0o755, 'mtime': 1788777600},
    './models/model_int8.onnx':   {'mode': 0o644, 'mtime': 1788665563},
    './models/tokenizer.json':    {'mode': 0o644, 'mtime': 1788665563},
    './ui':                       {'mode': 0o755, 'mtime': 1788777600},
    './ui/config':                {'mode': 0o755, 'mtime': 1788777600},
    './ui/images':                {'mode': 0o755, 'mtime': 1788777600},
    './ui/images/icon-256.png':   {'mode': 0o755, 'mtime': 1788777600},
    './ui/images/icon-64.png':    {'mode': 0o755, 'mtime': 1788777600},
    './env.example':              {'mode': 0o644, 'mtime': 1788777600},
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
    'app.tgz':    {'mode': 0o644, 'mtime': 1788777601},
    'cmd':        {'mode': 0o755, 'mtime': 1788777602},
    'cmd/config_callback':    {'mode': 0o755, 'mtime': 1788777602},
    'cmd/config_init':        {'mode': 0o755, 'mtime': 1788777602},
    'cmd/install_callback':   {'mode': 0o755, 'mtime': 1788777602},
    'cmd/install_init':       {'mode': 0o755, 'mtime': 1788777602},
    'cmd/main':               {'mode': 0o755, 'mtime': 1788777602},
    'cmd/uninstall_callback': {'mode': 0o755, 'mtime': 1788777602},
    'cmd/uninstall_init':     {'mode': 0o755, 'mtime': 1788777602},
    'cmd/upgrade_callback':   {'mode': 0o755, 'mtime': 1788777602},
    'cmd/upgrade_init':       {'mode': 0o755, 'mtime': 1788777602},
    'config':                 {'mode': 0o755, 'mtime': 1788777602},
    'config/env.example':     {'mode': 0o644, 'mtime': 1788777602},
    'config/privilege':       {'mode': 0o644, 'mtime': 1788777602},
    'config/resource':        {'mode': 0o644, 'mtime': 1788777602},
    'ICON.PNG':               {'mode': 0o705, 'mtime': 1788777601},
    'ICON_256.PNG':           {'mode': 0o705, 'mtime': 1788777601},
    'manifest':               {'mode': 0o644, 'mtime': 1788777601},
    'wizard':                 {'mode': 0o755, 'mtime': 1788777602},
    'wizard/install':         {'mode': 0o644, 'mtime': 1788777602},
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

echo "=== 4/4 verify (structural) ==="
GOT_SHA=$(sha256sum "$OUT" | cut -d' ' -f1)
GOT_SIZE=$(stat -c '%s' "$OUT")
FAIL=0

# 1) gzip+tar 可解
if ! tar -tzf "$OUT" >/dev/null 2>&1; then
  echo "FAIL: fpk is not a valid gzip+tar" >&2; FAIL=1
fi

# 2) manifest 版本/appname
GOT_VER=$(tar -xOf "$OUT" manifest | awk -F'= *' '$1 ~ /^version/ {print $2}' | tr -d ' ')
GOT_APP=$(tar -xOf "$OUT" manifest | awk -F'= *' '$1 ~ /^appname/ {print $2}' | tr -d ' ')
if [ "$GOT_VER" != "4.5.0-1" ] || [ "$GOT_APP" != "hyatlas" ]; then
  echo "FAIL: manifest appname=$GOT_APP version=$GOT_VER (expected hyatlas 4.5.0-1)" >&2; FAIL=1
fi

# 3) shares 自愈回调在位（install/upgrade 双回调都含特征串）
for f in install_callback upgrade_callback; do
  if ! tar -xOf "$OUT" "cmd/$f" 2>/dev/null | grep -q 'shares is NOT a symlink'; then
    echo "FAIL: cmd/$f missing shares-heal logic" >&2; FAIL=1
  fi
done

# 4) app.tgz payload 完整（二进制 + 模型三件套 + 网关 + ui/config）
tar -xf "$OUT" -O app.tgz > "$STAGING/inner.tgz"
for p in './hyatlas-go' './models/model_int8.onnx' './models/libonnxruntime.so' './models/tokenizer.json' './gateway/gateway_proxy.py' './ui/config' './env.example'; do
  if ! tar -tzf "$STAGING/inner.tgz" "$p" >/dev/null 2>&1; then
    echo "FAIL: app.tgz missing $p" >&2; FAIL=1
  fi
done

# 5) 内层二进制 = 本次构建的新二进制（防旧包混入）
INNER_BIN_SHA=$(tar -xzf "$STAGING/inner.tgz" -O ./hyatlas-go | sha256sum | cut -d' ' -f1)
if [ "$INNER_BIN_SHA" != "$SHA_BIN" ]; then
  echo "FAIL: inner hyatlas-go sha mismatch ($INNER_BIN_SHA != $SHA_BIN)" >&2; FAIL=1
fi

if [ "$FAIL" != "0" ]; then
  echo "STRUCTURE CHECK FAILED" >&2
  exit 1
fi
echo "PASS: structure ok (manifest 4.5.0-1, shares-heal callbacks present, app payload complete incl env.example)"
echo "$OUT  $((GOT_SIZE/1024/1024))MB  sha256=$GOT_SHA"
