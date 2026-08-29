# 设计文档

> 范围基线：`00-optimized-requirements.md` §2.1；`01-requirements-review.md` 的决策 1～7 已全部采纳。本文只设计 v1，不把 token 加密、zstd、30min 空闲探测或其它 provider 偷带回范围。

## 1. 架构总览（数据流 + 模块边界）

### 1.1 数据流

```text
登录/导入
  -> commands/auth.rs
  -> AuthService -> ProviderRegistry -> CodexProvider
  -> OAuth PKCE 或 auth.json -> token/model/quota 规范化
  -> Repository -> auth_accounts

下游 Chat / Messages / Responses
  -> security gate
  -> maybe_route_plan
       channels + request-scoped 可用 auth_accounts
       -> RouteCandidate 混合池 -> 协议组 -> priority -> weight
  -> build_prepared_attempt
       Chat      -> Responses（严格编码）
       Messages  -> Chat -> Responses（严格链式编码）
       Responses -> Responses（校验后原生）
  -> account adapter
       懒刷新 -> backend-api /responses（固定 stream:true）
       401 -> 刷新并仅重试同一账号一次
       响应头 -> QuotaState
  -> stream: Responses 原生 / Responses->Chat / Responses->Chat->Messages
     non-stream: 聚合 Responses SSE -> Responses JSON -> 对应 decoder
  -> request_logs(upstream_type=auth_account)
```

### 1.2 模块边界

- `db/models.rs`、`db/repository.rs`、`migrations/019_auth_accounts.sql`：只负责持久化形状、原子 upsert、账号状态和日志字段，不认识 OAuth HTTP 细节。
- 新 `auth_provider/`：`Provider` trait、通用 DTO/错误、provider registry、`AuthService`；`auth_provider/codex.rs` 是唯一 codex 特化处。路由层不得解析 `payload_json`。
- `core/route_plan.rs`、`core/attempt.rs`、`core/plan_executor.rs`：只认识通用 `RouteCandidate`，不直接刷新 token 或请求 backend-api。
- `endpoint_executor/`：在候选实体分叉；普通 Channel 保持原路径，Auth Account 交给 `AuthService::outbound`。401 刷新属于账号适配器内部，不进入 `AttemptFlow`。
- `protocol/codec/responses_codec.rs`：Responses 与 Chat 的双向请求/响应转换及 SSE 状态机；Messages 能力只做现有 Chat codec 的组合，不新增第三套协议机器。
- `commands/auth.rs`：Tauri 边界与无秘密 DTO；不把 `Provider` 细节扩散到前端。
- `src/pages/AuthChannelsPage.tsx`、`src/lib/api.ts`、`src/types/index.ts`：沿用 `useState + load()`；不引入 react-query/zustand 状态体系。

### 1.3 生产门禁

`maybe_route_plan` 先读取 enabled channels 和账号。若原 `new_routeplan=true`，行为不变；若 flag 为 false，但当前请求存在至少一个满足“active、未停用、quota 可用、模型快照命中、端点为 Chat/Messages/Responses”的账号，则仅对该请求强制进入混合 RoutePlan。账号分支固定具备 Responses 和跨协议能力，普通 Channel 仍遵守原 `native_responses/cross_protocol_codec` flags。没有 request-scoped 可用账号时继续走 legacy 路径，避免改变纯 Channel 流量。

## 2. 数据层（auth_accounts 表、upstream_type 迁移、DTO 掩码）

### 2.1 迁移

新增单个 `019_auth_accounts.sql`，由现有 `sqlx::migrate!("./migrations")` 自动纳入：

```sql
CREATE TABLE auth_accounts (
  id                   TEXT PRIMARY KEY,
  provider             TEXT NOT NULL,
  label                TEXT NOT NULL,
  account_id           TEXT NOT NULL,
  status               TEXT NOT NULL DEFAULT 'active',
  disabled             INTEGER NOT NULL DEFAULT 0,
  priority             INTEGER NOT NULL DEFAULT 0,
  weight               INTEGER NOT NULL DEFAULT 1,
  quota_json           TEXT,
  model_states_json    TEXT NOT NULL DEFAULT '{"version":1,"models":[]}',
  attributes_json      TEXT NOT NULL DEFAULT '{}',
  payload_json         TEXT NOT NULL,
  last_refreshed_at    TEXT,
  last_models_sync_at  TEXT,
  next_refresh_after   TEXT,
  next_retry_after     TEXT,
  created_at           TEXT NOT NULL,
  updated_at           TEXT NOT NULL,
  UNIQUE(provider, account_id),
  CHECK (disabled IN (0, 1)),
  CHECK (priority >= 0),
  CHECK (weight >= 1),
  CHECK (status IN ('active', 'invalid'))
);
CREATE INDEX idx_auth_accounts_route
  ON auth_accounts(disabled, status, priority, provider);

ALTER TABLE request_logs
  ADD COLUMN upstream_type TEXT NOT NULL DEFAULT 'channel';
```

`last_models_sync_at` 是确认书决策 2 的明确落点；唯一键采用 `(provider, account_id)`（决策 3）。数据库不对 `provider` 做 CHECK，避免未来加 provider 必须迁表；Rust `ProviderKind` 负责已注册实现校验。

### 2.2 JSON 语义

- codex `payload_json`：`{"version":1,"access_token":"…","refresh_token":"…","id_token":"…","expires_at":"RFC3339"}`。refresh token 永远按 opaque string 处理。
- `attributes_json`：非秘密展示属性，如 `email`、`plan_type`，以及 provider 自有非秘密标记；不可放 access/refresh/id token。
- `model_states_json`：`{"version":1,"models":[{"id":"gpt-…","status":"available","unavailable":false,"next_retry_after":null,"last_error":null}]}`。v1 路由只以非空模型 id 集合做精确授权，不实现 `model_mapping`，也不做模型级 quota；同步失败不覆盖快照和 `last_models_sync_at`。
- `quota_json`：`null` 表示上游从未返回限额，等价无限额；非空形状为 `version/exceeded/reason/next_recover_at/backoff_level/limits[]`，每个 limit 保存 `limit_id/limit_name/primary/secondary/credits`，窗口保存 `used_percent/window_minutes/reset_at`。

`disabled` 只表示用户开关；`status` 只表示凭据生命周期；quota 耗尽只写 `quota_json.exceeded`。加载路由账号时，若 `next_recover_at <= now`，repository 清除 expired quota 标记后再返回，恢复不依赖 12h 循环。`next_retry_after` 专供刷新失败退避，不与 quota 恢复点混用。

### 2.3 Repository 与 upsert

新增账号 CRUD、`upsert_by_provider_account_id`、`list_route_accounts(now)`、`update_tokens`、`update_models_if_success`、`update_quota`、`mark_invalid`。登录/导入命中唯一键时覆盖 token、attributes、刷新时间，保留 `id/label/priority/weight/disabled`；新账号默认 `priority=0, weight=1`。quota/model JSON 先反序列化校验再入库，禁止用字符串拼 SQL。

`RequestLog` 增加非 Optional 的 `upstream_type: String`，`Default` 实现值必须为 `channel`；repository INSERT/SELECT、约 19 个生产构造点和测试夹具全部接线。账号日志使用账号 id/name 填既有 `channel_id/channel_name`，同时写 `upstream_type=auth_account`。日志查询命令增加可选 `upstream_type`，Logs 页至少显示“API/Auth”来源标识并可过滤；统计 SQL 用该列分组时保留旧行默认 `channel`。

### 2.4 DTO 掩码

`AuthAccountDto` 只返回通用列、email/plan、模型快照、quota、`expires_at`、`has_refresh_token` 等摘要；access token、refresh token、id token、完整 `payload_json` 一律不序列化。不是“只掩码 refresh_token”，而是三类 token 均不越过 command 边界。`debug_json`、tracing、错误文本、request log 也不得包含 token、OAuth code、code_verifier 或 auth.json 原文。

## 3. Provider 抽象与 codex 实现

### 3.1 trait 与运行时服务

新模块建议：

```text
src-tauri/src/auth_provider/
  mod.rs          Provider trait / ProviderRegistry
  types.rs        AuthAccount, ProviderPayload, ProviderError, request/response
  service.rs      AuthService、per-account refresh single-flight、repository 编排
  codex.rs        OAuth/import/refresh、backend-api、models、quota parser
```

`Provider` 用 `async_trait`，至少暴露 `login`、`refresh`、`outbound` 三块，并把模型同步作为 provider 能力：

```rust
async fn login(&self, runtime: &dyn LoginRuntime) -> Result<LoginResult, ProviderError>;
fn import(&self, bytes: &[u8]) -> Result<LoginResult, ProviderError>;
async fn refresh(&self, payload: &ProviderPayload) -> Result<RefreshedPayload, ProviderError>;
async fn outbound(&self, request: ProviderRequest<'_>) -> Result<reqwest::Response, ProviderError>;
async fn list_models(&self, payload: &ProviderPayload) -> Result<Vec<ModelState>, ProviderError>;
```

`AuthService` 负责查库、provider 分派、持久化和 per-account `Mutex` single-flight；Provider 实现只做 provider 协议。并发请求发现临期 token 时，只允许一个刷新，后来的请求拿锁后重新读库，避免 refresh token 轮换被旧值覆盖。

### 3.2 OAuth PKCE

codex 常量集中定义并单测：authorize/token URL、`client_id=app_EMoamEEZ73f0CkXaXp7hrann`、backend base。流程为：

1. 绑定 `127.0.0.1:0`，先启动 axum 一次性 callback，再构造 `http://localhost:<port>/auth/callback`。
2. 生成高熵 `state` 和 PKCE verifier，challenge 为 S256；通过 `tauri-plugin-opener` 打开系统浏览器。
3. callback 只接受匹配 state 的一次 code；OAuth error、错 state、第二次回调均拒绝。整个 invoke 有固定 5 分钟 timeout，结束/窗口关闭/错误均释放 listener。
4. 用 `oauth2` crate 换 token。只把 id_token claims 解码用于 email/plan 展示，不把未验签展示 claim 用作授权；稳定去重键取 token 数据中的 account_id。
5. upsert 后立即拉 `/models`。模型首次同步失败仍返回“账号已保存、模型同步失败”的部分成功；因快照为空，该账号 fail-closed 不参与路由。

登录进度遵循确认书决策 4：`auth_login` 是单个异步 invoke；前端五步中“创建监听/打开浏览器/等待回调/入库/同步模型”由本地状态与 invoke 完成结果驱动，不新增轮询命令。

### 3.3 auth.json 导入与写回

路径优先 `$CODEX_HOME/auth.json`，否则 `~/.codex/auth.json`。导入严格接受真实嵌套形状：顶层 `auth_mode/OPENAI_API_KEY/last_refresh`，token 在 `tokens.id_token/access_token/refresh_token/account_id`。字段缺失即拒绝；access token 过期时先尝试 refresh，成功后导入。导入返回固定提示：本机 Codex 仍维持原登录态，双方 token 不自动同步。

写回只由 `auth_write_back` 触发：同目录临时文件写完并 `fsync`，权限设为 0600，再原子 rename；覆盖前创建带时间戳备份。任一步失败保持旧 auth.json 可恢复。Auth 不修改 `config.toml`，也不调用 `write_codex`。

## 4. 路由层改动（候选泛化、classify_channel 账号分支、过滤）

### 4.1 候选载体

在 `core/route_plan.rs` 引入：

```rust
enum RouteCandidate {
    Channel { channel: Channel, identity: ChannelIdentity },
    AuthAccount(AuthAccount),
}
```

通用方法只暴露 `id/name/priority/weight/upstream_type/provider/models/native_base_url`；秘密 payload 不进入 `debug_json`。`RouteGroupCandidate.channel` 改为 `candidate: RouteCandidate`，`HasPriorityWeight` 转发到通用方法。

优化需求书 §3.2 的 4 处消费面全部改，并补确认书指出的外围：

1. `resolve_model_candidates`：接收 channels + accounts；普通 Channel 继续 `status==1`、allowed_channels、空 models 通配；账号豁免 allowed_channels，空模型拒绝，过滤 invalid/disabled/quota。
2. `build_route_plan`：按候选类型解析能力并保持 native 组优先、priority/weight 无放回抽样和预算不变。
3. driver 非流/流两份 lookup：值改为通用候选并删除两处 `.expect()`；缺失映射返回内部错误并写失败日志，不 panic。
4. `build_prepared_attempt`：账号不读 `model_mapping`，模型原名直通；补 `upstream_type`。
5. `authorize_and_plan` 的空池判断、handlers 的账号加载、`plan_executor::AttemptMeta`、`RoutePlan::debug_json` 一并泛化。`AttemptFlow` 和重试预算状态机不改。

### 4.2 账号分类

保留 Channel 分类逻辑，新增 account 分支（可由 `classify_candidate` match 后调用 `classify_channel/classify_account`）：

| 下游 | 账号上游 | tier | codec |
| --- | --- | --- | --- |
| Responses | Responses `/responses` | Native | 无；原生 wire |
| Chat | Responses `/responses` | Conversion | `chat_to_responses_v1` / 响应 `ResponsesToChat` |
| Messages | Responses `/responses` | Conversion | `messages_to_responses_v1`（Messages→Chat→Responses） |
| CountTokens / Embeddings | 无 | 不出组 | 无 |

账号三条能力不受全局 `cross_protocol_codec/native_responses` 开关影响；这些开关对普通 Channel 继续生效。账号失败沿用现有失败分类和下一个候选/组降级（ADR-22）。错误枚举可保留 `NoChannels` 内部名以减少改动，但用户可见文案统一为 “No available upstream candidate”。

## 5. 出站适配（分叉点、懒刷新/401、限额解析、保守约束）

### 5.1 分叉与刷新

driver 闭包拿到 `RouteCandidate` 后：Channel 继续 `dispatch_executor/dispatch_stream_executor`；账号调用 `AuthService::dispatch_account_*`。这相当于现有 `gemini_native` override 的分叉，但不伪造 Channel，也不把 token 填进 `channel.api_key`。

账号适配器执行顺序：检查 `expires_at <= now + 5min` -> single-flight refresh -> 重读 payload -> 发送；收到 401 后在适配器内强制 refresh 并仅重发一次。第二次 401 或刷新失败标记 `status=invalid`，返回 `ChannelAuthTerminal` 让 RoutePlan 换候选；内部重发不增加 AttemptFlow attempt_no/is_retry。其它 429/5xx 仍按现有 failure class 降级。

### 5.2 请求

- 固定 `POST https://chatgpt.com/backend-api/codex/responses` 和 `GET .../models`；base 不接受前端或下游覆盖。
- 固定 `Authorization: Bearer <fresh access_token>`；`x-openai-actor-authorization` 只从账号的可信属性派生，绝不透传调用方同名 header。只转发现有 safe-header 白名单。
- 账号请求统一覆盖 `stream:true`。对于下游 `stream:false`，仍走流式 HTTP，再在账号适配器内聚合为目标非流 JSON。
- v1 backend allowlist 以 `model/input/instructions/tools/tool_choice/max_output_tokens/stream` 为核心；`stream` 由适配器强制。任何未列出的非空顶层字段（至少 `store/background/metadata/parallel_tool_calls/reasoning`）在发起 HTTP 前以稳定 JSON pointer 返回 400；不得静默整包直通。未来真实令牌验证后只扩 allowlist，不改变默认 fail-closed。
- 不设置 `Content-Encoding: zstd`，也不启用 reqwest `zstd` feature。

### 5.3 quota

在 `reqwest::Response` 被消费前解析并持久化安全响应头。parser 扫描 `x-<limit_id>-(primary|secondary)-(used-percent|window-minutes|reset-at)`，**不写死窗口类型**——primary/secondary 是平行可选项，实际由 `window-minutes` 决定（实测 free 号 `primary.window_minutes=43200` = 30 天月限额；非 free 常见周限额；`window_minutes` 单位是分钟）；未知 limit id 原样作为结构化项保存。前端只识别三种标签：**5H限额 / 周限额 / 月限额**，其它时长显示裸「限额」。

**空窗口丢弃（镜像上游 `has_data`）**：窗口仅当 `used_percent != 0 || window_minutes != 0 || reset_at 存在` 才保留；只有 `used-percent: 0` 的窗口（如 free 号的 `secondary`）丢弃。限额项 `primary/secondary/credits` 全空时整个丢弃（如 `codex-credits-has`，其 `x-codex-credits-*` 头为布尔标志，非数值，无法解析为 credits）。

任一窗口 `used_percent >= 100` 时账号级 `exceeded=true`；多个耗尽窗口的恢复点取最后一个 reset（必须全部恢复才能重新入池）。429 优先用 `Retry-After`，无有效值时指数退避；非 429 且所有已知窗口恢复时清除 exceeded。没有任何限额头时不凭空创建 quota，视为无限额。

`AttemptSuccess.response_headers` 仍负责向下游转发安全头；quota 更新失败只记脱敏 warning，不破坏已成功响应。`codex.rate_limits` SSE 事件不归一化：Responses 原生流字节级直通；Chat/Messages 转换流将该完整 SSE record 作为未改写 side-band record 透传，并用固定夹具锁定字节一致性。

**主动探测（无流量时更新）**：上游提供专门限额端点 `GET {backend-api}/wham/usage`（WaLiAPI base 为 `https://chatgpt.com/backend-api/codex` → 探测 `https://chatgpt.com/backend-api/wham/usage`），响应含 `rate_limit.primary_window`（`used_percent / limit_window_seconds(秒) / reset_at(UNIX 秒)`）、`credits`、`spend_control`。`quota_from_usage_payload` 归一化秒→分钟、UNIX→RFC3339 后写同一 `QuotaState`（只有 `primary`，无 `secondary`）。触发时机：**模型同步后**（登录/导入/同步模型）、**刷新令牌后**、**维护循环**（12h）。探测失败**静默保留旧值**，绝不擦除已知 quota（ADR-14 动态兼容）。

实测：free 号 `limit_window_seconds=2592000`（30 天月限额）、plus 号 `604800`（7 天周限额）；响应头窗口可能为流量上下文、`wham/usage` 为权威状态。

## 6. Codec（Responses→Chat、registry/SseMode 接线、usage 提取）

### 6.1 文件与 registry

新增 `protocol/codec/responses_codec.rs` 并在 `codec/mod.rs` 导出。`Downstream/Upstream` 都增加 `Responses`。registry 增加：

- Chat request -> Responses request：`chat_to_responses_v1`；
- Messages request -> Responses request：组合现有 `messages_to_chat_v1` encoder 与 Chat->Responses encoder；
- 对应的非流和流响应 decoder。

`core/attempt.rs::codec_direction()` 增以上方向；`driver.rs::sse_mode_for()`、`endpoint_executor/sse.rs::decoder_for()` 接 `SseMode::ResponsesToChat`。Messages 组合路径再用 `ResponsesToMessages` 模式/组合 decoder，但内部必须是 Responses→Chat→现有 `ChatStreamDecoder`，不另写直接 Responses→Messages 状态机。

### 6.2 严格请求编码

新 Chat→Responses encoder 定义 `SUPPORTED_TOP_LEVEL` 和每种 message/tool 的字段集合：未知字段、无法表示的 content、无效 tool arguments、图片超过现有限制均返回 `UnsupportedFeatures`，在零上游调用前终止。Messages 路径每一段都必须成功才生成请求。

现有 legacy `protocol::responses_to_openai` 仍供普通 Channel 的 Responses→Chat debt 使用，但其入口前增加同样的顶层 allowlist 校验并改为 `Result`；不再允许它遇到未知字段后 fail-open。账号原生 Responses 请求也先过 backend allowlist，因此三种下游均满足 fail-closed。

### 6.3 Responses→Chat 流式状态机

复用 `codec/sse.rs` 的字节 framing、`ResponsesSseAssembler`、`StreamDecoder` 泵与 report/usage 类型。新状态机至少持 `response_id/model/created/role_emitted/text/tool_calls/reasoning/usage/terminal_emitted`：

- created/in_progress 与 message `output_item.added`：最多一次 assistant role 起始 chunk；
- `output_text.delta`：Chat `delta.content`；done/item.done 只闭合状态，不重复文本；
- function_call added + arguments delta/done：按 output index 累积并输出 Chat `tool_calls[index]`，保留 call id/name/arguments；
- reasoning summary 生命周期：输出 Chat `reasoning_content`，以便后续链到 Messages thinking block；
- `response.completed.response.usage`：映射 `prompt_tokens/completion_tokens/total_tokens`，先输出 usage chunk，再恰好一次 finish chunk 与 `[DONE]`；有 tool call 时 finish_reason=`tool_calls`，否则 `stop`；
- `response.failed`/incomplete：提交前返回可降级错误，提交后输出协议可表达错误并终止；EOF 未见 terminal 为协议错误；
- `codex.rate_limits`：不改内容、不参与状态推进。

必须覆盖任意 TCP 分片、一个 chunk 多 record、UTF-8 中间分片、文本+tool、纯 tool、reasoning、重复 done、failed、缺 terminal、usage 恰好一次。

### 6.4 非流聚合与 decoder

账号上游恒为 SSE。`ResponsesEventAccumulator` 从完整 event records 等待 `response.completed.response`，构造最终 Responses JSON；`response.failed` 或 EOF 无 completed 返回 502。下游 Responses 直接返回该 JSON；Chat 用 Responses→Chat 非流 decoder；Messages 再链现有 Chat→Messages decoder。这样不依赖 backend-api 支持 `stream:false`，也不把 SSE 误送到非流客户端。

### 6.5 Native usage

扩展 `scan_usage_from_chunk`：Native Responses SSE 从 `response.completed.response.usage.input_tokens/output_tokens` 提取，保留当前顶层 `usage` 扫描。解析必须经过完整 SSE record framing，不能假设一个网络 chunk 就是一条 JSON。非流继续从聚合后的 Responses JSON取 usage。

## 7. 定时任务（12h 单循环）

`AppState` 增 `auth_service`（内部含 registry 和 refresh locks）。在 `lib.rs setup` 管理 state 后只 spawn 一个 Auth maintenance loop：启动后先跑一次轻量 due scan，随后用 `tokio::time::interval(12h)`；每 tick 按账号隔离执行：

1. 刷新临期/到期 active 账号；
2. 用 refresh token 重试符合 `next_retry_after` 的 invalid 账号，成功恢复 active；
3. 对有可用 token 的账号同步 `/models`，失败保留旧快照。

单账号失败不终止循环；使用 bounded concurrency 或顺序执行，避免启动时同时轰击 provider。loop handle 随 App 生命周期退出，不增加第二个 30min loop，也不做主动 quota probe。quota 到期恢复由路由加载时懒处理。

## 8. Tauri 命令集

新 `commands/auth.rs` 并在 `commands/mod.rs`、`lib.rs::generate_handler!` 注册恰好 10 条：

| 命令 | 输入 / 结果语义 |
| --- | --- |
| `auth_accounts_list` | 无输入；返回无 token 的 `Vec<AuthAccountDto>` |
| `auth_login` | provider；单 invoke 完成 OAuth、upsert、首次模型同步，允许部分成功 |
| `auth_login_import` | provider/path 可省略；前端先弹文件选择框(默认路径来自 `auth_default_import_path`)，读选中文件，过期先 refresh |
| `auth_default_import_path` | 返回默认 auth.json 路径,供文件选择框 defaultPath |
| `auth_logout` | account id；**纯本地删除**（删数据库行 + 模型快照），不调用 provider revoke（ADR-38） |
| `auth_refresh_token` | account id；刷新并返回新摘要，失败按规则置 invalid |
| `auth_sync_models` | account id；成功才替换快照，返回只读模型列表 |
| `auth_write_back` | account id；原子覆盖+0600+备份，返回写入路径/备份路径 |
| `auth_toggle` | account id + disabled；只改用户开关 |
| `auth_quota_status` | account id；返回规范 QuotaState/null 与有效恢复状态 |
| `auth_update` | account id + label/priority/weight；校验 label 非空、P>=0、W>=1 |

命令延续 `Result<T,String>` 外观，但内部保留 `ProviderError` 分类；对前端错误文本做脱敏。`auth_logout` 为**纯本地删除**（删数据库行 + 模型快照，返回被删账号摘要），不调用 provider 的 revoke 端点；不触达 provider 服务端，故无「revoke 失败」路径，也不存在伪报成功撤销的问题。

## 9. 前端（路由、Auth 页、卡片、弹窗、状态变体）

### 9.1 路由与页面壳

`App.tsx` 增 `/channels/auth`，必须置于 wildcard 前；`Sidebar` 的 `/channels` NavLink 设置 `end`（根 `/` 同样维持精确判断），保证 Auth 路由不会让 API 渠道入口错误 active。`ChannelsPage` 和新 `AuthChannelsPage` 都渲染 route-driven API|Auth underline tabs，并用各自路由导航。

`api.ts` 增薄 `authApi` 对应 10 命令；`types/index.ts` 增无秘密 DTO、quota/model/result/input 类型。页面使用 `useState/useEffect/load()`，每个 mutation 完成后 reload；按钮有 per-account pending 状态，防重复提交。

### 9.2 Auth 页

按 `01-ui-spec` 落地：标题/副标题和头部“登录账号、从 auth.json 导入”；codex 绿色 active pill，claude/kiro/kimi 灰置“规划中”；说明文字；ADR-29 固定风险 banner；`xl:grid-cols-2` 账号卡片 + 空槽。风险提示必须逐字渲染：

> ⚠️ 风险提示：此提供商使用的订阅 / OAuth 会话未获官方授权用于代理 / 路由器使用。账户可能被限制或封禁。使用风险自负。

颜色按实际 `App.css`：主色 `--color-primary:#2f6fed`、边框 `--color-border`、危险/成功/警告使用现有 `--color-destructive/#dc4c64`、`--color-success/#1f8f5f`、`--color-warning/#c48a21` 或对应 Tailwind palette，不引用不存在的 `--emerald/--amber/--red` 变量。

卡片展示 provider、label、email、plan、P/W、模型数/只读 tags、同步/刷新时间。有 quota 且含非空窗口才画限额块（标签按 `window-minutes` 推导，只识别 **5H限额 / 周限额 / 月限额** 三种，重置显示具体时间点）；无 quota 或窗口全空不画限额块。三态：正常（绿）、quota 耗尽（琥珀+踢出路由+恢复时间）、invalid（灰/红警示+重新登录）。右上横排编辑/启停/删除；底部均分刷新 token/同步模型/写回 CLI。

### 9.3 弹窗

- 登录：五步视图；单 invoke 期间根据本地阶段展示监听、浏览器、等待、入库、同步，timeout/error 后可重试且不遗留 listener。
- 编辑：label、priority、weight，客户端与后端同规则校验。
- 同步模型：打开即调用，spinner 后只读列出模型，无 checkbox。
- 删除与写回 CLI 必须二次确认；删除弹窗只问「是否删除」（删除后此账号不再参与路由），确认后纯本地移除；写回 CLI 显示将覆盖的目标与备份结果。
- 导入、刷新、同步、写回、重登按 UI spec 提示 toast；首次模型同步失败要明确“账号已保存但暂不参与路由”。

## 10. 错误处理与测试策略

### 10.1 错误边界

- DB/migration：事务失败整体回滚；非法 JSON 不进入路由；旧日志因默认列继续可读。
- OAuth/import：错 state、超时、缺字段、token exchange 失败均不创建半账号；账号已 upsert 而模型失败用结构化 partial success 表达。
- Provider：错误分类为 caller/config、auth、quota、transport、upstream protocol；任何错误实现 `Debug/Display` 时都先脱敏。
- 路由/codec：未知字段和无 codec 路径在 HTTP 前 400 fail-closed；候选 lookup 缺失返回 500/502 及日志，不 panic。
- 流：首个可编码 downstream frame 前允许候选 failover；commit 后错误不得换候选；terminal/[DONE] 恰好一次。
- 文件写回：temp+fsync+chmod+rename；失败保留原文件和备份。

### 10.2 不依赖真实令牌的测试

- repository：SQLite 临时库跑全部 migrations，测唯一键 upsert 保留路由配置、模型同步失败保留、quota 到期懒恢复、request_logs 默认/账号类型。
- Provider：注入 `LoginRuntime`、时钟、随机源和本地 axum mock；用假 JWT payload + opaque refresh fixture 测 PKCE state、timeout、导入、refresh rotation、401 仅一次、并发 single-flight、header/allowlist；不访问 auth.openai.com/chatgpt.com。
- quota：纯 header map table tests，覆盖多 limit id、动态窗口（月 43200 保留 / 周）、空窗口（仅 used-percent:0）丢弃、百分比边界、无头、坏 reset、Retry-After、多个窗口取最晚恢复点。
- routing：固定 RNG，覆盖混合池顺序、账号豁免 allowed_channels、空模型拒绝、disabled/invalid/quota 过滤、到期恢复、三下游出组、纯 Channel flags 行为未变。
- codec：静态 JSON/SSE fixtures，覆盖 §6.3 全事件链和任意分片；用计数 mock 断言请求编码拒绝时 upstream calls=0；Chat/Messages/Responses 的 stream/non-stream 都做 golden assertions。
- executor：本地 mock 返回 401→200、429、SSE、rate_limits；断言 AttemptFlow attempt_no 不因 token refresh 增加、日志 upstream_type 正确、Native usage 非零。
- commands：fake ProviderRegistry + temp HOME/CODEX_HOME，测 10 命令序列化不含 `access_token|refresh_token|id_token|payload_json`，写回权限/备份/失败恢复。
- frontend：`npm run build` 作为类型/构建门；把卡片状态派生和命令结果归并保持为纯函数并做确定性 fixture（若本轮不引入前端 test runner，则由 Rust command contract tests + build 兜底，不引入真实 token E2E）。

统一回归命令：`cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`，仓库根执行 `npm run build`。

## 11. open 风险处置

### 11.1 v1 保守处置

| 风险/待验证 | v1 处理 |
| --- | --- |
| backend-api 是否支持 `stream:false` | 不探测、不依赖；永远发 `stream:true`，非流在内部聚合 |
| backend-api 接受哪些字段 | 冻结最小 allowlist；未知/未验证字段 400 fail-closed，真实验证后只扩表 |
| `codex.rate_limits` 是否插入 SSE | 完整 record 原样透传，不归一化；fixture 锁字节一致 |
| zstd 是否强制 | v1 不实现；若真实环境证明强制，另立 ADR/任务启用 reqwest feature |
| 30min 空闲 quota 探测 | 不实现；响应头+429更新，恢复点在路由加载时懒恢复 |
| token 明文风险 | 与 channels.api_key 一致明文；DTO/日志全面隔离，后续单独设计加密迁移 |
| OAuth 进度/取消 | 单 invoke、5min timeout、资源清理；不扩第 11 条命令 |
| revoke 能力不明确 | **v1 删除不做 revoke**：删除=纯本地移除，不依赖远端能力（ADR-38；用户拍板 2026-08-09） |

### 11.2 ADR 覆盖矩阵

| ADR | 设计落点 |
| --- | --- |
| 1, 5, 6, 9, 11, 22 | §1、§4 混合候选、独立账号、同池权重和降级 |
| 2, 23, 24 | §3 OAuth/import、二元唯一键 upsert、部分成功 |
| 3, 13 | §2 通用列 + payload_json；ADR-13 按 ADR-3 同义项处理 |
| 4, 17, 26, 27, 29 | §9 双路由、卡片、编辑、状态、风险 banner |
| 7, 35, 36, 37 | §5 backend adapter、401 内部一次重试、强制流、allowlist |
| 8, 21, 34 | §2.2、§3.2、§7 模型快照、只读全量、空拒绝、无映射 |
| 10, 25 | §3.1、§5.1、§7 懒刷新/401/12h/失效恢复 |
| 12 | §3 Provider trait 与 codex 首实现 |
| 14, 15, 16, 28 | §2.2、§5.3、§7 quota 解析、账号级退出、缺失无限额、无 30min 探测 |
| 18, 19 | §3.3、§8、§9 手动写回且不联动 Usage/config.toml |
| 20 | §8 恰好 10 条 Tauri commands 与无秘密 DTO |
| 30 | §2.3、§8、§9 日志/命令/TS/UI 区分 upstream_type |
| 31 | §4.2、§6 三下游与 Responses→Chat/链式 Messages codec |
| 32 | §4.1 账号豁免 allowed_channels |
| 33 | §6.5 Native Responses usage 补提取 |

### 11.3 §2.1 范围核对

数据层见 §2；Provider/登录/刷新/模型见 §3、§7；混合路由与 rollout 边界见 §1.3、§4；出站、限额和保守约束见 §5；Codec 最小改动面与非流聚合见 §6；10 条命令见 §8；前端完整 UI spec 见 §9；泛化错误措辞和无秘密 debug 见 §2.4、§4.2。优化需求书 §3.2 的候选载体及全部消费点落在 §4.1；§3.3 的 framing/assembler/decoder 复用与从零状态机落在 §6；§3.4 的 driver 分叉、账号 adapter、懒刷新/401 落在 §5.1 和 §7。排除项在本文没有实现任务落点。
