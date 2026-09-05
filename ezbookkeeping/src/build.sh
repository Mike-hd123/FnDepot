#!/bin/bash
# build.sh — 从官方 release 组装 EZBookkeeping fnOS fpk（原生应用，参照 9router 标准打法）
#
# 用法: ./build.sh [VERSION] [REL] [ARCH]   默认 VERSION=1.6.1 REL=5 ARCH=x86
#   REL=微版本后缀(如 5),用于本地再打包迭代; 上游 tarball 始终用纯 VERSION。
# 依赖: curl, tar, sed, fnpack (https://static2.fnnas.com/fnpack/fnpack-1.2.1-linux-amd64)
# 产物: ezbookkeeping-<VERSION>-<REL>-<ARCH>.fpk
#
# 设计约束:
#   - 二进制 = 上游官方 release 原样（不重编译）
#   - 数据目录钉死 /volX/@appdata/ezbookkeeping（TRIM_PKGVAR），升级永不换目录
#   - 端口 8580（避开 8080/8081/20128）

set -euo pipefail

VERSION="${1:-1.6.1}"
REL="${2:-5}"
ARCH="${3:-x86}"
SRC_DIR="$(cd "$(dirname "$0")" && pwd)"
BUILD_DIR="/tmp/build-ezbookkeeping-fpk-$$"
OUT_FILE="${SRC_DIR}/ezbookkeeping-${VERSION}-${REL}-${ARCH}.fpk"
FULL_VERSION="${VERSION}-${REL}"

case "${ARCH}" in
    x86) UARCH="linux-amd64" ;;
    arm) UARCH="linux-arm64" ;;
    *) echo "ERROR: ARCH 只支持 x86|arm"; exit 1 ;;
esac

echo "=========================================="
echo "  EZBookkeeping fnOS fpk 构建"
echo "  Version: ${FULL_VERSION}  Arch: ${ARCH} (${UARCH})"
echo "=========================================="

rm -rf "${BUILD_DIR}"
mkdir -p "${BUILD_DIR}/app/server" "${BUILD_DIR}/app/ui/images"

# ── 1. 获取官方 release（优先本地缓存）──
TARBALL="${SRC_DIR}/ezbookkeeping-v${VERSION}-${UARCH}.tar.gz"
if [ ! -f "${TARBALL}" ]; then
    echo "[1/7] 下载上游 ${UARCH} release..."
    curl -fsSL -o "${TARBALL}" \
        "https://github.com/mayswind/ezbookkeeping/releases/download/v${VERSION}/ezbookkeeping-v${VERSION}-${UARCH}.tar.gz"
else
    echo "[1/7] 使用本地缓存: ${TARBALL}"
fi
md5sum "${TARBALL}"

# ── 2. 解包到 app/server（app/ 下内容由 fnpack 打入 app.tgz，解压到 APPDEST）──
echo "[2/7] 解包官方二进制..."
tar xzf "${TARBALL}" -C "${BUILD_DIR}/app/server"

# ── 2.5 前端跳转器保持上游原样（1.6.1-10 起撤销 -9 的对调 patch）
# 用户已确认：-9 把手机 UA 调到 mobile.html（底部 tab 导航），但用户手机端长期习惯的是
# desktop.html 的桌面版导航（顶部/侧边导航）。故恢复上游原样：手机 UA 命中
# `?t("desktop"):t("mobile")` 的 `desktop` 分支 → 桌面版。此段不 patch，仅断言上游未被改动，
# 上游结构变化会上游修复合并，此处 grep 失败即主动报错退出，不会静默。
echo "[2.5/7] 保持前端 UA 跳转器上游原样（手机→desktop 桌面版导航）..."
PUB="${BUILD_DIR}/app/server/public"
JUMPER="$(grep -rl 't("desktop"):t("mobile")' "${PUB}"/js/index-*.js 2>/dev/null | head -1 || true)"
if [ -z "${JUMPER}" ]; then
    echo "NOTICE: 未找到跳转器特征串（js/index-*.js 含 t(\"desktop\"):t(\"mobile\")），上游可能已修复合并，跳过断言（保持原样）"
else
    # 断言上游原样：必须仍是 手机→desktop（+分支）。-9 的对调 patch 会把这里改成 t("mobile"):t("desktop")，
    # 若发现被对调则说明打错 / 混入了 -9 产物，直接报错。
    if grep -q '?t("mobile"):t("desktop");' "${JUMPER}"; then
        echo "ERROR: 跳转器仍是 -9 的对调 patch（t(\"mobile\"):t(\"desktop\")），撤销失败或混入旧产物: ${JUMPER}"
        exit 1
    fi
    grep -q '?t("desktop"):t("mobile");' "${JUMPER}" || { echo "ERROR: 跳转器上游原样断言失败: ${JUMPER}"; exit 1; }
fi
echo "  上游原样保持: 手机 UA→desktop.html（桌面版导航）, 桌面 UA→mobile.html"

# ── 2.6 index.html 纯跳转页化（P3：消除双重页面加载）──
# 上游 index.html 是完整 Vue SPA（431B 入口 import vendor-common 734KB+CSS 101KB），
# 执行完 UA 检测才 location.replace 到目标页，目标页再拉一遍自身 bundle = 双重页面生命周期。
# 此处把 index.html 替换为纯 inline-JS 跳转页（~1KB，零 bundle），UA 检测逻辑与上游
# js/index-*.js 完全一致（移动设备→desktop.html，桌面→mobile.html），一步直达目标页。
echo "[2.6/7] index.html 纯跳转页化（消除双重加载 ~835KB 白拉）..."
PUB2="${BUILD_DIR}/app/server/public"
if [ -f "${PUB2}/index.html" ]; then
    cat > "${PUB2}/index.html" <<'INDEX_EOF'
<!DOCTYPE html>
<html lang="zh-CN">
<head>
    <meta charset="utf-8">
    <meta http-equiv="Content-Type" content="text/html;charset=utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no, minimal-ui, viewport-fit=cover">
    <meta name="mobile-web-app-capable" content="yes">
    <meta name="apple-mobile-web-app-capable" content="yes"/>
    <meta name="apple-mobile-web-app-title" content="记账"/>
    <meta name="apple-mobile-web-app-status-bar-style" content="default"/>
    <meta name="theme-color" content="#c67e48">
    <meta name="format-detection" content="telephone=no"/>
    <meta name="description" content="轻量自托管个人记账">
    <title>记账</title>
    <link rel="shortcut icon" type="image/x-icon" href="favicon.ico">
    <link rel="apple-touch-icon" href="touchicon.png">
</head>
<body>
    <noscript>
        <strong>We're sorry but ezBookkeeping doesn't work properly without JavaScript enabled. Please enable it to continue.</strong>
    </noscript>
    <script>
    // 纯跳转页：与上游入口 bundle 的 UA 检测逻辑一致
    // 移动设备（含 wearable/embedded）→ desktop.html（桌面版导航），桌面 → mobile.html
    !function(){
        var ua = navigator.userAgent;
        if (!ua) { window.location.replace('desktop.html'); return; }
        var mobile = /Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini|Mobile/i.test(ua);
        var page = mobile ? 'desktop.html' : 'mobile.html';
        window.location.replace(page + '#/');
    }();
    </script>
</body>
</html>
INDEX_EOF
    # 断言：确认已替换（不再是 SPA 入口；匹配真正的 module script 标签，不误伤注释）
    grep -q '纯跳转页' "${PUB2}/index.html" || { echo "ERROR: index.html 纯跳转页化失败"; exit 1; }
    grep -q 'script type="module"[^>]*src="\./js/index-' "${PUB2}/index.html" && { echo "ERROR: index.html 仍引用 SPA 入口 bundle，替换失败"; exit 1; }
    echo "  index.html 已替换为纯跳转页（~1KB，不再拉取 835KB bundle）"
else
    echo "NOTICE: 未找到 index.html，跳过纯跳转页化"
fi

# ── 3. 配置打占位符（首次启动由 cmd/main 换真实值）──
echo "[3/7] 配置占位符化..."
INI="${BUILD_DIR}/app/server/conf/ezbookkeeping.ini"
sed -i \
    -e 's#^http_port = 8080#http_port = 8580#' \
    -e 's#^secret_key =$#secret_key = @SECRET_KEY@#' \
    -e 's#^db_path = data/ezbookkeeping.db#db_path = @DB_PATH@#' \
    -e 's#^log_path = log/ezbookkeeping.log#log_path = @LOG_PATH@#' \
    -e 's#^local_filesystem_path = storage/#local_filesystem_path = @STORAGE_PATH@#' \
    -e 's#^enable_gzip = false#enable_gzip = true#' \
    "${INI}"
for k in http_port secret_key db_path log_path local_filesystem_path enable_gzip; do
    grep -q "^${k} = " "${INI}" || { echo "ERROR: ${k} 占位失败"; exit 1; }
done
grep -q '@SECRET_KEY@\|@DB_PATH@\|@LOG_PATH@\|@STORAGE_PATH@' "${INI}" || { echo "ERROR: 占位符未生效"; exit 1; }

# ── 4. 复制打包结构 ──
echo "[4/7] 复制 cmd/app/config/wizard..."
cp -r "${SRC_DIR}/cmd" "${BUILD_DIR}/cmd"
cp -r "${SRC_DIR}/app/ui/config" "${BUILD_DIR}/app/ui/config"
cp "${SRC_DIR}/app/ui/images/icon-64.png"  "${BUILD_DIR}/app/ui/images/"
cp "${SRC_DIR}/app/ui/images/icon-128.png" "${BUILD_DIR}/app/ui/images/"
cp "${SRC_DIR}/app/ui/images/icon-256.png" "${BUILD_DIR}/app/ui/images/"
cp -r "${SRC_DIR}/config" "${BUILD_DIR}/config"
cp -r "${SRC_DIR}/gateway" "${BUILD_DIR}/app/gateway"
cp -r "${SRC_DIR}/wizard" "${BUILD_DIR}/wizard"
cp "${SRC_DIR}/ICON.PNG"     "${BUILD_DIR}/ICON.PNG"
cp "${SRC_DIR}/ICON_256.PNG" "${BUILD_DIR}/ICON_256.PNG"

# ── 5. 生成 manifest ──
echo "[5/7] 生成 manifest..."
cat > "${BUILD_DIR}/manifest" <<EOF
appname               = ezbookkeeping
version               = ${FULL_VERSION}
display_name          = 记账
desc                  = 轻量自托管个人记账：多账本/报表洞察/账单批量导入(Excel·微信·支付宝·京东)/原生 MCP 与 API 接口。v11 新增 index.html 纯跳转页化消除双重加载；v10 撤销 v9 UA 跳转器 patch(恢复手机 desktop 桌面版导航)+enable_gzip=true；沿用 v5 gateway sidecar 剥前缀反代。
platform              = ${ARCH}
source                = thirdparty
maintainer            = MaysWind
maintainer_url        = https://github.com/mayswind/ezbookkeeping
distributor           = Mike
distributor_url       = https://github.com/Mike-hd123
desktop_uidir         = ui
desktop_applaunchname = ezbookkeeping.Application
service_port          = 8580
ctl_stop              = true
EOF

# ── 6. 权限 ──
echo "[6/7] 设置可执行权限..."
chmod +x "${BUILD_DIR}/app/server/ezbookkeeping" "${BUILD_DIR}"/cmd/*

# ── 7. fnpack 打包 ──
echo "[7/7] fnpack build..."
cd "${BUILD_DIR}"
fnpack build -d .
mv ezbookkeeping.fpk "${OUT_FILE}"
rm -rf "${BUILD_DIR}"

echo ""
echo "=========================================="
echo "  构建完成: ${OUT_FILE}"
ls -lh "${OUT_FILE}"
md5sum "${OUT_FILE}"
echo "=========================================="
