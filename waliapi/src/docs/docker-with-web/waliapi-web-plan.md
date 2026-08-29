# WaLiAPI Web 管理面板实施计划

## 1. 背景与目标

当前 Docker 部署中管理面板通过 VNC + noVNC 暴露桌面 UI，使用不便。本计划在**不影响桌面端**的前提下，新增纯 Web 管理面板：

- **功能范围**：全部 9 个页面 1:1 复刻（仪表盘、使用、渠道、Auth 账号、API 密钥、日志、服务/知识库/Wiki/MCP、设置、应用配置）
- **后端**：新增 `/admin/api/*` REST 接口，复用 `commands/*.rs` 业务逻辑；新增 `/admin/api/events` SSE 事件桥
- **前端**：`web/` 为独立 pnpm workspace 子包，**复用 `src/` 源码**，仅替换 `lib/api.ts` 为 HTTP fetch 实现
- **部署**：`rust-embed` 将 `web/dist` 内嵌到 waliapi 二进制，axum 直接 serve，无需 VNC/noVNC/fluxbox
- **认证**：SQLite `admin_users` 表 + argon2 密码哈希；首次启动生成随机临时密码（打印日志 + 写入 `/data/INITIAL_PASSWORD`），首次登录强制改密

## 2. 现状分析结论

- 前端 `src/` 通过 `@tauri-apps/api` 的 `invoke()` 调用约 100 个 Tauri command，无浏览器可直接访问的 HTTP 接口
- 后端 axum 服务器已暴露：`/v1/*`（LLM 协议）、`/api/kb/*`、`/api/wiki/*`、`/mcp`、`/health`
- 管理面板相关的 command（渠道、密钥、日志、设置、仪表盘、Auth、安全规则、导入导出、应用配置）**未暴露 HTTP**
- Tauri 专用能力：`plugin-updater`（App.tsx/UpdateChecker）、`plugin-dialog`（AuthChannelsPage/KnowledgeBasePage 文件选择）、`plugin-opener`（Sidebar 外链）、`api/event`（KnowledgeBasePage 4 处进度监听）
- 数据库已有 23 个迁移，新增 `024_admin_auth.sql` 即可承载管理员账户

## 3. 总体架构

```
浏览器 (Web 面板)
    │ HTTPS
    ▼
nginx (docker-compose, :8443)
    │ /admin/api/*  → waliapi:8777
    │ /*            → waliapi:8777 (内嵌静态资源)
    ▼
axum (src-tauri/src/server/router.rs)
    ├─ /v1/*              LLM 网关协议（已有）
    ├─ /api/kb/*          知识库（已有，Web 直接复用）
    ├─ /api/wiki/*        Wiki（已有，Web 直接复用）
    ├─ /mcp               MCP Server（已有）
    ├─ /admin/api/*       管理 REST API（新增，需认证）
    │   └─ /events        SSE 事件桥（新增）
    └─ /*                 rust-embed serve web/dist（新增，SPA fallback）
```

**Docker 简化**：运行时镜像可移除 `xvfb/x11vnc/novnc/websockify/fluxbox/libwebkit2gtk` 等 GUI 依赖，大幅减小镜像。但为兼容桌面构建，`Dockerfile` 中保留 builder 阶段的 webkit 依赖（Tauri 编译需要）。

## 4. 数据库迁移

**文件**：`src-tauri/migrations/024_admin_auth.sql`

```sql
CREATE TABLE IF NOT EXISTS admin_users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    salt TEXT NOT NULL,
    must_change_password INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

**初始化逻辑**（`db/mod.rs` 迁移后执行）：
- 查询 `admin_users`，若为空：生成随机用户名 `admin` + 16 位随机密码
- argon2 哈希后入库，`must_change_password = 1`
- 临时密码打印到 stdout 日志 + 写入 `$XDG_DATA_HOME/INITIAL_PASSWORD`（容器内即 `/data/INITIAL_PASSWORD`）

## 5. 后端实施（src-tauri）

### 5.1 Cargo.toml 新增依赖

```toml
argon2 = "0.5"
rand = "0.9"          # 已有
rust-embed = { version = "8", optional = true }

[features]
embed-web = ["dep:rust-embed"]
```

### 5.2 认证模块 `src-tauri/src/server/admin_auth.rs`

- `hash_password(password) -> (hash, salt)`：argon2id，随机 salt
- `verify_password(password, hash, salt) -> bool`
- `generate_token() -> String`：UUID v4
- Session 存储：内存 `DashMap<token, (user_id, expiry)>`，7 天过期，重启失效
- `require_auth` axum middleware：校验 `Authorization: Bearer <token>` 或 Cookie `waliapi_admin_token`
- 豁免路径：`/admin/api/auth/login`、`/health`、`/v1/*`（走 API Key 认证）

### 5.3 管理 REST 路由 `src-tauri/src/server/admin_routes.rs`

将 commands 逻辑抽出为可复用的 `pub(crate) async fn xxx_impl(state, input)`，HTTP handler 与 Tauri command 共用。路由前缀 `/admin/api`：

| 方法 | 路径 | 对应 Tauri command |
|---|---|---|
| POST | /auth/login | （新）校验密码，返回 token |
| POST | /auth/logout | （新）清除 session |
| POST | /auth/change-password | （新）修改密码，清除 must_change_password |
| GET | /auth/check | （新）返回当前用户 + must_change_password 标志 |
| GET | /channels | get_channels |
| GET | /channels/:id | get_channel |
| POST | /channels | create_channel |
| PUT | /channels/:id | update_channel |
| POST | /channels/:id/toggle | toggle_channel |
| DELETE | /channels/:id | delete_channel |
| POST | /channels/:id/test | test_channel |
| POST | /channels/test-draft | test_channel_draft |
| POST | /channels/sync-models | sync_upstream_models |
| GET | /channels/stats | get_channel_stats |
| POST | /channels/reorder | reorder_channels |
| GET | /channels/presets | get_channel_presets |
| GET | /channels/:id/extra-keys | get_channel_extra_keys |
| POST | /channels/extra-keys/:keyId/toggle | toggle_channel_extra_key |
| DELETE | /channels/extra-keys/:keyId | delete_channel_extra_key |
| GET | /api-keys | get_api_keys |
| POST | /api-keys | create_api_key |
| PUT | /api-keys/:id | update_api_key |
| DELETE | /api-keys/:id | delete_api_key |
| GET | /api-keys/stats | get_api_key_stats |
| GET | /logs | get_logs（query 传 limit/offset/keyword 等） |
| GET | /logs/:id | get_log |
| GET | /logs/:id/security | get_log_security_findings |
| GET | /logs/stats | get_log_stats |
| DELETE | /logs/:id | delete_log |
| POST | /logs/delete-before | delete_logs_before |
| POST | /logs/delete-all | delete_all_logs |
| GET | /auth-accounts | auth_accounts_list |
| POST | /auth-accounts/login | auth_login |
| POST | /auth-accounts/login/start | auth_login_start |
| GET | /auth-accounts/login/status/:sessionId | auth_login_status |
| POST | /auth-accounts/login/cancel | auth_login_cancel |
| POST | /auth-accounts/login/import | auth_login_import |
| GET | /auth-accounts/default-import-path | auth_default_import_path |
| POST | /auth-accounts/:id/logout | auth_logout |
| POST | /auth-accounts/:id/refresh | auth_refresh_token |
| POST | /auth-accounts/:id/sync-models | auth_sync_models |
| POST | /auth-accounts/:id/export | auth_export_json |
| POST | /auth-accounts/:id/toggle | auth_toggle |
| GET | /auth-accounts/:id/quota | auth_quota_status |
| PUT | /auth-accounts/:id | auth_update |
| GET | /dashboard/stats | get_dashboard_stats |
| GET | /settings | get_settings |
| PUT | /settings | save_settings |
| GET | /settings/feature-flags | get_feature_flags |
| GET | /server/status | get_server_status |
| POST | /server/restart | restart_server |
| GET | /security/builtin-rules | get_builtin_security_rules |
| PUT | /security/builtin-rules/:id | update_builtin_security_rule |
| DELETE | /security/builtin-rules/:id | delete_builtin_security_rule |
| POST | /security/builtin-rules/reset | reset_builtin_security_rules |
| GET | /security/custom-rules | get_custom_security_rules |
| POST | /security/custom-rules | create_custom_security_rule |
| POST | /security/custom-rules/:id/toggle | toggle_custom_security_rule |
| DELETE | /security/custom-rules/:id | delete_custom_security_rule |
| GET | /services/statuses | get_service_statuses |
| POST | /import-export/export-channels | export_channels |
| POST | /import-export/import-walicode | import_walicode_backup |
| POST | /import-export/import-waliapi | import_waliapi_export |
| POST | /import-export/scan-local | scan_local_ai_configs |
| POST | /import-export/import-scanned | import_scanned_sources |
| GET | /events | SSE 事件桥（见 5.5） |

**注意**：KB `/api/kb/*` 与 Wiki `/api/wiki/*` 已存在，Web 前端直接调用，**无需**在 `/admin/api` 下代理。

### 5.4 静态资源内嵌 `src-tauri/src/server/static_assets.rs`

```rust
#[cfg(feature = "embed-web")]
#[derive(rust_embed::RustEmbed)]
#[folder = "../web/dist"]
struct WebAssets;

pub fn static_router() -> Router<SharedState> {
    Router::new().fallback(get(serve_embedded))
}

async fn serve_embedded(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    // SPA fallback：未匹配到文件时返回 index.html
    let asset = WebAssets::get(path).or_else(|| WebAssets::get("index.html"));
    match asset {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
```

- 在 `router.rs` 中 `.merge(static_assets::static_router())`，**放在所有 API 路由之后**
- 无 `embed-web` feature 时 fallback 返回 404，不影响桌面开发

### 5.5 SSE 事件桥 `src-tauri/src/server/event_bridge.rs`

- AppState 新增 `event_tx: tokio::sync::broadcast::Sender<AdminEvent>`
- `AdminEvent { event: String, payload: serde_json::Value }`
- 修改现有 `app.emit("kb-import-progress", ...)` 等 4 处（knowledge_base.rs / importer.rs / index 构建处），同时 `state.event_tx.send(...)`
- `/admin/api/events` handler：订阅 broadcast，转 SSE 流，事件名与桌面端一致（`kb-import-progress`、`kb-index-progress`、`kb-document-progress` 等）

### 5.6 Tauri 专用能力的 HTTP 降级

| 桌面功能 | Web 版处理 |
|---|---|
| 应用更新检查 | `/admin/api/*` 不暴露 updater；前端隐藏入口 |
| 文件选择对话框（auth.json 导入） | 改为 `<input type="file">` 上传，`POST /admin/api/auth-accounts/login/import` 接收 JSON body |
| 导出文件保存 | 返回文件内容 + `Content-Disposition: attachment`，浏览器触发下载 |
| 打开配置文件夹 | 前端隐藏按钮 |
| 应用配置（8 款工具） | 仅返回 `available: false`，前端置灰并提示「仅桌面版可用」 |
| 系统托盘 / 开机自启 | 设置页隐藏相关开关 |

## 6. Web 前端实施（web/ 目录）


### 6.1 目录结构

```
web/
├── package.json        # name: waliapi-web, workspace 子包
├── vite.config.ts      # dev 代理 /admin/api + /api + /v1 → http://127.0.0.1:8777
├── tsconfig.json
├── index.html
└── src/
    ├── main.tsx        # 入口，渲染 App
    ├── App.tsx         # 复用 src/App.tsx，删除 updater，新增登录守卫
    ├── lib/
    │   ├── tauri-shim.ts        # 替换 @tauri-apps/api/core
    │   ├── tauri-event-shim.ts  # 替换 @tauri-apps/api/event
    │   └── auth.ts              # 登录/登出/token 管理
    └── (pages/components/hooks/types 通过 vite alias 指向 ../../src/...)
```

### 6.2 vite.config.ts 关键配置

通过 vite alias 把 `@tauri-apps/api/core` 和 `@tauri-apps/api/event` 替换为本地 shim，`src/lib/api.ts` 等现有代码零改动直接复用。dev server 代理 `/admin/api`、`/api`、`/v1` 到 `http://127.0.0.1:8777`。

### 6.3 Tauri shim web/src/lib/tauri-shim.ts

`invoke<T>(cmd, args)` 内部调用 `POST /admin/api/invoke`，body 为 `{ cmd, args }`。后端单一入口按 cmd 分发到内部函数。401 时跳转 `/login`。

**优势**：与 Tauri invoke 语义 1:1 对应，避免维护 60+ 条 REST 路径映射。

### 6.4 EventSource 封装 web/src/lib/tauri-event-shim.ts

与 `@tauri-apps/api/event` 的 `listen` 签名一致，内部用 EventSource 连接 `/admin/api/events`，按事件名分发。

### 6.5 登录页与守卫

- 新增 web/src/pages/LoginPage.tsx：用户名 + 密码表单，调用 POST /admin/api/auth/login，成功后存 token 到 localStorage
- App.tsx 新增 RequireAuth 组件：无 token 时跳转到 /login
- 首次登录后若 must_change_password = true，强制跳转 /change-password
- 修改密码页：输入新密码（确认两次），调用 POST /admin/api/auth/change-password

## 7. Docker 集成

### 7.1 Dockerfile 修改

builder 阶段：进入 /app/web 执行 pnpm install 和 pnpm build，再回 /app 执行 pnpm tauri build --no-bundle -- --features embed-web

runtime 阶段：

- 移除 xvfb x11vnc novnc websockify fluxbox libwebkit2gtk-4.1-0 libgtk-3-0 librsvg2-2 libayatana-appindicator3-1 dbus-x11 等 GUI 依赖
- 移除 DISPLAY、GDK_BACKEND、WEBKIT_DISABLE_*、LIBGL_ALWAYS_SOFTWARE 等环境变量
- 保留 ca-certificates curl libssl3 fonts-noto-cjk
- 仅暴露 8777 端口，移除 5900/6080
- WALIAPI_ENABLE_UI 默认 0，WALIAPI_HIDE_WINDOW 默认 1

### 7.2 docker-compose.yml 修改

- waliapi 服务 ports 改为 8777:8777，移除 6080
- nginx default.conf 中 location / 反代到 http://waliapi:8777
- 新增 volume 挂载 ./data:/data，确保 /data/INITIAL_PASSWORD 持久化

## 8. 实施顺序

1. **数据库迁移**：新增 024_admin_auth.sql + 初始化逻辑
2. **后端认证**：admin_auth.rs + middleware + /auth/* 路由
3. **后端 invoke 入口**：/admin/api/invoke 单一入口，按 cmd 分发到 commands 函数
4. **后端 SSE 桥**：event_bridge.rs + /admin/api/events
5. **后端静态资源**：static_assets.rs + rust-embed + router fallback
6. **Web 前端骨架**：web/package.json、vite.config.ts、index.html、main.tsx
7. **Web 前端适配层**：tauri-shim.ts、tauri-event-shim.ts、auth.ts
8. **Web 前端页面**：LoginPage、ChangePasswordPage、App.tsx 登录守卫
9. **Docker 集成**：Dockerfile 修改 + docker-compose.yml 修改
10. **验证**：本地 dev 代理测试 → Docker 构建测试 → 首次登录改密流程

## 9. 验证方案

### 9.1 本地开发

```bash
# 终端 1：启动后端（带 embed-web）
cd src-tauri && cargo run --features embed-web

# 终端 2：启动 web dev server
cd web && pnpm dev
```

访问 http://localhost:1420，验证登录、渠道 CRUD、日志查看、KB 进度 SSE。

### 9.2 Docker 构建

```bash
docker build -t waliapi:web .
docker run -p 8777:8777 -v waliapi-data:/data waliapi:web
```

访问 http://localhost:8777，验证：

- 首次启动日志输出临时密码
- /data/INITIAL_PASSWORD 文件存在
- 登录后强制改密
- 改密后正常访问仪表盘
- 渠道 CRUD、日志查看、KB 进度实时更新

### 9.3 桌面端回归

- cargo build（无 embed-web feature）编译通过
- pnpm tauri dev 启动正常，invoke 调用不受影响

## 10. 风险与缓解

| 风险 | 缓解措施 |
|---|---|
| rust-embed 路径在 Docker 构建时不存在 | Dockerfile 中确保 web/dist 先于 cargo build 生成 |
| SSE 事件在高并发下丢失 | broadcast channel 容量设 100，前端断线自动重连 |
| 首次部署忘记查看日志导致无法登录 | INITIAL_PASSWORD 文件同时写入 /data，可通过 docker exec 查看 |
| Web 版与桌面版 UI 不一致 | 复用 src/ 源码，仅替换 invoke 底层实现，UI 100% 一致 |
| /admin/api/invoke 暴露过多命令 | middleware 统一鉴权，且仅暴露 commands 中已定义的函数，无额外风险 |

## 11. 交付物清单

### 新增文件

- src-tauri/migrations/024_admin_auth.sql
- src-tauri/src/server/admin_auth.rs
- src-tauri/src/server/admin_routes.rs
- src-tauri/src/server/event_bridge.rs
- src-tauri/src/server/static_assets.rs
- web/package.json
- web/vite.config.ts
- web/tsconfig.json
- web/index.html
- web/src/main.tsx
- web/src/App.tsx
- web/src/lib/tauri-shim.ts
- web/src/lib/tauri-event-shim.ts
- web/src/lib/auth.ts
- web/src/pages/LoginPage.tsx
- web/src/pages/ChangePasswordPage.tsx

### 修改文件

- src-tauri/Cargo.toml（新增 argon2、rust-embed、mime_guess）
- src-tauri/src/server/router.rs（注册 admin_routes + static_assets）
- src-tauri/src/server/mod.rs（启动时初始化 admin_users）
- src-tauri/src/lib.rs（AppState 新增 event_tx）
- src-tauri/src/commands/knowledge_base.rs（emit 处同时发 broadcast）
- Dockerfile（builder 阶段构建 web，runtime 移除 GUI 依赖）
- docker-compose.yml（端口映射调整）
- package.json（新增 workspace: web）
- pnpm-workspace.yaml（新增 packages: [web]）

## 12. 文档输出

实施完成后，将本计划整理为用户向 Markdown 文档，保存到 docs/web-admin-panel.md，内容包括：

- 功能概述与架构图
- 构建与部署步骤（Docker 单容器 / docker-compose）
- 首次登录与密码修改流程
- 与桌面版的差异说明（Tauri 专用能力降级列表）
- 故障排查（无法登录、SSE 断连、静态资源 404）
