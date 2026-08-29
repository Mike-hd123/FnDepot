# Auth / Codex 登录：路由与协议兼容性审查

> 来源：独立 subagent 对 `src-tauri/src` 的只读审查（分支 `v0.1.8-auth-codex`）。此文件记录「账号型上游」接入现有路由/协议机器的兼容性结论与处置。相关决策见 [ADRs.md](ADRs.md)。

## 一、结论摘要

1. **Responses 事件链 —— 兼容（有 2 个注意点）**。`convert_openai_sse_to_responses`（`protocol/responses.rs:156-683`）发出的正是 backend-api 用的事件链（`output_item.added → content_part.added → output_text.delta/done → … → output_item.done`、function_call 链、reasoning 生命周期、`response.completed`+usage）。但账号路径**根本不走这个转换器**——backend-api 本身就是 Responses 上游，Responses 下游客户端走**原生字节直通**（`SseMode::Native`，`sse.rs:22-26`）。注意：(a) 原生直通对 Responses 流取 usage 读到 0（见问题 9）；(b) codex 后端可能在 `/responses` 流里插入非标准 `codex.rate_limits` SSE 事件，严格 SDK 可能拒绝，直通不做归一化。
2. **混合候选池（渠道+账号）—— 当前不可表达（v1 阻塞项）**。候选是具体 `Channel`（`route_plan.rs:111-117`），driver 用 `HashMap<String,(Channel,ChannelIdentity)>` 且 `.expect()`（driver.rs:87-97, 106-109）。这是 v1 首要改动。
3. **模型授权 —— 大体可用，有洞**。`channel_accepts_model`（route_plan.rs:382-397）对 `account.models`（= /models 快照）做精确名匹配或空=通配。但**没有** QuotaState/失效/停用过滤，`allowed_channels` 静默排除账号。

## 二、风险清单（编号）

| # | 严重度 | 问题 | 位置 | 处置 |
| --- | --- | --- | --- | --- |
| 1 | **High** | RoutePlan 候选是具体 `Channel`，账号无法进入候选集 | route_plan.rs:111-117, 366-380, 545-642, 662-680；debug_json:705-712 | 引入 `RouteCandidate` enum（`Channel \| Account`）或 trait（id/name/priority/weight/models/model_mapping）；`RouteGroupCandidate` 持有它。priority/weight 抽样已泛化到 `HasPriorityWeight`，不用改 |
| 2 | **High** | driver `lookup` map 对非 Channel 候选硬 panic | driver.rs:87-97, 354-364, `.expect()` | map 值改 `RouteCandidate`；executor 闭包分叉到账号适配器 |
| 3 | **High** | `send_request` 以 channel 为中心，无账号头/令牌刷新钩子 | mod.rs:426-476, auth_headers:140-154, auth_scheme_for:157-166 | 在 `dispatch_executor`/`dispatch_stream_executor` 加账号分支（或 `legacy_executor_override` 风格 `"codex_account"` 选择器），带账号头 + 注入新鲜 access_token（出站前懒刷新，ADR-10） |
| 4 | **High/Med** | 账号只服务 **Responses 下游**；Chat/Messages 路由到账号会发不兼容字节 | sse.rs:22-34, 506-521；attempt.rs:287-301；route_plan.rs:444-542 | **旧决策已由 D-1 / ADR-31 取代**：v1 账号服务**全部下游**（Responses / Chat / Messages），新增 `ResponsesToChat` codec + Messages 链式路径；`classify_channel` 对账号在三种下游均出组（CountTokens/Embeddings 不出组）。`ResponsesToChat` codec 从「未来工作」改为 v1 必做 |
| 5 | **Med** | ADR-10 的 401→刷新重试同一账号在 `AttemptFlow` 里不可表达 | attempt.rs:82-94（401→ChannelAuthTerminal）, 366-398 | 刷新在**账号适配器内部**做（成功刷新后重试一次→Retryable；失败→ChannelAuthTerminal/EndpointUnsupported），不动 AttemptFlow |
| 6 | **Med** | 429→账号级 QuotaState 踢出路由（ADR-14/16）没有钩子 | attempt.rs:88（429→Retryable）；route_plan.rs:366-380 | 账号适配器解析 `x-codex-primary/secondary-*` 写 `quota_json`；`resolve_model_candidates` 增加账号过滤（跳过 Exceeded/失效/停用，镜像 `status==1`） |
| 7 | **Med** | `allowed_channels` 静默排除账号 | route_plan.rs:371-377 | **决策：账号是否受 `allowed_channels` 约束**（见决策 D-2） |
| 8 | **Med** | 账号模型授权角落：首次同步前 models 空=通配（接受任何模型）；12h 快照可能过期 | route_plan.rs:382-397 | v1 接受该行为（空=通配是现有约定）；登录时用 /models 全量填充（ADR-8） |
| 9 | **Med** | 原生 Responses 直通日志记 0 token（usage 未从 `response.completed` 提取） | sse.rs:105-136（scan_usage_from_chunk 找顶层 usage）；非流式 mod.rs:667-682 已支持 input/output_tokens | 加 Responses 专用 usage 扫描（`response.completed.response.usage`），或专用 Responses decoder。**账号特性放大了这个既有缺口** |
| 10 | **Med** | `request_logs` 无 `upstream_type`（ADR-30）；账号身份未捕获 | db/models.rs:187-227；driver.rs 三处写点；latest migration=018 | 新迁移：`auth_accounts`（ADR-3）+ `request_logs.upstream_type DEFAULT 'channel'`；三处写点带上类型 |
| 11 | **Med** | `stream:false` 到 codex 后端可能 502（后端可能忽略 stream:false 仍回 SSE） | mod.rs:519-557（非 draft_test 时 2xx 非 JSON → UndecodableBody） | **需真实令牌探测**；若后端总回 SSE：账号适配器强制 `stream:true` 或扩展 `decode_non_stream` 的 SSE 容忍分支 |
| 12 | **Med** | 字段直通：backend-api 可能拒绝下游 Responses 客户端发的字段（store/background/metadata/parallel_tool_calls/reasoning…） | attempt.rs:200-220（原生 tier 原样转发） | 账号适配器做字段 allowlist/变换（参照 `codex-rs/codex-api` 实际发送）；400/422 走 `classify_http_status`（已 CallerTerminal） |
| 13 | **Low** | zstd 压缩（ADR-7）未实现 | Cargo.toml:35（reqwest 无 zstd feature）；全仓 grep zstd=0 | 加 reqwest zstd feature（响应解码）或账号适配器手动请求体 zstd 编码；确认后端不强制 |
| 14 | **Low** | channel 措辞的错误串/日志（"No channel available"） | attempt.rs:403；route_plan.rs:188-204 | 改成 "upstream candidate" 措辞 + `upstream_type` 进 PlanExecution |

## 三、实际兼容/可复用（已验证）

- **Responses 事件链**：账号路径的 Responses 下游客户端走 `SseMode::Native` 原生直通，backend-api 自己的链直接转发；首帧校验 `validate_native_first_record`（sse.rs:89-101）接受 `event:` 前缀；EOF 恰好一次结束（sse.rs:392-397）处理 `response.completed` 后无 `[DONE]`。**Responses 下游 wire 路径真正可复用。**
- **选择语义**：`order_by_priority_weight`、组序、组预算、`AttemptFlow` 故障转移（429/5xx→下个候选、组内上限、组过渡）全部泛化，与 ADR-9/11/22 吻合——**候选类型泛化（问题 1）后重试机器不用改**。`RouteGroup` 已支持每候选 `native_base_url`（ollama_native 先例）。
- **安全审计 gate**：`security/gate.rs` 协议无关、上游无关——`audit_request` 在路由前审计下游 JSON，无 channel_id 假设。账号请求同路径流过；只有日志写需要 `upstream_type`（问题 10）。
- **URL + 主鉴权**：`final_url(base,"responses")` + `auth_headers(Bearer, token)` 已能产生正确的 `https://chatgpt.com/backend-api/codex/responses` 与 `Authorization: Bearer <access_token>`——只要适配器提供 `native_base_url` 和令牌。
- **模型直通（ADR-21）**：原生 tier 构造 body（attempt.rs:200-220）原样转发 + 替换 model——正是账号想要的行为。

## 四、v1 最需要闭合的缺口（按序）

1. `RouteGroupCandidate`/`authorize_and_plan`/driver `lookup` 接受混合 Channel+Account 候选（问题 1-2）——其它改动都依赖它。
2. 账号适配器在 `dispatch_executor`/`dispatch_stream_executor`（问题 3, 5, 13）。
3. 明确范围：账号服务**全部下游**（Responses / Chat / Messages，含新增 `ResponsesToChat` codec）——见 §六 D-1 / ADR-31，取代问题 4 的「仅 Responses」旧限缩。
4. `auth_accounts` + `request_logs.upstream_type` 迁移 + QuotaState 过滤（问题 6, 10）。

## 五、待验证（需真实令牌探测）

- backend-api 是否接受 `stream:false`（问题 11）
- backend-api 拒绝哪些请求字段（问题 12）
- `codex.rate_limits` SSE 事件是否插入 `/responses` 流（00-facts §6）

## 六、由此产生的决策（已并入 ADRs）

| 决策 | 内容 | ADR |
| --- | --- | --- |
| D-1 | v1 账号服务**全部下游**（含 Chat），新增 `Responses→Chat` codec（用户选择 B） | ADR-31 |
| D-2 | 账号**不受** `allowed_channels` 约束，由 key 配额 gate 兜底 | ADR-32 |
| D-3 | 原生 Responses 流式补 usage 提取（`response.completed.response.usage`） | ADR-33 |
| D-4 | 账号空 models = 拒绝所有（区别于渠道空=通配） | ADR-34 |
| D-5 | 账号 401 刷新重试在适配器内部，AttemptFlow 无感知 | ADR-35 |
| D-6 | 账号强制流式，非流式下游内部缓冲 | ADR-36 |
| D-7 | 账号出站请求体字段 allowlist/变换 | ADR-37 |

> 注：兼容性审查问题 1-3（混合候选池、driver lookup、账号 executor 分支）为**实现必须项**（RouteCandidate enum/trait + 账号适配器），已在 ADR-6/7/9/11/22 隐含，实现时按 §四 顺序落地。
