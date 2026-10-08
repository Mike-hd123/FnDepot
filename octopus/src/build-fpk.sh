#!/bin/bash
# Octopus fnOS 安装包构建（官方 fnpack build 流程，遵循 octopus/README.md「src/fnos/ 重组装」）
#
# 既定配方（README 第 41-47 行 / 技能 native-app-package-pitfalls 第 61-62 行）：
#   解包上一版 fpk 作骨架 → 换 app/octopus 官方新二进制 + 本地 gateway sidecar
#   → 删 app/config（fnpack 会自动把顶层 config/ 注入 app.tgz，留着会重复两份）
#   → 更新 manifest（version/desc/changelog，删 checksum 行由 fnpack 重算）
#   → fnpack build -d .   （输出固定名 octopus.fpk，需手动 cp 命名）
#
# ⚠️ 不要用 tar 手工组装：INNER_PINS 需逐项列举，列漏即丢文件。
#   2026-10-08 教训：手写脚本漏 ui/images/（商店无图标）、漏 wizard/install、
#   漏 LICENSE/README.md/THIRD_PARTY_LICENSES.csv，且 config 只有一份与既定包不一致。
#
# 用法: bash build-fpk.sh [输出路径] [骨架 fpk]
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${1:-/vol2/1000/download/octopus-0.13.10-1-x86.fpk}"
SKEL="${2:-}"
STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT

VERSION=$(awk -F'= *' '/^version/ {gsub(/[ \t\r\n]/,"",$2); print $2}' "$HERE/fnos/manifest")
BIN_SHA=$(sha256sum "$HERE/octopus-linux-amd64" | cut -d' ' -f1)

if [ -z "$SKEL" ]; then
  echo "需要骨架 fpk（上一版已验证包）。用法: bash build-fpk.sh <输出路径> <骨架fpk>" >&2
  exit 1
fi

echo "=== 1/5 解骨架（$SKEL） ==="
cd "$STAGING"
tar -xzf "$SKEL"
mkdir -p app
tar -xzf app.tgz -C app
rm -f app.tgz
# fnpack 会把顶层 config/ 注入 app.tgz；app/ 内自带的必须删掉，否则重复两份
rm -rf app/config

echo "=== 2/5 换二进制 + sidecar ==="
cp "$HERE/octopus-linux-amd64" app/octopus
cp "$HERE/fnos/gateway/gateway_proxy.py" app/gateway/gateway_proxy.py
chmod 755 app/octopus
sha256sum -c <<< "$BIN_SHA  app/octopus"
./app/octopus version | grep -E 'Version|Commit|Built'

echo "=== 3/5 同步 manifest（version/desc/changelog，删 checksum） ==="
cp "$HERE/fnos/manifest" manifest
sed -i -E '/^checksum[[:space:]]*=/d' manifest
grep -E '^version' manifest
test "$(grep -cE '^checksum' manifest)" -eq 0

echo "=== 4/5 fnpack build（必须显式成功；stdout/stderr 不吞，防失败沿用旧包）==="
rm -f octopus.fpk
LOG="$STAGING/fnpack.log"
fnpack build -d . >"$LOG" 2>&1 || { cat "$LOG" >&2; echo "fnpack build FAILED" >&2; exit 1; }
grep -q 'Packing successfully' "$LOG" || { cat "$LOG" >&2; echo "fnpack 未成功" >&2; exit 1; }
[ -f octopus.fpk ] || { cat "$LOG" >&2; echo "FAIL: octopus.fpk 未生成" >&2; exit 1; }
tail -3 "$LOG"

echo "=== 5/5 结构自检（对齐既定包 0.13.9-1） ==="
# 注意：本脚本开了 pipefail。`tar | grep -q` 会在 grep 命中后立即退出，
# 使 tar 收到 SIGPIPE 返回 141，整条管道被判失败 → 假报「缺文件」。
# 必须先把清单一次性读进变量，再对变量做匹配。
OUTER_LIST=$(tar -tzf octopus.fpk)
tar -xOf octopus.fpk app.tgz > inner.tgz
INNER_LIST=$(tar -tzf inner.tgz)
has_outer() { grep -qxE -- "$1" <<<"$OUTER_LIST"; }
has_inner() { grep -qxF -- "$1" <<<"$INNER_LIST"; }

FAIL=0
for e in 'app\.tgz' 'cmd' 'config' 'manifest' 'ICON\.PNG' 'ICON_256\.PNG' 'wizard/install'; do
  has_outer "$e" || { echo "FAIL: 外层缺 $e" >&2; FAIL=1; }
done
for f in octopus gateway/gateway_proxy.py ui/config ui/images/icon-64.png ui/images/icon-256.png \
         LICENSE README.md THIRD_PARTY_LICENSES.csv config/privilege config/resource; do
  has_inner "$f" || { echo "FAIL: app.tgz 缺 $f" >&2; FAIL=1; }
done
CONF_C=$(grep -cxE 'config' <<<"$INNER_LIST")
[ "$CONF_C" -eq 1 ] || { echo "FAIL: app.tgz 内 config 应为 1 份，实际 $CONF_C" >&2; FAIL=1; }
tar -xzf inner.tgz -O octopus > bin.tmp
[ "$(sha256sum bin.tmp | cut -d' ' -f1)" = "$BIN_SHA" ] || { echo "FAIL: 内层二进制 sha 不符" >&2; FAIL=1; }
[ "$FAIL" = 0 ] || { echo "STRUCTURE CHECK FAILED" >&2; exit 1; }

mkdir -p "$(dirname "$OUT")"
cp octopus.fpk "$OUT"
echo "PASS: $VERSION 结构对齐"
echo "$OUT  $(stat -c '%s' "$OUT") B  sha256=$(sha256sum "$OUT" | cut -d' ' -f1)"
