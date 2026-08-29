# T06：协议端点执行器与 Responses/Ollama

## 目标

将 URL、鉴权、HTTP 发送与业务 codec 分离为 EndpointExecutor；接入原生 OpenAI Chat/Responses/Embeddings、Anthropic Messages/Count Tokens、Ollama `/api/chat`，并把 T04/T05 的 codec 与 RoutePlan 接入生产 handlers。

## 依赖

- T01、T02、T03、T05。
- T04 必须通过独立 codec 测试后才能开启跨协议功能开关。
- 与 T05 串行修改 handlers/core proxy。

## 文件所有权

- 新增 `src-tauri/src/executor/` 或重构 `src-tauri/src/adaptor/` 为 endpoint executors。
- 修改 `src-tauri/src/server/handlers.rs`、`router.rs`、`core/proxy.rs`。
- 修改 adaptor module 导出，保留 legacy Gemini executor。
- 新增 mock-upstream 集成测试。
- 不修改 migration、React UI、provider templates 数据内容。

## Executor 边界

Executor 输入为 PreparedAttempt 与安全 headers，输出原始上游 status/headers/body 或 byte stream。Executor：

- 按规范 base URL 与 endpoint 构造最终 URL。
- 应用 auth scheme、timeout、必要版本 header。
- 发送 HTTP、保留可安全透传 response headers。
- 不做模型随机、不做协议转换、不做路由选择、不做日志脱敏。

## 原生端点

- OpenAI Chat：`/chat/completions`
- OpenAI Responses：`/responses`，原始 Responses 请求直发，流式事件原样透传并由 supervisor 验证首帧。
- OpenAI Embeddings：`/embeddings`
- Anthropic Messages：按 preset 完整 endpoint rule，支持请求 header/query 安全转发。
- Anthropic Count Tokens：仅能力声明存在时调用。
- Ollama native：`/api/chat`；模型枚举 `/api/tags`。
- Legacy Gemini：保留 `generateContent?key=` 与 stream path，只有 identity override 能选中。

## 协议转换接入

- Chat G2 使用 `chat_to_messages_v1` 编码请求，响应经对应反向 decoder 输出 OpenAI Chat。
- Messages G2 使用 `messages_to_chat_v1`，响应输出 Anthropic Messages。
- Responses G2 仅旧 `responses_via_chat_v1`，保持当前兼容行为但接入原始请求安全审计和明确 codec capability。
- Responses→Anthropic 不接入。
- Ollama native 必须具备下游 Chat ↔ `/api/chat` 的严格转换或明确使用 Ollama OpenAI-compatible 配置。原生 Tab 的功能开关在 executor/codec 测试通过前关闭。

## 错误处理

解析上游错误为 T05 FailureClass。不得把所有 >=400 都重试。上游 credential 401/403 不能伪装成本地 API Key 错误。404 只有端点路径明确不存在时为 endpoint unsupported。

流式第一完整 frame 在提交下游前验证；原生流保留字节，转换流通过 codec 输出目标事件。commit 后不可重路由。

## 实施步骤

1. 定义 Executor trait 与协议实现，迁移 URL/auth/HTTP 逻辑。
2. 将 legacy Gemini 隔离为 override executor。
3. 实现原生 Responses；删除入口无条件先转 Chat 的行为。
4. 保留逐记录 legacy Responses→Chat 路径。
5. 接入 Chat/Messages 双向 codec 与 RoutePlan groups。
6. 实现 Ollama native executor、模型枚举和下游 Chat 转换；保持 WaLiAPI 不公开 `/api/chat`。
7. 接入 stream supervisor、取消和 exactly-once finalizer。
8. 移除 `is_native_anthropic_channel(type==claude)` 和 `get_adaptor(type)` 的生产选择职责。
9. 为每个 preset 建 mock URL、auth header、body 和流测试。

## 验收标准

- Chat 请求优先真实 Chat endpoint；Messages 优先 Messages；Responses 优先真实 Responses。
- 两个 DeepSeek 渠道分别为 Anthropic/OpenAI 时，Claude 请求先走 Anthropic，无候选或可降级失败才走 OpenAI。
- 原生 Responses 不经过 Chat codec，未知原生事件保持透传。
- 旧 Responses→Chat 仍可用且只对标记记录生效。
- Chat↔Messages 不支持字段 4xx、上游零调用。
- Ollama 原生配置能参与下游 Chat 路由，但 WaLiAPI 不新增公开 Ollama endpoint。
- 所有 executor URL 与鉴权经 mock 验证，不访问真实付费服务。

## 测试命令

- `cargo test endpoint_executor --manifest-path src-tauri/Cargo.toml`
- `cargo test protocol_routing_integration --manifest-path src-tauri/Cargo.toml`
- `cargo test stream_failover --manifest-path src-tauri/Cargo.toml`

## 交接输出

提供 executor 矩阵、最终 URL/auth 测试表、生产 handler 新流程、legacy adaptor 保留清单、功能开关状态和不支持端点列表。
