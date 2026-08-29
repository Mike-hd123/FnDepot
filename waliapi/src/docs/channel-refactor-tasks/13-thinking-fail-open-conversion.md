# T13：thinking/推理字段 fail-open 转换（Claude Messages ↔ OpenAI Chat）

> **状态:已实现**（codec + legacy 全部完成,`cargo test protocol::` 验证中）

## 目的

修复 WaLiAPI 对下游 Claude Messages 请求中 `thinking` / `output_config` /
`container` / `context_management` 一律 400 拒绝、对上游 OpenAI 返回的
`reasoning_content` 一律 502 的缺陷,改为**参考 CPA / 9router 的 fail-open 转换**:
不查模型能力、不拒绝,直接映射透传,由上游自行裁决。

- 用户报告:`API Error: 400 OpenCode-GO: incompatible with OpenAI Chat Completions:
  thinking, containers, output_config, and context management require a native Anthropic Messages channel`
- 参考实现:CPA（`/tmp/CLIProxyAPI`，转换基准 T12 已复核）+ 9router `thinkingUnified.js`
  （`/tmp/9router/open-sse/translator/concerns/thinkingUnified.js`）
- 对应实现:`src-tauri/src/protocol/`（codec + legacy 两条路径）

## 背景与根因

错误字符串精确匹配 **legacy 路径**（`protocol/mod.rs:593` + `handlers.rs:1608`），
而 `new_routeplan` flag 默认 OFF,legacy 是生产默认路径。codec 路径（rollout 目标）
同样拒绝 thinking。因此两条路径都要改。

**方向接线（权威依据 `endpoint_executor/driver.rs:42-53`）**:

| 方向 | 执行器 | 请求编码 | 流解码 | 非流解码 |
|---|---|---|---|---|
| 下游 Messages → 上游 Chat | `messages_to_chat_v1` → `SseMode::ChatToMessages` | `messages::encode_messages_to_chat` | `chat::ChatStreamDecoder` | `chat::NonStreamResponseDecoder` → `decode_chat_response_to_messages` |
| 下游 Chat → 上游 Messages | `chat_to_messages_v1` → `SseMode::MessagesToChat` | `chat::encode_chat_to_messages` | `messages::MessagesStreamDecoder` | `messages::NonStreamResponseDecoder` → `decode_messages_response_to_chat` |

> 方向映射曾有过相反误判。`registry.rs` 的 `Direction` 与 `sse.rs:482` `decoder_for`
> 均按 label 重新分派,`driver.rs:42-53` 是权威。

## 决策记录（已确认）

1. **两个方向都改**:下游 Messages→上游 Chat 与下游 Chat→上游 Messages。
2. **thinking 做 fail-open 转换**（参考 CPA/9router）;`container` /
   `context_management` / `context_management_config` 做 **allowlist 丢弃**（记录 normalized）。
3. **codec + legacy 都改**:codec 是 rollout 目标,legacy 是生产默认（用户 bug 来自 legacy）。
4. **budget→level 阈值表用 CPA 版**（非 9router 版）。
5. 响应侧采用 **CPA 做法**（`reasoning_content` 始终保留为 `thinking` 块,即使 content 非空）;
   9router 在 content 非空时删除 reasoning,不适合给 Claude 下游。
6. **首版不建模型能力表,恒降级;能力表拉取留作后续升级窗口**。
   - 首版:不做任何"模型是否支持 reasoning"的能力查询/注册表;`xhigh/max→high` 恒降级,
     由上游自行裁决。
   - **后续升级窗口(预留,不在首版范围)**:参考 CPA 自动拉取 `github.com/router-for-me/models`
     仓库 + 定时刷新（CPA 是启动 + 每 3h,我们用 **24h**）。架构上预留
     `resolve_effort_support(model)` 函数位,首版实现为 `None`(恒降级)。
   - **限制(评估结论)**:该仓库无 `openai` provider(只有 codex-* 编程代理专用模型),
     对"下游 Claude → 上游 OpenAI"方向帮不上(OpenAI 方向仍走官方 `/v1/models` 或恒降级);
     只对"下游 OpenAI → 上游 Claude"方向(仓库有 claude 15 条 + levels)有精确映射价值。
     因此拉取是可选优化,非核心依赖;首版恒降级在 OpenAI 方向恰好安全。

### 决策 6 补充调研:CPA / 9router 的能力表机制

- **CPA**:能力数据在独立仓库 `github.com/router-for-me/models`（`models.json`，一条模型 =
  `ModelInfo`，含 `ThinkingSupport{Min,Max,ZeroAllowed,DynamicAllowed,Levels}` + 窗口/模态/web
  搜索等）。编译时 `go:embed` 内嵌兜底，启动 + **每 3 小时**从两个 URL 自动热更新（带引用计数、
  校验）。维护者 CI 跑 `refresh-model-catalogs.sh` 订阅上游新模型。**未知模型 `modelInfo==nil`
  → `CapabilityUnknown`，跳过验证直接透传**——与 WaLiAPI 的"一律透传"殊途同归。
- **9router**:`capabilities.js`（353 行）能力表**内联在代码里**，纯手工维护，每出模型改代码提 PR。
  `applyThinking` 查 `caps.reasoning`，模型无推理能力时 `stripAll` 剥掉字段。
- **WaLiAPI**:不维护。`xhigh/max→high` 恒降级 = CPA 对未知模型（`supportsMax` 默认 false）的行为。

## 映射规则基准（CPA）

**budget_tokens → level**（CPA `ConvertBudgetToLevel`）:

| budget | level |
|---|---|
| `-1` | `auto` |
| `0` | `none` |
| 1–512 | `minimal` |
| 513–1024 | `low` |
| 1025–8192 | `medium` |
| 8193–24576 | `high` |
| ≥24577 | `xhigh` |

**Claude thinking → OpenAI reasoning_effort**（CPA `ConvertClaudeRequestToOpenAI`）:

| 输入 | reasoning_effort |
|---|---|
| `thinking.type="enabled"` + `budget_tokens` | `budget_to_level(budget)` |
| `thinking.type="enabled"` 无 budget | `auto` |
| `thinking.type="adaptive"` + `output_config.effort` | 透传 effort（小写） |
| `thinking.type="adaptive"` 无 effort | `xhigh` |
| `thinking.type="disabled"` | `none` |
| 未知 `thinking.type` | 忽略（不写字段） |

**OpenAI reasoning_effort → Claude thinking**（CPA `ConvertOpenAIRequestToClaude` +
`MapToClaudeEffort`）:

| reasoning_effort | 输出 |
|---|---|
| `none` | `thinking.type="disabled"` |
| `auto` | `thinking.type="adaptive"`（无 budget_tokens） |
| 其他 | `thinking.type="adaptive"` + `output_config.effort = map_effort_to_claude(effort)` |

`map_effort_to_claude`:

| 输入 | 输出 |
|---|---|
| `minimal` | `low` |
| `low` / `medium` / `high` | 原样 |
| `xhigh` / `max` | `high`（恒定降级,不引入模型能力表;与 9router `claude-adaptive` 一致） |
| `auto` | `high` |

9router `claude-adaptive` 佐证（`thinkingUnified.js:238-249`）:输出恒为
`{thinking:{type:"adaptive"}}` + `{output_config:{effort: level==="xhigh"?"high":level}}`。
9router 注释明确 Anthropic 要求显式 `thinking:{type:"adaptive"}`,与 CPA 的
`output_config.effort` 方案一致——**两个字段一起写**。

**响应侧**（CPA `ConvertClaudeResponseToOpenAINonStream` / stream）:
上游 `reasoning_content`（string 或 `{text}`）→ 下游 Messages `{"type":"thinking","thinking":…}`
块,**始终保留（即使 content 非空）**。`redacted_thinking` 无文本,忽略。

## 实现设计

### 1. 新共享模块 `src-tauri/src/protocol/thinking.rs`

纯函数,codec 与 legacy 共用:

```rust
/// CPA ConvertBudgetToLevel。
pub fn budget_to_level(budget: i64) -> Option<&'static str> {
    match budget {
        -1 => Some("auto"),
        0 => Some("none"),
        1..=512 => Some("minimal"),
        513..=1024 => Some("low"),
        1025..=8192 => Some("medium"),
        8193..=24576 => Some("high"),
        _ => Some("xhigh"), // ≥24577
    }
}

/// CPA MapToClaudeEffort（无模型注册表,一律保守降 high）。
pub fn map_effort_to_claude(effort: &str) -> &'static str {
    match effort.to_ascii_lowercase().as_str() {
        "minimal" => "low",
        "low" | "medium" | "high" => effort, // 回写原始大小写
        _ => "high", // xhigh / max / auto / 未知
    }
}
```

`map_effort_to_claude` 返回值保持"原样"而非 `to_ascii_lowercase` 结果,避免重复分配。
在 `protocol/mod.rs` 注册 `pub mod thinking;`。

### 2. report 机制:丢弃/转换可观测

`ConversionReport` 已有 `normalized: Vec<String>` 字段但恒空,`encode` fn 签名不返回
report。最小侵入方案:

- `codec/report.rs`:`ConversionContext` 增加 `normalized: Vec<String>`（Default 兼容,
  既有 `ConversionContext::new` 调用处无需改）。
- encode 在丢弃 `container`/`context_management` 等时 push JSON pointer（如 `/container`）。
- `codec/registry.rs` `prepare`:`ConversionReport::ok()` → `ConversionReport::new(vec![], context.normalized.clone())`。
- legacy 路径无 report 概念,丢弃不记录（保持现状）。

### 3. 方向 A:下游 Messages → 上游 Chat

#### 3a. codec 请求侧 `messages.rs::encode_messages_to_chat`

- 从 SUPPORTED_TOP_LEVEL 拒绝扫描中摘出 `thinking` / `output_config`:不再 reject,
  改为在装配段插入 `reasoning_effort`（映射规则见上）。
  - `thinking.type="enabled"`:`budget_to_level(budget_tokens)`;无 budget → `auto`。
  - `thinking.type="adaptive"`:`output_config.effort` 透传（小写）,无 effort → `xhigh`。
  - `thinking.type="disabled"` → `reasoning_effort="none"`。
  - 未知 type → 不写字段。
- `container` / `context_management` / `context_management_config`:fail-open **丢弃**,
  push normalized（`/container` 等）。
- **消息内容块** `convert_anthropic_message_to_chat`（当前 403-409 拒绝）:
  - assistant 消息的 `thinking` 块 → 提取 `thinking` 文本,追加为该条 Chat assistant 消息的
    `reasoning_content`（与 content / tool_calls 同消息;assistant 消息不拆分,沿用 CPA 排序语义）。
  - `redacted_thinking` → 忽略。
  - user / system 消息内的 thinking → 忽略（安全:不把推理注入用户通道）。
- **system 数组** `codec/request.rs::anthropic_system_to_chat`（当前 155-161 拒绝）:
  - `thinking` 块 → 丢弃（fail-open）。
  - `cache_control` 保持拒绝（PromptCache,回归不变）。

#### 3b. codec 响应侧 `chat.rs`

- 非流式 `decode_chat_response_to_messages`（当前 713-722 拒绝）:
  删拒绝,`message.reasoning_content` / `thinking` / `reasoning` 提取文本 →
  `{"type":"thinking","thinking":…}` 块 push 到 `content_blocks` **在 text 块之前**。
  `reasoning_content` 支持 string 与 `{"text":…}` 两种形态。
- 流式 `ChatSseState`（当前 1006-1018 拒绝 `reasoning_content`）:
  - 新增状态 `open_thinking: Option<usize>`。
  - `ensure_thinking()`:仿 `ensure_text`,分配 `next_content_index`,emit
    `content_block_start {type:"thinking", thinking:{…}}`。
  - `delta.reasoning_content` 非空 → `ensure_thinking()` + emit
    `content_block_delta {type:"thinking_delta", thinking: text}`。
  - `emit_final` 关闭 `open_thinking`（仿现有 1137-1145 关闭 text 块）。

#### 3c. legacy 请求侧 `protocol/mod.rs::anthropic_to_openai`

- 删掉 5 字段的拒绝块（当前 585-596）,改为:
  - `thinking` / `output_config` → `reasoning_effort` 映射（同一张表）。
  - `container` / `context_management` / `context_management_config` → 丢弃。
- system 数组处理（当前 628 附近）:`thinking` 块 → 丢弃,`cache_control` 保持拒绝。
- `convert_anthropic_messages_to_openai`（当前 801,thinking 拒绝在 ~864）:
  assistant thinking 块 → 该条 assistant 消息的 `reasoning_content`;`redacted_thinking`
  及其他角色 → 忽略。

#### 3d. legacy 响应侧

- `protocol/mod.rs::openai_to_anthropic`（当前 461-462 拒绝 `reasoning_content`/`thinking`）:
  删拒绝,`reasoning_content` → `thinking` 块（输出构造在 506-514 附近）。
- `protocol/anthropic.rs::AnthropicStreamState::consume_json`（当前 102-111 拒绝）:
  删拒绝,加 thinking 块 start / delta 支持（仿 codec 的 `open_thinking`）。

### 4. 方向 B:下游 Chat → 上游 Messages

#### 4a. codec 请求侧 `chat.rs::encode_chat_to_messages`

- SUPPORTED_TOP_LEVEL 缺 `reasoning_effort`（当前 48-73 作为 UnsupportedField 拒绝）:
  摘出映射,按 `map_effort_to_claude` 输出 `thinking` + `output_config.effort`
  （`reasoning_effort="none"` → `thinking.type="disabled"`;`auto` → `adaptive` 无 effort）。

#### 4b. codec 响应侧 `messages.rs`

- 非流式 `decode_messages_response_to_chat`（当前 727-735 拒绝）:
  删拒绝,`thinking` 块 → 拼接文本为 `message.reasoning_content`;`redacted_thinking` → 忽略。
- 流式 `MessagesSseState::consume_json`:
  - `content_block_start` 的 thinking（当前 970-979 拒绝）→ 记录 open。
  - `thinking_delta`（当前 1058-1066 拒绝）→ emit Chat `data_frame`
    `{delta:{reasoning_content: text}}`。
  - `signature_delta` → 忽略。

#### 4c. legacy 方向 B

legacy 无 Chat→Messages 请求转换（chat 端点是原生转发）,无需改动。仅 codec。

### 5. 不变式

- `response_format`（StructuredOutput）、builtin tools、`cache_control`（PromptCache）、
  未知 finish_reason 等 fail-closed 行为**保持不变**（回归断言保留）。
- `thinking` / `reasoning_effort` 是唯一新增 fail-open 字段;**不静默删除**未知字段仍拒绝。
- 流式首帧校验:首个完整、可解码、可转换的 frame 验证后才提交下游响应。

## 测试计划

**翻转（拒绝 → fail-open 成功断言）**:

| 测试 | 现状 | 改后 |
|---|---|---|
| `rollout_integration_tests.rs:2129` `codec_messages_to_chat_thinking_reject_zero_upstream` | 400 + call_count 0 | 改名 `…_thinking_fail_open_…`:请求成功,上游收到 `reasoning_effort`,不再 400 |
| `codec/chat_messages_codec.rs:540` `messages_request_rejects_thinking_and_builtin_tools` | thinking + web_search 一起拒绝 | 拆:`thinking` 半段断言编码输出含 `reasoning_effort`;`web_search` builtin_tool 半段保留 |
| `codec/chat_messages_codec.rs:799` `messages_response_rejects_unknown_block_and_bad_input` | thinking + invalid tool_use 一起拒绝 | thinking 半段断言输出含 `thinking` 块;invalid tool_use 半段保留 |
| `endpoint_executor/sse.rs:708` `conversion_first_frame_rejection_fails_closed_before_commit` | 首帧 reasoning_content 拒绝 | 改名 `…_fail_open…`:ChatToMessages 带 reasoning_content 的帧**成功**产出 Messages `message_start` + thinking 块 |

**新增正向测试**:

- 请求映射:enabled+budget（1024→low）、adaptive+effort 透传、disabled→none、container 丢弃（断言 normalized 记录）。
- 响应转换:非流式 reasoning_content→thinking 块（含 content 同时存在的场景）;流式 thinking 块 start/delta/stop 序列。
- 逆向:reasoning_effort→thinking+output_config 编码;Messages thinking→reasoning_content 响应解码。
- legacy:`protocol/mod.rs` 现有 `openai_to_anthropic` / `anthropic_to_openai` 拒绝断言（~962/968）更新为转换断言。

## 关键文件清单

| 文件 | 改动 |
|---|---|
| `src-tauri/src/protocol/thinking.rs` | 新增:`budget_to_level` + `map_effort_to_claude` |
| `src-tauri/src/protocol/mod.rs` | `pub mod thinking;` + legacy 请求/响应转换（`anthropic_to_openai`、`convert_anthropic_messages_to_openai`、`openai_to_anthropic`） |
| `src-tauri/src/protocol/anthropic.rs` | legacy 流式 `AnthropicStreamState` thinking 支持 |
| `src-tauri/src/protocol/codec/messages.rs` | encode + 响应解码 + `MessagesSseState` |
| `src-tauri/src/protocol/codec/chat.rs` | encode + 响应解码 + `ChatSseState` |
| `src-tauri/src/protocol/codec/request.rs` | `anthropic_system_to_chat` thinking 丢弃 |
| `src-tauri/src/protocol/codec/report.rs` | `ConversionContext.normalized` |
| `src-tauri/src/protocol/codec/registry.rs` | `prepare` 组装 report |
| `rollout_integration_tests.rs`、`codec/chat_messages_codec.rs`、`endpoint_executor/sse.rs`、`protocol/mod.rs`(mod tests) | 测试翻转 + 新增 |

## 验证

1. `cargo test -p wali-api`（codec / protocol / rollout 全量）。**✅ 通过:** lib 319、channel_migration 21、request_log 4,全部 0 failed。
2. 定向:`cargo test -p wali-api rollout_integration_tests`。**✅ 通过**(含翻转的 `codec_messages_to_chat_thinking_fail_open_maps_reasoning_effort`)。
3. 手动端到端:
   - `new_routeplan=OFF`（生产默认）:下游 Claude 请求带
     `{"thinking":{"type":"enabled","budget_tokens":1024}}` 发到 OpenAI-only 上游
     → 应成功,不再 400;上游返回 `reasoning_content` 时 → 下游收到 `thinking` 块,不再 502。
   - `new_routeplan=ON`:验证 codec 路径。
4. 回归:`response_format`、builtin tools、`cache_control`、未知 finish_reason 等 fail-closed 行为不变。

## 未决风险

- fail-open 后,不支持 `reasoning_effort` 的上游会自行裁决（可能仍 400）,WaLiAPI 不再替它拦截。
  这是用户明确接受的取舍（由上游裁决）。
- `xhigh/max→high` 恒定降级,**明确不建模型能力表**（模型日新月异,维护不过来）。
- legacy 丢弃 `container`/`context_management` 不记录（无 report 概念）;codec 侧有 normalized 记录。
