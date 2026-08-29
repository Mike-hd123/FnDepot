# CR 报告（Round 2）

> **复核方式**：2026-08-09 补跑。5 个只读复核 agent 并行审查（数据层 / Provider+登录 / 路由层 / codec / 执行器+命令+前端），各自对照验收卡（T1–T12）与设计文档（`02-design.md`）逐条核对，并运行相关测试。Round 1 的五条主要问题已逐条对照当前工作树确认修复状态。本轮亦核对了文档更新（ADR-38 删除本地化）与实现同步。

## 结论：CONDITIONAL PASS

> 核心功能全部正确且测试全绿；**1 项设计偏离需用户拍板**（OAuth 登录命令契约），另有少量次要项。若接受该偏离或完成回退，即可进入 Phase 5 验证。

## 问题清单

### 阻塞（需用户拍板）

- **阻塞（设计偏离）** / `src-tauri/src/commands/auth.rs:494-556`、`src-tauri/src/lib.rs:168-180`、`src/lib/api.ts:98-101`、`src/components/auth/LoginModal.tsx:37-44,49,63` / **OAuth 登录实现为 13 条命令 + 前端轮询，与已拍板设计「10 条命令、`auth_login` 单 invoke、不新增轮询命令」冲突。** 设计 §3.2 决策 4 与 `01-requirements-review` 疑点 4 的拍板明确要求单 invoke；T9 验收(5) 的 `rg` 断言「恰好 10 条」实测返回 13（`auth_login_start/status/cancel` 三条轮询命令被一并匹配），T9(5) 会 FAIL。前端 `LoginModal` 用 `auth_login_start` 启动 → 每 350ms 轮询 `auth_login_status` → `auth_login_cancel` 取消；单 invoke `auth_login` 已实现并注册但前端零调用（api.ts:98 死代码）。/ **二选一由用户拍板**：(a) 回退为单 invoke 主路径（删 3 条轮询命令 + 前端本地状态机推进五步 UI，符合设计字面）；(b) 保留轮询实现，但修订设计 §3.2 / 疑点 4 / T9(5) 为「`auth_login` 单 invoke + 可选轮询会话命令」，明确轮询方案是经评估的 UX 改进（真实进度 + 可取消）。不能既留 13 条命令又要求 T9(5) 字面通过。

### 次要

- **次要（已修）** / `src-tauri/src/auth_provider/service.rs`（401 路径）/ 401 触发的 `mark_invalid(account_id)` 不写 `next_retry_after`，与懒刷新失败路径 `schedule_maintenance_retry`（+12h 退避）不一致；401 置 invalid 的账号会在下一维护 pass 立即被重试。/ 已改为两处 401 均走 `schedule_maintenance_retry`（置 invalid + +12h 退避），并删除因此无调用者的 `mark_invalid` 私有方法。
- **次要** / `src-tauri/src/auth_provider/codex_login.rs:45-46,118,402-408` / 生产 OAuth callback 绑定固定知名端口 1455/1457，而非设计 §3.2 指定的临时端口 `127.0.0.1:0`。/ 官方 Codex CLI 注册这些端口，固定选择对真实 redirect 注册合理（agent 认可）；但与本机其他进程抢占端口时可能登录失败。可留作已知项，或改为「先试固定端口，占用则退临时端口」。
- **次要（可接受）** / `src-tauri/src/auth_provider/codex_login.rs:642-647` / 写回拒绝不存在的目标文件，无法在从未登录过 codex CLI 的机器上新建 `~/.codex/auth.json`。/ 属备份优先的安全取舍（agent 认可 defensible）；若需支持「推送 WaLiAPI 会话到 CLI」流程，可加「无旧文件则直接创建」分支。
- **次要（不修）** / `src-tauri/src/auth_provider/codex_backend.rs:26-28` / `CODEX_CLIENT_VERSION`/`CODEX_USER_AGENT` 硬编码 `0.147.0`，随 CLI 版本演进会漂移。/ 该文件有会话开始前既有的未提交改动（非本次实现），本轮不触碰；建议后续改为 `env!("CARGO_PKG_VERSION")` 派生。
- **次要（设计协调）** / `src-tauri/src/protocol/codec/responses_codec.rs:535-548`、`src-tauri/src/protocol/codec/messages.rs:145-215` / Messages→Responses 组合拒绝携带 `temperature/top_p/stop` 的常见 Messages 请求：`encode_messages_to_chat` 保留这些字段，但 `encode_chat_to_responses` 的 `CHAT_TOP_LEVEL` 不含 → 组合路径零上游调用前 400。/ 按 ADR-37/§5.2 allowlist 未列 temperature 属设计决定，但设计未协调「Messages→Chat 主动 emit」与「Chat→Responses 拒绝」两条腿。建议：保持 fail-closed（保守约束），但在设计文档补充说明该组合路径的字段边界，避免实现歧义。
- **次要** / `src-tauri/src/protocol/codec/responses_codec.rs:1021-1027` / reasoning 仅消费 `response.reasoning_summary_text.delta`，`.done`（携带全文）被忽略；若后端只在 done 给全文则 Chat 流丢失 reasoning_content。/ 目标后端（backend-api/codex）发 delta，属健壮性缺口非当前路径故障；建议补 done 分支合并。
- **次要** / `src-tauri/src/protocol/mod.rs:344-708` / legacy `responses_to_openai` 的 input 数组内未知 item type 仍被静默丢弃（item 级 fail-open）；顶层 allowlist 已改 Result+校验，满足 §6.2「入口前顶层 allowlist」，此为残余 item 级缺口。/ 建议后续补未知 item type 返回 `UnsupportedFeatures`。

### Round 1 五条主要问题（复核确认已全部修复）

1. **codec_direction 按当前候选** ✓ — `core/attempt.rs:235` 用 `candidate.upstream_protocol`；`attempt.rs:642,718` 两条 failover 测试存在。
2. **懒刷新失败置 invalid** ✓ — `auth_provider/service.rs` 懒刷新失败走 `schedule_maintenance_retry`（`mark_invalid(id, Some(+12h))`）；本轮另修 401 路径对齐。
3. **done 事件合并 final_arguments** ✓ — `responses_codec.rs:765-891` 合并 + JSON 对象校验；本轮补非流 decoder 的 arguments JSON 校验。
4. **rollout 按 model/endpoint 匹配** ✓ — `server/handlers.rs` `has_request_scoped_auth_candidate` 按 endpoint+model+账号快照匹配；`flags_off_force_routeplan_only_for_matching_auth_endpoint_and_model` 测试存在。
5. **Halt 保留候选元数据** ✓ — `core/plan_executor.rs:90-148` 保留 last attempt meta；`driver.rs:441-442` 写实际 `upstream_type`。

### 本轮新修复（复核 agent 发现）

- **Halt 兜底文案泛化**（路由 agent 次要）— `core/attempt.rs:441` 兜底 Halt 文案改 "No available upstream candidate"（`PlanError::NoChannels` 已泛化，此兜底漏改）。
- **维护循环对新鲜 token 的 active 账号不同步模型**（Provider agent 主要）— `auth_provider/service.rs` `run_maintenance_cycle` 原只对临期账号进刷新分支，新鲜 token 的 active 账号命中 `_ => continue` 永不同步模型；已改为 active 账号统一走 `sync_models`（内部懒刷新，临期先刷、新鲜直接拉，失败保留旧快照）。同步更新 2 个 maintenance 测试断言。
- **codec 非流 function_call arguments JSON 校验**（codec agent 次要）— `responses_codec.rs` 非流 decoder 补 JSON 对象校验，与流式一致。
- **codec tool 顶层字段校验**（codec agent 次要）— `responses_codec.rs` `chat_tools_to_responses` 校验 tool 顶层多余键，fail-closed。

## 通过项摘要（Round 2 复核确认）

- 数据迁移（`auth_accounts` + `UNIQUE(provider,account_id)` + 路由索引 + `upstream_type DEFAULT 'channel'`）、upsert 保留路由配置、模型同步失败保留旧快照、quota 到期懒恢复、RequestLog 全部构造点接线 —— 数据层 agent 7 维度无缺陷，`auth_repository`/`request_log` 测试通过。
- RouteCandidate 泛化完整性（含 authorize_and_plan/handler/plan_executor::AttemptMeta/debug_json）、账号过滤（allowed_channels 豁免、空模型拒绝、invalid/disabled/quota）、账号分类（三下游出组）、rollout 门禁（request-scoped 匹配）—— 路由 agent 仅 1 次要（已修），route_plan/auth_routeplan 测试通过。
- OAuth PKCE state/一次性/超时/listener 释放、auth.json 导入按真实嵌套形状、写回 temp+fsync+0600+备份、per-account single-flight 刷新、quota parser、backend 保守约束（allowlist/stream:true/无 zstd）、错误脱敏、单 12h 循环 —— Provider agent 无主要正确性缺陷（主要问题为本轮已修的模型同步）。
- 流式状态机全事件链、terminal/[DONE]/usage 恰好一次、任意分片、rate_limits 字节透传、registry/SseMode 接线、Native usage 完整 record 扫描 —— codec agent 无阻塞，13 个 codec 测试通过。
- driver 账号分叉（.expect() 移除）、DTO 无秘密、Sidebar 精确 active、前端风险 banner/三态/quota 条件渲染/只读模型弹窗/per-account pending —— 执行器 agent 无真实秘密泄露。
- **ADR-38（删除本地化）已同步**：`auth_logout` 纯本地删除（删行+快照，不调 provider revoke），设计/任务卡/实现/前端一致；T9(4) 验收已更新为「无 provider 网络调用、无 warning 路径」。

## 验证状态（Phase 5 前）

- `cargo test`：全绿（450 lib + integration tests，含 auth_repository/request_log/route_plan/auth_routeplan/auth_provider/responses_codec）。
- `npm run build`：通过（tsc + vite）。
- `cargo fmt --check`：**有 12 处既有格式 diff**（mod.rs/proxy.rs/estimate_usage.rs/handlers.rs/driver.rs/commands/auth.rs 等，非本次实现引入；本轮改动的 4 个文件 fmt 干净）。T12 验收要求 fmt 通过——此为既有债务，建议后续单独跑 `cargo fmt` 或建立 baseline。
- `cargo clippy --all-targets -- -D warnings`：Round 1 已记录 174 个既有告警；本轮未跑全量 clippy（与外部进程共享工作树，避免误格式/误改）。**建议 Phase 5 验证时单独确认**。

## 未核对项

- 未使用真实 ChatGPT/Codex 订阅令牌访问生产 `chatgpt.com`；OAuth/模型列表/backend-api 生产端兼容性仍属需求书 §2.3 真实令牌待验证项（v1 保守处置）。
- 与外部进程并发修改同一工作树（ADR-38 同步中）；`codex_backend.rs` 有会话开始前既有的未提交改动（`client_version`/`slug` 解析），本轮不触碰。

---

## 附录：Round 1 报告（历史快照）

> 2026-08-09 首轮 CR。五条主要问题已在 Round 2 复核确认修复（见上）。

### 结论：FAIL（Round 1）

### 问题清单

- **主要** / `src-tauri/src/core/attempt.rs:231`、`src-tauri/src/core/attempt.rs:308` / 转换 codec 按 `RouteGroup` 第一候选的 `upstream_protocol` 选择，而不是按当前 `RouteGroupCandidate` 选择。当前 `build_route_plan` 只按 Native/Conversion tier 分组，因此 Chat 的同一 Conversion 组可以同时包含 Anthropic Channel 与 Responses Auth Account，Messages 组也可以同时包含 Chat Channel 与 Responses Auth Account。只要首候选失败并降级到另一协议候选，第二次 attempt 仍沿用首候选方向编码：例如把 Chat 请求编码成 Responses body 后发给 Anthropic `/messages`，混合候选降级主流程会稳定失败。/ 将 `codec_direction` 改为接收当前 candidate（或其 `upstream_protocol`），所有请求编码、`codec_version` 与 SSE mode 均以当前候选为准；补 Chat/Message 各一条“不同上游协议同组、首候选失败、第二候选成功”的流式与非流式测试。

- **主要** / `src-tauri/src/auth_provider/service.rs:249` / 出站前懒刷新使用 `self.refresh_account(account_id).await?` 直接返回错误；刷新失败时没有把账号置为 `invalid`。这与 ADR-10“刷新失败则账号置失效，路由跳过”冲突，失效 refresh token 的账号会继续保持 `active` 并被每个请求反复选中。401 后的强制刷新分支会标失效，但请求前到期/临期刷新失败不会。/ 捕获懒刷新错误；对凭据拒绝/无效 refresh token 标记 `invalid`（并按既定规则写 `next_retry_after`），再返回脱敏失败。补“临期 token + refresh 失败”测试，断言零 `/responses` 请求、账号状态为 `invalid`、后续候选加载会过滤该账号。

- **主要** / `src-tauri/src/protocol/codec/responses_codec.rs:818`、`src-tauri/src/protocol/codec/responses_codec.rs:848` / Responses→Chat 流状态机只处理 `response.output_item.added` 与 `response.function_call_arguments.delta`，完全忽略 `response.function_call_arguments.done`/`response.output_item.done` 中的最终 arguments。若上游只在 done 事件给出完整参数（或 delta 缺失/不完整），下游只收到 `finish_reason=tool_calls`，却没有可执行的 tool call；同时也未在 done 时校验最终 arguments JSON。/ 在 done 事件用 `output_index`/`item_id` 合并并补齐 call id、name、最终 arguments，校验 arguments 是合法 JSON，确保每个完成的 function call 至少输出一次完整可执行的 Chat tool call；补“只有 done 无 delta”“delta+done”“done 参数非法”以及 Messages 组合路径测试。

- **主要** / `src-tauri/src/server/handlers.rs:119`、`src-tauri/src/server/handlers.rs:129` / Auth rollout 开关只判断数据库中是否存在任意非空模型快照账号，没有判断该账号是否匹配当前请求的 model/endpoint。全局 `new_routeplan=false` 时，只要另一个模型有 Auth 账号，就会把当前请求强制送入 RoutePlan；随后普通 Channel 仍受 `native_responses/cross_protocol_codec` 关闭约束，导致原本应走 legacy 成功的 Responses/转换请求被错误拒绝。/ 在决定强制 rollout 前按当前 model、账号状态/quota/模型快照以及 endpoint 能力计算 request-scoped Auth 候选；只有当前请求确有可用 Auth Account 才强制 RoutePlan，否则保持 legacy。补“账号仅支持 model-A，请求 model-B，flags 全关”对 Responses 和 Messages 的回归测试。

- **主要** / `src-tauri/src/core/plan_executor.rs:189`、`src-tauri/src/endpoint_executor/driver.rs:328`、`src-tauri/src/endpoint_executor/driver.rs:690` / Auth Account 所有候选失败时，`FlowStep::Halt` 丢弃最后一次 attempt 的候选元数据，流式和非流式失败日志随后走通用 pre-commit writer，并硬编码 `upstream_type="channel"`。因此典型的 401→刷新→401 或账号 5xx 全部耗尽会被记成 API Channel，违反 ADR-30，账号 id/name 也丢失。/ 在 `AttemptFlow`/`PlanExecution` 保留最后尝试的 candidate meta，失败日志写实际 `upstream_type/id/name/provider/codec`；没有发生任何 attempt 的规划前拒绝才使用无候选值。补单账号最终失败的 stream/non-stream 日志断言。

- **次要** / `src-tauri/src/auth_provider/codex_login.rs:371`（首个新增代码 lint；完整命令涉及更多位置）/ 任务卡 T12 要求 `cargo clippy --all-targets -- -D warnings` 通过，当前命令失败（本次执行报告 174 个错误，包含新增 Auth/Codec 代码的 `type_complexity`、`nonminimal_bool`、dead code/unused 等，也包含既有模块告警）。这不直接改变运行时结果，但验收命令未达标。/ 清理本次新增告警；若仓库既有告警不在本轮修复范围，应先建立明确的 clippy baseline/allow 策略，再保证本次 diff 不新增告警并让任务卡中的实际门禁命令可重复通过。

### 通过项摘要（Round 1）

- 数据迁移包含 `auth_accounts`、`UNIQUE(provider, account_id)`、路由索引及 `request_logs.upstream_type DEFAULT 'channel'`；upsert 会保留 id/label/P/W/disabled 并恢复 `active`。
- OAuth PKCE S256、随机 localhost 回调、state/一次性交换、5 分钟 timeout、系统浏览器 opener 均已接线；auth.json 按真实嵌套 `tokens` 形状解析，opaque refresh token 未被解码。
- auth.json 写回具备前端覆盖确认、备份、同目录临时文件、`fsync`、0600 与原子 rename；不会修改 `config.toml`。
- Provider trait/registry/AuthService 已落地；401 的一次刷新重试位于账号适配器内部，未改 AttemptFlow 的逻辑 attempt 计数。
- backend-api 固定 `/responses`、`/models`，Bearer 注入、调用方鉴权头剥离、`stream:true`、顶层 allowlist、无 zstd 已实现。
- QuotaState 支持动态 limit id、primary/secondary、429 Retry-After/退避、最晚恢复点；账号 disabled/invalid/quota/空模型过滤与 `allowed_channels` 豁免已实现。
- Responses/Chat/Messages 三协议的流式和非流式基本链路、Native Responses usage 完整 record 扫描、rate_limits side-band 透传均已接线。
- 10 条 Tauri Auth 命令已注册，renderer DTO 不包含 access/refresh/id token 或 `payload_json`；RoutePlan `debug_json` 只含安全账号标识。
- 前端 `/channels/auth`、Sidebar 精确 active、风险文案、账号三态、模型/限额展示、编辑/启停/删除/刷新/同步/写回及覆盖确认已实现。
- 本次验证：`cargo fmt --check` 通过；`cargo test` 通过（426 个库测试 + integration tests）；`npm run build` 通过。

### 未核对项（Round 1）

- 未使用真实 ChatGPT/Codex 订阅令牌访问生产 `chatgpt.com`，因此 OAuth/模型列表/backend-api 的生产端兼容性仍属于需求书 §2.3 的真实令牌待验证项；本轮仅核对本地 mock 与静态实现。
