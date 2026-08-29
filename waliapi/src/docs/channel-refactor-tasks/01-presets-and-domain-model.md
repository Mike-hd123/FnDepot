# T01：提供商模板与领域类型

## 目标

建立后端唯一的提供商模板 registry 和共享领域类型，为迁移、路由、草稿测试与前端提供稳定、无重复的数据来源。本任务不接入生产请求路由。

## 依赖

- 必读 [T00 架构决策](00-architecture-decisions.md)。
- 不依赖数据库迁移，可先定义纯 Rust 类型和只读命令。

## 文件所有权

- 新增 `src-tauri/src/channel_presets.rs` 或 `src-tauri/src/channel_presets/`。
- 新增/调整 `src-tauri/src/core/channel_identity.rs` 中的纯领域枚举；若 T02 已创建则只扩展公开类型。
- 调整 `src-tauri/src/lib.rs`、`src-tauri/src/main.rs` 或 commands 注册所需的最小模块声明。
- 调整 `src-tauri/src/commands/channel.rs`，增加只读 `get_channel_presets` 命令。
- 调整 `src/types/index.ts`、`src/lib/api.ts`，只增加模板 DTO 与查询方法。
- 不修改 `ChannelForm.tsx`、`ChannelsPage.tsx`、migration、handlers、dispatcher、adaptors。

## 领域契约

定义序列化稳定的枚举：

- `ChannelProtocol`: `openai | anthropic | ollama`
- `ChannelProvider`: `openai | google | deepseek | qwen | zhipu | doubao | doubao_coding_plan | moonshot | anthropic | ollama | custom`
- `NativeEndpoint`: `chat_completions | responses | messages | count_tokens | embeddings | api_chat`
- `AuthScheme`: `bearer | x_api_key | query_key | optional_bearer`
- `RegionGroup`: `international | domestic | local`

定义 `ChannelPreset`，至少包含：稳定 ID、协议、提供商、显示名称、地区、描述、图标 key、默认规范 Base URL、旧版兼容 Base URL、原生端点、默认勾选端点、鉴权方案、模型建议、模型枚举策略、端点测试策略、preset revision。

`custom` 不是厂商模板卡片，但 `get_channel_presets()` 必须为每个协议返回一个固定、置顶、默认选中的 custom option。custom 不提供默认 URL、密钥或模型；协议决定其允许端点。

## 模板内容

OpenAI 协议必须包含：OpenAI、Google、DeepSeek、通义千问、智谱 GLM、Moonshot AI、字节豆包、Ollama（本地）、自定义。

Anthropic 协议必须包含：Anthropic、DeepSeek、通义千问、智谱 GLM、字节豆包（Coding Plan）、Ollama（本地）、自定义。不得包含 Moonshot。

Ollama 协议必须包含：Ollama（本地）、自定义。

DeepSeek 地区为国内。智谱 Anthropic 规范根为 `https://open.bigmodel.cn/api/anthropic`。字节 Anthropic 名称固定为“字节豆包（Coding Plan）”。Ollama OpenAI 根为 `http://localhost:11434/v1`，Anthropic/Ollama 原生规范根为 `http://localhost:11434`。

## URL 兼容 fixture 契约

registry 不能只保存一个 Base URL。每个模板 fixture 必须同时断言 `native_base_url`、新 executor 最终 URL、`legacy_type`、`legacy_base_url`、旧 adaptor 最终 URL。以下关键组合为硬性样例：

| 组合 | native base | 新代码最终推理 URL | legacy type/base | 旧代码最终推理 URL |
| --- | --- | --- | --- | --- |
| OpenAI / OpenAI | `https://api.openai.com/v1` | `https://api.openai.com/v1/chat/completions`、`https://api.openai.com/v1/responses` | `openai` / 同 native base | `https://api.openai.com/v1/chat/completions` |
| Anthropic / Anthropic | `https://api.anthropic.com/v1` | `https://api.anthropic.com/v1/messages` | `claude` / `https://api.anthropic.com/v1` | `https://api.anthropic.com/v1/messages` |
| Anthropic / DeepSeek | `https://api.deepseek.com/anthropic/v1` | `https://api.deepseek.com/anthropic/v1/messages` | `claude` / `https://api.deepseek.com/anthropic/v1` | 同左最终 URL |
| Anthropic / 智谱 | `https://open.bigmodel.cn/api/anthropic/v1` | `https://open.bigmodel.cn/api/anthropic/v1/messages` | `claude` / `https://open.bigmodel.cn/api/anthropic/v1` | 同左最终 URL |
| Anthropic / 豆包 Coding Plan | `https://ark.cn-beijing.volces.com/api/coding/v1` | `https://ark.cn-beijing.volces.com/api/coding/v1/messages` | `claude` / `https://ark.cn-beijing.volces.com/api/coding/v1` | 同左最终 URL |
| Anthropic / Ollama | `http://localhost:11434/v1` | `http://localhost:11434/v1/messages` | `claude` / `http://localhost:11434/v1` | 同左最终 URL |
| Ollama / Ollama | `http://localhost:11434` | `http://localhost:11434/api/chat` | `openai` / `http://localhost:11434/v1` | `http://localhost:11434/v1/chat/completions` |

通义 Anthropic 的最终路径也必须按其官方文档落入 fixture；若官方 endpoint 规则与通用 `/v1/messages` 不同，以带 `verified_at/source_url` 的模板规则为准。任何尚未核实最终 URL 的模板不得作为可选预设发布，只能由 custom 承载。

模型建议与 URL 必须附 revision，不直接覆盖已保存渠道。模板更新只影响新建或用户显式点击“应用预设”。

模型快照必须以主设计 4.2 的 2026-08-04 清单为初始基线，并在实现合并当天重新核对官方文档。registry 为每条建议记录 `verified_at` 和官方 `source_url`；能安全枚举模型的供应商优先在 UI 提供同步，静态建议只负责新建引导。不得使用未经官方确认的型号，也不得把 `latest`/preview alias 作为默认生产模型。

## 实施步骤

1. 定义枚举与 `ChannelPreset`/`ProtocolPresetGroup` DTO，确保 serde 和 TypeScript 字符串一致。
2. 实现纯函数 `all_channel_presets()` 与 `presets_for_protocol()`，结果排序为 custom、international、domestic、local。
3. 对每个模板明确规范 URL 和旧版兼容 URL；禁止运行时通过字符串猜厂商。
4. 定义声明式 endpoint URL 规则与 auth scheme，不允许模板携带任意 body rewrite 回调。
5. 实现 `get_channel_presets()` Tauri 命令并注册。
6. 在前端 API 中增加 `channelApi.getPresets()`；不在 constants 中复制 fallback 模板。
7. 删除或弃用后端 `channel_types()` 的模板职责，但在本任务不删除仍被生产代码使用的函数；标记迁移调用点供 T06 移除。
8. 为模板 ID 唯一性、组合合法性、分组顺序和关键 URL 编写单元测试。
9. 逐供应商复核官方 Base URL、端点、模型 ID 和弃用状态，提交一份带检查日期与官方来源的 snapshot fixture；模型变更只更新 registry revision，不修改已保存渠道。

## 验收标准

- 前端通过一个只读命令可取得所有协议及 custom option。
- 同一 `(protocol, provider)` 不重复；custom 在三个协议都存在且为默认。
- Anthropic 不包含 Moonshot；DeepSeek 在国内；豆包 Coding Plan 名称正确。
- 每个非 custom 模板至少有规范 URL、原生端点、鉴权方案和 revision。
- OpenAI 模板默认至少选一个 Chat/Responses 端点；custom OpenAI 初始选 Chat，允许 UI 后续双选。
- 模板测试不访问网络。
- 每个静态模型建议均能追溯到 `verified_at/source_url`；过期型号不会作为新建默认值。

## 测试命令

- `cargo test channel_presets --manifest-path src-tauri/Cargo.toml`
- `pnpm build`

## 交接输出

完成后记录 Rust 类型导出位置、Tauri 命令 payload 示例、preset revision、所有模板组合表，以及仍使用旧 `CHANNEL_TYPES/channel_types()` 的调用点。
