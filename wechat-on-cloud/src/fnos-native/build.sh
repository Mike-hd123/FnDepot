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
#
# --from-source 模式（全量构建链路, 配合 build-app.sh）:
# - 应用体取 fnos-native/app.new.tgz（build-app.sh 从 panel 源码产出）, 不取定档 app.tgz;
# - manifest 的 checksum 字段同步改写为 md5(app.new.tgz) 后入包（仅改工作副本, 不写回定档 manifest）;
# - 定档 app.tgz 与 manifest 原样不动, 默认模式仍走定档路径保回归。
set -euo pipefail

NATIVE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJ_ROOT="$(dirname "$(dirname "$NATIVE_DIR")")"
WORK="$(mktemp -d)"
MAN_TMP=""
trap 'rm -rf "$WORK" ${MAN_TMP:+"$MAN_TMP"}' EXIT

FROM_SOURCE=0
for arg in "$@"; do
  case "$arg" in
    --from-source) FROM_SOURCE=1 ;;
    *) echo "ERROR: 未知参数 $arg (支持: --from-source)" >&2; exit 1 ;;
  esac
done

# --- 0. 应用体选择 + checksum 不变式校验 ---
if [ "$FROM_SOURCE" = 1 ]; then
  APP_SRC="$NATIVE_DIR/app.new.tgz"
  [ -f "$APP_SRC" ] || { echo "ERROR: --from-source 需要 app.new.tgz, 先跑 build-app.sh" >&2; exit 1; }
  APP_MD5="$(md5sum "$APP_SRC" | awk '{print $1}')"
  # manifest 工作副本: checksum 改写为 app.new.tgz 的 md5（定档 manifest 不动）
  MAN_TMP="$(mktemp)"
  # ⚠️ 用 \1 回captured 前缀，保留 manifest 原有「checksum<空格>= 」列对齐：旧写法硬编码 20 个空格，
  #    而定档 manifest 这行是 19 个空格（与上面 version 行的等号列对齐），导致 --from-source 与默认
  #    路径产出的 fpk 差 1 字节、sha 不同（1.6.0-1 实测）。改后两条路径逐字节一致。
  sed -E "s/^(checksum[[:space:]]*=).*/\1 $APP_MD5/" \
      "$NATIVE_DIR/manifest" > "$MAN_TMP"
  MANIFEST_SRC="$MAN_TMP"
  echo "from-source 模式: app.new.tgz md5=$APP_MD5 (checksum 已同步, 定档未动)"
else
  APP_SRC="$NATIVE_DIR/app.tgz"
  MANIFEST_SRC="$NATIVE_DIR/manifest"
  APP_MD5="$(md5sum "$APP_SRC" | awk '{print $1}')"
  MAN_CHKSUM="$(sed -n 's/^checksum[[:space:]]*=[[:space:]]*//p' "$MANIFEST_SRC")"
  if [ "$APP_MD5" != "$MAN_CHKSUM" ]; then
    echo "ERROR: manifest checksum($MAN_CHKSUM) != md5(app.tgz)($APP_MD5), 中止" >&2
    exit 1
  fi
  echo "checksum invariant OK: $APP_MD5"
fi

# --- 1. 组装中间目录 (fnpack 约定: app/ 目录为应用体) ---
cp -a "$NATIVE_DIR/cmd"     "$WORK/cmd"
cp -a "$NATIVE_DIR/config"  "$WORK/config"
cp -a "$NATIVE_DIR/wizard"  "$WORK/wizard"
cp -a "$MANIFEST_SRC" "$WORK/manifest"
cp -a "$PROJ_ROOT/ICON.PNG"     "$WORK/ICON.PNG"
cp -a "$PROJ_ROOT/ICON_256.PNG" "$WORK/ICON_256.PNG"
mkdir "$WORK/app"
tar xzf "$APP_SRC" -C "$WORK/app"

# ⚠️ .bak 闸门（与 emby-in-one/build.sh 同款）：$WORK 是即将 fnpack build 的源目录（step1 用
#    `cp -a cmd config wizard` 拷进来，源码里的 *.bak_* 会被一并带上）。step2 的 fnpack 会把
#    $WORK 整个打进它的 app.tgz（虽产物丢弃不发布），step3 的 python 重拼才用 fnmatch 跳过 *.bak_*。
#    闸门放最前，逼先把备份移出源码目录再构建——别指望 python 兜底（那是发布态，结构校验态不兜）。
BAK="$(find "$WORK/cmd" "$WORK/config" "$WORK/wizard" -name '*.bak*' -o -name '*_~' -o -name '*.orig' 2>/dev/null | head -20)"
if [ -n "$BAK" ]; then
  echo "FATAL: 包内含备份文件，先移出再打包：" >&2
  echo "$BAK" >&2
  exit 1
fi
# $WORK/app 是定档 app.tgz 解出的字节原样载荷，不归源码管：1.5.2-1 就已内含
# server/src/docker.ts.bak_20261012_0657 —— 清它要改定档字节并重算 manifest checksum
# （属需装机验证的变更），留到下次重建 app.tgz 一并做。故这里只 WARN，不阻断构建。
# ⚠️ 末尾 `|| true` 必需：本脚本开了 set -o pipefail，而 --from-source 用的是全量重建的
#    app.new.tgz（build-app.sh 已跳过 *.bak_*/.pyc），里面【没有】备份文件 → grep 无匹配退 1
#    → 命令替换返回 1 → set -e 静默中止（1.6.0-1 首建时就是这样一声不响 exit 1）。
#    没有备份文件是本链路期望的正常状态，不是错误。
APPBAK="$( { tar tzf "$APP_SRC" 2>/dev/null | grep -E '\.bak(\.|_|$)|\.orig|~$' | head -5; } || true)"
if [ -n "$APPBAK" ]; then
  echo "WARN: 定档 app.tgz 内有历史备份文件（本轮不阻断，下次重建 app.tgz 时清理）:" >&2
  echo "$APPBAK" >&2
fi

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
python3 - "$WORK" "$APP_SRC" "$MANIFEST_SRC" <<'PYEOF'
import gzip, os, pwd, grp, shutil, stat, sys, tarfile
from fnmatch import fnmatch

work, app_src, manifest_src = sys.argv[1], sys.argv[2], sys.argv[3]
out = os.path.join(work, "wechat-on-cloud.fpk")
os.chdir(work)

# 条目清单与顺序同原 fpk: app.tgz 最先, 其余按不区分大小写字典序
entries = ["app.tgz"] + sorted(
    ["cmd", "config", "ICON.PNG", "ICON_256.PNG", "manifest", "wizard"],
    key=str.lower,
)
# 定档来源覆盖: app.tgz / manifest 以选定的源为准 (copy2 保字节与 mtime)
shutil.copy2(app_src, os.path.join(work, "app.tgz"))
shutil.copy2(manifest_src, os.path.join(work, "manifest"))

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
