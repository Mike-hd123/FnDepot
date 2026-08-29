# Kimi Auth：Claude 施工任务卡

> 给 Claude 使用的逐卡施工清单。开始施工前必须完整阅读同目录的 `2026-08-16-kimi-auth.md`；该文件提供已复核的背景、参考 commit 和协议依据，本文件是执行顺序、文件所有权、验证与交接的权威索引，不依赖任何聊天上下文。

## 全局规则

- 工作目录：/Users/xian/Project/ai/WaLiAPI；Rust 目录：src-tauri。
- 每卡先写失败测试，再改实现，再执行卡片命令；每卡至少 cargo check --all-targets。
- Kimi Provider 只负责 OAuth、固定 URL、headers、HTTP 状态分类；Chat/Messages/Responses 转换继续使用现有 CodecRegistry。
- OAuth 固定 https://auth.kimi.com；业务固定 https://api.kimi.com/coding；不使用 Moonshot Open Platform 或 Codex endpoint。
- /models 的逐模型 protocol 是唯一 wire profile 来源：缺失/kimi=Chat，anthropic=Messages beta，未知非空值 fail closed。
- 不新增 migration、Kimi 专用 codec、真实凭据/账号文件/数据库/日志 fixture。
- device_code、user_code、verification URL、access/refresh token、OAuth response 不得进入 payload、日志、Debug 或 command DTO；UI 只展示 URL/user code。
- 每次提交执行 git diff --check，并汇报实际测试结果，不得把未执行命令写成通过。

## 任务 DAG

~~~text
C0 基线
 └─ C1 ProviderKind/spec/model snapshot
     └─ C2 generic LoginRuntime/AuthService replacement
         └─ C3 Kimi Device OAuth
C1+C2+C3 ─── C4 Kimi backend/model sync/request context
C1+C4 ─────── C5 RoutePlan profile -> PreparedAttempt
C2+C4+C5 ──── C6 executor framing + registry
C2+C3+C6 ──── C7 sessions/Tauri commands
C7 ────────── C8 frontend
C3+C4+C6+C7 ─ C9 maintenance/security
C1..C9 ────── C10 final verification/smoke
~~~

C0→C7 必须按依赖串行，避免共享 Rust 文件发生接口漂移。C8 与 C9 在 C7 完成后可并行，因为前后端文件不重叠；C10 必须等待两者全部完成。

## 不可并行的重叠文件

| 文件 | 必须顺序 | 原因 |
|---|---|---|
| auth_provider/mod.rs | C1→C2→C3→C4→C6 | 类型、runtime、login/backend、registry 分阶段 |
| auth_provider/types.rs | C1→C4→C9 | ProviderKind、ProviderRequest、错误/脱敏 |
| auth_provider/service.rs | C2→C4→C6→C9 | authenticate、outbound、refresh、quota |
| commands/auth.rs | C2→C7 | 先扩 runtime，再改 session runner |
| core/route_plan.rs、core/attempt.rs | C5 独占 | profile 与 attempt 必须原子改造 |
| endpoint_executor/driver.rs | C4→C6 | 先补请求上下文，再消费 framing |
| src/types/index.ts、src/lib/api.ts | C8 独占 | TS contract 与 UI 同步变更 |

## 共同固定契约

~~~rust
enum ProviderKind { Codex, Kimi, Other(String) }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AuthNonStreamFraming { Json, ForcedResponsesSse }

struct ModelState {
    id: String,
    status: String,
    unavailable: bool,
    next_retry_after: Option<String>,
    last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    protocol: Option<String>,
}

struct AuthRouteProfile {
    provider: String,
    native_base_url: String,
    upstream_protocol: UpstreamProtocol,
    upstream_endpoint: String,
    non_stream_framing: AuthNonStreamFraming,
}
~~~

Profile 固定映射：Codex/missing → https://chatgpt.com/backend-api/codex + Responses/responses/ForcedResponsesSse；Kimi/missing/kimi → https://api.kimi.com/coding/v1 + OpenAI/chat_completions/Json；Kimi/anthropic → https://api.kimi.com/coding + Anthropic/messages_beta/Json。alias 多目标只有 profile 完全一致才可形成 candidate。

## C0：基线与机械更新清单

**依赖：** 无。 **Owned files：** 无，只读。 **只读依赖：** 原方案、CodeGraph、Cargo/npm 配置。

### 步骤

1. 阅读原方案全部章节和固定参考 commit。
2. 用 CodeGraph 先定位，再用 rg 收集所有构造点。
3. 建立机械同步清单：ModelState 增加 protocol；ProviderRequest 增加 is_stream/upstream_protocol/upstream_endpoint；所有 LoginRuntime 实现增加新方法；所有 Provider::login 实现接收 context；所有 RouteGroupCandidate/PreparedAttempt 构造补齐 profile；所有 outbound 调用传同一组元数据；Tauri/TS 使用同一 camelCase contract。

### 精确命令、测试和 DoD

~~~bash
cd /Users/xian/Project/ai/WaLiAPI
codegraph explore "ProviderKind ProviderRequest LoginRuntime ModelState RouteGroupCandidate PreparedAttempt auth_login_start"
rg -n 'ProviderKind|ProviderRequest|LoginRuntime|ModelState \{|RouteGroupCandidate \{|PreparedAttempt \{|Provider::login|send_with_persisted_account|auth_login_start|run_codex_login_session' src-tauri/src
rg -n 'payload_json|ProviderPayload|verification|device_code|user_code|access_token|refresh_token' src-tauri/src src
cd src-tauri && cargo check --all-targets
cd .. && git diff --check
~~~

DoD：列出全部触点和冲突文件，不修改业务代码；C0 不单独提交空 commit，清单随 C1 的交接记录保存。

## 机械更新点（执行时重新运行 rg，不依赖行号）

| 变更 | 当前必须覆盖的文件/实现 |
|---|---|
| `ModelState { ... }` | `db/models.rs` 定义；`auth_provider/codex_backend.rs`、`auth_provider/service.rs`、`auth_integration_tests.rs`、`endpoint_executor/integration_tests.rs` 构造点 |
| `LoginRuntime` impl | `auth_provider/codex_login.rs` 的 `TauriLoginRuntime` 与 3 个测试 runtime；`commands/auth.rs` 的 `SessionLoginRuntime` |
| `Provider::login` / `impl Provider` | `auth_provider/codex_backend.rs`；`auth_provider/service.rs` fake；`auth_provider/maintenance.rs` fake；`endpoint_executor/integration_tests.rs` 的 `LocalProvider` |
| `ProviderRequest { ... }` | `auth_provider/service.rs` 生产构造点；`auth_provider/codex_backend.rs` 测试构造点 |
| `RouteGroupCandidate { ... }` | `core/route_plan.rs` 生产构造点；`core/attempt.rs` 消费字段 |
| `PreparedAttempt { ... }` | `core/attempt.rs` 生产构造点；`endpoint_executor/mock_tests.rs`、`services/channel_test.rs` 手工 fixture |
| Auth outbound 参数 | `auth_provider/service.rs`、`endpoint_executor/mod.rs`、`endpoint_executor/driver.rs` 及对应 integration/mock tests |
| Tauri/TS contract | `commands/auth.rs`、`lib.rs`、`src/types/index.ts`、`src/lib/api.ts`、Auth 页面和组件 |

每次结构体或 trait 变更后立即执行对应 `rg` 和 `cargo check --all-targets`，不能只修改本卡正文列出的首个文件。

## C1：ProviderKind、ProviderSpec、模型快照

**依赖：** C0。 **Owned files：** 新建 src-tauri/src/auth_provider/spec.rs；修改 auth_provider/types.rs、auth_provider/mod.rs、db/models.rs 及直接构造点/测试。 **只读依赖：** 现有 registry、Codex spec、ModelState 使用点。

### 接口和步骤

- 增加 ProviderKind::Kimi，字符串为 kimi；未知字符串进入 Other。
- 增加 renderer-safe ProviderSpec、AuthLoginMode::{BrowserCallback,DeviceCode}、AuthNonStreamFraming::{Json,ForcedResponsesSse} 和三个纯查询函数。
- Kimi spec 固定：id kimi、Kimi Code、moonshot、DeviceCode、import/export/quota 均 false；Codex 值不变。
- ModelState.protocol 为 serde default 可选字段，不升级 snapshot version、不迁移。
- 先写 string round-trip、spec 精确值、未知 provider、旧 JSON、protocol round-trip 失败测试，再实现。

### 命令和 DoD

~~~bash
cd /Users/xian/Project/ai/WaLiAPI/src-tauri
cargo test auth_provider::types
cargo test auth_provider::spec
cargo test db::models
cargo check --all-targets
git diff --check
~~~

DoD：旧 Codex snapshot 可读，所有 ModelState 构造点编译，未注册可执行 Kimi。建议 commit：refactor(auth): add provider metadata and model protocol snapshot。

## C2：通用 LoginRuntime 与 replacement 持久化

**依赖：** C1。 **Owned files：** auth_provider/mod.rs、service.rs、codex_login.rs、codex_backend.rs、maintenance.rs、commands/auth.rs、db/repository.rs 及直接 fake/test。 **只读依赖：** C1、Codex callback/PKCE、刷新锁和 schema。

### 接口和步骤

~~~rust
enum LoginTarget { New, Replace { local_account_id: String } }
struct ProviderLoginContext { replacement: Option<ReplacementContext> }
struct ReplacementContext {
    local_account_id: String,
    provider_account_id: String,
    previous_payload: ProviderPayload,
}
async fn authenticate(kind: ProviderKind, target: LoginTarget, runtime: &dyn LoginRuntime)
    -> Result<AuthenticatedLogin, ProviderError>;
async fn persist_authenticated(authenticated: AuthenticatedLogin)
    -> Result<AuthAccountSummary, ProviderError>;
~~~

1. 扩展 LoginRuntime：set_step、present_device_authorization、is_cancelled、cancelled；步骤固定 Preparing/Authorizing/Waiting/Exchanging/Saving/Syncing。
2. 先写 runtime 记录 step、verification、cancel 的测试；authenticate 不写库，session runner 只有在 `begin_save` commit gate 成功后才能调用 persist。
3. AuthService 内部按 local ID 读取旧账号和 payload；command 不读 payload。
4. replacement 使用与 refresh 相同账号 mutex；锁内校验 provider、provider account ID、Kimi device ID。
5. repository 用 local ID + 前置条件原子更新凭据，同时清空 model snapshot 和 last sync；受影响行数为 0 fail closed。
6. 更新 SessionLoginRuntime、TauriLoginRuntime、Codex/fake/test runtime；取消 receiver 下沉到 runtime。
7. auth_login 对 DeviceCode provider 在网络前返回 interactive_session_required；Codex callback/PKCE/timeout/port fallback 不变。

### 测试、命令和 DoD

覆盖 authenticate 无写库、replacement 竞态、删除/provider/device 变化、旧 snapshot 清空、sync 失败不路由、sync 成功恢复、due/401 refresh 并发、cancel/begin-save exactly-once。

~~~bash
cd /Users/xian/Project/ai/WaLiAPI/src-tauri
cargo test auth_provider::service
cargo test auth_provider::codex_login
cargo test commands::auth
cargo test db::repository
cargo check --all-targets
git diff --check
~~~

DoD：所有 trait 实现编译；replacement 不调用通用 conflict upsert；取消不能落库；Codex 回归通过。建议 commit：refactor(auth): add generic login context and locked replacement persistence。

## C3：Kimi Device OAuth 登录与刷新

**依赖：** C2。 **Owned files：** 新建 src-tauri/src/auth_provider/kimi_login.rs，必要时仅改 src-tauri/Cargo.toml。 **只读依赖：** C2 runtime/context、reqwest/Tokio、官方固定 commit。

### 接口、常量和步骤

固定常量：client id 17e5f671-d194-4dfb-9706-5516cb48c098；host https://auth.kimi.com；paths /api/oauth/device_authorization、/api/oauth/token；总 deadline 15 分钟；HTTP timeout 30 秒。

- Device response 严格验证非空 user/device code 和 complete URL；interval 默认 5 且为正。
- Token response 严格验证 access/refresh/expires；wire structs 不 Serialize/普通 Debug。
- payload 只含 version、access/refresh、token_type、scope、expires_at、expires_in、device_id；device ID 为 Uuid::new_v4().simple()。
- 实现 device authorization、polling、refresh、错误分类和设备 headers。
- polling：pending 继续；slow_down 永久 +5 秒；expired 重申但共享 15 分钟 deadline；denied 结束。
- 取消必须打断 HTTP 和 sleep；browser 先 present、后 open；open 失败可继续手动授权。
- refresh：401/403/invalid_grant=Unauthorized；429/5xx/网络最多 3 次，1/2 秒退避；字段错误=Protocol。

### 测试、命令和 DoD

必须用本地 Axum mock 覆盖 pending success、slow_down、expired、denied、timeout、cancel、rotation、refresh 错误和脱敏。

~~~bash
cd /Users/xian/Project/ai/WaLiAPI/src-tauri
cargo test auth_provider::kimi_login
cargo check --all-targets
git diff --check
~~~

DoD：离线测试，不请求真实 Kimi；失败不返回或持久化 LoginResult。建议 commit：feat(auth): implement Kimi device OAuth and refresh。

## C4：KimiProvider、模型同步、请求上下文

**依赖：** C1、C2、C3。 **Owned files：** 新建 auth_provider/kimi_backend.rs；修改 auth_provider/mod.rs、types.rs、service.rs、codex_backend.rs、maintenance.rs、endpoint_executor/mod.rs、必要 driver 构造点及测试。 **只读依赖：** C3、Provider trait、CodecRegistry、所有 ProviderRequest 构造点。

### 接口、步骤和测试

~~~rust
struct KimiProvider { client: reqwest::Client, coding_base: String, login: KimiLogin }
struct ProviderRequest<'a> {
    is_stream: bool,
    upstream_protocol: &'a str,
    upstream_endpoint: &'a str,
}
~~~

- 生产 base 固定 https://api.kimi.com/coding；测试可注入本地 URL。
- allowlist 只有 (openai,chat_completions)、(anthropic,messages_beta)；不匹配在 HTTP 前 Protocol error。
- Chat URL /v1/chat/completions，Bearer，移除 caller x-api-key；Anthropic URL /v1/messages?beta=true，x-api-key + anthropic-version: 2023-06-01，移除 caller Authorization。
- 两个 profile 都由 Provider 覆盖全部 `X-Msh-*` 身份 headers；调用方仅能透传现有安全通用 header 白名单；Accept 按 stream 为 JSON/SSE。
- outbound/send_with_persisted_account 传递 immutable endpoint 元数据；401 replay 必须完全复用。
- models：忽略空 ID、保序、missing/kimi 规范为 kimi，anthropic 保留，未知标 unavailable + 稳定非 secret error；quota 返回 None；本卡不注册 Kimi。
- 更新所有 service、Codex、fake 的 ProviderRequest 构造点。

~~~bash
cd /Users/xian/Project/ai/WaLiAPI/src-tauri
cargo test auth_provider::kimi_backend
cargo test auth_provider::service
cargo check --all-targets
git diff --check
~~~

DoD：本地 mock 覆盖 URL/header/body/models/错误；caller header 无法覆盖；提交 4 仍未注册 Kimi。建议 commit：feat(auth): add unregistered Kimi backend and protocol-aware model discovery。

## C5：RoutePlan profile → PreparedAttempt

**依赖：** C1、C4。 **Owned files：** core/route_plan.rs、core/attempt.rs、core/protocol_boundary.rs 及直接构造/测试。 **只读依赖：** C4 snapshot/protocol、现有 tier/mapping/rng 逻辑。

### 接口和步骤

- 增加 resolve_auth_route_profile(account, requested_model)。
- RouteGroupCandidate 增加 auth_provider、native_base_url、auth_non_stream_framing；Auth candidate 必须 Some，Channel 维持旧语义。
- PreparedAttempt 复制 provider/base/protocol/endpoint/framing；不重新查 DB、不从 body/Content-Type 猜。
- direct model 读 ModelState.protocol；alias 多目标必须 profile 全同；unknown provider/protocol、缺失目标、malformed snapshot、mixed mapping fail closed。
- native/conversion tier 按 downstream/native protocol 判断；CountTokens/Embeddings 不形成 Auth group。
- messages_beta 使用现有 Messages codec；普通 Anthropic messages 不变。
- 最终随机模型若 profile 与 candidate 不同，在请求前报错；用 rg RouteGroupCandidate/PreparedAttempt 补齐所有构造。

### 测试、命令和 DoD

覆盖 Codex regression、Kimi Chat/Anthropic 三入口、unknown/mixed fail closed、alias、CountTokens/Embeddings、prefer_same_protocol、priority/weight、PreparedAttempt 无 secret 序列化。

~~~bash
cd /Users/xian/Project/ai/WaLiAPI/src-tauri
cargo test core::route_plan
cargo test core::attempt
cargo test core::protocol_boundary
cargo test auth_integration_tests
cargo check --all-targets
git diff --check
~~~

DoD：RoutePlan 是 profile 唯一来源；PreparedAttempt 有完整可信 framing；未知/混合协议不能路由。建议 commit：refactor(route): carry model-level auth profiles into prepared attempts。

## C6：executor framing 与 Kimi 注册

**依赖：** C2、C4、C5。 **Owned files：** endpoint_executor/mod.rs、endpoint_executor/driver.rs、auth_provider/mod.rs 及 executor integration/mock tests。 **只读依赖：** C5 attempt、C4 request context、stream commit barrier、codec。

### 步骤和执行分支

- ForcedResponsesSse 保留 Codex：force stream → outbound → Responses SSE accumulator → complete JSON → decode。
- Json 非流：clone body、stream=false、移除 stream_options、outbound、读取 Chat/Messages JSON、现有 decode。
- 流请求 stream=true；仅 Kimi Chat 注入 stream_options.include_usage=true；Anthropic Messages 不注入 Chat 字段。
- Anthropic beta 顶层 betas 确保包含且不重复 interleaved-thinking-2025-05-14；不可被 renderer/header/attributes 覆盖。
- executor 只读 attempt 元数据，不查 DB、不猜 framing；保留首帧 commit barrier、pre-commit failover、post-commit protocol error。
- 先完成测试，最后才在 ProviderRegistry.default 注册 Kimi。

### 测试、命令和 DoD

覆盖 Codex non-stream、Kimi Chat/Anthropic 三入口流式/非流、tool/usage/include_usage/beta/DONE、首帧和 post-commit、401 一次 replay、第二次 401 terminal、429 failover、registry。

~~~bash
cd /Users/xian/Project/ai/WaLiAPI/src-tauri
cargo test endpoint_executor::integration_tests
cargo test endpoint_executor::mock_tests
cargo test auth_provider
cargo check --all-targets
git diff --check
~~~

DoD：所有分支离线可测；Kimi 注册后未知 provider 仍 fail closed；Codex forced SSE 不变。建议 commit：refactor(executor): execute auth attempt framing and register Kimi。

## C7：Provider-neutral LoginSessions 与 Tauri commands

**依赖：** C2、C3、C6。 **Owned files：** commands/auth.rs、必要的 lib.rs、command tests。 **只读依赖：** ProviderSpec、runtime、AuthService、registry。

### 接口、步骤和测试

- AuthProviderDto：id、displayName、iconKey、loginMode、supportsImport、supportsExport、supportsQuota。
- auth_providers_list 返回 Codex/Kimi；auth_login_start(provider, replace_account_id) 使用本地 account ID。
- DeviceVerificationDto 只含 url、user_code、expires_at；status 含 provider/state/step/verification/result/error code/error。
- runner：validate registry → start → runtime → LoginTarget → authenticate → begin_save → persist → sync → finish。
- begin_save/cancel/failed/succeeded 终态清空 verification；tombstone 只留摘要。
- initial sync 失败保留账号但空 snapshot、不路由；cancel-before-save 不写库；saving/syncing cancel 返回当前状态。
- replacement 校验 provider/device ID 并更新原卡；import/export 仍仅 Codex；legacy Kimi 在网络前拒绝。

~~~bash
cd /Users/xian/Project/ai/WaLiAPI/src-tauri
cargo test commands::auth
cargo check --all-targets
git diff --check
~~~

DoD：session provider 不再固定 Codex；DTO/handler 无 payload_json；verification 正确清理；Tauri 注册编译。建议 commit：feat(auth): add provider-neutral login sessions and commands。

## C8：前端 Provider 选择与 Device Flow UI

**依赖：** C7。 **Owned files：** src/types/index.ts、src/lib/api.ts、src/pages/AuthChannelsPage.tsx、src/components/auth/ProviderPills.tsx、LoginModal.tsx、AccountCard.tsx，必要时 QuotaBlock.tsx。 **只读依赖：** C7 DTO/commands、组件状态、opener API。

### 接口、步骤和测试

TS 增加 AuthProviderInfo、DeviceVerification、AuthLoginSessionStatus，字段与 C7 camelCase DTO 一致；api 增加 providersList、loginStart(provider, replaceAccountId?)；旧 login 仅 Codex 且标 deprecated。

- ProviderPills 从 backend list 渲染；Codex/Kimi 可点击，Claude/Kiro 若保留为 disabled placeholder。
- LoginModal 按 provider 显示 Codex callback 或 Kimi device steps；verification 展示 user code、复制、URL、重新打开和勿分享提示。
- AccountCard 按 provider 图标；replacement 传 account.id；Kimi 隐藏 export/quota，Codex 保持。
- AuthChannelsPage import 文案为 Codex auth.json；删除、空槽、成功/错误文案使用 provider display name。
- 不展示任何 token/device_code/original response；关闭按钮按 session gate 门控。

~~~bash
cd /Users/xian/Project/ai/WaLiAPI
npm run build
git diff --check
~~~

DoD：Kimi 可展示/复制 URL/user code、取消可重启、无不支持入口；Codex UI 回归。建议 commit：feat(ui): add Kimi auth login and provider-aware accounts。

## C9：维护、quota、错误与安全回归

**依赖：** C3、C4、C6、C7。 **Owned files：** auth_provider/service.rs、maintenance.rs、types.rs、必要时 security/redact.rs 及测试。 **只读依赖：** registry、session tombstone、Codex quota/error/redaction。

### 步骤和测试

- Kimi quota 返回 None；不得清空其他 provider quota，也不得显示“无限”。
- maintenance 使用 Kimi refresh/models；invalid refresh 进入既有 invalid/retry。
- ProviderError 的 Display 只输出稳定类别；redaction 覆盖 access/refresh/device/user code；verification URL 不入日志。
- wire response 无普通 Debug；手写 Debug 只输出字段存在性；terminal session 清空 verification；AuthAccountDto 不含 payload。

~~~bash
cd /Users/xian/Project/ai/WaLiAPI/src-tauri
cargo test auth_provider::maintenance
cargo test security
cargo check --all-targets
git diff --check
~~~

DoD：maintenance、quota-none、invalid/retry、redaction、DTO/tombstone 全有测试；无敏感泄漏。建议 commit：test(auth): cover Kimi routing replacement refresh and protocol flows。

## C10：最终验证和 smoke test

**依赖：** C1–C9。 **Owned files：** 无实现文件，只读检查。 **只读依赖：** 全部代码、测试、原方案验收门槛。

### 自动化命令

~~~bash
cd /Users/xian/Project/ai/WaLiAPI/src-tauri
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cd /Users/xian/Project/ai/WaLiAPI
npm run build
git diff --check
git status --short
~~~

status 只允许计划内文件；不得出现 credentials、数据库、日志或真实 token fixture。

### 离线 smoke matrix

- Device flow：pending、slow_down、expired、denied、timeout、cancel、browser failure。
- Profile：missing/kimi Chat、anthropic Messages beta、unknown/mixed fail closed。
- 三下游入口 × 流式/非流式；tool call/tool result；usage；首帧 commit/post-commit。
- token 接近过期、401 一次 replay、第二次 401 terminal、rotation/replacement 并发。
- Codex 登录、import/export 和至少一个 Responses 请求。

### 真实账号 smoke（不自动化、不提交凭据）

1. 选择 Kimi，确认 URL/user code、复制和手动打开。
2. 授权后确认 provider=kimi、账号写入、models sync、snapshot protocol。
3. 重启后调用三入口流式/非流式；若目录同时有两个 profile，分别指定模型。
4. 观测 Chat 固定 /coding/v1/chat/completions，Anthropic 固定 /coding/v1/messages?beta=true，不出现 /responses。
5. 覆盖 system/user/assistant/tool call/tool result、refresh、replacement 与并发 refresh。
6. 查日志确认没有 token、code、verification URL；再执行 Codex 登录和 Responses 请求。

### 最终 DoD

cargo fmt --check、cargo clippy --all-targets --all-features -- -D warnings、cargo test、npm run build 全部实际通过；unknown/mixed fail closed；replacement/refresh 不覆盖 token；sync 失败空 snapshot 不路由；Codex 回归；无 migration、专用 codec 或敏感泄漏。建议 commit：chore(auth): verify Kimi auth end to end。

## Claude 交接格式

按 C0→C10 顺序逐卡领取。每卡汇报：修改文件、实际执行命令和结果、未解决风险、commit hash。遇到跨多个核心模块、歧义、权限/安全、并发/事务、数据迁移或架构取舍，立即停止扩展并交回主 Agent。
