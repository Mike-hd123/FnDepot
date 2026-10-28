#!/bin/bash
# build-fpk.sh — Vikunja fnOS fpk 构建（下载上游官方 release + fnpack 打包）
#
# 参考结构：emby-in-one/build.sh + octopus/src/build-fpk.sh 的骨架拼装套路
# 上游：https://github.com/go-vikunja/vikunja  版本：v2.7.1
# 产物：vikunja-<VERSION>-x86.fpk
#
# 用法：bash build-fpk.sh [VERSION] [输出路径]
#   VERSION 默认 2.7.1-1（对应上游 v2.7.1；跨上游版本修订号归 1）
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
VERSION="${1:-2.7.1-1}"
OUT="${2:-/vol2/1000/download/vikunja-${VERSION}-x86.fpk}"
ARCH="x86"
UPSTREAM="v2.7.1"   # 上游 tag（与本地 VERSION 解耦，方便测试）

# sha256 断言（v2.7.1 起上游 zip 自带 .sha256 做强校验，此处不再硬编期望值）
EXPECTED_ELF_SHA=""
EXPECTED_ZIP_SHA=""

STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT

echo "=========================================="
echo "  Vikunja fnOS fpk 构建"
echo "  Version: ${VERSION}  Upstream: ${UPSTREAM}  Arch: ${ARCH}"
echo "=========================================="

# ── 1. 下载上游官方 release（优先本地缓存；zip 内自带 .sha256，不硬编断言防断链）──
ZIP="${STAGING}/vikunja-${UPSTREAM}-linux-amd64-full.zip"
SHA_REMOTE="${STAGING}/vikunja-${UPSTREAM}-linux-amd64-full.zip.sha256"
URL="https://github.com/go-vikunja/vikunja/releases/download/${UPSTREAM}/vikunja-${UPSTREAM}-linux-amd64-full.zip"
CACHE="${HERE}/vikunja-${UPSTREAM}-linux-amd64-full.zip"

if [ -f "$CACHE" ]; then
  echo "[1/7] 使用本地缓存: $CACHE"
  cp "$CACHE" "$ZIP"
else
  echo "[1/7] 下载上游 ${UPSTREAM} linux-amd64-full ..."
  # 网络对大文件不稳（HTTP/2 半途断流实测），断点续传循环直到拿全
  for _try in 1 2 3 4 5 6 7 8; do
    curl --http1.1 -fsSL -C - -m 240 -o "$ZIP" "$URL" && break
    _rc=$?
    echo "  下载中断 rc=$_rc，重试 $_try/8 ..."
    sleep 2
  done
  curl --http1.1 -fsSL -C - -m 240 -o "$ZIP" "$URL" || { echo "FATAL: 下载失败"; exit 1; }
  cp "$ZIP" "$CACHE"   # 留缓存，重打包不再拉
fi
ls -lh "$ZIP"
echo "  zip sha256: $(sha256sum "$ZIP" | cut -d' ' -f1)"
unzip -q "$ZIP" -d "${STAGING}/extracted-check" -x 'vikunja-v*linux-amd64' 2>/dev/null || true
unzip -tq "$ZIP" >/dev/null || { echo "FATAL: zip 完整性校验失败"; exit 1; }
rm -rf "${STAGING}/extracted-check"

# ── 2. 解包 zip，提取 ELF ──
echo "[2/7] 解包 zip ..."
unzip -q "$ZIP" -d "${STAGING}/extracted"
# zip 内文件名可能是 vikunja 或 vikunja-v<ver>-linux-amd64，取实际存在的 ELF
ELF=$(find "${STAGING}/extracted" -maxdepth 2 -type f \( -name 'vikunja' -o -name 'vikunja-v*-linux-amd64' \) | head -1)
[ -n "$ELF" ] && [ -f "$ELF" ] || { echo "FATAL: 解包后未找到 vikunja ELF"; find "${STAGING}/extracted" -maxdepth 3 -type f | head; exit 1; }
echo "  ELF: $ELF"
chmod +x "$ELF"
# zip 里可能有同目录 .sha256 文件，用它校验
SHA_LOCAL="${STAGING}/extracted/$(basename "$ELF").sha256"
if [ -f "$SHA_LOCAL" ]; then
  ( cd "${STAGING}/extracted" && sha256sum -c "$(basename "$SHA_LOCAL")" ) && echo "  ELF sha256 校验通过"
else
  echo "  WARN: zip 内未提供 .sha256"
fi
ACTUAL_ELF_SHA=$(sha256sum "$ELF" | cut -d' ' -f1)
echo "  ELF sha256: $ACTUAL_ELF_SHA"
if [ -n "${EXPECTED_ELF_SHA:-}" ] && [ "$ACTUAL_ELF_SHA" != "$EXPECTED_ELF_SHA" ]; then
  echo "  WARN: ELF sha 与期望值不同（可能上游重发过；如需严格断言，更新 EXPECTED_ELF_SHA）"
fi
file "$ELF"

# ── 3. 组装 fnpack 工作区 ──
echo "[3/7] 组装 fnpack 工作区 ..."
BUILD_DIR="${STAGING}/project"
mkdir -p "$BUILD_DIR"/{cmd,config,gateway,ui,ui/images,wizard,app/gateway,app/ui,app/ui/images,app/vikunja-embedded}

# 从 FnDepot/vikunja/ 复制模板（ICON + manifest + src/ 下的 cmd/config/gateway/ui/wizard）
cp "$HERE/ICON.PNG" "$BUILD_DIR/"
cp "$HERE/ICON_256.PNG" "$BUILD_DIR/"
cp "$HERE/manifest" "$BUILD_DIR/"

# cmd/main
cp "$HERE/src/cmd_main" "$BUILD_DIR/cmd/main"
# install / upgrade / uninstall —— init 与 callback 全搬
cp "$HERE/src/install_init" "$BUILD_DIR/cmd/install_init"
cp "$HERE/src/install_callback" "$BUILD_DIR/cmd/install_callback"
cp "$HERE/src/upgrade_init" "$BUILD_DIR/cmd/upgrade_init"
cp "$HERE/src/upgrade_callback" "$BUILD_DIR/cmd/upgrade_callback"
cp "$HERE/src/uninstall_callback" "$BUILD_DIR/cmd/uninstall_callback"
cp "$HERE/src/uninstall_init" "$BUILD_DIR/cmd/uninstall_init"
cp "$HERE/src/config_init" "$BUILD_DIR/cmd/config_init"
cp "$HERE/src/config_callback" "$BUILD_DIR/cmd/config_callback"
chmod +x "$BUILD_DIR"/cmd/*

# config（如果有）
[ -d "$HERE/config" ] && cp -r "$HERE/config/"* "$BUILD_DIR/config/" 2>/dev/null || true

# wizard
[ -d "$HERE/wizard" ] && cp -r "$HERE/wizard/"* "$BUILD_DIR/wizard/" 2>/dev/null || true

# gateway sidecar —— install_callback 引用 ${APP_DIR}/gateway/gateway_proxy.py
mkdir -p "$BUILD_DIR/gateway"
cp "$HERE/src/gateway_proxy.py" "$BUILD_DIR/gateway/gateway_proxy.py"
chmod +x "$BUILD_DIR/gateway/gateway_proxy.py"

# ui/config + ui/images —— 塞到 app/ui 里（fnpack 打包 app.tgz 时要求）
cp "$HERE/src/ui-config.json" "$BUILD_DIR/app/ui/config"
# 顶层 ui/config 也给一份（fnpack 外层可能校验）
cp "$HERE/src/ui-config.json" "$BUILD_DIR/ui/config"
# 图标（顶层 ui/images + app/ui/images 双份）
cp "$HERE/ICON.PNG" "$BUILD_DIR/ui/images/icon-64.png"
cp "$HERE/ICON_256.PNG" "$BUILD_DIR/ui/images/icon-256.png"
cp "$HERE/ICON.PNG" "$BUILD_DIR/app/ui/images/icon-64.png"
cp "$HERE/ICON_256.PNG" "$BUILD_DIR/app/ui/images/icon-256.png"
mkdir -p "$BUILD_DIR/app/ui/images"
cp "$HERE/ICON.PNG" "$BUILD_DIR/app/ui/images/icon-64.png"
cp "$HERE/ICON_256.PNG" "$BUILD_DIR/app/ui/images/icon-256.png"

# app/ui 空目录 —— fnpack 打包时 tar 会忽略空目录，放个 .keep 保结构
touch "$BUILD_DIR/app/ui/.keep"

# 二进制放到 app/vikunja（install_callback 里 APP_DIR=/volN/@appcenter/vikunja，fnpack 打包后 APPDEST=vikunja 目录 = fpk 内 app/ 内容）
mkdir -p "$BUILD_DIR/app"
cp "$ELF" "$BUILD_DIR/app/vikunja"
chmod +x "$BUILD_DIR/app/vikunja"

# gateway sidecar 也要在 app/ 内（install_callback: ${APP_DIR}/gateway/gateway_proxy.py）
mkdir -p "$BUILD_DIR/app/gateway"
cp "$BUILD_DIR/gateway/gateway_proxy.py" "$BUILD_DIR/app/gateway/gateway_proxy.py"

# 清理外层不需要的 gateway 目录（fnpack 会把 gateway/ 也作为顶层条目打进 fpk 外层，避免重复）
rm -rf "$BUILD_DIR/gateway"

echo "[3/7] 工作区结构："
find "$BUILD_DIR" -maxdepth 3 -type f | sed "s#${BUILD_DIR}/##" | sort | head -30

# ⚠️ .bak 闸门（与 emby-in-one/build.sh 同款）：${BUILD_DIR} 是即将 fnpack build 的源目录。
#    本脚本对 src/ 是「按具体文件名」逐个 cp（cmd_main→cmd/main 等），不会把 src/*.bak_* 拷进来；
#    但 config/ wizard/ 用 `cp -r "$HERE/config/"*`，若那两处混入备份会被带进包。命中即 FATAL 退出。
BAK="$(find "$BUILD_DIR" -name '*.bak*' -o -name '*_~' -o -name '*.orig' 2>/dev/null | head -20)"
if [ -n "$BAK" ]; then
  echo "FATAL: 包内含备份文件，先移出再打包：" >&2
  echo "$BAK" >&2
  exit 1
fi

# ── 4. fnpack build ──
echo "[4/7] fnpack build ..."
cd "$BUILD_DIR"
fnpack build -d "$BUILD_DIR" > /tmp/vikunja-fnpack.log 2>&1 || { cat /tmp/vikunja-fnpack.log >&2; echo "FATAL: fnpack build 失败" >&2; exit 1; }
grep -q 'Packing successfully' /tmp/vikunja-fnpack.log || { cat /tmp/vikunja-fnpack.log >&2; exit 1; }

FPK_NAME="$(ls vikunja*.fpk 2>/dev/null | head -1)"
[ -n "$FPK_NAME" ] || { echo "FATAL: fnpack 未产出 fpk"; exit 1; }
mv "$FPK_NAME" "$OUT"

# ── 5. 自检 ──
echo "[5/7] 自检 ..."
OUT_SIZE=$(stat -c '%s' "$OUT")
OUT_SHA=$(sha256sum "$OUT" | cut -d' ' -f1)
echo "  产物: $OUT  $((OUT_SIZE / 1024 / 1024))MB  sha256=$OUT_SHA"

# 结构校验
tar tzf "$OUT" >/dev/null && echo "✅ fpk 合法"
OUTER_LIST=$(tar tzf "$OUT")
INNER_LIST=$(tar xzOf "$OUT" app.tgz 2>/dev/null | tar tzf - 2>/dev/null)
grep -qxE 'manifest' <<<"$OUTER_LIST" && echo "✅ 外层 manifest" || echo "❌ 外层缺 manifest"
grep -qxE 'cmd/main' <<<"$OUTER_LIST" && echo "✅ 外层 cmd/main" || echo "❌ 外层缺 cmd/main"
grep -qxF 'vikunja' <<<"$INNER_LIST" && echo "✅ app.tgz 内 vikunja ELF" || echo "❌ app.tgz 缺 vikunja"
grep -qxF 'gateway/gateway_proxy.py' <<<"$INNER_LIST" && echo "✅ app.tgz 内 gateway/gateway_proxy.py" || echo "❌ app.tgz 缺 gateway_proxy.py"
grep -qxF 'ui/config' <<<"$INNER_LIST" && echo "✅ app.tgz 内 ui/config" || echo "❌ app.tgz 缺 ui/config"
grep -qE '^(appname|version) *=' <(tar -xOf "$OUT" manifest) | head -2

echo ""
echo "完成。产物: $OUT"
echo "sha256: $OUT_SHA"
