# Changelog

## v0.2.3 (2026-08-26)

- 🐛 **`/v1/models` 补全 Auth 账号模型**：模型列表接口此前仅聚合启用渠道（Channel）的 `models` 与 `model_mapping`，未包含 `auth_accounts` 登录账号同步的模型，导致「能路由却列不出」。现合并 auth 账号模型快照中 `available` 且未 `unavailable` 的条目及其 `model_mapping` 源别名，与渠道模型统一去重（渠道优先，`owned_by` 归属渠道；账号模型 `owned_by` 为 provider），OpenAI / Anthropic 两种响应格式均生效
- 📝 **README 文档完善**：更新代码贡献者信息表，补齐 v0.2.2 Docker / Web 管理面板贡献者 Fla1337，同步各贡献者最新提交量与代码变更统计
- 🔧 **版本号统一升级至 0.2.3**（package.json / Cargo.toml / tauri.conf.json / Cargo.lock）

## v0.2.2 (2026-08-26)

### Web 管理面板（Docker / headless 部署）

- ✨ **Linux headless 服务器部署**：新增 `waliapi-web` 二进制（无桌面窗口），支持 Docker 和 systemd 两种部署方式，适合放在 Linux 服务器上长期运行
- ✨ **Web 管理面板**：浏览器访问完整管理界面，与桌面版业务能力一致——仪表盘、渠道管理、密钥管理、日志审计、安全规则、知识库、Wiki、MCP、导入导出、应用配置等
- ✨ **多阶段 Docker 构建**：Node/pnpm 编译前端 → Rust 编译 `waliapi-server` → 运行时使用非 root 用户，SQLite 数据持久化到 `/data`
- ✨ **GitHub Actions 发布**：推送 `web-v*` 标签自动创建 Release、上传二进制包、发布 Docker 镜像到 GHCR
- ✨ **systemd 部署支持**：提供 systemd unit 文件和环境变量配置示例，适合不用 Docker 的场景
- ✨ **Web 管理面板用户设置**：支持修改管理员用户名和密码
- 🔧 **桌面版自动启动内嵌服务**：移除"随应用启动内嵌服务"开关，桌面版启动后自动运行 HTTP 服务
- 🔧 **后端重构分离桌面版与 Web 服务**：同一 Rust 代码库编译出桌面版（Tauri 窗口）和 headless 版（纯 HTTP 服务）

### Web 适配层修复

- 🐛 **`api.ts` 绕过 runtime 适配层**：`api.ts` 直接用 `@tauri-apps/api/core` 的 `invoke`，浏览器环境无 Tauri IPC 全部失败，改为统一走 `runtime.ts` 适配层
- 🐛 **`runtime.ts` 请求路径和格式不匹配后端**：修正 fetch 路径（`/api/admin/invoke` → `/admin/api/invoke`）、body 字段名（`command` → `cmd`）、响应解析逻辑、补齐 CSRF 头（`X-Requested-With`）、SSE 路径同步修正
- 🐛 **`default-run` 缺失导致 `cargo run` 报错**：`Cargo.toml` 有两个 binary（`waliapi` + `waliapi-web`），未设 `default-run`，补上 `default-run = "waliapi"`

### 流式请求超时修复（502 问题）

- 🐛 **流式请求被总超时掐断**：`reqwest` 的 `.timeout()` 是整个请求总超时（含 SSE 传输），大量对话时 LLM 生成时间超过 `timeout_secs`（默认 60s）连接被掐断，客户端收到 502
- 🔧 **分离流式/非流式超时策略**：新增 `streaming_client()`（仅 `connect_timeout` 10s，不设总超时）和 `blocking_client()`（`connect_timeout` + 总超时 `timeout_secs`），流式请求不再受总超时限制
- 🔧 **全链路覆盖**：5 个 adaptor（openai/claude/deepseek/gemini/custom）的 `forward_stream` + `endpoint_executor` + `handlers.rs` 的 `openai_messages_request` / `native_anthropic_request` + embeddings 全部切换到对应 client

### 模型映射编辑修复

- 🐛 **模型映射编辑输入丢失**：`useModelMappings` 的 `useEffect([initial])` 在每次 prop 变化时重置内部状态，`pairsToMapping` 丢弃 from/to 为空的不完整行后，`onChange → 父组件更新 → prop 变化 → useEffect 重置` 的循环把用户正在输入的数据吃掉。引入 `skipNextSyncRef` + `markSynced()` 跳过内部变更的 round-trip

### Codec 加固

- 🔧 **Chat store/stream_options 归一化**：归一化 Chat 请求的 `store` 和 `stream_options` 字段，合批 Responses 工具调用与 easy input
- 🐛 **thinking none/off 映射修复**：thinking 设为 none/off 时映射为 adaptive + low effort，不再报错
- 🐛 **`--help` 参数路由修复**：`--help` 在参数路由前拦截，恢复正常帮助文本和退出码 0

### Docker 构建修复

- 🐛 **Rust 基础镜像升级**：rust 1.88 → 1.96，notify-rust@4.18 要求 rustc ≥ 1.89
- 🐛 **Dockerfile.tp 兼容国内镜像**：新增国内镜像源构建变体，去掉 syntax 指令（tp 网络到不了 auth.docker.io）
- 🔧 **tauri.conf.json 显式指定 mainBinaryName**：修复构建时 binary 名称不确定的问题

### 其他

- 版本号统一升级至 0.2.2（package.json / Cargo.toml / tauri.conf.json）
- Cargo.toml 添加 `default-run = "waliapi"`

---

## v0.2.1 (2026-08-18)

### 协议转换层结构化重构

- 🔧 **protocol 模块目录化**：将 protocol 根转换逻辑拆分为独立子模块——codec/chat、codec/messages、codec/responses_codec、directions（messages_to_responses / responses_to_messages），每个方向独立 encode/decode/stream/test，消除 1500 行巨型文件
- 🔧 **死代码清理与 API 收敛**：清理 protocol 模块遗留 API 和死代码，clippy 告警归零，完成模块结构与 re-export 审计
- 🔧 **codec 加固**：移植 tool-call 回放保留空 reasoning_content 兼容性优化，修复测试编译问题，全仓 cargo fmt 格式化

### Kimi Code Auth 账号接入

- ✨ **Kimi 设备 OAuth 登录**：实现 Kimi 设备授权流程（device code → 授权 → token），支持 token 自动刷新
- ✨ **Provider 中立认证框架**：新增 provider metadata + model protocol snapshot，支持多登录方式扩展
- ✨ **认证路由集成**：model-level auth profiles 传入 prepared attempts，executor 注册 Kimi 认证尝试
- ✨ **登录会话管理**：provider-neutral login sessions and commands，通用 login context 与 locked replacement 持久化
- ✨ **协议感知模型发现**：Kimi 后端协议感知的模型发现与注册
- ✨ **前端 Auth 面板**：Kimi auth login UI + provider-aware accounts 页面
- 🐛 **402 订阅无效终态处理**：402 订阅无效分为终态，不再 12h 死循环重试
- 🐛 **令牌失效原因记录**：invalidation_reason 记录并透出到 DTO，失效账号卡片显示具体失效原因
- 🐛 **渠道页账号过滤修复**：渠道页按 provider 过滤账号卡片，不再混显
- ✅ **测试覆盖**：Kimi routing replacement refresh 与协议流程测试

### 审计日志流式响应修复

- 🐛 **流式响应内容记录修复**：流式请求的审计日志中 `response_choices` 字段此前始终为空，现已正确记录响应内容（content / reasoning_content / tool_calls），与非流式路径行为一致
- 🔧 **多协议流式累积**：新增 SSE 事件解析器，支持三种流式协议的响应内容累积
- 🔧 **StreamPumpCore 扩展**：新增 `accumulated_reasoning`、`response_role`、`finish_reason`、`tool_calls_map` 字段

### 其他

- 版本号统一升级至 0.2.1（package.json / Cargo.toml / tauri.conf.json）
- 121 个文件变更，+22,616 / -14,462 行代码

---

## v0.1.9 (2026-08-13)

- ✨ 渠道多 Key 负载均衡：单个渠道配置多个 API Key，按权重随机选择，分散并发压力
- ✨ 渠道复制快捷配置：一键复制现有渠道配置，快速创建相似渠道
- ✨ 审计日志自动刷新：页面可见时每 5 秒静默轮询，新日志自动出现，无需手动刷新
- ✨ 自动更新 Release Notes 动态化：从 CHANGELOG.md 自动提取版本说明

---

## v0.1.8 (2026-08-12)

- ✨ API 密钥黑白名单：密钥级别渠道+模型访问控制
- ✨ Auth 账号模型映射：`auth_accounts` 新增 `model_mapping_json` 列
- ✨ API Key 编辑功能：支持编辑密钥名称、配额、白/黑名单规则
- 🐛 路由优先级修复：关闭 `prefer_auth_accounts` 与 `prefer_same_protocol`
- ✨ Usage 密钥过滤：选中 API Key 后 MODEL 列表自动按白/黑名单过滤

---

## v0.1.7 (2026-08-09)

- ✨ Wiki 知识引擎：项目/页面/源文件三表结构，文档摄入管道，知识图谱，标签体系
- ✨ MCP Server 扩展：新增 16 个 Wiki MCP 工具，总数 13 → 29 个
- 🐛 SSE 字节级重组：修复 CJK 多字节边界帧泄漏问题
- 🐛 Responses 流式修复：handler 路径 SSE 帧重组 + reasoning 归属修复

---

## v0.1.6 (2026-08-08)

- ✨ 渠道协议大重构（T01–T14）：Provider preset registry、严格 codec、SSRF 防护、Provider 下拉组件等
- ✨ 渠道表单 URL 预览：端点下方实时展示实际请求 URL
- ✨ /v1/models 接口：聚合所有启用渠道的模型列表
- ✨ 数据库迁移备份：迁移前自动备份数据库，保留最近 3 份

---

## v0.1.5 (2026-08-03)

- ✨ 模型映射一对多：支持单目标→多目标数组映射
- 🐛 proxy.rs P0 修复：429/5xx 误返客户端，新增 failover 检查
- ✨ 渠道超时配置：`timeout_secs` 字段（默认 60s）
- 🐛 IME composing 修复、拖拽排序修复

---

## v0.1.4 (2026-07-30)

- ✨ 知识库引擎：文档解析 → tree-sitter 代码符号感知 → 智能分块 → 向量化 → HNSW 索引
- ✨ 混合检索：HNSW + FTS5 加权融合
- ✨ RAG 问答引擎 + MCP Server（13 个工具）
- ✨ 应用配置：一键写入 8 款 AI 编程工具
- ✨ 导入导出 + 应用更新检查

---

## v0.1.1 (2026-07-21)

- ✨ 多协议网关：OpenAI Chat + Responses + Anthropic Messages
- ✨ 仪表盘优化 + 渠道统计 + 接入示例页

---

## v0.1.0 (2026-07-18)

- 🎉 首发版本：多渠道管理 + 密钥管理 + 日志审计 + 安全审计 + SSE 流式
