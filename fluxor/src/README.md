# Fluxor

`Fluxor` 是一个轻量级、无冗余的 Mihomo (Clash.Meta) 内核管理面板与订阅生成系统。系统采用前后端一体化设计：Go 后端以 `//go:embed` 内嵌 Vue 3 前端构建产物，单体交付，支持自动化配置生成、透明代理（TProxy）管理与实时的内核状态监控。

---

## 核心特性

- **内核管理**：Mihomo 进程的启动、停止、状态查询与配置热重载（不中断长连接）。
- **订阅中心**：订阅链接的 CRUD 管理；支持**融合模式**（多订阅合并为一份配置）与**切换模式**（按订阅切换，含定时更新与健康检查）；自动生成内核可用的 `config.yaml` 并热重载应用。
- **实时监控**：基于 WebSocket 中转上传/下载速率、内存占用、内核日志流与连接历史。
- **透明代理 (TProxy)**：集成 nftables 透明代理与策略路由，支持源/目的例外 IP、端口过滤及本机出站代理开关，退出时自动清退规则。
- **节点质量评分**：依据内核记录的历史延迟数据，按「延迟 / 稳定性 / 成功率」三项加权给出 0–100 评分。
- **外部面板集成**：内置 MetaCubeXD 与 Zashboard 静态资源托管，可一键切换。

---

## 技术栈

| 层 | 技术 |
|----|------|
| 后端 | Go 1.27（标准库），唯一外部依赖 `gorilla/websocket` v1.5.3 |
| 前端 | Vue 3.5（Composition API）+ TypeScript 5.9 |
| 状态管理 | Pinia 4 |
| 构建工具 | Vite 8（Rolldown）+ vue-tsc |
| 样式 | Tailwind CSS 4（CSS-first 配置，`@tailwindcss/vite` 插件） |
| 国际化 | vue-i18n 11 |
| 图标 | @vicons/ionicons5 |
| 文档站 | VitePress（`docs/`） |

> 后端按功能域拆分为 `backend/internal/` 下的多个包，包结构与依赖方向详见 [AGENTS.md](AGENTS.md) 的「2.1 后端包结构」章节。

---

## 快速开始

### 环境要求

| 依赖 | 版本要求 | 说明 |
|------|----------|------|
| Go | 1.27+ | `backend/go.mod` 声明 `go 1.27` |
| Node.js | ≥ 22.12.0 | 见 `frontend/package.json` 的 `engines` |
| GNU Make | 4.x | 仅用于统一构建入口 |
| Mihomo | 任意近期版本 | 运行时需要，用于实际代理（构建不需要） |

### 构建

```bash
# 完整构建：安装依赖 → 构建前端 → 同步 dist → 编译后端，输出 ./fluxor
make

# 运行构建产物
./fluxor
```

`make` 会自动按依赖顺序补齐所有前置阶段，**无需手动分步执行**。

### 常用命令

```bash
make help       # 查看全部可用目标

make            # 完整构建，输出 ./fluxor
make frontend   # 仅构建前端 → frontend/dist
make sync       # 仅同步 frontend/dist → backend/dist
make backend    # 同步并编译后端 → ./fluxor
make deps       # 仅安装前端依赖
make run        # 以前台方式运行后端（自动确保 dist 已同步）
make dev        # 启动前端热更新开发服务器
make fmt        # 格式化后端 Go 代码
make vet        # 后端静态检查
make check      # gofmt 校验 + go vet + 编译（CI 友好）
make clean      # 清理构建产物
```

### 构建变量

可在命令行覆盖，例如 `make backend GOOS=linux GOARCH=arm64 BIN=fluxor-arm64`：

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `GOOS` / `GOARCH` | 跟随本机 | Go 交叉编译目标 |
| `GOFLAGS` | `-ldflags="-s -w"` | Go 构建附加参数 |
| `BIN` | `fluxor` | 输出二进制路径 |
| `GO` / `NPM` | `go` / `npm` | 工具链命令 |
| `NPM_INSTALL` | `npm install` | 依赖安装命令；CI 可设为 `npm ci` |

> 切换 `GOOS`/`GOARCH` 时会自动重新编译，不会复用其他架构的旧二进制。

### 构建流程与增量

```
deps ──> frontend ──> sync ──> backend
npm install   npm run build   cp -r   go build
```

各阶段通过产物目录内的戳记文件判断是否需要重跑，因此重复执行 `make` 会跳过已完成的阶段：

| 戳记 | 触发条件 |
|------|----------|
| `frontend/node_modules/.fluxor-install-stamp` | `package.json` / `package-lock.json` 变更 |
| `frontend/dist/.fluxor-build-stamp` | `frontend/src`、`index.html`、`vite.config.*`、`tsconfig*.json` 变更 |
| `backend/dist/.fluxor-sync-stamp` | `frontend/dist` 更新 |
| `.fluxor.target` | `GOOS`/`GOARCH` 变更（触发重新编译） |

删除对应产物目录（或执行 `make clean`）即会自动触发重建。

---

## 本地开发

### 前端热更新

```bash
make deps      # 安装依赖
make dev       # 启动 Vite 开发服务器
```

前端内置离线 Mock 模拟器（`frontend/src/utils/mock.ts`），可脱离 Go 后端独立联调，仅在 Vite 开发模式（`import.meta.env.DEV`）下生效，生产构建中会被整体摇树移除。控制台可通过 `localStorage.setItem('MOCK_BACKEND', 'true'|'false')` 强制开关。

### 后端开发

```bash
make sync          # 确保 backend/dist 存在（//go:embed 需要）
make run           # 前台运行后端
```

修改前端代码后需重新执行 `make`（或 `make sync`）以刷新内嵌产物，因为前端资源是**编译期内嵌**的。

### 跨平台构建

Linux / macOS 直接使用 `make`。Windows 下可使用 `build_linux.cmd`，它以相同流程完成前端构建、dist 同步与 Linux amd64 交叉编译。

---

## 运行时配置

### 运行模式

| 模式 | 启用方式 | 监听 | 主要路径 |
|------|----------|------|----------|
| `fnos`（默认） | 无参数或 `-f` / `--fnos` | Unix Socket `/var/apps/Fluxor/target/app.sock` | `/var/apps/Fluxor/…` |
| `openwrt` | `-w` / `--openwrt` | TCP `0.0.0.0:18080` | `/etc/fluxor/…` |

### 命令行参数

```
-w, --openwrt   以 OpenWrt 模式运行
-f, --fnos      以 fnos 模式运行（默认）
-a, --addr      指定 TCP 监听地址（优先级高于模式默认值）
```

### 环境变量

所有路径均可通过环境变量覆盖，优先级高于模式默认值：

`SOCKET_PATH`、`BASE_URL`、`FLUXOR_ADDR`、`FLUXOR_PID_FILE`、`FLUXOR_BIN_DIR`、`CORE_PID_FILE`、`CORE_BIN`、`CORE_SOCKET`、`META_DIR`、`ZASH_DIR`、`FLUXOR_CONFIG_FILE`、`CONFIG_TARGET`、`INFO_LOG_FILE`、`CORE_WORK_DIR`

### 默认端口

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `proxy_port` | 7890 | 混合代理端口（mixed-port） |
| `tproxy_port` | 7898 | TProxy 透明代理端口 |
| `panel_port` | 9090 | 外部控制面板端口（external-controller） |

配置文件为 `fluxor.json`（fnos 模式下位于 `/var/apps/Fluxor/var/fluxor.json`），字段说明见 [docs/config/fluxor-json.md](docs/config/fluxor-json.md)。

---

## 项目结构

```
fluxor/
├── Makefile          # 统一构建入口
├── backend/          # Go 后端（独立 module）
│   ├── main.go       #   入口：参数解析、路由注册、监听、优雅退出、go:embed
│   └── internal/     #   按功能域拆分的实现（config/core/tproxy/subscription/…）
├── frontend/         # Vue 3 + Vite 前端源码，构建产物内嵌进后端
├── docs/             # VitePress 文档站
└── build_linux.cmd   # Windows 下的 Linux amd64 构建脚本
```

---

## 文档

- **[AGENTS.md](AGENTS.md)** — 系统架构、后端包结构与依赖方向、API 路由对照表、前后端通信机制、TProxy 安全规约、前端开发约束。
- **[docs/](docs/index.md)** — 用户文档站：快速开始、功能说明、配置详解与 FAQ。

---

## 许可

本项目采用 [LICENSE](LICENSE) 中声明的许可协议。
