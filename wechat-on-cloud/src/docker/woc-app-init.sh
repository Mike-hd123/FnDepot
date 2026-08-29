#!/bin/bash
# /custom-cont-init.d 钩子（02）：把容器环境里的应用类型写入数据卷 /config/.woc-app，
# 供 autostart（以 abc 身份的桌面会话）读取。由 s6 在 autostart 之前以 root 运行。
# 缺 WOC_APP_TYPE（老实例/老面板）则不写文件 → autostart 回退微信，完全向后兼容。
APP_TYPE="${WOC_APP_TYPE:-}"
[ -z "$APP_TYPE" ] && exit 0

# 仅允许已知的简单标识，杜绝写入异常内容
case "$APP_TYPE" in
  wechat | telegram | chromium | custom) ;;
  *) exit 0 ;;
esac

TMP=/config/.woc-app.tmp
{
  echo "WOC_APP_TYPE='${APP_TYPE}'"
  # 自定义应用的启动命令由面板经环境传入（admin 设定）；用单引号包裹，转义内部单引号
  if [ -n "${WOC_CUSTOM_LAUNCH:-}" ]; then
    esc=${WOC_CUSTOM_LAUNCH//\'/\'\\\'\'}
    echo "WOC_CUSTOM_LAUNCH='${esc}'"
  fi
} > "$TMP"
mv -f "$TMP" /config/.woc-app
chown abc:abc /config/.woc-app 2>/dev/null || true
echo "[woc-app] 实例应用类型 = ${APP_TYPE}"
