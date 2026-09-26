# FnDepot — Agent 须知

> 项目说明见 [README.md](./README.md)。**本文件只讲 agent 该做什么 / 红线 / 坑**。
> 每个应用子目录有自己的 `AGENT.md`（详细版：架构/定制清单/血泪坑）；改任何应用前先读对应子项目 AGENT.md + 本文件的通用规矩。

## 子项目一览

| 目录 | 应用 | 当前版本 | 上游 | 子 AGENT.md |
|---|---|---|---|---|
| `wechat-on-cloud/` | 云微(飞牛云微信) | 1.4.9-6 | Gloridust/WechatOnCloud | [AGENT.md](./wechat-on-cloud/AGENT.md) |
| `hyatlas/` | HyAtlas(混元记忆) | 4.1.1-8 | tuancookiez-hub/HyAtlas-Memory | [AGENT.md](./hyatlas/AGENT.md) |
| `octopus/` | Octopus(LLM 网关) | 0.13.8-1 | bestruirui/octopus | [AGENT.md](./octopus/AGENT.md) |
| `ezbookkeeping/` | EZ记账 | 2.0.0-1 | mayswind/ezbookkeeping | [AGENT.md](./ezbookkeeping/AGENT.md) |
| `vikunja/` | 待办(Vikunja) | 2.6.0-14 | go-vikunja/vikunja | [AGENT.md](./vikunja/AGENT.md) |
| `app-template/` | 打包模板（非应用） | — | — | [AGENT.md](./app-template/AGENT.md) |

> **fluxor 已 2026-09-22 全量下线**（目录/commit/fnpack.json 记录均已清空），后续统一用官方版，勿在此仓库 fork。

## 目录约定

```
<app>/
├── AGENT.md          # 本索引链接的 agent 须知
├── README.md         # 包说明（人读）
├── ICON.PNG / ICON_256.PNG
├── <app>.fpk         # 当前发布（历史版本包按需保留，与 fnpack.json releases 对齐）
├── manifest / build.sh（部分应用放在 src/fnos-native/ 等打包层里）
└── src/              # 上游源码 fork + 全部 NAS 适配改动
```

## 通用红线（所有子项目通用）

1. **版本号口径**：飞牛应用版本 = `上游版本-本地版本号`（如上游 1.6.0 → 飞牛 1.6.0-1）。**禁止修改上游项目自身的版本号**。本地修订号惯例：一个迭代一个号，不烧修订号；本机已装的同号 appcenter 判无更新，必要时才进位。
2. **改动只落源码仓库**：禁直接改运行时目录（`/vol1/@appcenter/*`、`/vol1|2/@appdata/*` 是运行时，只读参考）。
3. **交付流程**：改动 → 测试通过 → 用户实测 → 才 `git push`。产物落 `/vol2/1000/download/`，不自动安装。
4. **git 纪律**：一个版本迭代一个 commit；push 前可 `reset --soft <base>` 收敛碎片。禁 GitHub Contents API 逐文件推送（会产生碎片 commit + `<<<<<<<` 冲突残留）。`git push` 走代理：先 `unset http.lowspeedlimit http.lowspeedtime`，再 `git config --global --add http.https://github.com/.proxy http://127.0.0.1:7890`。force push 用 `--force-with-lease` 而非 `--force`；干前先 `git push --dry-run --force-with-lease` 给用户看要覆盖哪些 commit。
5. **sudo 必须先向用户确认**：fnOS 服务启停（appcenter-cli）同理——Fluxor 重启=全屋代理瞬断，须明确授权。
6. **安装卷**：fnOS 默认卷=空间1（default-volume=1）；`install-fpk` 不加 `-v` 装空间1，升级忽略 `-v` 沿用原卷。卷 id：vol1=空间1 / vol2=空间2；查 `app` 表 `install_volume_id` + `readlink /var/apps/<app>/target`。
7. **凭证禁入库**：NAS 口令走 `.env` 的 `FNOS_PASSWORD_B64`，不入代码/文档。

## 常见坑

- **GitHub Contents API 碎片**：逐文件推产生 100+ commit + 冲突标记残留 → 只能 force push 覆盖，别再用。
- **fnpack.json 索引滞后**：部分 releases 会落后于目录实况（历史遗留：ezbookkeeping 曾未录 2.0.0-1）。**发版收口必须同步** `fnpack.json` + 根 README 表格 + 历史包文件名。
- **桌面入口 404 / WS 426**：fnOS entry.url 路径 vs 端口模式，唯一决定字段是 `target/ui/config` 的 `gatewaySocket/gatewayPrefix`；manifest 的 `micro_app/service_port` 完全无效（vikunja 2.6.0-12 死路坐实）。
- **fnpack build 缺脚本 → 10111**：`cmd/` 9 脚本（install_init/install_callback/config_init/config_callback/main/uninstall_init/uninstall_callback 等）必须齐全。
- **fnOS 桌面图标缓存**：改包图标后桌面显示旧的=缓存问题，不是打包问题（fluxor 1.6.1-2 排查结论）。
- **fnOS install-fpk 升级忽略 `-v`**：升级时沿用原卷；卷迁移必须重装，不能靠升级切卷。

## 工具链（NAS 本机）

- Go：`/vol2/1000/workspace/tools/go1.27/bin`
- Node：`/vol1/@appcenter/nodejs_v24/bin`（或 `tools/node/bin`）
- npm（Hermes 环境正确姿势）：`unset NODE_ENV && npm install --include=dev --registry=https://registry.npmmirror.com --https-proxy=http://127.0.0.1:7890 --proxy=http://127.0.0.1:7890`，必要时 `NODE_OPTIONS=--dns-result-order=ipv4first`
- 外网代理：`http://127.0.0.1:7890`（走 mihomo/ClashLite，装/重启期间会断，直连操作先关代理）
- 打包：fnpack build（见 `app-template/AGENT.md`）
- SSH/凭证：NAS 口令走 `.env` 的 `FNOS_PASSWORD_B64`

## 上游同步流程（改任何应用前）

1. 查上游 release/commit（走 7890 代理打 GitHub API）
2. merge 上游 → 保住 fork 定制（**教训：fluxor 1fe7e7d 合并 1.5.0 时把本地 smart 三件套覆盖丢了**——合并必做"定制存活清单"逐条核对）
3. 构建 + 测试全绿 → 打 fpk
4. 用户实测通过
5. push + 同步 `fnpack.json` / 根 README 表格 / 历史包文件命名
