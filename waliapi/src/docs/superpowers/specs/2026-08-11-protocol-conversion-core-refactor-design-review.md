# 协议转换核心重构设计稿 · 只读复核报告

> 日期：2026-08-11
> 分支：`v0.1.8-protocol-conversion-refactor`
> 被复核文档：`docs/superpowers/specs/2026-08-11-protocol-conversion-core-refactor-design.md`（复核完善稿）
> 复核方式：只读。通读设计稿 831 行，用 codegraph + grep + 定点读取逐条核对 §3「现状基线」、§5 目标类型、§6 边界、§7 矩阵、§9 legacy、§11 auth、§13 cell 顺序、§15 门禁、§2 引用文档，对照当前工作区。未改动任何源文件。

## 1. 总体结论

设计稿的目标状态（§5-§7、§9-§11）与工作区已落地代码**高度一致**：门禁可执行、引用文档全部存在、T00/T04 旧契约与 §2.1 ADR delta 逐字吻合。**主要问题集中在 §3「现状基线」——约 5/7 行已过时**（工作区已实现 C1-C2 大部分及 C3/C4 直连），且 §7.1 有两条 pump 级要求尚未实现也无测试。无阻断性设计缺陷。

## 2. 复核范围与方法

- **事实主张**：§3 现状基线逐行对照工作区代码。
- **目标类型**：§5.1-§5.4 的 `Protocol` / `CodecId` / `PreparedCodec` / `PreparedConversion` / `CodecDirection` / error 分型。
- **边界与矩阵**：§6 `protocol_boundary` 映射表、§7 9 格矩阵与 registry 注册。
- **legacy 收敛**：§9 直连模块、§10 迁移要求、§11.4 auth 路径。
- **门禁**：§15 四条 grep 门禁 + 两条范围门禁，实际运行验证可执行性。
- **引用文档**：§2 四个权威文档存在性与 ADR delta 旧契约列。

## 3. 核对通过项

| 文档主张 | 工作区证据 | 结果 |
|---|---|---|
| §5.1 `Protocol` 三值 + snake_case | `src-tauri/src/protocol/codec/types.rs:12-18` | 一致 |
| §5.2 `CodecId` 9 变体 + label 表 | `types.rs:21-51`，label 与表逐项一致 | 一致 |
| §5.3 `PreparedCodec` clone/factory/`is_identity`/Debug-Serialize 隐藏函数指针 | `types.rs:56-129` | 一致 |
| §5.3 `CodecDirection` trait（`new_response_decoder` / `new_stream_response_decoder` 反向命名） | `src-tauri/src/protocol/codec/direction.rs` | 逐字一致 |
| §5.4 error 分型 + `DecodedResponse{body,usage}` + `CommittedStreamError` | `codec/ports.rs`、`core/attempt.rs:44` | 一致 |
| §6 边界映射表（含 OpenAI 双 endpoint 歧义、CountTokens/Embeddings→None） | `src-tauri/src/core/protocol_boundary.rs` 全表 | 逐项一致 |
| §7 9 格矩阵 + strategy 自报 pair 与 key 一致 | `registry.rs:236-254` + foundation 结构测试 | 一致 |
| §9.1 V2 直连无 Chat 依赖 | C4 门禁 grep 实测 **PASS**；`directions/*` 仅一条注释提及 Chat | 一致 |
| §9.2 32000 synthetic default | `directions/responses_to_messages.rs:177-179` | 一致 |
| §9.6/9.7 `codex.rate_limits` 原样透传 | 两直连方向各自处理（825 / 679 行） | 一致 |
| §10/§11.4 auth provider 保留强制 stream/store/allowlist | `auth_provider/codex_backend.rs:302-325` | 一致 |
| §13 P0+C1-C6 顺序 | `docs/protocol-conversion-refactor/prompts/00-main.md` 同步抽查（§2 同步声明成立） | 一致 |
| §14.1 防错绑测试 + registry 结构测试 | `foundation_tests.rs:137` / `:180` | 一致 |
| §15 设计模式门禁 | 实测 **PASS**（consumer 不穿透 protocol primitive） | 一致 |
| §2.1 T00 决策 8 / T04 旧契约 | `docs/channel-refactor-tasks/00-architecture-decisions.md:117-125`、`04-codec-chat-messages.md:29-34` | 逐字吻合 |

## 4. 发现的问题（按严重度）

### H1 — §3「现状基线（以当前代码为准）」约 5/7 行已过时

工作区已完成 C1-C2 大部分及 C3/C4 直连，但 §3 仍按重构前状态描述：

| §3 行 | 文档描述 | 实际代码 |
|---|---|---|
| `registry.rs` | "Downstream/Upstream + 5 个静态 Direction，缺 identity 和 Responses→Chat" | 已是 **9 格 typed matrix**（3 identity + Responses→Chat，`registry.rs:236-254`） |
| `core/attempt.rs` | "Native/Conversion 双分支、`codec_direction()`、legacy else" | 已走 `protocol_boundary + prepare_pair`，grep `codec_direction\|responses_via_chat` **零命中** |
| `driver.rs` / `sse.rs` | "`codec_version` 字符串 → `SseMode`" | `SseMode`/`sse_mode_for` 已全部删除，pump 已持 factory decoder |
| `endpoint_executor/mod.rs` | "字符串 match + 内联非流组合解码" | `decode_non_stream`（`mod.rs:727`）已改为消费 `prepared_codec.new_non_stream_decoder()`，仅剩 CountTokens/Embeddings/draft 原生直通 |
| §3.2 | "`context` 与两个 decoder 随即 drop" | `attempt.rs:281` 已保存 `prepared_codec` |
| §3.3 | "registry V1/V2 响应方向绑反" | 已修正（`registry.rs:119-136`），且有防错绑测试 `foundation_tests.rs:137` |

**仍准确**的 §3.1 行：`server/handlers.rs`（rollout fallback 直调 legacy helper，`handlers.rs:2440/2485`）、`auth_provider/codex_backend.rs`（provider 保留强制 stream/store/allowlist）。

> 影响：文档自称"以当前代码为准"会误导后续实施。已完成部分无标记、未完成部分（§10/C5）仍成立。
> **建议**：把 §3 改写为"重构前基线（已完成部分）"+ 新增当前状态小节，或在 §13 各 cell 标注已完成。

### M1 — §7.1 两条 pump 级要求未实现、无测试

- **rate_limits 带外首帧暂存**：§7.1 要求"首业务事件前的 `codex.rate_limits` 不满足 commit barrier、暂存受 byte/record/deadline 上限约束"。但 `endpoint_executor/sse.rs`、`driver.rs`、`core/stream_supervisor.rs`、`core/plan_executor.rs` 中 `rate_limits` **零命中**。当前 identity decoder 会原样输出 rate_limits record，pump 首帧即 commit，违反 §4.1(5) 首帧门禁。§13 C2 验收写"带外首帧暂存受上限约束"，工作区没有。
- **identity 逐协议终止校验**：只实现了 Chat `[DONE]` 重复拒绝（`identity.rs:117-122`）；Messages `message_stop`、Responses `response.completed` / 尾随 `[DONE]` 至多一次的校验未实现，`finish()` 也不要求见过 terminal。

> 建议：在剩余 cell（C5/C6）的验收/门禁中显式追踪这两条，并补 §14.4 的 `rate_limits` 首帧测试，否则 identity + quota 首帧流会在带外帧上提前 commit。

### M2 — `PreparedConversion` 与 §5.3/§2.1 目标结构不符，deprecated 表面生产零消费

文档目标 3 字段 `{encoded_request, report, codec}`；实现为 5 字段（多 `context` + `non_stream` / `streaming` adapter，`types.rs:132-145`），且 `prepare_pair` **每次调用急切构造两个 decoder**（`registry.rs:276-279`）。而：

- `.non_stream` / `.streaming` 生产**零消费**（仅 `foundation_tests.rs:43` 测试用）；
- deprecated 的 `prepare` / `prepare_legacy` 生产调用只有 `chat_messages_codec.rs:1495`（在 `#[test]` 内）。

即为"兼容入口"付出的每次两 decoder 分配 + 5 字段结构，生产无人使用——与 §3.2 批评的"构造即丢弃"如出一辙，只是换了位置。

> 建议 C6 删除这些字段与入口，迁移这两个测试到 `prepare_pair` + factory。

### M3 — 字段名与 §5.3 不一致

- §5.3/§11.1 写 `PreparedAttempt.protocol_codec`，实现字段名为 **`prepared_codec`**（`attempt.rs:281`）。
- §5.3 "PreparedAttempt 删除运行时 `Option<String>` 真相"，但实现仍保留 `codec_version: Option<String>`（`attempt.rs:280`，仅日志/序列化）。§16 已说明写日志列，但 §5.3 措辞与 §16 需二选一表述清楚。

### L1 — §15 C2 门禁 `fn decode_non_stream` 会误报

函数名仍存在（`endpoint_executor/mod.rs:727`），但已是正确的工厂包装。按函数名 grep 会"失败"却无实际违规。

> 建议门禁改为 grep 字符串分派模式（`codec_version` 用于选择 / match label），或把 wrapper 内联到调用点。

### L2 — §5.1/§5.2 要求删除的类型仍在 Facade

`Downstream`/`Upstream`（`registry.rs:27-38`）、`Version`（`registry.rs:63`）、`CodecVersion`（`report.rs:12`，已无生产使用）仍定义并 re-export（`codec/mod.rs:38-39`）。C1 验收写"删除"，代码只引入未删除——C6 清理项，但 cell 验收与实际不符。

### L3 — 4 个 Chat 系方向未入 `directions/*`；helper 未标 deprecated

- §4.4 目标结构列出 6 个 direction 文件；实现只有 2 个 V2 模块，Chat↔Messages / Chat↔Responses / Responses→Chat 以 `FnDirection` 内联在 `registry.rs`。Facade 边界与矩阵结果都正确，但与 §4.4 "strategy wiring 必须进入 `directions/*`"字面不符。
- `protocol/mod.rs` 的 `openai_to_responses` / `responses_to_openai` 尚未标 `#[deprecated]`（§10 item 6），且 `handlers.rs:2440/2485`、`sse_bridge.rs:323/420` 仍生产调用（与 C5 待办一致，非错误）。

## 5. 建议下一步

1. **修订 §3**：改写为"重构前基线（已完成部分）"，并新增"当前状态"标注已完成 cell（C1-C4），使文档与实际一致。
2. **为 M1 补明确追踪项**：rate_limits 首帧暂存 + identity 终止校验，落在 C5/C6 验收与测试清单。
3. **C6 清理清单补全**：删除 `PreparedConversion` 的 `context` / `non_stream` / `streaming`、`prepare` / `prepare_legacy`、`Downstream` / `Upstream` / `Version` / `CodecVersion`；迁移 foundation 两个测试；给 helper 标 `#[deprecated]`。
4. **修订 §15 门禁**：C2 门禁的 `fn decode_non_stream` 改为按字符串分派模式 grep。

## 6. 附录：门禁实测结果（2026-08-11 工作区）

| 门禁 | 命令 | 实测 |
|---|---|---|
| C4 直连单跳 | `rg 'encode_chat\|decode_.*chat\|ChatStream\|responses_to_openai\|openai_to_responses' directions/{responses_to_messages,messages_to_responses}.rs` | **PASS**（仅一条注释提及 Chat） |
| C2 消费层 | `rg 'codec_version\.as_deref\|Some("[a-z_]+_v[0-9]+")\|sse_mode_for\|decoder_for\|fn decode_non_stream' endpoint_executor core/attempt.rs` | 仅 `fn decode_non_stream` 名称命中（L1）；`codec_version.as_deref` 均为测试断言 |
| 设计模式 | `rg 'protocol::codec::(chat::\|messages::\|responses_codec::\|directions::)' core endpoint_executor server auth_provider` | **PASS** |
| legacy helper | `rg 'responses_to_openai\|openai_to_responses' server core endpoint_executor` | 命中 `server/handlers.rs:2440/2485`——与 C5 待办一致，非文档错误 |
