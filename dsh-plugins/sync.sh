#!/bin/bash
# dsh-plugins 现场 <-> 仓库 同步（只 cp -a，覆盖前留 .bak，逐字节核对，绝不删除）
#   用法：bash dsh-plugins/sync.sh <插件名> <pull|push>
#     pull = 现场 → 仓库（在现场改完插件、准备提交前用）
#     push = 仓库 → 现场（回滚或换机重装用；推完要重启 Harness 才生效）
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLUGIN_DIR="$REPO_ROOT/dsh-plugins"

name="${1:?用法: sync.sh <插件名> <pull|push>}"; dir="${2:?用法: sync.sh <插件名> <pull|push>}"

# 插件名 → 现场路径（新插件在这里加一行；link 安装的插件，现场目录就是它的源）
case "$name" in
  hy-memory) LIVE=/vol1/@appshare/Harness/hy-memory ;;
  *) echo "FATAL: 未知插件 '$name'（已知：hy-memory；加新插件请在 sync.sh 的 case 里登记现场路径）" >&2; exit 1 ;;
esac
SRC_DIR="$PLUGIN_DIR/$name"
[ -d "$LIVE" ] || { echo "FATAL: 现场目录不存在 $LIVE" >&2; exit 1; }
[ -d "$SRC_DIR" ] || { echo "FATAL: 仓库目录不存在 $SRC_DIR" >&2; exit 1; }
[ "$dir" = pull ] || [ "$dir" = push ] || { echo "FATAL: 方向只能是 pull 或 push" >&2; exit 1; }

# 参与同步的文件清单（.bak_* 一律不进仓库、也不回写现场）
mapfile -t FILES < <(cd "$SRC_DIR" && find . -type f ! -name "*.bak_*" | sed 's|^\./||' | sort)
if [ "${#FILES[@]}" = "0" ]; then echo "FATAL: $SRC_DIR 里没有可比对的文件"; exit 1; fi

SRC="$SRC_DIR"; DST="$LIVE"; TS=$(date +%Y%m%d_%H%M%S); changed=0; failed=0
if [ "$dir" = pull ]; then SRC="$LIVE"; DST="$SRC_DIR"; fi

echo "== $name：$dir（$SRC → $DST），共 ${#FILES[@]} 个文件 =="
for f in "${FILES[@]}"; do
  s="$SRC/$f"; d="$DST/$f"
  if [ ! -f "$s" ]; then echo "  SKIP 源缺 $f"; continue; fi
  ss=$(sha256sum "$s" | cut -c1-64)
  if [ -f "$d" ] && [ "$(sha256sum "$d" | cut -c1-64)" = "$ss" ]; then
    echo "  SAME  $f"; continue
  fi
  if [ -f "$d" ]; then
    cp -a "$d" "$d.bak_$TS" || { echo "  FAIL 留底失败 $f"; failed=1; continue; }
    echo "  COPY  $f（原文件留底 $(basename "$d").bak_$TS）"
  else
    mkdir -p "$(dirname "$d")"; echo "  NEW   $f"
  fi
  cp -a "$s" "$d" || { echo "  FAIL 复制失败 $f"; failed=1; continue; }
  ds=$(sha256sum "$d" | cut -c1-64)
  if [ "$ds" != "$ss" ]; then echo "  ❌ $f 逐字节核对不一致，回滚留底"; cp -a "$d.bak_$TS" "$d" 2>/dev/null; failed=1; continue; fi
  echo "  OK    $f ($(stat -c %s "$d")B)"
  changed=$((changed+1))
done

echo "变更 $changed 个文件，失败 $failed"
[ "$failed" = "0" ] || exit 1
if [ "$dir" = push ]; then
  echo "下一步：重启 Harness 才会加载新代码（link 安装在启动时读入）。"
  echo "复核加载：ps -o lstart -p \$(pgrep -f 'dsh.*web' | head -1)  的时间应晚于文件 mtime。"
else
  echo "下一步：git -C $REPO_ROOT diff --stat dsh-plugins 后提交。"
fi
