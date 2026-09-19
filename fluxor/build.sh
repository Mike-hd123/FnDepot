#!/bin/bash
# build.sh — Fluxor fpk 构建（fork 自 fnpkg-app-template/build.sh，内容组装已在本目录就位）
# 用法: ./build.sh [OUT]  默认 OUT=/vol2/1000/download/fluxor-1.4.0-4-x86.fpk
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE"

APP_NAME="fluxor"
VERSION="1.4.0-6"
ARCH="x86"
OUT="${1:-/vol2/1000/download/${APP_NAME}-${VERSION}-${ARCH}.fpk}"

# ---- 0. 前置检查 ----
for f in cmd/main cmd/install_init cmd/install_callback cmd/upgrade_init cmd/upgrade_callback \
         cmd/uninstall_init cmd/uninstall_callback cmd/config_init cmd/config_callback \
         config/privilege config/resource manifest wizard/install \
         app/fluxor app/mihomo app/Model.bin app/ASN.mmdb app/geosite.dat app/geoip.metadb; do
  [ -e "$f" ] || { echo "FATAL: 缺必需文件 $f" >&2; exit 1; }
done
command -v fnpack >/dev/null || { echo "FATAL: fnpack 不在 PATH" >&2; exit 1; }

# 二进制新鲜度检查：app/fluxor 必须不旧于源码构建产物
SRC=/vol2/1000/workspace/fluxor-work/fluxor/fluxor
if [ -f "$SRC" ] && [ "$SRC" -nt app/fluxor ]; then
  echo "⚠️ 源码二进制比 app/fluxor 新，请重新拷贝后再打包（或忽略此警告继续）" >&2
fi

# ---- 1. app/ 内容组装 ----
mkdir -p app/ui/images
cp ui/config app/ui/                          # 桌入口 .url iframe 配置（fnpack 要求 app.tgz 内含 ui/）
[ -f ui/images/icon-64.png ] && cp ui/images/icon-64.png app/ui/images/
[ -f ui/images/icon-256.png ] && cp ui/images/icon-256.png app/ui/images/

# ---- 2. manifest 版本占位确认 ----
grep -q "^version                    = ${VERSION}" manifest || {
  echo "FATAL: manifest version != ${VERSION}" >&2; exit 1; }

# ---- 2. fnpack 打包 ----
echo "=== fnpack build ==="
fnpack build -d "$HERE"
FPK=""
for c in "${APP_NAME}.fpk" ${APP_NAME}-*.fpk; do
  [ -f "$c" ] && FPK="$c" && break
done
[ -n "$FPK" ] || { echo "FATAL: fnpack 未产出 ${APP_NAME}*.fpk" >&2; exit 1; }
mkdir -p "$(dirname "$OUT")"
cp "$FPK" "$OUT"

# ---- 3. 自检 ----
echo "=== 自检 ==="
echo "产物: $OUT  $(($(stat -c '%s' "$OUT") / 1024 / 1024))MB  sha256=$(sha256sum "$OUT" | cut -d' ' -f1)"
tar tzf "$OUT" >/dev/null && echo "✅ fpk 是合法 gzip+tar"
# app.tgz 在包内是嵌套归档，flat grep 不到 ui/config、fluxor 等，
# 必须先抽出 app.tgz 再查内层清单
INNER="$(tar xzOf "$OUT" app.tgz 2>/dev/null | tar tzf - 2>/dev/null)"
[ -n "$INNER" ] || { echo "❌ 无法读出 app.tgz 内层清单"; }
echo "$INNER" | grep -q '^ui/config$'   && echo "✅ 桌面入口 ui/config 在位（app.tgz 内层）" || echo "❌ 缺 ui/config（入口打不开）"
echo "$INNER" | grep -q '^fluxor$'      && echo "✅ fluxor 在位" || echo "❌ 缺 fluxor"
echo "$INNER" | grep -q '^mihomo$'      && echo "✅ smart 内核在位" || echo "⚠️  缺 mihomo（会回落外部内核）"
echo "$INNER" | grep -q '^Model.bin$'   && echo "✅ LightGBM 模型在位" || echo "⚠️  缺 Model.bin"
echo "$INNER" | grep -q '^ASN.mmdb$'    && echo "✅ Smart 模块 ASN.mmdb 在位（1.4.0-4 修复）" || echo "❌ 缺 ASN.mmdb（Smart 内核会 fatal 崩）"
echo "$INNER" | grep -q '^geosite.dat$' && echo "✅ GeoSite.dat 在位" || echo "⚠️  缺 geosite.dat"
echo "$INNER" | grep -q '^geoip.metadb$' && echo "✅ GeoIP.metadb 在位" || echo "⚠️  缺 geoip.metadb"
tar -xOf "$OUT" manifest | grep -E "^(version|appname|service_port)" || true

echo ""
echo "完成。产物在 $OUT —— 铁律：不自动安装，装哪先问用户。"
