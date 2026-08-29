# Auth / Codex 登录：ADR

> 决策随 grilling 逐条落地。格式参考 `docs/channel-refactor-tasks/00-architecture-decisions.md`（T00 架构决策冻结）。
> 每条 ADR：背景 → 决策 → 理由 → 影响。事实底座见 [00-facts.md](00-facts.md)，**UI 设计规格见 [01-ui-spec.md](01-ui-spec.md)**（含原型 `prototype.html` 的布局/组件/交互）。

## ADR-1：Auth 定位 = 账号即上游（account-as-upstream）

- **决策**：登录后的 codex 账号被 WaLiAPI 视为一个上游渠道——网关用 OAuth 令牌代表用户访问 ChatGPT 后端（backend-api），**消耗的是 ChatGPT 订阅额度**，不依赖 OpenAI 平台 API key。
- **理由**：贴合「渠道管理」语境；现有 `protocol/` 流式转换桥（Responses↔Chat↔Anthropic）已能消化 Codex 的 Responses 流。
- **影响**：账号需要具备 Channel 类似的路由能力（模型授权、优先级、统计、审计），但来源凭据是 OAuth 令牌而非静态 key。

## ADR-2：codex 登录执行方式 = 应用内嵌 OAuth PKCE + localhost 回调（优先），导入 auth.json 兜底

- **决策**：首选在 WaLiAPI 内嵌完整浏览器 OAuth 流（系统浏览器 + `localhost` 回调 + PKCE S256，复刻 `codex login` 默认行为，`client_id=app_EMoamEEZ73f0CkXaXp7hrann`，刷新端点 `https://auth.openai.com/oauth/token`）。同时支持从本机已有 `~/.codex/auth.json` 一键导入。
- **理由**：用户无需安装 codex CLI；本机已登录时导入成本最低。二者互不排斥。
- **影响**：需内置 PKCE/回调服务器/令牌交换逻辑；导入路径需解析 auth.json 结构并做校验（access/refresh/account_id）。

## ADR-3：令牌持久化 = WaLiAPI DB 通用两层结构（升级为 CPA 式）

- **决策**：新增 `auth_accounts` 表，采用**通用列 + provider 载荷**两层结构（对齐 CPA 的 `coreauth.Auth` + `TokenStorage` 思想，落到我们的 SQLite 仓储）：
  - **通用列**：`id / provider / label / account_id / status / disabled / priority / weight / quota_json / model_states_json / attributes_json / last_refreshed_at / next_refresh_after / next_retry_after / created_at / updated_at`。
  - **payload_json**：存 provider 特有令牌载荷（codex 的 `access_token / refresh_token / expires_at` 等）。claude/kiro/kimi 新增 provider 时**表结构零改动**，只需新增 provider 的 payload 解析。
- **理由**：用户要求「考虑后面的新 provider，基于 codex 设计的表不通用」；CPA 调研（00-facts §5）证实通用记录 + provider 载荷是经实践检验的模型；我们已有 SQLite 仓储，比 CPA 的每 auth 一文件更内聚。
- **影响**：新迁移（`auth_accounts`）、新 repository 层；token 加密、限额字段（quota_json）、模型状态（model_states_json）复用通用列。

## ADR-4：前端拆分 = 两个独立路由 `/channels`(API) + `/channels/auth`(Auth)

- **决策**：`/channels` 保持现状内容（更名为 API 管理），新增独立路由 `/channels/auth` 承载 Auth 账号管理；侧边栏「渠道」入口指向 `/channels`，Auth tab 通过 `/channels/auth` 访问。复用现有 SettingsPage 式 hash tab 或 route-driven tab 均可，但页面实体是两个路由。
- **理由**：路由可寻址、结构清晰、与 KnowledgeBasePage 的多路由 tab 模式一致。
- **影响**：`App.tsx` 加路由；ChannelsPage 顶部加分段控件跳转；Auth 页为独立组件。

## ADR-38：删除账号 = 纯本地移除，v1 不做远端 revoke

- **决策**：`auth_logout`（删除账号）v1 **只做本地移除**——删除 `auth_accounts` 行与模型快照，账号退出路由；**不调用 provider 的 `oauth/revoke` 端点**，不影响已写回 `~/.codex/auth.json` 的本机 Codex CLI 登录态。删除弹窗只问「是否删除」：`取消` / `确认删除`，无 revoke 选项；若检测到本机 auth.json 含该账号副本，提示可另行 `codex logout` 注销。
- **理由**：CPA 源码复核确认删除即本地移除（00-facts §5.3，`deleteAuthFileByName` → store `Delete`，无任何 `oauth/revoke` 调用）；自动 revoke 会把写回 CLI 的同一会话一并注销，误伤正在使用的 Codex CLI 登录；远端 revoke 能力依赖 provider 端点，v1 不承担该不确定性。
- **影响**：`auth_logout` 无「revoke 失败」路径，返回被删账号摘要；删除弹窗文案定为「是否删除该账号？删除后此账号不再参与路由。仅从本应用移除，不影响本机 Codex CLI 登录态。」；修订 ADR-20 的 `auth_logout` 语义、02-design §8 命令表与 §9.3 弹窗、01-ui-spec §4.4 删除弹窗、00-optimized-requirements 与 03-task-breakdown 验收标准。

## ADR-37：账号出站请求体 = 字段 allowlist/变换

- **决策**：账号适配器对发往 backend-api 的请求体做**字段 allowlist/变换**——参照 `codex-rs/codex-api` 实际发送的字段，过滤/改写后端不接受的字段（如 `store`/`background`/`metadata`/`parallel_tool_calls`/`reasoning` 等）。400/422 走 `classify_http_status`（已是 `CallerTerminal`，诚实报错给下游）。
- **理由**：现有原生 tier 原样转发（attempt.rs:200-220），后端可能拒绝部分公开 Responses 字段；allowlist 可基于 codex-rs 源码确定，不依赖真实令牌。
- **影响**：账号适配器持请求体变换逻辑；`Responses→Chat` codec（ADR-31）与 allowlist 分层，转换在前、过滤在后。

## ADR-36：账号强制流式，非流式下游内部缓冲

- **决策**：账号适配器对**所有**下游请求按 `stream:true` 打到 backend-api（codex CLI 本身总是流式，后端对 `stream:false` 支持不可靠）。下游请求非流式时，适配器把流式响应缓冲成非流式 JSON 返回。
- **理由**：规避后端对非流式行为的不确定性（审查问题 11）；codex 总是流式是已确认事实；缓冲逻辑简单。
- **影响**：账号适配器统一 `stream:true`；非流式分支做流→JSON 缓冲；`decode_non_stream` 不需为账号扩展。

## ADR-35：账号 401 刷新重试 = 适配器内部，AttemptFlow 无感知

- **决策**：账号 401 → 适配器内部刷新令牌 → 成功则**同一账号静默重试一次**，AttemptFlow 无感知（不占候选、`is_retry` 不因此置位）；刷新失败 → 返回让 AttemptFlow 走下一个候选（另一账号或普通渠道）。与 ADR-10 的「出站前懒刷新」同一思路。
- **理由**：令牌刷新是出站前准备性质，不是上游失败重试；隐藏进适配器避免污染 AttemptFlow 的重试语义和 attempt 计数。
- **影响**：账号适配器持有一个「刷新+重试一次」的内部循环；AttemptFlow 照常处理其它失败类。（见 `02-routing-compat-review.md` 问题 5）

## ADR-34：账号空模型 = 拒绝所有（区别于渠道空=通配）

- **决策**：账号 `models` 为空（首次同步前或同步失败后）时，**账号不参与任何模型的路由**（拒绝所有请求）。与普通渠道的「空=通配」语义明确区分。
- **理由**：账号模型列表来自上游 `/models`（ADR-8），未同步前无法可靠判断可用模型；通配会把请求路由到后端不支持的模型导致 400。安全优先。
- **影响**：`resolve_model_candidates` 对账号候选：models 空则跳过；登录成功即同步模型（ADR-8）使该窗口极短。

## ADR-33：原生 Responses 流式补 usage 提取

- **决策**：为原生 Responses 直通（含账号路径）新增 Responses 专用 usage 扫描——从 `response.completed.response.usage` 读 `input_tokens`/`output_tokens` 写入 `request_logs` 与 key 配额统计。**修复既有 0 token 缺口**（`scan_usage_from_chunk` 只找顶层 usage，sse.rs:105-136；非流式 mod.rs:667-682 已支持）。
- **理由**：账号订阅额度/日志统计依赖；低成本既有修复，随账号功能交付。
- **影响**：`SseMode::Native` 或专用 Responses decoder 增加 usage 提取；`StreamPumpCore.usage()` 返回真实值；driver 配额累加生效（此前 `usage_total>0` 才累加，driver.rs:739-743）。

## ADR-32：账号不受 allowed_channels 约束

- **决策**：API key 的 `allowed_channels` 过滤**不约束账号候选**——账号由调用方 key 的配额/权限 gate 兜底。`resolve_model_candidates` 中 `allowed_channels` 仅过滤普通渠道；账号候选跳过该过滤。
- **理由**：账号是独立实体（ADR-6），订阅额度属强资源由 key 配额控制；给 key 选账号交互复杂且收益低。
- **影响**：路由过滤需识别候选类型——账号候选不查 allowed_channels；日志/审计照常。（见 `02-routing-compat-review.md` 问题 7）

## ADR-31：账号服务全部下游协议（含 Chat）— 新增 Responses→Chat codec

- **决策**：账号型上游 v1 服务**所有下游协议**（Responses / Chat / Messages）。这意味着新增 **`Responses→Chat`** SseMode + codec（上游 backend-api 的 Responses SSE → 下游 Chat SSE），以及对应的 Messages 方向能力。账号候选在 `classify_channel` 中对 Chat/Messages/Responses 均返回有效分组。
- **事实核查**：现有 `convert_openai_sse_to_responses`（`protocol/responses.rs:156-683`）是 **Chat→Responses** 方向（`ResponsesViaChat`，上游 Chat、下游 Responses）；codec registry（`protocol/codec/registry.rs:112-119`）只注册 `Chat↔Messages` 两方向，`SseMode`（`endpoint_executor/sse.rs:22`）无 `ResponsesToChat`。**反向（Responses→Chat）需新增**。
- **理由**：用户确认 B 并指出已有协议转换基础。Responses 事件链结构已被现有转换器完整实现并有测试锁定（`responses.rs:1214-1633`），sse_bridge 的帧重组/tool 累积/usage 合并/恰好一次结束均可复用——反向 codec 是增量而非从零。
- **影响**：新增 `ResponsesToChat` 转换（状态机 + 帧重组 + usage 提取）；`SseMode` 增 `ResponsesToChat`；`classify_channel` 对账号在三种下游都出组；兼容性审查问题 4 由「范围限制」转为「新增 codec」。（见 `02-routing-compat-review.md` D-1 修订）

## ADR-30：请求日志区分 = upstream_type 列

- **决策**：`request_logs` 新增 `upstream_type` 列（`channel` / `auth_account`），账号型请求落 `auth_account`，`channel_id` 列填账号 id、`channel_name` 填账号名；普通渠道落 `channel`。日志页/统计按 `upstream_type` 区分两类上游。
- **理由**：账号是独立实体（ADR-6）不落 channels 表，日志需可区分；加一列最干净，日志页可过滤。
- **影响**：新迁移加列（默认 `channel` 兼容旧数据）；写入点在 Attempt 记录时带上类型。

## ADR-29：风险提示 = Auth tab 固定警示条

- **决策**：Auth tab 顶部（provider 一排下）常驻警示条，固定文案：
  > ⚠️ 风险提示：此提供商使用的订阅 / OAuth 会话未获官方授权用于代理 / 路由器使用。账户可能被限制或封禁。使用风险自负。
- **理由**：订阅账号代理属官方未授权用法，需明确告知用户风险，合规且保护用户知情权。
- **影响**：前端 Auth 页固定渲染，不随 provider 增减；文案入库（不做 i18n 变量）。

## ADR-28：限额缺失 = 不显示、不受限（动态兼容）

- **决策**：账号限额信息完全依赖上游返回（ADR-14 动态兼容）——**有则显示/参与限额路由，无则视为无限额**：
  - 上游响应返回限额字段（如 codex 的 `x-codex-primary/secondary-*`）→ 卡片显示进度条，参与限额退出/恢复（ADR-15/16）；
  - 上游不返回任何限额字段 → 卡片不显示限额块，路由**不因限额退出**；
  - 空窗口（仅 `used-percent:0`、无时长无重置）同样不渲染限额块——上游没有给出真实限额；
  - 无论有无返回，运行时实际撞 429 仍走 cooldown 退避兜底（ADR-14）。
- **理由**：不写死任何 provider 的窗口类型（codex 实际无 5h 窗口——free=30 天月限额、非 free=周限额，由 `window-minutes` 动态推导）；上游返回什么兼容什么。
- **影响**：`auth_accounts.quota_json` 无限额时为 null/空；前端有值且非空窗口才渲染限额条。

## ADR-27：账号编辑 = 名称 / 优先级 / 权重（弹窗）

- **决策**：账号卡片动作列含**编辑**按钮（✎），打开编辑弹窗修改：账号名称（默认取推断名如「Codex · ChatGPT」）、优先级 priority、权重 weight（ADR-26）。名称存通用列 `label`，优先级/权重存 `priority / weight`。对齐 API 渠道的编辑体验（不再单设内联改名）。
- **理由**：与渠道卡片一致的三件套（编辑/启停用/删除）；多账号需可区分的自定义名与可调权重。
- **影响**：`auth_accounts` 通用列 `label / priority / weight`；编辑动作对应一条命令（`auth_update`）。

## ADR-26：账号权重/优先级 = 登录默认 + 前端可调

- **决策**：账号登录成功默认 `priority=0, weight=1`（与新建渠道默认一致），账号卡片上可调整优先级/权重（对齐现有渠道编辑）。导入 auth.json 时若带权重字段则继承。
- **理由**：账号与渠道同为普通候选（ADR-9/11），权重/优先级可调才能实现「订阅优先 vs API key 优先」的调度意图；对齐渠道编辑体验成本低。
- **影响**：`auth_accounts` 通用列含 `priority / weight`；前端账号卡片提供编辑。

## ADR-25：失效账号 = 后台自动重试刷新 + 手动重登引导

- **决策**：账号置失效后：
  - **后台自动恢复**：定时刷新（12h）时对失效账号尝试用 refresh_token 刷新；refresh 仍有效则自动恢复，无需用户操作。
  - **前端呈现**：账号卡片显示"已失效（令牌过期）"+「重新登录」按钮，点击走 ADR-2 的 OAuth 流重新授权。
- **理由**：refresh_token 通常比 access_token 有效期长，失效多为 access 过期——后台重试能自动救回；手动重登兜底 refresh 也失效的情况。
- **影响**：定时刷新任务含失效账号重试分支；前端失效态 + 重登动作。

## ADR-24：导入 auth.json = 校验 + 提示本机 codex 状态

- **决策**：`auth_login_import` 校验 `auth.json` 字段齐全（auth_mode / tokens / account_id）且令牌未过期，读入 DB（Q23 覆盖刷新）。导入完成后**提示用户**：本机 codex 仍登录此账号，WaLiAPI 与 codex 令牌不自动同步（ADR-18 手动写回、ADR-19 不联动）。
- **理由**：导入只需字段级校验；提示避免"codex 那边令牌怎么没更新"的困惑。
- **影响**：导入成功返回时附状态提示文案；前端展示。

## ADR-23：登录/导入冲突 = 同 account_id 覆盖刷新

- **决策**：登录或导入时以 `account_id`（本机确认是稳定 UUID）为去重键——同 `account_id` 已存在则**覆盖刷新**该账号（更新 access/refresh/id_token/email/限额/时间戳），**保留账号 id、权重、优先级、启停状态**；不存在则新建。
- **理由**：account_id 是稳定键；同账号重新登录/导入是「续期」语义；保留路由配置避免用户重配。
- **影响**：`auth_login` / `auth_login_import` 先查 account_id，命中则 UPDATE 而非 INSERT。

## ADR-22：账号出站失败 = 与普通渠道同级降级

- **决策**：账号出站失败时，自动按现有 RoutePlan 降级逻辑试下一个候选——下一个候选可能是同 provider 的其他账号，也可能是普通渠道。账号与普通渠道同级，不做区别对待。
- **理由**：ADR-9 已定账号是普通候选、Q9 同池路由；降级语义应与普通渠道一致。「订阅优先 vs API key 优先」由权重/优先级表达，不由降级结构表达。
- **影响**：RoutePlan 重试循环把账号失败纳入与渠道同级的候选替换；失败计数、审计照常。

## ADR-21：账号 v1 不支持模型映射

- **决策**：账号型上游 v1 **不做 model_mapping**。下游请求的模型名直接透传给账号，账号按上游 `/models` 支持的模型名处理。Q8 的「只能看不能选、默认全部支持」不受影响。
- **理由**：用户确认 v1 先不支持；账号全量支持模型，映射与其方向不符；透传最简单。
- **影响**：`auth_accounts` 无 model_mapping 字段；路由层账号候选不做映射转换。未来需要时走 ADR 补充。

## ADR-20：Tauri 命令集

- **决策**：后端为 Auth 提供以下命令（前端 `invoke` 用）：
  - `auth_accounts_list` — 列所有账号（通用字段 + provider 载荷摘要，不含令牌明文）
  - `auth_login` — 启动 OAuth 登录（provider 参数），完成后入库
  - `auth_login_import` — 从 `~/.codex/auth.json` 导入（ADR-2 C 路径）
  - `auth_logout` — 删除账号（**纯本地移除**，v1 不做远端 revoke，见 ADR-38）
  - `auth_refresh_token` — 手动刷新某账号令牌
  - `auth_sync_models` — 手动拉取某账号模型列表（ADR-8）
  - `auth_write_back` — 手动写回 `~/.codex/auth.json`（ADR-18）
  - `auth_toggle` — 启停用账号
  - `auth_quota_status` — 查询某账号限额/恢复点
  - `auth_update` — 编辑账号（label / priority / weight，ADR-27）
- **理由**：覆盖登录/导入/删除/刷新/同步模型/写回/启停用/限额展示/编辑全部核心操作。
- **影响**：注册进 `lib.rs` invoke_handler；每个命令映射 repository + provider trait。

## ADR-19：Auth 与 UsagePage Codex 配置暂不联动

- **决策**：Auth 账号功能与现有 `UsagePage` 的 Codex 配置（`write_codex` → `~/.codex/config.toml`）**互不联动**。用户自行手动切换 codex 使用 WaLiAPI（在 config.toml 里选 waliapi provider）。Auth 不触碰 config.toml 逻辑。
- **理由**：两者解决不同问题（Auth=网关侧账号；Codex 配置=CLI 侧指向网关）；用户明确「先让用户手动切 codex 使用 WaLiAPI」，v1 不自动化联动。
- **影响**：不动 `app_config.rs` 的 `write_codex`；Auth 只做账号登录/存储/路由/限额 + 手动写回 auth.json（ADR-18）。

## ADR-18：回写本地 Codex CLI = 手动触发

- **决策**：不在登录/刷新时自动写 `~/.codex/auth.json`；在账号卡片上提供**「写回本地 Codex CLI」**按钮，用户手动点击才把该账号的令牌写成 codex 格式到 `~/.codex/auth.json`。避免自动覆盖本机已有 codex 登录态。
- **理由**：用户选择手动（D）；自动写会静默覆盖 CLI 登录态。
- **影响**：账号卡片多一个「写回」动作；写回前可提示将覆盖现有 auth.json。

## ADR-17：Auth tab UI = 账号卡片列表，界面细节延后

- **决策**：`/channels/auth` 用账号卡片列表（对齐 ApiKeysPage 卡片模式），展示 provider / 邮箱昵称 / 计划 / 模型数 / 限额状态 / 下次恢复时间；操作：登录、删除、手动刷新令牌、手动同步模型、启停用。**UI 视觉细节本版本后置，先交付核心功能**（登录、存储、路由、限额）。
- **理由**：用户明确「UI 界面后面再来设计，先核心功能」。
- **影响**：v1 先做可用的卡片列表；视觉打磨留到 UI 专项。

## ADR-16：限额退出粒度 = 账号级

- **决策**：限额触发后**整个账号踢出路由候选**（账号所有模型一起不可用），`QuotaState.Exceeded` 置位 + `NextRecoverAt`；恢复时账号整体回到候选。不按模型分别标记不可用。
- **理由**：限额（free=月、非 free=周，均为账号/订阅级）退出粒度与之对齐；实现简单、语义清晰。
- **影响**：`QuotaState` 是账号级字段；模型状态列 `model_states_json` 可仅作展示（ADR-3 已含），不承担路由分派。

## ADR-15：限额更新 = 每次请求响应头解析（主路径）+ 空闲 30min 主动探测兜底

- **决策**：限额更新的两层机制，**不做有流量时的定时探测**：
  1. **请求响应头解析（主路径）**：codex 每个响应头都返回 `x-<limit_id>-<primary|secondary>-*` 限额（used-percent / window-minutes / reset-at）。窗口类型由 `window-minutes` 决定，**不写死**（实测 free=30 天月限额、非 free=周限额；`secondary` 常为空，仅 `used-percent:0` 的空窗口按 `has_data` 规则丢弃）。有流量时**每次出站响应即解析更新** `QuotaState`，天然最新，无需额外定时。
  2. **空闲主动探测（兜底）**：仅当**无流量**时每 **30 分钟**主动查一次限额，防止闲置期间限额恢复/耗尽被漏过。
- **理由**：用户指出「codex 有返回就不用有流量 5min 更新一次」——响应头已覆盖活跃场景，定时探测在有流量时是重复劳动；只在空闲时兜底。
- **影响**：出站响应解析限额头（参考 codex `parse_rate_limit_for_limit`）；空闲探测仅无流量时触发。

## ADR-14：provider 限额 = 完全按上游动态解析 + 429/Retry-After 兜底

- **决策**：限额处理分层：
  1. **动态解析（主路径）**：不假设任何 provider 的窗口类型——解析响应头 `x-<limit_id>-<primary|secondary>-*`，窗口类型由 `window-minutes` 推导，前端只识别三种标签：**5H限额 / 周限额 / 月限额**（codex 实际无 5h 窗口，free=30 天月限额、非 free=周限额）；仅 `used-percent:0` 的空窗口按 `has_data` 规则丢弃；
  2. **运行时动态兼容（兜底）**：遇 429/限额错误，解析 `Retry-After` 头或错误体中的限额字段更新 `QuotaState`；有响应则更新恢复时间，未知/缺字段则回退到指数退避。**上游返回什么就兼容什么**；
  3. **自动恢复**：每 **30 分钟**检查 cooldown 状态，到期账号自动回到路由候选。
- **理由**：用户确认「codex 没有 5 小时限额，free=月限额、非 free=周限额，限额应以实际返回为准」；CPA 的运行时标记 + 恢复模型为基底，动态解析天然适配任何窗口。
- **影响**：`QuotaState` 通用列承载恢复点/退避；窗口类型不落 provider 定义，完全按 `window-minutes` 动态推导；响应解析"有就更新、无则退避"。

## ADR-13：存储模型 = 通用列 + payload_json（CPA 式两层结构）

- **决策**：`auth_accounts` 采用「通用列 + provider 载荷」两层结构——通用路由/展示字段落通用列（`id / provider / label / account_id / status / disabled / priority / weight / quota_json / model_states_json / attributes_json / last_refreshed_at / next_refresh_after / next_retry_after / created_at / updated_at`），provider 特有令牌数据（codex 的 `access_token / refresh_token / expires_at` 等）存 `payload_json`。
- **理由**：早期作为独立决策条目编号引用，正文未单独成篇；其内容与 ADR-3 完全重合（ADR-3 已完整记录该决策）。
- **影响**：以 ADR-3 为准，本号视为 ADR-3 的同义引用；不引入独立约束。设计落点见 `docs/auth-codex/work/02-design.md` §2。

## ADR-12：provider 抽象一步到位，codex 为首个实现

- **决策**：`auth_accounts.provider` 走枚举；登录、令牌刷新、出站三块均抽象为 `Provider` trait/接口。codex 是首个实现（OAuth PKCE + backend-api adapter）；claude / kiro / kimi 后续只新增实现类，不改表结构与路由层。
- **理由**：登录/刷新/出站三块差异天然适合 trait 抽象（OAuth 端点、令牌结构、backend 均不同）；现在抽成本低，避免「provider 字段有了但逻辑没抽」。
- **影响**：新增 `auth_provider` 模块（trait + codex impl）；`auth_accounts.provider` 驱动 trait 分派。

## ADR-11：同 provider 多账号 = 每账号独立候选，weight 抽样

- **决策**：同 provider 的多个账号各自作为独立候选，复用现有「同 priority 按 weight 无放回抽样」，实现多账号负载均衡，无新增轮询/主备逻辑。
- **理由**：与 ADR-9「账号是普通候选」一致，零新增状态；账号额度差异用 weight 表达。
- **影响**：`auth_accounts` 每行即一个候选；`weight` 字段参与抽样。

## ADR-10：令牌刷新 = 12h 定时兜底 + 出站前懒刷新 + 401 重试

- **决策**：令牌刷新分三层：
  1. **定时任务**：每 **12h** 批量刷新所有账号的 access_token（兜底，确保令牌不因闲置过期）；
  2. **懒刷新**：出站前检查令牌是否临近过期，临近则先刷新再发请求；
  3. **错误触发**：出站收到 401/令牌失效时触发一次刷新并重试；刷新失败则账号置为失效（需重新登录），路由跳过该账号。
- **理由**：Tauri 常驻但无高频请求保证，纯定时会空转；纯惰性会漏掉长时间闲置后令牌恰好过期的情况。三层互补。
- **影响**：刷新失败 = 账号失效态；失效账号被路由跳过（Q9 的普通候选语义 + 现有降级逻辑自动把流量转给其他候选）。

## ADR-9：账号在路由层是普通候选

- **决策**：账号型上游作为与普通渠道地位对等的候选，进入现有 RoutePlan 选择器（协议原生组 → priority tier → 同 priority 按 weight 无放回抽样）。账号不与普通渠道区分优先/降级。
- **理由**：符合 ADR-1「账号即上游」；「订阅额度优先 vs API key 优先」是权重/优先级配置问题，不是路由结构问题。
- **影响**：RoutePlan 候选集 = channels ∪ auth_accounts，二者共享同一选择逻辑与审计/统计。

## ADR-8：账号模型列表 = 只读、全量支持、自动+手动同步

- **决策**：账号可用模型由上游 `/models` 决定，**默认全部支持、只读不可勾选**。同步策略：
  - 登录成功后自动拉取一次；
  - 之后每 **12 小时**自动拉取；
  - 支持手动刷新。
  模型列表仅作展示与路由授权依据，UI 不能选择/过滤。
- **理由**：用户明确「只能看不能选，默认全部支持」；订阅账号的可用模型随计划变化，全量支持最简单且不违背意图；12h 周期避免频繁命中后端。
- **影响**：`auth_accounts` 存模型列表快照 + `last_models_sync_at`；一个定时任务/惰性过期检查每 12h 刷新；`GET /models` 失败时保留旧快照。
- **影响（聚合接口）**：对外 `GET /v1/models` 同时聚合渠道模型与 Auth 账号模型（`available` 且未 `unavailable` 的快照条目 + `model_mapping` 源别名），统一去重、渠道优先；禁用 / 非活跃账号不参与聚合。详见 ADR-6 的路由候选并列兼容。

## ADR-7：账号出站走薄 backend-api 适配器，复用现有 Responses 桥

- **决策**：新增账号适配器直连 `https://chatgpt.com/backend-api/codex` 的两个端点：`POST /responses`（推理，Responses wire）+ `GET /models`（模型列表）。适配器只承担账号特有逻辑：OAuth 令牌头（Bearer access_token）、`x-openai-actor-authorization`、可选 session/thread/subagent 头、zstd 压缩。下游流式事件复用现有 `protocol/responses.rs` / `sse_bridge.rs` 转换管线。
- **理由**：backend-api 特有头/session/压缩细节无法被普通 OpenAI 出站完全覆盖；转换侧复用现成 Responses 桥，新代码集中在账号出站。
- **影响**：模型列表来源 = 账号的 `GET /models`（ADR-7 事实）；适配器需处理 zstd、actor 头。

## ADR-6：账号型上游在路由层与普通渠道并列兼容

- **决策**：路由层同时识别两类上游候选——**API（普通 channels）** 与 **Auth（auth_accounts 账号）**。账号是独立路由实体（不落进 channels 表），在路由选择时与普通渠道一起参与候选（模型授权、优先级/权重、审计、统计）。
- **理由**：用户明确选择「路由层兼容两种」；账号语义（OAuth 令牌、订阅额度、可多账号）与静态 key 渠道不同，独立实体更干净；路由选择器统一抽象「上游候选」。
- **影响**：RoutePlan 的候选来源扩展为 channels + auth_accounts 两组；模型授权需同时覆盖两类；账号渠道暴露后端协议能力（见 Q7）。

## ADR-5：v1 支持同 provider 多账号并存

- **决策**：schema 以 `provider` 维度组织，且**同一 provider 允许多个账号**（如多个 ChatGPT 账号）。
- **理由**：用户明确选择多账号；路由层按账号做选择时具备更多自由度。
- **影响**：`auth_accounts` 表可存多行同 provider；路由/负载需在账号间选择（后续决策）。

## 决策索引（ADR-1 ~ ADR-37）

> 注：ADR-13 正文为 ADR-3 的同义引用（原编号正文缺失，复核已补）。正文编号在文件内非升序排列（按追加时间倒序），以索引为准。

| ADR | 主题 |
| --- | --- |
| 1 | Auth 定位 = 账号即上游（消耗订阅额度） |
| 2 | codex 登录执行 = 内嵌 OAuth PKCE+localhost 回调，导入 auth.json 兜底 |
| 3 | 令牌持久化 = DB 通用两层结构（通用列 + payload_json） |
| 4 | 前端拆分 = /channels（API）+ /channels/auth（Auth） |
| 5 | 同 provider 多账号并存 |
| 6 | 路由层兼容 API 渠道 + Auth 账号 |
| 7 | 账号出站 = 薄 backend-api 适配器，复用 Responses 桥 |
| 8 | 账号模型列表 = 只读、全量支持、自动+手动同步 |
| 9 | 账号在路由层是普通候选 |
| 10 | 令牌刷新 = 12h 定时兜底 + 懒刷新 + 401 重试 |
| 11 | 同 provider 多账号 = 每账号独立候选，weight 抽样 |
| 12 | provider 抽象一步到位，codex 为首个实现 |
| 13 | 存储模型 = 通用列 + payload_json（CPA 式两层结构，ADR-3 同义） |
| 14 | provider 限额 = 完全按上游动态解析 + 429/Retry-After 兜底 |
| 15 | 限额更新 = 每次请求响应头解析 + 空闲 30min 探测兜底 |
| 16 | 限额退出粒度 = 账号级 |
| 17 | Auth tab UI = 账号卡片列表，界面细节延后 |
| 18 | 回写 codex CLI = 手动触发 |
| 19 | Auth 与 UsagePage Codex 配置暂不联动 |
| 20 | Tauri 命令集 |
| 21 | 账号 v1 不支持模型映射 |
| 22 | 账号出站失败 = 与普通渠道同级降级 |
| 23 | 登录/导入冲突 = 同 account_id 覆盖刷新 |
| 24 | 导入 auth.json = 校验 + 提示本机 codex 状态 |
| 25 | 失效账号 = 后台自动重试刷新 + 手动重登引导 |
| 26 | 账号权重/优先级 = 登录默认 + 前端可调 |
| 27 | 账号编辑 = 名称 / 优先级 / 权重（弹窗） |
| 28 | 限额缺失 = 不显示、不受限（动态兼容） |
| 29 | 风险提示 = Auth tab 固定警示条 |
| 30 | 请求日志区分 = upstream_type 列 |
| 31 | 账号服务全部下游协议（含 Chat）— 新增 Responses→Chat codec |
| 32 | 账号不受 allowed_channels 约束 |
| 33 | 原生 Responses 流式补 usage 提取 |
| 34 | 账号空模型 = 拒绝所有（区别于渠道空=通配） |
| 35 | 账号 401 刷新重试 = 适配器内部，AttemptFlow 无感知 |
| 36 | 账号强制流式，非流式下游内部缓冲 |
| 37 | 账号出站请求体 = 字段 allowlist/变换 |
