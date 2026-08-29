# 渠道表单端点实际请求 URL 预览 — 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在渠道表单（新建+编辑）端点列表下方，按当前已选端点逐行显示实际请求 URL（Base URL + 端点路径）。

**Architecture:** 纯前端改动。在 `constants.ts` 新增端点→路径模板共享常量 `ENDPOINT_PATHS`，在 `ChannelForm.tsx` 加一个 `joinUrl` 帮助函数拼接完整 URL，并在端点区块下渲染只读预览行。端点 chip 上现有的内联路径字符串改由 `ENDPOINT_PATHS` 提供。

**Tech Stack:** React 19 + TypeScript（Vite），Tauri。无前端测试框架。

## Global Constraints

- 路径模板必须与后端 `src-tauri/src/endpoint_executor/mod.rs` 的 `endpoint_path()` 保持一致：
  - openai chat_completions → `/chat/completions`
  - openai responses → `/responses`
  - openai embeddings → `/embeddings`
  - anthropic messages → `/messages`（Base 自带 /v1，main 分支约定）
  - anthropic count_tokens → `/messages/count_tokens`
  - ollama api_chat → `/api/chat`
- 端点名显示用现有 `ENDPOINT_LABELS`（`src/lib/constants.ts`）。
- 不做后端改动、不新增依赖、不改 `native_endpoints` 语义。
- 验证命令：`npm run build`（tsc 类型检查）。后端 `cargo test` 不受影响，无需运行。
- 提交信息遵循仓库现有风格（`feat(...)` / `refactor(...)`）。

---

### Task 1: 新增 ENDPOINT_PATHS 常量

**Files:**
- Modify: `src/lib/constants.ts`（在 `ENDPOINT_LABELS` 定义之后追加）

**Interfaces:**
- Produces: `export const ENDPOINT_PATHS: Record<string, string>` —— 端点 key → 请求路径模板（带前导 `/`）。Task 2 与 Task 3 依赖它。

- [ ] **Step 1: 在 `src/lib/constants.ts` 的 `ENDPOINT_LABELS` 之后追加常量**

```ts
// 端点 → 请求路径模板（与后端 endpoint_executor::endpoint_path 对齐；
// Anthropic Base 自带 /v1，故 messages/count_tokens 只补 /messages）。实际请求
// URL = native_base_url 去尾斜杠 + 本路径去首斜杠（见 ChannelForm.joinUrl）。
export const ENDPOINT_PATHS: Record<string, string> = {
  chat_completions: "/chat/completions",
  responses: "/responses",
  messages: "/messages",
  count_tokens: "/messages/count_tokens",
  embeddings: "/embeddings",
  api_chat: "/api/chat",
};
```

- [ ] **Step 2: 类型检查**

Run: `cd /Users/xian/Project/ai/WaLiAPI && npm run build`
Expected: 构建通过（tsc 无错误）。

- [ ] **Step 3: Commit**

```bash
git add src/lib/constants.ts
git commit -m "feat(channel-form): 新增端点→请求路径模板常量 ENDPOINT_PATHS"
```

---

### Task 2: 新增 joinUrl 帮助函数

**Files:**
- Modify: `src/components/ChannelForm.tsx`（在 `deriveLegacyBaseUrl` 函数定义之后、`interface FormState` 之前）

**Interfaces:**
- Consumes: 无（纯字符串工具）。
- Produces: `function joinUrl(base: string, path: string): string` —— Base URL 去尾斜杠 + 路径去首斜杠，以一个 `/` 连接；Base 为空返回 `""`。Task 3 依赖它。

- [ ] **Step 1: 新增 joinUrl 函数**

```ts
/** Base URL（去尾斜杠）+ 端点路径（去首斜杠）→ 实际请求 URL；Base 为空返回空串。 */
function joinUrl(base: string, path: string): string {
  const root = base.trim().replace(/\/+$/, "");
  if (!root) return "";
  return `${root}/${path.replace(/^\/+/, "")}`;
}
```

- [ ] **Step 2: 类型检查**

Run: `cd /Users/xian/Project/ai/WaLiAPI && npm run build`
Expected: 构建通过。函数当前未被调用，tsc 不会报未使用（模块内被 Task 3 引用后即使用）。

- [ ] **Step 3: Commit**

```bash
git add src/components/ChannelForm.tsx
git commit -m "feat(channel-form): 新增 Base URL + 端点路径拼接帮助函数 joinUrl"
```

---

### Task 3: 端点区域下方新增实际请求 URL 预览块

**Files:**
- Modify: `src/components/ChannelForm.tsx`
  - 第 7-12 行：import 从 `"../lib/constants"` 增加 `ENDPOINT_PATHS`
  - 第 686、696 行：chip 内联路径三元表达式替换为 `ENDPOINT_PATHS[ep]`
  - 第 700-701 行（端点区块 `<div>` 结束 `</div>` 之后、`{/* API Key */}` 注释之前）：插入预览块

**Interfaces:**
- Consumes: `ENDPOINT_PATHS`（Task 1）、`joinUrl`（Task 2）、现有 `ENDPOINT_LABELS` 与 `form.native_endpoints`。
- Produces: 无（纯 UI 展示）。

- [ ] **Step 1: 引入 ENDPOINT_PATHS**

把现有 import 行（约第 10-12 行）改为：

```ts
import {
  PROTOCOL_LABELS, ENDPOINT_LABELS, ENDPOINT_PATHS,
} from "../lib/constants";
```

- [ ] **Step 2: chip 标签改用 ENDPOINT_PATHS**

第 686 行（OpenAI chip，chat_completions 分支）把：
```tsx
<span className="font-mono text-xs text-muted-foreground">{ep === "chat_completions" ? "/chat/completions" : "/responses"}</span>
```
替换为：
```tsx
<span className="font-mono text-xs text-muted-foreground">{ENDPOINT_PATHS[ep]}</span>
```

第 696 行（Anthropic/Ollama chip）把：
```tsx
<span className="font-mono text-xs text-muted-foreground">{ep === "messages" ? "/messages" : "/api/chat"}</span>
```
替换为：
```tsx
<span className="font-mono text-xs text-muted-foreground">{ENDPOINT_PATHS[ep]}</span>
```

- [ ] **Step 3: 端点区块下方插入预览块**

在第 700 行 `</div>`（端点区块结束）之后、第 703 行 `{/* API Key */}` 之前插入：

```tsx
{/* 实际请求 URL 预览：Base URL + 端点路径，随输入实时派生 */}
<div className="mt-4">
  <label className="mb-2 block text-sm font-medium">实际请求 URL</label>
  {form.native_base_url.trim() === "" ? (
    <div className="rounded-2xl border border-dashed border-border bg-background/40 px-3.5 py-2.5 text-xs text-muted-foreground">
      填写 Base URL 后显示各端点的实际请求地址
    </div>
  ) : (
    <ul className="space-y-2">
      {form.native_endpoints.map(ep => (
        <li key={ep} className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 rounded-2xl border border-border bg-background/40 px-3.5 py-2.5">
          <span className="shrink-0 text-xs font-medium">{ENDPOINT_LABELS[ep]}</span>
          <code className="break-all font-mono text-xs text-muted-foreground">
            {joinUrl(form.native_base_url, ENDPOINT_PATHS[ep])}
          </code>
        </li>
      ))}
    </ul>
  )}
</div>
```

- [ ] **Step 4: 类型检查**

Run: `cd /Users/xian/Project/ai/WaLiAPI && npm run build`
Expected: 构建通过，无类型错误。

- [ ] **Step 5: 手动核对渲染**

Run: `cd /Users/xian/Project/ai/WaLiAPI && npm run dev`（本地起 Vite；如需 Tauri 全量可 `npm run tauri dev`）

手动检查三协议（新建与编辑各一次）：
1. OpenAI 预设，勾选 Chat Completions + Responses：预览出现两行 `https://api.openai.com/v1/chat/completions` 与 `https://api.openai.com/v1/responses`；取消勾选 Chat 后该行消失。
2. Anthropic 预设：一行 `https://api.anthropic.com/v1/messages`。
3. Ollama 预设：一行 `http://localhost:11434/v1/api/chat`。
4. 清空 Base URL：显示占位提示而非 URL 行；Base 以 `/` 结尾（如 `https://api.anthropic.com/v1/`）时拼接无重复斜杠。
5. chip 上的路径标签与预览 URL 路径一致。

Expected: 以上全部符合。

- [ ] **Step 6: Commit**

```bash
git add src/components/ChannelForm.tsx
git commit -m "feat(channel-form): 端点下方展示实际请求 URL 预览，随输入实时更新"
```
