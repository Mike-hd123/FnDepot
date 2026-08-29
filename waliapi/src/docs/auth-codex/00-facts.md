# Auth / Codex 登录：事实底座（Facts）

> 本文件是 grill-with-docs 会话的事实记录。**决策**记录在 [ADRs.md](ADRs.md)，术语在 [glossary.md](glossary.md)，**UI 设计规格**在 [01-ui-spec.md](01-ui-spec.md)，静态原型在 `prototype.html`。
> 事实 = 我从代码/外部源查到的东西，不是用户决策。

## 1. 现状架构（代码事实）

- **前端**：React + react-router-dom v7。侧边栏 `src/components/layout/Sidebar.tsx:22-30`：
  `仪表盘 / 使用(API、Codex...) / 渠道 / 密钥 / 服务(RAG、Wiki、Skills) / 日志 / 设置`。
  渠道路由 `/channels` → `ChannelsPage`（`src/App.tsx:48`），页面标题「渠道管理」，副标题「拖拽排序 · 配置上游供应商与调度优先级」。
- **通信**：Tauri `invoke`（无 HTTP 前端层）。`src/lib/api.ts` 的 `channelApi` / `apiKeyApi` / `importExportApi` 都是薄封装。
- **渠道（Channel）**：上游供应商配置，`channels.api_key` 存**上游厂商凭证**（SQLite 明文 TEXT），
  `AuthScheme`（Bearer / x-api-key / query / optional_bearer）决定凭据怎么放（`src-tauri/src/endpoint_executor/mod.rs:139-166`）。
  新增协议身份字段：`protocol / provider / native_base_url / native_endpoints / preset_revision / identity_revision`。
- **密钥（ApiKey）**：**网关自己发给下游客户端的凭证**，与上游渠道无直接关系（`api_keys` 表）。
- **当前没有任何用户登录 / OAuth / 会话概念**。全库搜 `login|oauth|signin|account|logout` 前端后端均为 0 命中。
- **现有 Codex 关联**：
  - `protocol/` 下有 Responses ↔ Chat ↔ Anthropic 的流式转换桥（`sse_bridge.rs`），Codex 走 wire 层已支持。
  - `commands/app_config.rs:363-428` `write_codex()`：往 `~/.codex/config.toml` 写 `waliapi` provider（`experimental_bearer_token` 指向网关）。
  - `commands/import_export.rs:586-686` `scan_local_ai_configs`：读 `~/.codex/config.{json,toml}`（**不读 auth.json**）。
  - App 从不 spawn `codex` / `claude` 二进制。

## 2. Codex CLI 登录方式（外部源事实）

来自 [OpenAI codex 官方文档](https://learn.chatgpt.com/docs/auth) 与 [openai/codex 源码](https://github.com/openai/codex)：

- **命令**：`codex login`（默认浏览器流）→ `codex login status` → `codex logout`。
  备选：`codex login --device-auth`（设备码）、`--with-api-key`、`--with-access-token`。
- **OAuth 流程**：默认**浏览器流 + localhost 回调**（`localhost:1455`）；使用 **PKCE S256**（`codex-rs/login/src/pkce.rs`）。
  `client_id = "app_EMoamEEZ73f0CkXaXp7hrann"`；刷新端点 `https://auth.openai.com/oauth/token`；吊销端点 `https://auth.openai.com/oauth/revoke`。
- **令牌存储**：`~/.codex/auth.json`（明文 JSON），或 OS keyring（`cli_auth_credentials_store = file | keyring | auto`）。
  auth.json 结构（`codex-rs/login/src/token_data.rs` `TokenData`）：
  ```json
  { "id_token": { ... }, "access_token": "<JWT>", "refresh_token": "<JWT>", "account_id": "<id>" }
  ```
  id_token 含 `email`、`chatgpt_plan_type`（free/plus/pro/...）、`chatgpt_user_id` 等。
- **刷新**：Codex 会在使用中自动刷新令牌。auth.json 是**单账号**（active account 唯一）。
- **令牌用途**：`access_token` 作为 `Authorization: Bearer <token>` 访问 ChatGPT 后端（chatgpt.com backend-api），
  即「用 ChatGPT 订阅走官方 Web 后端」，不是 OpenAI 平台 API key。

## 3. 用户需求（原话）

> 「当前应用需要添加一个 codex auth 登录的功能，就在渠道管理的，把现在的渠道管理改为 API tab，新增要给 Auth tab。
> Auth 支持 codex、claude、kiro、kimi 等账号登录，首次先支持 codex」

- 位置：渠道管理区域。
- 渠道管理页面 → 拆成 **API** tab（现状内容）+ **Auth** tab（新增）。
- Auth 未来支持多个账号源（codex / claude / kiro / kimi），**首个实现 = codex**。

## 4. Codex ChatGPT 账号的 backend-api 端点（事实）

来自 [openai/codex 源码](https://github.com/openai/codex)（用户提示「codex 原生支持两个端点也要考虑到」）：

- **Base URL**：`https://chatgpt.com/backend-api/codex`（`codex-rs/model-provider-info/src/lib.rs:37` `CHATGPT_CODEX_BASE_URL`；auth 模式为 Chatgpt/ChatgptAuthTokens/Headers/AgentIdentity/PersonalAccessToken 时使用）。
- **端点 1 — 模型推理**：`POST {base}/responses`（Responses wire API，`codex-rs/codex-api/src/endpoint/responses.rs:100-102` path = `"responses"`）。流式事件链即现有 `protocol/responses.rs` 支持的那套。
- **端点 2 — 模型列表**：`GET {base}/models`（`codex-rs/model-provider/src/models_endpoint.rs:39` `MODELS_ENDPOINT = "/models"`）。**模型列表由上游 `/models` 实时返回，非本地硬编码**。
- **Wire**：默认 `wire_api = "responses"`；`chat` wire 已废弃（报错指向 discussion #7782）。
- **鉴权头**：`Authorization: Bearer <access_token>`；另有 `x-openai-actor-authorization`（托管 client 用，`model_provider_info.rs:35`）、可选 `x-openai-session-id` / `x-openai-thread-id` / `x-openai-subagent`、`oai-product-sku`。
- **客户端压缩**：支持 zstd（RequestCompression::Zstd）。

## 5. CPA（CLIProxyAPI）如何存储 auth（调研事实）

来源：[router-for-me/CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI)，用户要求「看一下 CPA 是如何存储的，因为要基于 codex 设计的表不通用」。

### 5.1 核心：通用 Auth 记录 + 每 provider 的 TokenStorage 载荷

- **统一记录** `coreauth.Auth`（`sdk/cliproxy/auth/types.go:47-101`）：ID、`Provider`（如 "codex"/"claude"/"kimi"/"antigravity"/"xai"/"vertex"）、`Label`、`Status`、`Disabled`、`Unavailable`、`Attributes map[string]string`、`Metadata map[string]any`、`Quota QuotaState`、`LastError`、`ModelStates map[string]*ModelState`、刷新时间戳、`Storage baseauth.TokenStorage`（**provider 专用令牌载荷**）。
- **持久化 = 磁盘 JSON 文件**（每 auth 一个），不是通用数据库表。`FileTokenStore.Save`（`sdk/auth/filestore.go:76`）：优先调 `auth.Storage.SaveTokenToFile(path)` 由 provider 的 TokenStorage 序列化自己的结构；否则把 `Metadata` 写成 JSON。`os.WriteFile(0o600)`。
- **每 provider 令牌结构不同**：`codex.CodexTokenStorage`（`internal/auth/codex/token.go:18-39`）字段 `id_token/access_token/refresh_token/account_id/last_refresh/email/type/expired` + `Metadata map[string]any`。claude/kimi/antigravity 各有一份。
- **共享元数据**：`type`（provider）、`email`、`project_id`、`priority`、`weight`（`AttributeWeight`）、`note`、`websockets` 等由 `gjson` 从文件头读取做列表展示（`auth_files.go:247-289`）。
- **泛化理由**：CPA 用 `Auth`（通用）+ `TokenStorage`（provider 特有）两层——**表/文件结构不绑死任何 provider 的字段**。这正是用户「基于 codex 设计的表不通用」想避免的。

### 5.2 限额/降级路由（用户第二点：限额与账号退出）

- **QuotaState**（`types.go:168-177`）：`Exceeded` / `Reason` / `NextRecoverAt` / `BackoffLevel`（指数退避 `quotaBackoffBase * 2^level`）。
- **ModelState**（`types.go:180-195`）：每模型执行状态 `Status` / `Unavailable` / `NextRetryAfter` / `LastError` / `Quota`。
- **触发**：HTTP 429 → `Quota.Exceeded=true, Reason="quota"`，`NextRecoverAt` 取 `Retry-After` 头或指数退避；404 冷 12h；408/500/502/503/504 冷临时错（`conductor_cooldown.go:1775-1806`）。
- **恢复**：`ResetQuota`（`conductor_cooldown.go:425`）清空该 auth 的 cooldown/quota；cooldown 状态独立持久化到 `.cds` 文件（`FileCooldownStateStore`），与 auth 令牌文件分离。
- **按 provider 限额窗口（free=月、非 free=周）**：codex 实际**没有 5 小时限额**——free 号实测返回 30 天月窗口（`primary.window_minutes=43200`），非 free 常见周窗口。CPA 没有写死专用窗口字段；它把限额当作**运行时 cooldown/quota 状态**（429 触发、Retry-After/退避恢复），通用化到所有 auth。CPA 的做法是「不特判，触发时标记 + 自动恢复」。WaLiAPI 对齐：窗口类型按 `window_minutes` 动态推导，不硬编码 free/非 free。
- **自动刷新路由**：`auto_refresh_loop.go` 存在（后台刷新循环）；cooldown 过期后 auth 自动回到候选（`restoreCooldownRecordLocked`）。

### 5.3 CPA 删除 auth = 纯本地移除，无远端 revoke（调研事实，2026-08-09 复核源码）

- **删除链路**：TUI `y` 确认 → 管理 API `deleteAuthFileByName`（`internal/api/handlers/management/auth_files_crud.go:342-375`）→ `os.Remove` 本地 auth 文件 + `deleteTokenRecord`（store `Delete`）+ `removeAuthsForPath` 清运行时内存。
- **Store 层 `Delete`**（三处实现，动作一致）：`ObjectTokenStore` 删本地文件 + 删远端**对象存储**副本（`internal/store/objectstore.go:273-294`）；`PostgresStore` 删本地文件 + 删 DB 行（`postgresstore.go:366-388`）；`GitTokenStore` 删本地文件 + git commit/push「Delete auth …」（`gitstore.go:522-529`）。
- **「远端」≠ provider revoke**：上面的远端只指 CPA 自己存储后端（对象存储 / git 仓库 / DB），是删除持久化副本，**不调用 provider 的 `oauth/revoke` 端点**。
- **全仓库 `revoke` 检索**：仅 refresh 错误分支出现 `refresh_token_reused` 字样（`internal/runtime/executor/helps/home_refresh.go`），**无任何 `oauth/revoke` 调用**。删除动作不触碰 OpenAI/Claude 服务端会话。
- **结论**：CPA 的删除语义 = 本网关移除（持久化 + 内存），与用户「删除后此账号不再参与路由」一致；WaLiAPI v1 对齐此语义（ADR-38），弹窗只问「是否删除」。

## 6. codex 限额响应头（事实）

来自 [openai/codex `rate_limits.rs`](https://github.com/openai/codex/blob/main/codex-rs/codex-api/src/rate_limits.rs)（用户要求「看看每个请求里有没有返回限额信息，有的话就更新一下」）：

- **响应头携带限额**：codex 在**每个响应头**返回限额，格式 `x-<limit_id>-<primary|secondary>-...`：
  - `x-codex-primary-used-percent` / `x-codex-primary-window-minutes` / `x-codex-primary-reset-at` —— primary/secondary 是**两个平行的可选项**，窗口类型完全由 `window-minutes` 决定，不写死；实测 free 号 `primary.window_minutes=43200` = **30 天月限额**（非 free 常见周限额）；`secondary` 常为空。
  - 另有 `x-codex-limit-name`、credits 快照（`x-codex-credits-*` 是**布尔标志**，非数值）、`rate_limit_reached_type` 等。
- **多 limit_id**：header 名里可带不同 limit id（`codex` / `codex_other`），解析时遍历。
- **codex 自带解析器** `parse_rate_limit_for_limit` / `parse_all_rate_limits`（`rate_limits.rs:23-101`），返回 `RateLimitSnapshot { limit_id, limit_name, primary, secondary, credits, ... }`。可移植参考。
- **空窗口丢弃**：上游保留规则（`has_data`）= `used_percent != 0 || window_minutes != 0 || reset_at 存在`；只有 `used-percent: 0` 的窗口不构成限额，WaLiAPI 解析层镜像该规则丢弃（见 02-design §5.3）。
- **窗口标签**：WaLiAPI 前端只支持三种限额标签，按 `window_minutes` 近似匹配（±5%）→ **5H限额 / 周限额 / 月限额**；其它时长不标类型，显示裸「限额」，避免误导（曾因单位错误误标「12小时窗口」）。重置时间显示**具体时间点**（本地化月/日/时/分），而非相对时长；**上游未返回 `reset_at` 则不显示重置行**。
- **事件通道**：`codex.rate_limits` SSE 事件也携带 `used_percent / window_minutes / reset_at`。
- **专门限额端点（无流量权威源）**：`GET {backend-api}/wham/usage` 返回权威限额状态，字段为 `rate_limit.primary_window.{used_percent, limit_window_seconds, reset_at(UNIX秒)}` + `credits` + `spend_control`。实测：free 号 `limit_window_seconds=2592000`（30 天月限额）、plus 号 `604800`（7 天周限额）。WaLiAPI 在**模型同步后/刷新后/维护循环**主动探测并归一化写 `QuotaState`，失败静默保留旧值。
- 这就构成「有流量时解析响应头、无流量时主动探测 `wham/usage`」的完整更新链。

## 7. 本机 codex 登录态（实际 auth.json 结构 — 导入/写回的权威依据）

来自本机 `/Users/xian/.codex` 实查（codex v0.147.0，`CODEX_HOME` 未设置）：

- **`~/.codex/auth.json`**（`-rw-------` 600）实际结构：
  ```json
  {
    "auth_mode": "chatgpt",
    "OPENAI_API_KEY": null,
    "tokens": {
      "id_token": "<JWT>",
      "access_token": "<JWT>",
      "refresh_token": "<opaque, 非标准 JWT>",
      "account_id": "88c15db6-97b7-4f89-96c5-a523531b6677"
    },
    "last_refresh": "2026-08-09T07:57:08.210318Z"
  }
  ```
  ⚠️ **与 codex 源码 `TokenData` 平铺结构不同**：真实文件把 token 三件套嵌在 `tokens` 下，顶层另有 `auth_mode` / `OPENAI_API_KEY` / `last_refresh`。导入与写回必须以**真实文件形状**为准，不能照源码 TokenData 结构。
- **邮箱**只在 `id_token` JWT payload 里（`email` claim，Google 登录 `z***@gmail.com`），顶层无 email 字段 → 导入时从 id_token 解析 email 作显示名。
- **refresh_token** 是 opaque 字符串（非标准 JWT，中间段无法 base64 解码）→ 导入/写回一律按不透明字符串处理，不解析。
- **account_id** 是稳定 UUID → 导入去重的自然键（同 account_id 视为已存在账号，覆盖刷新而非新增）。
- **config.toml**（600）当前已被 `write_codex` 写入 waliapi provider（根级有 `experimental_bearer_token`，已脱敏），`auth_mode=chatgpt` 的 OAuth 登录态与 waliapi provider 并存。
- 其他文件：`version.json`、`.codex-global-state.json`（全局状态，非令牌存储）、`history.jsonl`（会话历史）、`config.toml.bak-*` 系列、`mcp-oauth-locks/`。无 `auth.json.bak`、无 `.cds`。

## 8. 设计树（grilling 进度）

> 注：下表为 grilling 期间决策快照。Q14/Q15 中的「30min 主动探测/刷新路由」已被**已拍板决策 D-C（延后）**取代（见 `work/00-optimized-requirements.md` §2.2 / §3.1）；Q10 的「12h 定时兜底 + 懒刷新 + 401 重试」保留。

| 决策 | 状态 |
| --- | --- |
| Q1 Auth 定位 | ✅ A 账号即上游（消耗订阅额度） |
| Q2 codex 登录执行方式 | ✅ A 内嵌 OAuth PKCE+localhost 回调，C 导入 auth.json 兜底 |
| Q3 令牌存储 | ✅ A WaLiAPI DB 新表 auth_accounts |
| Q4 前端拆分 | ✅ B 两个独立路由 /channels + /channels/auth |
| Q5 多账号 | ✅ B 同 provider 多账号并存 |
| Q6 账号型上游的路由集成形态 | ✅ A 路由层兼容 API 渠道 + Auth 账号 |
| Q7 backend-api 协议适配 | ✅ A 新增薄账号适配器（backend-api adapter）：/responses 推理 + /models 模型列表，复用现有 Responses 桥 |
| Q8 模型列表来源/暴露 | ✅ 登录时自动拉 + 每 12h 自动拉 + 手动刷新；只能看不能选；默认全部支持 |
| Q9 账号参与路由选择 | ✅ A 账号也是普通候选：同池进 RoutePlan（协议组→priority→weight） |
| Q10 令牌刷新/失效 | ✅ C 每 12h 定时刷新兜底 + 出站前主动刷新临近过期令牌（懒加载）+ 401 触发刷新重试一次 |
| Q11 同 provider 多账号路由选择 | ✅ A 每个账号是独立候选，复用 weight 无放回抽样 |
| Q12 provider 抽象深度 | ✅ A 抽象层一步到位：Provider trait（登录/刷新/出站），codex 为首个实现 |
| Q13 存储模型 | ✅ A 通用列 + payload_json（CPA 式两层结构）：通用列 + 每 provider JSON 载荷 |
| Q14 限额实现 | ✅ C 动态窗口（free=月、非 free=周，按 `window_minutes` 推导）+ 运行时 429/Retry-After 兜底 + 30min 刷新路由 + 响应动态解析 |
| Q15 限额探测频率 | ✅ 有流量：每次请求响应头解析即更新（无需定时）；空闲：每 30min 主动探测兜底 |
| Q16 限额退出粒度 | ✅ A 账号级：限额触发整个账号踢出路由，恢复时整体回来 |
| Q17 Auth tab UI | ✅ A 账号卡片列表（对齐 ApiKeysPage）；界面细节后续再设计，先核心功能 |
| Q18 回写 codex CLI | ✅ D 手动：账号卡片上提供「写回本地 Codex CLI」按钮，点击才写 ~/.codex/auth.json |
| Q19 与 UsagePage Codex 配置关系 | ✅ C 暂不联动：两者独立，用户手动切换 codex 用 WaLiAPI |
| Q20 Tauri 命令集 | ✅ auth_accounts_list / auth_login / auth_login_import / auth_logout（纯本地删除，无 revoke）/ auth_refresh_token / auth_sync_models / auth_write_back / auth_toggle / auth_quota_status / auth_update(编辑名称/P/W) |
| Q21 账号模型映射 | ✅ B v1 不支持 model_mapping：账号只透传上游 /models 模型名 |
| Q22 账号失败降级 | ✅ A 账号与普通渠道同等级降级：失败自动试下一个候选（账号或普通渠道） |
| Q23 登录冲突/覆盖 | ✅ B 同 account_id 覆盖刷新现有账号（保留 id/权重/优先级），不重复新增 |
| Q24 导入校验/联动 | ✅ B 导入校验字段齐全+未过期，提示本机 codex 仍登录该账号、令牌不自动同步 |
| Q25 失效呈现/恢复 | ✅ B 失效账号：后台定时自动尝试刷新恢复 + 前端"重新登录"引导 |
| Q26 账号权重/优先级 | ✅ A 登录默认 priority=0 weight=1，账号卡片可调（对齐渠道编辑） |
| Q27 账号重命名 | ✅ 账号卡片可重命名（label 通用列，前端内联编辑），默认取邮箱/推断名 |
| Q28 限额缺失 | ✅ 上游返回限额才显示/参与路由；无返回视为无限额，仅 429 cooldown 兜底（ADR-28） |
| Q29 风险提示 | ✅ Auth tab 顶部常驻警示条（订阅/OAuth 代理风险，固定文案，ADR-29） |
| Q30 请求日志区分 | ✅ A 新增 upstream_type 列（channel / auth_account），channel_id 填账号 id |
| Q31 账号服务下游协议 | ✅ B v1 账号服务全部下游（含 Chat）：新增 Responses→Chat codec（复用现有事件链/基础设施） |
| Q32 allowed_channels 约束账号 | ✅ A 账号不受 allowed_channels 约束：由 key 配额/权限 gate 兜底 |
| Q33 Responses 流式 usage | ✅ A 现在补：原生 Responses 直通/账号路径提取 response.completed.response.usage（input/output_tokens） |
| Q34 账号空模型语义 | ✅ B 空 models = 拒绝所有：同步成功前账号不参与路由（区别于渠道的空=通配） |
| Q35 401 刷新重试归属 | ✅ A 适配器内部静默重试：刷新成功重试同一账号，AttemptFlow 无感知；刷新失败才走下一候选 |
| Q36 stream:false 处理 | ✅ A 账号强制流式：非流式下游也按 stream:true 打 backend-api，内部缓冲为非流式 JSON 返回 |
| Q37 backend-api 字段 allowlist | ✅ A 适配器做字段 allowlist/变换（参照 codex-rs 实际发送），400/422 走 classify_http_status |
