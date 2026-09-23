# FnDepot — Mike 的飞牛第三方应用源

飞牛 fnOS 外部应用源（schema_version 2），收录 5 个自打包应用 + 1 个打包模板。每个应用目录 = 图标 + 说明 + 上游源码 fork(`src/`)；fpk 二进制统一走 [GitHub Release `v2026.09.23`](https://github.com/Mike-hd123/FnDepot/releases/tag/v2026.09.23) 分发，不进 git 跟踪。

开发规范见 [AGENT.md](./AGENT.md)。

## 添加源

fnOS 应用中心 → 设置 → 外部应用源 → 添加：

```
https://github.com/Mike-hd123/FnDepot
```

## 应用一览

| 应用 | 版本 | 说明 | 上游项目 |
|---|---|---|---|
| 云微(飞牛云微信) `wechat-on-cloud` | 1.4.9-4 | NAS 原生微信面板，Node.js 面板 + dockerode 管理微信实例容器，ipvlan(woc-lan) 单网卡直连局域网，数据落主机路径 bind，创建实例可选数据目录，移动端触屏优化，实例「电源」下拉(重启/关机) | [Gloridust/WechatOnCloud](https://github.com/Gloridust/WechatOnCloud) |
| HyAtlas(混元记忆) `hyatlas` | 4.1.1-7 | AI 长期记忆系统。v4 纯 Go 单二进制：内置 ONNX Runtime int8 向量引擎(bge-large-zh 1024d)，移除 llama.cpp 18080 依赖；dashboard 全量汉化 + 移动端响应式；chromem-go 存储，socket 型 fnOS 入口 /app/hyatlas，端口 19528。v6 新增 index 写入合并(异步 coalesce，SSD 写入从 ~11GB/天降至 ~0.5GB/天) + 全删接口护栏 + L7 防膨胀。**源码不在本仓库**，见上游 | [tuancookiez-hub/HyAtlas-Memory](https://github.com/tuancookiez-hub/HyAtlas-Memory) |
| Octopus `octopus` | 0.13.6-1 | LLM API 聚合网关，Go 单二进制 + 内嵌前端 + SQLite，多渠道/多模型管理，支持单渠道多 Key，端口 8081 | [bestruirui/octopus](https://github.com/bestruirui/octopus) |
| EZ记账 `ezbookkeeping` | 2.0.0-2 | 家庭记账：本地优先 SQLite 存储，多账本/预算/报表，支持微信/支付宝/信用卡账单导入，gzip 压缩提速，移动端触屏优化。v2 新增信用卡额度/可用额度环、洞察报表自定义图表、S3 对象存储，1.x→2.x 数据自动迁移无损 | [mayswind/ezbookkeeping](https://github.com/mayswind/ezbookkeeping) |
| 待办(Vikunja) `vikunja` | 2.6.0-14 | 自托管待办面板：Go 静态 ELF 单二进制 + SQLite，中文 UI + CalDAV，API token 全自动读写（Hermes 提醒引擎），桌面 3456 + 手机 /app/vikunja 双通道 | [go-vikunja/vikunja](https://github.com/go-vikunja/vikunja) |
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
├── app-template/
│   ├── ICON.PNG / ICON_256.PNG / README.md / AGENT.md
│   ├── build.sh / manifest / config / cmd / ui / wizard / sidecar
│   └── （复制本目录作为新应用脚手架，不进 fnpack.json）
└── docs/                    # 通用研究文档（fnOS API / Octopus 日志审计等）
```

**HyAtlas 4.1.1-7 fpk**：105MB，走 GitHub Release `v2026.09.23` 分发，不进 git 跟踪。HyAtlas 源码工程位于 [tuancookiez-hub/HyAtlas-Memory](https://github.com/tuancookiez-hub/HyAtlas-Memory)，本仓库仅保留图标与 fnpack.json 索引条目。

## 打包说明

- 打包工具：fnOS 官方 `fnpack build -d <project-dir>`，产物内 manifest appname/version 与 fnpack.json 一致。
- 云微改动全在 `src/panel`（面板 web）与 fpk 打包层（cmd/、manifest），本仓库 `src/` 与 [Mike-hd123/WechatOnCloud](https://github.com/Gloridust/WechatOnCloud) 同步。
- HyAtlas v4 汉化与内置向量引擎在 Go 源码内，源码工程位于上游 [tuancookiez-hub/HyAtlas-Memory](https://github.com/tuancookiez-hub/HyAtlas-Memory)（不在本仓库）；105MB fpk 走 GitHub Release `v2026.09.23` 分发，本仓库 `hyatlas/` 目录仅保留 ICON。
- 发布者：Mike · https://github.com/Mike-hd123
