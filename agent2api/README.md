# agent2api — 本地 OpenAI 兼容 API 网关（v3.5.0-4 · x86）

飞牛 fnOS 原生应用封装：把多家 AI 桌面客户端的登录态包装成本地 OpenAI 兼容 API 网关，自带管理面板。

## 上级项目

- **上游**: [aimod-cc/agent2api](https://github.com/aimod-cc/agent2api) v3.5.0-beta.4
- **发布者**: Mike（fnOS 打包发行，非上游开发者）
- **形态**: 上游官方服务端二进制 `agent2api-server`（Rust，单文件）+ 内嵌管理面板 `ui/`，**不 fork 源码**（闭源发行包，仓库只维护 fnOS 打包层）

## 功能特性

- **登录态 → API**：把桌面客户端（Claude Code / Codex / Gemini CLI / Cursor / Qoder 等代理型客户端）的登录态转成本地 OpenAI 兼容 `/v1` 接口
- **多提供商**：内置数十家 provider 预设（anthropic / openai / deepseek / glm / kimi / qwen / grok / openrouter …）
- **网关 + 面板**：`/v1` 网关与 `/api/*` 管理面同端口；可选拆独立面板端口
- **逐把 Key 管控**：面板创建的 API Key 可绑定模型 / 提供商白名单
- **多语言面板**：zh / en / ja / ko / pt-BR / zh-Hant

## 打包形态

```ini
appname               = Agent2API
version               = 3.5.0-4
install_type          = root          # run-as package，服务以包身份运行
service_port          = 3065
maintainer            = aimod-cc      # 上游 owner
distributor           = Mike-hd123
desktop_applaunchname = Agent2API.Application
```

- **双通道入口**：桌面 iframe 直连 `3065`（`app/ui/config` 走 `port` 模式，无 gatewaySocket）+ 手机 `/app/Agent2API`
- **数据落 @appdata**（`TRIM_PKGVAR`）：SQLite 库 + 配置 + 日志，`AGENT2API_PROXY_HOME` 指向 `TRIM_PKGVAR`
- **生命周期**：`cmd/` 9 脚本齐全；`cmd/main` 起 `${TRIM_APPDEST}/server/agent2api-server`
- **卸载双选**：`wizard/uninstall` 的 `wizard_data_action`（keep / delete，默认保留）
- **免鉴权路径**：`disable_authorization_path = true`（面板自带登录/注册，不走 fnOS 网关鉴权）

## 目录结构（本仓库）

```
agent2api/
├── ICON.PNG / ICON_256.PNG
├── manifest                  # appname/version/changelog/checksum
├── cmd/                      # 9 个生命周期脚本（install/config/main/uninstall/upgrade × init/callback）
├── config/privilege, config/resource
├── wizard/install, wizard/config, wizard/uninstall
├── app/ui/config             # 桌面入口（iframe + port 模式）
├── app/ui/images/            # 入口图标
├── AGENT.md                  # agent 须知
└── README.md                 # 本文件
```

> **二进制与 fpk 不入库**：`server/agent2api-server`（36MB）与 `Agent2API.fpk`（23.8MB）走 GitHub Release 分发，沿用本仓库「fpk 不进 git 跟踪」规矩（见根 [AGENT.md](../AGENT.md)）。构建时把上游发行包的程序**落到打包目录 `app/server/`**（fnpack 只收 `app/` 下的程序/资源，根级 `server/` 不入包）后 `fnpack build`。

## 版本口径

上游版本 → `<上游版本>-N`（本地修订从 1 起，禁止改上游版本号）。当前 **3.5.0-4** 对应上游 v3.5.0-beta.4：

- beta.2 → beta.3：修复局域网 HTTP 添加例外无反应、修复 SOLO 并发数误显示为积分、WorkBuddy 凭据区分国内外版、插件目录增量扫描、请求体过大返回可读尺寸说明
- 分发者由 shuangji66 改为 Mike-hd123（本源接管分发）

## 本机集成

- 网关地址 `http://127.0.0.1:3065/v1`，面板 `http://127.0.0.1:3065`
- DSH 的 LLM 路由走本网关（`agent2api(3065)` 组 chat/default/compression 别名；octopus 已 2026-11-05 退役由本应用接管）
- 网关 API Key 走凭证库 `~/.dsh/credentials-vault/hermes.env`，禁入日志/会话
