#!/bin/bash
# build.sh — fnpack build for emby-in-one
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE"

APP_NAME="emby-in-one"
VERSION="1.4.4-5"
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
FPK="$(ls ${APP_NAME}*.fpk 2>/dev/null | head -1)"
[ -n "$FPK" ] || { echo "FATAL: fnpack 未产出 ${APP_NAME}*.fpk" >&2; exit 1; }
cp "$FPK" "$OUT"

# ---- 4. 自检 ----
echo "=== 自检 ==="
echo "产物: $OUT  $(($(stat -c '%s' "$OUT") / 1024 / 1024))MB  sha256=$(sha256sum "$OUT" | cut -d' ' -f1)"
tar tzf "$OUT" >/dev/null && echo "✅ fpk 是合法 gzip+tar"
tar xzOf "$OUT" app.tgz 2>/dev/null | tar tzf - 2>/dev/null | grep -q '^ui/config' && echo "✅ 桌面入口 ui/config 在 app.tgz 内层" || echo "❌ 缺 ui/config（入口打不开）"
tar xzOf "$OUT" app.tgz 2>/dev/null | tar tzf - 2>/dev/null | grep -q '^gateway/emby-gateway.py' && echo "✅ gateway sidecar 在 app.tgz 内层" || echo "❌ 缺 gateway/emby-gateway.py"
# 验证 ui/config 是 socket 模式（无 port 字段，有 gatewaySocket）
tar xzOf "$OUT" app.tgz 2>/dev/null | tar xzf - -O ui/config 2>/dev/null | grep -q '"gatewaySocket"' && echo "✅ ui/config socket 模式" || echo "❌ ui/config 缺 gatewaySocket"
tar -xOf "$OUT" manifest | grep -E "^(version|appname|maintainer|distributor)" || true

echo ""
echo "完成。产物在 $OUT —— 铁律：不自动安装，装哪先问用户。"
