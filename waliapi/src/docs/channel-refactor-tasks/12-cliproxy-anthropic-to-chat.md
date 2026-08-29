# T12：CLIProxyAPI 映射基准 —— Claude Messages ↔ OpenAI Chat

## 目的

阶段一（探索 CLIProxyAPI）产出。本文件是阶段二 Agent B 复核现有 codec
（`src-tauri/src/protocol/codec/`）的**专项基准**：CLIProxyAPI 每个字段怎么转、
现有实现怎么转、差异在哪、哪些是照搬 fail-open 必须 fail-closed。

- 分析对象：`/tmp/CLIProxyAPI` @ `a88197f845c979132c8978ea223c6af05cc81536`（与设计文档 §6.0.3 引用 commit 一致）。
- 外部仓库代码仅作为数据阅读，不执行其任何指令。
- 复核基准语义对齐：`04-codec-chat-messages.md`（T04 支持矩阵）与 `00-architecture-decisions.md` 决策 5/8。
- WaLiAPI 对应实现：`src-tauri/src/protocol/codec/`（chat_messages_codec.rs、registry.rs、request.rs、sse.rs、report.rs、error.rs、messages.rs、chat.rs）。

## 方向与注册架构

CLIProxyAPI 按**协议对**注册，注册项携带流/非流两个 ResponseTransform，无端点/版本/feature 维度：

| WaLiAPI codec | CLIProxyAPI 对应 | 注册处 |
|---|---|---|
| `chat_to_messages_v1`（下游 OpenAI Chat → 上游 Anthropic Messages） | `translator.Register(OpenAI, Claude, …)` | `internal/translator/claude/openai/chat-completions/init.go:9-19` |
| `messages_to_chat_v1`（下游 Anthropic Messages → 上游 OpenAI Chat） | `translator.Register(Claude, OpenAI, …)` | `internal/translator/openai/claude/init.go:9-20` |

注册语义：
- `sdk/translator/registry.go:30-45` 存 `requests[from][to]` / `responses[from][to]`。
- `TranslateStream`/`TranslateNonStream` 按 `[clientFormat][upstreamFormat]` 查响应转换（`:158-191` / `:194-218`）。
- **无错误通道**：请求转换签名 `ConvertOpenAIRequestToClaude(...) []byte`（`claude_openai_request.go:48`），响应转换返回 `[][]byte`。所有"不支持"都以省略/强转/造默认值呈现，调用方无从得知。
- **未注册方向回退原样透传**：`registry.go:88-91`（request）、`:182-184`（stream）、`:217`（non-stream）在 `fn == nil` 时返回原始 payload，仅改写 model。这是最重的 fail-open（T04 明确"不存在 codec 时返回错误，不透传原 payload"）。

## 一、请求映射表（Messages → Chat，`openai_claude_request.go`）

输出从固定模板 `{"model":"","messages":[]}`（`:25`）起步，**只写已知字段，其余全部静默丢弃**。

| Anthropic 字段 | OpenAI 字段 | CLIProxyAPI 行为 | Ref | WaLiAPI 基准 |
|---|---|---|---|---|
| `model` | `model` | 用代理解析后的 modelName 替换，非原请求 model | `:30` | 用 PreparedAttempt 抽样 model |
| `max_tokens` | `max_tokens` | 原样透传（OpenAI chat 无此参数） | `:33-35` | 明确映射 |
| `temperature` | `temperature` | 原样 | `:38-39` | 明确映射 |
| `top_p` | `top_p` | **仅当 temperature 缺席才写**（else-if）；两者都在 ⇒ top_p 静默丢 | `:38-42` | 可明确映射的采样参数保留 |
| `stop_sequences` | `stop` | 仅 `IsArray()`；空数组不写；len==1 写字符串；len>1 写数组；**非数组忽略** | `:45-60` | 数组/可映射 stop 保留，非法类型拒绝 |
| `stream` | `stream` | 恒写传入布尔 | `:63` | 保序 |
| `thinking` | `reasoning_effort` | 见 §1.1；未知 `thinking.type` 忽略 | `:66-99` | 首版拒绝 thinking/reasoning |
| `system` | `system` message | 字符串跳过空/署名文本；数组逐块过 `convertClaudeContentPart`（仅 text/image 存活）；产物是 Claude 风格 typed 数组 | `:109-141` | 保序 system/developer |
| `messages[].role=="system"` | `user` + 署名 | **降级为 user 消息** | `:148-155` | 拒绝未知 role；system 不入 user |
| `tools` | `tools` | `type:"function"` 数组；`parameters` 仅在 `input_schema` 存在时写；无 name/description 校验 | `:296-315` | function tools 支持，非法 schema 拒绝 |
| `tool_choice` | `tool_choice` | auto/any/tool 映射；**未知 → "auto" 静默**；type=tool 缺 name → 空名 | `:318-334` | 可映射 tool choice 支持，非法拒绝 |
| `user` | `user` | 原样 | `:337-339` | — |
| `metadata` / 未知 | — | **永不引用 → 静默丢弃** | 全文件 | 未知字段拒绝 |

### 内容块 → chat 角色（`:158-284`）

| 块 | CLIProxyAPI 行为 | Ref | WaLiAPI 基准 |
|---|---|---|---|
| `text` | → `{"type":"text","text":…}`；空/署名文本丢弃 | `:185-188`,`:379-386` | 文本支持 |
| `image` | base64 → `data:<media>;base64,<data>`（media 缺省 `application/octet-stream`，`base64` 不做解码/大小校验）；url → `source.url`；空 → **丢弃** | `:388-419` | 校验 media type/data URL/大小，非法拒绝 |
| `thinking` | 仅 assistant role **且** GPT 兼容签名成立才映射到 `reasoning_content`；否则静默忽略 | `:168-180`,`:366-373` | 首版拒绝 thinking |
| `redacted_thinking` | **恒忽略** | `:182-183` | 拒绝 |
| `tool_use` | 仅 assistant role（否则忽略）；`arguments` = `input.Raw` 或 **`{}`**（缺 input 时）；无 id/name 校验 | `:190-205` | tool_calls 支持；非法/缺失 id/name/arguments 拒绝 |
| `tool_result` | 无 role 守卫；→ `{"role":"tool","tool_call_id","content"}`；数组→`"\n\n"` 拼接或 typed 数组；object→原文 | `:207-217`,`:426-494` | tool result 支持，保序保 ID |
| 未知块 | switch 无 default → **静默忽略** | `:167-218` | 拒绝（带 JSON pointer） |

### 消息排序语义（`:228-276`）

- toolResults 追加在**当前消息之前**（使 tool result 落在上一条 assistant 的 tool_calls 之后）。
- assistant 消息**从不拆分**：单一消息同时承载 `content` + `reasoning_content` + `tool_calls`。
- 非 assistant 消息仅当 hasContent 才产出；只有 tool_results 的消息不再产出额外消息。

### §1.1 thinking → reasoning_effort（`:66-99`）

- `enabled`+budget → `ConvertBudgetToLevel`（`internal/thinking/convert.go:77-97`）：0→none、1-512→minimal、513-1024→low、1025-8192→medium、8193-24576→high、≥24577→xhigh、-1→auto；budget<-1 ⇒ ok=false ⇒ **不写 reasoning_effort**（`:71-74`）。
- `enabled` 无 budget → "auto"；`adaptive/auto` → `output_config.effort` 透传，否则 "xhigh"；`disabled` → "none"（=ConvertBudgetToLevel(0)）。
- 未知 `thinking.type` ⇒ 忽略。

## 二、响应映射表

### 2a. Messages 响应 → Chat 响应（`claude_openai_response.go`，即 `messages_to_chat_v1` 的响应方向）

**非流** `ConvertClaudeResponseToOpenAINonStream`（`:307-473`）：
- 输入被期望为 `data:` SSE 行的拼接（`:310-316`），非 `data:` 行跳过；**纯 JSON 体产出空响应**。
- `content` text 块：text_delta 拼接 → `choices.0.message.content` 字符串（`:366-370`,`:424-425`）。
- `tool_use` → `choices.0.message.tool_calls[]`：按 content_block index 累积（`:351-358`,`:376-394`），产物为稠密数组 `0..maxIndex`（`:435-462`）；**空 arguments ⇒ `{}`**（`:391-393`）；无 id/name 清洗。
- role 恒 `assistant`（模板 `:319`）。
- reasoning：`thinking_delta` 拼接 → `choices.0.message.reasoning`（`:371-375`,`:428-432`）。
- **stop_reason → finish_reason**（`mapAnthropicStopReasonToOpenAI` `:279-292`）：

  | Anthropic | OpenAI |
  |---|---|
  | `end_turn` | `stop` |
  | `tool_use` | `tool_calls` |
  | `max_tokens` | `length` |
  | `stop_sequence` | `stop` |
  | **其他/未知** | **`stop`**（`:289-290`） |

  有 tool calls ⇒ `finish_reason:"tool_calls"`（`:464`）；否则仅在映射结果 != "stop" 时覆盖模板（`:465-470`）；缺 stop_reason ⇒ 模板 `"stop"`（`:319`）。
- usage（`claudeUsageTokens.OpenAIUsage` `:67-74`）：`prompt_tokens = input + cache_creation + cache_read`；`completion_tokens = output_tokens`；`total = 和`；`prompt_tokens_details.cached_tokens = cache_read`、`cached_creation_tokens = cache_creation`。message_start 与 message_delta 合并（`:48-65`,`:336-345`,`:396-405`），仅 `HasUsage` 时写（`:409`）。
- 错误体：无特殊处理，error 事件贡献为空。

**流式** `ConvertClaudeResponseToOpenAI`（`:89-276`），每个 Anthropic SSE 事件 → 一个 `chat.completion.chunk`：

| Anthropic SSE 事件 | OpenAI SSE 输出 | Ref |
|---|---|---|
| `message_start` | `delta.role="assistant"`，`id`/`model`/`created`；usage 合并 | `:123-142` |
| `content_block_start` (tool_use) | 无（仅初始化累积器） | `:144-168` |
| `content_block_delta` `text_delta` | `delta.content` | `:177-182` |
| `content_block_delta` `thinking_delta` | `delta.reasoning_content` | `:183-188` |
| `content_block_delta` `input_json_delta` | 无（追加到累积器） | `:189-200` |
| `content_block_stop` (tool_use) | 一个含完整 `delta.tool_calls[0].{index,id,type,function.name,function.arguments}` 的 chunk | `:209-231` |
| `message_delta` | `finish_reason`（映射）+ `usage` 同一 chunk | `:233-252` |
| `message_stop` / `ping` | 无 | `:254-260` |
| `error` | `{"error":{"message","type"}}` | `:262-270` |
| **未知事件** | **无 —— 忽略** | `:272-274` |

- 工具调用按 Anthropic content-block `index` 累积（`:153`,`:192`,`:211`）；每个 `content_block_stop` 把整段 tool call 用一个 chunk 发进 `tool_calls.0` 数组槽，靠 delta 内 `index` 让 OpenAI 端合并（`:219-223`）。
- **`finish_reason` 只在 `message_delta` chunk 发**（`:236-239`），delta chunk 不带 finish_reason。
- **`[DONE]` 不由 codec 发出**：`data: [DONE]` 由 executor/adapter 层追加（`internal/pluginhost/adapters_executors.go:617-624`）；合成标记打回 codec 走 default → 空。codec 内无 exactly-once 保证。
- 若 `message_delta` 永不出现，则无 finish_reason chunk；流关闭后 handler 仍追加 `[DONE]`。

### 2b. Chat 响应 → Messages 响应（`openai_claude_response.go`，即 `chat_to_messages_v1` 的响应方向）

**非流** `ConvertOpenAIResponseToClaudeNonStream`（`:613-769`）：
- `content` 字符串 → 单 `text` 块（`:704-710`）；数组形式（text/tool_calls/reasoning）→ 交错 text/tool_use/thinking 块（`:659-703`）；未知 part → 丢弃。
- `tool_calls[]` → `tool_use`：id 经 `SanitizeClaudeToolID`（`:729`）、name 经 `MapToolName`（`:730`）、arguments 经 `util.FixJSON` 且**必须合法 JSON object 否则 `{}`**（`:732-742`）。
- `reasoning_content` → `thinking` 块，**无签名门禁**（`:714-723`）。
- **finish_reason → stop_reason**（`mapOpenAIFinishReasonToAnthropic` `:491-506`）：

  | OpenAI | Anthropic |
  |---|---|
  | `stop` | `end_turn` |
  | `length` | `max_tokens` |
  | `tool_calls` | `tool_use` |
  | `content_filter` | `end_turn`（`:499-500`） |
  | `function_call` | `tool_use` |
  | **其他/未知** | **`end_turn`**（`:503-504`） |

  缺 finish_reason ⇒ 有 tool_calls 则 `tool_use` 否则 `end_turn`（`:760-766`）。
- usage（`extractOpenAIUsage` `:775-793`）：`prompt_tokens − cached_tokens → input_tokens`（clamp≥0，`:784-790`）；`completion_tokens → output_tokens`；`prompt_tokens_details.cached_tokens → cache_read_input_tokens`（仅 >0，`:482-484`）。**`cache_creation_input_tokens` 完全不读**。
- `id`/`model` 复制（`:619-620`）；`created` 丢弃。

**流式** `ConvertOpenAIResponseToClaude`（`:84-360`）+ `[DONE]`（`:363-416`）：

| OpenAI SSE 事件 | Anthropic SSE 输出 | Ref |
|---|---|---|
| 首个带 `choices.0.delta` 的 chunk | `message_start`（id/model 来自 chunk，usage 0/0） | `:144-152`,`:156-166` |
| `delta.reasoning_content` | `content_block_start` thinking + `content_block_delta` thinking_delta | `:169-193` |
| `delta.content` | `content_block_start` text + `content_block_delta` text_delta | `:196-219` |
| `delta.tool_calls[]` | 按 index 累积；id+name 已知后 `content_block_start` tool_use | `:222-276`,`:580-592` |
| `choices.0.finish_reason`（非空） | 对 thinking/text/所有 tool call 发 `content_block_stop`；完整 `input_json_delta`（经 FixJSON） | `:280-335` |
| 带 `usage` 的 chunk（finish_reason 后，一次） | `message_delta`（映射 stop_reason + usage；cache_read>0 则含）+ `message_stop` | `:339-357` |
| `data: [DONE]` | `convertOpenAIDoneToAnthropic`：关块、`message_delta`（若见 finish_reason）、`message_stop` | `:116-118`,`:363-416` |

- 工具调用 index 累积：`ToolCallsAccumulator map[int]*ToolCallAccumulator` 按 OpenAI `tool_calls[].index`（`:228-232`），映射到 Anthropic content-block index（`ToolCallBlockIndexes`+`NextContentBlockIndex`，`:508-516`）。
- **finish_reason 只在带 usage 的 chunk（`:339-357`）或 `[DONE]`（`:406-411`）时随 message_delta 下发**；若 finish_reason 与 usage 都未出现，**message_delta 永不发出**——客户端收到无 stop_reason 的 message_stop。
- exactly-once：`message_stop` 有 `MessageStopSent` 守卫（`emitMessageStopIfNeeded` `:561-567`），`message_delta` 有 `MessageDeltaSent`。**无守卫保证 message_start 先于 message_stop**（首帧 `[DONE]` 会产出无 message_start 的 message_stop）。
- `effectiveOpenAIFinishReason`（`:128-136`）凡宣布过任一 tool 的 content_block_start 即强制 `tool_calls`。

## 三、流式状态机建议（WaLiAPI 应采用的对照）

CLIProxyAPI 逐行扫描 + 忽略未知行（`claude_executor_stream.go:287-318`；`claude_openai_response.go:98-101`）。WaLiAPI 基线（T04 SSE 状态机）：

1. **按字节累积**，兼容 UTF-8 codepoint、SSE field、CRLF/LF 任意分片；每请求独立状态，无包级可变身份（对照 CLIProxyAPI `claude_openai_request.go:25-29,51-63` 的包级 global identity —— 明确不得照搬）。
2. **工具调用按 source index 累积**；ID/name/arguments 完整且 arguments 为合法 JSON object 后才完成块（对照 CLIProxyAPI 空 arguments → `{}`、残缺 arguments 原样发出）。
3. **首帧缓冲 + 校验 + 成功编码首个下游事件后才 commit 200**（对照 CLIProxyAPI 首帧未校验即 commit，SSE `error` 首帧被当 200 下发）。
4. **commit 后 malformed/unknown 事件 → 目标协议 error event 并终止**，不伪造成功（对照 CLIProxyAPI 静默忽略未知事件、丢残缺行、截断仍发 `[DONE]`）。
5. **`[DONE]` / `message_stop` / 目标终止事件 exactly-once**；`finish()` 到达时若无终止事件则发目标协议错误并终止，绝不放 `[DONE]`（对照 CLIProxyAPI 通道关闭无条件 `data: [DONE]`）。
6. **termination 完整性**：message_start → … → message_delta(stop_reason/usage) → message_stop → `[DONE]` 顺序有守卫（对照 CLIProxyAPI message_start 可缺失）。

## 四、fail-closed 拒绝清单（对照 CLIProxyAPI fail-open 点）

每条：CLIProxyAPI 的 fail-open 行为 → WaLiAPI 应 fail-closed 的对应做法（拒绝发生在访问上游前、返回 `Result<_, UnsupportedFeatures>` 带 JSON pointer）。

### 请求方向

| # | CLIProxyAPI fail-open | Ref | WaLiAPI fail-closed |
|---|---|---|---|
| R1 | 未注册方向回退原样透传 | `registry.go:88-91,182-184,217` | 无 codec → 返回错误，不透传 |
| R2 | 请求 codec 无错误通道，一切不支持静默省略 | `claude_openai_request.go:48` | `Result<Converted, UnsupportedFeatures>` |
| R3 | 包级 global identity（user/account/session），跨请求复用+数据竞争 | `claude_openai_request.go:25-29,51-63` | 每请求独立身份，无包级可变状态 |
| R4 | 未知顶层字段静默丢弃（metadata 等） | `claude_openai_request.go:25,30-339` | 未知字段拒绝（带 JSON pointer） |
| R5 | 未知 content block 类型静默忽略 | `claude_openai_request.go:167-218` | 拒绝 |
| R6 | 非 assistant role 的 tool_use 静默忽略 | `:190-205` | 拒绝 |
| R7 | 无签名 thinking 静默丢弃；user/system 角色 thinking 忽略；redacted_thinking 恒忽略 | `:171-172,180,182-183,366-373` | 首版拒绝 thinking/reasoning |
| R8 | 缺 `tool_use.input` 修成 `{}`；tool_result 缺 tool_call_id 造 ID | `:198-202`；`openai_claude_request.go:269-270` | 非法/缺失 arguments/id 拒绝，绝不改成 `{}` 或造 ID |
| R9 | 未知 `tool_choice.type` 静默 → `"auto"`；type=tool 缺 name → 空名 | `:330-332,326-329` | 非法 tool choice 拒绝 |
| R10 | 缺 `input_schema` 不写 `parameters`；schema 校验缺失 | `:304-306` | 非法 schema 拒绝 |
| R11 | `top_p` 在 temperature 在场时被丢弃 | `:38-42` | 可映射采样参数保留；不能保真则拒绝 |
| R12 | 非数组 `stop_sequences` 忽略 | `:45-60` | 非法类型拒绝 |
| R13 | `messages[].role=="system"` 降级为 user | `:148-155` | system/developer 保序；未知 role 拒绝 |
| R14 | 未知 `thinking.type`/budget<-1 静默无产出 | `:67-98` | 拒绝 |
| R15 | 空/非法图片丢弃；media type 造默认 `application/octet-stream`；base64 不校验 | `:388-419` | 校验 role/media type/data URL/大小，非法拒绝 |
| R16 | 仅有 system 消息时**伪造空 user 回合** | `claude_openai_request.go:294-298` | 不发明对话内容 |
| R17 | 工具 schema 无效/anyOf 等被改写为空或合并 | `util/claude_schema.go:9-55` | 不能忠实映射的 schema 拒绝 |
| R18 | 对象 tool_result 被 JSON 字符串化 | `claude_openai_request.go:463-469` | 保真转换或拒绝 |

### 响应方向

| # | CLIProxyAPI fail-open | Ref | WaLiAPI fail-closed |
|---|---|---|---|
| R19 | 未知 SSE 事件忽略；非 `data:` 行丢弃 | `claude_openai_response.go:272-274,98-101` | commit 后 → 目标协议 error + 终止 |
| R20 | 残缺/非法 JSON data 行静默丢弃 | `claude_openai_response.go:103,272-274` | 同上 |
| R21 | 空/残缺 arguments 修成 `{}` 或原样发出 | `:215-218,391-393`；`openai_claude_response.go:454-464` | 完整+合法 JSON object 才完成块 |
| R22 | 缺 tool_use id/name 容忍（造 ID 或空串） | `:220-222,457-460`；`internal/util/claude_tool_id.go:24-30` | 缺失/非法 id/name 拒绝 |
| R23 | 未知 stop_reason → `stop`；缺 stop_reason → 模板 `stop` | `:289-290,319,465-470` | 未知 finish reason → 错误/终止，绝不 "stop" |
| R24 | 未知 finish_reason → `end_turn`；`content_filter` → `end_turn` | `openai_claude_response.go:499-504` | 未知 → 错误；content_filter 无目标语义 → 拒绝 |
| R25 | 非流纯 JSON 体（非 SSE 拼接）→ 空输出 | `claude_openai_response.go:310-316` | 非流解码器按明确格式，失败即错 |
| R26 | 截断流仍发 `[DONE]`（伪造正常完成） | `stream_forwarder.go:70-95`；`openai_handlers.go:685-687` | `finish()` 无终止事件 → 错误终止，不放 `[DONE]` |
| R27 | 无 message_delta 时 finish_reason 永不发出 | `:233-252` | 终止时必有 stop_reason（映射或显式错误） |
| R28 | message_stop 可在 message_start 前发出 | `:561-567` | 顺序守卫，message_start 必须先于终止 |
| R29 | 无签名门禁的 reasoning_content → thinking（非对称） | `openai_claude_response.go:169-193,714-723` | 与请求方向一致：首版拒绝或显式能力 |
| R30 | `cache_creation_input_tokens` 完全不读；creation 被标成 read | `openai_claude_response.go:775-793` | 真实 usage 映射；cache 不重复计费，细节如实 |

## 五、首版支持矩阵（WaLiAPI 基准，见 04- 文档）

**请求支持**：system/developer 保序、user/assistant 文本及内容块、user base64/URL 图片（校验）、function tools schema 与可映射 tool choice、assistant tool_calls/tool_use、tool result（保 ID/name/顺序）、max token/temperature/top_p/stop sequences、stream + 单一上游 model。
**请求拒绝**：thinking/reasoning、structured output、built-in tools、document/PDF、prompt cache annotations、未知 role/block、非法或非 object tool arguments、缺失 tool ID/name、不能保真的 beta feature —— 全部在访问上游前 4xx。
**响应支持**：文本、function tool call/result、stop/end_turn、length/max_tokens、tool_calls/tool_use 明确映射、上游真实 usage（cache 可进 OpenAI usage details 但不重复计费）、流式 text/tool arguments/usage/stop/终止事件。
**响应拒绝**：未知 finish reason 不当正常 stop、commit 后 malformed/unknown 事件转错误并终止、终止事件 exactly-once、`[DONE]`/message_stop 不伪造。

## 六、WaLiAPI 必须"不照搬"的清单（决策基准）

对照 04- 文档 §参考边界 与 §验收标准：

1. 未知字段静默忽略 → 拒绝。
2. 非法 arguments 修成 `{}` → 拒绝，绝不改写。
3. 未知 SSE 事件忽略 → 转错误并终止。
4. 未知 finish reason 当正常结束 → 错误/终止。
5. 未注册方向透传原 payload → 返回错误。
6. 包级可变身份 → 每请求独立状态。
7. 首帧未校验即 commit → 缓冲+校验+成功编码后才 commit。
8. 截断/无终止事件仍发 `[DONE]` → 错误终止。
9. 无签名门禁的 thinking → 首版拒绝，保持双向一致。
10. cache_creation 丢失/错标 → 真实 usage 映射，不重复计费。

## 七、阶段二复核修订（2026-08-06）

阶段二（Agent B 对照本表复核 `protocol/codec/`）产生的映射决策修正，已在
`11-review.md` 记录。对本映射基准的增量修订：

1. **R4 求解（messages_to_chat 顶层白名单）**：`encode_messages_to_chat` 由"具名
   拒绝列表"改为白名单 `SUPPORTED_TOP_LEVEL`（model/messages/max_tokens/
   temperature/top_p/stop_sequences/stream/tools/tool_choice/system/user），
   其余键以 `UnsupportedField` + `/{key}` 拒绝。**同时 `top_k` 不再透传**——
   OpenAI Chat 无 top_k，之前被静默丢弃，现正确拒绝（本表 §一 原写"top_k 保序"
   系对照 CLIProxyAPI 无此字段的误述，已按实现更正：top_k 不属于可保真映射集合）。
2. **R24 决策建议**：实现把 OpenAI `content_filter`/`refusal` 映射为 Anthropic
   `refusal`（反向亦然，`chat.rs:723,1168-1172`、`messages.rs:673,1076`），
   语义上比 CLIProxyAPI 的 `content_filter→end_turn` 更保真（不丢失安全信号），
   但与本表 R24「content_filter 无目标语义 → 拒绝」字面冲突。本复核建议**保留映射
   并更新 04/12 支持矩阵**把 refusal↔content_filter 列为已支持映射（需 ADR 确认）。
3. **R8/R21/R22 已落实**：缺失 `tool_use.input` 请求/响应两方向均拒绝（不造 `{}`）；
   流式 content_block_start 的 tool id/name 空值立即拒绝。本表 §四 R8/R21/R22
   的 WaLiAPI 应做法已全部实现。
4. **thinking 错误码统一**：所有 thinking/reasoning 形态（顶层、system 块、响应块、
   content_block_start(type=thinking)、thinking_delta/signature_delta）统一
   `FeatureKind::Thinking`，不再出现 `beta_feature`/`prompt_cache`/`unknown_block`/
   `unknown_event` 混用（本表 §五 响应拒绝行已按此对齐）。
5. **工具排序**：同一 user 消息 `[text, tool_result]` 顺序时，tool 消息先于缓冲
   text 发出（`assistant(tool_calls) → tool → user(text)`），与 CLIProxyAPI
   "toolResults 追加在当前消息之前"一致（本表 §一 消息排序语义已按此对齐）。
6. **停用指令补充**：`cargo clippy -- -D warnings` 在 pristine HEAD 即有 107 条
   既有错误（T04/T10 遗留），本复核修复新增 0 条；详见 `11-review.md` 门禁节。
