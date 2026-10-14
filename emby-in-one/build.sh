#!/bin/bash
# build.sh — fnpack build for emby-in-one
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE"

APP_NAME="emby-in-one"
VERSION="1.4.4-9"
ARCH="x86"
OUT="${1:-/vol2/1000/download/${APP_NAME}-${VERSION}-${ARCH}.fpk}"

# ---- 0. 前置检查 ----
for f in cmd/main cmd/install_init cmd/install_callback cmd/upgrade_init cmd/upgrade_callback \
         cmd/uninstall_init cmd/uninstall_callback cmd/config_init cmd/config_callback \
         config/privilege config/resource manifest wizard/install wizard/uninstall; do
  [ -e "$f" ] || { echo "FATAL: 缺必需文件 $f" >&2; exit 1; }
done
for f in ICON.PNG ICON_256.PNG ui/images/icon-64.png ui/images/icon-256.png; do
  [ -e "$f" ] || { echo "FATAL: 缺图标 $f" >&2; exit 1; }
done
command -v fnpack >/dev/null || { echo "FATAL: fnpack 不在 PATH" >&2; exit 1; }

# ⚠️ .bak 闸门：fnpack 会把源目录整个打进 app.tgz，备份文件混进包里 = 装到设备上多一堆垃圾
#    （实测 1.4.4-6 就带着 3 个 *.bak_20261014_sharefix 上了机）
BAK="$(find . -name '*.bak*' -o -name '*_~' -o -name '*.orig' 2>/dev/null | head -20)"
if [ -n "$BAK" ]; then
  echo "FATAL: 包内含备份文件，先移出再打包：" >&2
  echo "$BAK" >&2
  exit 1
fi

# ⚠️ 二进制闸门：app/ 里没主程序 = 装了个几十 KB 的空壳，应用直接瘫（实测踩过 38KB fpk）
[ -f "app/${APP_NAME}" ] || { echo "FATAL: app/${APP_NAME} 不存在。编译或从 /vol2/1000/workspace/emby-in-one/repo/${APP_NAME} 拷进来再打包" >&2; exit 1; }
BIN_SIZE="$(stat -c '%s' "app/${APP_NAME}")"
[ "$BIN_SIZE" -gt 1000000 ] || { echo "FATAL: app/${APP_NAME} 只有 ${BIN_SIZE} 字节，不像真二进制" >&2; exit 1; }
echo "binary ok: app/${APP_NAME} $((BIN_SIZE / 1024 / 1024))MB"

# ---- 1. app/ 内容组装 ----
mkdir -p app/ui/images
cp ui/config app/ui/
[ -f ui/images/icon-64.png ] && cp ui/images/icon-64.png app/ui/images/
[ -f ui/images/icon-256.png ] && cp ui/images/icon-256.png app/ui/images/

# gateway sidecar 必须在 app/ 内层
[ -f app/gateway/emby-gateway.py ] || { echo "FATAL: 缺 app/gateway/emby-gateway.py" >&2; exit 1; }

# ---- 2. manifest 版本占位符替换（已是固定值，sed 安全无操作）----
sed -i "s/<上游版本>-<本地版本号>/$VERSION/" manifest 2>/dev/null || true

# ---- 3. fnpack 打包 ----
echo "=== fnpack build ==="
fnpack build -d "$HERE"
FPK="$(ls -t ${APP_NAME}*.fpk 2>/dev/null | head -1)"
[ -n "$FPK" ] || { echo "FATAL: fnpack 未产出 ${APP_NAME}*.fpk" >&2; exit 1; }
cp "$FPK" "$OUT"

# ---- 4. 自检 ----
echo "=== 自检 ==="
echo "产物: $OUT  $(($(stat -c '%s' "$OUT") / 1024 / 1024))MB  sha256=$(sha256sum "$OUT" | cut -d ' ' -f1)"
# ⚠️ set -o pipefail 下别用 `tar | grep -q`：grep -q 命中就退管道，tar 收 SIGPIPE(141) → 整条被判失败（误报"缺文件"）。
#    先把列表/内容一次性取进变量，再对变量做判断。
tar tzf "$OUT" >/dev/null && echo "✅ fpk 是合法 gzip+tar"
INNER_LIST="$(tar xzOf "$OUT" app.tgz 2>/dev/null | tar tzf - 2>/dev/null || true)"
MANIFEST_IN="$(tar -xOf "$OUT" manifest 2>/dev/null || true)"
printf '%s\n' "$INNER_LIST" | grep -q '^ui/config' && echo "✅ 桌面入口 ui/config 在 app.tgz 内层" || echo "❌ 缺 ui/config（入口打不开）"
printf '%s\n' "$INNER_LIST" | grep -q '^gateway/emby-gateway.py' && echo "✅ gateway sidecar 在 app.tgz 内层" || echo "❌ 缺 gateway/emby-gateway.py"
# 包内必须有主程序（空包 = 装了必瘫，实测踩过）
if printf '%s\n' "$INNER_LIST" | grep -qx "${APP_NAME}"; then
  echo "✅ app.tgz 内含主程序 ${APP_NAME}"
else
  echo "❌ app.tgz 缺主程序 ${APP_NAME}（空包，装了必瘫）"
fi
# 包内不得有备份文件
if printf '%s\n' "$INNER_LIST" | grep -qE '\.bak|~$|\.orig$'; then
  echo "❌ app.tgz 内含备份文件"
else
  echo "✅ app.tgz 无备份文件污染"
fi
# 生命周期脚本必须在 fpk 外层 cmd/ 里，且 share 解析写法正确
printf '%s\n' "$(tar tzf "$OUT")" | grep -qx 'cmd/main' && echo "✅ cmd/main 在包内" || echo "❌ 缺 cmd/main"
tar -xOf "$OUT" cmd/main 2>/dev/null | grep -q 'resolve_share_root' \
  && echo "✅ cmd/main 用 share 软链优先解析（不是 /vol0 占位路径）" \
  || echo "❌ cmd/main 缺 resolve_share_root，share 路径可能落错"
# 验证 ui/config 是 socket 模式（无 port 字段，有 gatewaySocket）
tar xzOf "$OUT" app.tgz 2>/dev/null | tar xzf - -O ui/config 2>/dev/null | grep -q '"gatewaySocket"' && echo "✅ ui/config socket 模式" || echo "❌ ui/config 缺 gatewaySocket"
printf '%s\n' "$MANIFEST_IN" | grep -E "^(version|appname|maintainer|distributor)" || true
# 包内版本必须与 build.sh 的 VERSION 一致（防改了 manifest 忘了这里，或反之）
printf '%s\n' "$MANIFEST_IN" | grep -Eq "^version[[:space:]]+=[[:space:]]+${VERSION}[[:space:]]*$" \
  && echo "✅ 包内版本 = ${VERSION}" || echo "❌ 包内版本与 ${VERSION} 不符"

echo ""
echo "完成。产物在 $OUT —— 铁律：不自动安装，装哪先问用户。"
