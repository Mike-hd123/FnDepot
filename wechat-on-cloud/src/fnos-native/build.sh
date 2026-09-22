#!/bin/bash
# wechat-on-cloud fpk 打包层还原构建脚本
#
# 目录来源:
# - cmd/ config/ wizard/ manifest: 从原 fpk (sha256 bfef9610696e6ea74f7b52069eac41f8df0a30632f7c9d951fb516732f8a7401) 解包原样还原
# - app.tgz: 定档自原 fpk 包内 app.tgz, 字节原样, 不重打。
#   原因: 面板代码在 src/panel 可复现构建, 但产物带 node_modules, 构建环境差异
#   会导致字节不稳定, 故将 app.tgz 定档为唯一应用包来源。
#
# checksum 注意:
# - manifest 内 checksum 字段 = md5(app.tgz)。本脚本用 md5sum 校验两者一致,
#   不一致则中止, 防止 app.tgz 被替换后 checksum 失配。
# - 当前值: ad93204e90a67c128a726965a8467f2b。若将来更新 app.tgz, 需用
#   `md5sum app.tgz` 重算并同步 manifest 的 checksum 行。
#
# 构建流程说明:
# 1. 先在 mktemp -d 中间组装目录执行 `fnpack build -d .` 做结构校验。
#    为满足 fnpack v1.2.4 的源目录约定(app/ 目录), 先把定档 app.tgz 解到 app/。
#    注意: fnpack v1.2.4 会无条件重新压缩 app.tgz(压缩时 mtime 取拷贝时刻,
#    两次构建字节不同), 且把 manifest 的 checksum 改写为新 app.tgz 的 md5,
#    其产物不能直接发布(会破坏 checksum=md5(定档 app.tgz) 与 manifest 不变
#    两个约束), 故仅作结构校验。
# 2. 随后用 python3 tarfile 确定性重拼最终 fpk: app.tgz 与 manifest 直接取
#    fnos-native/ 定档原样装入(不经 fnpack 产物), 目录头不带尾斜杠、
#    gzip mtime=0, 条目顺序与原 fpk 一致, 保证 `tar tzf` 清单与原包逐行相同。
#
# 产物: 覆盖项目根 wechat-on-cloud.fpk
set -euo pipefail

NATIVE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJ_ROOT="$(dirname "$(dirname "$NATIVE_DIR")")"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# --- 0. checksum 不变式校验: checksum 字段必须等于 md5(app.tgz) ---
APP_MD5="$(md5sum "$NATIVE_DIR/app.tgz" | awk '{print $1}')"
MAN_CHKSUM="$(sed -n 's/^checksum[[:space:]]*=[[:space:]]*//p' "$NATIVE_DIR/manifest")"
if [ "$APP_MD5" != "$MAN_CHKSUM" ]; then
  echo "ERROR: manifest checksum($MAN_CHKSUM) != md5(app.tgz)($APP_MD5), 中止" >&2
  exit 1
fi
echo "checksum invariant OK: $APP_MD5"

# --- 1. 组装中间目录 (fnpack 约定: app/ 目录为应用体) ---
cp -a "$NATIVE_DIR/cmd"     "$WORK/cmd"
cp -a "$NATIVE_DIR/config"  "$WORK/config"
cp -a "$NATIVE_DIR/wizard"  "$WORK/wizard"
cp -a "$NATIVE_DIR/manifest" "$WORK/manifest"
cp -a "$PROJ_ROOT/ICON.PNG"     "$WORK/ICON.PNG"
cp -a "$PROJ_ROOT/ICON_256.PNG" "$WORK/ICON_256.PNG"
mkdir "$WORK/app"
tar xzf "$NATIVE_DIR/app.tgz" -C "$WORK/app"

# --- 2. fnpack build -d . 结构校验 (产物丢弃) ---
if ! ( cd "$WORK" && fnpack build -d . ); then
  echo "ERROR: fnpack build 结构校验失败, 中止" >&2
  exit 1
fi
[ -f "$WORK/wechat-on-cloud.fpk" ] || { echo "ERROR: fnpack 未产出 fpk" >&2; exit 1; }
echo "fnpack 结构校验通过 (fnpack 产物丢弃, 不发布)"

# --- 3. 确定性重拼最终 fpk ---
# app.tgz 与 manifest 从 fnos-native/ 定档直取, 不经 fnpack 产物,
# 保证内层 app.tgz 字节原样(md5=checksum) 且 manifest 逐字节不变。
python3 - "$WORK" "$NATIVE_DIR" <<'PYEOF'
import gzip, os, pwd, grp, shutil, stat, sys, tarfile
from fnmatch import fnmatch

work, native = sys.argv[1], sys.argv[2]
out = os.path.join(work, "wechat-on-cloud.fpk")
os.chdir(work)

# 条目清单与顺序同原 fpk: app.tgz 最先, 其余按不区分大小写字典序
entries = ["app.tgz"] + sorted(
    ["cmd", "config", "ICON.PNG", "ICON_256.PNG", "manifest", "wizard"],
    key=str.lower,
)
# 定档来源覆盖: app.tgz / manifest 以 fnos-native/ 为准 (copy2 保字节与 mtime)
shutil.copy2(os.path.join(native, "app.tgz"), os.path.join(work, "app.tgz"))
shutil.copy2(os.path.join(native, "manifest"), os.path.join(work, "manifest"))

class BareDirInfo(tarfile.TarInfo):
    """目录条目名字不带尾斜杠, 与原 fpk (Go tar 写入) 清单一致"""
    def get_info(self):
        info = tarfile.TarInfo.get_info(self)
        if info["type"] == tarfile.DIRTYPE and info["name"].endswith("/"):
            info["name"] = info["name"].rstrip("/")
        return info

def tarinfo_for(path, name):
    st = os.lstat(path)
    ti = BareDirInfo(name)
    ti.size = st.st_size
    ti.mtime = int(st.st_mtime)
    ti.mode = stat.S_IMODE(st.st_mode)
    ti.uid = st.st_uid
    ti.gid = st.st_gid
    try:
        ti.uname = pwd.getpwuid(st.st_uid).pw_name
    except KeyError:
        ti.uname = ""
    try:
        ti.gname = grp.getgrgid(st.st_gid).gr_name
    except KeyError:
        ti.gname = ""
    if stat.S_ISDIR(st.st_mode):
        ti.type = tarfile.DIRTYPE
        ti.size = 0
    elif stat.S_ISLNK(st.st_mode):
        ti.type = tarfile.SYMTYPE
        ti.linkname = os.readlink(path)
    else:
        ti.type = tarfile.REGTYPE
    return ti

raw = gzip.GzipFile(filename="", mode="wb", compresslevel=6, mtime=0,
                    fileobj=open(out + ".tmp", "wb"))
with tarfile.open(fileobj=raw, mode="w") as tf:
    for e in entries:
        if os.path.isdir(e) and not os.path.islink(e):
            # 目录条目: 名字不带尾斜杠, 先目录后子项(排序)
            tf.addfile(tarinfo_for(e, e))
            for child in sorted(os.listdir(e), key=str.lower):
                if fnmatch(child, "*.bak_*"):
                    continue  # 备份文件不入包
                cp = os.path.join(e, child)
                tf.addfile(tarinfo_for(cp, f"{e}/{child}"),
                           open(cp, "rb") if os.path.isfile(cp) else None)
        else:
            tf.addfile(tarinfo_for(e, e), open(e, "rb"))
raw.close()
os.replace(out + ".tmp", out)
PYEOF

# --- 4. 产物回移覆盖项目根 fpk ---
mv -f "$WORK/wechat-on-cloud.fpk" "$PROJ_ROOT/wechat-on-cloud.fpk"

echo "built: $PROJ_ROOT/wechat-on-cloud.fpk"
echo "size: $(stat -c '%s' "$PROJ_ROOT/wechat-on-cloud.fpk")"
sha256sum "$PROJ_ROOT/wechat-on-cloud.fpk"
echo "app.tgz md5: $APP_MD5 (manifest checksum 不变)"
