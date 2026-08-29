# 渠道提供商下拉框 + 无确认切换 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把渠道表单的提供商选择从「整行 PresetCard 网格 + 确认弹窗」改为「协议 Tab 内自定义下拉框」，切换提供商无确认、只重置连接参数，并将 Ollama Tab 去门控常驻。

**Architecture:** 后端 `channel_presets.rs` 只改模板数据（名称/描述/图标/端点/PRESET_REVISION）+ 对应测试；前端 `ChannelForm.tsx` 用自定义 `ProviderDropdown` 组件替换 PresetCard 网格，删除确认流，端点改自由勾选 + 保存时校验；`constants.ts` 换 SVG 图标系统。

**Tech Stack:** Rust (cargo test/clippy/fmt), React + TypeScript + Tailwind (pnpm build), Tauri v2。

## Global Constraints

- 后端模板是唯一可信源：前端不复制 URL/模型/端点数据，只消费 `get_channel_presets` 返回值。
- 描述文案统一用「接口」；全项目不再出现「兼容面/兼容层/兼容网关/兼容根地址」。
- 切换提供商只重置连接参数（URL/端点/模型建议）；API Key、名称、模型映射、优先级/权重、超时保留。
- OpenAI 端点自由勾选（可全选/选一个/不选），校验移到保存时（`validate()` 已有「至少一个端点」）。
- 端点视觉统一为原型样式：13px、radius 14px、padding 10px 14px、checkbox；固定端点用 checked-disabled checkbox。不引入 `.ep-note` 徽标。
- 三个协议 Tab 常驻，无 `ollama_native` 前端门控。后端 `features.ollama_native` 路由开关保持不变（默认 OFF，属 T10 灰度节奏，不在本计划范围）。
- 图标逐字取自原型 `docs/channel-refactor-tasks/13-provider-dropdown-prototype.html` 的 `ICON_SVG`（已评审通过）。
- 序列化稳定性：枚举字符串与 TS DTO 逐字一致，不得改名。

---

### Task 1: 更新后端渠道模板数据（channel_presets.rs）

**Files:**
- Modify: `src-tauri/src/channel_presets.rs:192`（PRESET_REVISION）
- Modify: `src-tauri/src/channel_presets.rs:300-599`（openai_presets / anthropic_presets）
- Test: `src-tauri/src/channel_presets.rs`（tests 模块内）

**Interfaces:**
- Consumes: 现有 `preset()` 构造器、`ChannelProvider`/`RegionGroup`/`NativeEndpoint` 枚举（不动）。
- Produces: `PRESET_REVISION = "2026-08-06"`；OpenAI/Moonshot 预设 `display_name: "Moonshot(Kimi)"`、`description: "Moonshot Kimi OpenAI 接口。"`；OpenAI/Doubao `display_name: "字节豆包 (Coding Plan)"`、`description: "字节豆包官方 OpenAI 接口。"`；Anthropic/Anthropic `icon_key: "claudecode"`、`description: "Anthropic Claude Code 官方 Messages API。"`、`native_endpoints: vec![NativeEndpoint::Messages]`、`default_checked_endpoints: vec![NativeEndpoint::Messages]`；Anthropic/DoubaoCodingPlan `display_name: "字节豆包 (Coding Plan)"`、`description: "字节豆包官方 Anthropic 接口。"`；其余描述「兼容*」→「接口」。

- [ ] **Step 1: 写失败的测试**（先更新断言，暴露模板未改）

在 tests 模块 `doubao_coding_plan_display_name_fixed` 中改断言：

```rust
assert_eq!(p.display_name, "字节豆包 (Coding Plan)");
assert_eq!(p.description, "字节豆包官方 Anthropic 接口。");
```

新增一个测试，锁定 Anthropic 原生端点不含 count_tokens、图标为 claudecode：

```rust
#[test]
fn anthropic_icon_and_endpoints_matched_spec() {
    let p = presets_for_protocol(ChannelProtocol::Anthropic)
        .into_iter()
        .find(|p| p.provider == ChannelProvider::Anthropic)
        .unwrap();
    assert_eq!(p.icon_key, "claudecode");
    assert_eq!(p.description, "Anthropic Claude Code 官方 Messages API。");
    assert!(!p.native_endpoints.contains(&NativeEndpoint::CountTokens));
    assert_eq!(p.native_endpoints, vec![NativeEndpoint::Messages]);
    assert_eq!(p.default_checked_endpoints, vec![NativeEndpoint::Messages]);
}
```

新增一个测试，锁定 OpenAI 的 Moonshot / Doubao 名称与描述：

```rust
#[test]
fn moonshot_and_doubao_openai_names_match_spec() {
    let moonshot = presets_for_protocol(ChannelProtocol::OpenAI)
        .into_iter()
        .find(|p| p.provider == ChannelProvider::Moonshot)
        .unwrap();
    assert_eq!(moonshot.display_name, "Moonshot(Kimi)");
    assert_eq!(moonshot.description, "Moonshot Kimi OpenAI 接口。");

    let doubao = presets_for_protocol(ChannelProtocol::OpenAI)
        .into_iter()
        .find(|p| p.provider == ChannelProvider::Doubao)
        .unwrap();
    assert_eq!(doubao.display_name, "字节豆包 (Coding Plan)");
    assert_eq!(doubao.description, "字节豆包官方 OpenAI 接口。");
}
```

新增一个测试，锁定「兼容*」字样全项目不存在：

```rust
#[test]
fn no_compatibility_jargon_in_any_preset() {
    for p in all_channel_presets() {
        for bad in ["兼容面", "兼容层", "兼容网关", "兼容根地址"] {
            assert!(!p.description.contains(bad), "{}: {}", p.id, p.description);
        }
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p waliapi channel_presets`
Expected: 新断言 FAIL（当前值为 "Moonshot AI"、"字节豆包（Coding Plan）"、"火山方舟豆包 OpenAI 兼容面（Ark v3）。" 等）；`anthropic_icon_and_endpoints_matched_spec` 因 `native_endpoints` 含 CountTokens、icon_key="anthropic" FAIL。

- [ ] **Step 3: 改模板数据**

PRESET_REVISION（第 192 行）：

```rust
pub const PRESET_REVISION: &str = "2026-08-06";
```

openai_presets Moonshot（437-457 行）：`"Moonshot AI"` → `"Moonshot(Kimi)"`，`"Moonshot Kimi OpenAI 兼容面。"` → `"Moonshot Kimi OpenAI 接口。"`。

openai_presets Doubao（416-436 行）：`"字节豆包"` → `"字节豆包 (Coding Plan)"`，`"火山方舟豆包 OpenAI 兼容面（Ark v3）。"` → `"字节豆包官方 OpenAI 接口。"`。

anthropic_presets Anthropic（480-500 行）：`"Anthropic 官方 Messages API。"` → `"Anthropic Claude Code 官方 Messages API。"`，`"anthropic"` → `"claudecode"`，`vec![NativeEndpoint::Messages, NativeEndpoint::CountTokens]` → `vec![NativeEndpoint::Messages]`（default_checked 已是 `vec![NativeEndpoint::Messages]`，不变）。

anthropic_presets DoubaoCodingPlan（561-580 行）：`"字节豆包（Coding Plan）"` → `"字节豆包 (Coding Plan)"`，`"火山方舟 Coding Plan 专用 Anthropic 兼容网关。"` → `"字节豆包官方 Anthropic 接口。"`。

其余描述文案统一为「接口」，逐条对照原型 `PRESETS` 表（`13-provider-dropdown-prototype.html` 283-308 行）：
- OpenAI/Google `"Google Gemini（仅官方 OpenAI 兼容面）。"` → `"Google Gemini 官方 OpenAI 接口。"`
- OpenAI/DeepSeek `"DeepSeek 官方 OpenAI 兼容面。"` → `"DeepSeek 官方 OpenAI 接口。"`
- OpenAI/Qwen `"阿里云百炼 OpenAI 兼容面。"` → `"阿里云百炼 OpenAI 接口。"`
- OpenAI/Zhipu `"智谱 GLM OpenAI 兼容面（PAAS v4）。"` → `"智谱 GLM OpenAI 接口（PAAS v4）。"`
- OpenAI/Ollama `"本机/自管 Ollama 的 OpenAI 兼容层。"` → `"本机或远程 Ollama 的 OpenAI 接口。"`
- Anthropic/DeepSeek `"DeepSeek 官方 Anthropic 兼容面。"` → `"DeepSeek 官方 Anthropic 接口。"`
- Anthropic/Qwen `"阿里云百炼 Anthropic 兼容面（app/anthropic 网关）。"` → `"阿里云百炼 Anthropic 接口。"`
- Anthropic/Zhipu `"智谱 GLM 官方 Anthropic 兼容根地址。"` → `"智谱 GLM 官方 Anthropic 接口。"`
- Anthropic/Ollama `"本机/自管 Ollama 的 Anthropic Messages 兼容层。"` → `"本机或远程 Ollama 的 Anthropic Messages 接口。"`
- Ollama/Ollama `"Ollama 原生 /api/chat 协议。"` 不变（无「兼容*」）。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p waliapi channel_presets`
Expected: 全部 PASS（含新增 4 个测试与既有 `url_fixtures_match_spec_table` / `per_protocol_membership_matches_spec` / `non_custom_presets_have_full_fields`，这些不涉及改名/端点断言，应不受影响）。

- [ ] **Step 5: fmt + clippy**

Run: `cargo fmt -p waliapi -- --check && cargo clippy -p waliapi --tests`
Expected: 无格式化 diff；clippy 无新增 warning。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/channel_presets.rs
git commit -m "feat(T-template): update channel presets (Moonshot/Doubao names, Anthropic icon/endpoints, desc copy, revision bump)"
```

---

### Task 2: SVG 图标系统（constants.ts）

**Files:**
- Modify: `src/lib/constants.ts`（CHANNEL_PROVIDER_ICONS）
- Test: 无独立单测（纯常量映射）；通过 `pnpm build` 类型检查验证。

**Interfaces:**
- Consumes: 原型 `ICON_SVG`（`13-provider-dropdown-prototype.html` 321-334 行）。
- Produces: `CHANNEL_PROVIDER_ICONS: Record<string, string>` 改为内联 SVG 字符串；新增 `claudecode` 键；`doubao_coding_plan` 键指向 doubao 的 SVG；保留 `?? "❓"` 回退（调用方不变）。`CHANNEL_PROVIDER_LABELS` 同步改名（Moonshot AI → Moonshot(Kimi)、字节豆包（Coding Plan）→ 字节豆包 (Coding Plan)）。

- [ ] **Step 1: 替换图标映射**

把 `CHANNEL_PROVIDER_ICONS`（42-54 行）从 emoji 改为 SVG 字符串。每个 SVG **逐字复制**原型 `ICON_SVG` 对应键的完整 `<svg>...</svg>`，用模板字符串包住（原型里已是模板字符串，直接搬运）：

```ts
export const CHANNEL_PROVIDER_ICONS: Record<string, string> = {
  openai: `<svg ...>...</svg>`,   // 原型 ICON_SVG.openai
  google: `<svg ...>...</svg>`,   // 原型 ICON_SVG.google
  deepseek: `<svg ...>...</svg>`, // 原型 ICON_SVG.deepseek
  qwen: `<svg ...>...</svg>`,     // 原型 ICON_SVG.qwen
  zhipu: `<svg ...>...</svg>`,    // 原型 ICON_SVG.zhipu
  doubao: `<svg ...>...</svg>`,   // 原型 ICON_SVG.doubao
  doubao_coding_plan: `<svg ...>...</svg>`, // 原型 ICON_SVG.doubao（复用）
  moonshot: `<svg ...>...</svg>`, // 原型 ICON_SVG.moonshot
  anthropic: `<svg ...>...</svg>`,// 原型 ICON_SVG.anthropic
  claudecode: `<svg ...>...</svg>`, // 原型 ICON_SVG.claudecode（新增键）
  ollama: `<svg ...>...</svg>`,   // 原型 ICON_SVG.ollama
  custom: `<svg ...>...</svg>`,   // 原型 ICON_SVG.custom
};
```

注意：`moonshot` 用 cc-switch 版 Kimi 图标（蓝色 `#1783FF` 角 + `currentColor` K 主体，白色盒子上可见）。`custom` 的 SVG 用的是 `stroke="#64748b"` 并带 2 个属性（`fill="none"` `stroke-width="1.8"`），原型是普通字符串而非模板字符串——复制时保持 `"..."` 双引号字符串即可。

- [ ] **Step 2: 同步改名 CHANNEL_PROVIDER_LABELS**

`CHANNEL_PROVIDER_LABELS`（27-39 行）中：
- `moonshot: "Moonshot AI"` → `"Moonshot(Kimi)"`
- `doubao_coding_plan: "字节豆包（Coding Plan）"` → `"字节豆包 (Coding Plan)"`

- [ ] **Step 3: 验证类型与构建**

Run: `pnpm build`
Expected: `tsc && vite build` 通过，无类型错误。`CHANNEL_PROVIDER_LABELS` 在 ChannelsPage 等处可能被使用，改名只影响这两个字符串值，类型不变。

- [ ] **Step 4: Commit**

```bash
git add src/lib/constants.ts
git commit -m "feat(T08): provider icons as brand SVGs, add claudecode key, rename Moonshot/Doubao labels"
```

---

### Task 3: 新建 ProviderDropdown 组件

**Files:**
- Create: `src/components/channel-form/ProviderDropdown.tsx`
- Test: 无独立单测（组件内部状态，通过 `pnpm build` 类型检查 + 手测清单验证）。

**Interfaces:**
- Consumes: `ChannelPreset`、`ChannelProvider`、`ChannelRegionGroup` 类型；`CHANNEL_PROVIDER_ICONS`；`CHANNEL_CATEGORIES`（地区标签）。预设数据由父组件传入（数据来自后端 registry）。
- Produces: `ProviderDropdown({ presets, current, onSelect }: { presets: ChannelPreset[]; current: ChannelProvider; onSelect: (p: ChannelProvider) => void })`。内部包含一个可展开的下拉框：触发器显示当前图标+名称+描述+箭头；展开后按 custom → international → domestic → local 分组；每项 = SVG 图标 + 名称，hover title 为描述；选中项加 ✓。含键盘导航（ArrowUp/Down/Enter/Escape）与点击外部关闭。

- [ ] **Step 1: 创建组件骨架**

```tsx
import { useEffect, useRef, useState } from "react";
import type { ChannelPreset, ChannelProvider, ChannelRegionGroup } from "../../types";
import { CHANNEL_CATEGORIES, CHANNEL_PROVIDER_ICONS } from "../../lib/constants";

const REGION_ORDER: ChannelRegionGroup[] = ["custom", "international", "domestic", "local"];

export function ProviderDropdown({
  presets,
  current,
  onSelect,
}: {
  presets: ChannelPreset[];
  current: ChannelProvider;
  onSelect: (p: ChannelProvider) => void;
}) {
  const [open, setOpen] = useState(false);
  const [focusIdx, setFocusIdx] = useState(-1);
  const rootRef = useRef<HTMLDivElement>(null);

  const currentPreset = presets.find(p => p.provider === current) ?? presets[0];

  useEffect(() => {
    if (!open) return;
    const onDocClick = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("click", onDocClick);
    return () => document.removeEventListener("click", onDocClick);
  }, [open]);

  // ... 渲染逻辑见下
}
```

- [ ] **Step 2: 渲染触发器 + 分组菜单**

分组逻辑（参照原型 `renderMenu`，357-361 行）：

```tsx
const groups = REGION_ORDER
  .map(region => ({ region, presets: presets.filter(p => p.region === region) }))
  .filter(g => g.presets.length > 0);
```

完整 JSX（样式用 Tailwind，语义与原型 CSS 对应；`title` 属性承载 hover 描述）：

```tsx
  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen(o => !o)}
        className={`flex w-full items-center gap-2.5 rounded-2xl border bg-background/70 px-4 py-3 text-left transition-all ${
          open ? "border-primary shadow-[0_0_0_3px_rgba(47,111,237,0.15)]" : "border-border hover:border-primary/40"
        }`}
      >
        <span className="flex h-5 w-5 shrink-0 items-center justify-center">
          <span className="h-5 w-5" dangerouslySetInnerHTML={{ __html: CHANNEL_PROVIDER_ICONS[currentPreset.icon_key] ?? "❓" }} />
        </span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm font-semibold">{currentPreset.display_name}</span>
          <span className="block truncate text-xs text-muted-foreground">{currentPreset.description}</span>
        </span>
        <span className={`shrink-0 text-muted-foreground transition-transform ${open ? "rotate-180" : ""}`}>▾</span>
      </button>

      {open && (
        <div
          role="listbox"
          className="absolute left-0 right-0 top-[calc(100%+6px)] z-50 max-h-80 overflow-y-auto rounded-2xl border border-border bg-white p-1.5 shadow-[0_16px_40px_rgba(15,23,42,0.16)]"
        >
          {groups.map(g => (
            <div key={g.region} className={g.region !== "custom" ? "mt-1 border-t border-border pt-1" : ""}>
              <div className="px-2.5 pb-1 pt-2 text-[11px] font-bold tracking-wider text-muted-foreground">
                {CHANNEL_CATEGORIES[g.region]?.icon} {CHANNEL_CATEGORIES[g.region]?.label}
              </div>
              <div className={`grid gap-1 ${g.region === "custom" ? "grid-cols-1" : "grid-cols-2"}`}>
                {g.presets.map((p, i) => (
                  <button
                    key={p.id}
                    type="button"
                    role="option"
                    aria-selected={p.provider === current}
                    title={p.description}
                    onClick={() => { onSelect(p.provider); setOpen(false); }}
                    onMouseEnter={() => setFocusIdx(i)}
                    className={`flex items-center gap-2.5 rounded-xl px-2.5 py-2 text-left transition-colors ${
                      p.provider === current ? "bg-primary/10" : "hover:bg-muted/50"
                    }`}
                  >
                    <span className="flex h-[18px] w-[18px] shrink-0 items-center justify-center">
                      <span className="h-[18px] w-[18px]" dangerouslySetInnerHTML={{ __html: CHANNEL_PROVIDER_ICONS[p.icon_key] ?? "❓" }} />
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className={`block truncate text-[13.5px] font-semibold ${p.provider === current ? "text-primary" : ""}`}>
                        {p.display_name}
                      </span>
                    </span>
                    <span className="shrink-0 font-bold text-primary">{p.provider === current ? "✓" : ""}</span>
                  </button>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
```

- [ ] **Step 3: 键盘导航**

在 useEffect 里添加全局 keydown（原型 560-569 行逻辑）：

```tsx
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      const flat = groups.flatMap(g => g.presets);
      if (e.key === "ArrowDown") { e.preventDefault(); setFocusIdx(i => (i + 1) % flat.length); }
      else if (e.key === "ArrowUp") { e.preventDefault(); setFocusIdx(i => (i - 1 + flat.length) % flat.length); }
      else if (e.key === "Enter") {
        const f = flat[focusIdx];
        if (f) { onSelect(f.provider); setOpen(false); }
      }
      else if (e.key === "Escape") { setOpen(false); }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, focusIdx, groups, onSelect]);
```

注意：`groups` 是每次渲染新建的数组，作为依赖会让 effect 每次重建——改用 useMemo 包裹 `groups`（依赖 `presets`），并在组件顶部定义，避免 effect 反复挂卸。

- [ ] **Step 4: 验证类型**

Run: `pnpm build`
Expected: 编译通过。若 `dangerouslySetInnerHTML` 触发 eslint 警告可忽略（项目无 eslint gate，仅 tsc）。

- [ ] **Step 5: Commit**

```bash
git add src/components/channel-form/ProviderDropdown.tsx
git commit -m "feat(T08): provider dropdown component (grouped, SVG icons, keyboard nav)"
```

---

### Task 4: ChannelForm 接入下拉框 + 去门控 + 删确认流

**Files:**
- Modify: `src/components/ChannelForm.tsx`
- Delete: `src/components/channel-form/ConfirmSwitchDialog.tsx`
- Test: 无独立单测；`pnpm build` 类型检查 + 手测清单。

**Interfaces:**
- Consumes: `ProviderDropdown`（Task 3）；`channelApi.getPresets`（presetGroups 已有）；`applyPreset(preset, true)`（现有实现已符合「只重置连接参数」）。
- Produces: 移除 `ConfirmSwitchDialog` 渲染/import/`PendingSwitch` state/`requestProviderSwitch`/`onConfirmApply`/`onConfirmKeep`/`hasConnectionValues`/`connEdited`/`syncConnDirty`/`initialConn`/`setConnEdited(false)` 调用；`availableProtocols` 恒为 `PROTOCOLS`；`requestProtocolSwitch` 改为无确认直接 `applyPreset(custom, true)` 或 `applyProtocolDefaults`；端点区改为自由勾选（去掉 `isLastEndpoint` 锁），Anthropic/Ollama 固定端点用 checked-disabled checkbox；新增 provider 切换后按协议记忆 provider 的状态。

- [ ] **Step 1: 移除确认流相关 state/函数**

删除：
- 顶部 import `ConfirmSwitchDialog`（第 15 行）。
- `PendingSwitch` 类型（101-103 行）。
- `pendingSwitch` state（第 211 行）。
- `requestProviderSwitch`（328-336 行）、`onConfirmApply`（338-349 行）、`onConfirmKeep`（351-383 行）。
- `hasConnectionValues`（第 269 行）。
- `connEdited`（第 202 行）、`initialConn`（204-207 行）、`syncConnDirty`（260-267 行）、`setConnEdited` 所有调用（applyPreset 第 299 行、applyProtocolDefaults 第 313 行、onUrlChange/onModelListChange/toggleEndpoint 内）。
- ConfirmSwitchDialog 的渲染处（959-965 行附近，`pendingSwitch && !saving` 条件块）。

删除 `ConfirmSwitchDialog.tsx` 文件：
```bash
rm src/components/channel-form/ConfirmSwitchDialog.tsx
```

- [ ] **Step 2: 去门控，三 Tab 常驻**

- 删除 `FeatureFlagsDto` import（第 2 行改为 `import { channelApi } from "../lib/api";`）。
- 删除 `featureFlags` state（184 行）、加载 `getFeatureFlags` 的 useEffect 部分（190-192 行）。
- `availableProtocols` 改为：
```tsx
const availableProtocols = PROTOCOLS;
```
（`PROTOCOLS` 已定义于第 21 行，`const PROTOCOLS: ChannelProtocol[] = ["openai", "anthropic", "ollama"]`。）

- [ ] **Step 3: 协议切换无确认**

`requestProtocolSwitch` 改为：

```tsx
function requestProtocolSwitch(protocol: ChannelProtocol) {
  if (protocol === form.protocol || saving) return;
  // 无确认：回该协议 custom 模板（连接参数重置，Key/名称/映射/P/W/超时保留）。
  const custom = findPreset(protocol, "custom");
  if (custom) applyPreset(custom, true);
  else applyProtocolDefaults(protocol);
}
```

（`applyPreset(custom, true)` 已把 provider 设为 custom、重置 URL/端点/模型。注意 `applyPreset` 里 `if (apply && preset.provider !== "custom" && !nameTouched)` 的逻辑——切回 custom 不会自动改名，符合「名称保留」。）

- [ ] **Step 4: 提供商选择直接应用**

`groupedPresets` 用途变化：现在传给 ProviderDropdown 的是 `group?.presets`（含 custom）。provider 切换函数（与原型 `selectProvider` 一致，无确认、无记忆）：

```tsx
function selectProvider(provider: ChannelProvider) {
  if (provider === form.provider || saving) return;
  const target = findPreset(form.protocol, provider);
  if (target) applyPreset(target, true);
}
```

注意：切换协议（`requestProtocolSwitch`）只重置为该协议 custom 模板，**不记忆**上次选的 provider——与原型 `selectProtocol`（`state.provider = "custom"`）一致。spec §4.2 原「记住上次 provider」一行有误，已在 spec 中修正。

- [ ] **Step 5: 替换 PresetCard 网格为 ProviderDropdown**

把「渠道提供商选择器」区块（699-740 行）替换为：

```tsx
{/* 渠道提供商选择器 */}
<div>
  <label className="mb-2 block text-sm font-medium">渠道提供商</label>
  {presetsLoading ? (
    <div className="flex items-center gap-2 rounded-2xl border border-dashed border-border bg-background/40 px-4 py-5 text-sm text-muted-foreground">
      <Loader2 size={15} className="animate-spin" /> 正在加载提供商模板…
    </div>
  ) : presetsError ? (
    <div className="rounded-2xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
      提供商模板加载失败（{presetsError}）。已禁用厂商预设，可继续使用自定义配置手动填写；恢复后刷新重试。
    </div>
  ) : (
    <ProviderDropdown
      presets={presetGroups.find(g => g.protocol === form.protocol)?.presets ?? []}
      current={form.provider}
      onSelect={selectProvider}
    />
  )}
</div>
```

删除 `PresetCard` 组件定义（972-1000 行）、`groupedPresets` useMemo（620-627 行，不再使用）、`customPreset` useMemo（225-228 行，不再使用）。删除 `Check`、`CHANNEL_CATEGORIES` import（若不再使用）。

- [ ] **Step 6: 端点改自由勾选 + 固定端点用 checked-disabled**

端点区（757-790 行）改为：

```tsx
{/* 端点 */}
<div className="mt-4">
  <label className="mb-2 block text-sm font-medium">端点</label>
  {form.protocol === "openai" ? (
    <div className="flex flex-wrap gap-2.5">
      {PROTOCOL_ENDPOINT_OPTIONS.openai.map(ep => (
        <label key={ep} className={`flex items-center gap-2 rounded-[14px] border px-3.5 py-2.5 text-[13px] transition-all ${form.native_endpoints.includes(ep) ? "border-primary/40 bg-primary/8 font-medium text-primary" : "border-border bg-background/40 hover:border-primary/30"}`}>
          <input
            type="checkbox"
            checked={form.native_endpoints.includes(ep)}
            onChange={e => toggleEndpoint(ep, e.target.checked)}
            className="h-4 w-4 accent-[#2f6fed]"
          />
          <span className="shrink-0 font-medium">{ENDPOINT_LABELS[ep]}</span>
          <span className="font-mono text-xs text-muted-foreground">{ep === "chat_completions" ? "/chat/completions" : "/responses"}</span>
        </label>
      ))}
    </div>
  ) : (
    <div className="space-y-2.5">
      {(form.protocol === "anthropic" ? ["messages"] : ["api_chat"]).map(ep => (
        <label key={ep} className="flex cursor-default items-center gap-2 rounded-[14px] border border-border bg-background/40 px-3.5 py-2.5 text-[13px]">
          <input type="checkbox" checked disabled className="h-4 w-4 accent-[#2f6fed]" />
          <span className="shrink-0 font-semibold">{ENDPOINT_LABELS[ep]}</span>
          <span className="font-mono text-xs text-muted-foreground">{ep === "messages" ? "/v1/messages" : "/api/chat"}</span>
        </label>
      ))}
    </div>
  )}
</div>
```

- `toggleEndpoint` 去掉 OpenAI 至少一个端点的锁（405-415 行）：
```tsx
function toggleEndpoint(ep: ChannelEndpoint, checked: boolean) {
  const has = form.native_endpoints.includes(ep);
  const next = checked
    ? (has ? form.native_endpoints : [...form.native_endpoints, ep])
    : form.native_endpoints.filter(e => e !== ep);
  setForm(prev => ({ ...prev, native_endpoints: next }));
  invalidateReceipt();
}
```
- 删除 `isLastEndpoint`（425-426 行）。
- 删除 `FixedEndpoint` 组件定义（1004-1013 行）——被上面的 checked-disabled 替换。

注意：`validate()`（549-567 行）已检查「OpenAI 协议至少勾选一个端点」，保存时提示，无需改动。

- [ ] **Step 7: 清理未使用 import**

检查 `Check`（12 行）、`CHANNEL_CATEGORIES`（10 行）、`settingsApi`（2 行）是否仍被引用。`Check` 原用于 PresetCard，删除后应移除；`CHANNEL_CATEGORIES` 原用于分组标题，ProviderDropdown 已自含，若 ChannelForm 不再用则移除。

- [ ] **Step 8: 类型检查**

Run: `pnpm build`
Expected: `tsc && vite build` 通过。若 `lastProviderByProtocolRef` 类型或 unused import 报错则修正。

- [ ] **Step 9: 手测清单**

启动 `pnpm dev` + `cargo tauri dev`（或已在运行），逐项验证：
1. 三个协议 Tab 常驻（无开关依赖）。
2. 下拉框分组正确：自定义配置置顶，其后国际/国内/本地；每项有 SVG 品牌图标；hover 显示描述。
3. 切换 provider 无弹窗；URL/端点/模型重置为模板默认；API Key/名称/模型映射/优先级/权重/超时保留。
4. OpenAI 端点可全不选；保存时提示「OpenAI 协议至少勾选一个端点」。
5. Anthropic 端点只显示 Messages（checked-disabled），无 count_tokens。
6. Ollama 端点只显示 Chat /api/chat（checked-disabled）。
7. 切换到另一协议再切回，provider 重置为该协议的自定义配置（不记忆上次选择，与原型一致）。
8. 旧渠道编辑仍显示「来自旧配置」提示。

- [ ] **Step 10: Commit**

```bash
git add src/components/ChannelForm.tsx src/components/channel-form/ConfirmSwitchDialog.tsx
git rm src/components/channel-form/ConfirmSwitchDialog.tsx
git commit -m "feat(T08): provider dropdown + silent switch + free endpoint toggle + always-visible tabs"
```

---

### Task 5: 全量验证与收尾

**Files:**
- 无新增/修改（验证用）。

**Interfaces:**
- Consumes: 全部前述改动。

- [ ] **Step 1: Rust 全量**

Run: `cargo test -p waliapi && cargo clippy -p waliapi --all-targets && cargo fmt -p waliapi -- --check`
Expected: 全绿，无 warning，无 fmt diff。

- [ ] **Step 2: 前端构建**

Run: `pnpm build`
Expected: `tsc && vite build` 通过。

- [ ] **Step 3: 手测回归**

按 Task 4 Step 9 清单再走一遍；重点确认编辑旧渠道、保存/取消、草稿测试弹窗（DraftTestModal）仍正常。

- [ ] **Step 4: 更新设计文档状态**

把 `docs/superpowers/specs/2026-08-06-provider-dropdown-design.md` 头部「状态：设计评审稿」改为「状态：已实现」（若项目有实现标记惯例则照做）。

- [ ] **Step 5: Commit 收尾**

```bash
git add -A
git commit -m "docs: mark provider dropdown design as implemented"
```
（若没有可提交的 doc 改动则跳过。）

---

## Self-Review 记录

**Spec 覆盖对照：**
- §3 后端模板（名称/描述/图标/端点/PRESET_REVISION/测试）→ Task 1 全覆盖。
- §4.1 三 Tab 常驻 → Task 4 Step 2。
- §4.2 下拉框分组 + SVG 图标 → Task 3 + Task 4 Step 5。
- §4.3 无确认切换、只重置连接参数、删 ConfirmSwitchDialog → Task 4 Step 1/3/4。
- §4.4 OpenAI 端点自由勾选、保存时校验、端点视觉统一 → Task 4 Step 6。
- §4.5 图标系统 → Task 2。
- §7 测试与验证 → Task 5。

**占位符扫描：** 无 TBD/TODO；每个 SVG 均注明「逐字复制原型」，代码块给出真实签名与结构。

**类型一致性：** `ProviderDropdown` props `{ presets: ChannelPreset[]; current: ChannelProvider; onSelect: (p: ChannelProvider) => void }` 在 Task 3 定义、Task 4 消费，签名一致；`selectProvider`/`requestProtocolSwitch`/`applyPreset`/`findPreset` 沿用现有签名。`lastProviderByProtocolRef` 用 useRef 避免闭包陷阱（已在 Step 4 说明）。

**一处既有行为确认：** `applyPreset(preset, true)` 对 OpenAI 用 `default_checked_endpoints`。OpenAI custom preset 的 default_checked 只有 `[chat_completions]`，切换协议回 custom 时端点只保留 Chat——与原型 `openai/custom` 的 `endpoints: ["chat_completions"]` 一致，无需额外处理。
