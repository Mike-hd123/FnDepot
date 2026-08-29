# 协议转换核心模块重构：统一 9 项矩阵与单跳 codec

> 日期：2026-08-11
> 分支：`v0.1.8-protocol-conversion-refactor`
> 状态：复核完善稿（替代本文旧版）
> 实施入口：`docs/protocol-conversion-refactor/prompts/00-main.md`

## 1. 目标与结论

本次重构把 Chat Completions、Anthropic Messages、OpenAI Responses 三种协议的请求编码、非流响应解码和流式响应解码收敛到一个版本化核心。完成后：

1. 核心对外只暴露一个 `Protocol { Chat, Messages, Responses }`。
2. 三种协议形成完整 3×3 矩阵：3 个 identity + 6 个有向转换。
3. 每个转换方向都是源协议到目标协议的单跳实现；Responses↔Messages 不再借道 Chat。
4. attempt、executor、stream pump、legacy handler 和 auth 账号路径不再根据字符串重新推导转换行为。
5. 请求阶段不支持的语义在访问上游前失败；响应阶段无法解码的上游数据按 commit 状态归类，绝不伪造成成功。
6. auth provider 仍独立拥有鉴权、强制流式、字段策略和 401 刷新；业务协议解码与普通渠道共用核心。
7. 代码结构按 Facade + Strategy + typed Registry + Factory + Adapter + State 组织，使新增协议方向只需新增 strategy 并注册，不修改 consumer 分派。

本稿同时修正旧版中四个不可直接实施的假设：

- `PreparedConversion` 虽然创建了解码器，但目前在 `build_prepared_attempt` 返回前已被丢弃，执行层拿不到它。
- 当前 registry 的 Chat↔Messages 两项响应 decoder 绑定方向相反；不能直接把现有 boxed decoder 接到 executor。
- `UpstreamProtocol::OpenAI` 既可能指向 `chat_completions`，也可能指向 `responses`；边界映射必须同时看 upstream protocol 与 endpoint。
- auth allowlist 是可失败的 provider preflight，不是无错误的 `Fn(Value) -> Value`，不应下沉为 codec hook。

## 2. 权威约束与冲突处理

实现必须同时满足以下文档；冲突按顺序处理：

1. 本文：本次重构的范围、类型、矩阵、迁移顺序和验收。
2. `docs/channel-refactor-tasks/00-architecture-decisions.md`：fail-closed、错误分类、commit barrier、exactly-once。
3. `docs/channel-refactor-tasks/04-codec-chat-messages.md`：codec 的请求/响应/SSE 契约。
4. `docs/auth-codex/ADRs.md`：ADR-31、33、35、36、37。
5. `docs/auth-codex/02-routing-compat-review.md`：账号路由与执行边界。

`00-main.md` 已随本稿同步为同一套 API、方向与 P0+C1-C6 顺序。后续修改任一文档时必须同步另一份；发生遗漏时，以本文 §5、§6、§11、§13 为准，禁止注册 stub、把 response 方向写成 request 同向、保留生产 v1 组合或下沉 provider policy。

本文对两个历史表述作明确收口：

- “fail-open”仅表示**有明确目标语义的映射**或**不改变模型输出/工具/副作用语义的可观察归一化**；不等于静默丢弃任意字段。
- “往返幂等”指语义投影经规范化后幂等，不要求随机 ID、时间戳、默认字段和 wire JSON 字节完全相同。

### 2.1 ADR 补充：对 T00/T04 首版契约的演进

T00 要求实现发现必须改变冻结决策时形成 ADR 补充。本节即为本次设计的 ADR delta；本稿获批即表示以下演进获批，未获批前不得开始生产实现。

| 旧契约 | 本次决策 | 理由与影响 |
|---|---|---|
| T00 决策 8：Responses→Anthropic 不进入当期 | 纳入本次 3×3 矩阵，并重写为 V2 直连 | 当前仓库已经有 `responses_to_messages_v1` 生产路径，本次不是首次开放路由，而是消除既有组合债务 |
| T00/T04 首版拒绝 reasoning/thinking | 沿用当前代码已落地的可表达 reasoning↔thinking 映射；不可读/不可表达形态仍拒绝或按响应错误处理 | 后续行为已超越 T04 首版，auth 与现有 fixtures 依赖该能力；必须逐形态测试，不能泛化为“所有 reasoning 都支持” |
| T04：`prepare(..., version, ...)` | 保留为 deprecated Adapter 入口；新代码使用 `prepare_pair(Protocol, Protocol, ...)`，输出携带方向化 `CodecId` | Rust 不支持同名重载；兼容入口防止既有调用立即失效，未来多版本并存时再加 `prepare_exact(CodecId, ...)` |
| T04：`PreparedConversion { encoded_request, context, report }`，现代码又直接装 boxed decoder | 改为 `PreparedConversion { encoded_request, report, codec: PreparedCodec }`，codec 保存 context + decoder factory | 既保持 request/response 同一上下文，又让 attempt 可 Clone/Serialize；有状态 stream decoder 每次独立创建 |
| T04：`NonStreamDecoder::decode(status, headers, body)` | business decoder 接受 body，返回 body+usage；status/header 继续由 executor transport 层处理 | 当前实际实现已经只接受 body；非 2xx 分类和安全 header 转发不应进入业务协议机 |
| 旧设计：auth core `post_process` | 不新增 hook；provider 保留可失败 preflight | ADR-36/37 是 provider transport/policy，不属于三协议语义转换 |

此 ADR delta 不改变 T00 的错误分类、首帧 commit barrier、客户端取消、post-commit 不重试和 exactly-once 终止要求。

## 3. 现状基线（以当前代码为准）

### 3.1 转换选择散落

| 位置 | 当前行为 | 必须消除的问题 |
|---|---|---|
| `protocol/codec/registry.rs` | `Downstream`/`Upstream` + 5 个静态 `Direction` | 缺 identity 和 Responses→Chat；版本参数未参与查表 |
| `core/attempt.rs` | Native/Conversion 双分支、`codec_direction()`、legacy else | 映射重复；任意未映射方向可能误入 legacy Responses→Chat |
| `endpoint_executor/driver.rs` | `codec_version` 字符串 → `SseMode` | 编译期无法验证；auth 有额外特判 |
| `endpoint_executor/sse.rs` | `SseMode` → 新建 decoder | 丢失 prepare 时的 context/request id；重复选择逻辑 |
| `endpoint_executor/mod.rs` | 字符串 match + 内联非流组合解码 | 与 registry 重复且 V4/V5 再走一遍 Chat |
| `server/handlers.rs` | rollout fallback 直接调用 legacy helper | 绕过 registry，流式另有一套 Chat→Responses pipeline |
| `auth_provider/codex_backend.rs` | provider preflight/allowlist/强制 stream/store | 这是传输策略，必须保留在 provider，不与 codec 混合 |

### 3.2 当前 `PreparedConversion` 的真实生命周期

新入口 `CodecRegistry::prepare_pair` 当前返回：

```rust
PreparedConversion {
    encoded_request,
    context,
    report,
    non_stream: Box<dyn NonStreamDecoder>,
    streaming: Box<dyn StreamDecoder>,
}
```

但 `build_prepared_attempt` 只保存 `encoded_request`、JSON 化的 report 和字符串 label；`context` 与两个 decoder 随即 drop。随后：

- non-stream executor 重新创建 `ConversionContext` 并手写 decode；
- stream driver 从字符串选择 `SseMode`；
- `decoder_for()` 再创建一个 request id 为空的新 decoder。

所以本次不是“把现成字段接过去”的机械改造，而是一次明确的所有权/API 修正。

### 3.3 当前 registry 的响应方向缺陷

方向名称表示**请求方向**，响应必须反向解码：

```text
请求：downstream Chat → upstream Messages
响应：upstream Messages → downstream Chat
```

当前 V1 却注册了 Chat response → Messages decoder；V2 反之。执行器之所以未暴露该缺陷，是因为它绕过 registry 自己选择了正确 decoder。C1 必须先用失败测试锁定并修正这一点，否则 C5 直接消费 registry 会产生协议反转。

### 3.4 当前版本类型不是运行时真相

现有三份表示并不一致：

- `registry::Version(String)` 永远为 `"v1"`，`prepare` 参数名为 `_version`，未参与选择。
- `report::CodecVersion { major, minor }` 对所有方向固定为 1.0，不能表达方向或 native。
- executor/log 实际依赖 `Option<String>`，如 `chat_to_messages_v1`。

本次必须收敛为一个带方向的 `CodecId`，而不是把现有数值 `CodecVersion` 原样搬进 attempt。

## 4. 不变量与范围

### 4.1 必须始终成立

1. **单一入口**：三协议的请求准备都调用 `CodecRegistry::prepare_pair`。
2. **单跳**：一个有向 codec 不得调用第三种协议的 encoder、non-stream decoder 或 stream decoder。
3. **fail-closed**：无矩阵项、边界映射失败、请求语义不可表达均在上游访问前返回 `CallerTerminal`。
4. **响应诚实**：上游 malformed/unknown response 不能变成正常 stop/end_turn/completed。
5. **首帧门禁**：转换后的首个完整下游 record 成功生成后才能 commit。
6. **终止恰好一次**：Chat `[DONE]`、Messages `message_stop`、Responses `response.completed`（以及协议要求的尾随 `[DONE]`）各自最多一次。
7. **上下文一致**：请求编码、响应解码、日志中的 mapped model 和 codec id 来自同一个 prepared attempt。
8. **每次流独占状态**：重试或 clone attempt 时必须创建新的 stream decoder，禁止共享已消费状态。
9. **无静默协议损失**：保留、归一化或拒绝必须有明确策略；归一化记录进 `ConversionReport`。

### 4.2 允许复用与禁止复用

允许复用协议无关的底层能力：

- SSE record byte framing、CRLF/LF 处理；
- JSON pointer 错误构造；
- tool arguments 的 JSON object 校验；
- usage、tool index/id 和 stop reason 的小型值对象；
- Responses/Chat/Messages 各自协议内的 parser。

禁止在 Responses↔Messages 直连实现中调用：

- Chat request encoder；
- Chat non-stream response converter；
- Chat stream state machine；
- `protocol::responses_to_openai` 或 `protocol::openai_to_responses`。

### 4.3 本次不改

DB schema/迁移、前端 UI、`security/gate`、`channel_presets`、`ClaudeAdaptor`、knowledge/wiki/stats/commands 均不在范围内。`route_plan` 仅为边界映射测试提供现有枚举，不改变分组和优先级语义。

### 4.4 设计模式驱动的结构

本次按“职责、变化轴和依赖方向”使用设计模式，不以堆叠 GoF 名称为目标。Rust 实现优先使用 trait、enum、不可变值对象和显式所有权，不模拟面向对象继承树。

| 模式/思想 | 在本重构中的角色 | Rust 落点 | 约束 |
|---|---|---|---|
| **Facade** | 为所有消费方提供唯一协议入口 | `CodecRegistry::prepare_pair` + `PreparedCodec` | core/executor/handler/auth 不得直接调用具体方向模块 |
| **Strategy** | 每个有向矩阵项是一种可替换转换策略 | `CodecDirection` trait；6 个 conversion strategy + identity strategy | 一个 strategy 同时拥有 request encoder 与反向 response decoder factory，避免再次绑反 |
| **Registry** | 以 typed pair 选择当前策略 | `(Protocol, Protocol) -> &'static dyn CodecDirection` | 选择只发生一次；禁止字符串 match、consumer 二次推导 |
| **Abstract Factory / Factory Method** | 为一次 attempt 创建互不共享的 non-stream/stream decoder | `PreparedCodec::new_*_decoder()` | factory 捕获同一 `ConversionContext`；每次 stream/retry 创建新状态 |
| **Adapter / Anti-Corruption Layer** | 隔离 route、legacy、auth transport 与 codec domain | `core/protocol_boundary.rs`、legacy handler seam、auth provider | 外部 enum/header/provider policy 不泄漏到方向策略 |
| **State** | 表达 SSE 合法事件序列与 exactly-once 终止 | 每方向独立 state object；复杂 phase 使用 enum + transition 方法 | 状态转换集中；协议事件判断不得泄漏到 pump/handler。不为套模式强制重写已清晰的成熟 state struct |
| **Identity / Null Object** | 同协议也走统一策略，消除 Native 特判 | 同一 `IdentityDirection` 类型的 Chat/Messages/Responses 三个 static 实例 | 除 model 替换外原样；每个实例自报自己的 pair，仍执行 response/SSE 验证与 usage 提取 |
| **Value Object** | 消除原始字符串和无约束 tuple | `Protocol`、`CodecId`、`Usage`、tool/item index、stop category | 构造时校验，日志 label 仅由 typed value 派生 |
| **Ports and Adapters / Dependency Inversion** | transport 依赖 codec port，不依赖具体转换实现 | `ports.rs` 中的 `NonStreamDecoder`、`StreamDecoder` traits | 依赖方向固定为 consumer→port←strategy；provider policy 留在外层 adapter |

推荐目标结构：

```text
protocol/codec/
├── mod.rs                     # 只 re-export Facade、ports、value objects
├── types.rs                   # Protocol / CodecId / PreparedCodec / DecodedResponse
├── ports.rs                   # NonStreamDecoder / StreamDecoder / usage-facing ports
├── direction.rs               # CodecDirection Strategy port
├── registry.rs                # typed matrix + Facade
├── identity.rs                # Identity strategy
├── directions/
│   ├── mod.rs
│   ├── chat_to_messages.rs    # 可先包装现有严格 primitives
│   ├── messages_to_chat.rs
│   ├── chat_to_responses.rs
│   ├── responses_to_chat.rs
│   ├── responses_to_messages.rs  # V2 直连，禁止依赖 Chat
│   └── messages_to_responses.rs  # V2 直连，禁止依赖 Chat
├── chat.rs / messages.rs / responses_codec.rs  # 迁移期协议 primitives
├── request.rs / report.rs / error.rs
└── sse.rs                     # 仅协议无关 framing helper
```

现有 `chat.rs/messages.rs/responses_codec.rs` 可以在本次作为底层协议 primitive 保留，避免为了目录美观或“State 模式纯度”做大搬迁；其中已有且职责清晰的 state struct/布尔字段无需机械改写。strategy wiring 必须进入 `directions/*`，consumer 不能再依赖这些 primitive。新 V2 方向直接在对应 strategy 内实现，不新增“万能中间模型”或巨大 `match (source, target)`。

明确禁止以下伪模式化实现：

- 用 service locator、全局可变 registry 或运行时字符串反射替代 typed registry；
- 为了复用把所有协议塞进一个大 IR/万能 message enum；
- 用 `Arc<Mutex<dyn StreamDecoder>>` 共享有状态 strategy；
- 在 Facade 之外新增第二个 registry、第二套 factory 或 consumer special case；
- 把 auth allowlist/强制流包装成 codec decorator，造成业务协议与 provider policy 混层。

## 5. 目标类型与公共 API

### 5.1 单一协议类型

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Chat,
    Messages,
    Responses,
}
```

删除 codec 内的 `Downstream`/`Upstream`；方向由两个 `Protocol` 字段表达，避免同一概念的镜像枚举。

### 5.2 稳定的 codec 标识

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum CodecId {
    Native,
    ChatToMessagesV1,
    MessagesToChatV1,
    ChatToResponsesV1,
    ResponsesToChatV1,
    // 仅供 P0 oracle / 迁移期识别；C4 后不得由生产 registry 选择。
    MessagesToResponsesV1,
    ResponsesToMessagesV1,
    MessagesToResponsesV2,
    ResponsesToMessagesV2,
}
```

稳定日志 label：

| `CodecId` | label |
|---|---|
| `Native` | `native` |
| `ChatToMessagesV1` | `chat_to_messages_v1` |
| `MessagesToChatV1` | `messages_to_chat_v1` |
| `ChatToResponsesV1` | `chat_to_responses_v1` |
| `ResponsesToChatV1` | `responses_to_chat_v1` |
| `MessagesToResponsesV1` | `messages_to_responses_v1`（legacy composition） |
| `ResponsesToMessagesV1` | `responses_to_messages_v1`（legacy composition） |
| `MessagesToResponsesV2` | `messages_to_responses_v2` |
| `ResponsesToMessagesV2` | `responses_to_messages_v2` |

Responses↔Messages 的直连重写是可观察行为变化，必须从旧组合实现的 `*_v1` 升为 `*_v2`。文档中的“V1–V6”只能表示开发顺序，不作为版本号；新 Responses→Chat 的正式 label 是 `responses_to_chat_v1`。

两个 legacy V1 id 只用于迁移期日志/fixture 识别；C4 后生产 registry 不得选择它们。最终是否从 enum 删除由兼容日志反序列化需求决定，但保留 id 不代表保留可执行的组合 codec。

删除未生效的 `registry::Version`。`ConversionReport` 改为记录 `codec_id: CodecId`，不再使用对所有方向都相同的数值 `CodecVersion`。如果未来同一方向并存多版本，再新增显式 `prepare_exact(CodecId, ...)`；本次 `prepare` 只选择矩阵当前版本。

### 5.3 可克隆的 decoder plan，而不是可克隆的 decoder 状态

```rust
#[derive(Clone)]
pub struct PreparedCodec {
    id: CodecId,
    downstream: Protocol,
    upstream: Protocol,
    context: ConversionContext,
    strategy: &'static dyn CodecDirection,
}

impl PreparedCodec {
    pub fn id(&self) -> CodecId;
    pub fn label(&self) -> &'static str;
    pub fn is_identity(&self) -> bool;
    pub fn new_non_stream_decoder(&self) -> Box<dyn NonStreamDecoder + Send + Sync>;
    pub fn new_stream_decoder(&self) -> Box<dyn StreamDecoder + Send + Sync>;
}

#[derive(Clone)]
pub struct PreparedConversion {
    pub encoded_request: Value,
    pub report: ConversionReport,
    pub codec: PreparedCodec,
}
```

每个方向实现同一个 Strategy port；方法名必须显式表示 response 的反向语义：

```rust
trait CodecDirection: Send + Sync {
    fn id(&self) -> CodecId;
    fn downstream(&self) -> Protocol;
    fn upstream(&self) -> Protocol;
    fn encode_request(
        &self,
        request: &Value,
        mapped_model: &str,
    ) -> Result<(Value, ConversionContext), PrepareError>;
    fn new_response_decoder(
        &self,
        context: &ConversionContext,
    ) -> Box<dyn NonStreamDecoder + Send + Sync>;
    fn new_stream_response_decoder(
        &self,
        context: &ConversionContext,
    ) -> Box<dyn StreamDecoder + Send + Sync>;
}
```

这样 `PreparedAttempt` 仍可 clone；每个消费方从不可变 plan 创建自己的 decoder。尤其 stream decoder 是新实例，不会因 async closure clone 或重试共享状态。禁止把 `Box<dyn StreamDecoder>` 包进 `Arc<Mutex<_>>` 来伪造 Clone。

`PreparedCodec` 的 `Debug`/`Serialize` 只暴露 protocol pair、context 的非敏感字段和 codec label，不序列化函数指针。`PreparedAttempt` 删除运行时 `Option<String>` 真相；对 Chat/Messages/Responses，日志字符串一律由 `attempt.protocol_codec.label()` 派生。

`CountTokens`/`Embeddings` 不属于三协议矩阵，但现有 native route 仍会构造 `PreparedAttempt`。为避免本次重构误伤它们，attempt 使用显式 `protocol_codec: Option<PreparedCodec>`：

- 三协议 endpoint 必须为 `Some`，包括 identity；
- CountTokens/Embeddings 的既有 native 路径为 `None`，维持原 response passthrough；
- 三协议 endpoint 出现 `None` 是编程/配置错误，必须 fail-closed。

这里的 `Option` 只表达“是否属于本次 codec 领域”，不再承担 native/conversion 选择，也不允许 executor 由它推导字符串模式。

### 5.4 请求与响应错误分型

当前 response decoder 也返回 `UnsupportedFeatures`，容易把上游协议错误误归为客户端请求错误。本次拆分：

```rust
CodecRegistry::prepare_pair(...) -> Result<PreparedConversion, PrepareError>
NonStreamDecoder::decode(...) -> Result<DecodedResponse, DecodeError>
StreamDecoder::feed/finish(...) -> Result<Vec<String>, DecodeError>

pub struct DecodedResponse {
    pub body: Value,
    pub usage: Option<Usage>,
}
```

non-stream usage 必须由 decoder 在解析 raw upstream body 的同一次过程中返回；executor 不得为了计费再按协议字符串解析第二遍。stream usage 继续由有状态 `StreamDecoder::usage()` 暴露。identity decoder 使用协议公共 usage parser，并同样返回/暴露真实值与 `usage_unknown`。

映射固定为：

| 阶段 | 错误 | AttemptFlow 分类 |
|---|---|---|
| boundary/prepare/provider preflight，尚未访问上游 | `PrepareError` / provider unsupported | `CallerTerminal`，400 |
| non-stream 上游响应解码失败 | `DecodeError` | `UpstreamProtocolError`，502，可按预算换候选 |
| stream 首帧/commit 前解码失败 | `DecodeError` | `UpstreamProtocolError`，502，可换候选 |
| stream commit 后解码失败 | `DecodeError` | 目标协议 error event + `CommittedStreamError`，不可重试 |

非 2xx status、安全 header 过滤、transport error 继续由 executor 处理，不塞入 business codec。

## 6. 边界适配层

边界映射放在 `core/protocol_boundary.rs`，依赖方向为 `core -> protocol::codec`；禁止让 `protocol/codec` 反向依赖 `core::route_plan`。

```rust
pub fn downstream_protocol(endpoint: EndpointKind) -> Option<Protocol>;
pub fn upstream_protocol(
    protocol: UpstreamProtocol,
    endpoint: &str,
) -> Option<Protocol>;
```

映射表：

| route 输入 | codec 输出 |
|---|---|
| `EndpointKind::ChatCompletions` | `Some(Chat)` |
| `EndpointKind::Messages` | `Some(Messages)` |
| `EndpointKind::Responses` | `Some(Responses)` |
| `CountTokens` / `Embeddings` | `None` |
| `(OpenAI, "chat_completions")` | `Some(Chat)` |
| `(OpenAI, "responses")` | `Some(Responses)` |
| `(Anthropic, "messages")` | `Some(Messages)` |
| `(Responses, "responses")` | `Some(Responses)` |
| Ollama、count_tokens、embeddings、未知或不一致 pair | `None` |

不能只写 `UpstreamProtocol -> Protocol`：`OpenAI` 本身有歧义。对 Chat/Messages/Responses 下游，upstream 映射失败必须返回 `CallerTerminal` 配置/规划错误，绝不能再进入 `responses_via_chat` else。对 CountTokens/Embeddings，下游映射为 `None` 表示明确绕过本三协议核心并保留既有 native executor，不是错误。

## 7. 9 项矩阵与注册内容

矩阵中的箭头表示请求方向；同一项携带反向 response decoder。

| 下游请求 \ 上游请求 | Chat | Messages | Responses |
|---|---|---|---|
| **Chat** | `Native` | `ChatToMessagesV1` | `ChatToResponsesV1` |
| **Messages** | `MessagesToChatV1` | `Native` | `MessagesToResponsesV2` |
| **Responses** | `ResponsesToChatV1` | `ResponsesToMessagesV2` | `Native` |

每个 matrix test 必须同时验证：

1. request encoder 接受的是行协议，输出的是列协议；
2. non-stream decoder 接受列协议响应，输出行协议响应；
3. stream decoder 接受列协议 SSE，输出行协议 SSE；
4. report 与日志 label 对应该方向；
5. identity 只改 model，不改其它 JSON；identity response body 原样返回。

### 7.1 identity

identity request：克隆已审计 body，仅替换顶层 `model`。没有 object 顶层或无法写 model 时 fail-closed。

实现为同一 `IdentityDirection` 类型的三个 `static` 实例，分别固定 `(Chat,Chat)`、`(Messages,Messages)`、`(Responses,Responses)`；registry 不得复用一个无法自报具体 pair 的无类型 singleton。

identity non-stream：body 原样 clone，usage 由同一 decoder/公共 usage parser 暴露。

identity stream：按完整 SSE record 缓冲并原样输出，拒绝非法 UTF-8/JSON、半条 EOF 和重复终止。终止要求：

- Chat：至多一个 `[DONE]`；
- Messages：至多一个 `message_stop`；
- Responses：至多一个 `response.completed`，允许至多一个随后 `[DONE]`；
- `codex.rate_limits` 是带外事件，原样输出，不初始化或终止业务状态。

若 `codex.rate_limits` 出现在首个业务事件之前，pump 暂存该带外 record；它**不满足 commit barrier**。只有同一 decoder 成功生成首个业务下游事件后，才按原顺序一次性释放暂存带外 record + 业务事件并 commit。暂存必须复用现有 first-frame deadline，并增加明确的 byte/record 上限；超过任一上限按 pre-commit upstream protocol error 处理，禁止无限缓存。若流在此之前 malformed 或 EOF，仍可 failover，客户端不应只因 quota 帧而被提前 commit。

identity 也必须经过 first-record commit barrier。`StreamPumpCore` 最终总是持有 decoder，不再以 `None` 代表 native。

## 8. 字段语义政策

所有 request 顶层字段和内容块必须落入以下三类之一：

| 分类 | 条件 | 行为 |
|---|---|---|
| Preserved | 目标有等价语义 | 映射并测试 |
| Normalized | 表示不同但语义等价，或明确的非模型语义 provider hint | 映射/移除并在 report 记录 JSON pointer |
| Rejected | 会改变输出、工具调用、异步生命周期、远端副作用，或无法证明等价 | prepare 前返回稳定 code + pointer |

禁止用 `normalized` 掩盖以下损失：非法 tool arguments、缺 tool id/name、未知 role/block/event、无法表达的 structured output、built-in tool、document/PDF、异步/background 生命周期或 `store:true` 远端副作用。

`parallel_tool_calls` 应在目标支持时映射；例如 Messages 的 `tool_choice.disable_parallel_tool_use` 与 `false` 对应。只有目标默认值与源值可证明等价时才可归一化省略。

`prompt_cache_key`、`client_metadata`、`include` 等字段必须各自有 fixture 和显式政策，不能使用笼统 `DROPPED` 数组。若只影响 provider 缓存/观测且本期决定忽略，必须记录 pointer；若影响响应内容或副作用则拒绝。

## 9. Responses↔Messages 直连协议机

### 9.1 文件与依赖边界

新增：

- `protocol/codec/directions/responses_to_messages.rs`
- `protocol/codec/directions/messages_to_responses.rs`

二者只依赖各自源/目标协议 parser 与协议无关 helpers。完成后删除以下组合类型：

- `ResponsesMessagesStreamDecoder`
- `ResponsesMessagesNonStreamDecoder`
- `MessagesResponsesStreamDecoder`
- `MessagesResponsesNonStreamDecoder`

并删除直连文件对 Chat encoder/decoder 与 legacy Responses↔Chat helper 的调用。

### 9.2 Responses request → Messages request

最低映射表：

| Responses | Messages | 规则 |
|---|---|---|
| `model` | `model` | 使用 mapped model，不在 codec 内再次映射 |
| `instructions` | `system` | 保序；string/受支持 block 逐项转换 |
| `input` message + `input_text` | `messages[].content[].text` | 保留 role 与顺序 |
| `input_image` | Messages image block | 校验 URL/data URI/media type/role/size |
| `function_call` | assistant `tool_use` | 保留 call_id/name/order，arguments 必须是 object JSON |
| `function_call_output` | user `tool_result` | 保留 call_id 与内容顺序 |
| function `tools` | `tools[].input_schema` | built-in tools 无等价语义则拒绝 |
| `tool_choice` | Messages `tool_choice` | auto/required/none/specific function 明确映射 |
| `parallel_tool_calls:false` | `disable_parallel_tool_use:true` | 其它值按目标能力映射/归一化 |
| `reasoning.effort` | `thinking`/`output_config` | 沿用已批准 effort mapping，记录非 1:1 归一化 |
| `max_output_tokens` | `max_tokens` | 缺失时沿用兼容默认 32000，并记录 synthetic default |
| `stream` | `stream` | 原值保持 |

### 9.3 Messages request → Responses request

| Messages | Responses | 规则 |
|---|---|---|
| `model` | `model` | 使用 mapped model |
| `system` | `instructions` | 保序；受支持的 text block 合并/逐项表达 |
| user/assistant text | `input` message + `input_text` | 保留 role 与内容顺序 |
| image block | `input_image` | 校验 source/media type/role/size |
| assistant `tool_use` | `function_call` | id→call_id，name/order 保留，input 序列化为 object arguments |
| user `tool_result` | `function_call_output` | tool_use_id→call_id，内容保持顺序 |
| `tools[].input_schema` | function `tools` | name/description/schema 保留 |
| Messages `tool_choice` | Responses `tool_choice` | auto/any/tool/none 与 parallel policy 显式映射 |
| `disable_parallel_tool_use:true` | `parallel_tool_calls:false` | false/缺失按源默认语义处理 |
| `thinking`/`output_config` | `reasoning.effort` | 沿用已批准 budget↔effort 映射并报告归一化 |
| `max_tokens` | `max_output_tokens` | provider 若后续剥离属于 auth policy，不由 codec 静默删除 |
| `stream` | `stream` | 原值保持；auth 强制 true 发生在 provider 层 |

Messages 独有的 `metadata`、`container`、`context_management*`、cache annotations 必须逐字段决定 normalized/rejected；不得先转换成 Chat 再依赖 Chat allowlist 偶然处理。

### 9.4 Messages response → Responses response

- text block → Responses assistant message/output_text；
- thinking block → Responses reasoning item/summary；不可读的 redacted/encrypted 数据不得伪造成可读 reasoning；
- tool_use → function_call，保留 id/name/order，input object 序列化为 arguments；
- usage 的 input/output/cache tokens 只统计一次；
- `end_turn`/`stop_sequence`/`max_tokens`/`tool_use` 必须走显式 stop/status 表，未知值报 `DecodeError`；
- 不生成随机空 tool name、空 call id 或 `{}` arguments。

### 9.5 Responses response → Messages response

- Responses output message 的 output_text → Messages text block；
- reasoning item/summary → thinking block；
- function_call → tool_use，保持 call_id/name/order；
- usage 映射到 Messages usage，缺失时标记 unknown，不把估算值伪装成上游真实 usage；
- completed/incomplete/failed 与 incomplete reason 显式映射到 Messages stop/error；未知状态报错。

### 9.6 Responses SSE → Messages SSE

每请求独立状态至少包含：response id/model、是否已发 `message_start`、source item index 到 Messages content index 的映射、当前 text/reasoning/tool block、tool arguments accumulator、usage、stop 状态、terminal 状态。

| Responses 事件族 | Messages 输出 |
|---|---|
| `response.created` / `response.in_progress` | 恰好一个 `message_start` |
| message/reasoning/function item added | 对应 `content_block_start` |
| `response.output_text.delta` | `text_delta` |
| reasoning delta/summary delta | `thinking_delta` |
| function arguments delta | `input_json_delta` |
| content/item done | 对应 `content_block_stop`，不得重复 |
| `response.completed` | `message_delta`(stop+usage) + `message_stop` 恰好一次 |
| `response.failed` / malformed / unknown | target error；commit 前可 failover，commit 后终止 |
| `codex.rate_limits` | 原 record 原样透传，不使用字符串 contains 猜测 |
| `[DONE]` | 只验证源终止完整性，不额外生成第二个 `message_stop` |

### 9.7 Messages SSE → Responses SSE

状态至少包含：response id/model、created 标志、content index 到 Responses item id/index 的映射、text/reasoning/tool accumulator、stop reason、usage、terminal 状态。

| Messages 事件族 | Responses 输出 |
|---|---|
| `message_start` | `response.created` + `response.in_progress` 恰好一次 |
| text/thinking/tool_use block start | 对应 output item/content part added |
| `text_delta` | `response.output_text.delta` |
| `thinking_delta` | reasoning summary delta |
| `input_json_delta` | function_call_arguments.delta |
| content block stop | 对应 part/item done，arguments 必须形成合法 object |
| `message_delta` | 累积 stop reason/usage，不提前 completed |
| `message_stop` | `response.completed` + `[DONE]` 各一次 |
| malformed / unknown | target error；按 commit 状态分类 |

任意 TCP 分片（包括 UTF-8 codepoint、SSE field、JSON token、多个 record 同 chunk）必须得到相同事件序列。

## 10. Responses↔Chat 注册与 legacy 收敛

`ResponsesToChatV1` 是直接方向：请求用 Responses→Chat encoder；响应用 Chat→Responses non-stream/stream decoder。允许先把现有 helper 封装进该方向，但 wrapper 必须补齐严格验证、context、report 和 decoder factory。

迁移要求：

1. `attempt.rs` 删除 `responses_via_chat_v1` else 分支；Responses→Chat 正常命中矩阵。
2. rollout fallback 仍可保留旧 handler 的渠道选择/日志框架，但请求准备和响应解码必须来自 `PreparedCodec`。
3. legacy 非流：用 `prepared.encoded_request` 发上游，成功 body 用 `prepared.codec.new_non_stream_decoder().decode(...)`。
4. legacy 流：每次选定 mapped model 后 prepare；`UpstreamSseBridge` 若仍为 ClaudeAdaptor 兼容所需，只负责把 transport/adaptor 输出整理为 Chat SSE；Chat→Responses 业务事件转换交给 prepared stream decoder。
5. 删除 handler 内 `process_openai_record_for_responses` 的协议转换职责，避免第二套 response.created/completed 生成器。
6. `protocol/mod.rs` helper 可暂时保留并 `#[deprecated]`，但生产调用只允许位于对应 codec 内部。

## 11. 消费方目标调用链

### 11.1 attempt

```text
EndpointKind + (UpstreamProtocol, upstream_endpoint)
  → protocol_boundary
  → CodecRegistry::prepare_pair(downstream, upstream, mapped_model, audited.forward_json)
  → PreparedAttempt { encoded_body, conversion_report, protocol_codec: Some(PreparedCodec), ... }
```

三协议的 Native/Conversion 均走同一段；`GroupTier` 只用于路由排序，不决定是否调用 codec。若 route tier 与实际 protocol pair 不一致，记录配置错误并 fail-closed。CountTokens/Embeddings 明确走既有非 codec native 分支，`protocol_codec=None`。

### 11.2 普通 non-stream

```text
send encoded_body
  → status/header/JSON transport handling
  → attempt.protocol_codec.new_non_stream_decoder().decode(raw_body)
  → downstream body
```

删除 executor 的 `decode_non_stream` 字符串 match。usage 优先来自 decoder 观察到的真实 upstream usage；transport 公共 parser 仅做 identity/兼容补充，不能重复计数。

### 11.3 普通 stream

```text
connect raw upstream stream
  → buffer first complete record
  → attempt.protocol_codec.new_stream_decoder()
  → feed first record + carry
  → 成功生成合法下游 bytes 后 commit
  → 同一 decoder 继续 feed/finish
```

`StreamPumpCore` 删除 `SseMode` 与 `Option<decoder>`；进入该 pump 的三协议 stream 必有 decoder。它只负责 commit supervisor、decoder 驱动和 post-commit error 格式化。content-type 是否保留由 `codec.is_identity()` 判断，不再由字符串 label 判断。

### 11.4 auth

auth 与普通渠道共享 `PreparedCodec` 的 decoder，但保留不同 transport：

- provider `validate_backend_request`：最终 allowlist、`stream:true`、`store:false`，可失败；
- `AuthService::outbound`：token/header、quota、401 内部刷新并仅重试一次；
- 下游非流：Responses SSE 先由 `ResponsesEventAccumulator` 聚合为完整 Responses body，再交 prepared non-stream decoder；
- 下游流：raw Responses SSE 直接交 prepared stream decoder；
- Responses 下游/Responses 账号使用 identity；Chat、Messages 下游使用对应 Responses conversion。

删除 executor 的重复 `force_responses_stream`，但只能在 provider 层相关测试证明最终 body 始终 `stream:true`、`store:false` 后删除。不得把 provider allowlist 移入 codec，也不新增 core `post_process` hook。

## 12. 文件级改造

| 文件 | 改动 |
|---|---|
| `protocol/codec/**` | 本次所有 business codec 生产改动的允许根；未在下列行单列的 `chat.rs`、`messages.rs`、`request.rs`、`sse.rs` 只允许为 trait/error/helper 迁移作必要修改，不得顺带扩功能 |
| `protocol/codec/types.rs`、`ports.rs` | value objects/prepared plan/decoded response；non-stream/stream decoder ports。ports 不再定义在 registry 中 |
| `protocol/codec/mod.rs`、`registry.rs` | 注册/re-export 新模块；`Protocol`、`CodecId`、9 格注册、cloneable `PreparedCodec`、factory API、修正 V1/V2 response wiring |
| `protocol/codec/report.rs` | report 记录 `CodecId`；清理重复数值版本概念 |
| `protocol/codec/error.rs` | 分离 prepare 与 decode error；稳定 code/pointer |
| `protocol/codec/identity.rs` | identity request/non-stream/SSE、usage、terminal 验证 |
| `protocol/codec/direction.rs`、`directions/mod.rs` | Strategy port 与 6 个方向的模块注册；每个方向将 encoder 和反向 response factories 成对封装 |
| `protocol/codec/directions/responses_to_messages.rs` | 新直连 strategy：request encoder + Messages→Responses non-stream/SSE response decoder |
| `protocol/codec/directions/messages_to_responses.rs` | 新直连 strategy：request encoder + Responses→Messages non-stream/SSE response decoder |
| `protocol/codec/responses_codec.rs` | 保留/拆出 Chat↔Responses 直连；删除 V4/V5 组合类型 |
| `core/mod.rs`、`core/protocol_boundary.rs` | 注册边界模块；route types 到 `Protocol` 的唯一映射 |
| `core/attempt.rs` | 单一 prepare 路径；attempt 持有 `PreparedCodec`；删除 `codec_direction`/legacy else |
| `core/plan_executor.rs` | 日志 label 从 typed codec 派生，不保留第二份 runtime string truth |
| `endpoint_executor/mod.rs` | non-stream 直接消费 prepared decoder；auth accumulator 后共用；删字符串分派 |
| `endpoint_executor/driver.rs` | stream decoder factory 直入 pump；删 `sse_mode_for` |
| `endpoint_executor/sse.rs` | pump 只驱动 decoder；identity 替代 Native mode；保留 commit barrier |
| `server/handlers.rs` | rollout fallback 改走 registry；删除内联 Responses 转换职责 |
| `auth_provider/codex_backend.rs` | 仅在去重 executor 强制流时调整测试/注释；provider policy 仍在此处 |
| `protocol/mod.rs` | 被核心取代的 helper 标 deprecated；不扩大其它 legacy 重构 |
| `protocol/codec/**/*test*`、同模块 `#[cfg(test)]` | 单元、fixture、semantic projection、任意 split 测试 |
| `endpoint_executor/{integration_tests,mock_tests}.rs` | consumer、commit barrier、普通/auth 共用 decoder 路径 |
| `auth_integration_tests.rs`、`rollout_integration_tests.rs` | auth 三下游与 rollout on/off e2e |
| `docs/protocol-conversion-refactor/prompts/00-main.md` | 实施前同步本文 API、Cell 顺序和门禁，消除双计划 |

## 13. 实施顺序（TDD Cell）

P0 是只读/characterization 基线阶段，新增测试应直接为绿，不适用红测试要求。C1-C6 每个实现 Cell 都执行红→绿→重构；未通过本 Cell 门禁不得进入下一 Cell。

### P0：基线与语义 oracle 冻结

先不改生产代码，冻结：

- 当前 V4/V5 组合 request/non-stream/SSE fixtures；
- ResponsesViaChat 的 UTF-8 分片、tool id/name/arguments、usage、终止序列；
- auth 三下游 × stream/non-stream 的现有输出与 provider body；
- native Responses usage 从 `/response/usage` 提取。

验收：characterization tests 在未改生产代码时全部为绿；旧组合 fixture 可作为后续语义 oracle。V1/V2 registry wiring 缺陷记录为 C1 的第一个红测试，不在 P0 提交一个长期失败用例。

### C1：核心类型、所有权与 identity

1. 先新增 desired-direction test，证明 V1 必须解 Messages response→Chat、V2 必须解 Chat response→Messages；确认测试为红。
2. 引入 `Protocol`/`CodecId`，删除重复 `Downstream`/`Upstream`/无效 `Version`。
3. 引入 `ports.rs`、`CodecDirection` Strategy port、`directions/*` wiring wrappers 与 cloneable `PreparedCodec` decoder factories；修正 V1/V2 响应绑定，使首个红测试转绿。
4. 实现三项 identity。
5. 新增 `core/protocol_boundary.rs`，但暂不切换生产 attempt。
6. 保留现有 ResponsesViaChat attempt fallback 与消费路径，保证 C1 全量既有测试仍绿；正式 attempt 迁移与 fallback 删除在 C2 原子完成。

验收：core 层 3 identity + 当时已实现的 5 conversion 可 prepare，core 直接请求 Responses→Chat 仍明确未注册；registry 每个 key 恰好对应一个 strategy，且 strategy 自报 pair 与 key 一致；生产 attempt 的 legacy ResponsesViaChat 行为不变且既有 e2e 全绿；V1/V2 encoder/response decoder 方向测试转绿；clone prepared codec 后创建的两个 stream decoder 状态相互独立。

### C2：Responses→Chat V1、attempt 与 consumer 原子迁移

1. 把 Responses→Chat request、Chat→Responses non-stream/stream 封装为正式 direction。
2. 注册 `ResponsesToChatV1`。
3. 在同一提交/Cell 内把三协议 attempt 切到 `protocol_boundary + PreparedCodec`，并删除 legacy else；不得产生中间 fail-closed 提交。
4. `PreparedAttempt` 持有 typed codec plan；日志/序列化需要的兼容 label 只能由 typed id 派生，不能成为第二份选择真相，更不能参与 consumer 行为选择。
5. 普通 non-stream 与 auth accumulator 后的 non-stream 都改为 `new_non_stream_decoder()`，消费 `DecodedResponse { body, usage }`。
6. 普通/auth stream driver 与 pump 都改为 `new_stream_decoder()`；保留 first-frame/carry/commit barrier，带外首帧暂存受 byte/record/deadline 上限约束。
7. 删除 `sse_mode_for`、`decoder_for`、`SseMode` 和 executor 字符串 decode；这与 typed attempt 切换必须是同一个原子 Cell。

验收：9 个 protocol pair 均可 prepare；所有 route-plan consumer 已从 typed factory 取 decoder，不识别 label；其中 Responses↔Messages 此时仍是已冻结的 v1 组合，明确不得宣称“全量直连完成”；Responses→Chat 的 request/response/SSE、普通/auth、错误和 terminal 测试通过。

### C3：Responses→Messages V2 直连

1. 先复制 P0 语义 oracle 到新模块测试。
2. 实现 Responses request→Messages request。
3. 实现 Messages response→Responses non-stream。
4. 实现 Messages SSE→Responses SSE 独立状态机。
5. registry 切换到 `ResponsesToMessagesV2`，再删除该方向旧组合。

验收：新模块生产代码无 Chat encoder/decoder/helper 引用；任意 split、多个交错 tool call、reasoning、usage、rate_limits、EOF、重复 finish 全部通过。

### C4：Messages→Responses V2 直连

与 C3 对称：实现 Messages request→Responses request、Responses response→Messages non-stream、Responses SSE→Messages SSE；registry 切换到 V2 后删除旧组合。

验收：两条 Responses↔Messages 生产调用链均不经过 Chat；所有 6 个 conversion 为单跳，3 identity 完整。

### C5：legacy handler 与 auth transport 去重

1. legacy handler fallback request/response 改走 registry，删除第二套 Responses event 生成职责。
2. rollout on/off 都验证相同 core codec；handler 只保留 legacy transport/渠道选择/日志框架。
3. provider 测试通过后删除 executor 重复 `force_responses_stream`。

验收：普通/auth 的同方向继续从同一个 `PreparedCodec` factory 创建 decoder；用 test-only spy direction/factory counter 证明两条 transport 路径都调用核心 factory，而不只比较最终输出。auth 401/allowlist/stream/store 行为不变；rollout on/off 均无 helper 直调；`codex.rate_limits` 首帧不触发 commit且缓存有界，首个业务帧失败仍可 failover，commit 后绝不 retry。

### C6：清理、全量测试与独立评审

1. 删除旧组合类型、死代码、旧版本类型、字符串 label 分派。
2. 完成 9 格 contract test、语义往返、直连 oracle、consumer/auth/e2e。
3. 更新注释和模块文档，确保不再声称“只注册两个/五个方向”。
4. 执行完整 fmt/clippy/test/grep 门禁。
5. 发起独立 code review，修复高/中优先级问题后重跑门禁。

## 14. 测试设计

### 14.1 9 格 contract tests

表驱动逐格断言 codec id、model、request shape、反向 non-stream response shape、stream 首尾事件。专门加入一条测试防止再次把 request encoder 与同方向 response decoder 错绑；另加 registry 结构测试，断言 9 个 pair 无重复/遗漏、strategy 自报 downstream/upstream 与注册 key 完全一致。

### 14.2 语义往返而非字节往返

为三协议定义测试内 `SemanticProjection`：

```text
ordered system/instructions
ordered roles and visible text
ordered images
ordered tool definitions
ordered tool calls/results (id, name, object arguments)
reasoning text/effort
stop category
real usage
```

比较 `canonicalize(project(A)) == canonicalize(project(B→A))`。排除随机 response id、created timestamp、mapped model、协议强制默认和已在 report 记录的无语义归一化。禁止直接 `assert_eq!(raw_json_a, raw_json_roundtrip)`。

### 14.3 直连对旧组合 oracle

旧组合 fixture 只作为共享语义下限，不要求新旧输出字节相同：

- 旧实现保留的语义，新实现不得丢；
- 新实现可保留更多字段；
- 新实现不得产生额外正常终止、伪造 usage 或 tool 字段；
- 每个有意差异写在 fixture expectation 中，不用模糊 snapshot 一次性批准。

### 14.4 SSE 必测维度

- 每个字节边界拆分一次；
- UTF-8 多字节中间拆分；
- event/data 多行与 CRLF/LF；
- 一 chunk 多 record、record + partial carry；
- text/reasoning/tool 并行与交错；
- tool arguments 非法 JSON、缺 id/name；
- usage absent/known/cache；
- `codex.rate_limits` 在首帧、中段、终止前；
- EOF 半条、缺 terminal、重复 terminal、重复 `finish()`；
- pre-commit decode error 可换候选，post-commit error 不可重试。

### 14.5 auth 必测矩阵

| 下游 | 上游账号 | 非流 | 流 |
|---|---|---|---|
| Responses | Responses identity | accumulator→identity body | identity SSE |
| Chat | Responses | accumulator→Responses→Chat | Responses→Chat SSE |
| Messages | Responses | accumulator→Responses→Messages | Responses→Messages SSE |

每格同时断言 provider 最终 body `stream:true`、`store:false`；unknown non-null field 在网络前失败；现有 strip/allow 字段政策不变；401 最多内部刷新重试一次。

## 15. 验收命令与静态门禁

每 Cell 至少运行相关测试，最终运行：

```bash
cargo test protocol::codec --manifest-path src-tauri/Cargo.toml
cargo test endpoint_executor --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

若仓库 clippy 基线为零告警，最终加 `-- -D warnings`；若存在既有债务，必须记录基线并证明本次不新增，不能把既有无关告警扩成范围外改动。

C4 后的单跳门禁：

```bash
rg -n 'encode_chat|decode_.*chat|ChatStream|responses_to_openai|openai_to_responses' \
  src-tauri/src/protocol/codec/directions/responses_to_messages.rs \
  src-tauri/src/protocol/codec/directions/messages_to_responses.rs
```

必须无生产命中。

C2 后的消费层门禁：

```bash
rg -n 'codec_version\.as_deref|Some\("[a-z_]+_v[0-9]+"\)|sse_mode_for|decoder_for|fn decode_non_stream' \
  src-tauri/src/endpoint_executor src-tauri/src/core/attempt.rs
```

必须无生产命中（测试 fixture 的日志字符串断言允许存在）。

设计模式依赖门禁：

```bash
rg -n 'protocol::codec::(chat::|messages::|responses_codec::|directions::)' \
  src-tauri/src/core src-tauri/src/endpoint_executor \
  src-tauri/src/server src-tauri/src/auth_provider
```

必须无生产命中。consumer 只能依赖 `protocol::codec` Facade re-export 的 ports/value objects；具体 strategy 与协议 primitive 只允许 registry/directions 内部依赖。auth 强制非流聚合器若需公开，必须经 Facade re-export 成稳定 port/type，不能从 consumer 穿透到 `responses_codec` 模块。

legacy helper 门禁：

```bash
rg -n 'responses_to_openai|openai_to_responses' \
  src-tauri/src/server src-tauri/src/core src-tauri/src/endpoint_executor
```

必须无命中。codec 内若暂时复用 Responses↔Chat helper，只允许对应 direction 模块有白名单调用；Responses↔Messages 两个直连模块始终零命中。

范围门禁：

```bash
git status --short
git diff --name-only
```

生产改动必须落在 §12 列出的模块；单元测试可与所属模块同文件，集成测试/fixture 只允许落在 §12 明列的测试文件或 `protocol/codec` 测试目录。实施提示文档只允许同步 `00-main.md`。发现 DB/UI/gate/presets/ClaudeAdaptor 等范围外变化立即停止并报告。

## 16. 回滚与兼容

- 不改 rollout flag、DB schema 或既有日志列；`CodecId::label()` 继续写现有 `codec_version` 文本列。
- 原 V4/V5 组合 fixture（测试数据与语义投影，不是生产 decoder）保留至少一个发布周期，便于回归定位；生产 registry 只注册 V2。
- 关闭 `cross_protocol_codec` 仍能禁用转换组；identity native 路径不受影响。
- rollout 关闭时 legacy handler 仍可工作，但其协议转换已走同一个 registry，不构成第二实现。
- 任何方向出现问题时先关闭 `cross_protocol_codec`，必要时回滚构建/分支；不得把生产 registry 临时切回 Chat 组合，也不得在 consumer 层恢复字符串 special case。

## 17. 完成定义

只有同时满足以下条件才能宣布完成：

1. 9 格都能 prepare，3 identity + 6 conversion 的 request/response/stream contract 全部通过。
2. Responses↔Messages 两个方向的生产调用链不含 Chat 中间协议。
3. attempt 持有 typed prepared codec；executor/driver/pump 不按 label 选择行为。
4. 普通渠道、auth、rollout fallback 共用相同业务 decoder。
5. auth provider 的强制流、store、allowlist、401 刷新语义未退化。
6. fail-closed、commit barrier、post-commit 不重试、exactly-once 全部有测试。
7. 全量 test/fmt/clippy/grep/范围门禁通过。
8. 独立代码评审完成，所有阻断项已修复并重新验证。
9. consumer 只依赖 codec Facade/ports；9 个 strategy pair 唯一且自描述一致；协议状态转换不散落在 driver/executor/handler。
