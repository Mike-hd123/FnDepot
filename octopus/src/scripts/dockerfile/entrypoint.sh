#!/bin/sh
set -e

PUID="${PUID:-0}" # 容器进程使用的用户 ID。
PGID="${PGID:-0}" # 容器进程使用的用户组 ID。

chmod +x "/app/${APP_NAME}"
chown -R "$PUID:$PGID" /app
cd /app
exec su-exec "$PUID:$PGID" "./${APP_NAME}" start
