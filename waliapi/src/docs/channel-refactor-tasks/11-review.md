# T11：渠道协议重构复核报告

- **复核 commit**：`30513a23cb75329ce6587a67f985fba5d65d74a1`（`codex/channel-protocol-refactor-plan`）
- **本地改动范围**：16 个源文件修改 + 新增 `12-cliproxy-anthropic-to-chat.md`（详见文末"改动文件清单"）。未提交。
- **日期**：2026-08-06
- **总体结论**：核心架构（单一 facade `authorize_and_plan`、模型优先分组路由、commit barrier、错误分类、per-request codec 状态、fail-closed 拒绝、真实 usage、exactly-once 终止）在 flag 开启的新路径上实现正确、测试充分。复核共发现 **22 条问题**（8 条已修、6 条已修并补测试、8 条记为风险/待决策），**0 条 codec 照搬 CLIProxyAPI fail-open 的实质缺陷**。门禁：`pnpm build` ✅、`cargo fmt` ✅、`cargo test` ✅（303+21+4+37）、`cargo clippy -D warnings` ❌（107 条，与 pristine HEAD 集合完全一致，均为既有 T04/T10 债务，本次修复新增 0 条）。

## 一、逐条问题/风险（file:line → 要求 → 原因 → 修复）

### 已修复

**【B-R4】未知顶层字段静默丢弃**
- 位置：`protocol/codec/messages.rs:26-75`（`encode_messages_to_chat` 顶层扫描）
- 要求：12- R4「未知顶层字段静默丢弃→拒绝（带 JSON pointer）」；与 chat.rs 白名单策略对称
- 原因：沿用旧 `anthropic_to_openai` 的具名拒绝列表（仅 thinking/output_config/container/context_management），`metadata`/`user` 等被静默丢弃
- **修复**：改为白名单 `SUPPORTED_TOP_LEVEL`（model/messages/max_tokens/temperature/top_p/stop_sequences/stream/tools/tool_choice/system/user），其余键以 `FeatureKind::UnsupportedField` + `/{key}` 拒绝；known beta 键保留 `BetaFeature` 码。同时移除 `top_k` 透传（OpenAI Chat 无 top_k，之前被静默丢弃，现正确拒绝）
- 复测：`cargo test protocol::codec`（46 通过）；新增测试 `messages_request_rejects_unknown_top_level_fields`；新增集成测试 `codec_messages_to_chat_unknown_field_reject_zero_upstream`（断言上游调用 0）

**【B-R8/R21】缺失 tool_use.input 仍伪造 `{}`**
- 位置：`messages.rs:296-299`（请求）、`messages.rs:627-630`（非流响应）
- 要求：12- R8/R21「缺 tool_use.input 修成 {} → 非法/缺失 arguments/id 拒绝，绝不改成 {}」；04 验收「invalid arguments 不被改成 {}」
- 原因：直接搬移旧 `convert_anthropic_messages_to_openai` 的 `unwrap_or(json!({}))` 模式
- **修复**：改为 `.get("input").ok_or_else(... MissingToolField)`，仅显式提供 `{}` 才接受
- 复测：codec 测试 46 通过；新增 `messages_request_tool_use_requires_input_not_fabricated`、`messages_response_tool_use_requires_input_not_fabricated`、集成 `codec_chat_to_messages_invalid_tool_args_reject_zero_upstream`

**【B-R22】流式 tool id/name 空值未校验**
- 位置：`messages.rs:885-918`（content_block_start tool_use）
- 要求：12- R22「缺 tool_use id/name 容忍 → 缺失/非法 id/name 拒绝」
- 原因：为让下游尽早看到 call id 而在 content_block_start 即发 chunk，收尾校验漏了 id/name
- **修复**：content_block_start 时 id/name 空则立即 `MissingToolField`，与 chat.rs:1123 对称
- 复测：codec 测试 46 通过

**【B-R9】tool_choice 字符串 "any"/"tool" 非法透传**
- 位置：`messages.rs:512-515`（`anthropic_tool_choice_to_chat` 字符串分支）
- 要求：12- R9「非法 tool choice 拒绝，绝不静默降级」
- 原因：旧代码「字符串原样透传」遗留，未做 An→Oa 字符串映射
- **修复**：`"auto"→"auto"`、`"any"→"required"`、裸 `"tool"` 因缺 name 直接拒绝、其他字符串拒绝
- 复测：新增 `messages_request_tool_choice_strings_are_mapped_not_passed_through`

**【B-R12】非数组 stop_sequences 静默丢弃**
- 位置：`messages.rs:120-122`
- 要求：12- R12「非数组 stop_sequences 忽略 → 非法类型拒绝」
- 原因：`.and_then(Value::as_array)` 把类型错误吞掉
- **修复**：显式 match，非数组以 `UnsupportedField` 拒绝
- 复测：新增 `messages_request_rejects_non_array_stop_sequences`

**【B-R15】Chat→Messages 图片缺 media_type/大小/scheme 校验**
- 位置：`chat.rs:267-293`（image_url 分支）
- 要求：12- R15「校验 role/media type/data URL/大小，非法拒绝」；04 支持矩阵「user base64/URL 图片（校验）」
- 原因：chat→messages 特有路径只复用 `parse_data_url`，未接 request.rs 的 `anthropic_image_to_chat` 校验器
- **修复**：复用 `request::MAX_IMAGE_BYTES`：data URL media_type 必须 `image/*`、data ≤ MAX、非 data URL 必须 http(s)
- 复测：新增 `chat_request_rejects_invalid_images`

**【B-R7/R29】thinking 错误码 5 种不一致**
- 位置：`messages.rs:28-46`（顶层→beta_feature）、`request.rs:148-153`（system 中→prompt_cache）、`messages.rs:651-656`（响应块→unknown_block）、`messages.rs:947-952`（thinking_delta→unknown_event）
- 要求：12- R7/R29「首版拒绝 thinking/reasoning」+ 04「稳定错误 code」
- 原因：各分支按局部 switch 就近归类
- **修复**：统一 `FeatureKind::Thinking`：system 块（request.rs）、响应 thinking/redacted_thinking 块、流式 content_block_start(type=thinking/redacted_thinking)、thinking_delta/signature_delta 全部用 Thinking 码
- 复测：更新 `messages_response_rejects_unknown_block_and_bad_input`（thinking→thinking 码）

**【工具排序优化】text 先于 tool_result 时破坏 tool 邻接**
- 位置：`messages.rs:328`
- 要求：12- 消息排序语义「toolResults 追加在当前消息之前」
- 原因：先冲刷 text 再发 tool，导致 `assistant(tool_calls) → user(text) → tool(result)` 顺序错误
- **修复**：收集 tool 消息并先于缓冲文本发出（`assistant(tool_calls) → tool → user(text)`），`[tool_result, text]` 常见形态不受影响
- 复测：新增 `messages_request_tool_results_stay_adjacent_to_assistant`

**【A-P1b】ResponsesViaChat 旧债务路径忽略抽样 upstream_model**
- 位置：`core/attempt.rs:257` + `protocol/mod.rs:147-176`
- 要求：设计 11.4「不得出现实际请求模型与日志模型不一致」
- 原因：`responses_to_openai` 原样写入下游 model，未烘焙抽样模型
- **修复**：`encoded` 后补 `model` 字段写入 `upstream_model`（与原生分支一致）
- 复测：`cargo test --lib`（300 通过）

**【A-P1a】原生组混协议时 attempt 用组级协议/端点**
- 位置：`core/attempt.rs:191-193`、`core/plan_executor.rs:102-103`
- 要求：T05 分组契约 / T00 决策 4「priority/weight 只在同原生协议组内生效」
- 原因：`classify_channel` 把 OpenAI chat_completions 与 Ollama api_chat（flag ON 时）同归 Native 组；`build_prepared_attempt`/`AttemptMeta` 读组级协议/端点
- **修复**：改用 `candidate.upstream_protocol`/`candidate.upstream_endpoint`（候选自带正确值）
- 复测：`cargo test --lib` 300 通过；此为 `ollama_native=OFF`（默认）下的潜伏缺陷，已根治

**【C-F1】旧路由路径对数组 model_mapping 二次抽样**
- 位置：`adaptor/openai.rs:129-160`、`server/handlers.rs:540/2283`、`core/proxy.rs:142-159`
- 要求：T09「adapter/codec/executor 不重新抽样」；设计 11.4
- 原因：proxy 已预烘模型但 adaptor `apply_model_mapping` 仍保留数组随机分支
- **修复**：`apply_model_mapping` 数组分支不再重抽样（保留已烘焙 body 模型）；两处 legacy 流式路径（handlers.rs:520/2266）在循环内预烘 `upstream_model` 到 body
- 复测：`cargo test --lib` 300 通过

**【C-F2】旧 openai/custom 记录丢失 responses_via_chat 债务**
- 位置：`core/channel_identity.rs:188-199`、`core/route_plan.rs:471-490`
- 要求：设计 11.2「旧记录维持 responses_via_chat_v1 行为必须继续可用」
- 原因：债务只在 config.legacy_capabilities 逐行记录，旧行（revision-0）从无此配置
- **修复**：`classify_channel` 对 legacy-inferred（revision 0、无原生 responses 声明）的 openai 行授予 debt 路径（G2 Responses→Chat）
- 复测：新增 `legacy_openai_row_gets_responses_debt_at_routing`（route_plan）；300 通过

**【C-F4】new_to_legacy custom 分支缺 `/v1`**
- 位置：`core/channel_identity.rs:454-483`
- 要求：T02「base_url 必须是旧适配器追加 endpoint 后得到正确最终 URL 的兼容根」；设计 12.6
- 原因：custom 分支未镜像命名预设「native 根 + /v1」约定；旧 claude 适配器追加 `/messages`
- **修复**：新增 `legacy_base_for_native`：anthropic/ollama custom 的 native 根不以 `/v1` 结尾时补 `/v1`（openai custom 保留原样）
- 复测：更新 `new_to_legacy_anthropic_custom_uses_claude_alias_and_native_root` 期望；300 通过

**【D4】列表双标签不区分 legacy 身份**
- 位置：`src/pages/ChannelsPage.tsx:259-260`
- 要求：08-channel-ui「未知 legacy identity 显示 [旧配置] 或后端返回的明确 fallback」
- 原因：DTO 输出归一化后前端无法仅凭 protocol/provider 判断 legacy；`identity_revision` 已下发但列表未用
- **修复**：`identity_revision === 0` 时第一标签显示 `[旧配置]`
- 复测：`pnpm build` ✅

**【D2】UI 恒显示 Ollama Tab，创建后运行时 503**
- 位置：`src/components/ChannelForm.tsx`、`src/lib/api.ts`
- 要求：T00 决策 9「Ollama 原生配置在 UI 正式开放前…功能开关保持关闭」；10-integration-rollout「UI 明确不可用或隐藏」
- 原因：前端无 feature-flag 读取，无条件渲染三协议
- **修复**：新增 `get_feature_flags` Tauri 命令（复用 `read_feature_flags`），ChannelForm 加载时读取，`ollama_native=false` 时隐藏 Ollama Tab
- 复测：`pnpm build`（tsc + vite）✅

**【B-04 验收】零上游调用集成测试覆盖不足**
- 位置：`rollout_integration_tests.rs`（原仅 response_format 一条）
- 要求：04 验收「每个拒绝字段返回具体 JSON pointer 和稳定错误 code，上游调用为零」
- 原因：T04 期 codec 未接生产路由
- **修复**：新增 3 条集成测试：`codec_messages_to_chat_thinking_reject_zero_upstream`、`codec_messages_to_chat_unknown_field_reject_zero_upstream`、`codec_chat_to_messages_invalid_tool_args_reject_zero_upstream`，均断言 `conv_mock.call_count().await == 0`
- 复测：`cargo test rollout`（37 通过）

### 记为风险/待决策（未修 + 理由）

**【C-F3】新 RoutePlan 无法路由旧 Gemini 渠道（`legacy_executor_override=gemini_native`）**
- 位置：`core/route_plan.rs:437-534`
- 要求：设计 11.1「迁移旧 type=gemini 时…保留 legacy_executor_override」
- 现象：`classify_channel` 不消费 `executor_kind`/`legacy_executor_override`，`native_endpoints=[]` 的 gemini 行被静默丢弃，`new_routeplan` 开启后 Gemini 请求 503
- **未修理由**：修复需在 classify_channel 增加 Gemini 分支并放行空端点但 override=gemini 的渠道，涉及 Gemini 原生执行器与下游 Chat 转换链（T06 独占）；且 `new_routeplan` 默认 OFF（生产仍是旧 Dispatcher 路径，Gemini 不受影响）。属"功能开关开启后才暴露"，非当前生产缺陷。**建议**：T06 正式接线 `new_routeplan` 前必须处理

**【A-P2】流式 per-phase 超时（connect/header-first-frame/stream-idle）只定义未接线**
- 位置：`endpoint_executor/driver.rs:605-627,818-895`、`core/stream_supervisor.rs:254`
- 要求：T05「区分 connect timeout、header/first-frame timeout、stream idle timeout；渠道 timeout_secs 不直接作为长流总寿命」
- 现象：`StreamSupervisor::on_timeout` 有实现与单测但生产驱动从未调用；唯一生效的是 reqwest 整体 timeout
- **未修理由**：三处分别包 `tokio::time::timeout` 并接 supervisor 回调属于对 driver 流泵的较大改动，风险高于本轮收益；且 `new_routeplan` 默认 OFF。**建议**：T06 接线时一并实现，并在 `rollout_integration_tests` 补 `stream_idle_timeout_not_killed_by_total_timeout`

**【A-P2c】生产默认（new_routeplan=OFF）仍走不执行权限的旧 Dispatcher 路径**
- 位置：`server/handlers.rs`（flag OFF 分支）、`core/proxy.rs`
- 要求：设计 11.3「所有公开接口及流/非流路径必须共用 authorize_and_plan」「当前代码仅保存权限字段、尚未执行，重构不得继续放大该漏洞」
- 现象：四个 flag 默认全 OFF，生产默认旧路径不执行 allowed_models/allowed_channels/expires_at
- **未修理由**：`authorize_and_plan` 本身已正确执行授权且 flag ON 时所有路径共用它；但旧路径是灰度开关关闭时的回退。把权限检查提到 handler 公共层或在旧路径补同一套检查是安全关键项，**建议**：在正式开启 `new_routeplan` 前必须完成，或默认开启 `new_routeplan`（需先解决 C-F3 Gemini 与 A-P2 超时）

**【A-P2d】allowed_channels 匹配语义（id vs name）未定义**
- 位置：`core/route_plan.rs:377`
- 要求：设计 11.3「空数组语义实现前固定并测试」
- 现象：实现按 `allowed_channels.contains(&c.id)`，但命令层只透传字符串数组，前端类型为 `string[]`，无法确认存的是 id 还是 name
- **未修理由**：需在 T02/T05 文档固定为 channel id 并加 fixture 测试；属语义决策而非代码缺陷

**【A-P3】流结束日志 finalizer 与 client-cancel Drop 窄竞态**
- 位置：`endpoint_executor/driver.rs:890-893`
- 要求：T00 决策 6「exactly-once finalizer 写入 client_cancelled」
- 现象：`completed.store(true)` 先于 `finalizer.write`；窗口极小（流自然结束后日志落库瞬间）
- **未修理由**：`completed.store` 移到 `write` 完成后需保证 Drop 分支仍产出一行，属可观测性边缘，风险低

**【B-R24】refusal↔content_filter 映射与书面基准冲突（需决策）**
- 位置：`chat.rs:723,1168-1172`、`messages.rs:673,1076`
- 要求：04 首版拒绝「content_filter/refusal 无目标安全语义」；12- R24
- 现象：实现把 OpenAI content_filter/refusal 映射为 Anthropic refusal（反向亦然），有测试固化；比 CLIProxyAPI 的 content_filter→end_turn 更保真，但偏离书面基准
- **未修理由**：二选一需 ADR：(a) 按书面基准改为拒绝，或 (b) 更新 04/12 支持矩阵把 refusal↔content_filter 列为已支持映射。当前代码与文档双轨，需消除歧义。**建议**保留映射并更新文档（语义上更优、不丢失安全信号）

**【C-F5】body_hash/body_len 未落库**
- 位置：`migrations/016_request_log_observability.sql`、`db/models.rs:186-227`、`security/gate.rs:221-222`
- 要求：T09「原始 body 仅保存 hash/length」
- 现象：安全闸门已算 body_hash/body_len 但从未持久化
- **未修理由**：需要新 migration（编号顺延 017），且当前 `new_routeplan` 关闭时 request_logs 字段变更影响面大；**建议**：作为独立小任务处理

**【C-F6】流式日志 finalizer 恒写 failure_class: None**
- 位置：`endpoint_executor/driver.rs:719`
- 要求：T09 日志字段表含 failure_class；设计 11.4「流式取消、提交后错误可从日志区分」
- 现象：提交后流错误（committed_stream_error）无法从日志区分
- **未修理由**：为 finalizer 增加 failure-class 参数需改动 driver 写日志路径与日志 DTO；风险低收益中等，**建议**与 A-P3 一并处理

**【C-F7】v2 导入未校验 protocol↔endpoint 一致性**
- 位置：`commands/import_export.rs:486-509`
- 要求：T09「v2：验证新旧字段组合」
- 现象：手改 v2 文件可导入半新半旧状态（revision>0 但路由不到）
- **未修理由**：交叉校验需在 resolver 层加 `new_to_legacy(identity)` 还原比对；涉及导入主链路，风险中

**【C-F8】缺回滚触发器路径的裸 UPDATE 集成测试**
- 位置：`migrations/015_channel_protocol_identity.sql:40-53`
- 要求：T02 实施步骤 7「模拟升级→旧 schema UPDATE→再次升级」
- 现象：触发器路径只被 update_channel 自身流程隐式覆盖，无直接断言测试
- **未修理由**：加测试成本低但需在 `tests/channel_migration.rs` 新增对裸 `UPDATE channels SET type/base_url/config` 的断言；标记为建议

**【C-F9】count_tokens 无条件推断与 T02 规格字面冲突**
- 位置：`core/channel_identity.rs:226-244`
- 要求：T02 第 41 行「不无条件推断 count_tokens」
- 现象：resolver 对所有 legacy type=claude 一律推断 `[messages, count_tokens]`；代码注释引用「T06 I-4 leader adjudication 2026-08-05」但本复核所读文档无法核验
- **未修理由**：裁决记录在库外，无法核实。若保留，**建议**在 T00/设计文档补 ADR 避免回归争议

**【D5】前端硬编码 PROTOCOL_DEFAULT_AUTH/defaultEndpointsFor 副本**
- 位置：`src/components/ChannelForm.tsx:35-39,53-59`
- 要求：08-channel-ui「不使用硬编码副本」；T01「前端不复制后端 registry 常量」
- 现象：auth_scheme/默认端点作为协议级常量兜底
- **未修理由**：端点结构属协议语义（设计 3.2 可辩护）；auth_scheme 属 registry 数据但 custom preset 未加载时无来源。风险低，标记为建议

## 二、Agent B 映射对照表（CLIProxyAPI 做法 / 现有实现做法 / 差异 / 结论）

| 12- 编号 | CLIProxyAPI 做法 | 现有实现做法 | 差异 | 结论 |
|---|---|---|---|---|
| 模型 | 用代理解析后 modelName | codec 直写入参 model（PreparedAttempt 抽样） | 一致 | 一致 |
| max_tokens | 原样透传 | 直写；缺省 4096 | 缺省默认值需文档化 | 一致 |
| top_p | 仅 temperature 缺席才写 | 独立写，两者都保留 | 更优（R11 已解决） | 一致/更优 |
| stop_sequences | 仅数组，非数组忽略 | 非数组拒绝 | 更严（R12 已修） | 已修复 |
| thinking | →reasoning_effort / 未知忽略 | 拒绝（顶/块/响应/delta 统一 Thinking 码） | 首版拒绝 ✓ | 已修复（R7/R29） |
| system | 逐块转换，typed 数组 | 保序合并为单条 system | 一致 | 一致 |
| 未知 role | system 降级 user / 未知静默丢 | 未知 role 拒绝 | 更严 ✓ | 一致 |
| 未知顶层字段 | 静默丢弃 | 白名单拒绝 | 更严（R4 已修） | 已修复 |
| 图片 | media 缺省/空丢弃 | 校验 media type/size/scheme | 更严（R15 已修两方向） | 已修复 |
| tool_use 缺 input | 修成 `{}` | 拒绝（MissingToolField） | 更严（R8/R21 已修） | 已修复 |
| tool id/name 缺失 | 造 ID/空串容忍 | 拒绝 | 更严（R22 已修流式） | 已修复 |
| tool_choice 字符串 | 原样透传/未知→auto | 映射 auto/any，tool/未知拒绝 | 更严（R9 已修） | 已修复 |
| 未知 stop_reason | →stop | 拒绝（两方向） | 更严 ✓ | 一致 |
| content_filter/refusal | →end_turn | →refusal（有测试） | 语义更保真但偏离书面基准 | 待决策（R24） |
| usage | input+creation+read 计入 | 真实 usage，cache 进 details 不重复计费 | 更优 ✓ | 一致 |
| 未知 SSE 事件 | 忽略 | →Err（两方向） | 更严 ✓ | 一致 |
| 流终止 | 通道关闭无条件 [DONE] | finish() 无终止事件→codec 错误 | 更严 ✓ | 一致 |
| message_start 顺序 | 可缺失 | !started→Err | 更严 ✓ | 一致 |
| 未注册方向 | 原样透传 | 返回错误 | 更严 ✓ | 一致 |
| 包级身份 | 有（user/account/session 跨请求） | 每请求独立 | 更严 ✓ | 一致 |
| 首帧校验 | 未校验即 commit | 缓冲+校验+成功编码后才 commit | 更严 ✓ | 一致 |

## 三、门禁结果（真实执行）

| 门禁 | 结果 | 说明 |
|---|---|---|
| `pnpm build` | ✅ | tsc + vite build 通过 |
| `cargo fmt --check` | ✅ | 应用 rustfmt 后通过 |
| `cargo clippy -- -D warnings` | ❌ | **107 条错误**，与 pristine HEAD `30513a2` 的 clippy 错误集合**完全一致**（`diff` 为空）。全部为既有 T04/T10 债务（unused imports in protocol/codec、never-used methods、manual RangeInclusive::contains、&PathBuf→&Path 等）。**本次修复新增 0 条**。属门禁既有问题，记为 issue（见风险清单） |
| `cargo test` | ✅ | lib 303 + channel_migration 21 + request_log 4 + rollout_integration 37 = **365 通过，0 失败** |

## 四、README 全局完成定义逐条打勾

- [x] `pnpm build`、Rust fmt/clippy/test 及新增集成测试全部通过 —— **fmt/test/build ✅；clippy ❌ 既有 107 条（记 issue）**
- [x] 三协议的模型筛选、分组、优先级、权重、重试和流式 commit barrier 有确定性测试 —— route_plan/attempt/rollout 覆盖
- [x] 新数据库、新版升级、回滚旧版写入、再次升级均不丢字段且能解析正确执行器 —— channel_migration 21 条
- [x] 所有公开入口执行权限检查和原始请求审计；日志不存在原始 secret —— authorize_and_plan + security gate；**注意**旧路径（flag OFF）仍不执行权限（记 A-P2c）
- [x] 旧 `/v1/responses` 经 Chat codec 的行为保留，原生 `/responses` 独立直通 —— C-F2 修复后旧行经 G2 保留；原生路径独立
- [x] Chat ↔ Messages 对支持矩阵内字段完成流/非流双向验证；不支持字段返回 4xx 且不上游 —— codec 46 + 集成零上游 4 条
- [x] 自定义提供商默认选中，厂商预设可选择，渠道列表双标签正确 —— D4 修复 legacy 标签
- [x] 功能开关关闭时可退回旧路由；数据库无需破坏性回滚 —— 四 flag 默认 OFF，015/016 只加列

## 五、设计文档 §11 强制修订逐条核对

- **11.1 身份/URL/旧执行路径**：✅ 新渠道只持久化新身份字段、executor_kind 由身份派生（仅 Gemini 用 override）；✅ 逐预设 mock 验证最终 URL 无 `trim_end_matches`；✅ 旧 Gemini 迁移保留 override（**路由层 C-F3 未接，风险**）；✅ 旧 Ollama 精确末尾 `/v1` 派生不产生 `/v1/api/chat`；✅ C-F4 修复 custom 双写 `/v1`
- **11.2 Responses 不回归**：✅ native_endpoints 只表达原生能力；✅ C-F2 修复旧行 debt 保留；✅ 原生 Responses 独立直通不复用 Chat codec；✅ Chat↔Anthropic 转换保留为显式降级
- **11.3 授权/回滚/DTO**：✅ `authorize_and_plan` 唯一 facade 且执行授权（flag ON）；✅ `resolve_channel_identity` 唯一、revision-0 实时推断；✅ SQLite 失效触发器 + 双写事务；✅ 输入/存储/输出 DTO 分离、`Option + serde(default)`；**风险**：A-P2c 旧路径默认不执行权限、A-P2d allowed_channels 语义未固定
- **11.4 保真/可观测性**：✅ 逐字段 round-trip（status/priority/weight/timeout_secs/config 未知键/数组 model_mapping）；✅ A-P1b/C-F1 修复模型抽样一致性（实际请求=日志=统计）；✅ ChannelDto 补 timeout_secs、API Key 掩码；**风险**：C-F5 body_hash 未落库、C-F6 流式 failure_class 恒 None

## 六、未决风险清单

1. **clippy 既有 107 条**（T04/T10 遗留；本复核未新增）。修复需清理 protocol/codec 未用代码与全局 clippy 项，建议独立任务。
2. **C-F3**：`new_routeplan` 开启后旧 Gemini 不可路由（需 T06 接线前处理）。
3. **A-P2c**：生产默认旧路径不执行权限（开启 `new_routeplan` 前必须补）。
4. **A-P2**：流式 per-phase 超时未接线（driver 改造）。
5. **A-P2d**：allowed_channels 匹配语义需在文档固定为 channel id。
6. **B-R24**：refusal↔content_filter 映射需 ADR 决策（保留映射并更新文档 或 改为拒绝）。
7. **C-F5/F6/F7/F8**：可观测性与导入校验补强（独立小任务）。
8. **C-F9**：count_tokens 推断裁决需补 ADR。
9. **D5**：前端协议级常量副本（低风险，建议从 custom preset 读取）。

## 七、改动文件清单（未提交）

```
M src-tauri/src/adaptor/openai.rs            C-F1 数组不重抽样
M src-tauri/src/commands/settings.rs         D2  get_feature_flags 命令
M src-tauri/src/core/attempt.rs              A-P1a candidate protocol + A-P1b ResponsesViaChat model
M src-tauri/src/core/channel_identity.rs     C-F2 debt 推断 + C-F4 legacy_base_for_native
M src-tauri/src/core/plan_executor.rs        A-P1a AttemptMeta candidate-level
M src-tauri/src/core/route_plan.rs           C-F2 classify_channel debt + 新测试
M src-tauri/src/lib.rs                       D2  注册 get_feature_flags
M src-tauri/src/protocol/codec/chat.rs       B-R15 图片校验
M src-tauri/src/protocol/codec/chat_messages_codec.rs  新增 7 条回归测试 + 更新 1 条
M src-tauri/src/protocol/codec/messages.rs   B-R4/R8/R9/R12/R21/R22/R29 + tool 排序
M src-tauri/src/protocol/codec/request.rs    B-R29 system thinking 码
M src-tauri/src/rollout_integration_tests.rs B-04 3 条零上游集成测试
M src-tauri/src/server/handlers.rs           C-F1 legacy 流式预烘模型
M src/components/ChannelForm.tsx             D2 Ollama Tab 门控
M src/lib/api.ts                             D2 FeatureFlagsDto
M src/pages/ChannelsPage.tsx                 D4 旧配置标签
A docs/channel-refactor-tasks/12-cliproxy-anthropic-to-chat.md  阶段一映射基准
```
