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
上游版本 → `<上游版本>-N`（本地修订从 1 起，禁止改上游版本号）：
- **4.9.0-1 同步上游 v4.7.3→v4.9.0**（48 commits / 71 files / +6173 −375；重编二进制 sha `e3dd4abf`，15,780,328B；`build-fpk.sh` 的 `VER` 变量与 `SHA_BIN`/`REUSE_BIN=0` 已同步；产物 242,752,791B，sha256 `7068b75e…`，fpk 端到端重打逐字节可复现）
  - 合并手法：`git apply --reject --directory=hyatlas/src` 打上游区间补丁（排除 `plugins/**`），31 个 fork 缺失文件整取上游，再手工合两份必冲突大文件
  - **每次都要做的固定回填**（漏一处就编不过/行为漂移）：①上游新代码的 `searchWhere(...)` 是 5 参，fork 是 6 参（多 `includeExpired`）→ 新调用点补 `false`；②测试里的 `NewMemoryStore(...)` 上游 4 参，fork 5 参（多 `dims`）→ 补维度；③`Embedder` 上游新增**可选** `Semantic()`（类型断言识别，不强制实现），fork 的 `LocalEmbedder` 建议补；④测试 helper 撞名要改名（本次 `search_490_test.go` 的 `hasID` → `hasIDMap`，因为 `metadata_test.go` 已有同名）
  - 已知失败基线：**11 项**（fork 语义差异 10 项：L1 mirror 3、L7 dedup 1、SetExtracted 1、delete_all 守卫 3、describeEmbed/监听行 2）+ `TestEpisodePersistsAcrossRestart`（**上游自带夹具竞争**：TempDir RemoveAll 时后台 episode 还在写；在未合并的上游 v4.9.0 纯净树上同样失败，不是回归）
- **4.9.1-1 同步上游 v4.9.0→v4.9.1**（9 commits / 16 files / +713 −57，全 server 侧；上游 Hermes 插件未变 → `plugins/**` 一律不取）。本轮上游是复查后的修复，四条最该记：reconcile 覆盖别名 id、一条新事实只走一种操作、**抽取不再看到 task experience**（否则普通事实会把经验的 task/solution/environment 顶掉）、**episode 摘要失败改 5/10/20/40/80 分钟退避六次**（此前 3 次即永久丢，走易限流的端点等于白丢一段工作）、图谱辅助召回落在 limit 之内（≤1/3、排末尾）。合并 reject 4 处：`experience.go`（上游改成先 `embed` 再 `searchWhere(qv, limit, layer, where)` → fork 补第 5 参 `false`）、`extract_context.go`（`extractRelatedMax*2` + 满额 break + 跳过 `isExperience` → fork 的 `search` 补第 6 参 `false`）、`.env.example`（只有示例模型名的注释行）、`README.md`（banner alt 用上游新措辞、保留 fork 的 eight-layer 修正与 `V3_V4_COMPARISON` 指针行）。质量：build/vet 全绿；`go test` 失败 11 项与 4.9.0-1 基线**逐名相同** → 零新增回归；两份不同挂载重编逐字节一致（`5ef89a15…`，15,786,320B），fpk 两次重打一致（`8d6ef6a2…`，242,761,769B）。旧二进制留底 `bin/hyatlas-go-linux-amd64.bak_20261018_4902`（`e3dd4abf…`）。
- **4.9.0-2 纯控制层修复**（上游、二进制与模型三件套零变更，沿用 4.9.0-1 的二进制 `e3dd4abf`，`REUSE_BIN=1`；产物 242,758,303B，sha256 `624366c8…`，两次重打逐字节一致）。修的是 4.9.0-1 装机复发的「shares 被安装器重建成实体空目录 → 服务读空壳、面板全 0」：
  - 解析第①跳**只采信 shares 本身是软链、且 realpath 落在 `/volN`** 的情形；第②跳吃 `TRIM_DATA_SHARE_PATHS` 首段的两种形状；新增第③跳**按各卷 `go-data/doc_index.json >1MB` 找回有货真库**的救援
  - 自愈建链改 `heal_shares_link()` 硬闸：源为空 / 源等于软链自身 / 源不在数据卷 → **一律拒绝**（旧版正是 `ln -sfn` 源=目标建出自指链）；heal 后复核 `SHARE_ROOT`/`DATA_DIR` 必须落 `/volN`
  - `cmd/main` 加**空壳库保护闸**：生效库 `doc_index.json <1MB` 而别卷有 `>1MB` 真库 → 拒绝启动，错误写 `panel.log` + `TRIM_TEMP_LOGFILE`（应用中心可见）；`HYATLAS_ALLOW_EMPTY_START=on` 才放行；`config/env.example` 补 `HYATLAS_GO_DATA`/`HYATLAS_ALLOW_EMPTY_START`（注释行，升级合并不注入）
  - 回归：`/vol2/1000/workspace/hy490-work/test_share_resolve.py`（17 断言）+ `test_empty_guard.py`（12 断言），二者都是**从出货脚本按 if/fi 配对抽真实代码**挪进 /tmp 沙箱重演现场；`/vol2/1000/workspace/hyatlas-fix-shares.sh` 是这类故障的停机修复器
- 4.7.3-1 同步上游 v4.7.3（重编二进制 sha `1bc38015`）
- 4.7.3-2 **纯打包层 share 合规修复**（上游与二进制/模型零变更，沿用 4.7.3-1 的二进制；`build-fpk.sh` 用 `REUSE_BIN=1` 显式声明允许复用，身份闸降级为 WARN 但 `SHA_BIN == bin/ 实际 sha` 强校验始终生效）

## v4.8/v4.9 上线后要注意的语义变化
- **L4 摘要不再是「每写一条产一条」**：改由 episode 聚合（`HYATLAS_EPISODE_GAP` 默认 30m）。把该值设成 `0/off/false` 会**彻底关掉 L4 产出路径**（旧路径上游已删），别当成"省资源开关"随手关。
- **`HYATLAS_RECONCILE` 默认关，本机也保持关**：打开等于每条写入多一次 LLM 调用，octopus 免费额度与延迟都不宽裕。
- **key 交接改走 `POST /api/v1/llm/key`（只留内存）**：上游明确禁止任何程序读别的应用凭证文件（Hermes `auth.json` 那套）；本机走 fnos env 的 `HYATLAS_LLM_KEY` 显式配置，`cmd/main` 里读 octopus `data.db` 的兜底属于历史遗留，后续可用该端点摘掉。
- **DSH 侧插件要跟着改渲染**：`~/hy-memory/index.js` 的召回块原来只认四通道；v4.9.0 多了 `experiences` 通道、每条命中多了 `event_date`/`history`，已在 2026-10-18 适配（`CHANNEL_LIMIT.experiences=300`、事件日优先显示并标 `记于`、`history` 只取 1 条截 120 字符）。改 JS 后必须重启 DSH。

## 装机后「界面全 0 / 记忆没数据」的标准判法（2026-10-18 第三次复现后固化）
先分清**是服务读到空库**还是**真库被搬走/损坏**——九成是前者，数据通常毫发无损：
```bash
curl -s --noproxy '*' http://127.0.0.1:19528/api/v1/status | python3 -c "import json,sys;d=json.load(sys.stdin);print(d.get('version'),d.get('vdb_points'),(d.get('layers') or {}).get('l2_raw'))"
sudo ls -l /proc/$(pgrep -x hyatlas-go|head -1)/fd | grep -o '/[^ ]*go-data' | sort -u   # 进程在读写哪一份
stat -c '%s %n' /vol*/@appshare/hyatlas/go-data/doc_index.json                            # 真库多大、在哪
ls -ld /var/apps/hyatlas/shares; readlink /var/apps/hyatlas/shares                        # 期望：软链 -> /volN/@appshare
```
- `shares` 变成**实体目录**（不是软链）= 安装器又把它重建了：服务会在系统盘上现造一个空壳 `go-data`，真库留在 `@appshare` 没人读。修复走 `bash /vol2/1000/workspace/hyatlas-fix-shares.sh`（只读预检）→ `sudo … --apply --pin-env`（mv 留底、绝不 rm、建正链、钉 `HYATLAS_GO_DATA`）。**必须先停服务**：运行中的进程内存里是空索引，一旦换路径回写会把真库 `doc_index.json` 覆盖成空的。
- 误库里的新写入别丢：留底目录下的 `doc_index.json` 逐条取 `content` 回灌 `POST /api/v1/add` 即可（`/add` 只写 L2，抽取会重新加工，重复由 dedupe/supersede 收敛）。
- **启动慢 4–5 分钟不是故障**：HTTP 监听在 `NewMemoryStore` 之后才开，而 `rebuildIndex()` 要把 11.8 万个 `.gob` 全量读一遍（本机 1.3GB，HDD 顺序读）。判进度：`sudo cat /proc/<pid>/io` 的 `read_bytes` 对比 `sudo du -sb go-data`。这期间面板与召回都是空的，别急着判"没数据"。

## 血泪坑（别重蹈）
- **env 必须 `set -a` 导出**：只 `.` source 不 export，`HYATLAS_EMBED_KEY` 等静默丢失（2026-10-08 embed 401 事故根因）。
- **别用 Python 探可写性**：降权进程可能没有可用的 python3，且 `to_thread` 探测本身会抛错（曾导致 `parse failed` + `No model` 双重故障）。用 shell `test -w`。
- **ORT 缓存写 `$HOME/.cache`**：`HOME`/`TMPDIR` 必须指到 `TRIM_PKGVAR`（不能写 `/root`），否则降权进程加载模型失败。
- **`panel.pid` 会与真身错位**：`as_user` 里 `exec` 前的 `&` 子 shell 让 `$!` 记成已退出的过渡 PID → `status` 误判停用；已加 `pgrep -x hyatlas-go` 兜底回写校准。
- **日志/pid 的 root 属主会卡住降权进程**：启动前按运行用户身份 `test -w` 探测，不可写即删除重建。
- 二进制含 dashboard `go:embed` 内嵌前端：**前端有改动必须重编**再装进 `bin/`，只改 manifest 不算交付。
- **`case` 模式里引号位置会静默失效**：`*/"/${_name}")` 在 bash 5.2 下**匹配不上**，`*/"${_name}")` 才匹配（引号跨在 `/` 前后会改变模式解析）。4.9.0-2 的沙箱测试就是这么抓出「`TRIM_DATA_SHARE_PATHS` 已是 `<app>` 本体时多拼一层 `hyatlas`」的真缺陷——**路径解析类逻辑必须有沙箱场景测试**，光 `bash -n` 和肉眼都不够。
- **自愈脚本的 `ln -sfn` 必须挡住自指**：`ln -sfn "$SRC" "$LINK"` 当 `$SRC` 恰好等于 `$LINK` 时不会报错，而是建出一条指向自己的软链，后续 `mkdir -p` 会顺着它把空壳目录"造成功"，看起来一切正常。任何"重建软链"的代码都要显式拒绝 ①源为空 ②源等于目标 ③源不在数据卷 `/volN` 上这三种输入。

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
