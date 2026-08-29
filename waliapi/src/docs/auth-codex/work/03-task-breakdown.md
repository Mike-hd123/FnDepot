# 任务拆分

> 依赖拓扑：`T1 -> T2 -> {T3,T4,T8}`；`T5` 可与 T1～T4 并行；`T1 -> T6`；`{T3,T4,T5,T6} -> T7`；`{T3,T4,T8} -> T9`；`{T1,T9} -> T10 -> T11`；`{T1..T11} -> T12`。同一时刻不得让两个写入 agent 修改重叠文件；特别是 `lib.rs` 由 T8 后再交 T9，`endpoint_executor/` 由 T5 后再交 T7。

## T1：数据迁移、账号仓储与日志来源字段

- 目标：建立通用 `auth_accounts` 持久化层，并让所有 request log 明确区分 Channel/Auth Account。
- 涉及文件：`src-tauri/migrations/019_auth_accounts.sql`、`src-tauri/src/db/models.rs`、`src-tauri/src/db/repository.rs`、全部 `RequestLog` 生产/测试构造点、`src-tauri/src/commands/log.rs`。
- 改动点：创建通用列+`payload_json` 表、`last_models_sync_at`、`UNIQUE(provider,account_id)` 和 route index；加 `request_logs.upstream_type DEFAULT 'channel'`；定义 AuthAccount/QuotaState/ModelState；实现 CRUD、保留路由配置的 upsert、同步成功才替换快照、quota 到期懒恢复；日志 INSERT/查询/DTO/可选过滤接线，旧路径显式或默认写 `channel`。
- 验收标准：（1）`cd src-tauri && cargo test auth_repository -- --nocapture` 通过，测试断言同 provider+account 覆盖后 id/P/W/disabled 不变，不同 provider 同 account_id 可并存；（2）同命令测试断言模型同步失败后 JSON 与 `last_models_sync_at` 未变化、到期 quota 返回可路由；（3）`cd src-tauri && cargo test request_log_upstream_type -- --nocapture` 通过，断言旧写点落 `channel`、账号写点落 `auth_account`、filter 只返回目标类型；（4）`cd src-tauri && cargo check` 通过，证明所有显式 `RequestLog` 构造点已补齐。
- 依赖：无
- 独立 agent：是

## T2：Provider trait、registry 与 AuthService 基础

- 目标：提供与 provider 无关的登录/刷新/出站抽象、错误分类和并发刷新 single-flight。
- 涉及文件：新 `src-tauri/src/auth_provider/mod.rs`、`types.rs`、`service.rs`，`src-tauri/src/lib.rs`（仅模块声明；AppState 接线留 T8），`src-tauri/Cargo.toml`（`oauth2` 可由 T3 实际加入）。
- 改动点：定义 object-safe async `Provider` 的 login/import/refresh/outbound/list_models；实现 `ProviderRegistry` 按 ProviderKind 分派；`AuthService` 编排 repository、时钟和 per-account mutex；ProviderError 映射 failure class 且 Display/Debug 脱敏；route/executor 只拿通用账号摘要，不解析 payload。
- 验收标准：（1）`cd src-tauri && cargo test auth_provider::service::tests -- --nocapture` 通过；（2）并发测试启动 20 个同账号 refresh，fake provider 计数断言为 1，且所有调用读到同一新 payload；（3）未知 provider 测试断言在零 provider HTTP 调用前返回 typed error；（4）错误脱敏测试断言格式化结果不包含 fixture 的 access/refresh/id token。
- 依赖：T1
- 独立 agent：是

## T3：Codex OAuth、auth.json 导入与原子写回

- 目标：完成无需 Codex CLI 的 PKCE 登录、真实 auth.json 形状导入和手动安全写回。
- 涉及文件：新 `src-tauri/src/auth_provider/codex_login.rs`（或 `codex/login.rs`）、provider registry 接线、`src-tauri/Cargo.toml`/`Cargo.lock`（新增 `oauth2`）、测试 fixtures。
- 改动点：127.0.0.1 随机端口一次性 axum callback、PKCE S256/state、opener runtime、5min timeout/资源清理、token exchange；解析嵌套 tokens、opaque refresh、id_token 展示 claims；过期导入先 refresh；`CODEX_HOME` 优先路径；upsert 后首次模型同步返回部分成功；写回 temp+fsync+0600+backup+rename；`auth_logout` 纯本地删除（删行 + 快照，不调 provider revoke）。
- 验收标准：（1）`cd src-tauri && cargo test auth_provider::codex_login::tests -- --nocapture` 通过；（2）本地 mock OAuth 测试断言 callback 在浏览器打开前已监听、错 state 不换 token、正确 state 只换一次、timeout 后端口可重新绑定；（3）导入 fixture 测试断言嵌套 tokens 可读、opaque refresh 不解码、缺 account_id 拒绝、过期 token 先 refresh；（4）写回测试断言 JSON 形状与 facts §7 一致、Unix 权限为 0600、存在备份，注入 rename 失败后旧文件字节不变；（5）所有测试仅访问本地 mock 地址。
- 依赖：T2
- 独立 agent：是

## T4：Codex backend-api、模型同步与 quota parser

- 目标：实现 codex 的 `/responses`/`/models` 薄适配和账号级限额状态，不依赖真实令牌。
- 涉及文件：新 `src-tauri/src/auth_provider/codex_backend.rs`（或 `codex/backend.rs`）、`auth_provider/types.rs`、`service.rs`、本地 HTTP fixtures。
- 改动点：固定 backend base；Bearer 和可信 actor header；请求最小 allowlist、未知字段 HTTP 前 400、强制 `stream:true`、无 zstd；懒刷新临界值；401 single-flight refresh 后同账号只重试一次；`GET /models` 规范化/失败保留旧快照；解析动态 limit id 的 primary/secondary headers、Retry-After、退避、多个耗尽窗口最晚恢复点。
- 验收标准：（1）`cd src-tauri && cargo test auth_provider::codex_backend::tests -- --nocapture` 通过；（2）mock 断言出站 URL 精确为 `/backend-api/codex/responses`、body `stream=true`、调用方 Authorization/actor header 未透传、未设置 zstd；（3）未知顶层字段测试断言响应为 caller 400 且 mock hit count=0；（4）401→200 断言 HTTP hit=2、refresh=1，401→401 也只 hit=2 并标 invalid；（5）quota table tests 覆盖无头=null、动态窗口（月 43200 保留 / 周）、空窗口（仅 used-percent:0）丢弃、多 limit id、坏 reset、429 Retry-After、两个耗尽窗口取最晚 reset；（6）models 失败测试断言旧快照/时间戳原值不变。
- 依赖：T2
- 独立 agent：是

## T5：Responses codec 与 Native usage

- 目标：闭合 Chat/Messages/Responses 到账号 Responses 上游的严格 request 编码、流/非流 response 解码和 usage 统计。
- 涉及文件：新 `src-tauri/src/protocol/codec/responses_codec.rs`、`src-tauri/src/protocol/codec/{mod.rs,registry.rs,request.rs,sse.rs}`、`src-tauri/src/protocol/mod.rs`、`src-tauri/src/endpoint_executor/sse.rs`；`driver.rs` 的 mode 选择留 T7。
- 改动点：Downstream/Upstream Responses 变体；Chat→Responses 严格 encoder；Messages→Chat→Responses 组合 encoder；legacy `responses_to_openai` 入口改 Result+allowlist；Responses→Chat 流状态机/非流 decoder；Responses→Chat→Messages 组合 decoder；Responses SSE 聚合 final response；`SseMode::ResponsesToChat/ResponsesToMessages` 和 `decoder_for`；Native 完整 record usage 扫描；rate_limits record 不改写。
- 验收标准：（1）`cd src-tauri && cargo test responses_codec -- --nocapture` 通过；（2）golden tests 覆盖 text、function call、reasoning、usage、failed、缺 terminal、重复 done，断言 terminal/[DONE]/usage 各恰好一次；（3）对同一 fixture 的每个字节边界切分和多 record chunk 运行，输出与未切分完全相同；（4）Chat 和 Messages 未知/不可表示字段断言 `UnsupportedFeatures` 且 fake upstream hit=0；（5）stream=false 聚合对三种下游分别输出合法 Responses/Chat/Messages JSON；（6）Native `response.completed.response.usage` fixture 断言 prompt/completion/total 非零；（7）rate_limits 输入 record 与输出字节相同。
- 依赖：无
- 独立 agent：是

## T6：RouteCandidate 混合池与 Auth rollout 门禁

- 目标：把账号作为普通候选加入 RoutePlan，同时不改变无可用账号时的纯 Channel rollout 行为。
- 涉及文件：`src-tauri/src/core/route_plan.rs`、`src-tauri/src/core/attempt.rs`、`src-tauri/src/core/plan_executor.rs`、`src-tauri/src/server/handlers.rs`、相关 route/rollout tests。
- 改动点：RouteCandidate enum 和通用访问器；`RouteGroupCandidate`/HasPriorityWeight 泛化；accounts 过滤、allowed_channels 豁免、空模型拒绝、无 mapping；Chat/Messages/Responses account 分类；authorize/build/AttemptMeta/debug_json 泛化；handlers request-scoped 加载账号并在存在可用账号时强制混合 RoutePlan；普通 Channel 继续遵守原 flags；用户文案泛化为 upstream candidate。
- 验收标准：（1）`cd src-tauri && cargo test core::route_plan::tests -- --nocapture` 通过；（2）固定 RNG 测试断言 Channel+两个账号按 priority/weight 同池且每账号是独立候选；（3）账号即使不在 allowed_channels 也保留，空模型/disabled/invalid/未来 quota recovery 则剔除，到期 quota 恢复；（4）账号在 Responses 为 Native、Chat/Messages 为 Conversion、CountTokens/Embeddings 不出组；（5）`cd src-tauri && cargo test auth_routeplan_rollout -- --nocapture` 断言 flag=false+可用账号走 RoutePlan，flag=false+无 request-scoped 可用账号返回 legacy，且普通 Channel capability flags 未被隐式打开；（6）debug snapshot 只含 id/name/type/provider/P/W，测试断言不含 payload/token。
- 依赖：T1
- 独立 agent：是

## T7：Executor/driver 账号分叉与日志闭环

- 目标：让流式和非流式执行真正消费 Auth 候选，删除 lookup panic，并保持 AttemptFlow 重试语义。
- 涉及文件：`src-tauri/src/endpoint_executor/driver.rs`、`src-tauri/src/endpoint_executor/mod.rs`、必要的 integration/mock tests。
- 改动点：非流/流 lookup 改通用候选；两处 `.expect()` 改可观测错误；普通 Channel 保留 send_request，账号分支调用 AuthService；接 `sse_mode_for` 的 Responses 模式；下游非流走强制上游 SSE 聚合；安全响应头/quota 持久化；RequestLog 写 upstream_type 和账号 id/name；内部 401 refresh 不改 attempt_no/is_retry。
- 验收标准：（1）`cd src-tauri && cargo test endpoint_executor::integration_tests::auth_account -- --nocapture` 通过；（2）Chat/Messages/Responses 的 stream 与 non-stream 共 6 条本地 mock 用例均返回对应协议；（3）401→200 用例断言 RequestLog 只有一个逻辑 attempt、`is_retry=0`、HTTP 请求为两个、upstream_type=`auth_account`；（4）账号 5xx 后下一普通 Channel 成功，断言两候选顺序和最终日志；（5）伪造 lookup 缺失断言函数返回错误响应并写 failure log，进程不 panic；（6）普通 Channel 既有 `cargo test endpoint_executor` 全部通过。
- 依赖：T3、T4、T5、T6
- 独立 agent：否

## T8：单个 12h 维护循环

- 目标：用一个后台循环完成 token 刷新、失效重试和模型同步，不实现 30min quota 探测。
- 涉及文件：新 `src-tauri/src/auth_provider/maintenance.rs`、`src-tauri/src/lib.rs`、`auth_provider/service.rs`、fake clock/provider tests。
- 改动点：AppState 注入 AuthService；setup 只 spawn 一个 maintenance loop；启动 due scan + 12h interval；active 临期刷新、到期 invalid 重试、成功恢复、模型同步失败隔离；单账号失败不终止循环；不增加 quota probe timer。
- 验收标准：（1）`cd src-tauri && cargo test auth_provider::maintenance::tests -- --nocapture` 在 paused Tokio time 下通过；（2）advance 12h 后 fake provider 对每个 due 账号 refresh/models 各调用一次，未 due 账号为 0；（3）invalid+refresh 成功变 active，失败账号只更新 next_retry_after 且不阻止后一账号执行；（4）源码断言 `test "$(rg -n 'interval\(' src-tauri/src/auth_provider/maintenance.rs | wc -l | tr -d ' ')" = 1` 通过，且 `! rg -n '30 ?\* ?60|thirty|quota.?probe' src-tauri/src/auth_provider/maintenance.rs` 返回成功。
- 依赖：T3、T4
- 独立 agent：是

## T9：10 条 Tauri Auth 命令与无秘密 DTO

- 目标：给前端提供 ADR-20 完整命令面，并保证任何列表/错误都不泄漏 token。
- 涉及文件：新 `src-tauri/src/commands/auth.rs`、`src-tauri/src/commands/mod.rs`、`src-tauri/src/lib.rs`（在 T8 后串行注册）、command tests。
- 改动点：实现并注册 `auth_accounts_list/auth_login/auth_login_import/auth_logout/auth_refresh_token/auth_sync_models/auth_write_back/auth_toggle/auth_quota_status/auth_update`；输入校验；partial success/warning DTO；列表只含 payload 摘要；所有错误脱敏。
- 验收标准：（1）`cd src-tauri && cargo test commands::auth::tests -- --nocapture` 通过；（2）序列化 list/login/import/logout/writeback 结果，断言字符串不含 fixture token 和键名 `access_token|refresh_token|id_token|payload_json`；（3）update 输入 label 空、priority<0、weight<1 均在 repository 调用前拒绝；（4）`auth_logout` 删除账号后断言 DB 行与模型快照均移除，返回被删账号摘要（无 provider 网络调用、无 warning 路径）；（5）`test "$(rg -n '^pub async fn auth_(accounts_list|login|login_import|logout|refresh_token|sync_models|write_back|toggle|quota_status|update)' src-tauri/src/commands/auth.rs | wc -l | tr -d ' ')" = 10` 通过；（6）`cd src-tauri && cargo check` 通过，证明 handler 注册名有效。
- 依赖：T3、T4、T8
- 独立 agent：是

## T10：前端路由壳、Auth API/types 与日志来源 UI

- 目标：建立 `/channels` 与 `/channels/auth` 可寻址双页壳、无秘密 TS contract，并在日志页区分 API/Auth。
- 涉及文件：`src/App.tsx`、`src/components/layout/Sidebar.tsx`、`src/pages/ChannelsPage.tsx`、`src/lib/api.ts`、`src/types/index.ts`、`src/pages/LogsPage.tsx`。
- 改动点：新增 Auth route import/route；API|Auth underline tabs；Sidebar `/channels` 精确 active；`authApi` 10 方法；Auth/Quota/Model/command DTO；Log upstream_type 类型、来源 badge 和过滤；仍用 useState+load，不引入 react-query/zustand。
- 验收标准：（1）仓库根 `npm run build` 通过；（2）`test "$(rg -n 'path="/channels/auth"' src/App.tsx | wc -l | tr -d ' ')" = 1` 通过；（3）`rg -n 'to === "/channels".*end|end=\{to === "/channels"' src/components/layout/Sidebar.tsx` 至少命中一处精确匹配实现；（4）`test "$(rg -n 'invoke<.*>\("auth_(accounts_list|login|login_import|logout|refresh_token|sync_models|write_back|toggle|quota_status|update)"' src/lib/api.ts | wc -l | tr -d ' ')" = 10` 通过；（5）`rg -n 'upstream_type' src/types/index.ts src/pages/LogsPage.tsx src/lib/api.ts` 三个文件均命中；（6）`! rg -n 'access_token|refresh_token|id_token|payload_json' src/types/index.ts src/pages src/lib/api.ts` 返回成功。
- 依赖：T1、T9
- 独立 agent：是

## T11：Auth 账号页、卡片和弹窗状态

- 目标：完整实现 `01-ui-spec` 的 Auth 页面交互与三种账号状态。
- 涉及文件：新 `src/pages/AuthChannelsPage.tsx`，新 `src/components/auth/` 下 AccountCard/LoginModal/EditModal/ModelSyncModal/ProviderPills/QuotaBlock 等组件，必要的 `src/App.css`（只复用/补通用 token，不重定义错误 token）。
- 改动点：provider pills、说明、固定风险 banner、双列卡片/空槽；正常/限额/失效态；只在 quota 存在时显示窗口；编辑 P/W、启停/删除、刷新、同步、写回、重登；五步登录单 invoke UI；partial success、warning、confirm 和 per-account pending；颜色使用实际 App.css token。
- 验收标准：（1）仓库根 `npm run build` 通过；（2）`test "$(rg -F '⚠️ 风险提示：此提供商使用的订阅 / OAuth 会话未获官方授权用于代理 / 路由器使用。账户可能被限制或封禁。使用风险自负。' src/pages/AuthChannelsPage.tsx src/components/auth | wc -l | tr -d ' ')" = 1` 通过；（3）`rg -n '正常|已踢出路由|已失效|重新登录' src/pages/AuthChannelsPage.tsx src/components/auth` 四种文案均有实现；（4）`rg -n '登录 Codex 账号|同步模型|写回.*CLI|priority|weight' src/pages/AuthChannelsPage.tsx src/components/auth` 各交互均命中；（5）`! rg -n -- '--emerald|--amber|--red|#3b82f6|#e2e8f0' src/pages/AuthChannelsPage.tsx src/components/auth` 返回成功；（6）`rg -n 'quota.*&&.*QuotaBlock|quota.*\?.*QuotaBlock' src/pages/AuthChannelsPage.tsx src/components/auth` 至少命中一处，且 `! rg -n "<input[^>]*type=['\"]checkbox['\"]" src/pages/AuthChannelsPage.tsx src/components/auth` 返回成功。
- 依赖：T10
- 独立 agent：是

## T12：无真实令牌端到端回归与安全验收

- 目标：把数据、provider、路由、codec、执行器、commands 和前端契约串成可重复的离线验收，确认排除项未混入。
- 涉及文件：`src-tauri/src/rollout_integration_tests.rs` 或新 `src-tauri/src/auth_integration_tests.rs`、测试 fixtures、必要的 test-only 注入；不新增生产功能。
- 改动点：本地 axum fake provider 覆盖登录/导入、模型、三协议流/非流、401、429/quota、候选降级、日志/usage；快照扫描 DTO/debug/log/error 无秘密；现有纯 Channel rollout 回归；前端 build；确认没有 zstd 和 30min probe。
- 验收标准：（1）`cd src-tauri && cargo fmt --check` 通过；（2）`cd src-tauri && cargo clippy --all-targets -- -D warnings` 通过；（3）`cd src-tauri && cargo test` 全绿；（4）仓库根 `npm run build` 通过；（5）端到端测试内 fake server 断言外网请求计数为 0，Chat/Messages/Responses 的 stream/non-stream 六条路径、401→200、429→候选降级、模型首次失败 fail-closed 全部有具名用例；（6）`! rg -n 'zstd' src-tauri/Cargo.toml src-tauri/src/auth_provider` 返回成功；（7）`test "$(rg -n 'interval\(' src-tauri/src/auth_provider/maintenance.rs | wc -l | tr -d ' ')" = 1` 通过；（8）命令 DTO/debug/log/error 快照逐一断言不包含四个 fixture secret；（9）现有无 Auth、flags 全关的 rollout 集成测试断言仍走 legacy。
- 依赖：T1、T2、T3、T4、T5、T6、T7、T8、T9、T10、T11
- 独立 agent：否

## 并行执行建议

- 第一波：T1 与 T5 并行；T1 完成后开始 T2、T6。
- 第二波：T2 完成后 T3、T4 并行；T5/T6 可继续独立收尾。
- 第三波：T3+T4 完成后 T8；T3+T4+T5+T6 完成后 T7。T7 与 T8 文件所有权不重叠，可并行。
- 第四波：T8 后 T9；随后 T10、T11 串行。T12 最后统一回归。
- `lib.rs` 所有权顺序固定为 T2（模块声明）→T8（AppState/setup）→T9（invoke registration）；`endpoint_executor/sse.rs` 先 T5、`driver.rs/mod.rs` 后 T7，避免并行冲突。
