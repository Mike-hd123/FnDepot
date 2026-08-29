# protocol 模块结构化重构 Implementation Plan

> 本方案仅调整 Rust 文件与模块组织，不删除死代码、不修改转换逻辑、不改变现有公开路径。实现时按任务顺序执行；每个任务必须独立编译、全量测试、独立提交并可单独回滚。

**Goal:** 将 `src-tauri/src/protocol/` 中的超大生产文件和测试块按职责拆分；生产文件目标不超过 600 行（硬上限 650），测试文件不超过 800 行，同时保持行为、类型、错误、事件顺序和公共 API 不变。

**Architecture:**

- 生产模块目录化：`foo.rs` → `foo/mod.rs` + 职责子模块，原 `crate::...::foo::*` 路径由 facade 精确 re-export 保持。
- 平铺测试文件：非目录模块使用 `#[path = "foo_tests.rs"] mod foo_tests;`，确保测试仍是原模块的子模块并保留 `super::` 语义。
- 目录模块测试：使用 `<module>/tests.rs` 或 `<module>/tests/`，测试显式 import 所测子模块，不依赖 facade 意外导出私有 helper。
- 跨子模块生产依赖：只把确实需要跨子模块访问的项提升为 `pub(super)`；不得为了省 import 把内部项做成 `pub`。
- facade 只 re-export 拆分前已经是 `pub` 且需要保持路径的项；内部协作类型不得 re-export。

**Tech Stack:** Rust 2021 / Tauri 2 / `cargo fmt` / `cargo test` / `cargo clippy` / `git mv`

**Spec:** `docs/superpowers/specs/2026-08-11-protocol-conversion-core-refactor-design.md`。本方案不得改变该设计的功能语义、转换矩阵、错误契约和验收标准。

## 执行约定

所有命令默认从仓库的 `src-tauri/` 目录执行：

```bash
cd /Users/xian/Project/ai/WaLiAPI/src-tauri
```

开始实现前，方案文档应单独提交或明确排除；当前任务不得使用 `git add -A`。每个任务统一按以下门禁收尾：

```bash
cargo fmt --all
cargo fmt --all -- --check
cargo test
git diff --check
git status --short
git add <本任务明确列出的路径>
git diff --cached --check
git diff --cached --name-status
git commit -m "refactor（v0.2.1-optimal）：<中文说明>"
```

若 `cargo test` 失败，先判断是否为本任务引入；不得用改业务逻辑、删测试或新增 `#[allow(...)]` 的方式绕过。提交前的暂存区只能包含本任务文件。

## 全局约束

- **零行为变更**：只搬移、拆文件、调整 import、精确 re-export、为跨子模块协作提升到 `pub(super)`。
- **零公共 API 变化**：既有公开路径继续可达，不新增公开模块、公开类型或公开函数。
- **不做死代码清理**：`is_anthropic_request`、`is_responses_request`、`estimate_anthropic_input_tokens` 本次只搬移并保持原可见性；清理由独立方案处理。
- **每任务全量验证**：过滤测试仅用于快速定位，不能替代完整 `cargo test`。
- **文件规模**：生产文件目标 ≤600 行、硬上限 650；测试文件硬上限 800。
- **状态机优先保持内聚**：拆文件只沿已有函数/类型边界进行，不在本次重写状态机。
- **禁止顺手改名或格式重写 JSON**：避免把结构 diff 混入语义 diff。

## 已核实的公共路径

| 必须保持的路径 | 主要消费方 |
|---|---|
| `codec::{CodecRegistry, PreparedCodec, Protocol, StreamDecoder, DecodeError, Usage, ResponsesEventAccumulator}` | `core/attempt.rs`、`endpoint_executor/{sse,mod,driver}.rs` |
| `codec::sse::{record_end, parse_data_payload}` | `endpoint_executor/sse.rs` |
| `codec::messages::MessagesSseState` | `protocol/sse_bridge.rs` |
| `codec::chat::{encode_chat_to_messages, NonStreamResponseDecoder, ChatStreamDecoder}` | `codec/registry.rs` |
| `codec::messages::{encode_messages_to_chat, NonStreamResponseDecoder, MessagesStreamDecoder}` | `codec/registry.rs` |
| `protocol::{extract_api_key, openai_to_responses, responses_to_openai, openai_to_anthropic, anthropic_to_openai, estimate_anthropic_input_tokens, is_anthropic_request, is_responses_request}` | 保持现状；不得因仓库内引用少而删除或降可见性 |
| `protocol::responses::{ResponsesSseAssembler, StreamState, ToolCallState, convert_openai_sse_to_responses, create_response_created_event, create_synthetic_completed_events, parse_usage_from_sse_chunk}` | `server/handlers.rs`、`protocol/sse_bridge.rs` |
| `protocol::anthropic::AnthropicStreamState` | `server/handlers.rs` |
| `protocol::sse_bridge::{is_anthropic_upstream, UpstreamSseBridge}` | `server/handlers.rs` |
| `codec::directions::{MESSAGES_TO_RESPONSES_V2, RESPONSES_TO_MESSAGES_V2}` | `codec/registry.rs` |

`codec::responses_codec::*` 的深层路径目前无模块外消费者，但其中原有 `pub` 项仍保持原可见性；本方案不借机缩减 API。`ResponsesChatState`、两个 direction 的 decoder/stream 状态类型当前是私有实现，不得加入 facade。

## 目标目录结构

```text
src/protocol/
├── mod.rs
├── detect.rs
├── legacy/
│   ├── mod.rs
│   ├── responses_encode.rs
│   ├── responses_decode.rs
│   ├── anthropic_encode.rs
│   ├── anthropic_decode.rs
│   └── tests/{mod.rs,responses.rs,anthropic.rs}
├── anthropic.rs
├── anthropic_tests.rs
├── responses/
│   ├── mod.rs
│   ├── assembler.rs
│   ├── state.rs
│   ├── convert.rs
│   ├── events.rs
│   ├── usage.rs
│   └── tests.rs
├── sse_bridge.rs
├── sse_bridge_tests.rs
├── thinking.rs
└── codec/
    ├── mod.rs
    ├── identity.rs
    ├── identity_tests.rs
    ├── tests/chat_messages/
    │   ├── mod.rs
    │   ├── support.rs
    │   ├── chat_request.rs
    │   ├── chat_response_stream.rs
    │   ├── messages_request.rs
    │   ├── messages_response_stream.rs
    │   └── registry.rs
    ├── chat/{mod.rs,encode.rs,decode.rs,stream.rs,tests.rs}
    ├── messages/{mod.rs,encode.rs,decode.rs,stream.rs}
    ├── responses_codec/
    │   ├── mod.rs
    │   ├── encode_chat.rs
    │   ├── chat_tools.rs
    │   ├── encode_messages.rs
    │   ├── decode.rs
    │   ├── accumulator.rs
    │   ├── state.rs
    │   ├── stream.rs
    │   └── tests.rs
    └── directions/
        ├── mod.rs
        ├── messages_to_responses/
        │   ├── mod.rs
        │   ├── encode.rs
        │   ├── decode.rs
        │   ├── stream/{mod.rs,record.rs,emit.rs}
        │   └── tests.rs
        └── responses_to_messages/{mod.rs,encode.rs,decode.rs,stream.rs,tests.rs}
```

## Task 0：建立基线与冻结边界

**Writes:** 无。

- [ ] 记录 `git status --short`；若除方案文档外还有改动，明确归属并在后续暂存时排除。
- [ ] 运行 `cargo test`，记录通过数量和既有 warnings。当前复核基线为全绿；实现当天重新确认。
- [ ] 运行 `cargo clippy --all-targets --all-features`，保存既有告警摘要，用于最终判断“无新增告警”。
- [ ] 用 `rg` 保存上述公共路径的当前定义与消费位置；不要只检查 `server/`。
- [ ] 记录目标文件行数，作为最终规模审计基线。

## Task 1：整理大型 codec 测试聚合文件

**Files:**

- Remove: `src/protocol/codec/chat_messages_codec.rs`
- Create: `src/protocol/codec/tests/chat_messages/{mod.rs,support.rs,chat_request.rs,chat_response_stream.rs,messages_request.rs,messages_response_stream.rs,registry.rs}`
- Modify: `src/protocol/codec/mod.rs`

- [ ] 将原测试按现有自然边界拆分：Chat request（约 25–332）、Chat response/stream（333–629）、Messages request（630–1049）、Messages response/stream（1050–1423）、registry/稳定码（1424–结尾）。
- [ ] 共享的 `reject_features` 等测试 helper 放入 `support.rs`；各测试文件显式 import `crate::protocol::codec::*` 所需项。
- [ ] `codec/mod.rs` 改为：
  ```rust
  #[cfg(test)]
  #[path = "tests/chat_messages/mod.rs"]
  mod chat_messages_codec_tests;
  ```
- [ ] 确认每个测试文件 ≤800 行，测试名称和断言内容不变。
- [ ] 执行统一门禁并提交：`拆分 chat/messages codec 聚合测试`。

## Task 2：抽取三个非目录模块的内联测试

**Files:**

- Create: `src/protocol/{sse_bridge_tests.rs,anthropic_tests.rs}`
- Create: `src/protocol/codec/identity_tests.rs`
- Modify: `src/protocol/{sse_bridge.rs,anthropic.rs}`、`src/protocol/codec/identity.rs`

- [ ] 将三个 `mod tests { ... }` 的内容原样移到平铺文件，去掉模块外壳，保留 `use super::*;`。
- [ ] 原文件分别使用显式路径声明：
  ```rust
  #[cfg(test)]
  #[path = "sse_bridge_tests.rs"]
  mod sse_bridge_tests;
  ```
  `anthropic.rs` 和 `identity.rs` 使用对应文件名；`identity.rs` 的相对路径仍是 `identity_tests.rs`。
- [ ] 先运行三个过滤测试定位问题，再运行完整门禁。
- [ ] 提交：`抽取 protocol 平铺测试文件`。

## Task 3：拆分 protocol 根转换逻辑

**Files:**

- Create: `src/protocol/detect.rs`
- Create: `src/protocol/legacy/{mod.rs,responses_encode.rs,responses_decode.rs,anthropic_encode.rs,anthropic_decode.rs,tests/mod.rs,tests/responses.rs,tests/anthropic.rs}`
- Rewrite: `src/protocol/mod.rs`

**Facade contract:** `detect` 与 `legacy` 必须是私有模块；只从 `protocol` 根 re-export 原有八个 `pub fn`，不得新增 `protocol::detect::*` 或 `protocol::legacy::*` 公共路径。

- [ ] `detect.rs` 搬入 `extract_api_key`、`is_anthropic_request`、`is_responses_request`，保持函数签名和 `pub` 可见性不变。
- [ ] `legacy/responses_encode.rs` 搬入 `openai_to_responses`（自包含）。
- [ ] `legacy/responses_decode.rs` 搬入 `responses_to_openai`、`convert_responses_input_to_messages`、`responses_tool_choice_to_chat`（其唯一调用方是 `responses_to_openai`，mod.rs:325）。
- [ ] `legacy/anthropic_encode.rs` 搬入 `openai_to_anthropic`。
- [ ] `legacy/anthropic_decode.rs` 搬入 `anthropic_to_openai`、`estimate_anthropic_input_tokens` 及其私有助手。
- [ ] `legacy/mod.rs` 只公开 re-export 五个原有 public conversion/token 函数；跨子模块 helper 使用 `pub(super)`，不公开 re-export。
- [ ] 原根测试按 Responses 与 Anthropic 两组拆到 `legacy/tests/`；测试显式 import 被测公开函数或 `pub(super)` helper。
- [ ] `protocol/mod.rs` 保持既有公共模块，新增私有装配：
  ```rust
  pub mod anthropic;
  pub mod codec;
  mod detect;
  mod legacy;
  pub mod responses;
  pub mod sse_bridge;
  pub mod thinking;

  pub use detect::{extract_api_key, is_anthropic_request, is_responses_request};
  pub use legacy::{
      anthropic_to_openai, estimate_anthropic_input_tokens, openai_to_anthropic,
      openai_to_responses, responses_to_openai,
  };
  ```
- [ ] 确认所有生产文件 ≤650 行，执行完整门禁并提交：`拆分 protocol 根转换逻辑`。

## Task 4：目录化 protocol/responses

**Files:** `src/protocol/responses/{mod.rs,assembler.rs,state.rs,convert.rs,events.rs,usage.rs,tests.rs}`，删除原 `responses.rs`。

**Facade contract:** 仅 re-export 原有公开项：`ResponsesSseAssembler`、`StreamState`、`ToolCallState`、`convert_openai_sse_to_responses`、`create_response_created_event`、`create_synthetic_completed_events`、`parse_usage_from_sse_chunk`。

- [ ] `assembler.rs`：`ResponsesSseAssembler`。
- [ ] `state.rs`：`StreamState`、`ToolCallState`、`now_ts`、`next_seq`；后两个改为 `pub(super)`，供 `convert.rs`/`events.rs` 使用但不 re-export。
- [ ] `convert.rs`：`convert_openai_sse_to_responses`。
- [ ] `events.rs`：两个 event 构造函数。
- [ ] `usage.rs`：`parse_usage_from_sse_chunk`。
- [ ] 测试移入 `tests.rs`，私有 helper 通过具体子模块路径 import；断言不变。
- [ ] 核对 `server/handlers.rs` 和 `sse_bridge.rs` 的既有路径，执行门禁并提交：`目录化 protocol responses`。

## Task 5：目录化 codec/responses_codec

**Files:** `src/protocol/codec/responses_codec/{mod.rs,encode_chat.rs,chat_tools.rs,encode_messages.rs,decode.rs,accumulator.rs,state.rs,stream.rs,tests.rs}`，删除原 `.rs`。

**Facade contract:** re-export 拆分前已有的 public 函数和 decoder/accumulator 类型；不 re-export 私有的 `ResponsesChatState`、`ToolCallState`、`responses_response_id`。

- [ ] `encode_chat.rs`：Chat request 主转换、message/content/reasoning/tool-call 转换；控制在 600 行内。
- [ ] `chat_tools.rs`：tool list/tool choice 转换。
- [ ] `encode_messages.rs`：`encode_messages_to_responses`、`encode_responses_to_messages`。
- [ ] `decode.rs`：三个 non-stream decoder、响应转换、finish reason、usage。
- [ ] `accumulator.rs`：`ResponsesEventAccumulator`。
- [ ] `state.rs`：`ResponsesChatState`、内部 `ToolCallState`、`responses_response_id`；跨 `stream.rs`/tests 所需项精确标为 `pub(super)`。
- [ ] `stream.rs`：四个 stream decoder 及其 trait impl。
- [ ] `tests.rs` 显式 import 具体子模块的内部测试对象，测试内容不变。
- [ ] `codec/mod.rs` 继续只通过 `pub use responses_codec::ResponsesEventAccumulator` 暴露 facade primitive。
- [ ] 执行门禁并提交：`目录化 codec responses_codec`。

## Task 6：目录化 messages_to_responses direction

**Files:** `src/protocol/codec/directions/messages_to_responses/{mod.rs,encode.rs,decode.rs,stream/mod.rs,stream/record.rs,stream/emit.rs,tests.rs}`，删除原 `.rs`。

**Facade contract:** 只 re-export 原有 public 项：`MESSAGES_TO_RESPONSES_V2`、`MessagesToResponses`、`encode_request`、`decode_response`。`ResponsesMessageDecoder` 与 `ResponsesMessagesStream` 保持内部实现。

- [ ] `mod.rs` 保留 strategy struct/static 与 `CodecDirection` impl；内部 decoder/stream 的类型及构造所需字段精确标为 `pub(super)`，保持现有 struct literal 接线方式。
- [ ] `encode.rs` 搬入 request 转换及其 helper。
- [ ] `decode.rs` 搬入 non-stream decoder、`decode_response`、usage helper；decoder 类型只设 `pub(super)`。
- [ ] `stream/mod.rs` 放状态结构、构造器、索引 helper 和 `StreamDecoder` impl。
- [ ] `stream/record.rs` 放 SSE record 解析/事件分派；`stream/emit.rs` 放终止、part 生命周期和完成事件生成。跨文件方法使用最窄的 `pub(super)`。
- [ ] 测试移入 `tests.rs`，显式 import `stream::ResponsesMessagesStream` 等内部对象。
- [ ] 确认三个 stream 文件均 ≤600 行，执行门禁并提交：`目录化 messages_to_responses direction`。

## Task 7：目录化 responses_to_messages direction

**Files:** `src/protocol/codec/directions/responses_to_messages/{mod.rs,encode.rs,decode.rs,stream.rs,tests.rs}`，删除原 `.rs`。

**Facade contract:** 只 re-export `RESPONSES_TO_MESSAGES_V2`、`ResponsesToMessages`、`encode_request`、`decode_messages_response`；`MessagesResponseDecoder`、`MessagesResponsesStream` 和 usage helper 保持内部。

- [ ] `mod.rs` 保留 strategy struct/static 与 `CodecDirection` impl；内部 decoder/stream 的类型及构造所需字段精确标为 `pub(super)`，保持现有 struct literal/`new` 接线方式。
- [ ] `encode.rs` 搬入 request 转换及 helper。
- [ ] `decode.rs` 搬入 non-stream decoder、响应转换、usage/merge helper；内部 decoder 为 `pub(super)`。
- [ ] `stream.rs` 搬入 stream state 与 impl；内部 stream 类型为 `pub(super)`。
- [ ] 测试移入 `tests.rs` 并显式 import 内部项。
- [ ] 执行门禁并提交：`目录化 responses_to_messages direction`。

## Task 8：目录化 codec/chat

**Files:** `src/protocol/codec/chat/{mod.rs,encode.rs,decode.rs,stream.rs,tests.rs}`，删除原 `chat.rs`。

**Facade contract:** 精确 re-export `encode_chat_to_messages`、`NonStreamResponseDecoder`、`decode_chat_response_to_messages`、`usage_from_chat`、`ChatSseState`、`ChatStreamDecoder`。

- [ ] `encode.rs` 搬入 request 转换和 request helper。
- [ ] `decode.rs` 搬入 non-stream decoder、response 转换和 usage。
- [ ] `stream.rs` 搬入 `ToolAccum`、`ChatSseState`、`ChatStreamDecoder`，保持状态机内聚。
- [ ] 原内联测试移入 `tests.rs`；如测试私有 helper，使用具体子模块路径与最窄 `pub(super)`。
- [ ] 确认 `registry.rs` 的 `chat::...` 路径不变，执行门禁并提交：`目录化 codec chat`。

## Task 9：目录化 codec/messages

**Files:** `src/protocol/codec/messages/{mod.rs,encode.rs,decode.rs,stream.rs}`，删除原 `messages.rs`。

**Facade contract:** 精确 re-export `encode_messages_to_chat`、`NonStreamResponseDecoder`、`decode_messages_response_to_chat`、`usage_from_messages`、`MessagesSseState`、`MessagesStreamDecoder`。

- [ ] `encode.rs` 搬入 request 转换及 Anthropic message/tool/thinking helper。
- [ ] `decode.rs` 搬入 non-stream decoder、response 转换和 usage。
- [ ] `stream.rs` 搬入 `MsgToolAccum`、`MessagesSseState`、`MessagesStreamDecoder`。
- [ ] 本文件原本无内联测试，不创建空 `tests.rs`；相关用例继续位于 Task 1 的聚合测试目录。
- [ ] 确认 `registry.rs` 与 `sse_bridge.rs` 的路径不变，执行门禁并提交：`目录化 codec messages`。

## Task 10：最终审计

**Writes:** 仅允许修正本次重构造成的 import、re-export、模块声明和格式问题；不得删代码或改业务逻辑。

- [ ] 用 `rg` 全仓核对“已核实的公共路径”，确认定义与消费方仍存在。
- [ ] 核对 facade：所有原 public 项仍保持原路径；没有新增公开的 `detect`、`legacy`、state/helper/decoder 类型。
- [ ] 核对可见性：新增的 `pub(super)` 每一处都有跨子模块生产依赖或测试依赖；无新增 `pub(crate)`/`pub`。
- [ ] 核对测试组织：生产文件中不存在大段 `#[cfg(test)] mod tests { ... }`；测试文件均 ≤800 行。
- [ ] 核对生产文件规模：
  ```bash
  find src/protocol -name '*.rs' \
    ! -name '*_tests.rs' ! -name 'tests.rs' ! -path '*/tests/*' \
    -print0 | xargs -0 wc -l | sort -n
  ```
  所有生产文件 ≤650 行；超过 600 行必须在提交说明中注明保持内聚的理由。
- [ ] 运行最终验证：
  ```bash
  cargo fmt --all -- --check
  cargo test
  cargo clippy --all-targets --all-features
  git diff --check
  ```
- [ ] 与 Task 0 的 clippy 基线比较，确认无新增 warning；不得顺手清理既有 warning。
- [ ] 提交：`完成 protocol 模块结构与 re-export 审计`。

## 验收标准

1. `cargo test` 全绿，包含 `tests/` 集成测试；测试数量不得无解释减少。
2. `cargo clippy --all-targets --all-features` 无新增告警。
3. 所有原公开路径、函数签名、类型和可见性保持；无新增公开 API。
4. protocol 生产文件均 ≤650 行，测试文件均 ≤800 行。
5. 大测试块已物理抽离，生产文件只保留测试模块声明。
6. 行为相关代码 diff 仅包含搬移、import、module、re-export、`pub(super)` 和 rustfmt 变化。
7. 每个任务独立提交，暂存区范围明确，可逐条回滚。

## 本次明确不做

- 删除 `is_anthropic_request`、`is_responses_request`、`estimate_anthropic_input_tokens` 或任何其他死代码。
- 重写转换算法、状态机、错误文本、JSON 字段、SSE 事件或 usage 计算。
- 清理仓库既有 clippy/rustc warnings。
- 拆分 `protocol/` 之外的大文件；另行制定方案。
