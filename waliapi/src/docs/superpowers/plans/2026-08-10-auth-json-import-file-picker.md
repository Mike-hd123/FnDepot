# auth.json 导入文件选择器 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让「从 auth.json 导入」弹原生文件选择框，默认选中 `~/.codex/auth.json`（或 `$CODEX_HOME/auth.json`），用户自行选文件后导入。

**Architecture:** 前端 `AuthChannelsPage.tsx::importAuth()` 改为两步：先 `await authDefaultImportPath()` 拿到默认路径作 `defaultPath` 弹 `plugin-dialog` 的 `open()`，选中后 `authApi.loginImport("codex", path)`。新增一个 5 行 Rust command `auth_default_import_path` 复用现有 `CodexLogin::default_auth_json_path()`，保证路径逻辑单一来源在后端。toast 用实际选中路径，不再硬编码。

**Tech Stack:** Tauri 2 (Rust), React + TS, `tauri-plugin-dialog`（已注册、capability 已开、`@tauri-apps/plugin-dialog` 已在依赖）。

## Global Constraints

- 默认路径逻辑**只在后端**：`default_auth_json_path()`（`$CODEX_HOME` 优先 → home 兜底），前端不复制。
- 对话框 `multiple: false`（单一账号文件），返回 `string | null`；取消返回 `null` → 静默 no-op，不弹 toast。
- 不改 `auth_login_import` 签名（已支持可选 `path`）。
- toast 文案用实际选中路径，不再出现硬编码「~/.codex/auth.json」。
- 前端弹框模式参照 `KnowledgeBasePage.tsx:1274`：动态 `import("@tauri-apps/plugin-dialog")`。
- Rust 侧不新增测试（纯透传 command）；前端无测试基建。

---

### Task 1: 新增 `auth_default_import_path` command 并注册

**Files:**
- Modify: `src-tauri/src/commands/auth.rs`（在 `auth_login_import` 附近新增）
- Modify: `src-tauri/src/lib.rs:173`（`invoke_handler` 注册）

**Interfaces:**
- Consumes: `CodexLogin::default_auth_json_path()`（`src-tauri/src/auth_provider/codex_login.rs:325`，已存在，返回 `Result<PathBuf, ProviderError>`）、`safe_error`
- Produces: command `auth_default_import_path` — `async fn() -> Result<String, String>`，返回默认 auth.json 路径字符串

- [ ] **Step 1: 在 `auth_login_import` 命令前新增 command**

在 `src-tauri/src/commands/auth.rs`，`auth_login_import` 定义之前（`import_path` 辅助函数之后）加入：

```rust
/// Return the default Codex CLI auth file path for the native file picker.
/// Reads no secrets; path logic stays in `CodexLogin::default_auth_json_path`.
#[tauri::command]
pub async fn auth_default_import_path() -> Result<String, String> {
    CodexLogin::default_auth_json_path()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(safe_error)
}
```

- [ ] **Step 2: 在 `invoke_handler` 注册**

在 `src-tauri/src/lib.rs:173`（`commands::auth::auth_login_import,` 之后）加入：

```rust
            commands::auth::auth_default_import_path,
```

- [ ] **Step 3: 编译验证**

Run: `cd src-tauri && cargo check`
Expected: 编译通过，无 warning 新增

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands/auth.rs src-tauri/src/lib.rs
git commit -m "feat(auth): 新增 auth_default_import_path command(文件选择器默认路径)"
```

---

### Task 2: 前端 `authApi` 新增 `defaultImportPath` 封装

**Files:**
- Modify: `src/lib/api.ts:102`（`loginImport` 之后新增）

**Interfaces:**
- Consumes: command `auth_default_import_path`（Task 1）
- Produces: `authApi.defaultImportPath: () => Promise<string>`

- [ ] **Step 1: 新增封装**

在 `src/lib/api.ts` 的 `authApi` 对象中，`loginImport` 之后新增：

```typescript
  defaultImportPath: () => invoke<string>("auth_default_import_path"),
```

- [ ] **Step 2: 类型/编译验证**

Run: `npx tsc --noEmit`
Expected: 无类型错误

- [ ] **Step 3: Commit**

```bash
git add src/lib/api.ts
git commit -m "feat(auth): 前端 authApi 新增 defaultImportPath"
```

---

### Task 3: `importAuth()` 弹文件选择框，默认选中默认路径

**Files:**
- Modify: `src/pages/AuthChannelsPage.tsx:53-57`（`importAuth` 函数体）
- Modify: `src/pages/AuthChannelsPage.tsx:2`（无新增 import 需要 — 用动态 `import()`）

**Interfaces:**
- Consumes: `authApi.defaultImportPath()`（Task 2）、`authApi.loginImport(provider, path)`、`@tauri-apps/plugin-dialog` 的 `open()`
- Produces: 更新后的 `importAuth` 流程：默认路径 → 弹框 → 取消 no-op / 选中导入

- [ ] **Step 1: 重写 `importAuth`**

替换 `AuthChannelsPage.tsx:53-57` 的整个 `importAuth` 函数体为：

```tsx
  const importAuth = async () => {
    let path: string | null = null;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      let defaultPath: string | undefined;
      try {
        defaultPath = await authApi.defaultImportPath();
      } catch {
        // 默认路径解析失败(无 home)时回退为不带 defaultPath 弹框,仍可手动选文件
      }
      path = await open({
        title: "选择 Codex auth.json 文件",
        filters: [{ name: "Codex auth", extensions: ["json"] }],
        multiple: false,
        defaultPath,
      });
    } catch {
      // 对话框不可用,忽略(保持旧行为直接读默认路径)
    }
    if (path === null) return; // 用户取消 → 静默 no-op
    const label = path; // 实际选中路径
    setPendingId("import"); setNotice({ kind: "success", message: `正在读取 ${label} …` });
    try {
      const result = await authApi.loginImport("codex", path);
      setNotice(result.warning ? { kind: "warning", message: "账号已保存但暂不参与路由：模型同步失败。" } : { kind: "success", message: result.notice || `已从 ${label} 导入账号。` });
      await load();
    } catch (_) {
      setNotice({ kind: "error", message: "导入失败，请确认 auth.json 可读且字段完整。" });
    } finally { setPendingId(null); }
  };
```

> 注：`open()` 的 `defaultPath` 类型为 `string`（传 `undefined` 即不设默认）。对话框取消返回 `null`。`multiple: false` 时选中返回 `string`。

- [ ] **Step 2: 类型/编译验证**

Run: `npx tsc --noEmit`
Expected: 无类型错误（`@tauri-apps/plugin-dialog` 已在依赖）

- [ ] **Step 3: Commit**

```bash
git add src/pages/AuthChannelsPage.tsx
git commit -m "feat(auth): 导入 auth.json 弹文件选择框,默认选中默认路径"
```

---

### Task 4: 文档同步

**Files:**
- Modify: `docs/auth-codex/01-ui-spec.md:112`
- Modify: `docs/auth-codex/work/02-design.md:272`

**Interfaces:**
- Consumes: 已完成的实现（Task 1-3 的行为）

- [ ] **Step 1: 更新 UI spec 导入 toast 文案**

`docs/auth-codex/01-ui-spec.md:112`：

```markdown
- **导入 auth.json**（⇧）：弹文件选择框，默认选中 `~/.codex/auth.json`（`$CODEX_HOME` 优先，与 ADR-2 C 一致）；选中后 toast「正在读取 <实际路径> …」（ADR-2 C / ADR-24）。
```

- [ ] **Step 2: 更新 design 文档导入流程**

`docs/auth-codex/work/02-design.md:272` 附近，将 `auth_login_import` 行更新为：

```markdown
| `auth_login_import` | provider/path 可省略；前端先弹文件选择框(默认路径来自 `auth_default_import_path`)，读选中文件，过期先 refresh |
| `auth_default_import_path` | 返回默认 auth.json 路径,供文件选择框 defaultPath |
```

- [ ] **Step 3: Commit**

```bash
git add docs/auth-codex/01-ui-spec.md docs/auth-codex/work/02-design.md
git commit -m "docs(auth): 同步导入文件选择器行为"
```

---

## Self-Review

- **Spec 覆盖**：D1（弹框+默认路径+取消 no-op）→ Task 3；D2（`auth_default_import_path` command）→ Task 1；D3（toast 实际路径）→ Task 3 Step 1；D4（文档）→ Task 4。错误处理（默认路径失败回退）→ Task 3 Step 1 的 catch。全部覆盖。
- **占位扫描**：无 TBD/TODO；所有步骤含完整代码。
- **类型一致性**：`authApi.defaultImportPath()` 返回 `Promise<string>`，Task 1 command 返回 `Result<String, String>`；`loginImport(provider, path)` 签名沿用现有；`open()` 返回 `string | null` 在 Task 3 与 Global Constraints 一致。
