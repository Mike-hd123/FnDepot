# T04：Chat Completions ↔ Anthropic Messages 严格 codec

## 目标

实现版本化 `chat_to_messages_v1` 和 `messages_to_chat_v1`，覆盖请求、非流响应和流式响应；所有不支持特征在访问上游前明确拒绝，不静默丢字段、不伪造工具参数。

## 依赖

- T00 的 codec 契约。
- T03 的 `request_features` 与安全审计输出契约。
- 不依赖 RoutePlan；codec 必须可独立测试并在未接生产路由时合并。

## 文件所有权

- 新增 `src-tauri/src/protocol/codec/`，包含 registry、错误、feature matrix 和两个方向模块。
- 重构 `src-tauri/src/protocol/anthropic.rs`、`src-tauri/src/protocol/mod.rs` 中可复用的严格转换代码。
- 新增 codec 单元和属性测试。
- 不修改 handlers、dispatcher、DB、React UI。
- 不扩展现有 `ClaudeAdaptor`；T06 负责停用其转换职责。

## 参考边界

参考 CLIProxyAPI 的方向注册、请求上下文传入响应 codec、并行工具调用 index、SSE 状态机和 usage 映射。不得照搬：未知字段忽略、非法 arguments 修成 `{}`、未知 event 忽略、未知 finish reason 当正常结束。

## 公共接口

定义：

- `CodecRegistry::prepare(downstream_endpoint, upstream_endpoint, version, audited_request)`
- `PreparedConversion { encoded_request, context, report }`
- `NonStreamDecoder::decode(status, headers, body)`
- `StreamDecoder::feed(bytes)`、`finish()`
- `UnsupportedFeatures { features, json_pointers, message }`
- `ConversionReport { rejected, normalized, codec_version }`

不存在 codec 时返回错误，不透传原 payload。

## 首版支持矩阵

请求支持：

- system/developer 保序转换；developer 不降为普通 user。
- user/assistant 文本及内容块。
- user base64/URL 图片，校验 role、media type、data URL 和大小。
- function tools schema、可明确映射的 tool choice。
- assistant tool_calls/tool_use、tool result，保持 ID/name/顺序。
- max token、temperature、top_p、stop sequences。
- stream 标志与映射后的单一 upstream model。

响应支持：

- 文本。
- function tool call/result。
- stop/end_turn、length/max_tokens、tool_calls/tool_use 的明确映射。
- 上游真实 usage；Anthropic cache creation/read 可进入 OpenAI usage details，但计费不得重复。
- 流式 text、tool arguments、usage、stop 和终止事件。

首版拒绝：thinking/reasoning、structured output、OpenAI built-in tools、document/PDF、prompt cache annotations、未知 role/block/event、content_filter/refusal 无目标安全语义、非法或非 object tool arguments、缺失 tool ID/name、无法映射的 beta feature。

## SSE 状态机

- 输入按 bytes 累积，兼容 UTF-8 codepoint、SSE field 和 CRLF/LF delimiter 任意分片。
- 每请求独立状态，不使用包级可变身份。
- tool call 按 source index 累积；ID/name/arguments 完整且 JSON object 合法后才完成块。
- 第一帧验证失败返回 codec 错误，供 T05/T06 在 commit 前 failover。
- commit 后 malformed/unknown event 转为目标协议 error event并终止，不伪造成功。
- `[DONE]`、`message_stop` 和目标终止事件 exactly once。

## 实施步骤

1. 从现有严格 Messages→Chat 代码提取 registry 与通用错误，不降低其拒绝策略。
2. 实现请求 feature analyzer，逐顶层字段和内容块分类 supported/rejected。
3. 实现两个方向的请求转换，生成 report。
4. 实现非流响应转换，保留错误 status 和目标错误格式。
5. 实现两个方向的 SSE byte parser 与 per-request state。
6. 删除任何随机模型映射；codec 只使用 PreparedAttempt 传入模型。
7. 对现有 `ClaudeAdaptor` 行为写回归测试，证明新 codec 修复 tools/images/SSE/finish reason。
8. 建立 CLIProxyAPI 对照 fixture，但期望结果按 WaLiAPI fail-closed 契约，而非照搬其容错结果。

## 验收标准

- 支持矩阵内字段双向流/非流通过。
- 每个拒绝字段返回具体 JSON pointer 和稳定错误 code，上游调用为零。
- invalid arguments 不被改成 `{}`。
- 未知 finish reason 不变成正常 stop/end_turn。
- SSE 任意字节分片输出确定；终止事件一次。
- usage 来自上游真实值，不用字符数估算。

## 测试命令

- `cargo test protocol::codec --manifest-path src-tauri/Cargo.toml`
- `cargo test chat_messages_codec --manifest-path src-tauri/Cargo.toml`

## 交接输出

提供公开 codec API、逐字段支持矩阵、错误 code、流状态图、fixture 清单和未支持功能列表。
