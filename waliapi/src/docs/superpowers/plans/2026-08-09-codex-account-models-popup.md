# Codex 账户卡片「全部模型」弹窗 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 点击 Codex 账户卡片的 `+N` 按钮，弹出居中小弹窗查看该账户全部模型 id（数据来自前端已有的 `account.models`，不请求后端）。

**Architecture:** 纯前端改动。在 `AccountCard.tsx` 内新增一个内部状态 `showModels` 控制的 `ModelsPopup` 弹窗组件，复用应用现有对话框模式（`fixed inset-0 z-50 bg-foreground/35` + `.surface` 卡片）。`+N` 静态 span 改为按钮。

**Tech Stack:** React 19 + TypeScript + Tailwind v4（现有 chip 样式复用，不新增依赖）。

## Global Constraints

- 不新增任何 npm 依赖。
- 不改 `src/pages/AuthChannelsPage.tsx`（弹窗由卡片内部自管理）。
- 不向后端 / Codex 发任何请求；只用 `account.models`（类型 `AuthModelState[]`，`src/types/index.ts:170-176`）。
- 仅当 `account.models.length > 4` 时才渲染 `+N` 按钮（与现状一致）。
- 弹窗视觉与现有 `ConfirmationDialog` / `LoginModal` 一致（`surface` 卡片 + 遮罩）。
- 前端无测试框架，验证方式为 `npm run build`（tsc 类型检查 + vite build）。

---

### Task 1: AccountCard 弹窗

**Files:**
- Modify: `src/components/auth/AccountCard.tsx:1` (imports)、`:23` (模型区块)

**Interfaces:**
- Consumes: `AuthAccount`（`src/types/index.ts:201-222`），`account.models: AuthModelState[]`，每个元素有 `id: string`。
- Produces: `AccountCard` 内部组件 `ModelsPopup`（props: `models: AuthModelState[]`, `onClose: () => void`）。

- [ ] **Step 1: 新增弹窗组件 + 状态 + 改按钮**

在 `src/components/auth/AccountCard.tsx` 中：

```tsx
import { useEffect, useState } from "react";
import { Download, Edit3, KeyRound, Loader2, Power, RefreshCw, RotateCw, Trash2, X } from "lucide-react";
```

在 `AccountCard` 组件顶部（`const invalid = ...` 前）加状态：

```tsx
const [showModels, setShowModels] = useState(false);
```

把第 23 行的静态 `+N` span 改为按钮：

```tsx
{account.models.length > 4 && <button onClick={() => setShowModels(true)} className="rounded-lg border border-border bg-muted px-2 py-1 text-[11px] text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground" title="查看全部模型" aria-label="查看全部模型">+{account.models.length - 4}</button>}
```

（注：样式与旁边模型 chip 一致，追加 hover 反馈。）

- [ ] **Step 2: 添加 ModelsPopup 组件定义**

在 `AccountCard` 函数之后（文件末尾）添加弹窗组件：

```tsx
function ModelsPopup({ models, onClose }: { models: AuthModelState[]; onClose: () => void }) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-foreground/35 p-4" role="dialog" aria-modal="true" aria-labelledby="models-popup-title" onClick={onClose}>
      <div className="surface w-full max-w-md rounded-[24px] p-6 shadow-2xl" onClick={(event) => event.stopPropagation()}>
        <div className="flex items-start justify-between gap-3">
          <h2 id="models-popup-title" className="text-lg font-semibold">全部模型 ({models.length})</h2>
          <button onClick={onClose} aria-label="关闭全部模型弹窗" className="rounded-lg p-1 text-muted-foreground hover:bg-muted"><X size={18} /></button>
        </div>
        <div className="mt-4 flex max-h-[60vh] flex-wrap gap-1.5 overflow-y-auto">
          {models.map((model) => <span key={model.id} className="rounded-lg border border-border bg-muted px-2 py-1 text-[11px] text-muted-foreground">{model.id}</span>)}
        </div>
      </div>
    </div>
  );
}
```

`AuthModelState` 类型从 `../../types` 导入（`import type { AuthAccount, AuthModelState } from "../../types";`）。

- [ ] **Step 3: 在卡片内挂载弹窗**

在 `AccountCard` 返回的 JSX 末尾（`</article>` 之前，`<div className="mt-4 grid grid-cols-3 ...">...</div>` 之后）加：

```tsx
{showModels && <ModelsPopup models={account.models} onClose={() => setShowModels(false)} />}
```

- [ ] **Step 4: 构建验证**

Run: `npm run build`
Expected: `tsc` 无类型错误，`vite build` 成功（无未使用 import 报错——确保 `useEffect`、`useState`、`X`、`AuthModelState` 均被使用）。

- [ ] **Step 5: 提交**

```bash
git add src/components/auth/AccountCard.tsx
git commit -m "feat: Codex 账户卡片 +N 按钮弹出全部模型弹窗"
```

---
