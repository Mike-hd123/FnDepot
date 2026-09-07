#!/bin/bash
# build.sh — fnpack 标准构建（模板）
#
# 用法: 把应用二进制/前端放进 app/ 后，执行:
#   ./build.sh [OUT]     默认 OUT=/vol2/1000/download/<appname>-<version>-<arch>.fpk
#
# fnpack build -d <dir> 规则:
#   - <dir>/app/ 下全部内容（含 app/ui/）自动打成 app.tgz（安装解压到 APPDEST）
#   - 校验 cmd/ 9 脚本齐全（缺任一直接报 Required file missing）
#   - checksum 自动计算，产物输出到调用时 cwd
#   - 只保留 app/ 下内容；根目录游离 ui/ 会被 fnpack 当非法 JSON 报错/丢弃 → 桌面入口失效

set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE"

APP_NAME="<appname>"
VERSION="<上游版本>-<本地版本号>"
ARCH="x86"
OUT="${1:-/vol2/1000/download/${APP_NAME}-${VERSION}-${ARCH}.fpk}"

# ---- 0. 前置检查 ----
for f in cmd/main cmd/install_init cmd/install_callback cmd/upgrade_init cmd/upgrade_callback \
         cmd/uninstall_init cmd/uninstall_callback cmd/config_init cmd/config_callback \
         config/privilege config/resource manifest wizard/install; do
  [ -e "$f" ] || { echo "FATAL: 缺必需文件 $f" >&2; exit 1; }
done
# 图标可选但强烈建议（缺了商店/桌面图标不显示）：
for f in ICON.PNG ICON_256.PNG ui/images/icon-64.png ui/images/icon-256.png; do
  [ -e "$f" ] || echo "⚠️ 提示: 无 $f —— 从 FnDepot/<app>/src/fnos-native 拷真实图标（见 ui/images/说明-从FnDepot拷贝.md）"
done
command -v fnpack >/dev/null || { echo "FATAL: fnpack 不在 PATH" >&2; exit 1; }

# ---- 1. app/ 内容组装（改这节）----
mkdir -p app/ui/images
# 示例：拷二进制 + SPA 前端 + 入口 config + 桌面图标
# cp bin/<binary> app/
# cp -a web-dist/. app/
cp ui/config app/ui/                      # 桌入口 .url iframe 配置
[ -f ui/images/icon-64.png ] && cp ui/images/icon-64.png app/ui/images/
[ -f ui/images/icon-256.png ] && cp ui/images/icon-256.png app/ui/images/
# sidecar socket 反代（如需 /app/<app> 网关路径）
# cp sidecar/gateway_proxy.py app/gateway/

# ---- 2. manifest 版本占位符替换 ----
sed "s/<上游版本>-<本地版本号>/$VERSION/" manifest > .manifest.tmp && mv .manifest.tmp manifest

# ---- 3. fnpack 打包（产物输出到 cwd）----
echo "=== fnpack build ==="
fnpack build -d "$HERE"   # 产物落在调用时 cwd（= $HERE）
FPK="$(ls ${APP_NAME}-*.fpk 2>/dev/null | head -1)"
[ -n "$FPK" ] || { echo "FATAL: fnpack 未产出 ${APP_NAME}-*.fpk" >&2; exit 1; }
cp "$FPK" "$OUT"

# ---- 4. 自检 ----
echo "=== 自检 ==="
echo "产物: $OUT  $(($(stat -c '%s' "$OUT") / 1024 / 1024))MB  sha256=$(sha256sum "$OUT" | cut -d' ' -f1)"
tar tzf "$OUT" >/dev/null && echo "✅ fpk 是合法 gzip+tar"
tar tzf "$OUT" | grep -q '^ui' && echo "✅ 桌面入口 ui/ 在位" || echo "❌ 缺 ui/（入口打不开）"
tar -xOf "$OUT" manifest | grep -E "^(version|appname|maintainer|distributor)" || true

echo ""
echo "完成。产物在 $OUT —— 铁律：不自动安装，装哪先问用户。"