# 渠道提供商下拉框 + 无确认切换设计

状态：已实现

日期：2026-08-06

参考原型：`docs/channel-refactor-tasks/13-provider-dropdown-prototype.html`（已评审通过，本设计以其为最终视觉/交互基准）

前置设计：[渠道配置重构设计](../channel-protocol-provider-refactor-design.md)（T01–T10）

范围：渠道管理页新建/编辑渠道表单的提供商选择、切换语义、端点交互；后端渠道模板 registry（`channel_presets.rs`）的数据修正。不改变下游已公开的 `/v1/*` 网关地址。

## 1. 目标与结论

把当前“整行展开的 PresetCard 网格 + 切换弹确认框”改为**协议 Tab 内的提供商下拉框**：

- 顶部：OpenAI / Anthropic / Ollama 三个协议 Tab（**去掉 `ollama_native` 门控，常驻显示**）。
- 下拉框：选中后展开分组列表 —— **自定义配置（置顶）→ 国际 → 国内 → 本地**。每项 = SVG 品牌图标 + 名称，悬停展示描述。
- 切换提供商**不再弹确认框**；立即应用模板默认值。
- 每次切换只重置**连接参数（URL / 端点 / 模型建议）**；保留 **API Key、名称、模型映射、优先级/权重、超时**。
- OpenAI 端点自由勾选（可全选/选一个/不选），校验移到**点击保存时**。

## 2. 非目标与产品边界

- 不改后端路由/执行器；`ollama_native` 后端路由开关保持现状（默认 OFF）。本设计仅移除**前端 Tab 可见性**的门控。
  - ⚠️ 风险：Tab 常驻后，用户可在路由开关仍为 OFF 时创建 Ollama 渠道，运行时请求将 503，直至灰度开启 `features.ollama_native=true`（见 `docs/channel-refactor-tasks/10-release-rollout.md` 第 5 步）。本设计不处理该风险——它属于 T10 灰度节奏，但须在实现说明中记录。
- 不新增第四个协议（Google 原生 Gemini 不在本期）。
- 不改变模型映射、优先级/权重、超时的字段与语义。

## 3. 后端渠道模板改动（`src-tauri/src/channel_presets.rs`）

### 3.1 PRESET_REVISION

从 `"2026-08-04"` 升至 `"2026-08-06"`，使存量渠道在模板变更后触发“模板有更新”提示。

### 3.2 名称 / 描述 / 图标 / 端点

| 位置 | 现值 | 改后 |
| --- | --- | --- |
| openai_presets · Moonshot | `display_name: "Moonshot AI"` | `"Moonshot(Kimi)"` |
| openai_presets · Moonshot | `desc: "Moonshot Kimi OpenAI 兼容面。"` | `"Moonshot Kimi OpenAI 接口。"` |
| openai_presets · 豆包 | `display_name: "字节豆包"` | `"字节豆包 (Coding Plan)"` |
| openai_presets · 豆包 | `desc: "火山方舟豆包 OpenAI 兼容面（Ark v3）。"` | `"字节豆包官方 OpenAI 接口。"` |
| anthropic_presets · Anthropic | `icon_key: "anthropic"` | `"claudecode"`（Claude Code 橙色星标） |
| anthropic_presets · Anthropic | `desc: "Anthropic 官方 Messages API。"` | `"Anthropic Claude Code 官方 Messages API。"` |
| anthropic_presets · Anthropic | `native_endpoints: [Messages, CountTokens]` | `[Messages]`（移除 CountTokens） |
| anthropic_presets · 豆包 Coding Plan | `display_name: "字节豆包（Coding Plan）"` | `"字节豆包 (Coding Plan)"`（英文括号） |
| anthropic_presets · 豆包 Coding Plan | `desc: "火山方舟 Coding Plan 专用 Anthropic 兼容网关。"` | `"字节豆包官方 Anthropic 接口。"` |

### 3.3 描述文案统一

将所有协议剩余描述中的「兼容面 / 兼容层 / 兼容网关 / 兼容根地址」全部替换为「接口」。涉及 DeepSeek、阿里云百炼、智谱 GLM、Google Gemini、Ollama 等预设，逐条与原型 `PROVIDERS` 表对齐。

### 3.4 测试更新

- `doubao_coding_plan_display_name_fixed`：断言改 `"字节豆包 (Coding Plan)"`。
- `non_custom_presets_have_full_fields`：跟随模板字段变化。
- `per_protocol_membership_matches_spec`：跟随模板字段变化。
- `url_fixtures_match_spec_table`：Anthropic 原生端点断言去掉 CountTokens。
- 运行 `cargo test -p wali_api channel_presets`（或对应 test 路径）全绿。

## 4. 前端下拉框实现（`src/components/ChannelForm.tsx`）

完全照原型实现，替换现有 PresetCard 网格与确认流。

### 4.1 顶层协议 Tab：三个常驻

- 删除 `featureFlags` state、`settingsApi.getFeatureFlags()` 调用、`FeatureFlagsDto` import。
- `availableProtocols` 恒为 `["openai", "anthropic", "ollama"]`。
- Tab 外观保留现有三栏等宽样式（`grid-cols-3`）。

### 4.2 提供商下拉框

- 结构：一个触发器（显示当前 provider 图标 + 名称 + 下拉箭头）→ 点击展开下拉面板。
- 面板内分组：**自定义配置**（置顶固定一项）→ **国际** → **国内** → **本地**，按 `region` 分组。
- 每项：SVG 品牌图标（`CHANNEL_PROVIDER_ICONS`）+ `display_name`；悬停 `title` 显示 `desc`。
- 每项下拉面板项 = 原型 `PROVIDERS[protocol]` 的内容，数据来自后端 `get_channel_presets`。
- 切换协议时 provider 重置为该协议的自定义配置（与原型 `selectProtocol` 一致，不记忆上次选择）。

### 4.3 切换语义：无确认弹窗

- 删除 `pendingSwitch` state、`requestProviderSwitch`、`onConfirmApply`、`onConfirmKeep`、`hasConnectionValues` 判断分支。
- 删除 `ConfirmSwitchDialog` 的渲染与 import；删除 `ConfirmSwitchDialog.tsx` 文件。
- 选择某项 → 立即 `applyPreset(preset, true)`：
  - 重置：`native_base_url`、`native_endpoints`（`endpointsForPreset`）、`models`（`model_suggestions`）、`preset_revision`。
  - 保留：`api_key`、`name`（自动命名逻辑保留）、`model_mapping`、`priority`、`weight`、`timeout_secs`。
- `applyPreset` 现有实现已符合该语义，复用即可；仅移除确认分支。

### 4.4 OpenAI 端点自由勾选

- 删除 `isLastEndpoint` 锁定逻辑。
- 端点多选可全选/选一个/不选；`validate()` 已有「OpenAI 至少选一个端点」校验，保存时提示「请至少选择一个端点」。
- 端点视觉：三种协议统一为原型样式 —— `label.ep` + `label.ep.on`，`input[type=checkbox]` + 端点名 + 路径；固定端点（Anthropic `messages`、Ollama `api_chat`）用 checked-disabled checkbox。CSS 以原型 `.ep` / `.ep.on` / `.ep-note`(删除) 为准，去掉 `ep-note` 徽标。

### 4.5 图标系统（`src/lib/constants.ts`）

- `CHANNEL_PROVIDER_ICONS` 从 emoji 换为内联 SVG（`<svg>` 字符串），键：
  `openai`、`google`、`deepseek`、`qwen`、`zhipu`、`doubao`、`moonshot`、`claudecode`、`ollama`、`custom`。
- `moonshot` 用 cc-switch 版 Kimi 图标（蓝色 #1783FF 角标 + `currentColor` K 主体，白色盒子上可见）。
- `claudecode` 用橙色 #D97757 Claude Code 星标（取自 `claudecode-color.svg`）。
- 新增 `claudecode` 键（Anthropic 协议图标），其余沿用已探明的官方图标。
- 原 `CHANNEL_PROVIDER_ICONS[preset.icon_key] ?? "❓"` 的回退逻辑保留。

## 5. 数据流

```
点击协议 Tab
  └─ 切换 form.protocol；provider 恢复为该协议自定义配置；端点重置为该协议默认
点击下拉框某项 provider
  └─ applyPreset(preset, true)
       ├─ 只写 connection 字段（url / endpoints / models / preset_revision）
       ├─ 保留 key / name / model_mapping / priority / weight / timeout
       └─ invalidateReceipt()（连接参数变更使草稿测试失效）
点击保存
  └─ validate()：OpenAI 端点 ≥1 → 否则提示「请至少选择一个端点」；其余校验不变
```

## 6. 错误处理

- `get_channel_presets` 失败：下拉框显示空态与错误信息（现有 `presetsError` 逻辑复用），Tab 仍可切换，provider 回退自定义。
- 切换 provider 时 `findPreset` 返回 null（数据不一致）：保持当前 provider，不崩溃。
- 端点无选中在保存时拦截，不产生半成品渠道。

## 7. 测试与验证

- Rust：`cargo test`（channel_presets 相关用例）+ `cargo clippy` + `cargo fmt`。
- 前端：`pnpm build` 通过；手测清单：
  1. 三个 Tab 常驻，无开关依赖。
  2. 下拉框分组正确（自定义/国际/国内/本地），悬停出描述，图标为品牌 SVG。
  3. 切换 provider 无弹窗；URL/端点/模型重置，API Key/名称/映射/优先级/权重/超时保留。
  4. OpenAI 端点可全不选、可保存时拦截提示。
  5. Anthropic 端点只显示 `messages`（无 count_tokens），checked-disabled。
  6. Ollama 端点 `api_chat` checked-disabled。
  7. 旧渠道编辑仍显示「来自旧配置」提示（该逻辑保留）。

## 8. 涉及文件

| 文件 | 操作 |
| --- | --- |
| `src-tauri/src/channel_presets.rs` | 改模板数据 + 测试 |
| `src/components/ChannelForm.tsx` | 下拉框、无确认切换、端点交互、去门控 |
| `src/components/channel-form/ConfirmSwitchDialog.tsx` | 删除 |
| `src/lib/constants.ts` | SVG 图标系统 |
| `docs/channel-refactor-tasks/13-provider-dropdown-prototype.html` | 原型基准（已移入 docs，不改） |
