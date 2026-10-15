EZBookkeeping fnOS 打包说明（fpk-src 构建模板）
================================================

上级项目: mayswind/ezbookkeeping (https://github.com/mayswind/ezbookkeeping)
打包发布者: Mike (https://github.com/Mike-hd123)
上游版本: v2.0.0 (官方 linux-amd64 release 原样打包, 不重编译)

目录结构
--------
build.sh          # 一键构建脚本（从本模板组装 fpk 并调 fnpack）
manifest          # fpk 元信息（9router 标准极简字段, service_port=8580）
config/privilege  # run-as: package（进程管理用 PID 文件，不需要 root）
config/resource   # data-share 声明（share 名 ezbookkeeping；无独立用户故不带 permission）
wizard/install    # 安装向导（MIT 协议展示）
wizard/uninstall  # 卸载向导（保留/清除数据双选, 字段 wizard_delete_data）
app/ui/config     # 桌面入口（标准 iframe, 端口 8580, allUsers）
app/ui/images/    # 桌面图标（上游 ezbookkeeping-512.png 缩放: icon-64/128/256）
cmd/              # fnOS 生命周期九件套（main + 8 个 init/callback）
server/           # 【构建时生成】官方二进制 + public/ + conf/ + templates/ + storage/
                  # 构建后 conf/ezbookkeeping.ini 会打上占位符:
                  #   http_port=8580 / secret_key=@SECRET_KEY@ / db_path=@DB_PATH@
                  #   log_path=@LOG_PATH@ / local_filesystem_path=@STORAGE_PATH@
                  # 首次启动由 cmd/main 换成真实值并随机生成 secret_key
                  #   （包内 ini 只是模板；运行时真身在 share/ezbookkeeping.ini）

设计要点
--------
- 原生应用（非 Docker），Go 静态单二进制零依赖，无 systemd（cmd/main PID 文件管理）
- 端口 8580（避开 8080 云微 / 8081 octopus / 20128 9router）
- MCP 服务默认关闭（enable_mcp=false），用户可在数据目录配置里打开
- **数据与配置落点（2.0.1-2 share 合规修复）**：真身在 data-share「ezbookkeeping」
  （文件管理器 → 应用文件，软链 /var/apps/ezbookkeeping/shares/ezbookkeeping）：
  `data/ezbookkeeping.db`、`storage/`（账单附件）、`log/`、根下 `ezbookkeeping.ini`。
  `@appdata`（TRIM_PKGVAR）只留 boot.log / PID / gateway.log 等运行态文件。
  升级替换 @appcenter 不再影响数据；`config/resource` 声明 data-share 后由 fnOS 自动建目录与授 ACL。
- **幂等迁移**：install/upgrade_callback 只在「老 @appdata 位置有内容且 share 目标为空」时 `cp -a`，
  源目录不删，写 `<@appdata>/migrate.log`。已在 share 的数据不会被重复搬运或反向覆盖。
- **卸载双选**：`wizard/uninstall` 提供保留/清除（默认保留）。清除分支先停进程，
  再 `find <share> -mindepth 1 -maxdepth 1 -exec rm -rf {} +` 清空 share 内容（含隐藏文件）与 @appdata。
- **失败可见**：install/upgrade/main 的致命错误写 `TRIM_TEMP_LOGFILE`，应用中心弹窗可见。
- SQLite 数据库位于 <share>/data/ezbookkeeping.db
- gateway sidecar（app/gateway/gateway_proxy.py, 随 app.tgz 落 APPDEST/gateway/）：
  fnOS 统一网关 socket 自注册。入站 /app/ezbookkeeping/* 剥前缀后转后端 8580，
  无尾斜杠精确前缀 301 到带斜杠（前端相对资源 ./js/... 才能正确解析）。
  v4 及以前是裸 TCP 盲转发 → 手机端 /app/ezbookkeeping 报 100001 api not found；
  v5 改为 HTTP 层前缀代理（对齐 EasyTier gateway-proxy -prefix 语义）。
  v7/v8/v9 增加 SW 自杀、静态资源 gzip、缓存分流。
  v12 增加 desktop/mobile.html 静态兜底（EZ 后端 Gin 只 serve /，UA 跳转器
  location.replace 到这两个页面在内置服务器场景会 404 100001，sidecar 直接回
  APPDEST/server/public/ 下的文件）+ 后端 gzip 单层保护（后端 enable_gzip=true
  后，先解压再按需重压，防 gzip 双层白屏）。
- ui/config 为 socket 型（gatewayPrefix=/app/ezbookkeeping + gatewaySocket=app.sock），
  桌面/手机图标走 /app/ezbookkeeping 经 sidecar 反代（剥前缀+gzip+缓存分流），
  端口 8580 仅作为后端监听与桌面直连兜底。
- install_callback 幂等兜底 enable_gzip=true（升级不覆盖旧配置的老问题）。

构建
----
./build.sh 2.0.1 2 x86     # VERSION REL ARCH
产物: ezbookkeeping-2.0.1-2-x86.fpk
