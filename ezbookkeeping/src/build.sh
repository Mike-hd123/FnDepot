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

# ── 3. 配置打占位符（首次启动由 cmd/main 换真实值）──
echo "[3/7] 配置占位符化..."
INI="${BUILD_DIR}/app/server/conf/ezbookkeeping.ini"
sed -i \
    -e 's#^http_port = 8080#http_port = 8580#' \
    -e 's#^secret_key =$#secret_key = @SECRET_KEY@#' \
    -e 's#^db_path = data/ezbookkeeping.db#db_path = @DB_PATH@#' \
    -e 's#^log_path = log/ezbookkeeping.log#log_path = @LOG_PATH@#' \
    -e 's#^local_filesystem_path = storage/#local_filesystem_path = @STORAGE_PATH@#' \
    "${INI}"
for k in http_port secret_key db_path log_path local_filesystem_path; do
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
display_name          = EZBookkeeping
desc                  = 轻量自托管个人记账：多账本/报表洞察/账单批量导入(Excel·微信·支付宝·京东)/原生 MCP 与 API 接口。v${REL} 修复手机端 /app/ 100001：gateway sidecar 由裸 TCP 盲转发改为剥前缀 HTTP 反代（对齐 EasyTier gateway-proxy），无尾斜杠自动 301，前端相对/绝对资源与 API 均可正确路由。
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
