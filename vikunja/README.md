# Vikunja — 待办管理面板（v2.7.0 · x86）

飞牛 fnOS 原生应用封装：自托管待办，Go 单二进制**静态链接**（零 GLIBC 依赖），SQLite 存储。

## 上级项目

- **上游**: [go-vikunja/vikunja](https://github.com/go-vikunja/vikunja) v2.7.0（Vikunja 官方 Go 重写）
- **发布者**: Mike（fnOS 打包发行，非上游开发者）
- 上游 release 资产 `vikunja-v2.7.0-linux-amd64-full.zip`，解压得静态 ELF `vikunja`（65.7MB，实测 `not a dynamic executable`）

## 功能特性

- **单二进制 + SQLite**：59MB 静态 ELF，数据单文件 `.backup` 一行全量备份；常驻 ~96MB
- **中文 UI**：自带完整 SPA 前端（打进二进制），列表/看板/甘特/表格/日历
- **API token**：`tk_` 全自动读写（Hermes 用独立 token 增/查/改/删）
- **提醒引擎**：reminder（due 前 N 天/时）+ 逾期 Webhook 事件白名单
- **CalDAV**：可被手机日历/待办 App 订阅

## 打包形态

```ini
appname               = vikunja
version               = 2.7.0
install_type          = volume      # 数据落 /volN/@appdata（用户数据）
service_port          = 3456
maintainer            = kolaente    # 上游 owner
distributor           = Mike
```

- **原生 systemd 模式**（抄 octopus）：cmd/main 管理 systemd unit，`User=hermes-studio`
- **双通道入口**：桌面 iframe 直连 `3456`（协议相对 `://${host}:<PORT>/`）+ 手机 `/app/vikunja` 走 gateway socket 剥前缀反代（`app/gateway/gateway_proxy.py`）
- **数据落 @appdata**：`/volN/@appdata/vikunja/`（vikunja.db + files/ + config.yml），卸载保留
- **注册已关**：`enableregistration: false`，管理员 `user create` 建号，Hermes 用独立 token

## 首次安装后配置（管理员）

```bash
# 管理员建号（注册已关）
sudo -u hermes-studio /volN/@appdata/vikunja/vikunja user create --config /volN/@appdata/vikunja/config.yml -u mike -e mike@qq.com -p '<pwd>'
sudo -u hermes-studio /volN/@appdata/vikunja/vikunja user set-admin --config /volN/@appdata/vikunja/config.yml mike --admin

# Hermes API token（最小权限：tasks 读写 + projects 读）
# POST /api/v1/tokens → 存 .env VIKUNJA_API_TOKEN
```

## 构建

`src/` 含打包关键文件（manifest / ui-config / gateway_proxy.py）。完整 build 流程：

```bash
# 1. 下载官方 release 并验 sha256（zip 内自带 .sha256）
# 2. 二进制备份到 project/app/vikunja
# 3. ui/config + gateway + 图标
# 4. fnpack build
export PATH=/vol1/@appcenter/nodejs_v24/bin:$PATH
cd project && fnpack build -d .
```

## 与本机 Hermes 集成（2026-09-06 实证）

- `VIKUNJA_API_TOKEN` 存 `hermes-home/.env`（不入代码）
- `scripts/vikunja_client.py`：建任务/标 done/查逾期（自然语言日期解析）
- `scripts/festival_seed_vikunja.py`：农历/节气 sxtwl 转公历建年度任务（每年 11-01 cron）
- `scripts/vikunja_notify.py`：节律推送（09:00 到期 / 14:00+19:00 逾期追缴）
- 4 条 cron 接入 default jobs.json，deliver weixin

## 验证记录

- 静态 ELF sha256 与官方 .sha256 一致
- `/api/v1/info` 200，`/` 完整 SPA
- 建号/建项目/建任务/设 reminder/查 filter/标 done/建 tk_/建 webhook 全 200~201
- Webhook 目标**必须公网 IP**：Vikunja 拒所有私有网段（127.0.0.0/8、192.168.0.0/16）实测
## v9 变更（2026-09-07）

- **修复**：网关路径 `/app/vikunja` 下 vue-router history base 硬编码 `/`，站内跳转产生裸路径（地址栏丢前缀），刷新掉回 fnOS 根页面。v8 的 SPA shim（history.replaceState 剥前缀）方案废弃——路由器会采纳剥过的路径且不回写。
- **新方案**：sidecar 对带前缀的 JS 流量动态改写 router 工厂 base（`history:<fn>(\`/\`)` → `history:<fn>(\`/app/vikunja/\`)`，锚点唯一），桌面直连（无前缀）流量原样透传。
- **sidecar 运行形态**：nohup 裸跑 → systemd 单元 `vikunja-sidecar`（BindsTo=vikunja，开机自启/崩溃自拉，ExecStartPre 清理 v8 遗留 gateway.pid）。
- **desktop_applaunchname** 对齐 `vikunja.panel`（fnpack 强校验 ui/config 入口名一致）。
- **安装脚本卷探测**：install_callback 可能不带 TRIM_* env 被调用，APPDEST/DATA_DIR 缺失时扫描 /vol1-4 实际落点，不写死卷。
- 实测：安装器原生写入 `trim_sac.entry.micro_app=t` + `appcenter.app.micro_app=true`（v8 首装未触发，v9 正常）。

## v10 变更（2026-09-07，t_de3a27dc 实证）

- **根因**：vite 动态 import chunk 的 base 工厂硬编码根绝对路径 `` return`/`+e ``（preload-helper 把懒加载 chunk/样式拼成 `http://<host>/assets/...`），经网关丢 `/app/vikunja` 前缀 → 全 404 → `Unable to preload CSS` 中断启动 → SPA 卡“Vikunja 正在加载…”/部分加载失败。
- **修复（sidecar 网关流量三层补丁，桌面直连零影响）**：
  1. **JS preload base**：锚点 `` return`/`+e `` → `` return`../`+e ``（模块相对，网关下解析 `/app/vikunja/assets/`，桌面直连解析 `/assets/`）。
  2. **CSS 绝对资源**：`url(/assets/...)` → `url(/app/vikunja/assets/...)`（字体/背景图，静态文件网关 301 会掉前缀）。
  3. 保留 v9 的 router base 改写。
- **验证（CDP 真实登录 + JWT 注入，全部通过）**：首页海报 200→首页渲染（项目/任务/概览）→ SPA 深路由 `/tasks/3` 详情→ 编辑→ Tiptap 富文本编辑器（标题/粗体/斜体/表格工具栏）→ 深路由硬刷新不掉根、零 404、零 JS 异常。直连 3456 桌面模式 preload 仍 `return`/`+e`、CSS 未打前缀。
- **skill 已加**：vite 动态 chunk 网关前缀丢失专节 + JWT HS256 会话自签登录法（见 fnos-app-package）。

## v12 变更（2026-09-09，t_68437863 —— 失败实验，回退）

- 尝试通过 manifest 改 `micro_app=true→false` + 删 sidecar 单元 + 移除 cmd_main 里的 sidecar 启动逻辑，让 fnOS 自动生成端口模式 entry。
- **实测结论**：装完 `trim_sac.entry.url` 仍是 `{"path":"/app/vikunja"}` + `gateway_socket=/var/apps/vikunja/target/app.sock`（指向已删的 sidecar → 反代 502 Bad Gateway）。
- **v12 假设错误的地方**：以为改 manifest 字段（micro_app/service_port）能影响 entry 生成。实测这些字段与 URL 模式无关。
- **代码改动**：仅 v12 fpk 内（scratch 临时改，未落 FnDepot 源码），本 v13 直接跳过此尝试。

## v13 变更（2026-09-09，t_c99a3b7a）

**真正的根因（v13 逆向 + 实测确认）**：

1. entry.url 走「路径反代」还是「端口直连」，决定字段是 **`target/ui/config` 文件里的 `gatewaySocket` / `gatewayPrefix`**。appcenter 首次安装时读它，写进 `trim_sac.entry` 表。
   - 对比实测：`hermes-studio`、`wechat-on-cloud` 的 `ui/` 里**没有 config 文件**（或无 gatewaySocket 字段）→ entry 天生端口模式；`hyatlas`、`vikunja` 的 `ui/config` 里有 `gatewaySocket:"app.sock"` → entry 路径反代。
   - `micro_app` / `service_port` / `desktop_uidir` 等 manifest 字段**与 URL 模式无关**（v12 的死路由此解释）。
2. **但**：appcenter 只在**首次安装**时读 ui/config 一次。之后 `appcenter-cli start/stop` 会用它**内部缓存**（entry 表 + appcenter 状态）重生成 entry——即便 ui/config 已改、SQL 已 UPDATE，`appcenter-cli start` 也会把 entry 覆盖回路径模式。**这就是为什么单纯装完改 SQL 不持久。**
3. **唯一稳定保持端口模式的组合拳**：
   - 立即 `psql UPDATE` 把 `entry.url` 改成 `{"port":"3456","path":"/"}` + `gateway_socket=''` + `gateway_prefix=''`
   - **延时 30s 再 UPDATE 一次**（覆盖 appcenter 在 callback 完成后自动 `appcenter-cli start` 触发的回滚）
   - 重启 `trim_http_cgi` + `trim_open_gateway` 让反代/网关缓存失效
   - **callback 内绝不主动 `appcenter-cli start`**（那只会触发回滚）

**v13 实现**（`src/install_callback` 末尾 v13 段落）：

- **a) 改 ui/config**：重写 `${APP_DIR}/ui/config`，去掉 `gatewaySocket` / `gatewayPrefix`，把 `url` 改成端口根路径 `"/"` + `port:3456`。防御性——若日后走全新安装流程，appcenter 首次读到的就是端口模式，不再需要 SQL 兜底。
- **b) 立即 SQL UPDATE** + **c) 后台 `( sleep 30; UPDATE ) &` 延时 UPDATE** 两次，用 `_update_entry_url` 函数（幂等，按 `app_name='vikunja' AND service_name='vikunja.panel'` 定位，不硬编码 id）。
- **d) 三服务重启**：仅 `trim_http_cgi` + `trim_open_gateway`（应用本体不重启，避免 appcenter 覆盖）。
- **降级**：`psql` / PostgreSQL socket 缺失 → 打印 `[WARN]` 不中断安装，ui/config 改写仍生效，用户可后续手动 SQL 改。

**实测验证（本任务会话内）**：
- 恢复路径模式 → v13 立即 UPDATE → `entry.url = {"port":"3456","path":"/"}` ✅
- 触发 `appcenter-cli start` 模拟回滚 → entry 被打回路径模式（证实回滚机制）→ 延时 UPDATE 救回端口模式 ✅
- 双端连通：直连 `http://192.168.5.2:3456/` = **200**，反代 `http://192.168.5.2/app/vikunja/` = **200**（sidecar inactive 也能通——网关按 app_name 直接路由到 3456，不依赖 socket）✅
- WS 握手：直连 3456 无 nginx `safe_code_access.conf` 的 426 拦截 → 前端 WS 不再断 ✅

**manifest**：`version=2.6.0-13`，`micro_app=true` / `service_port=3456` 保持不变（cmd_main 里 sidecar 启动逻辑保留作为 socket 兜底，inactive 也不影响端口直连）。手机端 `/app/vikunja` 反代路径继续可用（网关 app_name 路由，不依赖 sidecar）。

**产物**：`vikunja-2.6.0-13-x86.fpk`（sha256 `1a8c2ea270ce8d8d173c120659580044f27e519a0f89794f9d181d2e1bcad1d9`，45885108 B，staging 打包，app.tgz/ICON 等 v11 复用——本次仅改 install_callback + manifest version）。

**参考**：`skills/fnos-app-admin/references/entry-direct-port-migration.md`——该文档「从 manifest 预测 URL 模式 = 死路」结论方向正确，但「唯一可靠路径 = SQL + 三服务重启」不完整（缺延时 UPDATE 对抗 appcenter 自动 start 回滚这一步）。v13 实测补充见上。

## v2.7.0-1 变更（2026-10-04，同步上游 v2.7.0）

**上游**：v2.6.0 → v2.7.0（2026-10-02 发布，858 commits：317 fixes / 62 features / 58 依赖升级）。要点：6 个安全修复（登录 cookie 跨站与会话劫持面、已吊销会话仍收 live 推送、被移除协作者的 webhook 残留、子项目只读共享被忽略(2.6.0 回归)、写权限成员可删 admin link share、saved filter 可被撑爆库）；新增 MCP server（AI 助手可直连 Vikunja）、新 date picker、**前端整体切 v2 API**（旧 v1 客户端代码移除，v1 服务端仍在但建议迁移）、出网 HTTP(S)_PROXY 支持；性能 P99 260ms→22ms（Postgres 连接复用查询计划 / 连接寿命 10s→30min / 嵌套项目权限查询改查表 / 只读请求不占连接 / API token 校验近零成本）。

**数据库迁移（12 个新增 migration，单向不可降级）**：

| migration | 行为 | sqlite 影响 |
|---|---|---|
| 20260901001942 | 新表 `project_task_counters` + 按项目回填最大 task index | 增表+回填，批 500 防 32766 绑参上限 |
| 20260901220330 | 新表 `task_index_aliases` | 增表 |
| 20260903104500 | 根项目 `parent_project_id` 0→NULL + 部分索引 | 数据改写（语义：根=NULL） |
| 20260903120000 | tasks 索引 `(done,due_date)` → `(project_id,done,due_date)` | DROP/CREATE INDEX（幂等吸收重跑） |
| 20260906010000 | projects.parent_project_id 部分索引（非 MySQL） | 增索引 |
| 20260906180128 | **api_tokens 整表重建**：rename→Sync(加 token_sha256)→copy→drop，事务内 | 旧 token 行保留，token_hash/token_salt/token_last_eight 改 nullable；新行写 token_sha256 |
| 20260908202544 | 新表 `project_ancestors`（祖先后代闭包）+ 回填 | 增表+回填（嵌套权限 O(1) 查表） |
| 20260911150507 | 新表 `user_invite_links` / `user_invite_link_teams` | 增表（邀请链接功能） |
| 20260911193534 | migration_status 加 `heartbeat_at` | 增列 |
| 20260911193552 | migration_status 加 `error_kind` / `error_message` | 增列 |
| 20260914185746 | migration_status 加 `upload_file_id` | 增列（后台导入队列） |
| 20260928140648 | `DELETE FROM label_tasks WHERE label_id NOT IN (SELECT id FROM labels)` | 删孤儿行（不可逆，仅清理） |

**净室实测（/tmp 独立 sqlite，官方 2.6.0 二进制造库→播种 17 任务+1 tk_ token→官方 2.7.0 二进制就地升级）**：12 个迁移全部 `Ran all migrations successfully`、零 error/panic；表计数前后一致（users=1 projects=4 tasks=17 api_tokens=1）；**api_tokens 整表重建后既有 tk_ token 在 `/api/v1/tasks` 与 `/api/v2/tasks` 两条路由均鉴权 200**（Hermes 集成安全）；root 项目 parent_project_id 0→NULL 生效；tasks 索引 swap 生效；project_task_counters/project_ancestors 回填完成。

**本地打包定制存活清单（逐条实测，零改动）**：sidecar `gateway_proxy.py`（v9/v10）四个改写锚点在 2.7.0 前端产物中全部命中——router base `history:ai(\`/\`)`、vite preload `return\`/\`+e`、CSS `url(/assets/`（9 处）、HTML `window.API_URL` 注入 + 55 处绝对引用加前缀；起真实网关反代打流量，patch 计数日志全部命中（router×1 / preload×1 / css×9），经前缀的 `/app/vikunja/api/v2/tasks` + tk_ 鉴权返回 200。关键兼容点：新前端 base 推导函数 `d()` 用 `/\/api\/v[12]$/` 同时剥 v1/v2 后缀，故 sidecar 注入的 `window.API_URL='/app/vikunja/api/v1'` 仍被正确解析为 `/app/vikunja/api/v2`，无需改 sidecar。

**二进制**：上游官方 `vikunja-v2.7.0-linux-amd64-full.zip` 原样（zip sha256 `569861fd…`、内层 `.sha256` 校验 ELF `9745ad79…`、`file` 报 statically linked），未重编译。包内二进制逐字节等于上游官方。

**产物**：`vikunja-2.7.0-1-x86.fpk`（48926361 B / 46 MB，sha256 `a133fdfc7c00826483b81832b9e7868813ea8acac4350849b7c84827e4f18455`，md5 `1eef0d346877cd52866b912b80d87c53`），落 `/vol2/1000/download/`。

**版本命名依据**：包版本沿用「上游版本-本地修订号」惯例，跨上游大版本修订号归 1（2.6.0 系打到第 14 次重打包 → 2.7.0 系第 1 次 = `2.7.0-1`），与 ezbookkeeping 2.0.1-1 / octopus 0.13.9-1 一致。

**升级建议**：升级前先 `sqlite3 /vol2/@appdata/vikunja/vikunja.db ".backup '/vol2/@appdata/vikunja/vikunja.db.bak_$(date +%Y%m%d_%H%M%S)'"` 全量备份（迁移单向、含数据改写与孤儿删除）。升级即换包安装，新二进制首启自动跑迁移，无需手动操作。

**待实测验证点**：① 生产库升级后 `/api/v1/info` 返回 v2.7.0、任务数=22 不变；② Hermes 提醒引擎用 tk_ token 增删改查正常；③ 桌面直连 :3456 与手机 `/app/vikunja` 双通道渲染无 404；④ CalDAV 订阅仍可用；⑤ 首启 boot.log 无 migration error。

**⚠️ 历史包错版提示**：GitHub Release `v2026.09.23` 上名为 `vikunja-2.6.0-14-x86.fpk` 的资产经解剖，内层 manifest 实写 `version = 2.6.0`、ui/config 仍是旧 `vikunja.Application` + gatewaySocket 路径反代、cmd 脚本零 v13/v14 痕迹、gateway_proxy.py 为 v8 版（15801B vs 仓库 v10 版 15978B）——即该发布资产是 v8 时代构建的错版，与仓库 src/（v13/v14 权威）及 fnpack.json 记录的 sha 不自洽。本次 2.7.0-1 以仓库 src/ 为权威基线重建，已纠正此偏差；待 push 时需把 `v2026.09.23` 上的错版 `vikunja-2.6.0-14-x86.fpk` 一并替换/下架。
