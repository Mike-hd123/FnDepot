# fpkg-app-template — fnOS 原生应用打包标准模板

> **归属**：本模板住在 FnDepot 仓库根（与全部应用源码同家），**不在 fnpack.json 商店索引、不打包**；用法知识索引在 Hermes 技能 `fnos-app-package/SKILL.md`。
> 从 hyatlas / octopus / wechat-on-cloud 现役结构提炼的开箱即用模板。
> 复制本目录 → 全局替换占位符 `<appname>` / `<上游版本>-<本地版本号>` / `<PORT>` / `<显示名>` / `<上游org>/<上游repo>` → 按 build.sh 注释填应用内容 → 打包。

## 目录结构
```
fpkg-app-template/
├── manifest              # INI 元信息（改 build.sh 会替换 version 占位符）
├── config/
│   ├── privilege         # run-as: root（systemd 应用必须 root）
│   └── resource          # 空 {}（原生无 docker）
├── ui/
│   └── config            # 桌面入口 .url iframe（TAG 协议自适应，禁硬编码 http）
├── wizard/install        # 安装向导（可加 wizard/uninstall 选保留/删数据）
├── cmd/                  # 9 脚本必须齐全（缺任一 → 10111 / fnpack build 直接报错）
│   ├── main              # start/stop/status，PID 文件管理，TRIM_PKGVAR 日志
│   ├── install_init / install_callback
│   ├── upgrade_init / upgrade_callback
│   ├── uninstall_init / uninstall_callback
│   └── config_init / config_callback
├── sidecar/
│   └── gateway_proxy.py  # socket 反代（v5 前缀代理 + v7 SW自杀 + v8 gzip + v9 缓存分流 + v12 后端gzip）
├── ICON.PNG / ICON_256.PNG   # 商店图标（模板需要自备：见下）
├── build.sh              # fnpack 打包（参数化 APP_NAME/VERSION/ARCH/OUT）
└── README.md             # 本 checklist
```
- **图标三路径**：商店 = 根 `ICON.PNG/ICON_256.PNG`；桌面 = `ui/images/icon-{0}.png`（app.tgz 内，
  安装到 `@appcenter/<app>/ui/images/`）；面板内 logo 是应用自身资源，**不属于 fpk 图标**。
- **cmd/ 9 脚本**：main + install/uninstall/upgrade/config 各 init/callback。空 `exit 0` 也必须存在。
- **fnpack 打包规则**：`app/` 下全量自动打 app.tgz；fpk 根目录游离 `ui/` 会被 fnpack 当非法 JSON 报错或静默丢弃 → 桌面入口打不开。打包前清根目录游离 `ui/`、`.bak*`、旧 `.fpk`。

---

## 交付 checklist（从开发 → 测试 → 用户实测 → push → fpk 落 download 全流程）

> **以下 1-7 条是用户交付偏好铁律，每次打包必过，一字不改。**（0907 技能瘦身固化）

- [ ] **1. 改动一律落源码仓库（FnDepot 原位升级），禁改运行时文件，严禁新建项目结构**
      改的必须是 `/vol2/1000/workspace/FnDepot/<app>/src/` 下的源码/模板，不是运行中
      的 `@appcenter/<app>/`。运行时改动随卸载丢失。
- [ ] **2. 测试通过 → 用户实测 → 才 git push**
      自测（解包对照/curl/直连验证）通过 → 汇报用户 → **用户实测通过后才 push**。
      用户确认前一个 commit/push/fpk-release 都不做。不发包、不推送。
- [ ] **3. 上游更新 → 先 merge 上游再改本地补丁再打包**
      上游发新版本：先 merge 上游到工作区 → 再叠加本地补丁 → 再打包。
      严禁在旧包上反复打补丁。
- [ ] **4. 版本号 = `<上游版本>-<本地版本号>`**（如 `4.1.1-5`）
      本地迭代/修复用 `-N` 后缀；**上游更新后本地后缀重置为 1**。
      **改一次不加号**——只有用户安装测试通过后才 bump。产物 fpk 落 `/vol2/1000/download/`，
      **不自动安装，装哪先问用户**。调试期不加版本号。
- [ ] **5. 调试期直接 CDP/改原应用验证，验证 OK 才打包**
      先 headless Chromium CDP 进应用面板实际点验/改原应用进程，功能验证通过再打 fpk，
      别先把包打出来反复装。
- [ ] **6. 改配置前先备份（.bak_日期），备份保留不主动删**
      动 manifest / cmd / config 前，先 `cp <f> <f>.bak_<日期>`。备份留着，不主动删。
- [ ] **7. 大包走 GitHub Release（curl API，gh CLI 不可用，代理 -x http://127.0.0.1:7890）**
       >100MB 的 fpk 用 curl 打到 GitHub Release；常规体积按 fnpack.json 入库。
      `curl -x http://127.0.0.1:7890 -X POST ...`（GitHub Release API）。

### 打包后自检
- [ ] `cmd/` 9 脚本全在（`ls cmd/` 该有 9 个）
- [ ] `manifest` 里 `version=<上游>-<本地>`、`appname`、`maintainer_url`、`distributor_url` 字段带全
- [ ] `ui/config`：`protocol: ""`（禁硬编码 http，否则 https 混合内容白屏）+ `port` 写死数值
- [ ] `tar tzf <fpk>` 里 `ui/` 存在（桌面入口在）；`tar tzf app.tgz | grep ui` 非空
- [ ] `tar xOf <fpk> manifest | grep -E 'maintainer|distributor'` 字段进了产物
- [ ] `app.tgz` 代码完整（`tar xOf <fpk> app.tgz | tar tz | grep -E '^(dist|server|package.json)'` 存在）——
      **包体积骤减 = 漏代码最强信号**
- [ ] sha256/size 实算写进交付 summary（禁编造）

### 安装后 DB 核验（只读优先）
- [ ] `app_service.url/default_url/full_url` 带 `http://` + 端口；`gateway_socket/gateway_prefix` 空
- [ ] `file_types='[]'`（写成 `{}` 毒整张表 → 所有应用 10000）
- [ ] `app_open` 有 `(app_id, 'iframe', '<app>.panel')`
- [ ] `app_service.icon` 同步 `ui/images/icon-{0}.png`（改图标名后必须 `UPDATE ... REPLACE`）
- [ ] 数据落点实查 `/proc/<pid>/fd` 或逐卷 `ls`，**别信卡 body / worker 自报路径**

---
## 上游对齐（每个 fpk 都要标注）
- manifest `maintainer`+`maintainer_url` = **上游作者**（Mike 不是开发者）
- manifest `distributor`+`distributor_url` = Mike（`https://github.com/Mike-hd123`）
- version = 上游最新 release tag（`curl -s https://api.github.com/repos/<org>/<repo>/tags?per_page=3` 自查）

## 参考
- `/vol1/@apphome/hermes-studio/hermes-home/skills/fnos/fnos-app-package/references/gateway-proxy.md` — socket 桥接详解
- `/vol1/@apphome/hermes-studio/hermes-home/skills/fnos/fnos-app-package/references/fpk-build-lessons.md` — 打包/重打包教训合集
- `/vol1/@apphome/hermes-studio/hermes-home/skills/fnos/fnos-app-package/references/fpk-webdist-repack.md` — 换前端重打包 + 5 项验收清单
- `/vol1/@apphome/hermes-studio/hermes-home/skills/fnos/fnos-app-package/references/delivery-and-merge-rules.md` — 交付迭代规则（本文档 1-7 是其技能侧固化）