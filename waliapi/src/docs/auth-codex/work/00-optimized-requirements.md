# 优化需求书 · Auth/Codex 登录（v1 校准版）

> **用途**：本文档是对 `docs/auth-codex/` 需求集的一次「对照代码现状的校准 + v1 范围收敛」。**与原文冲突时以本文档为准。**
> **方法**：5 个只读核对 agent 并行核对代码现状（路由/执行层、DB/仓储层、commands/服务层、前端层、protocol 层），逐条比对原文档声明。
> **分支**：`v0.1.8-auth-codex`。Codex 工作流输入顺序：本文档 → 00-facts → ADRs → 02-routing-compat-review → 01-ui-spec → glossary。

---

## 0. 校准总评

原文档（00-facts / ADRs / 02-routing-compat-review / 01-ui-spec）**绝大多数声明属实**（行号基本吻合，说明基于当前状态编写）。需修正的集中在：**文件路径迁移**、**候选类型已半成品化**、**driver 双 panic**、**前端颜色 token 与未用依赖**、**依赖面比预期充分**。详见 §1。

**本轮的 v1 范围收敛决策（用户拍板）**：账号 v1 **服务全部下游协议**（Responses / Chat / Messages，ADR-31 生效），附带保守约束；token 明文存储；zstd 延后；30min 空闲限额探测延后；后台任务合并为单个循环。详见 §2。

---

## 1. 校准修正（原文档 vs 代码现状）

### 1.1 已过时 / 需修正

| # | 原文档说法 | 修正后的现状 | 引用 |
|---|---|---|---|
| C-1 | `route_plan.rs` / `attempt.rs` 位于 `endpoint_executor/` | **已迁至 `core/`**（`core/route_plan.rs`、`core/attempt.rs`）。行号仍吻合，仅目录路径过时 | `core/mod.rs` 声明 |
| C-2 | 兼容审查 risk #1「没有候选抽象，RoutePlan 候选是具体 Channel」 | **`RouteGroupCandidate` 中间类型已存在**（`core/route_plan.rs:110-117`），`HasPriorityWeight` 已为它实现（:226-233）。候选泛化需把 `channel: Channel` 换成 Channel∪Account 载体，并同步全部消费点（不止 4 处，见 §3.2 / 01-requirements-review §4） | 见 §3.2 |
| C-3 | 兼容审查 risk #2「driver lookup map 一处硬 panic」 | **两处**：非流式 `route_plan_response`（`endpoint_executor/driver.rs:106-109`）+ 流式 `route_stream_plan`（driver.rs:414-417），各有一份 `HashMap<String,(Channel,ChannelIdentity)>` + `.expect()` | |
| C-4 | `01-ui-spec.md` 颜色 token `--primary=#3b82f6`、`--border=#e2e8f0` | 实际 `src/App.css:12` `--color-primary: #2f6fed`、`:8` `--color-border: rgba(15,23,42,0.08)`；**无 `--emerald/--amber/--red` 变量**，配色走 Tailwind 默认 palette | 前端实现按实际 token |
| C-5 | （隐含）zustand / @tanstack/react-query 可用 | 两依赖在 `src/` **零使用**；页面全用 `useState` + 手动 `load()`。新页面沿用此模式，不引入 react-query 体系 | |
| C-6 | `write_codex` 是「命令」 | 是普通 helper 函数（`commands/app_config.rs:363-428`），非 `#[tauri::command]`；`apply_app_config`（:827-880）在 :848 调用。**不写 auth.json** | ADR-18/19 不受影响 |
| C-7 | `scan_local_ai_configs` 读 config 文件 | 属实（`import_export.rs:541-750`，Codex 段 :586-686）；但**只解析顶层 `api_key`**，WaLiAPI 写出的 config.toml（key 在 `[model_providers.waliapi].experimental_bearer_token`）扫描读不到。此为现状缺口，与 Auth（读 auth.json）弱相关，v1 不处理 | |
| C-8 | 依赖面未知 | **依赖面已充分**：`axum 0.8`（`Cargo.toml:20`）、`tokio` full（:21）、`reqwest` json+stream（:35）、`tauri-plugin-opener`（`lib.rs:42` + capability 已配）。**唯一实质新增依赖 = `oauth2`**（或手写 PKCE 则零新增）。`reqwest` 未启用 zstd（gzip/brotli 已启用） | 见 §3.7 |
| C-9 | 迁移组织未明确 | **独立 SQL 文件** `src-tauri/migrations/NNN_*.sql`，`sqlx::migrate!("./migrations")` 编译期内嵌自动纳入，**无需注册**。最新 = `018_wiki_tags.sql`。`db/mod.rs:120-151` 迁移前自动备份、`:27-54` checksum 修复 | |
| C-10 | `adaptor/` 目录定位 | 是按渠道类型分发的**连通性测试 + 请求转发适配器**（`adaptor/mod.rs:41-74`），T07 后新执行器（endpoint_executor）不走 adaptor。Auth 出站**不走 adaptor**，走新账号适配器 | |
| C-11 | 后台定时任务机制 | **无**任何周期性调度器；唯一 `tokio::time::interval` 先例在 `services/mcp/handlers.rs:622`（SSE keepalive）。后台任务挂载点：`lib.rs setup`（:124-128）`tauri::async_runtime::spawn` 常驻 loop；状态挂 `AppState`（:24-33）仿 `test_receipts` | 见 §3.6 |

### 1.2 确认属实、构成 v1 硬缺口

| # | 缺口 | 关键位置 |
|---|---|---|
| C-12 | `SseMode` 无 `ResponsesToChat`；codec registry 只注册 Chat↔Messages 两方向，`Downstream/Upstream` 枚举**连 `Responses` 变体都没有** | `endpoint_executor/sse.rs:22-34`、`protocol/codec/registry.rs:18-28, 102-119` |
| C-13 | **流式 Responses SSE→Chat SSE 状态机完全从零写**（无任何现成实现）；字节级 framing（`codec/sse.rs:16-59`、`ResponsesSseAssembler` `responses.rs:14-54`）可复用 | 见 §3.3 |
| C-14 | `request_logs` 无 `upstream_type` 列；无 `auth_accounts` 表 | `db/models.rs:187-227`；迁移最新 018 |
| C-15 | 原生直通流式 usage 只读顶层 `usage`（`sse.rs:116`）→ `response.completed.response.usage` 取不到；非流式已支持（`endpoint_executor/mod.rs:667-682`） | D-4，v1 必补 |
| C-16 | 无出站前懒刷新令牌钩子（`send_request` 用静态 `channel.api_key`，`endpoint_executor/mod.rs:461`）；可仿 `gemini_native` override 分叉先例（:434-452） | |
| C-17 | 命令层返回 `Result<T,String>`，DTO 掩码模式（`ChannelDto.mask_key`，`commands/channel.rs:49-88`）；auth 表需 DTO 掩码 refresh_token。复核者补强：不止 refresh_token，**access_token / id_token / 完整 payload_json 一律不得越过 command 边界**（ADR-20），见 01-requirements-review §4 | |
| C-18 | Sidebar NavLink 前缀匹配（`Sidebar.tsx:87-96`）会把 `/channels/auth` 也标 active，需 `endpoint` 特判 | |
| C-19 | `allowed_channels` 是 **channel-id 白名单**（`core/route_plan.rs:371-380`），账号无 channel id，语义上天然豁免 → 需显式决策 | 见 §2.2 D-B |

---

## 2. v1 范围（收敛决策，用户拍板）

### 2.1 v1 纳入范围（In scope）

**数据层**
- `auth_accounts` 表（通用列 + `payload_json` 两层结构，ADR-3/13）：通用列 `id / provider / label / account_id / status / disabled / priority / weight / quota_json / model_states_json / attributes_json / last_refreshed_at / next_refresh_after / next_retry_after / created_at / updated_at`；`payload_json` 存 codex 令牌载荷（access_token / refresh_token / id_token / expires_at）。
- `request_logs.upstream_type` 列（`TEXT NOT NULL DEFAULT 'channel'`，ADR-30）；**全部** RequestLog 生产构造点（非三处——复核确认至少 19 处生产构造点 + 测试构造点）带上类型，旧路径显式或默认写 `channel`；`RequestLog` 结构体追加非 Optional 字段。

**Provider 抽象（ADR-12）**
- `Provider` trait：登录 / 刷新 / 出站三块。codex 为首个实现。

**登录（ADR-2）**
- 内嵌 OAuth PKCE S256 + localhost 回调：`client_id=app_EMoamEEZ73f0CkXaXp7hrann`、刷新端点 `https://auth.openai.com/oauth/token`、`redirect_uri=localhost:随机端口`。回调服务器用 axum 0.8 一次性监听；开浏览器用 `tauri-plugin-opener`；token 交换用 `oauth2` crate（或手写 PKCE）。
- `auth.json` 导入兜底：**以真实文件形状为准**（facts §7：顶层 `auth_mode / OPENAI_API_KEY / last_refresh`，令牌三件套嵌在 `tokens` 下；refresh_token 是 opaque 字符串不解析；email 从 id_token `email` claim 解析；account_id 为去重键）。

**令牌存储 / 刷新（ADR-10，D-F）**
- **明文存储**（与 channels.api_key 一致），加密延后。
- 出站前懒刷新临近过期令牌 + 401 触发一次刷新重试（**在账号适配器内部**，不动 AttemptFlow，D-3）。
- 单个 12h 后台任务（D-F 合并）：令牌刷新 + 模型同步 + 失效账号重试刷新，一个循环。

**路由层（ADR-6/9/11/16/22）**
- 混合候选池：`RouteGroupCandidate.channel` 泛化为 Channel∪Account 载体；`HasPriorityWeight` 对账号候选 impl；同步全部消费点（§3.2，含 `authorize_and_plan`/handler/`plan_executor::AttemptMeta`/`debug_json`）。
- 账号过滤：`resolve_model_candidates` 增加账号级 QuotaState / 失效 / 停用过滤（镜像 `status==1`）。
- `classify_channel` 账号分支：**Chat / Messages / Responses 均出组**（D-A）。
- `allowed_channels` 账号豁免（D-B，保持白名单 channel-id 语义）。

**出站适配（ADR-7）**
- 账号出站分支（仿 `gemini_native` override 先例），backend-api 两端点：`POST {base}/responses`、`GET {base}/models`，`base=https://chatgpt.com/backend-api/codex`。
- Bearer access_token 头 + `x-openai-actor-authorization`（托管 client 用）等可选头。
- **保守约束（D-A）**：字段 allowlist（backend-api 拒绝的字段不直通）；`stream:false` 不可靠时强制 `stream:true` 兜底；`codex.rate_limits` SSE 事件**原样透传不归一化**。

**限额（ADR-14/15/16/28）**
- 出站响应头动态解析 `x-<limit_id>-<primary|secondary>-*`（used-percent / window-minutes / reset-at）更新 `quota_json`（钩子：`AttemptSuccess.response_headers` 已透传）；**不写死窗口类型**（codex 实际无 5h 窗口——free=月限额、非 free=周限额，按 `window-minutes` 推导）；仅 `used-percent:0` 的空窗口丢弃；429 / Retry-After 动态兼容；无返回视为无限额。
- 账号级踢出/恢复（整个账号为粒度）；30min 空闲主动探测**延后**。

**Codec（ADR-31，D-A）**
- 新增 **Responses→Chat**：流式 SSE 状态机（核心新写）+ 非流 decoder。
- **Messages 方向**：经 Responses→Chat 后链式复用现有 Chat→Messages codec（`chat_to_messages_v1`），或设计直接路径（由架构师②定，但不得新增协议机器）。
- registry 扩展：`Downstream/Upstream` 加 `Responses` 变体、`direction()` 加方向、`SseMode::ResponsesToChat` + `decoder_for` + `sse_mode_for` 接线。
- 严格 fail-closed 请求编码器：现有 `responses_to_openai`（`protocol/mod.rs:177-311`）是 fail-open legacy，需加 `SUPPORTED_TOP_LEVEL` 式校验，零上游调用前拒绝。
- usage 提取：`response.completed.response.usage` 在 Native 直通下补扫描（D-4）。

**模型列表（ADR-8）**
- 登录时自动拉取 `GET /models` + 12h 后台同步 + 手动刷新；只读、全量支持；`GET /models` 失败保留旧快照。

**Tauri 命令集（ADR-20）**
- `auth_accounts_list / auth_login / auth_login_import / auth_logout（纯本地删除，无 revoke）/ auth_refresh_token / auth_sync_models / auth_write_back / auth_toggle / auth_quota_status / auth_update`。命令文件新建 `commands/auth.rs`，注册 `lib.rs` invoke_handler（:133-228）。

**前端（ADR-4/17/25/26/27/29 + 01-ui-spec）**
- 路由：`/channels`（更名 API 管理）+ `/channels/auth`（Auth 页）；Sidebar 渠道项前缀匹配需 `endpoint` 处理。
- Auth 页：API|Auth underline tab（复用 `SettingsPage.tsx:195-214` class）；provider pill 一排（复用 `UsagePage.tsx:413-453`，codex 绿）；风险 banner 常驻（ADR-29 固定文案）；账号卡片网格（对齐 `ApiKeysPage.tsx:118-120`）；空槽卡片（登录 / 导入）；登录分步弹窗；编辑弹窗（名称 / priority / weight）；同步模型弹窗（只读）；状态变体（正常 / 限额耗尽 / 失效）；写回 CLI。
- `api.ts` 加 `authApi` 薄封装 + `types/index.ts` 加类型。

**其它**
- 错误措辞泛化：`PlanError::NoChannels` / Halt 文案改为「upstream candidate」（兼容审查 risk #14，低优先）。
- `debug_json` 快照加账号标识字段（不泄漏令牌）。

### 2.2 排除范围（Out of scope，延后）

| 项 | 依据 |
|---|---|
| token 存储加密 | D-D：与渠道一致明文 |
| zstd 压缩 | D-E：先探测后端是否强制；强制则加 reqwest `zstd` feature |
| 30min 空闲限额主动探测 | D-C：先只做响应头解析 + 429 兜底 + 懒刷新 |
| claude / kiro / kimi provider | 只留 `Provider` trait 抽象，不实现 |
| model_mapping | ADR-21 |
| 与 UsagePage Codex 配置联动 | ADR-19 |
| 账号级 allowed_channels 约束 | D-B：账号豁免 |
| `codex.rate_limits` 事件归一化 | 保守约束：原样透传 |

### 2.3 待验证项（需真实令牌，v1 保守处置）

| 待验证 | v1 兜底处置 |
|---|---|
| backend-api 是否接受 `stream:false` | 账号适配器**统一强制 `stream:true`**，非流式下游在适配器内部缓冲聚合（D-A / ADR-36，复核确认不再保留 `decode_non_stream` 容忍分支选项） |
| backend-api 拒绝哪些请求字段 | 账号适配器做字段 allowlist（参照 `codex-rs/codex-api` 实际发送），400/422 走 `CallerTerminal` |
| `/responses` 流是否插入 `codex.rate_limits` SSE 事件 | Native 直通原样透传，不归一化 |

---

## 3. 已拍板决策与实现约束

### 3.1 本轮用户拍板

- **D-A**：账号 v1 服务全部下游协议（Responses / Chat / Messages），**附带保守约束**（字段 allowlist、强制 stream:true 兜底、rate_limits 原样透传）。→ 兼容审查 D-1（仅 Responses）**被取代**，ADR-31 生效。
- **D-B**：账号**不受** `allowed_channels` 约束（白名单保持 channel-id 语义，账号豁免）。
- **D-C**：30min 空闲限额主动探测延后。
- **D-D**：token 明文存储（加密延后）。
- **D-E**：zstd 延后。
- **D-F**：后台任务合并为**单个循环**（12h 令牌刷新 + 模型同步 + 失效重试）。

### 3.2 候选泛化的最小改动面（校准后收窄）

`RouteGroupCandidate`（`core/route_plan.rs:110-117`）已存在。改动点：
1. 引入候选载体（enum `RouteCandidate { Channel(Channel), Account(AuthAccount) }` 或 trait）。
2. `RouteGroupCandidate.channel: Channel` 换成载体；`HasPriorityWeight` 转发目标改（:226-233）。
3. 消费点不止 4 处（复核收窄后补全，01-requirements-review §4）：`resolve_model_candidates`（:366-380）、`build_route_plan`（:545-642）、driver 两处 lookup（`driver.rs:87-97` 非流 / `:354-364` 流，panic 在 :106-109 / :414-417）、`build_prepared_attempt`（`core/attempt.rs:200-285`），以及 `authorize_and_plan` 签名与 `channels.is_empty()` 早退（:662-679, 682-715）、生产 handler 的账号加载（`server/handlers.rs`）、`plan_executor::AttemptMeta` 对候选的读取（`plan_executor.rs:56-113`）、`debug_json`。
4. **AttemptFlow 重试状态机不需要改**（executor 闭包已泛化，`plan_executor.rs:74-206`），但 `plan_executor.rs` 的候选元数据读取必须改。

### 3.3 Responses→Chat codec 复用面与缺口（校准后）

- **可复用**：字节级 SSE framing（`codec/sse.rs:16-59`）、帧重组器（`ResponsesSseAssembler` `responses.rs:14-54`）、`StreamDecoder` trait 泵骨架（`codec/registry.rs:59-75` + `sse.rs:139-161`）、`codec/report.rs` 报告结构、非流反向参考（`protocol/mod.rs:57-145` `openai_to_responses` 字段映射反写）、测试数据（`responses.rs:1117-1181` `REAL_FRAGMENTS`）。
- **从零写**：流式 Responses SSE→Chat SSE 状态机（消费 `event: response.*` 帧链：created/in_progress→role 帧、output_item.added(message)→assistant 起始、output_text.delta→content delta、output_text.done/item.done→finish_reason、function_call 链→tool_calls 累积、reasoning 生命周期、response.completed+usage→usage 帧、response.failed→错误帧）；非流 Responses→Chat decoder；严格请求编码校验；registry/SseMode/attempt 接线。
- **落地文件**：新 `protocol/codec/responses_codec.rs`（或并入 codec/）、`protocol/codec/registry.rs`、`protocol/codec/mod.rs`、`endpoint_executor/sse.rs`、`endpoint_executor/driver.rs`、`core/attempt.rs`、`endpoint_executor/mod.rs`、`core/route_plan.rs`。

### 3.4 账号出站分叉点（校准后）

- `send_request`（`endpoint_executor/mod.rs:426-476`）按 override 分叉有先例（`gemini_native`，:434-452）。账号分支仿此：`"codex_account"` 选择器 → 换 `native_base_url` + 账号头（Bearer access_token + 可选 actor 头）+ 出站前懒刷新。
- driver 两处闭包（`driver.rs:105-126` / `:414-428`）解构候选 → 分叉到账号适配器。
- 懒刷新/401 重试**全部在账号适配器内部**（D-3），AttemptFlow 不动。

### 3.5 usage 补提取（D-4）

- Native 模式（`sse.rs:105-136` `scan_usage_from_chunk`）加 Responses 专用扫描：从 `data:` 记录里找 `response.completed` 事件，取 `response.usage`。非流式已支持（`endpoint_executor/mod.rs:667-682`）。

### 3.6 后台任务挂载（D-F）

- `lib.rs setup`（:124-128）`tauri::async_runtime::spawn` 单个常驻 loop：每 12h 执行「令牌刷新（含失效重试）+ 模型同步」；挂载状态进 `AppState`（:24-33）。
- `auth_state` / 状态字段仿 `test_receipts`（`channel_test.rs:756-793`）的 AppState 内存范式。

### 3.7 依赖清单

| 依赖 | 必要性 |
|---|---|
| `oauth2` crate（新增） | 推荐（PKCE + token 交换）；或手写 PKCE（`rand`+`base64`/`sha2` 已在树内）则零新增 |
| `reqwest` feature `zstd` | 仅当后端强制 zstd（延后） |
| 其它 | **零新增**：axum 0.8（回调服务器）、tokio full（定时）、tauri-plugin-opener（开浏览器）、reqwest json/stream（token 请求）均已就绪 |

---

## 4. 对 Codex 工作流的作用

- **生效方式**：Codex 的架构师①（需求复核）**不再需要**把 ADR-31 vs D-1、D-2、token 加密、zstd、后台任务等作为疑点抛出——本轮已拍板（§3.1）。架构师① 复核本文档与原文的冲突点是否已全部收敛，仅对**新增发现**的不确定点产决策清单。
- **架构师②（设计）** 直接以本文档为范围基线做设计文档 + 任务拆分；任务拆分必须覆盖 §2.1 全部条目，并把 §3.2/3.3/3.4 的最小改动面纳入任务卡。
- **复核者 / 验证者** 以 §2.1（In scope）为验收边界：不得因 Out of scope 项 FAIL（如 token 未加密、zstd 未实现、无 30min 探测）。
- **已知但刻意延后**：C-7 扫描器读不到 WaLiAPI 写出的 config.toml key（现状缺口，非 Auth 引入，不处理）。
