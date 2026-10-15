#!/bin/bash
# build.sh — 从官方 release 组装 EZBookkeeping fnOS fpk（原生应用，参照 9router 标准打法）
#
# 用法: ./build.sh [VERSION] [REL] [ARCH]   默认 VERSION=2.0.1 REL=2 ARCH=x86
#   REL=微版本后缀(如 5),用于本地再打包迭代; 上游 tarball 始终用纯 VERSION。
# 依赖: curl, tar, sed, fnpack (https://static2.fnnas.com/fnpack/fnpack-1.2.1-linux-amd64)
# 产物: ezbookkeeping-<VERSION>-<REL>-<ARCH>.fpk
#
# 设计约束:
#   - 二进制 = 上游官方 release 原样（不重编译）
#   - 数据与配置真身落 data-share「ezbookkeeping」（TRIM_DATA_SHARE_PATHS，经两跳解析）；
#     @appdata（TRIM_PKGVAR）只放日志/PID 等运行态文件
#   - 端口 8580（避开 8080/8081/20128）

set -euo pipefail

VERSION="${1:-2.0.1}"
REL="${2:-2}"
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
desc                  = 轻量自托管个人记账：多账本/报表洞察/账单批量导入(Excel·微信·支付宝·京东)/原生 MCP 与 API 接口。上游 v2.0.1（v2.0.x 系：Monaco 编辑器/洞察报表自定义图表/信用卡额度/S3 对象存储/全新仪表盘；2.0.1 收紧 API token 权限——不可再创建/吊销 token、清数据、改资料、配置 2FA/外部认证/同步，另有移动端加记账按钮与定时模板时区修复）；延续 v1.6.1-14 本地定制：index 纯跳转页(恢复上游原版 SPA 入口)、sidecar desktop/mobile.html 静态兜底 + 剥前缀反代 + gzip、端口 8580；2.0.1-2 起数据与配置真身落 data-share「ezbookkeeping」（应用文件），@appdata 只留日志/PID/临时运行文件。
changelog             = 2.0.1-2（2026-10-15）：share 合规修复。①数据与配置真身全面改落 data-share「ezbookkeeping」（文件管理器 → 应用文件），db/附件 storage/日志/ezbookkeeping.ini 都在共享目录里，升级/重装不丢；@appdata 只留 boot.log 与 PID 等运行态文件。②config/resource 声明 data-share（此前是空 {}），四个生命周期脚本各内联一份 resolve_share_root 两跳解析（先 /var/apps/<app>/shares/<name> 软链，退化 TRIM_DATA_SHARE_PATHS 首段），不再用 dirname 拼未落卷占位路径。③新增幂等不覆盖迁移：老 @appdata 的 data/log/storage 有东西且 share 目标为空时才 cp -a，源目录保留不删，日志写 migrate.log——已在 share 的数据不会被重复搬运；升级与首装两条通道都做，处理跨卷历史遗留（vol1/vol2 @appdata 并存）。④卸载新增双选向导 wizard/uninstall（字段 wizard_delete_data，默认保留），选清除时先停进程再 find -mindepth 1 清空 share 内容（含隐藏文件）并清 @appdata。⑤安装/升级失败信息写 TRIM_TEMP_LOGFILE，应用中心界面可见。2.0.1-1：上游 v2.0.1 对齐（API token 权限收紧、移动端加记账按钮、定时模板时区修复）。
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

# ⚠️ .bak 闸门（与 emby-in-one/build.sh 同款）：${BUILD_DIR} 是即将 fnpack build 的源目录，
#    step4 的 `cp -r cmd/config/wizard` 会把源码目录里的 *.bak_* 一并拷进来，
#    fnpack 会静默打进 app.tgz = 装到设备上多一堆垃圾。命中即 FATAL 退出，先移出再打包。
BAK="$(find "${BUILD_DIR}" -name '*.bak*' -o -name '*_~' -o -name '*.orig' 2>/dev/null | head -20)"
if [ -n "$BAK" ]; then
  echo "FATAL: 包内含备份文件，先移出再打包：" >&2
  echo "$BAK" >&2
  exit 1
fi

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
