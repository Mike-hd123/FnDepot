# Fluxor 项目 AI 代理指南 (AGENTS.md)

本文件为后续接手的 AI 编码助手或开发者提供项目的系统架构、核心逻辑、通信接口及开发规约，以便于快速理解项目并进行无缝维护与扩展。

---

## 1. 项目定位与架构概述

`Fluxor` 是一个轻量级、无冗余的 Mihomo 内核管理面板与订阅生成系统。它采用**前后端不分离**的架构设计：
- **后端 (Go)**：位于 `backend/` 目录，使用 Go 标准库（仅引入 `gorilla/websocket` 作为唯一外部依赖）。后端托管在 Unix Socket (`/var/apps/Fluxor/target/app.sock`) 上，对外通过前端反向代理暴露，内嵌了前端的所有静态资源。
- **前端 (Vue 3 TypeScript 版)**：位于 `frontend/` 目录，由 Vue 3 (Composition API / Setup) + Vite + TailwindCSS + Pinia + TypeScript 构成，是项目唯一且主维护的前端实现。
  - **前端构建与内嵌**：`frontend/` 使用 Vite 构建，产物输出到 `frontend/dist/`（含 `index.html` 模板与 `assets/` 静态资源）。通过 `make sync`（或完整 `make`）将 `frontend/dist` 同步到 `backend/dist/`，后端在 `backend/main.go` 中以 `//go:embed dist` 直接内嵌该产物。
  - **开发环境编译**：任何新功能或漏洞修复请在 Vue 3 版本中维护，修改前端代码后，需在 `frontend` 目录下执行 `npm run build`，并在项目根目录执行 `make` 完成整体构建。

### 核心功能职责

1. **内核进程生命周期管理**：负责本地 Mihomo 二进制文件的启动、停止、状态查询及配置热重载（不中断长连接）。
2. **配置文件订阅与生成**：读取用户的订阅链接及自定义规则集，生成内核可运行的 `config.yaml`。
3. **通信中转代理 (Bridge)**：由于 Mihomo 运行在本地 Unix Socket 上，Fluxor 后端作为前端与本地内核之间的“双向桥梁”，代理所有的 HTTP API 请求与 WebSocket 数据流（流量、内存、连接、日志等），并自动附加 `Bearer Token` 认证。

---

## 2. 目录结构与索引

```text
fluxor/
├── Makefile               # 统一构建入口：make / make frontend / make backend / make sync / make run / make clean
├── backend/               # Go 后端源码目录（独立 Go module，root 位于 backend/go.mod）
│   ├── main.go            # 程序入口：参数/环境变量解析、路由注册、监听启动、优雅退出，
│   │                      #   并以 //go:embed dist 内嵌前端构建产物
│   ├── internal/          # 按功能域拆分的后端实现（详见「2.1 后端包结构」）
│   ├── go.mod / go.sum    # Go module 定义与依赖
│   └── dist/              # 构建产物目录（由 `make sync` 从 frontend/dist 同步，已 gitignore）
└── frontend/              # 主维护 Vue 3 前端源码目录
    ├── package.json       # Vue 3.5 + Pinia 4 + vue-i18n 11 + Vite 8 (Rolldown) + Tailwind CSS 4 + TypeScript 5.9 + @vicons/ionicons5
    ├── vite.config.js     # 构建输出到 dist/（index.html + assets/），集成 @tailwindcss/vite 插件
    ├── index.html         # HTML 入口（Go 模板：{{.BaseHref}} / {{.RawBase}}）
    └── src/
        ├── main.ts        # 挂载 Pinia + vue-i18n (Composition API, legacy:false)
        ├── App.vue        # 根组件：响应式侧边栏/移动端底部 Tab、亮暗/跟随系统主题、中英切换、Toast 队列、Promise 确认框、统一轮询 coreStatus 状态
        ├── env.d.ts       # .vue 类型声明 & Window.BASE_URL 接口扩展
        ├── i18n.ts        # 全站国际化（zh/en），从 localStorage 读取语言偏好，禁止硬编码中文
        ├── index.css      # Tailwind v4 入口（@import/@source/@custom-variant/@theme/@utility，取代原 tailwind.config.js）
        │                  #   + CSS 变量亮暗主题（data-theme 选择器）+ 自定义滚动条
        ├── components/    # 公共及细粒度组件 (ProxyGroupCard, FormSwitch)
        ├── composables/   # 全局解耦组合式函数 (useTheme, useLanguage)
        ├── utils/
        │   ├── api.ts     # withBase() 拼接 BASE_URL、apiFetch() HTTP 封装、wsConnect() WebSocket 封装（自动 ws/wss 协议选择）
        │   └── mock.ts    # 前端离线开发模拟器：拦截 HTTP/WS 请求提供 mock 数据，支持脱离后端独立测试
        ├── store/
        │   ├── global.ts   # 标签页激活状态、侧边栏折叠、亮暗/跟随系统主题、Toast 队列（3s 自动消失）、Promise 驱动确认框
        │   ├── config.ts   # 内核常规配置参数（allow-lan/ipv6/mode/log-level/tun/端口等，通过 app.vue 统一轮询 coreStatus，与订阅解耦）
        │   ├── subscription.ts # 订阅管理 Pinia Store：负责订阅配置 CRUD、解析及状态更新，由 config.ts 中拆分解耦而来
        │   ├── overview.ts # 仪表盘实时统计（速度/流量/内存/连接数/版本/当前节点）、60 点流量历史、3 路 WS + 1 路 HTTP 轮询
        │   ├── proxies.ts  # 代理组列表、节点延迟字典、手风琴展开状态、并发受限（10）批量测速
        │   ├── connections.ts # 活跃/已关闭连接列表、汇总统计、排序/搜索、WS 瞬时速率计算（快照差分）
        │   ├── rules.ts    # 规则列表、规则提供商列表、fetch/refresh 方法
        │   └── logs.ts     # 日志缓冲区（上限 2000 条）、自动滚动、暂停/继续、指数退避重连（1s~30s）
        └── views/
            ├── Overview.vue     # 概览：4 指标卡（上传/下载速度+总量）+ 4 信息卡（内存/连接数/版本/外部面板）+ Canvas 自绘折线图（max 60 点）
            ├── Proxies.vue      # 代理：手风琴展开/折叠、点击切换选择、单节点/组/全部测速、延迟着色（绿≤150 / 黄≤300 / 红>300ms/超时）
            ├── Rules.vue        # 规则：搜索过滤、启用/禁用开关（乐观更新+回滚）、规则提供商单个/全部更新
            ├── Connections.vue  # 连接：活跃/已关闭双标签、多列排序、搜索过滤、单条/全部断开（乐观更新移入 closed）、清空已关闭
            ├── Logs.vue         # 日志：暗色终端风格、级别过滤（Debug/Info/Warning/Error）、搜索、暂停/继续、智能自动滚动
            ├── Config.vue       # 配置：内核状态卡（启动/停止/升级）、常规参数、端口校验（1025-65535+重复检测）、TUN（gVisor/System/Mixed）、高级运维（重载/清缓存/GEO）、内置 DNS 查询
            └── Subscription.vue # 订阅：代理/面板端口、密钥显隐切换、规则集（lite/base/full）、UI 面板选择、订阅 CRUD 模态框（zoomIn 动画，支持订阅名称、链接、检测间隔、节点前缀）、流量/健康度/有效期卡片、「保存并应用」
```

> **构建流程**：`make` → ① `frontend`：`npm run build` 输出到 `frontend/dist/`；② `sync`：拷贝至 `backend/dist/`；③ `backend`：在 `backend/` 内 `go build`（依赖 `//go:embed dist`）并输出二进制到项目根目录 `./fluxor`。

> **页面路由机制**：未使用 vue-router，通过 `globalStore.activeTab` 与 `<component :is="..." />` 动态组件切换视图。在此基础上，外层包裹了 `<KeepAlive :max="7">`（与视图总数一致，避免 LRU 淘汰引发的重挂）进行视图缓存，以长效留存页面各交互状态（如滚动进度与折叠状态）并规避切换页面时的重复连接请求。

### 2.1 后端包结构 (`backend/internal/`)

后端按**功能域**拆分为多个包，每个包内再按细分功能拆分为小文件。拆分原则：单一职责、依赖单向、禁止循环依赖。

```text
backend/
├── main.go                       # 唯一入口：参数/环境变量解析、路由注册、监听、信号处理、go:embed
└── internal/
    ├── config/                   # 【叶子】全局配置与持久化状态
    │   ├── doc.go                #   包说明
    │   ├── model.go              #   SubscribeConfig / Subscription 数据结构
    │   ├── state.go              #   Current（当前配置快照）+ Mu（读写锁）
    │   ├── paths.go              #   全部运行路径（Socket/PID/内核/面板/日志等）
    │   ├── modes.go              #   fnos / openwrt 两套默认路径
    │   ├── env.go                #   环境变量读取工具
    │   └── load.go               #   配置加载、默认值补齐、持久化
    ├── configgen/                # 【叶子】config.yaml 纯模板渲染（无状态、无网络）
    │   ├── generator.go          #   GenerateConfig / GenerateBaseConfig
    │   ├── template_base.go      #   基础字段骨架
    │   ├── template_dns.go       #   统一注入的 DNS 块
    │   ├── groups_lite.go        #   base 规则集的代理组
    │   ├── rules_lite.go         #   base 规则集的规则
    │   ├── groups_full.go        #   full 规则集的代理组（含 __SUB_NAMES__ 占位）
    │   ├── providers_full.go     #   full 规则集的 rule-providers
    │   ├── rules_full.go         #   full 规则集的规则
    │   └── names.go              #   订阅名称提取工具
    ├── httpx/                    # 【叶子】HTTP 通用工具
    │   ├── response.go           #   WriteJSONError / RespondJSON
    │   └── regex.go              #   外部面板后端地址校验正则
    ├── core/                     # 内核进程生命周期
    │   ├── client.go             #   CoreRequest + cancelableReadCloser（Context 回收）
    │   ├── lifecycle.go          #   启动/停止/热重载（重载后同步 TProxy 规则）
    │   ├── logger.go             #   内核操作日志记录器
    │   ├── tmpcore.go            #   DownloadWithTempCore：临时内核下载订阅节点文件
    │   └── handlers.go           #   /core/* HTTP 接口
    ├── tproxy/                   # TProxy 防火墙与策略路由
    │   ├── state.go              #   启用状态、例外缓存、读写锁
    │   ├── store.go              #   例外列表/本机代理开关的持久化
    │   ├── rules.go              #   规则解析 + nftables 应用/清理
    │   └── handlers.go           #   /config/tproxy* HTTP 接口
    ├── subscription/             # 订阅中心
    │   ├── api.go                #   /subscribe/config 读写
    │   ├── api_generate.go       #   /subscribe/generate
    │   ├── api_update.go         #   /subscribe/update/{name}
    │   ├── api_updateinfo.go     #   /subscribe/update-info/{name}
    │   ├── patch.go              #   向节点文件注入端口/密钥/DNS
    │   ├── ensure.go             #   切换模式下确保订阅文件就绪
    │   ├── generator.go          #   调用 configgen 生成 config.yaml
    │   ├── update.go             #   切换模式下的单个订阅更新流程
    │   ├── metadata.go           #   元数据抓取与键规范化
    │   ├── timers.go             #   定时更新/健康检查定时器生命周期
    │   ├── healthcheck.go        #   switch 模式下激活订阅的周期测速
    │   ├── panel.go              #   MetaCubeXD config.js 后端地址改写
    │   ├── fileutil.go           #   文件复制工具
    │   └── download/             #   【叶子】订阅下载层
    │       ├── download.go       #     直连优先、临时内核回退
    │       ├── direct.go         #     直接 HTTP 下载 + Base64 解析 + userinfo 头解析
    │       ├── validate.go       #     订阅内容有效性校验
    │       └── keys.go           #     map 键小写规范化
    ├── dashapi/                  # 内核 HTTP API 反向代理
    │   ├── dashboard.go          #   版本/流量/内存/连接
    │   ├── configs.go            #   配置读写、重载、GEO、缓存、DNS 查询
    │   ├── proxies.go            #   代理组、单节点测速、策略组测速
    │   ├── providers.go          #   订阅代理信息
    │   ├── rules.go              #   规则与规则提供商
    │   ├── connections.go        #   连接断开
    │   └── upgrade.go            #   内核升级
    ├── wsproxy/                  # WebSocket 双向桥接
    │   ├── handler.go            #   WsProxyHandler
    │   └── upgrader.go           #   全局 upgrader 配置
    ├── netinfo/                  # 网络信息查询
    │   ├── interfaces.go         #   网卡枚举 + 本地出口 IP 回退
    │   ├── iplookup.go           #   公网 IP 查询（JSON/纯文本兼容）
    │   ├── geo.go                #   IP 归属地查询（10 分钟缓存）
    │   ├── proxyport.go          #   从内核配置读取实际代理端口
    │   ├── addr.go               #   TCP 监听地址校验
    │   └── handlers.go           #   /ipinfo/* 与 /interfaces
    ├── delaytest/                # 连通延迟测试
    │   ├── probe.go              #   HEAD 探测实现
    │   └── handlers.go           #   /delaytest/* HTTP 接口
    ├── quality/                  # 节点质量评分
    │   ├── model.go              #   评分模型与权重常量
    │   ├── calculator.go         #   延迟/稳定性/成功率三项算法
    │   └── handler.go            #   /proxies/quality
    ├── appupdate/                # 版本检查与自更新
    │   ├── github.go             #   Release/Tag 查询与版本比较
    │   ├── cache.go              #   版本信息缓存（TTL 10 分钟）
    │   ├── coreversion.go        #   内核本地/远程版本比对
    │   ├── selfupdate.go         #   Fluxor 自更新（多加速源回退）
    │   └── handler.go            #   /check-update
    └── web/                      # 前端入口渲染
        ├── index.go              #   index.html 模板渲染
        └── whoami.go             #   /whoami
```

**依赖方向（严格单向，不得出现环）**：实际图如下，可用 `go list -f '{{.ImportPath}} {{.Imports}}' ./...` 复核。

```text
main ──> 所有 internal 包

【叶子层】 config     （无 internal 依赖）
           httpx      （无 internal 依赖）

【基础层】 configgen  ──> config
           tproxy     ──> config, httpx
           web        ──> config
           wsproxy    ──> config

【核心层】 core       ──> config, configgen, httpx, tproxy
           netinfo    ──> core, httpx

【业务层】 dashapi    ──> config, core, httpx, tproxy
           quality    ──> config, core, httpx
           delaytest  ──> netinfo, httpx
           appupdate  ──> config, httpx, netinfo
           subscription        ──> config, configgen, core, dashapi, httpx, subscription/download
           subscription/download ──> config, core
```

> **循环依赖规避要点（拆分时实际采用的三个手段）**：
> 1. **下沉叶子包**：`core` 需要生成配置文件，而 `subscription` 依赖 `core`。若把生成逻辑留在 `subscription` 就会与 `core` 形成环，故将纯字符串渲染逻辑抽成不依赖任何 internal 包的叶子包 `configgen`，由 `core` 与 `subscription` 共同依赖。
> 2. **同层共用工具下沉**：`subscription` 与其子包 `subscription/download` 都需要 map 键规范化，若留在父包则子包反向依赖父包，故下沉为 `subscription/download/keys.go`（`NormalizeMapKeys`）。
> 3. **依赖方向统一为单向链**：订阅相关的内核调用统一为 `subscription → core`；`core.DownloadWithTempCore` 只接收 `config.Subscription` 数据结构，不反向引用订阅包，因此不产生环。

> **新增后端代码时的约定**：先判断属于哪个功能域，再放入对应包；新增 HTTP Handler 放在该域的 `handlers.go` / `api*.go` 中；若新代码引入跨层调用，务必先确认不会形成依赖环——若会形成环，应把共用逻辑下沉为新的叶子包，或改为在同一方向上调用，切勿直接互相 import。


---

## 3. 前后端通信与代理机制 (重要规避点)

### 3.1 统一路由前缀 (BASE_URL)
所有的请求均有统一的基本路径前缀：`baseURL = "/app/Fluxor"`。
在 Vue 源码中，所有 `apiFetch` 或 WebSocket 通信必须调用 [api.ts](frontend/src/utils/api.ts)，它会自动且妥善地完成前缀拼接。

### 3.2 HTTP 代理流过早截断修复与 Context 释放
前端向后端发起管理请求时，后端通过 Unix Socket 拨号并发 Do(req) 请求内核。为了防止大 JSON 数据（例如代理组数据、连接历史）在传输中因超时 Context 被提前取消导致流被中断（抛出 `Unterminated string in JSON` 错误），后端在 [backend/internal/core/client.go](backend/internal/core/client.go) 中通过 `cancelableReadCloser` 实现了：
```go
type cancelableReadCloser struct {
	io.ReadCloser
	cancel context.CancelFunc
}
func (c *cancelableReadCloser) Close() error {
	err := c.ReadCloser.Close()
	c.cancel() // 确保数据全部读取拷贝完毕后，在 Close 时才会真正执行 cancel 释放资源
	return err
}
```
编写新的 API 代理 Handler 时，必须使用此包装类接管 Context 的回收。

### 3.3 后端 API 路由对照表（`main.go` 注册）

| 路由 | 方法 | Handler | 说明 |
|------|------|---------|------|
| `/` | GET | `web.HandleIndex` | SPA 主页模板渲染 |
| `/whoami` | GET | `web.HandleWhoAmI` | 获取当前用户信息 / 角色 |
| `/core/status` | GET | `core.HandleCoreStatus` | 内核运行状态（PID 文件检测） |
| `/core/start` | POST | `core.HandleCoreStart` | 启动内核进程 |
| `/core/stop` | POST | `core.HandleCoreStop` | 停止内核进程（SIGTERM） |
| `/core/restart` | POST | `core.HandleCoreRestart` | 热重启（重载配置） |
| `/upgrade` | POST | `dashapi.HandleUpgrade` | 升级内核（透传内核 /upgrade） |
| `/subscribe/config` | GET/POST | `subscription.HandleSubscribeConfigAPI` | 订阅配置读写（持久化到 subscribe.json） |
| `/subscribe/generate` | POST | `subscription.HandleGenerateConfig` | 保存配置 + 生成 config.yaml + 重载内核 |
| `/subscribe/update/{name}` | GET/POST | `subscription.HandleSubscribeUpdate` | 手动更新指定订阅节点数据 |
| `/subscribe/update-info/{name}` | POST | `subscription.HandleUpdateSubscriptionInfo` | 更新订阅元信息（名称、链接、检测间隔等） |
| `/traffic` | WS | `wsproxy.WsProxyHandler("/traffic")` | 实时流量数据 WebSocket 代理 |
| `/memory` | WS | `wsproxy.WsProxyHandler("/memory")` | 实时内存数据 WebSocket 代理 |
| `/logs` | WS | `wsproxy.WsProxyHandler("/logs")` | 实时日志流 WebSocket 代理 |
| `/connections` | WS/DELETE | `wsproxy.WsProxyHandler` / `dashapi.HandleConnectionsClose` | 连接实时流或全部断开 |
| `/connections/{id}` | DELETE | `dashapi.HandleConnectionsClose` | 断开指定连接 |
| `/version` | GET | `dashapi.HandleVersion` | 内核版本信息 |
| `/configs` | GET/PATCH/PUT | `dashapi.HandleConfigsAPI` | 获取/修改/重载内核配置 |
| `/configs/geo` | POST | `dashapi.HandleConfigsGeo` | 更新 GEO 数据库 |
| `/providers/geo` | POST | `dashapi.HandleProvidersGeo` | 回退 GEO 更新接口 |
| `/providers/rules` | GET | `dashapi.HandleRuleProviders` | 获取规则提供商列表 |
| `/providers/rules/{name}` | PUT | `dashapi.HandleUpdateRuleProvider` | 更新单个规则提供商 |
| `/interfaces` | GET | `netinfo.HandleInterfaces` | 返回系统物理接口名称（过滤回环及未启用接口） |
| `/providers/proxies/{name}` | GET/PUT | `dashapi.HandleProviderProxies` | 获取/更新订阅代理信息 |
| `/rules` | GET | `dashapi.HandleRules` | 获取所有规则 |
| `/rules/disable` | PATCH | `dashapi.HandleRulesDisable` | 启用/禁用规则 |
| `/proxies` | GET | `dashapi.HandleProxies` | 获取所有代理组 |
| `/proxies/{name}/delay` | GET | `dashapi.HandleProxyDelay` | 测速（需 ?url=&timeout= 参数） |
| `/proxies/{name}` | PUT | `dashapi.HandleProxySwitch` | 切换代理选择 |
| `/proxies/quality` | GET | `quality.HandleQualityScores` | 获取所有代理节点的质量分数评分列表 |
| `/cache/fakeip/flush` | POST | `dashapi.HandleFlushFakeIP` | 清空 FakeIP 缓存 |
| `/cache/dns/flush` | POST | `dashapi.HandleFlushDNS` | 清空 DNS 缓存 |
| `/dns/query` | GET | `dashapi.HandleDNSQuery` | DNS 查询（?name=&type=） |
| `/restart` | POST | `dashapi.HandleRestart` | 内核远端重启 |
| `/config/tproxy` | GET/POST | `tproxy.HandleTproxyState` | 获取或切换 TProxy 防火墙状态 |
| `/config/tproxy/exceptions` | GET/POST | `tproxy.HandleTproxyExceptions` | 获取或配置 TProxy 源/目的例外过滤规则 |
| `/config/tproxy/proxy-local` | GET/POST | `tproxy.HandleTproxyProxyLocal` | 获取或切换本机出站流量代理开关 |
| `/ipinfo/local/v4` | GET | `netinfo.HandleLocalIPv4` | 查询本地出站 IPv4 归属信息 |
| `/ipinfo/local/v6` | GET | `netinfo.HandleLocalIPv6` | 查询本地出站 IPv6 归属信息 |
| `/ipinfo/proxy/v4` | GET | `netinfo.HandleProxyIPv4` | 查询代理出站 IPv4 归属信息 |
| `/ipinfo/proxy/v6` | GET | `netinfo.HandleProxyIPv6` | 查询代理出站 IPv6 归属信息 |
| `/delaytest/google` | GET | `delaytest.HandleDelayTestGoogle` | 测试 Google 连通延迟 |
| `/delaytest/youtube` | GET | `delaytest.HandleDelayTestYouTube` | 测试 YouTube 连通延迟 |
| `/delaytest/github` | GET | `delaytest.HandleDelayTestGitHub` | 测试 GitHub 连通延迟 |
| `/delaytest/baidu` | GET | `delaytest.HandleDelayTestBaidu` | 测试百度连通延迟 |
| `/delaytest/bilibili` | GET | `delaytest.HandleDelayTestBilibili` | 测试 Bilibili 连通延迟 |
| `/delaytest/custom` | GET | `delaytest.HandleDelayTestCustom` | 测试用户自定义地址连通延迟 |
| `/meta/` | GET | `http.FileServer` | MetaCubeXD 外部面板静态文件 |
| `/zash/` | GET | `http.FileServer` | Zashboard 外部面板静态文件 |
| `/assets/` | GET | `http.FileServer` | 内嵌前端构建静态资源（Vite assets 目录） |

> 所有路由均挂载在 `baseURL = "/app/Fluxor"` 之下，如 `/app/Fluxor/core/status`。

### 3.4 TProxy 防火墙安全操作与退避规约 (`backend/internal/tproxy/`)

为了保障系统网络安全，在调用系统防火墙（如 `nft`）时必须遵循以下规则：
1. **防止命令注入**：严禁采用拼接 shell 字符串并使用 `sh -c` 的方式执行。必须使用 `exec.Command` 原生多参数切片传参，并在后台通过 `runCmd` 限制外部参数注入（特别是针对例外 IP/CIDR 等由用户表单输入的配置项）。
2. **退出彻底清退**：在面板退出时（通过监听 `syscall.SIGINT` 和 `syscall.SIGTERM` 信号），必须在退出前调用 `tproxy.DisableTProxyRules` 以清除所有已应用的网络重定向规则，以避免断网残留。

---

## 4. 前端数据更新与缓存架构 (开发约束)

为了保障页面切换时的流畅交互体验，并避免在后台静置运行时产生资源泄漏：

### 4.1 状态托管与快速加载
- 将各个页面的核心业务数据（如配置列表、代理节点、规则、连接快照）统一由全局 Pinia store 维护。组件重新挂载时直接呈现历史数据快照，随后在后台静默发起 fetch 刷新以同步最新状态（通过 `silent = true` 参数）。

### 4.2 WebSocket 引用计数生命周期管理
所有实时数据流（流量、内存、连接、日志）均采用 **引用计数 + 防抖断开** 模式，而非在 `onUnmounted` 中直接关闭 WS：

```
subscribe() → subscriberCount++ → 首次订阅时建立连接
unsubscribe() → subscriberCount-- → 归零后延迟 3 秒（防抖）→ 若无新订阅才真正断开
```

**优势**：
- 同一组件可多次安全订阅（如 `Overview.vue` 中的流量、内存、状态三路数据各自独立计数）。
- 组件切出与切回时保持 WS 连接的连续性，实现平滑过渡。
- 连接断开时，若仍有订阅者，自动尝试重连：
  - 流量/内存/连接 WS：5 秒后重连。
  - 日志 WS：指数退避重连（1s → 2s → 4s → ... → 最大 30s）。

**缓存留存**：
- 流量历史（`overview.ts`）保留最近 65 个数据点。
- 日志缓冲区（`logs.ts`）上限 2000 条。
- 已关闭连接（`connections.ts`）上限 100 条，超限时从旧到新截断。

### 4.3 连接瞬时速率计算
`connections.ts` 通过 **快照差分法** 计算每条连接的瞬时速率：记录每次 WS 消息到时的 `(upload, download, timestamp)` 快照，下一帧按 `(uploadDiff + downloadDiff) / timeDiff` 得出速率。
- 首帧（或暂停恢复后）跳过归档，防止陈旧快照误判。
- 前后端断开瞬间自动归档为已关闭连接。

### 4.4 批量测速并发控制规约
- 无论是 Vue 3 前端中的批量测速（全部测速或组测速），**必须强制实施并发控制（默认并发数限制为 10）**。绝对禁止一次性无限制发起数百个网络测速请求，防止浏览器连接队列拥堵与后端 Unix Socket 重试负载崩溃。

### 4.5 弹窗与交互去原生化
- 全站**绝对禁止使用**浏览器的阻塞式 `alert(...)`。如有提示需要，一律使用 `globalStore.showToast(text, 'success' | 'error' | 'warning' | 'info')` 发送非阻塞的全局自定义 Toast。
- 确认操作使用 `globalStore.showConfirm({title, message, confirmText?, cancelText?})` 返回 Promise，在 `App.vue` 中以模态渲染。
- 所有在页面上展示的图标**必须使用 `xicons` (@vicons/ionicons5)**，禁止在系统硬编码 SVG、Emoji 或颜文字符号（包括内置配置模板中也绝对禁止在代理组和规则名中硬编码 Emoji），保持视觉绝对统一。

### 4.6 乐观更新与回滚
涉及高频用户操作的接口（如代理选择切换、规则启用/禁用、连接断开）采用**乐观更新**策略：
1. 立即更新本地状态，呈现即时反馈.
2. 并发发起 API 请求。
3. 请求失败时自动回滚至更新前的状态。

### 4.7 前端离线 Mock 联调机制
为了方便前端脱离 Go 后端独立运行与联调，`frontend/src/utils/mock.ts` 实现了完整的 HTTP API 与 WebSocket 数据流模拟器。
- **启用机制**：仅在 Vite 开发模式（`import.meta.env.DEV`）下启用。`api.ts` 通过 `import.meta.env.DEV ? await import('./mock') : null` 动态装载，生产构建中模拟器模块整体被摇树移除，线上无法通过任何 localStorage 值开启。
- **手动控制**（仅 DEV 有效）：可通过在控制台修改 `localStorage` 的键 `MOCK_BACKEND` 来强制覆盖：
  - 强制启用 Mock 模式：`localStorage.setItem('MOCK_BACKEND', 'true')`
  - 强制关闭 Mock（直接连接真实后端）：`localStorage.setItem('MOCK_BACKEND', 'false')`

### 4.8 缓存视图下的生命周期管理与滚动能效控制
在 KeepAlive 缓存组件后台化时，为了防止后台页面（如 `Logs.vue`）继续对增长的日志流进行无效的 DOM 渲染和滚动条计算，必须绑定 `onActivated` / `onDeactivated` 生命周期钩子来冻结/激活这些滚动计算（如 logs watcher），有效控制组件静置状态下的 CPU 负载与系统开销。

### 4.9 复杂数据预解析与计算缓存
严禁在组件渲染周期内，或者在模板中高频调用包含重度计算或复制排序的操作（如 `sort` 历史延迟记录）。所有这类解析操作应收归至 Store 中，在拉取到最新数据的第一时间完成一轮预解析，并直接挂载为节点的响应式扁平字段（如 `latestDelay`、`recentColors`），组件仅负责以 O(1) 效率读取。

### 4.10 表单直连与双向绑定规范
表单交互输入（如端口设定）应直接通过 `v-model` 或 `v-model.number` 直连 Pinia Store 托管的数据对象。严禁声明冗余的局部 ref 变量并配合 watch-deep 进行繁重的手动双向映射赋值。当表单输入非法导致校验不通过时，可通过 Store 的 fetch 行为从后端重新拉取真实数据完成本地强制回滚。

### 4.11 Tailwind CSS v4 迁移要点与样式兼容约束

前端已从 Tailwind CSS 3 迁移至 v4（`@tailwindcss/vite` 插件 + CSS-first 配置）。**原 `tailwind.config.js` 与 `postcss.config.js` 已删除**，全部定制集中在 `src/index.css` 顶部。修改样式前请务必了解以下规则，否则极易引入视觉回归：

1. **配置位置**：`@import "tailwindcss" source(none)` 关闭自动扫描后，由 `@source` 显式声明扫描范围（务必保留 `source(none)`，否则会误扫 `node_modules` 使产物膨胀 30KB+）。
2. **暗色模式**：由 `@custom-variant dark (&:is([data-theme="dark"] *))` 复刻 v3 的 `darkMode: ['class','[data-theme="dark"]']`，选择器形态与 v3 完全一致，**不要**改成 `:where()` 或其他形式。
3. **语义色必须用 `@theme inline`**：`accent/success/danger/warning` 基于 `var(--accent)` 等动态变量，必须写在 `@theme inline` 中。若改用普通 `@theme`，v4 将无法为这些颜色生成透明度修饰符（`bg-accent/10`、`ring-accent/30`、`shadow-accent/15` 等会全部失效）。
4. **尺寸档位覆盖**：v4 的档位整体上移一级（v4 的 `shadow-sm` 等于 v3 的 `shadow`，v4 的 `rounded-sm` 等于 v3 的 `rounded`）。`index.css` 已覆盖 `--shadow-sm` / `--radius-sm` 回到 v3 取值，保证不改类名即维持原效果。**请勿删除这两行覆盖**。
5. **`space-y-*` / `space-x-*` 已用 `@utility` 重写**：v4 内置版本为 `:where(...)`（特异性为 0）且作用于 `margin-block`，会导致子元素自身的 `my-*` 反压 space 间距并叠加外边距（曾使 Overview 卡片矮 2px）。重写后与 v3 行为一致，**不要移除**。
6. **`@layer` 顺序敏感**：项目自定义 CSS 全部位于 `@layer utilities` 之内。v4 把工具类放进 CSS 级联层，若把自定义样式写在层外（无层级），其优先级会反超工具类，导致 `transition-all` 之类属性被意外覆盖。新增全局样式时请同样置于该层内。
7. **v4 默认值差异**：v4 preflight 移除了 `button{cursor:pointer}`（已在 `@layer base` 补回）；`border` 默认色由 `#e5e7eb` 变为 `currentColor`；`hover:` 变体被包进 `@media (hover:hover)`。改动按钮或边框样式时注意这些差异。
8. **调色板为 v4 oklch 配色**：站点主色 slate 系偏差 ≤4/255（基本无感），但 red/amber/rose/emerald/blue 等饱和色最大偏差可达 ~38/255，属有意接受的 v4 新调色板，并非缺陷。
9. **透明度修饰符已生效**：v3 时代因非静态色值而静默失效的 18 处类（如 `bg-accent/10`、`ring-accent/5`、`dark:text-accent/90`、`py-4.5`）在 v4 下已正常生成。这是预期内的修复，若需回退到"失效"状态需另行评估。
10. **回归验证方法**：改动样式后，除 `npm run build` 外建议做一次浏览器计算样式比对：以迁移前的 v3 产物 CSS 为基准，将两版 CSS 分别套在相同 DOM 上 diff `getComputedStyle`（重点看 `display`/`position`/宽高/内外边距/`borderRadius`/`transitionProperty`/`cursor`）。仅比对类名是否存在不足以发现级联层与特异性类回归。

---

