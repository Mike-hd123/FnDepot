# 设计：Codex 账户卡片「全部模型」弹窗

日期：2026-08-09
状态：已批准

## 背景

渠道管理页的 Codex 账户卡片（`src/components/auth/AccountCard.tsx`）在「可用模型」区块只渲染前 4 个模型 id，超出部分用一个静态 `+N` 标签隐藏，无法查看完整列表。`account.models` 数据已在前端（`src/types/index.ts` 的 `AuthAccount.models: AuthModelState[]`），无需任何后端请求。

## 需求

- 点击 `+N` 小按钮，弹出一个小窗口查看该账户**全部**模型 id。
- 弹窗数据来自前端已有的 `account.models`，**不请求 Codex**。
- 仅当 `models.length > 4` 时 `+N` 才是可点击按钮；否则保持现状（不显示）。

## 方案

**纯前端改动**，两个文件，无后端改动。

### `src/components/auth/AccountCard.tsx`

把第 23 行的静态 `+N` span 改为按钮，点击打开居中小弹窗：

```tsx
{account.models.length > 4 && (
  <button onClick={() => setShowModels(true)} className="...">+{account.models.length - 4}</button>
)}
```

弹窗组件 `ModelsPopup`（`AccountCard` 内部状态 `useState<boolean>` 自管理，不提升到页面层）：

- 复用应用现有对话框模式：`fixed inset-0 z-50 flex items-center justify-center bg-foreground/35 p-4` + `.surface` 卡片，与 `ConfirmationDialog`（`AuthChannelsPage.tsx:20`）/ `LoginModal` 视觉一致。
- 内容：标题「全部模型 (N)」+ ✕ 关闭按钮；模型 id 以现有 chip 样式（`rounded-lg border border-border bg-muted px-2 py-1 text-[11px]`）flex-wrap 排列；超过弹窗最大高度时内部滚动（`max-h-[60vh] overflow-y-auto`）。
- 关闭方式：✕ 按钮 / 点击遮罩 / Esc 键（Esc 在弹窗 `useEffect` 加 `keydown` 监听）。

### `src/pages/AuthChannelsPage.tsx`

无需改动。

### 边界情况

- `models.length === 0`：现状逻辑不受影响（显示「尚无模型快照，不参与路由」）。
- `models.length` 在 1–4：不显示 `+N`，与现状一致。
- `models.length > 4`：点击 `+N` 弹出完整列表。

## 测试

前端无测试框架（`package.json` 无 vitest/jest）。通过 `npm run build`（tsc 类型检查 + vite build）验证。
