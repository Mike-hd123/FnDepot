#!/usr/bin/with-contenv bash
# linuxserver 启动钩子（/custom-cont-init.d，root 身份，每次启动、在各服务起来之前执行）。
#
# 目的：给每个实例一个【唯一且持久】的设备身份，避免所有实例共用镜像里烤死的同一个 machine-id。
#
# 背景（P0）：Debian/基础镜像把 machine-id 固化在镜像层里，于是全世界每个 wechat-on-cloud
# 实例的 /etc/machine-id 都相同。machine-id 是 Linux 上最接近"设备指纹"的标识，微信会读它做
# 风控；成千上万个账号共用同一个 machine-id = 典型"设备农场"特征 → 被腾讯批量判风险 → 登录即
# 被强制退出、反复循环。
#
# 解法：
#   1) 在数据卷（/config，随实例持久）里存一个本实例专属的随机 machine-id；
#   2) 每次启动把它写回 /etc/machine-id 与 /var/lib/dbus/machine-id。
# 这样：实例之间互不相同（破掉设备农场特征），且重启 / 升级 / 重建容器都保持不变（"设备老在变"
# 同样可疑）。仅在卷里尚无该文件时才生成，故老实例首启会拿到一个新的唯一 id（之后恒定）。
set -e

ID_FILE=/config/.woc-machine-id

# 生成 32 位小写十六进制（systemd machine-id 格式）：优先用内核 uuid 源，去掉连字符。
if [ ! -s "$ID_FILE" ]; then
    if [ -r /proc/sys/kernel/random/uuid ]; then
        tr -d '-' < /proc/sys/kernel/random/uuid | tr 'A-F' 'a-f' > "$ID_FILE"
    else
        head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n' > "$ID_FILE"
    fi
fi

MID="$(tr -dc 'a-f0-9' < "$ID_FILE" | head -c 32)"
# 兜底：若文件内容异常（长度不足 32），重新生成
if [ "${#MID}" -ne 32 ]; then
    MID="$(tr -d '-' < /proc/sys/kernel/random/uuid | tr 'A-F' 'a-f' | head -c 32)"
    printf '%s\n' "$MID" > "$ID_FILE"
fi

printf '%s\n' "$MID" > /etc/machine-id 2>/dev/null || true
mkdir -p /var/lib/dbus
printf '%s\n' "$MID" > /var/lib/dbus/machine-id 2>/dev/null || true

# 抹掉最明显的容器标记（微信可能据此判定非真实桌面）。/.dockerenv 由 docker 注入，删掉无副作用。
rm -f /.dockerenv 2>/dev/null || true

# 设备伪装：把 /etc/os-release 改成 deepin（微信官方支持的发行版；Deepin 本就基于 Debian，
# 与本镜像的 Debian 用户态一致，不自相矛盾）。面板按 WOC_SPOOF_OS 控制（默认开，=0 关）。
# /etc/os-release 是指向 /usr/lib/os-release 的软链，重定向会写穿到目标，故直接写它即可。
if [ "${WOC_SPOOF_OS:-1}" = "1" ]; then
    cat > /etc/os-release <<'OSEOF'
PRETTY_NAME="deepin 23"
NAME="deepin"
VERSION_ID="23"
VERSION="23"
VERSION_CODENAME=beige
ID=deepin
ID_LIKE=debian
HOME_URL="https://www.deepin.org/"
BUG_REPORT_URL="https://bbs.deepin.org/"
OSEOF
fi

echo "[woc-identity] machine-id 已设为本实例专属（持久化于数据卷）；os 伪装=${WOC_SPOOF_OS:-1}"
