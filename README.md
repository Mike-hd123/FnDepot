# FnDepot — Mike 的飞牛第三方应用源

飞牛 fnOS 外部应用源（schema_version 2），收录 6 个自打包应用 + 1 个打包模板。每个应用目录 = 图标 + 说明 + 上游源码 fork(`src/`)；fpk 二进制统一走 [GitHub Release `v2026.09.23`](https://github.com/Mike-hd123/FnDepot/releases/tag/v2026.09.23) 分发，不进 git 跟踪。

开发规范见 [AGENT.md](./AGENT.md)。

## 添加源

fnOS 应用中心 → 设置 → 外部应用源 → 添加：

```
https://github.com/Mike-hd123/FnDepot
```

## 应用一览

| 应用 | 版本 | 说明 | 上游项目 |
|---|---|---|---|
| 云微(飞牛云微信) `wechat-on-cloud` | 1.5.1-1 | NAS 原生微信面板，Node.js 面板 + dockerode 管理微信实例容器，ipvlan(woc-lan) 单网卡直连局域网，数据落主机路径 bind，创建实例可选数据目录，移动端触屏优化，实例「电源」下拉(重启/关机) | [Gloridust/WechatOnCloud](https://github.com/Gloridust/WechatOnCloud) |
| HyAtlas(混元记忆) `hyatlas` | 4.5.0-2 | AI 长期记忆系统。v4 纯 Go 单二进制：内置 ONNX Runtime int8 向量引擎(bge-large-zh 1024d)，chromem-go 存储，fnOS 网关 socket 入口 /app/hyatlas，端口 19528。4.5.0-1 同步上游 v4.4.0 + v4.5.0（27 commits / 66 files）——搜索改 hybrid(BM25+向量 RRF，reader=legacy/keyword_tag/hybrid_v2 三态)、L5 知识转为可检索(启动 BackfillL5 幂等补齐历史图边，无开关)、新增 /api/v1/admin/compact_raw 与 dedupe_facts 维护端点(默认 403，HYATLAS_ADMIN=on 才放行；compact_raw 不可逆)、consolidation 按 owner 分组且 batch 默认 200→50 避超时、事实改用 supersede(invalid_at/superseded_by) 不再删除、fingerprint 改 fact IDs 哈希(升级后每 owner 重跑一轮)、drop 必须带 reason、routes() 抽出并套 guardLocal(HYATLAS_ALLOWED_HOSTS)+limitBody(8MiB)。4.5.0-2 增 delete_all 防误删护栏：默认 dry_run 只统计、按 id 的窄删除(遗忘功能)不受限、只给单个 user_id/agent_id 的宽范围删除需 HYATLAS_ADMIN=on 且 confirm_mass_delete=true、命中数超 HYATLAS_DELETE_MAX(默认 1000) 需显式确认、实际删除写审计日志。本地定制保留（L7 去重硬顶、L1 不被异步提取写、touch/patch 双时态、SearchIncludeExpired 软过期可见、dashboard 全量汉化 + 移动端抽屉 + 时间线分页、shares 自愈、/vol1/@appdata/hyatlas/env 配置化）。**fork 源码随本仓库 `hyatlas/src/` 维护**，上游见 [tuancookiez-hub/HyAtlas-Memory](https://github.com/tuancookiez-hub/HyAtlas-Memory) | [tuancookiez-hub/HyAtlas-Memory](https://github.com/tuancookiez-hub/HyAtlas-Memory) |
| Octopus `octopus` | 0.13.10-1 | LLM API 聚合网关，Go 单二进制 + 内嵌前端 + SQLite，多渠道聚合/协议转换/故障转移，端口 8081（agent 模型出口，最高优先级服务）。二进制=上游官方 CI 原样，本地价值在 fnOS 打包层：gateway sidecar 剥前缀反代 + SSE 流式透传 + 动态 cache-bust，手机端 /app/octopus 全链路可路由。 | [bestruirui/octopus](https://github.com/bestruirui/octopus) |
| EZ记账 `ezbookkeeping` | 2.0.1-1 | 家庭记账：本地优先 SQLite 存储，多账本/预算/报表，支持微信/支付宝/信用卡账单导入，gzip 压缩提速，移动端触屏优化。v2 新增信用卡额度/可用额度环、洞察报表自定义图表、S3 对象存储，1.x→2.x 数据自动迁移无损 | [mayswind/ezbookkeeping](https://github.com/mayswind/ezbookkeeping) |
| 待办(Vikunja) `vikunja` | 2.7.0-1 | 自托管待办面板：Go 静态 ELF 单二进制 + SQLite，中文 UI + CalDAV，API token 全自动读写（Hermes 提醒引擎），桌面 3456 + 手机 /app/vikunja 双通道。v2.7.0 同步：6 安全修复+MCP server+前端切 v2 API（sidecar 锚点存活实测）+12 条 sqlite 迁移净室验证 tk_ token 不破 | [go-vikunja/vikunja](https://github.com/go-vikunja/vikunja) |
| Emby `emby-in-one` | 1.4.4-4 | Emby 多账号聚合反向代理：Go 静态二进制 + Python socket 网关 sidecar，治 Emby Boost CDN 签名 URL 900 秒过期拖拽 403；后台 token 失效自动重登无感续命；桌面入口 /app/emby/admin/，TV 直连 18096 | [ArizeSky/Emby-In-One](https://github.com/ArizeSky/Emby-In-One) |
| `app-template` | — | 打包模板（非应用）：从现役应用结构提炼的标准骨架，新应用上架 = 复制模板 → 全局替换占位符 → fnpack build | — |

**Fluxor** 已 2026-09 起不再维护（原 fork 分支 2026-09 全量下线，目录/commit/`fnpack.json` 记录均已清空）——请改用 [官方版 shuangji66/fluxor](https://github.com/shuangji66/fluxor)。

## 目录结构

```
FnDepot/
├── fnpack.json              # 源索引（V2 单文件模式，含 sha256+size）
├── AGENT.md                 # agent 开发规范
├── README.md                # 本文件（人读）
├── wechat-on-cloud/
│   ├── ICON.PNG / ICON_256.PNG / README.md / AGENT.md
│   ├── wechat-on-cloud.fpk
│   └── src/                 # fork 上游源码 + 全部 NAS 适配改动
├── hyatlas/
│   └── ICON.PNG / ICON_256.PNG   # 仅图标；源码已迁出本仓库
├── octopus/
│   ├── ICON.PNG / ICON_256.PNG / README.md / AGENT.md
│   ├── octopus.fpk
│   └── src/                 # 上游 v0.13.5 源码 + fnos/（fnOS 打包层 cmd/config/gateway/manifest）
├── ezbookkeeping/
│   ├── ICON.PNG / ICON_256.PNG / README.md / AGENT.md
│   ├── ezbookkeeping.fpk
│   └── src/                 # 打包层源码（sidecar/gateway/manifest 等 fnOS 适配）
├── vikunja/
│   ├── ICON.PNG / ICON_256.PNG / README.md / AGENT.md
│   ├── manifest / vikunja.fpk（当前版本）
│   └── src/                 # 打包层源码（manifest / ui-config / gateway_proxy.py）
├── emby-in-one/
│   ├── ICON.PNG / ICON_256.PNG / README.md / AGENT.md / LICENSE (GPL-3.0)
│   ├── manifest / build.sh / cmd / config / ui / wizard
│   ├── app/gateway/         # socket sidecar（自动重登）；Go 二进制 app/emby-in-one 不入库
│   ├── src/                 # 上游 Emby-In-One Go 源码 fork（含 validateAPIKey 定制补丁）
│   ├── docs/ tests/         # 排障报告 / e2e 测试
│   └── （emby-in-one.fpk 走 GitHub Release 分发，不进 git）
├── app-template/
│   ├── ICON.PNG / ICON_256.PNG / README.md / AGENT.md
│   ├── build.sh / manifest / config / cmd / ui / wizard / sidecar
│   └── （复制本目录作为新应用脚手架，不进 fnpack.json）
└── docs/                    # 通用研究文档（fnOS API / Octopus 日志审计等）
```

**HyAtlas 4.5.0-2 fpk**：231MB，走 GitHub Release `v2026.10.08` 分发，不进 git 跟踪。fork 源码在本仓库 `hyatlas/src/`，上游工程 [tuancookiez-hub/HyAtlas-Memory](https://github.com/tuancookiez-hub/HyAtlas-Memory)。

## 打包说明

- 打包工具：fnOS 官方 `fnpack build -d <project-dir>`，产物内 manifest appname/version 与 fnpack.json 一致。
- 云微改动全在 `src/panel`（面板 web）与 fpk 打包层（cmd/、manifest），本仓库 `src/` 与 [Mike-hd123/WechatOnCloud](https://github.com/Gloridust/WechatOnCloud) 同步。
- HyAtlas v4 汉化与内置向量引擎在 Go 源码内；fork 源码随本仓库 `hyatlas/src/` 一起维护（上游 [tuancookiez-hub/HyAtlas-Memory](https://github.com/tuancookiez-hub/HyAtlas-Memory)），fnOS 控制层与打包脚本在 `hyatlas/src/fnos-native/`（22 文件），模型三件套(~355MB)超 GitHub 限制不入库。fpk 走 GitHub Release 分发，构建确定性可复现：同一 commit 重编三次 sha256 逐字节相同。
- 发布者：Mike · https://github.com/Mike-hd123
