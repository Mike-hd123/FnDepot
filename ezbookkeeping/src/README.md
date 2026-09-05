EZBookkeeping fnOS 打包说明（fpk-src 构建模板）
================================================

上级项目: mayswind/ezbookkeeping (https://github.com/mayswind/ezbookkeeping)
打包发布者: Mike (https://github.com/Mike-hd123)
上游版本: v1.6.1 (官方 linux-amd64 release 原样打包, 不重编译)

目录结构
--------
build.sh          # 一键构建脚本（从本模板组装 fpk 并调 fnpack）
manifest          # fpk 元信息（9router 标准极简字段, service_port=8580）
config/privilege  # run-as: package（进程管理用 PID 文件，不需要 root）
wizard/install    # 安装向导（MIT 协议展示）
app/ui/config     # 桌面入口（标准 iframe, 端口 8580, allUsers）
app/ui/images/    # 桌面图标（上游 ezbookkeeping-512.png 缩放: icon-64/128/256）
cmd/              # fnOS 生命周期九件套（main + 8 个 init/callback）
server/           # 【构建时生成】官方二进制 + public/ + conf/ + templates/ + storage/
                  # 构建后 conf/ezbookkeeping.ini 会打上占位符:
                  #   http_port=8580 / secret_key=@SECRET_KEY@ / db_path=@DB_PATH@
                  #   log_path=@LOG_PATH@ / local_filesystem_path=@STORAGE_PATH@
                  # 首次启动由 cmd/main 换成真实值并随机生成 secret_key
                  #   （数据目录钉死 /volX/@appdata/ezbookkeeping, 升级永不换目录）

设计要点
--------
- 原生应用（非 Docker），Go 静态单二进制零依赖，无 systemd（cmd/main PID 文件管理）
- 端口 8580（避开 8080 云微 / 8081 octopus / 20128 9router）
- MCP 服务默认关闭（enable_mcp=false），用户可在数据目录配置里打开
- 数据目录 = TRIM_PKGVAR（fnOS 注入, 即 /volX/@appdata/ezbookkeeping）
- SQLite 数据库位于 <数据目录>/data/ezbookkeeping.db
- gateway sidecar（app/gateway/gateway_proxy.py, 随 app.tgz 落 APPDEST/gateway/）：
  fnOS 统一网关 socket 自注册。入站 /app/ezbookkeeping/* 剥前缀后转后端 8580，
  无尾斜杠精确前缀 301 到带斜杠（前端相对资源 ./js/... 才能正确解析）。
  v4 及以前是裸 TCP 盲转发 → 手机端 /app/ezbookkeeping 报 100001 api not found；
  v5 改为 HTTP 层前缀代理（对齐 EasyTier gateway-proxy -prefix 语义）。

构建
----
./build.sh 1.6.1 5 x86     # VERSION REL ARCH
产物: ezbookkeeping-1.6.1-5-x86.fpk
