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

# ⚠️ 闸门 1 —— 备份文件污染：fnpack 把 app/ 与 cmd/ 整个塞进包，`.bak` 会跟着上机
#    （实测装机后 /var/apps/<app>/cmd/ 里躺着 3 个 *.bak_20261014_*）。备份请放到 <app>/ 之外。
BAK="$(find . -name '*.bak*' -o -name '*.orig' -o -name '*~' 2>/dev/null | head -20)"
if [ -n "$BAK" ]; then
  echo "FATAL: 目录里有备份文件，先移到仓库外再打包：" >&2
  echo "$BAK" >&2
  exit 1
fi

# ⚠️ 闸门 2 —— 主程序必须在 app/ 里：漏编译 = 几十 KB 空包，装上直接瘫（实测踩过 38KB fpk）
#    把 <主程序名> 换成 manifest/README 里写的那个（例如 app/emby-in-one）
# [ -f "app/<主程序名>" ] || { echo "FATAL: app/<主程序名> 不存在，先编译或拷进来" >&2; exit 1; }
# [ "$(stat -c '%s' "app/<主程序名>")" -gt 1000000 ] || { echo "FATAL: app/<主程序名> 不足 1MB，不像真二进制" >&2; exit 1; }

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
FPK="$(ls -t ${APP_NAME}-*.fpk 2>/dev/null | head -1)"
[ -n "$FPK" ] || { echo "FATAL: fnpack 未产出 ${APP_NAME}-*.fpk" >&2; exit 1; }
cp "$FPK" "$OUT"

# ---- 4. 自检 ----
# ⚠️ 本脚本开了 pipefail：`tar | grep -q` 会让 grep 提前退管道，tar 收 SIGPIPE(141) → 整条被判失败
#    （误报"缺文件"）。先把列表取进变量，再对变量 grep。
echo "=== 自检 ==="
echo "产物: $OUT  $(($(stat -c '%s' "$OUT") / 1024 / 1024))MB  sha256=$(sha256sum "$OUT" | cut -d' ' -f1)"
FPK_LIST="$(tar tzf "$OUT" 2>/dev/null || true)"
INNER_LIST="$(tar xzOf "$OUT" app.tgz 2>/dev/null | tar tzf - 2>/dev/null || true)"
MANIFEST_IN="$(tar -xOf "$OUT" manifest 2>/dev/null || true)"
[ -n "$FPK_LIST" ] && echo "✅ fpk 是合法 gzip+tar"
printf '%s\n' "$FPK_LIST" | grep -q '^cmd/main$' && echo "✅ cmd/main 在包内" || echo "❌ 缺 cmd/main"
printf '%s\n' "$FPK_LIST" | grep -qE '^ui/config$|^app.tgz$' && echo "✅ 桌面入口在位" || echo "❌ 缺 ui/（入口打不开）"
printf '%s\n' "$INNER_LIST" | grep -q '^ui/config' && echo "✅ ui/config 在 app.tgz 内层" || echo "⚠️ app.tgz 内无 ui/config（无桌面入口可忽略）"
# 包内不得带备份文件
printf '%s\n' "$INNER_LIST" | grep -qE '\.bak|~$|\.orig$' && echo "❌ app.tgz 内含备份文件" || echo "✅ app.tgz 无备份污染"
# cmd/main 必须用 share 软链优先解析，不能是 dirname 占位路径写法
if printf '%s\n' "$(tar -xOf "$OUT" cmd/main 2>/dev/null || true)" | grep -q 'resolve_share_root'; then
  echo "✅ cmd/main 用 resolve_share_root（软链优先）"
else
  echo "❌ cmd/main 缺 resolve_share_root —— share 路径可能落到 /vol0 占位目录"
fi
printf '%s\n' "$MANIFEST_IN" | grep -E "^(version|appname|maintainer|distributor)" || true
printf '%s\n' "$MANIFEST_IN" | grep -Eq "^version[[:space:]]+=[[:space:]]+${VERSION}[[:space:]]*$" \
  && echo "✅ 包内版本 = ${VERSION}" || echo "❌ 包内版本与 ${VERSION} 不符"

echo ""
echo "完成。产物在 $OUT —— 铁律：不自动安装，装哪先问用户。"