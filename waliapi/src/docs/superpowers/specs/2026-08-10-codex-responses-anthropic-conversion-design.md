# Codex 双路径修复: Responses→Anthropic Messages 转换 + codexAuth validator

> 日期: 2026-08-10
> 分支: v0.1.8-codex-auth
> 状态: 已批准设计

## 1. 背景与问题

codex CLI 0.147.0 配置 `wire_api = "responses"` 指向 WaLiAPI, 高频报错两类:

1. **400 route_plan_error — `unsupported provider request field at /parallel_tool_calls`**
   codex 请求路由到 **codexAuth 路径** (auth 账号 → chatgpt.com backend) 时, `CodexProvider::outbound` 调用 `validate_backend_request`, 而 codex 必带的 `parallel_tool_calls` 不在 ALLOWED 白名单 → 整请求被拒。
2. **503 — `No available upstream candidate supports this endpoint for model: deepseek-v4-flash-free`**
   模型 `deepseek-v4-flash-free` 通过 model_mapping 命中 4 个 9router anthropic 渠道 (候选存在), 但 `classify_channel` 对 `EndpointKind::Responses` 没有 anthropic 分支 → 分组为空 → `PlanError::NoEndpointSupported`。

### 1.1 铁证: codex 真实请求 (2026-08-10 本地 capture server 实测)

顶层字段: `model, instructions, input[message×3], tools[function×21], tool_choice, parallel_tool_calls(bool·恒定), reasoning:{effort:high}, store, stream, include:["reasoning.encrypted_content"], prompt_cache_key, client_metadata`。
可选 (其他配置/版本): `stream_options, service_tier, text`。
Headers: `Accept: text/event-stream, Originator, Session-Id, Thread-Id, X-Client-Request-Id, X-Codex-Turn-Metadata, X-Codex-Window-Id, User-Agent: codex_exec/0.147.0`。

与官方源码 `codex-rs/codex-api/src/common.rs::ResponsesApiRequest` 完全一致。

### 1.2 两条路径

| 路径 | 上游 | 服务模型 | 触发报错 | 修复 |
|---|---|---|---|---|
| ① codex → WaLiAPI → Anthropic 协议 | 9router 渠道 (Messages 协议) | `deepseek-v4-flash-free` (model_mapping → `oc/...`) | 503 | V5 Responses→Messages 转换 |
| ② codex → WaLiAPI → codexAuth | auth 账号 (chatgpt.com backend) | gpt-5.x | 400 `/parallel_tool_calls` | validator 白名单扩展 |

两条路径互不冲突 (服务不同模型), 本次**同时修复**。

## 2. 设计决策

| 决策 | 结论 |
|---|---|
| 路径①实现方案 | **组合现有转换器, 镜像 V4** (`messages_to_responses_v1` 同构) |
| `reasoning.effort` | 映射为 anthropic `thinking` (`{type:"adaptive"}` + `output_config.effort`, 走现有 `chat_to_messages` 机制, 注释明指 9router `claude-adaptive`) |
| `parallel_tool_calls:false` | **丢弃**, 上游并行执行 (少一个 beta 头兼容风险; codex 本身能处理并行 function_call) |
| `max_tokens` 默认 | **32000** (仅作用于 V5 新路径; legacy `responses_via_chat` 保持 4096) |
| 路径②字段处理 | `parallel_tool_calls`/`reasoning`/`prompt_cache_key`/`stream_options`/`service_tier`/`text` 直接**转发**给 chatgpt 后端 |

## 3. 路径②: codexAuth validator 修复

### 3.1 `src-tauri/src/auth_provider/codex_backend.rs` `validate_backend_request`

- `ALLOWED` 增加: `parallel_tool_calls`, `reasoning`, `prompt_cache_key`, `stream_options`, `service_tier`, `text`。
- 字段直接转发 (codex 直发同一后端, 后端接受); `store`/`stream` 仍强制 `false`/`true`。
- 更新测试 `backend_request_rejects_public_responses_controls` (现断言拒绝 `reasoning`) → 断言完整 codex 请求体可通过且字段保留。

### 3.2 效果

codex 用 gpt-5.x 模型走 auth 账号 (Native Responses passthrough, `classify_auth_account` 不变) 不再被 400 拒。无路由/转换改动。

## 4. 路径①: V5 `responses_to_messages_v1` 转换

### 4.1 组合构件

| 构件 | 实现 | 来源 |
|---|---|---|
| `encode_responses_to_messages` | `responses_to_openai` → `encode_chat_to_messages` | 复用 + 扩展 |
| `MessagesResponsesNonStreamDecoder` | `decode_messages_response_to_chat` → `openai_to_responses` | 复用 |
| `MessagesResponsesStreamDecoder` | `MessagesStreamDecoder` → (新) `ChatToResponsesStreamDecoder` | 复用 + 新增 |

### 4.2 扩展 `responses_to_openai` (protocol/mod.rs)

`SUPPORTED_TOP_LEVEL` 当前含 `model/input/instructions/tools/tool_choice/max_output_tokens/stream/temperature/top_p`, 会拒绝 codex 字段。改动:

- 容忍并**丢弃**: `parallel_tool_calls`, `store`, `include`, `prompt_cache_key`, `client_metadata` (Chat 无对应, 记入 ConversionReport)。
- **映射**: `reasoning.effort` → 顶层 `reasoning_effort` (供 `encode_chat_to_messages` 转成 thinking)。
- `max_tokens` 默认保持 4096 (legacy `responses_via_chat` 路径不变)。`responses_to_openai` 恒产出 `max_tokens`; **V5 组合包装 `encode_responses_to_messages`** 内: 下游未携带 `max_output_tokens` 时把产物 `max_tokens` **覆盖**为 32000, 已携带则尊重原值。

### 4.3 路由接线

- **`classify_channel`** (core/route_plan.rs): `EndpointKind::Responses` 分支增加:
  `anthropic && has("messages") && flags.cross_protocol_codec` → Conversion 组, `UpstreamProtocol::Anthropic`, endpoint `"messages"`。
- **`codec_direction`** (core/attempt.rs): 增加 `(EndpointKind::Responses, UpstreamProtocol::Anthropic) → Some((Downstream::Responses, Upstream::Messages, "responses_to_messages_v1"))`。
- **registry.rs**: 注册 `(Downstream::Responses, Upstream::Messages)` 为 V5; 加便捷方法 `responses_to_messages()`。
- **sse.rs**: `SseMode` 新增 `MessagesToResponses`, 纳入 codec 转换通路 (`as_str`, `push`/`finish` 的 conversion 分支, `decoder_for`)。
- **driver.rs `sse_mode_for`**: `Some("responses_to_messages_v1") => SseMode::MessagesToResponses`。

### 4.4 流式转换 (关键新增)

`MessagesResponsesStreamDecoder` (链式, 镜像现有 `ResponsesMessagesStreamDecoder`):

```
Messages SSE → [MessagesStreamDecoder] → Chat SSE 串 → [ChatToResponsesStreamDecoder·新] → Responses SSE
```

`ChatToResponsesStreamDecoder` 包装 `convert_openai_sse_to_responses` + `StreamState` + 内容累积 + SSE record 缓冲 (镜像 sse.rs `encode_responses_buffered` 逻辑), 实现 `StreamDecoder` trait。

## 5. 数据流

**请求方向** (codex → WaLiAPI → 9router):
```
codex POST /v1/responses {model:deepseek-v4-flash-free, input, instructions, tools,
  tool_choice, parallel_tool_calls:false, reasoning:{effort:high}, store, stream, include,
  prompt_cache_key, client_metadata}
  → 路由: 4×9router anthropic 命中 model_mapping → classify_channel 新分支 → Conversion/Anthropic/messages
  → attempt: codec_direction → responses_to_messages_v1
      responses_to_openai:  input→messages, instructions→system, tools→nested,
                             reasoning.effort→reasoning_effort, 丢弃 store/include/prompt_cache_key/parallel_tool_calls
      encode_chat_to_messages: reasoning_effort→thinking{adaptive}, max_tokens=32000, system, tools, tool_choice
  → 上游 POST {base}/messages, 头 x-api-key + anthropic-version, model=oc/deepseek-v4-flash-free
```

**响应方向** (9router → WaLiAPI → codex):
```
9router Messages SSE (message_start / content_block_start / content_block_delta /
  content_block_stop / message_delta / message_stop / ping)
  → [MessagesStreamDecoder] → Chat SSE 串
  → [ChatToResponsesStreamDecoder] → Responses SSE (response.created / response.output_item.added /
       response.output_text.delta / response.function_call_arguments.delta / response.completed / [DONE])
```

模型映射 `deepseek-v4-flash-free → oc/deepseek-v4-flash-free` 由 `resolve_upstream_model` (attempt.rs) 自动应用, codec 不重映射。

## 6. 错误处理

- 上游 4xx/5xx: 沿用现有 attempt 流程 (可重试换候选 / terminal 400)。
- thinking 兼容风险: `thinking:{type:adaptive}` 为现有机制, 注释明指 9router `claude-adaptive`, 预期兼容; 若上游拒绝, 报错暴露, 降级开关不在本次范围。
- 未知 Responses 顶层字段: fail-open 丢弃 + ConversionReport。
- 流中途断开/不完整: `StreamDecoder.finish()` 报错 → pre-commit failover。

## 7. 测试策略

- **codec 单测** (protocol/codec/responses_codec.rs):
  - `encode_responses_to_messages`: 用实测捕获的 codex 请求体 → 断言 Messages 各字段 (system / thinking / max_tokens=32000 / tools / tool_choice / model=oc/…)。
  - `MessagesResponsesNonStreamDecoder`: Messages 响应 → Responses 响应。
  - `MessagesResponsesStreamDecoder`: 喂 9router Messages SSE → 断言 Responses SSE 事件序列; 跨 split 边界喂字节 (镜像 `responses_stream_terminal_usage_once_and_any_split`)。
- **registry 单测**: `(Responses, Messages)` 可 prepare; 未注册方向仍报错。
- **route_plan 单测**: anthropic channel + Responses endpoint → Conversion 组; `cross_protocol_codec` off → 无组 (503)。
- **validator 测试更新** (路径②)。
- **集成测试** (endpoint_executor/integration_tests.rs): mock Messages 上游 → 端到端下游 Responses SSE。
- **真实连通 (人工)**: codex 配 `deepseek-v4-flash-free` 跑一次真实 turn, 验证 9router 接受 adaptive thinking。

## 8. 范围外

- 9router 不支持 thinking 时的降级开关。
- Responses→Messages 之外的更多 Responses 特性 (encrypted_content、background、流式 store)。
- 其他下游端点 (Messages/Chat) 经 anthropic 渠道的响应转换。
