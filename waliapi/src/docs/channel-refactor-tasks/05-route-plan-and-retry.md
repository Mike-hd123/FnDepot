# T05：模型优先 RoutePlan 与重试状态机

## 目标

替换扁平候选队列，建立统一 `authorize_and_plan()`：先权限和模型候选，再原生/转换分组，最后组内 priority/weight；实现明确错误分类、组内/组间预算与流式 commit barrier。

## 依赖

- T01 领域类型与模板能力。
- T02 `resolve_channel_identity()`。
- T03 `AuditedRequest`。
- 与 T04 对接 codec capability registry；RoutePlan 可先使用接口 mock。

## 文件所有权

- 重构 `src-tauri/src/core/dispatcher.rs`。
- 新增 `src-tauri/src/core/route_plan.rs`、`attempt.rs`、`stream_supervisor.rs`。
- 修改 `src-tauri/src/core/proxy.rs`，逐步改为 plan executor facade。
- 修改 `src-tauri/src/server/handlers.rs` 的共享规划/重试骨架；端点 HTTP 发送细节由 T06 串行接手。
- 不修改 DB schema、codec 内部、React UI。

## 授权语义

在候选构建前检查：API Key status、expires_at、quota、allowed_models、allowed_channels。空 allowed 数组表示不限制。授权失败不得访问上游。

allowed model 使用下游请求模型/映射源名称检查；allowed channels 在模型匹配前过滤，防止重试落到未授权渠道。

## 模型候选

- 渠道 models 包含请求模型则命中。
- model_mapping 包含请求模型源名称则命中。
- 旧渠道 models 为空按 wildcard。
- native endpoints 空时通过 identity resolver 推断；无法推断则不进入候选并记录配置错误。
- 数组 mapping 在 `PreparedAttempt` 创建时只抽样一次；同一 attempt 的 body、日志、统计共享该模型。

## RouteGroup

- Chat G1：OpenAI Chat；G2：Anthropic Messages codec；Ollama native 由 T06 完成后按明确策略接入。
- Responses G1：OpenAI native Responses；G2：逐记录标记 `responses_via_chat_v1` 的旧兼容 codec；不接 Anthropic。
- Messages G1：Anthropic Messages；G2：OpenAI Chat codec。
- Count Tokens：仅 Anthropic count_tokens。
- Embeddings：仅 OpenAI embeddings。

G1 和 G2 各自按 priority 降序分 tier；同 tier 按 weight 无放回随机排列。测试使用注入 seed 或 deterministic RNG。

## 失败和预算

实现 T00 错误分类。401/403 可尝试同组下一渠道，但不跨组。404 必须由 endpoint executor/错误解析证明为路径不存在才归 endpoint unsupported。

每组独立最大尝试数，请求另有总上限；候选同一请求只尝试一次。`background/store` 等非幂等 Responses 禁止自动重试。

## 流式 supervisor

实现 Planned→Connecting→Headers→FirstFrameValidated→Committed→Streaming→Completed/Aborted。commit 前可换候选，commit 后上游/codec 错误只能发目标错误并关闭。

区分 connect timeout、header/first-frame timeout、stream idle timeout；渠道 `timeout_secs` 不直接作为长流总寿命。客户端断开取消上游并记录一次 client_cancelled。

## 实施步骤

1. 定义 EndpointKind、RouteGroup、RoutePlan、PreparedAttempt、FailureClass。
2. 实现 authorize、模型候选和组构建的纯函数。
3. 将 priority/weight 算法迁入每组，保留当前语义。
4. 实现 per-group 和 total budget。
5. 实现失败分类与转换组进入条件。
6. 实现 stream supervisor 和 commit barrier，不发送真实上游请求。
7. 将 Chat/Messages/Responses 的公共选择逻辑迁到 facade；T06 再接 executor。
8. 增加计划/状态机确定性测试。

## 验收标准

- 高优先级 G2 不能越过低优先级 G1。
- 原生组无模型候选时直接进入转换组。
- 原生组全部发生可降级故障后进入转换组；400/422 和跨组 401/403 不进入。
- 权重只在同协议同 priority tier 生效。
- 实际上游模型等于日志/统计模型。
- 首帧无效可换候选；下游收到一字节后第二上游调用为零。
- allowed model/channel 和 expires_at 在所有入口一致执行。

## 测试命令

- `cargo test route_plan --manifest-path src-tauri/Cargo.toml`
- `cargo test stream_supervisor --manifest-path src-tauri/Cargo.toml`
- `cargo test dispatcher --manifest-path src-tauri/Cargo.toml`

## 交接输出

提供 RoutePlan JSON/debug 示例、错误矩阵、默认预算、流式状态转换表、功能开关接入点和旧 Dispatcher 待删除清单。
