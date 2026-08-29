# auth.json 导入文件选择器设计

日期:2026-08-10

## 背景与动机

当前「从 auth.json 导入」按钮(`AuthChannelsPage.tsx:53-56`)不弹任何文件选择框,直接读取**默认路径**:

- 前端 `importAuth()` 调 `authApi.loginImport("codex")`(不带 path)
- 后端 `auth_login_import` → `import_path(None)` → 兜底到 `CodexLogin::default_auth_json_path()`(`$CODEX_HOME/auth.json` 优先,否则 `~/.codex/auth.json`)
- toast 硬编码「正在读取 ~/.codex/auth.json …」(`AuthChannelsPage.tsx:54`)

问题:用户无法从其它位置导入 auth.json(多账号、备份文件、其它机器同步过来的文件)。用户需求原话:**「从 auth.json 导入应该要用户自己选 json 文件。默认选中 ~/.codex/auth.json」**。

利好:文件对话框基础设施已齐备,改动小。

- `tauri-plugin-dialog` 已注册(`src-tauri/src/lib.rs:49`),capability `dialog:default` 已开
- 前端已有使用范例(`KnowledgeBasePage.tsx:1274`:动态 `import("@tauri-apps/plugin-dialog")`)
- 后端 `auth_login_import` 与 `authApi.loginImport` **已支持可选 `path` 参数**,无需改命令签名

## 决策

### D1:前端弹原生文件选择框,默认选中默认路径

点击「从 auth.json 导入」(页头按钮 + 空槽卡片按钮共用 `importAuth()`),流程改为:

1. `await authDefaultImportPath()`(`auth_default_import_path` command)→ 返回当前默认路径字符串
2. `open({ title: "选择 Codex auth.json 文件", filters: [{ name: "Codex auth", extensions: ["json"] }], multiple: false, defaultPath: <默认路径> })`
3. 用户取消(`selected === null`)→ **静默 no-op**,不弹任何 toast
4. 用户选中 → `authApi.loginImport("codex", selected)`

### D2:新增 `auth_default_import_path` command(Rust)

几行透传,不读文件、不读密钥:

```rust
#[tauri::command]
pub async fn auth_default_import_path() -> Result<String, String> {
    CodexLogin::default_auth_json_path()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(safe_error)
}
```

注册进 `lib.rs` 的 `invoke_handler`。

**路径逻辑单一来源在后端**:保持 `default_auth_json_path()`(CODEX_HOME 优先 → home 兜底)不分裂,前端不复制该逻辑。

### D3:toast 提示用实际选中路径

`importAuth()` 的 loading toast 与成功/失败文案不再硬编码「~/.codex/auth.json」,改用用户实际选中的路径(用户可能选了其它文件)。loading 文案:「正在读取 <路径> …」。

### D4:文档同步

- `docs/auth-codex/01-ui-spec.md:112`(ADR-2 C / ADR-24 引用处)的导入 toast 文案「正在读取 ~/.codex/auth.json …」→ 同步为「实际选中路径」。
- `docs/auth-codex/work/02-design.md:272` 的 `auth_login_import` 行:注明前端弹框 + 默认路径来自 `auth_default_import_path`。

## 不做什么(明确排除)

- **不改** `auth_login_import` 签名(已支持 path)
- **不改** 后端默认路径解析逻辑(CODEX_HOME 优先保持不变)
- **不做** Rust 侧弹对话框(tauri-plugin-dialog 有 Rust API,但与现有 JS 侧模式不一致,不采用)
- **不做** 多文件选择 / 批量导入(单一账号文件,multiple: false)

## 错误处理

- 默认路径解析失败(罕见,无 home)→ `auth_default_import_path` 返回错误,前端 catch 后回退为**不带 defaultPath** 直接弹框(仍可手动选文件)
- 导入失败(文件不可读 / 字段不全 / 令牌过期后刷新失败)→ 现有 `auth_login_import` 错误文案不变
- 对话框取消 → no-op

## 影响面

| 文件 | 改动 |
|---|---|
| `src/pages/AuthChannelsPage.tsx` | `importAuth()` 加弹框逻辑;toast 用实际路径 |
| `src-tauri/src/commands/auth.rs` | 新增 `auth_default_import_path` |
| `src-tauri/src/lib.rs` | `invoke_handler` 注册新 command |
| `src/lib/api.ts` | 新增 `authDefaultImportPath` 封装 |
| `docs/auth-codex/01-ui-spec.md` | toast 文案同步 |
| `docs/auth-codex/work/02-design.md` | 导入流程注明 |

无测试改动:Rust 侧仅新增透传 command;前端无测试基建。
