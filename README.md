# FnDepot — Mike 的飞牛第三方应用源

飞牛 fnOS 外部应用源（schema_version 2），收录 4 个自打包应用 + 1 个打包模板。每个应用目录 = 图标 + 说明 + 上游源码 fork(`src/`)（agent2api 为闭源发行包，只维护打包层）；fpk 二进制统一走 GitHub Release 分发，不进 git 跟踪。

开发规范见 [AGENT.md](./AGENT.md)。

## 添加源

fnOS 应用中心 → 设置 → 外部应用源 → 添加：

```
https://github.com/Mike-hd123/FnDepot
```

## 应用一览

| 应用 | 版本 | 说明 | 上游项目 |
|---|---|---|---|
| 云微(飞牛云微信) `wechat-on-cloud` | 1.6.0-2 | 飞牛原生运行微信 | [Gloridust/WechatOnCloud](https://github.com/Gloridust/WechatOnCloud) |
| HyAtlas(混元记忆) `hyatlas` | 5.0.0-1 | AI 长期记忆系统 | [tuancookiez-hub/HyAtlas-Memory](https://github.com/tuancookiez-hub/HyAtlas-Memory) |
| Agent2API `agent2api` | 3.5.0-3 | 本地 OpenAI 兼容 LLM 网关 | [aimod-cc/agent2api](https://github.com/aimod-cc/agent2api) |
| 待办(Vikunja) `vikunja` | 2.8.0-1 | 自托管待办面板 | [go-vikunja/vikunja](https://github.com/go-vikunja/vikunja) |
| `app-template` | — | 打包模板（非应用） | — |

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
├── agent2api/
│   ├── ICON.PNG / ICON_256.PNG / README.md / AGENT.md
│   ├── manifest / cmd / config / wizard / app（ui-config + 入口图标）
│   └── （闭源发行包：二进制 server/ 与 fpk 走 GitHub Release，不入库）
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

## 打包说明

- 打包工具：fnOS 官方 `fnpack build -d <project-dir>`，产物内 manifest appname/version 与 fnpack.json 一致。
- 各应用的源码 fork、NAS 适配改动与版本历史见对应子目录的 `README.md` / `AGENT.md`；版本与包 sha256 以 `fnpack.json` 为准。
- fpk 一律走 GitHub Release 分发，不进 git 跟踪；超 GitHub 限制的大文件（如模型三件套）同样不入库。
- 发布者：Mike · https://github.com/Mike-hd123
