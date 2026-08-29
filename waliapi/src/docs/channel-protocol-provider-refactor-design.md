# 渠道配置重构设计：协议、供应商与端点能力

状态：设计评审稿

日期：2026-08-04
范围：渠道管理页、新建/编辑渠道、渠道持久化与 WaLiAPI 上游路由；不改变下游已公开的 `/v1/*` 网关地址。

## 1. 目标与结论

将当前“类型（实际同时表达厂商、请求格式和适配器）”拆为三个明确概念：

| 概念 | 例子 | 用途 |
| --- | --- | --- |
| 协议 `protocol` | `openai`、`anthropic`、`ollama` | 决定上游请求/响应格式、鉴权、测试方式和端点集合 |
| 渠道提供商 `provider` | `deepseek`、`qwen`、`ollama` | 决定默认 Base URL、模型建议、地区分组和厂商提示 |
| 原生端点 `native_endpoints` | `chat_completions`、`responses` | 描述该渠道上游真实提供的端点；用于路由前的能力过滤 |

新 UI 按原型提供 OpenAI、Anthropic、Ollama 三个等宽 Tab。协议切换后，提供商下拉框只列出该协议支持的供应商，并自动带入可编辑的默认 URL、端点与模型建议。**协议切换或供应商切换不得静默覆盖用户已经改过的 URL、模型、映射或密钥**；有未保存修改时必须二次确认。

此变更不能只改前端：当前 Rust 的 `channels.type` 既用于 `get_adaptor()`，又用于 Anthropic 原生直通判断；若仅按原型改表单，Anthropic 兼容的 DeepSeek/Qwen 等仍会被按 OpenAI 格式转码，功能会错配。

## 2. 非目标与产品边界

- 不承诺“模型静态列表永远最新”。模型和可用区域由厂商动态变化；默认列表只是创建时的建议，保存后以用户模型列表为准，并提供“从上游同步模型”能力。
- 不把 Google 原生 Gemini 格式列为第四个协议。Google 在本期通过其 OpenAI 兼容面接入；若未来需要原生 Gemini，应独立增加 `gemini` 协议，不能伪装为 OpenAI。
- 不把任意 OpenAI 兼容服务都错误称作“完全 OpenAI API”。界面与能力提示需要标注“兼容子集”；尤其工具调用、多模态、Responses 和 `count_tokens` 的支持不同。
- 每个协议均提供并默认选中“自定义配置”，以兼容私有网关和未内置厂商；它是提供商选择器顶部的固定卡片，不属于厂商预设列表。

## 3. UI/交互规格

### 3.1 表单顺序

1. 标题：新建渠道 / 编辑渠道；关闭按钮。
2. 协议：三段式、整行铺满的 OpenAI / Anthropic / Ollama Tab；使用 `role="tablist"`、键盘左右箭头可切换。
3. 名称与备注：桌面双列、窄屏单列。预设切换时，仅在名称仍为旧自动名称或为空时更新名称。
4. 渠道提供商：整行展开选择器，顶部固定展示整行的“⚙️ 自定义配置 / 手动配置协议与 Base URL”卡片；其后按“国际 / 国内 / 本地”分组。DeepSeek 必须归入“国内”。预设选项显示图标、厂商名、地域标签和“兼容”小标签。
5. 协议配置区：按当前协议展示标题、Base URL 帮助文案、端点、API Key。
6. 模型列表：默认模型以可删 Tag 预置；输入框可新增；新增“同步上游模型”按钮（不可用或失败时不覆盖已有列表）。
7. 保留原有模型映射、优先级、权重、超时和保存/取消区域，字段含义不变。

### 3.2 协议配置区

| 协议 | Base URL 说明 | 端点控件 | API Key |
| --- | --- | --- | --- |
| OpenAI | 不包含端点路径；通常以 `/v1` 或兼容服务根路径结束 | 多选：`/chat/completions` 与 `/responses`；两项至少选一项，可同时选 | 必填（Ollama 式兼容服务除外） |
| Anthropic | 以 `/v1` 结尾（main 分支约定，如 `https://api.anthropic.com/v1`）；不要以斜杠结尾 | 固定显示 `/messages`；如厂商支持，显示只读的 `/messages/count_tokens` 能力 | 必填 |
| Ollama | 本机或远程 Ollama 的主机与端口 | 固定 `/api/chat`；模型发现用 `/api/tags` | 可选，默认空；远程反向代理可填写 |

OpenAI 的两个端点是**同一渠道的两个能力开关，不是两个 URL 输入框，也不是二选一**。保存时为 `native_endpoints: ["chat_completions", "responses"]`。若只勾选 Chat，Responses 原生组不可选该渠道；反之亦然。旧渠道的 Responses→Chat 兼容债务不属于原生端点，只能记录在 `config.legacy_capabilities`。

### 3.3 切换规则

- 新建时：任一协议 Tab 默认选中“自定义配置”；不预填厂商 URL 或模型，用户手填 Base URL、模型与密钥。选择某个预设供应商后才带入该预设的 URL、端点和模型建议。
- 已编辑的字段有改动时：切换协议/供应商弹出“应用预设会重置 URL、端点和建议模型；API Key、备注、模型映射、优先级、权重和超时保留”的确认框。用户可选择“应用预设”或“仅切换标识，保留当前连接参数”。
- 编辑旧渠道：以迁移计算出的协议/提供商初始化；显示“来自旧配置”提示。未保存前不能写回新字段。
- 保存校验：名称非空；URL 为 `http(s)`；OpenAI 至少一个端点；Anthropic 不能手工删除 Messages；Ollama API Key 可为空；模型去空白、去重但保持输入顺序。

“自定义配置”不是第四种协议，而是每个协议都可用的提供商 `provider=custom`：OpenAI 自定义仍按 OpenAI Chat/Responses 规则，Anthropic 自定义仍按 Messages 规则，Ollama 自定义仍按 `/api/chat` 规则。它默认选中，确保用户可以配置任何私有网关或未内置的提供商；切换到厂商预设才属于显式套用模板。

### 3.4 点击“保存”时的连通性验证

保存不是立即写库。前端先完成本地校验，然后调用**不落库**的 `test_channel_draft`；该命令只在内存中使用当前表单的 URL、密钥、模型、协议和已选端点，返回每个端点的独立结果，绝不将密钥或完整请求体写入日志。

| 情况 | 流程与结果 |
| --- | --- |
| OpenAI 协议，勾选一个端点 | 必测该端点；成功后直接保存。失败、认证失败或超时则显示错误与“修改配置 / 仍然保存”。 |
| OpenAI 协议，Chat 与 Responses 都勾选 | 必须分别测试两项，结果逐项显示。若 Chat 成功、Responses 返回 404/不支持，明确提示“该上游不支持或未开通 Responses，建议取消勾选 `/responses` 后重试”；用户仍可选择“仍然保存并保留两个端点”。 |
| Anthropic / Ollama 协议 | 测试其实际 Messages 或 `/api/chat` 路径；Ollama 空 Key 合法。 |
| 未选模型或端点不可做无费用探测 | 显示“未验证”而不是伪报成功；建议先选择模型。用户可选择仍然保存。 |

端点可用性无法仅靠 `/models` 判定；对 Chat、Responses、Messages 和 `/api/chat` 的确认需发送最小有效推理请求，可能产生极少上游费用。按钮附近必须明示这一点。测试请求应设置最短合理超时、`stream: false`、最小输出上限，并以首个模型作为探测模型；模型/参数错误需与“端点不支持”和“网络不可达”分别展示。

“仍然保存”是明确的次级危险操作，不能因测试失败自动触发；一旦用户选择，保存原始表单值并在渠道卡片显示“连接未验证 / 最近测试失败”。这样既满足用户可强制保存，也不会让故障配置被误认为健康。

### 3.5 渠道浏览列表：协议与提供商双标签

渠道管理页的每条渠道卡片中，名称后固定显示两个标签，顺序不可交换：

```text
渠道名称  [协议]  [提供商]
```

第一个标签来自规范化的 `protocol`（`OpenAI`、`Anthropic`、`Ollama`）；第二个来自 `provider` 的展示名称（如 `DeepSeek`、`字节豆包（Coding Plan）`、`Ollama（本地）`），自定义配置统一显示 `自定义`。例如：

```text
DeepSeek-Claude  [Anthropic] [DeepSeek]
DeepSeek         [OpenAI]   [DeepSeek]
内部网关          [OpenAI]   [自定义]
```

标签不是旧 `type` 的直接展示：旧渠道先经 `resolve_channel_identity()` 推导协议与提供商后再显示；因此可避免目前 `type` 混合表达厂商/适配器导致 `Anthropic Claude`、`DeepSeek` 等含义混乱。名称下方保留 Base URL，现有状态、模型数、成功率、延迟、优先级、权重与操作按钮的位置不变。

## 4. 供应商预设与能力矩阵

### 4.1 预设范围

严格遵从原型的默认下拉内容；“国际/国内”是产品分组，不是服务器部署地域判断。

| 协议 | 国际 | 国内 | 本地 |
| --- | --- | --- | --- |
| OpenAI | OpenAI、Google | DeepSeek、通义千问、智谱 GLM、字节豆包、Moonshot AI | Ollama（本地） |
| Anthropic | Anthropic | DeepSeek、通义千问、智谱 GLM、字节豆包（Coding Plan） | Ollama（本地） |
| Ollama | 无 | 无 | Ollama（本地） |

每一个协议的选择器顶部均有默认选中的“自定义配置”卡片，不归入国际/国内/本地分组；上表只列厂商预设。选择器视觉布局参考原型：先显示自定义整行卡片，再显示地域标题及两列厂商卡片。

Ollama 已有 OpenAI 与 Anthropic 兼容层，因此在相应协议的供应商下拉中显示“Ollama（本地）”；在 Ollama Tab 中则显示其原生协议。三项是同一上游的三种接入格式，用户按需要创建对应渠道。本期**不新增或对外公开 WaLiAPI 的 `/api/chat` 等 Ollama 服务端点**；Ollama 仅是上游供应商。

### 4.2 内置预设（2026-08-04 复核）

`模型建议`只作为创建模板。实际可用模型必须以账户权限、地域和上游 `/models`/厂商列表为准。

| 协议 / 供应商 | 默认 Base URL | 路由端点 | 初始模型建议 | 备注 |
| --- | --- | --- | --- | --- |
| OpenAI / OpenAI | `https://api.openai.com/v1` | Chat、Responses | `gpt-5.2`、`gpt-5-mini`、`gpt-5-nano`、`gpt-4.1`、`gpt-4.1-mini` | 两端点均默认勾选；Responses 为新项目优先项 |
| OpenAI / Google | `https://generativelanguage.googleapis.com/v1beta/openai` | Chat | `gemini-3.6-flash`、`gemini-3.5-flash`、`gemini-3.5-flash-lite` | 仅使用官方 OpenAI 兼容面；不走现有原生 Gemini 转换器；预览模型不作为默认值 |
| OpenAI / DeepSeek | `https://api.deepseek.com` | Chat | `deepseek-v4-pro`、`deepseek-v4-flash` | `/v1` 是可接受别名；不再预置即将废弃的 `deepseek-chat`/`deepseek-reasoner` |
| OpenAI / 通义千问 | `https://dashscope.aliyuncs.com/compatible-mode/v1` | Chat、Responses | `qwen3.7-plus`、`qwen3.7-max`、`qwen3-coder-next` | 新工作空间可改用带 Workspace ID 的新域名；具体地域支持以账户模型列表为准 |
| OpenAI / 智谱 GLM | `https://open.bigmodel.cn/api/paas/v4` | Chat | `glm-4.7`、`glm-4.7-flash`、`glm-4.6v` | 保存前以官方控制台 API 示例为最终 URL 依据 |
| OpenAI / 字节豆包 | `https://ark.cn-beijing.volces.com/api/v3` | Chat | `doubao-seed-2-0-pro-260215`、`doubao-seed-2-0-lite-260215`、`doubao-seed-1-6` | 模型 ID 与区域/接入点绑定，支持同步而非写死 |
| OpenAI / Moonshot AI | `https://api.moonshot.ai/v1` | Chat | `kimi-k2.5`、`kimi-k2-thinking`、`kimi-k2-turbo-preview` | 模型目录更新频繁，首选同步结果 |
| OpenAI / Ollama（本地） | `http://localhost:11434/v1` | Chat、Responses | 空（从 `/v1/models` 获取） | API Key 可留空；仅接入已安装的本地/自管 Ollama |
| Anthropic / Anthropic | `https://api.anthropic.com/v1` | Messages、Count Tokens | `claude-opus-4-6`、`claude-sonnet-4-6`、`claude-haiku-4-5-20251001` | 原生透传；Base 自带 `/v1`（main 分支约定），端点模板只补 `/messages` |
| Anthropic / DeepSeek | `https://api.deepseek.com/anthropic/v1` | Messages | `deepseek-v4-pro`、`deepseek-v4-flash` | 官方明确提供 Anthropic 兼容面；`count_tokens` 不标为可用 |
| Anthropic / 通义千问 | `https://dashscope.aliyuncs.com/apps/anthropic/v1` | Messages | `qwen3.7-plus`、`qwen3-coder-next` | 只对文档明确列出的模型/地域开放；新域名由厂商控制台覆盖 |
| Anthropic / 智谱 GLM | `https://open.bigmodel.cn/api/anthropic/v1` | Messages | `glm-4.7`、`glm-4.7-flash` | 按已确认的智谱 Anthropic 根地址预填；不启用 `count_tokens`，除非厂商正式提供 |
| Anthropic / 字节豆包（Coding Plan） | `https://ark.cn-beijing.volces.com/api/coding/v1` | Messages | Coding Plan 当前开通模型 | 名称必须完整显示“字节豆包（Coding Plan）”；此为 Coding Plan 专用兼容网关，非通用 Ark v3 |
| Anthropic / Ollama（本地） | `http://localhost:11434/v1` | Messages | 空（从本地模型列表获取） | API Key 可留空；Ollama 不支持精确 `/v1/messages/count_tokens` |
| Ollama / Ollama（本地） | `http://localhost:11434` | `/api/chat` | 空（从 `/api/tags` 获取） | 默认无 API Key；远程部署时用户改 URL 与密钥 |

Moonshot AI 没有 Anthropic 协议，故**不在 Anthropic Tab 的提供商下拉中展示**，也不生成 Anthropic/Moonshot 预设。

### 4.3 资料依据与持续更新机制

- OpenAI Chat Completions 与 Responses 都是正式端点，官方推荐新项目优先使用 Responses：[Chat Completions API](https://platform.openai.com/docs/api-reference/chat)、[Quickstart](https://platform.openai.com/docs/quickstart/make-your-first-api-request)。
- DeepSeek 官方同时提供 OpenAI 与 Anthropic 格式；Anthropic 根为 `https://api.deepseek.com/anthropic`，并列出当前替代模型及旧模型弃用时间：[快速开始](https://api-docs.deepseek.com/guides/function_calling/)、[Anthropic 兼容说明](https://api-docs.deepseek.com/guides/anthropic_api)。
- 通义当前 OpenAI 兼容面支持 Responses，并推荐迁移到工作空间新域名：[Responses 参考](https://help.aliyun.com/en/model-studio/qwen-api-via-openai-responses)、[Anthropic 兼容工具接入](https://help.aliyun.com/en/model-studio/more-tools)。
- Ollama 的官方文档说明其 OpenAI 兼容 Chat/Responses 与 Anthropic Messages 兼容层；`count_tokens` 不受支持：[OpenAI compatibility](https://docs.ollama.com/api/openai-compatibility)、[Anthropic compatibility](https://docs.ollama.com/api/anthropic-compatibility)。
- Google 当前稳定模型以官方模型目录为准；2026-08-04 快照使用 `gemini-3.6-flash`、`gemini-3.5-flash`、`gemini-3.5-flash-lite`：[Gemini models](https://ai.google.dev/gemini-api/docs/models)、[OpenAI compatibility](https://ai.google.dev/gemini-api/docs/openai)。

### 4.4 提供商模板的存放与分发

提供商模板必须以 Rust 后端为唯一可信源，新增 `src-tauri/src/channel_presets.rs`（必要时拆分为同目录模块）。该 registry 负责维护协议、供应商、地域分组、展示名称、默认模型、规范 URL、旧版兼容 URL、完整 endpoint URL 模板、原生端点能力、鉴权方案、旧 `type` 映射和草稿测试策略。

新增只读 Tauri 命令 `get_channel_presets()`，由 `src/lib/api.ts` 调用，`ChannelForm` 通过返回结果渲染协议 Tab、供应商下拉与预设提示。前端的 `src/lib/constants.ts` 不再保存任何渠道/提供商/模型/URL 模板，只可保留纯展示辅助函数。这样请求路由、迁移、测试和 UI 共享同一份定义，避免当前 `src/lib/constants.ts` 与 `src-tauri/src/adaptor/mod.rs` 各复制一份而发生漂移。

发布包携带 `preset_revision`；渠道在保存时记录该 revision 仅供追溯，后续预设更新通过应用升级或签名的预设清单发布。绝不在桌面端静默联网下载、或自动覆盖用户已保存的 URL、密钥、模型与映射。

## 5. 数据模型、迁移与 API

### 5.1 数据库迁移（新增 015）

保持现有 `channels.type` 不删、不改语义，以保证旧二进制、导入文件和回滚可读；新增字段：

```sql
ALTER TABLE channels ADD COLUMN protocol TEXT;
ALTER TABLE channels ADD COLUMN provider TEXT;
ALTER TABLE channels ADD COLUMN native_base_url TEXT;
ALTER TABLE channels ADD COLUMN native_endpoints TEXT NOT NULL DEFAULT '[]';
ALTER TABLE channels ADD COLUMN preset_revision TEXT;
ALTER TABLE channels ADD COLUMN identity_revision INTEGER NOT NULL DEFAULT 0;
ALTER TABLE channels ADD COLUMN legacy_executor_override TEXT;
```

迁移程序必须事务内回填：

| 旧 `type` | 解析身份 | 执行器解析 | 原生 `native_endpoints` / 迁移规则 |
| --- | --- | --- | --- |
| `openai` | `openai/openai`（若历史 URL 明确不是官方地址，UI 标为 `openai/custom`） | 由 `protocol=openai` 派生 | `chat_completions`；同时写 `config.legacy_capabilities=["responses_via_chat_v1"]`，不能臆测原生 Responses |
| `deepseek` | `openai/deepseek` | 由 `protocol=openai` 派生 | `chat_completions` |
| `claude` | `anthropic/anthropic` | 由 `protocol=anthropic` 派生 | `messages`；仅当原始配置已明确支持才加 `count_tokens` |
| `gemini` | `openai/google`（供 UI 展示） | `legacy_executor_override=gemini_native` | 保留原 Google URL、query-key 鉴权与原生执行，直至用户显式转换 |
| `qwen`、`zhipu`、`moonshot`、`doubao` | `openai/同旧 type` | 由 `protocol=openai` 派生 | `chat_completions` |
| `ollama` | `ollama/ollama` | 由 `protocol=ollama` 派生 | `api_chat`；原 `base_url` 不变，精确剥除末尾 `/v1` 写入 `native_base_url` |
| `custom` 或未知 | `openai/custom` | 旧版 fallback adaptor | `chat_completions` + `config.legacy_capabilities=["responses_via_chat_v1"]`；保留原 URL，不猜具体厂商 |

旧 `type` 是历史适配器标识；新建/更新时由 `(protocol, provider)` 推导为兼容别名写入它，不能再把它作为运行时真相。**独立兼容性复核后，不能只靠一个 `base_url` 同时表达 UI 显示值、新协议实际地址和旧版拼接地址。**新记录须保存 `native_base_url`（新协议规范根）与 `base_url`（旧代码兼容根），并由 registry 以完整的 endpoint URL 模板生成请求。UI 显示/编辑 `native_base_url`；`base_url` 仅是迁移期兼容字段。

| 新协议 | 写入旧 `type` | 旧代码可用的 Base URL | 说明 |
| --- | --- | --- | --- |
| OpenAI（所有 provider，含 Google/Ollama） | `openai` | OpenAI 兼容根（例如 Ollama 为 `http://localhost:11434/v1`） | 避免旧 `gemini` 原生适配器错误处理 Google 的 OpenAI 兼容 URL |
| Anthropic（所有 provider，含 DeepSeek/智谱/豆包/Ollama） | `claude` | 每个预设经测试的“旧 Claude 适配器拼接根” | 不能直接保存规范根；旧代码只追加 `/messages`，缺失 `/v1` 或厂商路径时会请求错误 URL |
| Ollama 原生 | `openai` | `http://localhost:11434/v1` | `native_base_url` 保存 `http://localhost:11434`；新代码用原生 `/api/chat`，旧代码退化使用 Ollama OpenAI 兼容层 |

因此，`base_url` 是旧代码的可运行兼容 URL，`native_base_url` 是新协议规范根；registry 必须为每个 provider/协议保存并以 mock 上游测试这两种 URL 的最终拼接结果。以 Anthropic 官方为例，Base 自带 `/v1`（`https://api.anthropic.com/v1`），端点模板只补 `/messages`，最终请求为 `https://api.anthropic.com/v1/messages`。这一双写只服务过渡期，避免旧版本把原生 Ollama 根拼成 `/api/chat/chat/completions`。建议在下一主版本、完成数据迁移验证后才评估删除 `type`。

### 5.2 前后端 DTO

新增且向后兼容：

```ts
type ChannelProtocol = "openai" | "anthropic" | "ollama";
type ChannelProvider = "openai" | "google" | "deepseek" | "qwen" | "zhipu" |
  "doubao" | "doubao_coding_plan" | "moonshot" | "anthropic" | "ollama" | "custom";
type ChannelEndpoint = "chat_completions" | "responses" | "messages" |
  "count_tokens" | "embeddings" | "api_chat";

interface Channel {
  // 输出 DTO：服务端已通过 resolve_channel_identity 规范化
  protocol: ChannelProtocol;
  provider: ChannelProvider;
  native_base_url: string;
  native_endpoints: ChannelEndpoint[];
  identity_revision: number;
  legacy_executor_override?: "gemini_native";
  preset_revision?: string;
}
```

Create/Update 输入 DTO 的上述新增字段均为 `Option + serde(default)`：Create 缺省时走旧身份推断，Update 的 `None` 表示保留而非清空；显式空端点拒绝。服务端验证组合合法性，不能信任 UI；输出 `get_channels` 对尚未迁移的记录实时提供规范化值。导出文件升级到 `version: "2.0"`，同时写入 `protocol/provider/native_base_url/native_endpoints/identity_revision` 与原 `type/base_url`。导入同时接受 v1/v2，v1 走同一迁移函数。

另增 `test_channel_draft(input)`，返回：

```ts
interface DraftEndpointTestResult {
  endpoint: ChannelEndpoint;
  status: "passed" | "failed" | "skipped";
  category?: "network" | "timeout" | "authentication" | "endpoint_unsupported" |
    "model" | "request" | "protocol" | "unknown";
  message: string;       // 已脱敏
  latency_ms: number;
}
```

此命令不得创建/更新渠道、不得调用配额计数或持久化请求日志。既有 `test_channel(id)` 继续保留，用于列表页对已保存渠道的手动复测。

## 6. 后端路由与适配器设计

### 6.0 统一安全闸门与路由管线（强制）

任意会接收或转发模型内容的下游入口必须走同一顺序，包括当前 Chat、Legacy Completions、Responses、Messages、Count Tokens、Embeddings，以及未来启用的 Images/Audio；协议转换不是审计绕过路径。当前直接返回 501 且不读取或转发内容的占位接口可以保持早拒绝，但一旦启用就必须接入安全闸门：

```text
认证/状态/过期/配额
  → 解析与基础 schema 校验
  → API Key 的模型、渠道授权
  → security_gate(原始协议 JSON)
  → 对原始 JSON 脱敏，生成安全日志体
  → 按请求模型筛选渠道并解析模型映射
  → 请求特征分析
  → 构建分组 RoutePlan
  → 转换 + ConversionReport + 目标能力校验
  → 上游执行与组内重试/组间降级
  → 响应转换、响应审计、日志与配额
```

`security_gate` 的输入必须是**下游原始 JSON 全树**，而非已经转换的 Chat JSON。Responses 的内置工具、图片、文件、未知内容块等会在转换时被跳过或压缩，转换后审计无法证明原始请求已覆盖。内部结构与 T00 保持一致：

```text
RequestEnvelope {
  downstream_protocol, endpoint, original_json, safe_forward_headers,
  query, model, stream, trace_id
}

AuditedRequest {
  envelope, forward_json, sanitized_log_json, body_hash, body_len,
  audit_result, request_features
}
```

原始 JSON 只在请求生命周期内用于覆盖保证，不进入持久化日志。转换后无需对同一内容重复全量扫描，但任何转换器或版本化 execution profile 新增、改写的字符串必须做 delta scan。codec 先执行 feature validation；不能保真的字段返回 `UnsupportedFeatures` 并在上游访问前 4xx，成功时返回 `ConversionReport { normalized, codec_version }`。不存在“成功但 dropped 非空”的状态，也不可像兼容代理一样静默删除不支持字段。

`security_gate` 返回两份独立对象：`forward_json`（原始协议脱敏后的、可继续转换/直通的请求）与 `sanitized_log_json`。任何日志不得保存未经脱敏的请求体。HTTP 网关没有交互确认能力，`SecurityAction::Confirm` 必须 fail-closed（推荐 409/403 + `approval_required`），而不能继续向上游发送。

扫描器还须实行整个请求的累计字节、节点、深度与时限预算，使用 UTF-8 安全边界截断；超过预算按明确的高风险拒绝处理，不能逐字符串计量或伪报为“已安全扫描”。

### 6.0.1 原生优先、可降级的 RoutePlan

模型是第一层路由条件，协议亲和性是第二层，优先级/权重是第三层。对外提供一个 `authorize_and_plan()` facade，内部依次调用 `authorize_request`、`resolve_model_candidates` 与 `build_route_plan`：先确认请求模型获授权，再仅保留模型列表命中或 `model_mapping` 源名称命中的渠道；随后才按协议分组，并在每组内部应用优先级降序和权重随机。不同组不得混在同一个 retry budget。

因此，某协议的原生组若**没有该模型的候选**，不会阻塞下一转换组；但原生组存在同模型候选时，转换组中即使有更高的优先级或权重也不能抢占。模型映射须在每个 attempt 只解析一次，得到的上游模型同时用于实际请求、日志和统计。

| 下游请求 | G1：原生优先 | G2：明确允许的转换降级 | 本期不进入候选 |
| --- | --- | --- | --- |
| Chat `/v1/chat/completions` | OpenAI Chat（含以 OpenAI 兼容方式配置的 Ollama） | Anthropic Messages，仅当请求特征被 Chat→Messages codec 完整支持 | 原生 Ollama `/api/chat`，待其 codec 完成后再开启 |
| Responses `/v1/responses` | OpenAI 原生 Responses，保留原始请求并直发 `/responses` | 仅旧记录/显式能力 `responses_via_chat_v1` 的 Responses→Chat；保留当前兼容行为 | Anthropic；Responses→Anthropic 为二期 |
| Messages `/v1/messages` | Anthropic Messages（含 Anthropic 兼容方式配置的 Ollama） | OpenAI Chat，仅当 Messages→Chat codec 完整支持 | 原生 Ollama `/api/chat`，待后续 codec |
| Count Tokens | Anthropic `count_tokens` | 无 | 所有转换 |

组切换只在本组无合格候选、候选关闭/健康状态不可用、连接失败、超时、408/409/429/5xx/529，或明确 `endpoint_unsupported`（405/501；404 只有确认是端点不存在时）发生；400/401/403/422 等语义/权限错误不得跨协议掩盖。每组有独立重试预算，并设置总尝试上限；一旦已向下游写出 headers 或 body 字节，禁止再重试。日志必须记录 `route_group`、上游协议、codec 版本和失败类别。本期不以新增熔断器作为前置条件。

这正是“DeepSeek Anthropic 与 DeepSeek OpenAI 同时配置”时的行为：Claude 请求先在 Anthropic 组内按优先级/权重调度；该组无匹配或发生可降级故障后，才进入 OpenAI 转换组。OpenAI 组再高的优先级也不能抢占 G1。

### 6.0.2 Responses → Anthropic：二期 codec

本期不将 Responses → Anthropic 放入 G3：用户请求 Responses 时优先走上游原生 Responses，其次仅保留已经存在的 Responses → Chat 兼容路径。二期可参考 [CLIProxyAPI 的成对 translator registry](https://github.com/router-for-me/CLIProxyAPI/blob/a88197f845c979132c8978ea223c6af05cc81536/internal/translator/claude/openai/responses/init.go#L10-L17)，实现 `Responses ↔ Anthropic Messages` 的请求、非流响应与流响应三套 codec。

但不能照搬其静默降级：其实现对多项 Responses 工具/参数无损性不足，也会静默省略部分 built-in tool。WaLiAPI 的二期验收前提是逐字段 support matrix；未知或不支持字段返回 `UnsupportedFeatures`、直接 4xx、上游零调用；并遵循“首次响应字节下发前可切换上游、下发后不可重试”的流式约束。

### 6.0.3 本期优先：Chat Completions ↔ Anthropic Messages codec

已对照 CLIProxyAPI 的两个方向注册与流/非流拆分：[OpenAI Chat → Claude Messages](https://github.com/router-for-me/CLIProxyAPI/blob/a88197f845c979132c8978ea223c6af05cc81536/internal/translator/claude/openai/chat-completions/init.go#L9-L18)、[Claude Messages → OpenAI Chat](https://github.com/router-for-me/CLIProxyAPI/blob/a88197f845c979132c8978ea223c6af05cc81536/internal/translator/openai/claude/init.go#L9-L19)。本项目借鉴其“方向成对注册、请求与响应共享状态、流/非流独立状态机、工具调用按 index 累积”的结构，**不照搬**其对未知字段静默忽略、非法工具参数修复为 `{}`、未知 SSE 事件忽略等 fail-open 行为。

本期先实现/加强版本化 `chat_to_messages_v1` 与 `messages_to_chat_v1`，所有转换函数均返回 `Result<Converted, UnsupportedFeatures>`，而不是直接返回 JSON。首版支持集仅为：

- 保序的 system/developer（提升为 Anthropic 顶层 system）与 user/assistant 文本；
- 合法的 user 图片内容块；
- function tools、tool choice、assistant tool calls/tool use、tool results；
- `max_tokens`、`temperature`、`top_p`、stop sequences；
- 真实上游 usage、可映射的 stop reason；
- 非流与流式 SSE，包括工具调用增量和任意网络分块下的 UTF-8/SSE framing。

`thinking`/reasoning、结构化输出（`response_format`/JSON Schema）、OpenAI built-in tools、未知内容块、未知/不安全 finish reason、无效或非 object 的工具参数、无法映射的媒体/角色，一律在**访问上游前**以 `UnsupportedFeatures` 返回 4xx。不得修复或伪造工具参数；`content_filter`、refusal 与未知结束原因不得降级为正常 `stop/end_turn`。usage 缺失记录为 unknown；只有对外协议强制要求数值时才输出兼容 0，同时在日志标记 `usage_unknown=true`，绝不把 0 当作精确计量。

现有 `ClaudeAdaptor` 仅转换文本/system/max_tokens/temperature，非流响应固定 `finish_reason=stop`，流式却直接返回 Anthropic SSE 给期待 OpenAI SSE 的 Chat 路径，因此不能复用为 `chat_to_messages_v1`。现有 Messages → Chat codec 较严格，已处理 text/system/image/function tools/results 并拒绝部分不支持特征；应在保留其 fail-closed 语义的前提下重构为上述统一 codec。

首期必测：并行工具调用分块、缺失 tool id/name、无效 JSON 参数、图片角色/媒体校验、cache usage、content_filter/refusal、未知 SSE 事件、UTF-8/SSE 分片、`[DONE]` 与 EOF 的恰好一次终止。任一不支持字段均断言 mock 上游零调用。

### 6.1 核心原则

1. `protocol` 决定**上游格式**；`provider` 只选预设与厂商差异；请求路由不得再依据厂商名猜协议。
2. 选择渠道时同时过滤“模型映射命中 + 启用 + 下游请求所需能力”；优先级/权重只在同一个 RoutePlan 分组内生效。
3. 原生可透传的协议优先直通，避免无谓格式转换和丢失 Claude Code/Responses 的新字段。
4. 只有明确的、版本化 codec 兼容降级路径才允许跨协议转换；降级前验证工具调用、图片、思维链、缓存、结构化输出等能力，不支持时返回清晰的 4xx，而不是静默丢字段。

### 6.2 必须改动的现有实现

| 当前位置 | 当前风险 | 目标改动 |
| --- | --- | --- |
| `src/components/ChannelForm.tsx` | 类型下拉把厂商、协议混用 | 改为协议 Tab + 提供商全行选择器 + 动态配置区；保留模型映射和调度控件 |
| `src/lib/constants.ts` 与 `src-tauri/src/adaptor/mod.rs` | 预设重复且已经漂移 | 移至单一 registry，由 Tauri 命令返回给 UI |
| `channels` / DTO / repository | 没有协议和端点能力 | 按 5.1 增量迁移、读写新字段 |
| `get_adaptor(channel_type)` | `type` 同时承担厂商/协议职责，转换藏在 adaptor 内 | 改为由 `resolve_channel_identity()` 派生 `executor_kind`，再选择 `EndpointExecutor(protocol, endpoint)` + 版本化 `Codec(downstream_endpoint, upstream_endpoint)`；仅旧 Gemini 使用 override |
| `is_native_anthropic_channel()` | 仅 `type == "claude"`，会使 Anthropic/DeepSeek、通义等错误走转码 | 改为 `channel.protocol == "anthropic" && endpoint messages 已启用` |
| `handle_messages_count_tokens` | 只允许 `claude` | 只选择标有 `count_tokens` 的 Anthropic 渠道；无此能力返回现有 501 语义 |
| `handle_responses` | 无条件 Responses→Chat，且转换后才审计 | 原始 Responses 先经安全闸门；G1 原生 `/responses` 直通，G2 仅显式 `responses_via_chat_v1` 走 codec |
| `test_channel` | 一律请求 `/models`，对 Anthropic/Ollama 原生不可靠 | 保留已保存渠道的基础连通性检查；新增草稿测试逐一调用已选实际端点。草稿测试可能产生最小费用，但绝不能写入生产请求日志或配额 |
| `import_export.rs` | 通过 URL 猜旧 type | 复用统一 `infer_legacy_channel_identity()`；导出 v2，导入兼容 v1 |

### 6.3 下游公开接口兼容表

| 下游 WaLiAPI 接口 | G1：首选上游候选 | G2：允许降级 | 不可用时 |
| --- | --- | --- | --- |
| `/v1/chat/completions` | OpenAI + `chat_completions` | 完整支持请求特征的 Anthropic Messages codec；Ollama `/api/chat` 另期 | 503，说明没有支持 Chat 的渠道 |
| `/v1/responses` | OpenAI + 原生 `responses` | 仅 `responses_via_chat_v1` 的显式 Responses→Chat codec | 503，说明没有支持 Responses 的渠道 |
| `/v1/messages` | Anthropic + `messages` 语义 JSON 直通 | 完整支持请求特征的 OpenAI Chat codec；原生 Ollama 另期 | 503/4xx，禁止静默删字段 |
| `/v1/messages/count_tokens` | Anthropic + `count_tokens` | 无 | 501（保持当前准确计数语义） |
| 现有 `/v1/embeddings`、图像、音频等 | 保持现有逻辑 | 本期不把它们误路由给仅聊天协议渠道 | 现有错误语义 |
| Ollama 原生入站接口 | 本期不新增为 WaLiAPI 公共接口 | 无 | 不适用；Ollama 仅作为上游协议，正式开放其配置 UI 前必须完成 `/api/chat` executor 与下游 Chat 转换链 |

特别注意：界面中的“Ollama 协议”指上游原生 `/api/chat`。当前 WaLiAPI 对外提供的是 OpenAI/Anthropic 兼容网关；如果要开放 `/api/chat` 作为下游公共接口，应单独立项，不应把上游选择器的改动误认为 API 承诺。

## 7. 模型同步、映射与默认值

1. 新建预设填入上表建议模型，并加 `model_source: preset`（置于 `config` JSON）。用户手改后改为 `manual`。
2. “同步上游模型”只在上游支持安全枚举时显示：OpenAI 兼容 `/models`、Ollama `/api/tags`；Anthropic 不假设存在兼容模型列表，因此展示厂商建议 + 手工添加。
3. 同步结果采用预览 diff：新增、保留、将移除；默认只新增、不删除用户已配置模型或 `model_mapping` 引用的模型。
4. `model_mapping` JSON 格式、数组负载均衡语义、全局映射名建议维持原样。删除模型时继续提示并清理其作为源模型的映射；若模型作为映射目标，先提示风险而不是无提示删除。
5. 后端模型路由仍先按映射名匹配。端点能力只是第二层筛选，不能因同步模型失败造成已有模型不可路由。

## 8. 兼容性、数据安全与回滚

- **已有渠道：** 迁移只新增列、不改 `base_url`、`api_key`、`models`、`model_mapping`、`priority`、`weight`、`timeout_secs` 或 `status`。DeepSeek 当前 URL 的 `/v1` 与根 URL 都保留，不能批量替换用户 URL。
- **API Key 权限：** `allowed_channels` 保存渠道 ID，`allowed_models` 保存映射名；两者均无需数据迁移。任何新的路由过滤必须仍在 API Key 授权检查后执行。
- **导入导出：** 旧备份原样可导入；新备份包含双字段；未知 `provider` 降级为 `custom` 而不丢失 URL/模型/密钥。
- **日志与统计：** `channel_id`、`channel_name`、`model`、`upstream_model`、重试计数的语义不变。可新增 `protocol/provider/endpoint` 到日志 `config` 或新列，但不改既有报表查询。
- **密钥：** API Key 永不出现在预设 registry、前端常量或导出预览日志。编辑时“留空不修改”的现有语义必须保留；Ollama 空 Key 需要与“编辑不修改”区分为显式 `api_key_mode` 或后端补丁字段。
- **回滚：** 旧程序会忽略新增 SQLite 列且继续读取 `type`。发布前保留新字段推导回旧 `type` 的写入；回滚后新建的非旧组合只能按兼容别名工作，不能保证原生协议特性，发布说明需明确。

## 9. 验收标准与测试矩阵

### 9.1 UI

- 三个协议 Tab 等宽；每个 Tab 仅出现指定供应商，DeepSeek 标签为“国内”。
- 每个协议 Tab 的提供商选择器顶部都有默认选中的“自定义配置”；它保留当前协议语义，但不预填厂商 URL、模型或密钥。
- OpenAI/任意供应商可同时勾选 Chat Completions 和 Responses，提交 payload 两项都存在。
- Anthropic 选择 DeepSeek 后默认 `https://api.deepseek.com/anthropic/v1` 与 `/messages`；Ollama 默认 `http://localhost:11434` 与 `/api/chat`、空 Key 合法。
- OpenAI、Anthropic 两个 Tab 都显示 Ollama（本地）；Anthropic Tab 不显示 Moonshot AI，字节选项完整显示“字节豆包（Coding Plan）”，智谱默认 `https://open.bigmodel.cn/api/anthropic/v1`。
- 渠道列表每条名称后始终按 `[协议] [提供商]` 展示两个标签；自定义渠道显示 `[协议] [自定义]`，旧渠道通过身份解析显示而非直接使用历史 `type`。
- 切换预设不会未经确认覆盖用户 URL/模型/映射；编辑旧记录显示正确推断值。
- 键盘、窄屏、错误提示与保存中状态可用。

### 9.2 后端与回归

- 对 v1 数据库执行迁移后，所有渠道数、ID、状态、排序、密钥、模型、映射和超时逐字段一致。
- `/v1/messages` 能直通 `protocol=anthropic, provider=deepseek`，不再以 `type == claude` 为条件；`count_tokens` 仅命中明确有能力的渠道。
- `/v1/responses` 不会选择只启用 Chat 的渠道；OpenAI 双端点同时启用时两条路由都可选中同一渠道。
- 保存草稿时仅测试已勾选的端点；Chat 与 Responses 同时勾选时两项均必须有独立测试结果。Responses 未支持时，默认操作为回表单取消该勾选，用户可显式选择“仍然保存”。
- OpenAI Chat、Anthropic Messages、Responses 的非流和流式：测试授权、配额、优先级、权重、失败重试、模型映射、安全扫描、日志与 token 统计。
- 导入 v1、导入 v2、导出 v2 后再导入；未知预设、空 Ollama Key、URL 带/不带尾斜杠、URL 含 `/v1` 的组合全覆盖。
- 将新建的 OpenAI/Google、Anthropic/DeepSeek、Anthropic/Ollama、原生 Ollama 渠道交由上一版程序读取并实际调用，验证兼容 `type`、`base_url` 与 `native_base_url` 的降级行为。
- 现有 Rust 单测与前端 `pnpm build` 均通过；增加 registry、迁移、能力过滤、Anthropic 原生选择和序列化的单元测试。

## 10. 实施顺序

1. 建立共享 provider registry 和 Rust 端校验，先以只读 Tauri 命令供 UI 使用。
2. 添加 015 迁移、DTO、repository、导入导出 v2 与旧数据推断测试。
3. 先完成统一安全闸门、模型优先的 RoutePlan 与 `Chat Completions ↔ Messages` 严格 codec；通过流式、工具调用和零静默丢字段测试后，才将其作为 G2 降级路径。
4. 重构适配器与 dispatcher 的 protocol/endpoint 能力筛选；完成 Anthropic 原生直通泛化、原生 Responses G1 与显式 legacy G2，再接 UI。
5. 重写 `ChannelForm`，保留其模型映射和调度子区；完成 ChannelsPage 展示的协议/供应商徽标。
6. 接入模型同步预览、测试连接策略和全面回归测试。
7. 灰度发布：先迁移既有数据库、记录迁移摘要；出现上游异常时可关闭新建入口，但不得自动修改既有渠道参数。

## 11. 独立兼容性复核后的强制修订

以下要求覆盖本文之前任何冲突表述；未满足前不得实施或合并该重构。

### 11.1 身份、URL 与旧执行路径

- 新渠道只持久化 `protocol/provider/native_base_url/native_endpoints/identity_revision`；`resolve_channel_identity()` 由协议身份派生 `executor_kind`。不为所有新渠道再保存一份 `execution_mode`，避免两份运行时真相漂移；仅旧 Gemini 使用 `legacy_executor_override=gemini_native`。
- registry 对每个预设定义 `native_base_url`、新代码完整 endpoint 模板、`legacy_type` 与 `legacy_base_url`。禁止用通用 `trim_end_matches + "/messages"` 推断所有 Anthropic 厂商 URL；必须逐预设 mock 验证最终 URL。
- 迁移旧 `type=gemini` 时，保留原 `base_url`、鉴权和 `legacy_executor_override=gemini_native`；UI 可显示为“Google（旧原生配置）”，仅当用户确认应用 Google OpenAI 兼容预设后才移除 override。这避免把原生 Gemini `generateContent?key=` 错改为 Bearer Chat Completions。
- 迁移旧 `type=ollama` 时，原字段保持不变；仅从**精确末尾** `/v1` 派生 `native_base_url`。新原生执行器使用后者，旧代码继续用 `base_url` 的 OpenAI 兼容层，绝不产生 `/v1/api/chat`。

### 11.2 Responses 与协议转换不回归

- `native_endpoints` 只表达上游原生能力；codec 是网关能力，不作为普通渠道字段持久化。旧记录不能因 `type=openai` 自动获得原生 `responses`；仅在 `config.legacy_capabilities=["responses_via_chat_v1"]` 中逐行记录历史兼容债务。
- 旧记录维持 `responses_via_chat_v1`：现有 `/v1/responses` 先转 Chat、上游走 `/chat/completions`、再合成 Responses 的行为必须继续可用。新建渠道是否允许该降级由产品预设明确，不得静默发生。
- 原生 Responses 另设路径：保留原始请求体，选中 `native_endpoints` 含 `responses` 的渠道后直发 `/responses`；流式事件原样透传，不能复用 Chat codec。404/不支持只影响该候选及重试策略，不得把原先可用的旧渠道变为 503。
- 现有 Chat ↔ Anthropic 转换保留为显式、能力校验后的降级路径；不得因新增 `protocol` 过滤掉原本支持转换的渠道。

### 11.3 授权、回滚升级与输入 DTO

- 实现唯一 facade `authorize_and_plan(api_key, model, endpoint)`：内部先执行 `allowed_models` 语义，再过滤 `allowed_channels`，再做模型候选、协议分组、端点能力、优先级和权重排序。所有公开接口及流/非流路径必须共用它；空数组的“全允许/全拒绝”语义在实现前固定并测试。当前代码仅保存权限字段、尚未执行，重构不得继续放大该漏洞。
- 实现唯一的 `resolve_channel_identity(row)`，供 DTO、dispatcher、测试、导入全部使用。`protocol/provider` 为 NULL、`native_endpoints` 为空或 `identity_revision=0` 时，按原 `type/base_url/config` 推断，不能因 015 已跑过就认为新字段有效。
- 为兼容“升级 → 回滚旧版 INSERT/UPDATE → 再升级”，本期固定采用数据库失效触发器：旧字段 `type/base_url/config` 发生变更时，将新身份字段清空并把 `identity_revision` 置 0；新版更新在同一事务中先写旧字段、再以最后一条 UPDATE 重写完整新身份和当前 revision。旧版新增行使用列默认值 revision 0。resolver 必须实时推断 revision 0 的记录，不能等待 migration 再跑。
- 输入、存储、输出 DTO 分离：Create 的新增字段 `Option + serde(default)`，缺省走 legacy 推断；Update 的 `None` 必须表示保留，显式空端点拒绝；输出始终返回解析后的非空身份。旧前端仅发送 `type/base_url` 的 create/update 必须通过。

### 11.4 保真与可观测性

- 导入导出与编辑的逐字段 round-trip 是硬性契约：`status`、`priority`、`weight`、`timeout_secs`、`config` 未知键、URL、密钥、模型及数组 `model_mapping` 逐值保持。现有 `ChannelDto` 漏出 `timeout_secs`、v1 导入未保留 `status/timeout`，应与本重构一并修复。
- 每个尝试在调度层只解析一次数组模型映射目标，解析后的 `upstream_model` 同时传给适配器、日志和统计；重试是否重新抽样要显式定义。不得出现实际请求模型与日志模型不一致或 Anthropic 原生日志恒为空。
- 增加 migration、dispatcher、导入导出、权限、URL 拼接、流式协议转换的 mock-upstream 集成测试。现有 Rust 测试主要覆盖 codec，不能作为上述兼容承诺的证据。

## 12. 已确认的产品决策

1. Ollama 支持 OpenAI/Anthropic 兼容时，必须出现在相应协议 Tab 的供应商下拉；不新增 WaLiAPI 对外 Ollama 服务。
2. Anthropic 的字节预设名称固定为“字节豆包（Coding Plan）”。
3. 智谱 Anthropic 根地址固定为 `https://open.bigmodel.cn/api/anthropic`；Moonshot AI 不提供 Anthropic 预设。
4. 每次提交前必须验证当前草稿的已启用端点；测试失败时由用户显式决定修改配置或仍然保存。
5. OpenAI 至少勾选一个端点；两个端点同时勾选时分别验证，并对未支持的 Responses 给出取消该端点的明确建议。
6. 新配置要以兼容 `type`、`base_url` 与 `native_base_url` 供旧代码降级运行；旧配置进入新版本后自动推导为新字段，原始连接参数不变。

## 13. 执行任务索引

完整的阶段拆分、文件所有权、依赖、验收标准与交接物见 [渠道协议重构任务总索引](channel-refactor-tasks/README.md)。实现 Agent 必须先阅读 T00，再按索引依赖领取任务，不得绕过架构决策直接改生产路由。
