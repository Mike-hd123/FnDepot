# hyatlas — 子项目 AGENT 须知

> 通用规矩见根 [AGENT.md](../AGENT.md)。本文件讲 hyatlas（混元记忆）特有的架构、打包层定制与血泪坑。

## 定位
8 层记忆服务（纯 Go 单二进制 + 内置 ONNX Runtime int8 向量模型 bge-large-zh 1024d），面板汉化 + 移动端响应式。
上游 = [tuancookiez-hub/HyAtlas-Memory](https://github.com/tuancookiez-hub/HyAtlas-Memory)（Apache-2.0），本仓库为 fork。
源码在 `src/`（Go 业务码，上游同源 + 本地定制），**fnOS 打包层在 `src/fnos-native/`**（cmd / config / wizard / manifest / build-fpk.sh / bin）。
本机现役端口 **19528**（loopback），桌面入口经 fnOS 网关 `/app/hyatlas`（socket 模式，sidecar `gateway_proxy.py` 转发 TCP）。

## 数据与路径（share 合规，4.7.3-2 起）
- `config/resource` 声明 `data-share`（name=**hyatlas**，`permission.rw=["hy-memory"]`）→ fnOS 自动建目录 + 授 Windows ACL。
- 数据实际落点：**`/vol2/@appshare/hyatlas/go-data`**（本机，由 fnOS 按卷决定 vol1/vol2）。
- `/var/apps/hyatlas/shares` 是 fnOS 维护的**软链**（本机 → `/vol2/@appshare`），故 `/var/apps/hyatlas/shares/hyatlas` == `/vol2/@appshare/hyatlas`。
- 四个生命周期脚本（`cmd/main`、`cmd/install_callback`、`cmd/upgrade_callback`、`cmd/uninstall_callback`）**各自内联一份** `resolve_share_root`（卸载阶段 `cmd/` 可能已被清，禁止 source 兄弟文件）：
  第①跳 `/var/apps/<app>/shares/<name>` 软链 → 第②跳 `TRIM_DATA_SHARE_PATHS` 首段（`${VAR%%:*}`），并跳过 `/vol0/*` 占位路径。
- `@appdata`（`TRIM_PKGVAR`）只放日志 / PID / HOME / TMPDIR / env 配置。

### ⛔ 已证伪的写法（禁止再用）
- `APP_SHARE="$(dirname "${TRIM_DATA_SHARE_PATHS}")"` —— 安装阶段该变量是**未落卷占位路径** `/vol0/@appshare/<app>`，dirname 拼出的目录不存在也建不动，`|| true` 把失败吞掉 → 日志写 "ready" 但数据从没进 share。**这是 hyatlas 两次「改名包后数据空」的根因**（4.7.3-2 已改正为两跳）。
- `${APP_DIR}/shares`（= `@appcenter`）—— 升级会整体替换 `@appcenter`，数据必丢。
- 官方文档写 `/var/apps/<app>/share/`（**单数**），实测系统用 **`shares/`（复数）**，以实测为准。

## 生命周期脚本职责
- `cmd/main`：start/stop/status。降权（`privilege username=hy-memory`，root 调用时经 `setpriv` 降权）；配置装载优先级 = shell env > `${TRIM_PKGVAR}/env` > 内置默认（`set -a` 导出，见「血泪坑」）。
- `cmd/install_callback`：运行用户兜底 → share 两跳解析 → **幂等不覆盖迁移**（配方 C：只在 share 目标为空时从老 `@appdata/@appcenter` `cp -a`，源保留不删，写 `${TRIM_PKGVAR}/migrate.log`）→ shares 软链存在性校验 → 属主归还 + env 首建（600 + hy-memory）。
- `cmd/upgrade_callback`：同款两跳解析 + **同款幂等迁移**（存量库进 share 的主通道，只拷不删不用 `mv`）+ env 只补新键不覆盖用户值。
- `cmd/uninstall_callback`：读 `wizard_delete_data` 双选分支。true → 先停 gateway+panel（含 `pkill` 兜底）再 `find "${SHARE_ROOT}" -mindepth 1 -maxdepth 1 -exec rm -rf {} +`（连隐藏文件一起清，保留目录本体便于重装复用）+ 清 `TRIM_PKGVAR`；false/其它 → 全保留。原有 PID/gateway/app.sock 清理段保留。
- `wizard/uninstall`：字段**严格** `wizard_delete_data`（系统约定，不可改名），默认 `false`（保留）。tips 强调这是长期记忆库、清除不可恢复。

## shares 软链校验（原「实体化自愈」，4.7.3-2 改为非破坏性校验）
历史故障：安装/升级期 fnOS 安装器曾把 `/var/apps/hyatlas/shares` **重建为实体目录**，应用随即读到空壳、真库留在 appshare（4.1.1-4 改名包后数据空过两次）。
现在主路径解析第①跳**就是读这条软链**，所以必须在 install/upgrade 里显式核对它：软链正常 → 只记录指向；被换成实体目录 → `mv` 留底（保留最近 2 份，**绝不 rm 数据**）后重建软链。
**保守保留的理由**：这条校验是唯一能发现「安装器重建 shares」故障的闸门，且它本身幂等 + 不破坏（有 bak、不删数据），保留成本低、失效代价高。

## 版本口径
上游 v4.7.3 → `4.7.3-N`（本地修订从 1 起，禁止改上游版本号）：
- 4.7.3-1 同步上游 v4.7.3（重编二进制 sha `1bc38015`）
- 4.7.3-2 **纯打包层 share 合规修复**（上游与二进制/模型零变更，沿用 4.7.3-1 的二进制；`build-fpk.sh` 用 `REUSE_BIN=1` 显式声明允许复用，身份闸降级为 WARN 但 `SHA_BIN == bin/ 实际 sha` 强校验始终生效）

## 血泪坑（别重蹈）
- **env 必须 `set -a` 导出**：只 `.` source 不 export，`HYATLAS_EMBED_KEY` 等静默丢失（2026-10-08 embed 401 事故根因）。
- **别用 Python 探可写性**：降权进程可能没有可用的 python3，且 `to_thread` 探测本身会抛错（曾导致 `parse failed` + `No model` 双重故障）。用 shell `test -w`。
- **ORT 缓存写 `$HOME/.cache`**：`HOME`/`TMPDIR` 必须指到 `TRIM_PKGVAR`（不能写 `/root`），否则降权进程加载模型失败。
- **`panel.pid` 会与真身错位**：`as_user` 里 `exec` 前的 `&` 子 shell 让 `$!` 记成已退出的过渡 PID → `status` 误判停用；已加 `pgrep -x hyatlas-go` 兜底回写校准。
- **日志/pid 的 root 属主会卡住降权进程**：启动前按运行用户身份 `test -w` 探测，不可写即删除重建。
- 二进制含 dashboard `go:embed` 内嵌前端：**前端有改动必须重编**再装进 `bin/`，只改 manifest 不算交付。

## ⚠️ 跨应用耦合：Octopus `data.db`（4.7.3-2 标注）
`cmd/main` 在 `HYATLAS_LLM_KEY` 为空时有兜底：从 **Octopus 的 `data.db`** 读 `settings.master_key` 当 LLM key。
**耦合点**：这读写的是**别的应用**的数据。另一个任务正把 octopus 的 `data.db` 迁进它自己的 data-share，届时老路径 `/vol1/@appdata/octopus/data.db` 会消失。
**当前处理**：改为**两处都试**（优先 octopus 的 share 路径 `/var/apps/octopus/shares/octopus/data.db`、`/vol{1,2}/@appshare/octopus/data.db`，再退回 `${TRIM_PKGVAR}/data.db` 与 `/vol{1,2}/@appdata/octopus/data.db`），全部取不到则保持 KEY 为空（LLM 判 `unconfigured`，不影响启动与本地向量召回）。
**残留风险**：① octopus 若换了 share 布局（子目录名变化）仍需同步本列表；② 只记路径与命中与否，**绝不打印 key / token / 记忆内容**。
**根治建议**：hyatlas 自己往 `${TRIM_PKGVAR}/env` 里显式写 `HYATLAS_LLM_KEY`（env 优先于兜底），或让 Octopus 暴露一个稳定只读接口。

## 打包
- `src/fnos-native/build-fpk.sh`：控制层归档原件 + `bin/hyatlas-go-linux-amd64` + `gateway/` + `ui/` + `config/env.example` → `app.tgz` → fpk；模型三件套（~355MB，不入库）按 `MODEL_SRC` → `/tmp/b2-models` → `REF_FPK` 提取。
- ⚠️ **本机升级唯一通道 = 应用中心 Web 手动安装**（CLI 全路径无效，见 skill `fnos-app-package`）。
- 产物落 `/vol2/1000/download/`，不自动装。
