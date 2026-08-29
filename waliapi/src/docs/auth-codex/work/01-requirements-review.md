# 需求确认书

> 复核基线：`00-optimized-requirements.md` 为最高优先级；D-A～D-F 均直接采纳，不重复提请决策。复核对象为当前工作树（2026-08-09）。

## 1. 确认项

- **范围与优先级已收敛（ADR-1/6/9/10/14/15/16/22/28/31～37）**：账号是与 Channel 同池的上游候选；v1 服务 Responses / Chat / Messages；账号豁免 `allowed_channels`；30min 空闲探测、token 加密、zstd 延后；单个 12h 循环；401 在账号适配器内仅重试一次；账号强制上游流式；请求字段使用 allowlist；`codex.rate_limits` 原样透传。来源：`docs/auth-codex/work/00-optimized-requirements.md:50-139`、`docs/auth-codex/ADRs.md:32-95,129-239`。结论：**VERIFIED**（均为已拍板需求，不再列疑点）。
- **数据层仍是待实现硬缺口（ADR-3/5/13/23/30）**：迁移最新为 `018_wiki_tags.sql`，当前无 `auth_accounts` 表，`RequestLog` 也无 `upstream_type`。迁移由 `sqlx::migrate!("./migrations")` 自动纳入，并在升级前备份。来源：`src-tauri/migrations/`、`src-tauri/src/db/mod.rs:56-63,113-184`、`src-tauri/src/db/models.rs:182-227`。结论：**GAP**（与需求书 C-9/C-14 一致；ADR-13 在 ADR 正文中缺号，见 §4）。
- **Provider / 登录 / 命令层均尚未存在（ADR-2/12/20/24/25）**：当前没有 `auth` command/module、Provider trait、OAuth callback 或账号 repository；`commands/mod.rs` 未声明 auth，`lib.rs` invoke handler 未注册 auth 命令。现有命令返回 `Result<T, String>`，渠道 DTO 会掩码凭据。来源：`src-tauri/src/commands/mod.rs:1-12`、`src-tauri/src/lib.rs:133-228`、`src-tauri/src/commands/channel.rs:49-88`。结论：**GAP**（与 §2.1 一致）。
- **依赖和挂载点足够（ADR-2/10/12）**：已有 axum 0.8、tokio full、reqwest json+stream、async-trait、base64/sha2、tauri-plugin-opener；缺少 `oauth2` 且 reqwest 未启用 zstd。`lib.rs` setup 已有常驻 spawn 和 AppState 模式；全库唯一周期 interval 先例是 MCP keepalive。来源：`src-tauri/Cargo.toml:15-68`、`src-tauri/src/lib.rs:24-33,41-43,111-128`、`src-tauri/src/services/mcp/handlers.rs:622`。结论：**VERIFIED**。
- **候选半成品与现有选择语义属实（ADR-6/9/11/16/21/22/32/34）**：`RouteGroupCandidate` 已存在但仍持有具体 `Channel`；priority/weight 无放回抽样已泛化；渠道空 models 为通配；`allowed_channels` 是 channel id 白名单；账号过滤、空模型拒绝、失效/停用/QuotaState 过滤均不存在。来源：`src-tauri/src/core/route_plan.rs:109-117,211-250,363-397,545-642`。结论：**VERIFIED + GAP**（基础可复用，账号语义待实现）。
- **AttemptFlow 的重试状态机无需改变（ADR-22/35）**：候选遍历、组预算、可降级失败跨组逻辑与候选实体类型无关；401 刷新仍应封装在账号适配器内。来源：`src-tauri/src/core/attempt.rs:303-406`。结论：**VERIFIED**。但 `plan_executor` 的候选元数据读取必须改，见 §4。
- **执行层缺口属实（ADR-7/10/35/37）**：非流与流式 driver 各有一份 `HashMap<String,(Channel,ChannelIdentity)>` 和 `.expect()`；`send_request` 只接受 Channel，并从静态 `channel.api_key` 生成鉴权头；gemini override 是可参考的分叉。来源：`src-tauri/src/endpoint_executor/driver.rs:77-126,344-428`、`src-tauri/src/endpoint_executor/mod.rs:426-476`。结论：**GAP**。
- **Codec 与 usage 缺口属实（ADR-31/33/36）**：registry 仅有 Chat↔Messages，`Downstream/Upstream` 无 Responses；`SseMode` 无 ResponsesToChat；字节级 framing 与 `ResponsesSseAssembler` 可复用；Native usage 只扫顶层 `usage`，读不到 `response.completed.response.usage`。来源：`src-tauri/src/protocol/codec/registry.rs:16-28,98-119`、`src-tauri/src/endpoint_executor/sse.rs:20-44,103-136,498-521`、`src-tauri/src/protocol/codec/sse.rs:9-59`、`src-tauri/src/protocol/responses.rs:5-54`。结论：**VERIFIED + GAP**。
- **模型同步需求无现成实现（ADR-8/21/34）**：账号模型须来自 `/models`、只读全量、空快照拒绝路由、失败保留旧快照；当前仅有 Channel 的模型同步能力，没有账号快照或同步时间字段。来源：`docs/auth-codex/work/00-optimized-requirements.md:92-93`、`docs/auth-codex/ADRs.md:50-54,135-139,225-233`，以及当前无 `AuthAccount` 模型。结论：**GAP**。
- **日志区分需求成立（ADR-30/33）**：`request_logs.channel_id` 无外键，可复用为账号 id；repository 的 insert 和 Rust/TS DTO 都要增加 `upstream_type`，日志页当前只有 channel name 过滤。来源：`src-tauri/migrations/001_init.sql:40-60`、`src-tauri/src/db/repository.rs:536-582,658-705`、`src/types/index.ts:126-165`、`src/pages/LogsPage.tsx:167-198`。结论：**GAP**。
- **前端基线与校准一致（ADR-4/17/18/19/26/27/29）**：目前只有 `/channels`；Sidebar 的非 `end` NavLink 会让 `/channels/auth` 同时命中；页面采用 `useState` + 手动 load；现有 underline tab、provider pill、双列卡片和实际颜色 token 均与优化需求书指出的位置一致。来源：`src/App.tsx:42-57`、`src/components/layout/Sidebar.tsx:22-30,85-108`、`src/pages/SettingsPage.tsx:194-214`、`src/pages/UsagePage.tsx:413-453`、`src/pages/ApiKeysPage.tsx:111-120`、`src/App.css:3-20`。结论：**VERIFIED + GAP**（样式基础存在，Auth 页面/路由/API/types 尚未实现）。注：ADR-27 正文为「编辑弹窗（名称/P/W）」，非早期标题「重命名 = label 内联编辑」；以正文为准。
- **本机 CLI 配置逻辑与 Auth 相互独立（ADR-18/19）**：`write_codex` 是普通 helper，只写 `config.toml` 且明确不写 auth.json；扫描器只读 Codex config 的顶层 `api_key`。来源：`src-tauri/src/commands/app_config.rs:363-428`、`src-tauri/src/commands/import_export.rs:586-686`。结论：**VERIFIED**，C-7 保持 out of scope。

ADR 核对覆盖：ADR-1～12、14～37 均已在以上确认项或下方疑点/缺口中核对；文档中不存在 ADR-13 正文，已按优化需求书将其视为 ADR-3 的同义引用并在 §4 记录。

## 2. 疑点清单（可假设）

1. **账号模型快照及同步时间没有明确持久化形状**
   - 问题：§2.1 的表列包含 `model_states_json`，但没有 ADR-8 明写的 `last_models_sync_at`，也未定义可用模型 id 与 ModelState 的 JSON 结构。
   - 影响：迁移、repository、路由模型过滤、GET `/models` 失败保留旧快照和前端“同步于”展示会采用不同数据源。
   - 建议假设（**假设**）：`model_states_json` 作为唯一模型快照（至少含 model id 列表与 per-model 状态），同步时间增加明确的 `last_models_sync_at` 通用列；同步失败不覆盖该列和旧快照。理由：这与 ADR-8 的明确影响项一致，避免把路由关键数据塞入无约束的 `attributes_json`。

2. **通用账号的唯一键作用域未明**
   - 问题：导入/登录按 `account_id` 覆盖，但表面向多 provider；未说明唯一约束是全局 `account_id` 还是 `(provider, account_id)`。
   - 影响：未来 provider 的 opaque id 若碰撞，可能错误覆盖另一 provider 的账号。
   - 建议假设（**假设**）：使用 `UNIQUE(provider, account_id)`，覆盖也按二元组查找。理由：不改变 codex v1 行为，同时满足 ADR-3/5/12 的 provider 通用性。

3. **账号状态字段的值域和 cooldown 到期恢复时机未写清**
   - 问题：schema 同时有 `status`、`disabled`、`quota_json`；需求只描述“正常/失效/停用/限额耗尽”。D-C 延后 30min 空闲探测、D-F 后台循环为 12h，但账号仍必须在 `NextRecoverAt` 到期后及时恢复。
   - 影响：不同实现可能把限额耗尽重复写入 status，或让已到恢复点的账号最长再等 12h。
   - 建议假设（**假设**）：`disabled` 只表达用户开关；`status` 只表达凭据生命周期（active/invalid）；限额态完全来自 `quota_json`。每次路由过滤时若 `NextRecoverAt <= now` 即懒恢复，不依赖 12h 循环。理由：避免状态双写，也不重新引入 D-C 延后的 30min 探测。

4. **OAuth 登录进度、取消和超时的命令契约未定义**
   - 问题：命令集只有 `auth_login`，UI 却要求五步实时状态和取消；没有 event 名称、超时或重复登录约束。
   - 影响：前后端可能分别实现阻塞 invoke、事件流或轮询，接口无法对齐；localhost listener 可能悬挂。
   - 建议假设（**假设**）：一次 `auth_login` 为单个异步 invoke，前端只展示本地可推断的阶段；后端持有明确超时并在命令取消/窗口关闭时释放 listener，不新增轮询命令。理由：满足 v1 交互且保持命令面最小。

5. **导入过期 access token 与首次模型同步失败的结果语义未定义**
   - 问题：ADR-24 要求“令牌未过期”，但 refresh token 是 opaque；ADR-8 又规定 `/models` 失败保留旧快照，新账号没有旧快照。
   - 影响：可恢复的本机登录态可能被错误拒绝；或登录已入库但 UI 误报全流程失败。
   - 建议假设（**假设**）：字段完整后，access token 已过期则先尝试 refresh；refresh 成功即导入。首次 `/models` 失败仍保存账号并返回“登录成功、模型同步失败”，模型快照为空所以按 ADR-34 不参与路由。理由：不丢可恢复凭据且保持 fail-closed 路由。

6. **revoke / 写回的文件与远端失败语义未定义**
   - 问题：`auth_logout(revoke)` 未说明 revoke 失败是否阻止本地删除；`auth_write_back` 会覆盖单账号 auth.json，但未写明原子性、权限和备份。
   - 影响：网络故障可能让账号无法本地删除；写回失败可能破坏 Codex CLI 登录文件。
   - **拍板（2026-08-09）**：**v1 删除不做 revoke**——`auth_logout` 仅本地移除账号（数据库行 + 模型快照），不调用 provider `oauth/revoke`。理由：CPA 调研证实删除即本地移除（00-facts §5.3），且自动 revoke 会误伤已写回 `~/.codex/auth.json` 的本机 Codex CLI 会话。写回仍采用同目录原子替换、权限 0600，并在覆盖前生成可恢复备份。

## 3. 阻塞性疑点

1. **RoutePlan feature flags 的上线边界未决**
   - 问题：账号混合候选只能走新 RoutePlan，但生产入口 `maybe_route_plan` 在 `features.new_routeplan=false` 时直接退回 legacy 路由；四个 flag 缺省全关，且 UI 只有读取 API、没有开关。Chat/Messages 转换还依赖 `cross_protocol_codec`，Responses native 依赖 `native_responses`。来源：`src-tauri/src/server/handlers.rs:90-135`、`src-tauri/src/core/feature_flags.rs:1-75`、`src/lib/api.ts:97-110`。
   - 影响：如果不改变门禁，用户即使成功登录也永远不会使用 Auth 账号；若直接把 flags 全局改为默认开启，又会改变所有现有 Channel 请求的路由/codec 行为，属于兼容性与上线范围变化。
   - 建议假设：当存在可用 Auth 账号时，该请求强制进入混合 RoutePlan，并对账号分支启用 Responses/跨协议能力；没有 Auth 候选时保留现有 flag 行为。此假设减少对纯 Channel 流量的影响，但仍需要用户确认该 rollout 边界。

## 4. 需求缺口或矛盾（事实与代码现状不符处）

- **候选泛化不是“同步 4 处消费点”**（OUTDATED）：除 `resolve_model_candidates`、`build_route_plan`、driver 两个 lookup、`build_prepared_attempt` 外，`authorize_and_plan` 的签名和 `channels.is_empty()` 早退、生产 handler 的账号加载、`plan_executor::AttemptMeta` 对 `candidate.channel` 的读取、`debug_json` 也必须适配。`AttemptFlow` 状态机确实可不改，但 `plan_executor.rs` 文件不能整体不改。来源：优化需求书 `:141-147`；代码 `route_plan.rs:662-679,682-715`、`server/handlers.rs:119-135`、`plan_executor.rs:56-113`。
- **RequestLog 不止三处写点**（OUTDATED）：生产代码当前至少有 19 个 `RequestLog { ... }` 构造点（driver 3、core/proxy 4、server/handlers 12），另有测试构造点；repository INSERT、Rust model、命令 DTO、TS type 也需接线。若新字段非 `Option`，所有显式构造点都要补；即使依赖 Default，也必须保证 legacy 路径写出默认 `channel`。来源：优化需求书 `:54-56`；`rg "RequestLog \{" src-tauri/src`；`db/repository.rs:536-582`。
- **Codec 请求方向写反且漏实现**（GAP）：registry 的“方向”按“下游请求协议 → 上游请求协议”定义。账号上游固定 Responses，因此 Chat 下游必须新增严格 **Chat request → Responses request** encoder；现有 `responses_to_openai` 是 **Responses request → Chat request**，不能承担该职责。Messages 链式路径还需 Messages→Chat→Responses 的请求链，响应方向才是 Responses→Chat→Messages。来源：优化需求书 `:85-90,149-153`；`protocol/codec/registry.rs:1-10,77-155`；`protocol/mod.rs:56-145,176-311`。
- **“账号强制流式”漏了非流 Responses 聚合**（GAP）：当下游 `stream:false` 时，上游仍返回 Responses SSE。除 Responses→Chat 非流 decoder 外，Responses 下游本身也需要从 `response.completed.response` 得到最终 Responses JSON；Messages 非流还需完成链式 decode。现有 `ResponsesSseAssembler` 只做字节 framing，不构造最终响应。来源：优化需求书 `:79,86-90,120-125`；`protocol/responses.rs:5-54`、`endpoint_executor/mod.rs:519-557`。
- **DTO 不能只“掩码 refresh_token”**（GAP）：ADR-20 明确列表不含令牌明文；因此 access_token、refresh_token、id_token 均不得由 `auth_accounts_list` DTO 返回，日志/debug/error 也不得包含。优化需求书 C-17 的措辞过窄。来源：优化需求书 `:44`；`ADRs.md:141-154`。
- **日志 UI/统计接线在优化需求书中范围不足**（GAP）：ADR-30 要求日志页/统计按 `upstream_type` 区分，但 §2.1 只明确 migration、RequestLog 和三个写点；当前 log 查询输入和页面没有 upstream_type 过滤/展示。应把 Rust command DTO、TS type、至少来源标识纳入 v1 验收；是否增加筛选可由架构设计按 ADR-30 落地。来源：`ADRs.md:75-79`、优化需求书 `:54-56`、`commands/log.rs:126-160`、`LogsPage.tsx:167-198`。
- **同一优化需求书对非流兜底表述不一致**（OUTDATED）：§2.1/ADR-36/D-A 指向账号上游统一强制 `stream:true` 并内部缓冲，而 §2.3 仍保留“或扩展 decode_non_stream SSE 容忍分支”。按最高优先级中更明确的 D-A 与 ADR-36，复核采用“统一强制 + 内部缓冲”，不再视为待选方案。来源：优化需求书 `:79,124,134`、`ADRs.md:38-42`。
- **ADR 编号缺口**（OUTDATED）：`ADRs.md` 原没有 ADR-13 正文，但优化需求书和决策索引把 ADR-13 用作“通用列 + payload_json”存储模型；该内容与 ADR-3 完全重合。复核按 ADR-3 采纳，不新增产品疑点。**（复核补记：ADR-13 正文已补为 ADR-3 同义引用，决策索引已扩到 ADR-37。）**

## 5. 决策清单

| # | 问题 | 影响 | 建议假设 | 是否阻塞 |
|---|------|------|---------|---------|
| 1 | RoutePlan flags 默认全关时，Auth 如何进入生产路由：全局启用新路由，还是仅在存在 Auth 候选时强制进入混合 RoutePlan？ | 前者改变全部既有 Channel 流量；后者只改变拥有 Auth 候选的请求；不处理则 Auth 登录后永远不承接流量 | 存在可用 Auth 候选时强制混合 RoutePlan，并对账号分支启用 Responses/跨协议能力；无 Auth 候选时保留现有 flags 行为 | 是 |
| 2 | 模型快照和同步时间如何持久化？ | 影响 migration、路由过滤、失败保留旧快照及 UI | `model_states_json` 为唯一快照，新增 `last_models_sync_at`；失败不覆盖 | 否 |
| 3 | 通用账号唯一键是全局 account_id 还是 provider+account_id？ | 影响未来 provider 账号碰撞与覆盖 | `UNIQUE(provider, account_id)` | 否 |
| 4 | cooldown 到期如何恢复，status/disabled/quota 如何分工？ | 影响账号是否错误停用或最长延迟 12h 恢复 | disabled=用户开关；status=凭据生命周期；quota_json=限额；路由时懒恢复到期 cooldown | 否 |
| 5 | OAuth 五步进度、取消、超时如何对齐单一 auth_login 命令？ | 影响前后端契约和 callback listener 资源释放 | 单异步 invoke + 前端本地阶段 + 后端明确超时/取消清理，不增轮询命令 | 否 |
| 6 | 过期 access token 导入和首次模型同步失败如何返回？ | 影响可恢复账号导入与登录成功语义 | 先 refresh；模型同步失败仍保存账号并明确返回部分成功，空快照拒绝路由 | 否 |
| 7 | revoke 失败及 auth.json 覆盖失败如何处理？ | 影响本地删除可用性与 CLI 凭据文件安全 | **v1 不做 revoke**：删除=纯本地移除，不调 provider 端点（ADR-38）；写回原子、0600、覆盖前备份 | 否 |

## 6. 用户决策

用户于 2026-08-09 对上述决策清单 **1～7 项全部采纳建议假设**；其中第 6、7 项在采纳后进一步拍板：**删除不做 revoke，改为纯本地移除**（见上）。第 1 项的生效边界为：存在可用 Auth 账号时强制进入混合 RoutePlan 并为账号分支启用所需 Responses/跨协议能力；无可用 Auth 账号时保持既有 feature flags 行为。
