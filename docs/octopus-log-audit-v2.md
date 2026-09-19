# Octopus 日志审计报告 v2

任务：t_b64c94ff
日期：2026-09-10
执行：灵犀（只读审计，全程 sudo 仅查询，未改配置、未重启、未 commit）

---

## 一、核心结论（先说人话）

**Octopus v0.13.3 没有「日志停写」问题。网关完全正常，日志一直在写，DB 一直在更新。**

「日志全停写 / stats 全 0 / apikey 全空」是**上一轮 worker 查错了数据库**造成的假象——
它查的是 `/vol2/@appdata/octopus/data.db`（一个 09-04 迁移时留下的**空库残留**），
而**真实数据库在 `/vol1/@appdata/octopus/data.db`**，一直活跃、一直有流量。

| 项 | vol1（真实，在用） | vol2（残留空库，上一轮查错） |
|---|---|---|
| DB 文件 | `/vol1/@appdata/octopus/data.db` | `/vol2/@appdata/octopus/data.db` |
| 大小 | 143360 B | 118784 B |
| 最近写 | **2026-09-10 10:59**（持续在写） | 09-05 04:08（死库） |
| api_keys | **1** | 0 |
| channels | **4** | 0 |
| channel_keys | **7** | 0 |
| groups | **4** | 0 |
| group_items | **47** | 0 |
| channel_grants | **84** | 0 |
| channel_models | **53** | 0 |
| users | **1** | 0 |
| stats_dailies | 每天都有（09-04..09-10 全在） | 仅 09-04/09-05 两条，全 0 |

> 环境变量铁证：进程 env `OCTOPUS_DATABASE_PATH=/vol1/@appdata/octopus/data.db`（vol1）。
> 进程 FD 13/4/7 指向 vol1 data.db / -wal / -shm。
> `/data/config.json` 里 `database.path` 也写的是 `/vol1/@appdata/octopus/data.db`。

---

## 二、任务1深挖：v0.13.3「日志停写」根因

### 现象核验（全部实测，不是上一轮的转述）

1. **进程活着且正常服务**
   - `ps aux`：`/vol1/@appcenter/octopus/octopus start` PID **2112321**，今天 **10:39** 启动
   - 监听 `127.0.0.1:8081`，`curl http://127.0.0.1:8081/` → **HTTP 200**（0.3ms）
   - `octopus-gw sidecar` `gateway_proxy.py` PID **2112420**，`app.sock` → HTML 正常返回
   - **实测** `GET /v1/models` → `chat / compression / default / flash` 四组齐全
   - **实测** `model=chat` 请求 → 200 OK 命中 `sensenova-6.8-flash-lite`（reasoning=0，说明 param_override 关思考也生效）

2. **panel.log 一直在写（vol1）**
   - 最后一段（v0.13.3）：
     ```
     Version:     v0.13.3
     Commit:      7a720e9
     Build Time:  2026-09-10 09:30:48 +0800
     2026/09/10 10:39:40 INFO Using config file: /data/config.json
     2026/09/10 10:39:40 INFO Program started, press Ctrl+C to exit
     ```
   - 前面还有 v0.13.2 的多段记录（09-05 重启多次、09-10 05:40 一次），**panel.log 全程在追加**，mtime 09-05 之后的 Access 也能持续 = 正在被写。

3. **真实 DB 流量巨大（vol1）**
   - stats_totals：累计 **1,159,788,751 input token / 19,514,636 output**，累计 wait 13.35 亿 ms
   - stats_dailies 最近几天（成功/失败）：
     - 09-10（今天）：**588 ok / 10 fail**，42.6M input / 388K output
     - 09-09：2774 ok / 72 fail，186M input / 2M output
     - 09-08：2325 ok / 75 fail，131M input / 3.3M output
     - 09-07：3433 ok / 126 fail，238M input / 4M output
     - 09-06：4278 ok / 4260 fail（商汤崩那天）
   - 说明：**网关 24/7 都在服务**，失败率也很正常（<3%，09-06 例外）。

4. **唯一「真 0」：gateway.log（vol1）为 0 字节 —— 但这是设计使然，非故障**
   - FD 1/2（stdout/stderr）被 appcenter 重定向到 `/vol1/@appdata/octopus/gateway.log`
   - 但 sidecar `gateway_proxy.py`（v8 版）**源码里没有任何 print/log 语句**（grep 无输出）
   - 所以 gateway.log 天然 0 字节；它**不是** Octopus 主进程的日志，是 sidecar 的 stdout 落点，而 sidecar 不写日志而已。
   - （gateway-proxy.log 里那两条 09-04 的记录是更早 v8 前代脚本写的，之后改到 gateway.log 落点就断了。）

### 上一轮 worker 错在哪（审计教训）

- **查错 DB**：`/vol2/@appdata/octopus/data.db` 是 09-04 迁移时留下的空库残留（`data.db.empty_20260904_195151` 备份旁），里面 schema 旧、0 数据。真实库在 vol1。
- **被** `vol2` 的 `gateway.log` / `panel.log` / `data.db-shm` 时间戳带偏**：vol2 里也有同名文件（09-05 的 install 痕迹），但那些是**旧打包轮次/残留**，不是当前进程在用的。
- **错误给结论**：编了「50 条日志、apikey=1 defualt」等，实际根本没查对位置。

**根因定性：不是配置缺失，不是版本 bug，不是 DB 锁，是「查错库」造成的误报。v0.13.3 安装正常、运行正常、日志和统计都在写。**

### 修复方向
- **无需任何修复**。当前 v0.13.3 运行健康。
- 如需消除「网关没日志」的困惑，可选（仅建议，本次未做）：
  - 可删 `/vol2/@appdata/octopus/data.db` 及其 -wal/-shm/备份（残留空库，占 240KB，非当前在用），避免后续排查再被带偏 —— 但删除前先确认 vol1 是唯一在用（已确认）。
  - 可考虑给 `gateway_proxy.py` 加一句启动打点日志（`print` 或 logging），让 gateway.log 有内容可查 —— 这属于**源码改动 + 重新打包 + 重启 sidecar**，属于任务报告范围外的落地动作，留待用户拍板。

---

## 三、任务2：上游是否发了新版

**是。v0.13.4 已于 2026-09-10 02:54:57Z（= 今天上午 10:54 北京时间）发布。**
用户说「又发了一版」指的就是它。

### v0.13.4 changelog（GitHub releases，2026-09-10T02:54:57Z，commit 6dce286）
- 🚀 Features：`✨ 请求头新增 client_header` — by bestruirui (1c48e)
- 🐞 Bug Fixes：`🐛 渠道禁用` — by bestruirui (0b919)
- 🏎 Performance：`🎨 前端日志UI` — by bestruirui (9a80d)
- Asset：`octopus-linux-amd64.zip` 21.3 MB，sha256 `484e10b599cac1824476eadab32960efcacf15fbf1254aec286e44e85d8cfb4e`

### 是否建议升级？
**不急于升级。** 理由：
1. v0.13.4 的 changelog **没有**「日志修复」「配置迁移修复」「DB 停写修复」相关条目——它解决的不是我们关心的「日志/统计」问题，且当前 v0.13.3 本就没有日志问题（是查错库）。
2. v0.13.4 新增 be `client_header`（请求头转发）和 `渠道禁用`（禁用渠道功能）——都是**功能增强**，对现网（聊天/看板/压缩四组正常跑）无紧迫性。
3. 升级会**重开一轮 fpk 打包 + 装机 + 重启 octopus（断 LLM 几秒）**，需用户知情决策。

### 若决定升级的路径
- 上游 tag：`v0.13.4`（commit 6dce286）
- 产物：`octopus-linux-amd64.zip`（21.3MB，sha256 `484e10b59...`）
- 本地版本号：`0.13.4-1`
- 沿用现有 `octopus/src/fnos` 打包骨架（manifest 改 version + changelog，`build.sh` 打 fpk）
- 因为 v0.13.4 删了 Gemini 渠道支持？——**没有**，那是 **v0.13.0** 的 warning（本次对比中可见 v0.13.0 的「⚠️ 升级前请先备份数据库 / 本次删除了 Gemini 渠道的支持」）。v0.13.3→0.13.4 无破坏性警告。
- DB 兼容：本机 migration_records 已到 v12，v0.13.x 系列迁移稳定，前几轮升级（v0.13.2→2→0.13.3）均无 DB 迁移问题。

---

## 四、交付物

- 本报告已存：`/vol2/1000/workspace/FnDepot/docs/octopus-log-audit-v2.md`
- 全程只读：未改配置、未重启进程、未停服务、未 commit。
- 复核命令（均可只读重现）：
  - 进程/环境：`ps aux | grep octopus`；`sudo cat /proc/2112321/environ | grep OCTOPUS_DATABASE`
  - 真实 DB：`sudo sqlite3 /vol1/@appdata/octopus/data.db "SELECT COUNT(*) FROM groups"` → 4
  - 日志：`sudo tail /vol1/@appdata/octopus/panel.log`
  - 功能：`curl -H "Authorization: Bearer $OCTOPUS_API_KEY" http://127.0.0.1:8081/v1/models`