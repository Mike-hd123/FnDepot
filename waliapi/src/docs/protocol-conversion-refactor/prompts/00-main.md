# 主会话提示词 · 协议转换核心模块重构

> 用法：把本文件整段交给 Codex，严格按权威设计逐 Cell 实施。
> 权威设计：`docs/superpowers/specs/2026-08-11-protocol-conversion-core-refactor-design.md`
> 本提示已按该设计的复核完善稿同步；若后续再次冲突，始终以权威设计为准。

## 一、角色与目标

你是 WaLiAPI 的资深 Rust 后端工程师。工作分支：`v0.1.8-protocol-conversion-refactor`。

目标：把 Chat Completions、Anthropic Messages、OpenAI Responses 的协议转换收敛到唯一核心，形成完整 3×3 矩阵（3 identity + 6 conversion）。Responses↔Messages 必须是真正单跳，不经过 Chat；普通渠道、auth 账号、rollout fallback 必须消费同一个 prepared codec。

## 二、权威输入

冲突优先级：

1. `docs/superpowers/specs/2026-08-11-protocol-conversion-core-refactor-design.md`
2. 该设计 §2.1 的 ADR delta（覆盖 T00/T04 的首版旧边界）
3. `docs/channel-refactor-tasks/00-architecture-decisions.md`
4. `docs/channel-refactor-tasks/04-codec-chat-messages.md`
5. `docs/auth-codex/ADRs.md`（ADR-31/33/35/36/37）
6. `docs/auth-codex/02-routing-compat-review.md`

开始前完整阅读这些输入。仓库若有 `.codegraph/`，理解或定位代码时必须先用 `codegraph explore`，再用 `rg`/定点读取补充。

## 三、强制工作方法

P0 只冻结当前绿基线，不适用红测试要求。每个实现 Cell（C1-C6）：

1. 先写能证明目标行为的测试并确认红。
2. 写最小实现使其变绿。
3. 重构，但不得改变已验证语义。
4. 运行该 Cell 相关 test/fmt/clippy/grep/范围门禁。
5. 保存验收证据后才能进入下一 Cell。

使用以下 superpowers 技能：

- `superpowers:test-driven-development`
- `superpowers:executing-plans`
- `superpowers:systematic-debugging`
- `superpowers:verification-before-completion`
- `superpowers:requesting-code-review`
- `superpowers:receiving-code-review`
- `superpowers:finishing-a-development-branch`（全部完成时）

设计已冻结，禁止重新 brainstorming。发现权威设计未覆盖且会改变协议语义、范围或数据所有权的问题时，停止相关 Cell，列出证据并请用户拍板。

## 四、已经复核的现状事实

1. `CodecRegistry::prepare` 当前创建 `encoded_request/context/report/non_stream/streaming`，但 `build_prepared_attempt` 只保存 body/report/string label，decoder 与 context 随即 drop。
2. registry 的 Chat↔Messages request encoder 正确，但 V1/V2 response decoder 当前绑定反了；executor 因绕过 registry 而暂时掩盖问题。
3. `registry::Version("v1")` 未参与查表，`report::CodecVersion(1,0)` 又不能表达方向/native；执行层实际靠 `Option<String>`。
4. V4 Messages→Responses 与 V5 Responses→Messages 的 request/non-stream/SSE 都是经 Chat 的组合。
5. `driver::sse_mode_for`、`sse::decoder_for`、executor `decode_non_stream` 重复根据字符串选择协议行为。
6. `server/handlers.rs` rollout fallback 直接调用 `responses_to_openai/openai_to_responses`，并有第二套 Chat SSE→Responses pipeline。
7. auth provider 的 `validate_backend_request` 已是可失败 allowlist，并最终强制 `stream:true/store:false`；executor 还有重复强制流逻辑。
8. auth 非流必须保留 Responses SSE accumulator，再把完整 Responses body 交给同一个 prepared non-stream decoder。
9. `UpstreamProtocol::OpenAI` 有 chat_completions/responses 两种 endpoint，不能只按 upstream enum 映射协议。
10. CountTokens/Embeddings 不属于三协议矩阵，继续走现有 native passthrough，`protocol_codec=None`。

## 五、设计模式与结构约束

这次不是把 `match` 从一个文件搬到另一个文件，而是按变化轴重构：

- **Facade**：所有新 consumer 只调用 `CodecRegistry::prepare_pair` 与 `PreparedCodec`，不得调用具体协议转换函数；旧五参数 `prepare` 仅由 Adapter 兼容层保留，禁止新增使用。
- **Strategy**：定义 `CodecDirection` trait；每个 request 方向一个 strategy，同时成对拥有 request encoder 与反向 non-stream/stream response factory。
- **Typed Registry**：registry 只做 `(Protocol, Protocol) -> &'static dyn CodecDirection`；选择一次后 consumer 不得按 label 二次推导。
- **Factory Method**：`PreparedCodec::new_*_decoder()` 捕获 prepare context；每个 stream/retry 创建独立状态。
- **Adapter / ACL**：route enum 映射只在 `core/protocol_boundary.rs`；legacy handler 与 auth provider 只做外层适配，不泄漏进 strategy。
- **State**：每个 SSE 方向用独立 state object 表达事件次序、block 生命周期和 terminal；复杂 phase 用 enum/transition。允许保留职责清晰的成熟 state struct，不为套模式机械改写；pump 不判断具体协议事件。
- **Identity / Null Object**：同一 `IdentityDirection` 类型提供 Chat/Messages/Responses 三个 static 实例，各自自报 pair；不保留 Native consumer special case。
- **Ports and Adapters**：`NonStreamDecoder`/`StreamDecoder` ports 放在独立 `ports.rs`；executor 依赖 ports，不依赖 `chat.rs/messages.rs/responses_codec.rs` primitives。
- **Value Objects**：`Protocol/CodecId/Usage/stop category/tool index` 取代原始字符串和无约束 tuple。

目标布局以权威设计 §4.4 为准：strategy wiring 进入 `protocol/codec/directions/*`；现有 `chat.rs/messages.rs/responses_codec.rs` 可作为迁移期 primitives 保留，但 production consumer 不得直接依赖。

禁止万能 IR、巨大 source/target match、全局可变 registry、service locator、共享 `Arc<Mutex<StreamDecoder>>`，也禁止把 auth policy 伪装成 codec decorator。

## 六、必须落地的核心 API

### 6.1 类型

```rust
enum Protocol { Chat, Messages, Responses }

enum CodecId {
    Native,
    ChatToMessagesV1,
    MessagesToChatV1,
    ChatToResponsesV1,
    ResponsesToChatV1,
    MessagesToResponsesV1, // 仅 legacy fixture/log 识别
    ResponsesToMessagesV1, // 仅 legacy fixture/log 识别
    MessagesToResponsesV2,
    ResponsesToMessagesV2,
}
```

正式 label 由 `CodecId::label()` 唯一生成。删除无效 `Version` 参数与执行选择用的 runtime string truth。

### 6.2 所有权

`PreparedCodec` 保存：typed id、downstream/upstream、原 prepare context、`&'static dyn CodecDirection` strategy。它可 Clone，但不共享有状态 decoder。

```rust
PreparedConversion { encoded_request, report, codec: PreparedCodec }

PreparedCodec::new_non_stream_decoder()
PreparedCodec::new_stream_decoder()
```

`PreparedAttempt.protocol_codec`：

- Chat/Messages/Responses 必为 `Some(PreparedCodec)`，identity 也不例外；
- CountTokens/Embeddings 为 `None`；
- 三协议 endpoint 出现 `None` 必须 fail-closed。

禁止用 `Arc<Mutex<Box<dyn StreamDecoder>>>` 伪造 Clone。每次 stream/retry 必须从 immutable factory 创建新状态机。

### 6.3 response API

```rust
NonStreamDecoder::decode(body) -> Result<DecodedResponse, DecodeError>
DecodedResponse { body, usage }

StreamDecoder::feed/finish(...) -> Result<Vec<String>, DecodeError>
StreamDecoder::usage() -> Option<Usage>
```

prepare/provider preflight error → CallerTerminal；上游 response decode error 在 commit 前 → UpstreamProtocolError；commit 后 → target error + CommittedStreamError，禁止重试。

### 6.4 边界映射

放在 `core/protocol_boundary.rs`：

```rust
downstream_protocol(EndpointKind) -> Option<Protocol>
upstream_protocol(UpstreamProtocol, upstream_endpoint: &str) -> Option<Protocol>
```

必须支持：

- ChatCompletions→Chat、Messages→Messages、Responses→Responses；
- `(OpenAI, chat_completions)`→Chat；
- `(OpenAI, responses)`→Responses；
- `(Anthropic, messages)`→Messages；
- `(Responses, responses)`→Responses。

CountTokens/Embeddings 是明确旁路；三协议遇到 Ollama/未知/不一致 pair 是配置错误，绝不进入 legacy else。

## 七、目标矩阵

箭头表示请求方向；同一 matrix item 的 response decoder 必须反向。

| 下游请求 \ 上游请求 | Chat | Messages | Responses |
|---|---|---|---|
| Chat | Native | ChatToMessagesV1 | ChatToResponsesV1 |
| Messages | MessagesToChatV1 | Native | MessagesToResponsesV2 |
| Responses | ResponsesToChatV1 | ResponsesToMessagesV2 | Native |

Responses↔Messages V2 生产模块不得引用 Chat encoder/decoder/state/helper。允许复用的只有 SSE framing、JSON validation、usage/tool/stop 小型值对象及源/目标协议 parser。

## 八、流式不变量

- 任意 TCP/SSE/UTF-8/JSON 分片得到相同输出。
- first complete record + carry 都由同一个 prepared decoder 在 commit 前处理。
- `codex.rate_limits` 是带外 record：原样保序，但首个业务事件前必须暂存，不能单独触发 commit；暂存受 first-frame deadline 与明确 byte/record 上限约束，禁止无限缓存。
- Chat `[DONE]`、Messages `message_stop`、Responses `response.completed` 与尾随 `[DONE]` 各自至多一次。
- 缺 terminal、重复 terminal、半条 EOF、未知 event、非法 tool arguments 都不能变成正常完成。
- post-commit error 只能发目标协议 error 并结束，不得换候选。

## 九、auth 边界

- codec 核心只做协议语义，不新增 `post_process` hook。
- `codex_backend::validate_backend_request` 继续拥有可失败 allowlist、`stream:true`、`store:false`。
- `AuthService::outbound` 继续拥有 token/header/quota 与 401 内部刷新一次。
- executor 重复 `force_responses_stream` 只能在 provider 防线测试通过后删除。
- 普通/auth 同方向必须从同一个 `PreparedCodec` factory 创建 decoder；用 test-only spy/counter 证明调用路径，而不只比较输出。

## 十、严格 Cell 顺序

### P0：冻结绿基线

只新增 characterization fixtures，不改生产语义：V4/V5 旧组合、ResponsesViaChat 分片/tools/usage/terminal、auth 3×2、native Responses usage。所有新增基线测试必须为绿。

### C1：core 类型、factory、identity

- 先以红测试证明 V1 response 必须 Messages→Chat、V2 必须 Chat→Messages。
- 引入 Protocol/CodecId、独立 `ports.rs`、CodecDirection Strategy、`directions/*` wiring、PreparedCodec/DecodedResponse，修正 V1/V2。
- 实现 3 identity 与 boundary 模块。
- 暂不切 production attempt；保留 ResponsesViaChat fallback，保证全量既有测试不回归。

### C2：Responses→Chat V1 + attempt/consumer 原子迁移

- 注册正式 ResponsesToChatV1。
- 在同一 Cell 内把三协议 attempt 切到 boundary+PreparedCodec，并删除 legacy else。
- 同一 Cell 内把普通/auth non-stream 和 stream 全部切到 PreparedCodec factory，并删除 SseMode、decoder_for、sse_mode_for 与字符串 decode；不得让 typed label 落入旧 consumer。
- 不允许提交“旧 fallback 已删、新方向未注册”的中间状态。
- 此时 9 格结构闭合，但 Responses↔Messages 仍暂为 legacy V1 composition，不得宣称全量直连。

### C3：Responses→Messages V2 直连

- request：Responses→Messages；
- non-stream response：Messages→Responses；
- SSE response：Messages→Responses；
- registry 切换到 V2 后删除该方向生产组合类型。

### C4：Messages→Responses V2 直连

- request：Messages→Responses；
- non-stream response：Responses→Messages；
- SSE response：Responses→Messages；
- registry 切换到 V2 后删除该方向生产组合类型。

### C5：legacy handler 与 auth transport 去重

rollout fallback registry 化并删除 handler 第二套 Responses event 生成职责；rollout on/off 验证同一 core codec；provider 防线测试通过后删除 executor 重复强制流。consumer factory 迁移已在 C2 完成，C5 不得再保留任何字符串选择过渡态。

### C6：清理与独立评审

完成 9 格 contract、semantic projection 往返、direct-vs-legacy oracle、SSE 全分片、auth 3×2、rollout on/off、全量 e2e；清死代码；独立 code review 后修复阻断/高风险问题并重跑所有门禁。

## 十一、测试比较规则

- 不比较随机 id/timestamp/default 的 raw JSON 字节相等。
- 用 SemanticProjection 比较 system/instructions、role/text/image、tool schema/call/result id/name/order/object arguments、reasoning、stop category、真实 usage。
- 旧组合是共享语义下限，新直连可以保留更多字段；每个有意差异必须写入 fixture expectation。
- 每条 stream fixture 遍历所有 byte split point，并覆盖 UTF-8 中点、CRLF/LF、多 record+partial carry、重复 finish/terminal。

## 十二、验收门禁

最终命令：

```bash
cargo test protocol::codec --manifest-path src-tauri/Cargo.toml
cargo test endpoint_executor --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

单跳：

```bash
rg -n 'encode_chat|decode_.*chat|ChatStream|responses_to_openai|openai_to_responses' \
  src-tauri/src/protocol/codec/directions/responses_to_messages.rs \
  src-tauri/src/protocol/codec/directions/messages_to_responses.rs
```

consumer 无字符串选择：

```bash
rg -n 'codec_version\.as_deref|Some\("[a-z_]+_v[0-9]+"\)|sse_mode_for|decoder_for|fn decode_non_stream' \
  src-tauri/src/endpoint_executor src-tauri/src/core/attempt.rs
```

consumer 只依赖 Facade/ports：

```bash
rg -n 'protocol::codec::(chat::|messages::|responses_codec::|directions::)' \
  src-tauri/src/core src-tauri/src/endpoint_executor \
  src-tauri/src/server src-tauri/src/auth_provider
```

必须无生产命中；协议 primitive 与具体 strategy 只能被 registry/directions 内部依赖。必须有 registry 结构测试证明 9 个 typed pair 无重复/遗漏，且每个 strategy 自报 pair 与注册 key 一致。

legacy helper 不越界：

```bash
rg -n 'responses_to_openai|openai_to_responses' \
  src-tauri/src/server src-tauri/src/core src-tauri/src/endpoint_executor
```

所有命中必须符合权威设计白名单；production consumer 命中即失败。

## 十三、范围

允许改动：权威设计 §12 明列的 codec/core/executor/handler/auth provider/模块入口、测试、fixture 和本提示文档。

禁止改动：DB/迁移、前端 UI、security/gate、channel_presets、ClaudeAdaptor、knowledge/wiki/stats/commands。

每 Cell 用 `git status --short` 与 `git diff --name-only` 复核；遇到用户已有改动（包括未跟踪文件）不得覆盖或清理。

## 十四、完成提交内容

向用户提供：

- 改动文件清单；
- 每 Cell 红→绿和门禁证据；
- 全量 test/fmt/clippy 汇总；
- grep 与范围门禁结果；
- 独立评审结论与已修复项；
- 遗留风险（如有）。
