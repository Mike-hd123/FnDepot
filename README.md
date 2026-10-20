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
| 云微(飞牛云微信) `wechat-on-cloud` | 1.5.2-2 | NAS 原生微信面板，Node.js 面板 + dockerode 管理微信实例容器，ipvlan(woc-lan) 单网卡直连局域网，数据落主机路径 bind（面板数据落 @appshare 抗升级覆盖），创建实例可选数据目录，移动端触屏优化，实例「电源」下拉(重启/关机)。1.5.2-1 修残包(app.tgz 缺 tar.ts/upload.ts)改全量重建 + 合上游 v1.5.2(QQ 下载改实时签名 #153)。1.5.2-2 补 share 合规：config/resource 声明 data-share（此前仅 api-scope），目录由 fnOS 创建与授权，脚本只校验存在性；数据落 ${SHARE_ROOT}/data，日志/PID 留 @appdata；迁移用 readlink -f 比物理路径（SHARE_ROOT 是软链、老路径是实体挂载点，字面不等但同体，按字符串比会把目录拷到自己身上）；卸载双选向导（默认保留，实例数据目录不在清理范围） | [Gloridust/WechatOnCloud](https://github.com/Gloridust/WechatOnCloud) |
| HyAtlas(混元记忆) `hyatlas` | 5.0.0-1 | AI 长期记忆系统（8 层）。v4 纯 Go 单二进制：内置 ONNX Runtime int8 向量引擎(bge-large-zh 1024d)，chromem-go 存储，fnOS 网关 socket 入口 /app/hyatlas，端口 19528。5.0.0-1 同步上游 v4.9.1→v5.0.0（49 commits / 148 files / +11715 −2412，上游首个 5.x 大版本）——检索改显式流水线（embed once + MMR 多样性 HYATLAS_MMR_LAMBDA + 链接事实补入 + 层配额 + 命名步骤）、别名（HYATLAS_USER_ALIASES）全面进 store 层（owners/canonicalOwner/ownerWhere）、catch-up 补抽（漏抽/抽失败的 L2 定期补抽）、写路径单次 embedding + 倒排关键词索引、镜像 Hermes replace/remove 为可逆 forget、/flush + 优雅停机（等待在跑抽取）、typed retire 记录、周期事件 series、tokenizer 有界扫描与 embedding 张量释放；fork 定制全保留并逐项核验（System1 不写 L1、L7 dedup 0.80+硬顶 100、includeExpired 六参签名、NewMemoryStore dims、delete_guard 三道闸 + dry_run 默认开 + 审计、bge-large-zh 1024d 双布局 + longestToken 有界扫描移植、touch/patch、LLM 关思考补丁、dashboard 汉化零触碰、端口 19528）；持久层换上游 1s flushLoop（fork 的 HYATLAS_INDEX_FLUSH_SEC debounce 合流器退役，1s=fork 默认，行为等价），/api/v1/flush 新增；go build/vet 全绿，go test 失败 12 项 = 旧基线 9 项逐名保持（SetExtracted 被上游修复转 pass）+ 新增 3 项（上游 L1 mirror/admin 闸断言 2 项属 fork 语义差异、episode TempDir 夹具竞争 1 项单跑 6/6 过）——按「新失败即停」约定**未推送**等拍板；二进制 sha bb3376bab085b4a5…(16,873,936B，go1.27.1，-trimpath -buildvcs=false，/vol2 与 /vol1 两份不同挂载重编逐字节一致)；fpk 243,464,854B sha 4ee0a69595b1f621…两次重打逐字节一致。4.9.1-1 同步上游 v4.9.0→v4.9.1（9 commits / 16 files / +713 −57，全 server 侧）——reconcile 覆盖同一人的所有别名 id 且一条新事实只走一种操作、任务经验不再被抽取顶掉且重复经验只在经验集合内判定(0.92 不变)、episode 摘要失败改 5/10/20/40/80 分钟退避六次(约 2.5h，不再 3 次即丢)、图谱辅助召回落在结果上限之内(≤1/3、排末尾)且纯关键词检索不放向量候选；go build/vet 全绿，go test 失败 11 项与 4.9.0-1 基线逐名相同→零新增回归；二进制 sha 5ef89a1532366c4e…(15,786,320B，两份不同挂载重编逐字节一致)。**4.9.1-1 另含 fork 补丁**：`llm.go` 请求体统一在 `chatOnce` 构造，默认带顶层 `enable_thinking:false`（`HYATLAS_LLM_DISABLE_THINKING=off` 回上游姿势）、`HYATLAS_LLM_MAX_TOKENS>0` 才发 max_tokens、遇 400 自动去键重试一次；治的是"octopus 各别名 pri=1 全是 reasoning-first 模型、整理每轮把 10 分钟预算烧在推理上而净进度≈0"（同一输入实测 token 从 11,673 降到 732）。最终二进制 sha `1cb6d4cc…`(15,795,272B)。4.9.0-2 为纯控制层修复（上游与二进制、模型零变更）——实测装机后 fnOS 把 `/var/apps/hyatlas/shares` 重建成实体空目录，旧解析被空目录骗过导致服务读到系统盘空壳库、真库无人读、面板全 0：现在 ①解析第①跳只认「shares 是软链且 realpath 落在 /volN」；②新增按各卷 `go-data/doc_index.json` >1MB 找回真库的救援跳；③自愈建链硬闸（拒绝源为空/源等于自身/源不在 /volN，杜绝自指软链）；④cmd/main 空壳库保护闸——生效库是空壳而别卷有真库时拒绝启动并写可见错误（`HYATLAS_ALLOW_EMPTY_START=on` 可放行，`HYATLAS_GO_DATA` 可钉死数据路径）。4.9.0-1 同步上游 v4.7.3→v4.9.0（48 commits / 71 files，+6173 −375）——事件日期 event_at/event_date、查询里认时间窗(昨天/上周/11月/具体日期，窗内分数下限放宽 0.20)、更新计数(同一件事写"两次"不产生近似重复)、意图按到期日失效(不再固定 7 天)、episode 摘要(HYATLAS_EPISODE_GAP 默认 30m，一段工作产一条 L4；上游已移除"每写一条产一条 L4"，关 episode 即失去 L4 产出路径)、读时带历史 history(was: … until …)+superseded_how、用量学习独立存 usage_learning.json、程序不再读别的应用凭证并新增 POST /api/v1/llm/key 运行时交 key(只留内存)、任务经验 experiences 独立召回通道、schema 保留证据链、>300 字长事实按句进只服务搜索的 l3_clause 集合(启动自动 backfill)、实体加成 + 图谱辅助召回、按查询类型自适应权重、召回配额与 schema/意图槽位、可选写后对齐 HYATLAS_RECONCILE(默认 off)；fork 定制全保留并逐项核验(System1 不写 L1、L7 dedup 0.80 + 硬顶 100、search/searchWhere 的 includeExpired、NewMemoryStore dims、delete_all 加固、touch/patch 双时态、bge-large-zh 1024d + embedDims、dashboard 汉化 + 移动端、shares 两跳、19528 + 网关 socket)；二进制 sha e3dd4abfa1eb7cca…(15,780,328B，go1.26.5，/vol2 与 /vol1 两份不同挂载重编逐字节一致)，go build/go vet 全绿，go test 失败 11 项 = 基线 10 项 fork 语义差异 + TestEpisodePersistsAcrossRestart(已在上游 v4.9.0 纯净树复现同款 TempDir 竞争，非本次回归)→ 零新增回归；env.example 补 12 个代码在读但模板漏记的键。4.7.0-1 同步上游 v4.7.0（L8 Entity pages 每件事一页 + Now page 当前工作记忆 + 使用学习 usage.json + 积压处理 backlog 作业）；4.7.2-1 再同步 v4.7.1+v4.7.2——consolidation 从周级降到小时级(cluster 持久化进 consolidate_state.json，只跑变化的 cluster，2 并行 + 20 分钟 catch-up + 单 pass 10 分钟预算)、提取超时 180s→300s、新增 fallback model(HYATLAS_LLM_FALLBACK_MODEL)、backlog 只重试失败且 provider 故障不弃单(跳过的 cluster 24h 后 failCooldown 重试)、consolidation 跟随 HYATLAS_USER_ALIASES 归 owner、fallback endpoint 跨 host 不带主 key。4.7.3-1 同步上游 v4.7.3（3 commits / 16 files）——合并质量守卫(cosine<0.65 的 fact 不折进合并、不足 2 条不合并、报表 kept_off_topic)、schema 相关性(取 cluster 最近邻而非 owner 最新 20、与已存 ≥0.92 不重复写)、history 追踪(新增 history.go，supersede 链可追溯、删除时指后继、补记 dropped without a recorded reason)、/api/v1/list 分页按 id tie-break(修同时间戳漏行/重复)、新增 POST /api/v1/admin/restore(需 HYATLAS_ADMIN=on、默认 dry run、恢复错合/错删行)；前端修复 dashboard LAYERS 补 l8_entity(实体页 #c0715b)、totalLayers 7→8（L5-L7 图谱标签保留，L8 非图谱节点，与上游一致）。4.5.0-1 同步上游 v4.4.0 + v4.5.0（27 commits / 66 files）——搜索改 hybrid(BM25+向量 RRF，reader=legacy/keyword_tag/hybrid_v2 三态)、L5 知识转为可检索(启动 BackfillL5 幂等补齐历史图边，无开关)、新增 /api/v1/admin/compact_raw 与 dedupe_facts 维护端点(默认 403，HYATLAS_ADMIN=on 才放行；compact_raw 不可逆)、consolidation 按 owner 分组且 batch 默认 200→50 避超时、事实改用 supersede(invalid_at/superseded_by) 不再删除、fingerprint 改 fact IDs 哈希(升级后每 owner 重跑一轮)、drop 必须带 reason、routes() 抽出并套 guardLocal(HYATLAS_ALLOWED_HOSTS)+limitBody(8MiB)。4.5.0-2 增 delete_all 防误删护栏：默认 dry_run 只统计、按 id 的窄删除(遗忘功能)不受限、只给单个 user_id/agent_id 的宽范围删除需 HYATLAS_ADMIN=on 且 confirm_mass_delete=true、命中数超 HYATLAS_DELETE_MAX(默认 1000) 需显式确认、实际删除写审计日志。本地定制保留（L7 去重硬顶、L1 不被异步提取写、touch/patch 双时态、SearchIncludeExpired 软过期可见、dashboard 全量汉化 + 移动端抽屉 + 时间线分页、shares 自愈、/vol1/@appdata/hyatlas/env 配置化）。4.7.3-2 为纯打包层修订（上游与二进制、模型三件套均零变更，SHA 断言保持）：弃用已被实测证伪的 `dirname "${TRIM_DATA_SHARE_PATHS}"`（安装阶段该值是未落卷占位路径 /vol0/@appshare，拼出的目录既不存在也建不动；多值时还会得到含冒号的非法路径），改为官方两跳解析；inode 实测新旧解析同为 39:712570，1.5G 记忆库原地不动不搬家；卸载双选向导（默认保留，点明长期记忆正源、清除不可恢复）**fork 源码随本仓库 `hyatlas/src/` 维护**，上游见 [tuancookiez-hub/HyAtlas-Memory](https://github.com/tuancookiez-hub/HyAtlas-Memory) | [tuancookiez-hub/HyAtlas-Memory](https://github.com/tuancookiez-hub/HyAtlas-Memory) |
| Octopus `octopus` | 0.13.12-2 | LLM API 聚合网关，Go 单二进制 + 内嵌前端 + SQLite，多渠道聚合/协议转换/故障转移，端口 8081（agent 模型出口，最高优先级服务）。二进制=上游官方 CI 原样，本地价值在 fnOS 打包层：gateway sidecar 剥前缀反代 + SSE 流式透传 + 动态 cache-bust，手机端 /app/octopus 全链路可路由。0.13.12-2 补 share 合规：此前是「假合规」（data-share 声明了、@appshare/octopus 也被建出来了，但 DATA_DIR 取 TRIM_PKGVAR，data.db 含 api_keys 与 config.json/listen.conf 实际全在 @appdata，共享目录是空壳）；现数据三项全入 share，日志/PID 留 @appdata；迁移按白名单文件幂等（已有即 skip，沙箱实测逐字节未覆盖）；卸载双选向导；跨应用已同步——hyatlas 读 data.db 取 master_key 改为候选顺序试 share 与 @appdata 两处 | [bestruirui/octopus](https://github.com/bestruirui/octopus) |
| EZ记账 `ezbookkeeping` | 2.0.1-2 | 家庭记账：本地优先 SQLite 存储，多账本/预算/报表，支持微信/支付宝/信用卡账单导入，gzip 压缩提速，移动端触屏优化。v2 新增信用卡额度/可用额度环、洞察报表自定义图表、S3 对象存储，1.x→2.x 数据自动迁移无损。2.0.1-2 补 share 合规：config/resource 从空 `{}` 改为声明 data-share「ezbookkeeping」，db/附件/ini 全落共享目录（文件管理器可见），@appdata 只留日志与 PID；幂等迁移（源不删、写 migrate.log、已在 share 不重复搬运）；每次启动强制把 ini 的 db_path/log_path/local_filesystem_path 重写为 share 路径——抢救来的老 ini 里写死的 @appdata 不会漏；卸载双选向导 | [mayswind/ezbookkeeping](https://github.com/mayswind/ezbookkeeping) |
| 待办(Vikunja) `vikunja` | 2.7.0-2 | 自托管待办面板：Go 静态 ELF 单二进制 + SQLite，中文 UI + CalDAV，API token 全自动读写（Hermes 提醒引擎），桌面 3456 + 手机 /app/vikunja 双通道。v2.7.0 同步：6 安全修复+MCP server+前端切 v2 API（sidecar 锚点存活实测）+12 条 sqlite 迁移净室验证 tk_ token 不破。2.7.0-2 补 share 合规：config/resource 从 2.6.0 起就声明了 data-share 但脚本从未使用（vikunja.db/files/logs/config.yml 全落 @appdata，用户不可见=声明与实现脱节），现数据与配置真身入 share；修复 cmd_main 调用未定义的 die（share 解析失败会报 command notfound 而非可读提示）；set -e 下的命令替换赋值加保护（此前解析失败会整段跳过 systemd unit 清理，残留旧 unit 与新 app_id 冲突）；运行用户统一为 `$TRIM_USERNAME`（unit/chown/permission 四源同源，此前 unit 写 hermes-studio 而 data-share 只授 vikunja，无本机 override 的全新安装会拿不到 share 权限而起不来）；卸载双选向导 | [go-vikunja/vikunja](https://github.com/go-vikunja/vikunja) |
| Emby `emby-in-one` | 1.4.4-5 | Emby 多账号聚合反向代理：Go 静态二进制 + Python socket 网关 sidecar，治 Emby Boost CDN 签名 URL 900 秒过期拖拽 403；后台 token 失效自动重登无感续命；桌面入口 /app/emby/admin/，TV 直连 18096 | [ArizeSky/Emby-In-One](https://github.com/ArizeSky/Emby-In-One) |
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

**HyAtlas 5.0.0-1 fpk**：232MB（243,464,854B，sha256 `4ee0a69595b1f621e842d5aacc4f61680f558c62515b230219fba1a02f744835`；二进制 sha `bb3376bab085b4a568c48ad9ce2bd15e1c69c96509cea66cac2bc371e5459ae3` / 16,873,936B，go1.27.1，-trimpath -buildvcs=false，/vol2 与 /vol1 两份不同挂载重编逐字节一致），本地产物 /vol2/1000/download/hyatlas-5.0.0-1-x86.fpk，两次重打逐字节一致；Release [hyatlas-v5.0.0-1](https://github.com/Mike-hd123/FnDepot/releases/tag/hyatlas-v5.0.0-1)。上一发布 **4.9.1-1 fpk**：231MB（242,768,007B，sha256 `2d2550e9…`；内含 fork 的 LLM 关思考补丁，二进制 sha `1cb6d4cc…` / 15,795,272B。先前未含补丁的同名产物 242,761,769B / sha `8d6ef6a2…` 已在本机留底 `/vol2/1000/workspace/hy491-work/hyatlas-4.9.1-1-x86.fpk.pre-llmpatch`，未发布过、可直接删），本地产物 /vol2/1000/download/hyatlas-4.9.1-1-x86.fpk；已发布 Release [hyatlas-v4.9.1-1](https://github.com/Mike-hd123/FnDepot/releases/tag/hyatlas-v4.9.1-1)，附件服务端 digest 与本地 sha256 逐字节一致，不进 git 跟踪。上一版 4.9.0-2（242,758,303B，sha256 `624366c8…`）已发 Release，其二进制 `e3dd4abf…` 在本仓库留底为 `hyatlas/src/fnos-native/bin/hyatlas-go-linux-amd64.bak_20261018_4902`。该包二进制与 4.9.0-1 相同（sha `e3dd4abf…`，REUSE_BIN=1 的打包层修订），模型三件套逐件 sha 零漂移。fork 源码在本仓库 `hyatlas/src/`，上游工程 [tuancookiez-hub/HyAtlas-Memory](https://github.com/tuancookiez-hub/HyAtlas-Memory)。历史包：4.9.0-1（242,752,791B，sha `7068b75e…`）**只留本机未发 Release**（装后触发 fnOS shares 实体化故障，已被 4.9.0-2 覆盖，二进制与 4.9.0-2 相同）、`hyatlas-v4.7.3-2` 留 4.7.3-2（242,654,348B，sha `9aceae36…`）、`hyatlas-v4.7.3-1` 留 4.7.3-1（242,648,812B，sha `7a8fcc6a…`）、`hyatlas-v4.7.2-1` 留 4.7.2-1（242.6MB）、`v2026.10.12` 留 4.6.0-1、`v2026.10.08` 留 4.5.0-2（fnpack.json releases 六条均在，4.7.0-1 未单独发 Release，被 4.7.2-1 覆盖）。

## 打包说明

- 打包工具：fnOS 官方 `fnpack build -d <project-dir>`，产物内 manifest appname/version 与 fnpack.json 一致。
- 云微改动全在 `src/panel`（面板 web）与 fpk 打包层（cmd/、manifest），本仓库 `src/` 与 [Mike-hd123/WechatOnCloud](https://github.com/Gloridust/WechatOnCloud) 同步。
- HyAtlas v4 汉化与内置向量引擎在 Go 源码内；fork 源码随本仓库 `hyatlas/src/` 一起维护（上游 [tuancookiez-hub/HyAtlas-Memory](https://github.com/tuancookiez-hub/HyAtlas-Memory)），fnOS 控制层与打包脚本在 `hyatlas/src/fnos-native/`（22 文件），模型三件套(~355MB)超 GitHub 限制不入库。fpk 走 GitHub Release 分发，构建确定性可复现：同一 commit 重编三次 sha256 逐字节相同。
- 发布者：Mike · https://github.com/Mike-hd123
