#!/bin/bash
# 微信下载/解压控制脚本。由面板经 docker exec 触发（不再用共享卷/守护进程）：
#   install / update   下载官方 deb、dpkg-deb -x 解压到 /config/wechat、原子替换、pkill 让 autostart 用新版重启
#   status             输出当前状态 JSON（面板轮询用）
# 用 docker exec --user abc 调用，文件归属与微信运行用户一致。
set -u

STATE_DIR="${WOC_STATE_DIR:-/config/.woc-state}"
STATUS_FILE="$STATE_DIR/status.json"

INSTALL_DIR="/config/wechat"            # dpkg-deb -x 解压根；二进制在 opt/wechat/wechat
WORK_DIR="/config/.woc-dl"              # 下载/解压临时区（同卷，便于原子 mv）
VERSION_FILE="$INSTALL_DIR/.woc-version"

CDN_MAIN="${WECHAT_CDN:-https://dldir1v6.qq.com/weixin/Universal/Linux}"
CDN_FALLBACK="${WECHAT_CDN_FALLBACK:-https://dldir1.qq.com/weixin/Universal/Linux}"
UA="Mozilla/5.0"

wechat_bin() { echo "$INSTALL_DIR/opt/wechat/wechat"; }
is_installed() { [ -x "$(wechat_bin)" ]; }
cur_version() { [ -f "$VERSION_FILE" ] && cat "$VERSION_FILE" || echo ""; }

deb_filename() {
  case "$(dpkg --print-architecture 2>/dev/null)" in
    amd64) echo "WeChatLinux_x86_64.deb" ;;
    arm64) echo "WeChatLinux_arm64.deb" ;;
    *) echo "" ;;
  esac
}

# write_status <phase> <percent> <message>
# phase: idle|downloading|extracting|installing|done|error
write_status() {
  local phase="$1" percent="$2" message="$3"
  local installed=false version
  is_installed && installed=true
  version="$(cur_version)"
  mkdir -p "$STATE_DIR"
  cat > "$STATUS_FILE.tmp" <<EOF
{"phase":"$phase","percent":$percent,"installed":$installed,"version":"$version","message":"$message","updatedAt":$(date +%s)}
EOF
  mv -f "$STATUS_FILE.tmp" "$STATUS_FILE"
}

print_status() {
  if [ -f "$STATUS_FILE" ]; then
    cat "$STATUS_FILE"
  elif is_installed; then
    echo "{\"phase\":\"done\",\"percent\":100,\"installed\":true,\"version\":\"$(cur_version)\",\"message\":\"已安装\",\"updatedAt\":$(date +%s)}"
  else
    echo "{\"phase\":\"idle\",\"percent\":0,\"installed\":false,\"version\":\"\",\"message\":\"未安装\",\"updatedAt\":$(date +%s)}"
  fi
}

log() { echo "[$(date '+%F %T')] $*" >> "$STATE_DIR/install.log" 2>/dev/null; }

do_install() {
  local file tmp pid total cur pct rc=1 attempt=0
  file="$(deb_filename)"
  if [ -z "$file" ]; then
    write_status error 0 "不支持的架构：微信仅提供 x86_64 / arm64"
    return
  fi

  mkdir -p "$STATE_DIR" "$WORK_DIR"
  # 并发保护（issue：反复点安装会互相踩）：锁目录 mkdir 原子。已有【活着的】安装在跑 → 直接返回，
  # 让它继续（下面的 curl 支持断点续传，会自己下完）；否则每次触发都 rm 掉下到一半的包 → 永远装不完。
  local lock="$STATE_DIR/.install.lock"
  if ! mkdir "$lock" 2>/dev/null; then
    local lpid; lpid="$(cat "$lock/pid" 2>/dev/null || echo)"
    if [ -n "$lpid" ] && kill -0 "$lpid" 2>/dev/null; then
      log "已有安装进行中(pid=$lpid)，本次触发跳过"
      return
    fi
    rm -rf "$lock"; mkdir "$lock" 2>/dev/null || { log "抢锁失败，跳过"; return; }
  fi
  echo "$$" > "$lock/pid"
  trap 'rm -rf "$lock" 2>/dev/null' EXIT   # 本次 exec 短命，退出即释放锁
  log "开始安装 file=$file"

  tmp="$WORK_DIR/wechat.deb"

  # 取总大小用于进度 + 完整性判断（HEAD 可能失败，失败则进度走不确定值 -1）
  for base in "$CDN_MAIN" "$CDN_FALLBACK"; do
    total="$(curl -fsSLI -A "$UA" "$base/$file" 2>/dev/null | tr -d '\r' \
            | awk 'tolower($1)=="content-length:"{v=$2} END{print v}')"
    [ -n "${total:-}" ] && break
  done
  : "${total:=0}"

  # 磁盘空间预检（NAS 小盘写满是「卡进度/干脆没进度」头号真凶）：
  # 盘满时 curl 以退出码 23=本地写失败告终，连 status.json 都写不进去（面板遂显示无进度），
  # 极易被误当网络/代理问题（真实案例：用户为此查了半天梯子）。这里提前 df 判定，给可执行的磁盘报错。
  # 需求 ≈ deb 本体 + dpkg-deb -x 解压(约 3× deb) + 更新时新旧并存余量 → 取 deb 4 倍，不低于 900MB 兜底。
  local deb_kb need_kb avail_kb
  deb_kb=$(( ( total > 0 ? total : 220000000 ) / 1024 ))
  need_kb=$(( deb_kb * 4 )); [ "$need_kb" -lt 921600 ] && need_kb=921600
  avail_kb="$(df -Pk "$WORK_DIR" 2>/dev/null | awk 'NR==2{print $4}')"
  if [ -n "${avail_kb:-}" ] && [ "$avail_kb" -lt "$need_kb" ] 2>/dev/null; then
    log "磁盘空间不足：$WORK_DIR 可用 $((avail_kb/1024))MB < 需要 $((need_kb/1024))MB"
    write_status error 0 "磁盘空间不足：约需 $((need_kb/1024))MB 空闲，当前仅 $((avail_kb/1024))MB。请在宿主清理磁盘/旧镜像（docker image prune）后重试"
    return
  fi

  write_status downloading 0 "正在下载微信安装包"
  # 断点续传下载（-C -）：网络半路中断/被中间设备掐断时，下次从已下字节【继续】而非从 0 重来
  #（这正是"反复卡在同一百分比退出"的解药）。--retry-all-errors 对传输中断也重试；外层再多轮兜底。
  # 关键：绝不在重试前删 $tmp —— 保留部分文件才能续传。
  while [ "$attempt" -lt 6 ]; do
    attempt=$((attempt+1))
    for base in "$CDN_MAIN" "$CDN_FALLBACK"; do
      curl -fSL -C - --retry 3 --retry-all-errors --retry-delay 2 --connect-timeout 20 \
           -A "$UA" -o "$tmp" "$base/$file" & pid=$!
      while kill -0 "$pid" 2>/dev/null; do
        if [ "${total:-0}" -gt 0 ] 2>/dev/null; then
          cur="$(stat -c%s "$tmp" 2>/dev/null || echo 0)"
          pct=$(( cur * 90 / total )); [ "$pct" -gt 90 ] && pct=90
          write_status downloading "$pct" "正在下载微信安装包"
        else
          write_status downloading -1 "正在下载微信安装包"
        fi
        sleep 1
      done
      wait "$pid"; rc=$?
      [ "$rc" -eq 0 ] && break 2
      log "curl 退出码 $rc（base=$base，attempt=$attempt），已下 $(stat -c%s "$tmp" 2>/dev/null || echo 0) 字节"
    done
    # 已下满（校验用 total）也算成功——防某些实现在收尾时给非 0 退出码
    cur="$(stat -c%s "$tmp" 2>/dev/null || echo 0)"
    if [ "${total:-0}" -gt 0 ] && [ "$cur" -ge "$total" ]; then rc=0; break; fi
    write_status downloading -1 "下载中断，正在续传重试（$attempt/6）"
    sleep 2
  done
  if [ "$rc" -ne 0 ]; then
    log "下载最终失败 rc=$rc，已下 $(stat -c%s "$tmp" 2>/dev/null || echo 0)/$total"
    # curl 退出码 23=本地写失败：几乎总是盘写满（本场景最常见）。别误导用户查网络——
    # 再 df 复查一次，剩余空间过低同样判为磁盘问题，给磁盘专属报错。
    avail_kb="$(df -Pk "$WORK_DIR" 2>/dev/null | awk 'NR==2{print $4}')"
    if [ "$rc" -eq 23 ] || { [ -n "${avail_kb:-}" ] && [ "${avail_kb:-0}" -lt 51200 ] 2>/dev/null; }; then
      log "判定为磁盘空间不足（rc=$rc，$WORK_DIR 可用 ${avail_kb:-?}KB）"
      write_status error 0 "磁盘空间不足，下载无法写入。请在宿主清理磁盘/旧镜像（docker image prune）后重试"
      return
    fi
    write_status error 0 "下载失败（多次续传仍未完成，请检查网络/镜像后重试）"
    return
  fi

  write_status extracting 92 "正在解压安装"
  # 完整性校验：能被 dpkg-deb 读出版本才算完整包；半包/损坏包会解压失败或装出坏微信。
  if ! dpkg-deb -f "$tmp" Version >/dev/null 2>&1; then
    log "包不完整/损坏，删除重下"
    rm -f "$tmp"
    write_status error 0 "安装包不完整或损坏，已清理，请再次点击安装（将重新下载）"
    return
  fi
  local newroot="$WORK_DIR/new"
  rm -rf "$newroot"; mkdir -p "$newroot"
  if ! dpkg-deb -x "$tmp" "$newroot" 2>/dev/null; then
    write_status error 0 "解压失败，安装包可能损坏"
    rm -rf "$WORK_DIR"; return
  fi
  local ver; ver="$(dpkg-deb -f "$tmp" Version 2>/dev/null || echo "")"

  if [ ! -x "$newroot/opt/wechat/wechat" ]; then
    write_status error 0 "解压后未找到微信可执行文件"
    rm -rf "$WORK_DIR"; return
  fi

  write_status installing 96 "正在安装"
  # 原子替换：先挪走旧版再就位新版，最后清理
  rm -rf "$INSTALL_DIR.old"
  [ -e "$INSTALL_DIR" ] && mv "$INSTALL_DIR" "$INSTALL_DIR.old"
  mv "$newroot" "$INSTALL_DIR"
  echo "$ver" > "$VERSION_FILE"
  rm -rf "$INSTALL_DIR.old" "$WORK_DIR"

  write_status done 100 "安装完成"
  # 让 autostart 循环用新版本重启微信（若正在运行）
  pkill -f "$INSTALL_DIR/opt/wechat/wechat" 2>/dev/null || true
}

case "${1:-status}" in
  status)
    print_status
    ;;
  install|update)
    do_install
    ;;
  *)
    echo "用法: $0 {install|update|status}" >&2; exit 1 ;;
esac
