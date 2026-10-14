# app-template — Agent 须知

> 项目说明见 [README.md](./README.md)。本文件只讲 agent 该做什么 / 红线 / 坑。

## 定位
fnOS 原生应用打包**标准模板**（非应用）：从 hyatlas / octopus / wechat-on-cloud 现役结构提炼。住 FnDepot 仓库根，**不进 fnpack.json 索引、不打 fpk**。

## 用法
新应用上架 = 复制本目录 → 全局替换占位符 `<appname>` / `<上游版本>-<本地版本号>` / `<PORT>` / `<显示名>` / `<上游org>/<上游repo>` → 按 build.sh 注释填内容 → `fnpack build`。

## 结构要点
- `manifest`（INI 元信息）、`config/privilege`（run-as **package**，官方唯一推荐值，user 官方零命中）、`config/resource`（**data-share 声明** ← 数据落 share 的唯一入口）
- `ui/config`：桌面入口 .url iframe，**TAG 协议自适应，禁硬编码 http**
- `wizard/`：install（`text`/`password` 字段收集端口/地址/密钥）+ uninstall（radio 保留/清除数据双选）。field 名 = 环境变量名，**必须 `wizard_` 前缀，禁 `TRIM_`**
- `cmd/` 9 脚本必须齐全（缺任一 → 10111 / fnpack build 直接报错）；main 管 start/stop/status + PID 文件 + **TRIM_PKGVAR 日志**（不是数据）

## 数据落点（官方明文，红线）
- **应用数据必须放 share**：`config/resource` 声明 `data-share.shares[].name` → fnOS 安装时**自动创建目录并自动授 Windows ACL**，不用 `mkdir`、不用 `chmod`、不用 root
- **绝不用 `@appcenter` 放任何要留的东西**：**升级也会整体替换**（不只是卸载）。实测 Emby 的 `config.yaml` 放在 `@appcenter/<app>/config/` 被一次重装抹掉，应用直接瘫了半天
- **`@appdata`（TRIM_PKGVAR）只放日志/PID/临时运行文件**，配置真身与 DB/附件一律进 share（用户在文件管理器里看得见、改得了、重装还在）
- 目录解析用两跳：`/var/apps/<app>/shares/<share名>`（fnOS 维护的软链，永远指向真实卷，优先）→ `${TRIM_DATA_SHARE_PATHS%%:*}`（官方取**首段**写法）
  - ⚠️ `dirname $TRIM_DATA_SHARE_PATHS` 是**错的**：安装阶段该变量还是未落卷的占位路径（实测 `/vol0/@appshare/...`），拼出来的目录不存在也建不动
  - ⚠️ 四个生命周期脚本**各带一份** `resolve_share_root`，不 `source` 兄弟文件（卸载阶段 `cmd/` 可能已被清）
- ⚠️ 官方文档写 `share/` 单数，实测是 `shares/` 复数，以实测为准

## 红线
- **端口只认一个事实源 = `manifest service_port`（`$TRIM_SERVICE_PORT`）**。很多应用的配置文件的优先级高于环境变量，配置里写死的端口会盖掉 manifest 声明，导致 sidecar/网关与电视端连不上（实测 Emby：manifest 18096、config 8096 → 监听 8096，裸端口拒连）。所以 `cmd/main` 启动前必须把 `TRIM_SERVICE_PORT` **注入/覆盖**进应用自己的配置，别指望应用读 env。
- 注入配置要按应用解析器的规矩来，不能想当然按 YAML 语义：Emby-In-One 用的是自制行解析器（0 缩进切 section、`dataDir` 只认 0 缩进），`dataDir` 插到 `server:` 后面虽然能跑，但文件在人眼里就是坏的，换成真 YAML 解析器直接炸。**追加到文件末尾**最稳，且必须幂等（先删旧的再追加）。
- 模板本身慎改：改了要回归验证 build.sh 占位符替换逻辑

- 打包部署全流程知识 → Hermes 技能 `fnos-app-package`（Docker 容器 + 原生 systemd 双模式）
