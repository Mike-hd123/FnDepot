# 设计：编辑/新建渠道时展示实际请求 URL 预览

日期：2026-08-07
状态：已确认

## 背景

渠道表单的 Base URL 与端点分开填写，用户无法直观看到每个端点的**实际请求 URL**（Base URL + 端点路径拼接结果）。本设计在端点区域下方按当前已选端点逐行展示实际请求 URL，随输入实时更新。

## 需求

- 在端点列表下方，按 `form.native_endpoints` 的每个端点显示一行"实际请求 URL"。
- URL = Base URL 去尾斜杠 + 端点路径模板去首斜杠，二者以一个 `/` 连接。
- 随 Base URL 编辑、端点勾选/取消**实时更新**（纯派生，无新 state、无网络往返）。
- 新建与编辑模式都显示（同一组件 `ChannelForm` 共用）。
- 端点 chip 上现有的路径标签（`/chat/completions`、`/messages` 等）改由共享常量提供，消除前端重复字符串。

## 方案（已确认）

**前端计算**：路径模板与拼接在前端完成，不新增 Tauri 命令。路径映射与后端 Rust `endpoint_path`（`src-tauri/src/endpoint_executor/mod.rs`）保持一份重复（现状如此），靠两侧各自测试对齐。

### 改动

1. **`src/lib/constants.ts`** — 新增路径模板常量：

   ```ts
   /** 端点 → 请求路径模板（与后端 endpoint_executor::endpoint_path 对齐；Anthropic Base 自带 /v1）。 */
   export const ENDPOINT_PATHS: Record<string, string> = {
     chat_completions: "/chat/completions",
     responses: "/responses",
     messages: "/messages",
     count_tokens: "/messages/count_tokens",
     embeddings: "/embeddings",
     api_chat: "/api/chat",
   };
   ```

2. **`src/components/ChannelForm.tsx`** — 新增帮助函数（置于 `deriveLegacyBaseUrl` 附近）：

   ```ts
   /** Base URL（去尾斜杠）+ 端点路径（去首斜杠）→ 实际请求 URL。 */
   function joinUrl(base: string, path: string): string {
     const root = base.trim().replace(/\/+$/, "");
     if (!root) return "";
     return `${root}/${path.replace(/^\/+/, "")}`;
   }
   ```

3. **`src/components/ChannelForm.tsx`** — 端点区域（约第 700 行）下方、API Key 上方新增只读预览块：

   - 标题"实际请求 URL"；
   - 对 `form.native_endpoints` 的每个端点一行：左侧端点名（`ENDPOINT_LABELS[ep]`），右侧 `font-mono` 完整 URL；
   - Base URL 为空时显示提示文本而非 URL 行；
   - 样式沿用现有表单的 `text-muted-foreground` / `font-mono` / 圆角边框卡片风格。

4. **`src/components/ChannelForm.tsx`** — 把第 686、696 行端点 chip 的内联路径三元表达式（`ep === "chat_completions" ? "/chat/completions" : "/responses"` 等）替换为 `ENDPOINT_PATHS[ep]`。

### 数据流

```
form.native_endpoints × ENDPOINT_PATHS → joinUrl(form.native_base_url) → 只读展示
```

### 边界情况

- Base URL 为空 → 显示占位提示，不渲染空 URL。
- OpenAI 勾选/取消 Chat 或 Responses → 行数对应增减。
- 已存渠道 `native_endpoints` 含能力端点（count_tokens/embeddings）→ 同样列出，路径模板已包含。
- 尾斜杠（Base 或路径）双向容错。

## 不做的事（YAGNI）

- 不新增后端命令/接口。
- 不改持久化、不动 `native_endpoints` 语义。
- 不加前端测试框架（当前项目无 vitest/jest）；验证靠 `npm run build`（tsc）+ 后端 `cargo test` 兜底。

## 验证

- `npm run build` 类型检查通过。
- 手动核对：OpenAI 选 Chat+Responses、Anthropic、Ollama 三协议各看一眼 URL 拼接与 chip 标签一致。
- 后端无改动，`cargo test` 不受影响。
