# octopus.fpk — Octopus 安装包

- 版本：0.13.12-1（2026-10-14）
- 打包时间：2026-10-14
- 上游项目：[bestruirui/octopus](https://github.com/bestruirui/octopus)
- 上游 commit：`3d1ff13`（tag v0.13.12，2026-10-14 11:22 +0800）
- 源码：本目录 `src/`（上游 v0.13.12 源码）+ `src/fnos/`（fnOS 打包层：cmd/manifest/config/gateway/ui/wizard）
- 打包方式：fnOS `fnpack build`（v1.2.4，`/usr/local/bin/fnpack`）

## v1/0.13.12-1（2026-10-14，本地补丁号归零重开）

同步上游 **v0.13.11 + v0.13.12**（本地跳过 0.13.11 直接并到 .12，两版均为增量）：

- **v0.13.11 `fd8b3b8`**（feat）流式响应更严格检查：空启动事件暂存到「输出开始或成功结束」，期间失败仍可换目标重试；流式耗时口径改为「到启动事件可提交为止」；同协议透传请求补 `Cache-Control: no-cache` / `Connection: keep-alive`。
- **v0.13.12 `baa5d65`**（fix）请求头 UA 透传修复。

二进制为上游官方 CI 产物原样（`octopus-linux-amd64` sha256 `96474a06aa7d333ca12992f6fbe40d60544eb35946970ed79df5ee40b854d9f0`；反解 Version=v0.13.12 / Commit 3d1ff13 / Built 2026-10-14 11:22:42 +0800；与 Release `SHA256SUMS` 中 `octopus-linux-amd64.zip` 条目 `fb48a40a…` 解出件一致）。**`internal/conf` 与 `main.go` 逐字节未变，无 breaking change、无配置键变更**，src 全量核对无残留差异。

### ★ 新增：监听地址开关（默认只监听 127.0.0.1）

**背景**：fnOS 统一网关注入 `host=127.0.0.1`，照搬会让 octopus 只听回环，局域网客户端（手机/电视/其它机器）连不上 8081——历史上只能用 socat 桥到 8082 绕路。

**做法**：把「听哪儿」交给用户显式选择，**直接复用上游原生键 `OCTOPUS_SERVER_HOST`**，不发明第二层配置。依据上游 `internal/conf/config.go`：`viper.AutomaticEnv()` + `SetEnvPrefix("octopus")` + `EnvKeyReplacer(".", "_")`，故 `server.host` → `OCTOPUS_SERVER_HOST`，且**优先级高于** `data/config.json` 里的 `server.host`。

- **默认 127.0.0.1**：只服务本机消费方 —— fnOS 网关 sidecar（`app.sock` → 8081）与 DSH/Hermes 本机模型出口。暴露面最小。
- **可切 0.0.0.0**：局域网直连 8081，供飞牛 app / 手机 / 其它设备使用。

| 打开方式 | 操作 | 特点 |
|---|---|---|
| A) 持久开关（推荐） | 编辑 `/vol1/@appdata/octopus/listen.conf` → `OCTOPUS_LISTEN_HOST=0.0.0.0` | 与 `data.db` 同级，**升级/重装不丢**；首次启动自动落带注释样例 |
| B) 临时覆盖 | 启动前设环境变量 `OCTOPUS_LISTEN_HOST=0.0.0.0` | 覆盖 A，不落盘 |

改完**重启应用**生效。取值白名单只认 `127.0.0.1` / `0.0.0.0`，非法值告警并回落默认（防手滑写错致监听失败或意外暴露）。

实测（包内真实二进制 + 包内真实 `cmd/main`，四态全绿）：

| 场景 | resolved | 实际监听 |
|---|---|---|
| A 首次启动（自动落样例） | 127.0.0.1 | `127.0.0.1:8081` |
| B listen.conf=0.0.0.0 | 0.0.0.0 | `*:8081` |
| C 改回 127.0.0.1 | 127.0.0.1 | `127.0.0.1:8081` |
| D 环境变量覆盖 | 0.0.0.0 | `*:8081` |

### 打包脚本的严重坑（本轮实测修复，务必知悉）

`build-fpk.sh` 原先只同步「二进制 + sidecar + manifest」，**不复制 `fnos/cmd/`**——骨架 fpk 里的旧 `cmd/main` 会被原样带过去，于是**新功能根本没打进包，而构建全绿**。本轮监听开关首包就是这样被静默丢弃的（包内 `cmd/main` 仍是 3274 B 旧版）。自检当时只查「`cmd/main` 是否存在」、不查内容，完全挡不住。

**修法**：脚本新增 3b 步显式同步 `fnos/cmd`、`fnos/ui`、`fnos/wizard` 整层 + 逐文件 `cmp` 内容复核；5/5 自检改为**内容级**校验（包内 `cmd/main` 与 `fnos/cmd/main` 逐字节比对），并对监听开关做存在性回归护栏。**「存在性检查挡不住旧内容」是本轮最贵的教训。**

### 保留的本地补丁

gateway sidecar v8 全部保留：API 前缀手工改写默认关（`GATEWAY_API_REWRITE=0`，上游 ≥0.13.10 前端自带子路径解析）、cache-bust 改为运行时取 `index.html` entry chunk 文件名、`gw-debug.log` 诊断、`ui/config` + `ui/images` 图标齐全。

- 产物：sha256 `96ad6c70c8656cf7ced719b821021de712c207a97ec989eefd95c9212aae39db`，22,683,210 B；落 `/vol2/1000/download/octopus-0.13.12-1-x86.fpk`

## 历史版本

### v1/0.13.10-1（2026-10-08，本地补丁号按规矩归零重开）

同步上游 v0.13.10（commit `d1a26bd`）——数据库迁移修复 `1cdb163`、新增 passwd 改密命令 `8cf04fb`、分组编辑置顶置底 `0538c3e`、**子路径反代支持 `bdc9948`**。二进制为上游官方 CI 产物原样（`octopus-linux-amd64` sha256 `fe85cec994fceaf13634e1bbd6ca8fb852fe9eaff38a4c6f82c3466607320b94`）。

#### 关键修复：关闭 sidecar 的 API 前缀手工改写（`GATEWAY_API_REWRITE` 默认 0）

**症状**：0.13.10-2 ~ -5 手机端登录一律 404，回滚 0.13.9-1 立即恢复。

**根因**：nginx access log 抓到的实际请求是 `POST /app/octopus/app/octopus/api/v1/user/login`——**前缀重复两次**。上游 `bdc9948` 起前端请求封装自己改成了
`fetch(new URL('.' + path, document.baseURI))`，已能正确解析 `/app/octopus` 子路径；sidecar 再把手工改写 JS 里 `` `/api/v1/ `` 为 `` `/app/octopus/api/v1/ ``，两边各加一次 → 双前缀。0.13.9 无此特性，所以一直正常。**这不是 0.13.10 的 bug，是我们 sidecar 补丁与上游新特性冲突。**

修法则是在 sidecar 加 `_API_REWRITE` 开关（默认关），把子路径解析交给上游；回退到 ≤v0.13.9 时设 `GATEWAY_API_REWRITE=1` 可恢复旧行为。

### 打包方式纠偏（重要）

之前一轮（-2 ~ -6）用了自写 tar 脚本手工组装 app.tgz，INNER_PINS 需逐项列举，**列漏即丢文件**：漏了 `ui/images/`（商店无图标）、`wizard/install`、`LICENSE`/`README.md`/`THIRD_PARTY_LICENSES.csv`，且 config 只有一份与既定包不一致。正确做法是本目录既有流程：**解上一版 fpk 作骨架 → 换二进制与 sidecar → 删 `app/config` → 改 manifest（删 checksum 行）→ `fnpack build -d .`**。`build-fpk.sh` 已重写为这套流程并带结构自检，产物外层清单与 0.13.9-1 逐项一致。

### 打包脚本自身的坑（记录备查）

- fnpack 要求 `app/ui/config` 必须存在，否则报 `Required file "app/ui/config" is missing`。校验 `fnpack` 是否真的成功时**不要把 stdout 吞掉**，且打包前先 `rm -f octopus.fpk`，否则会在失败时沿用上一次的成功产物，误判为成功。
- 本脚本开了 `pipefail`。`tar -tzf x | grep -q pat` 中 `grep -q` 命中即退，`tar` 收 SIGPIPE 返回 141，整条管道被判失败 → 自检假报「缺文件」。**必须先把清单读进变量再匹配**（`has_outer`/`has_inner`）。
- `wizard/install` 是必需项；旧 README「octopus 无 wizard 目录」的说法有误。
- fnpack 输出固定名 `octopus.fpk`，需手动 `cp` 成 `-x86.fpk` 交付名。

## 打包说明

1. 官方 v0.13.12 linux-amd64 release 二进制（约 51MB，前端已嵌入，Go 1.26.4 静态编译），未修改。
   zip 内附带的 LICENSE / README.md / THIRD_PARTY_LICENSES.csv 一并同步替换。
   校验：`sha256sum octopus-linux-amd64.zip` = `fb48a40a82ff46f7d0aa172384e2376d390a9b801f78c9dd196a832b4a899840`（与上游 Release SHA256SUMS 一致）；包内二进制 sha256 = `96474a06aa7d333ca12992f6fbe40d60544eb35946970ed79df5ee40b854d9f0`（与官方 zip 解出件逐字节一致）。
2. fnOS 原生 volume 安装（落 `/vol1/@appcenter/octopus/`）：cmd/ 生命周期脚本 + install_callback 数据目录准备，`OCTOPUS_SERVER_PORT=8081`（避开云微 8080）。
3. 数据目录 `/vol1/@appdata/octopus/data.db`（TRIM_PKGVAR 注入，卸载重装不丢），SQLite 存储；监听开关 `listen.conf` 同目录。
4. **v3-v4**：接入飞牛统一网关 socket 自注册（对齐 minibill），gateway sidecar 监听 `APPDEST/app.sock` → TCP 8081，手机端 /app/octopus 恢复路由。
5. **v5**：gateway sidecar 由裸 TCP 盲转发升级为**剥前缀 HTTP 反代**（修复手机端 404）。
6. **v6-v8**：sidecar 迭代修 SPA 白屏——SSE 流式透传、POST Content-Length 透传、JS chunk 同版本参数注入保证单模块图单实例（ThemeProvider Context 不断裂）。
7. **v1/0.13.4-1（2026-09-10）**：同步上游 v0.13.4，保留 sidecar v8。上游升级，本地号重置 1。（0.13.3-1 一轮与本轮合并提交。）
8. **v1/0.13.5-1（2026-09-19）**：同步上游 v0.13.5（commit 6f330f5）——首字延迟/缓存率面板、手动取消请求、新 Logo、全局模型过滤。保留 sidecar v8。
9. **v1/0.13.6-1（2026-09-20，commit 03401b5，分支 sync-octopus-0.13.6 未 push）**：同步上游 v0.13.6（addOutput 计数 2→1、markSucceeded 简化）。⚠️ 主 Release v2026.09.23 上挂的 octopus.fpk（sha 70cab405）实际是 0.13.5-1 错包（09-22 上传手滑），真 0.13.6-1 包（sha 845b4b78）只在 sync 分支；本机 `/var/apps/octopus/manifest` 登记 0.13.6-1。本轮起商店 releases 直接删 0.13.6-1 条目、只留 0.13.7-1 真件，避免错包继续可下载。
10. **v1/0.13.7-1（2026-09-23）**：同步上游 v0.13.7（commit 0e1c3fc）——厂商预设 bug 修复：火山方舟 base_url 去 `/api/v3` 后缀改路径拆分、DeepSeek/通义千问/Moonshot/智谱等 Anthropic 兼容端点按厂商实际路径纠正（channel-presets.tsx）。保留 sidecar v8 不变。
11. **v1/0.13.8-1（2026-09-23 晚）**：同步上游 v0.13.8（commit 9357a32）——创建渠道错误时弹窗提醒（channel/Form.tsx）、增加思考等级、优化日志布局（log/Item.tsx、api/log.ts）；依赖 axonhub/llm 升级（go.mod/go.sum），relay handler/state 微调。保留 sidecar v8 不变。
12. **v1/0.13.10-1（2026-10-08）**：同步上游 v0.13.10（commit d1a26bd）——数据库迁移修复、passwd 改密命令、分组编辑置顶置底、子路径反代支持。关闭 sidecar 的 API 前缀手工改写（`GATEWAY_API_REWRITE=0`）。详见上方「历史版本」。
13. **v1/0.13.12-1（2026-10-14）**：同步上游 v0.13.11 + v0.13.12（commit 3d1ff13）——流式响应空启动事件暂存重试、请求头 UA 透传修复。**新增监听地址开关**（默认 127.0.0.1，可切 0.0.0.0）。并修复 `build-fpk.sh` 不同步 `fnos/cmd` 导致新功能被静默丢弃的严重 bug。

## 验证方式（打包后自检，本轮全绿）

```bash
tar xzf octopus.fpk -C /tmp/chk          # fpk 本体可解
tar tzf /tmp/chk/app.tgz                 # app.tgz 含 octopus/ui/config/gateway
grep '^version' /tmp/chk/manifest        # version = 0.13.12-1
tar xzf /tmp/chk/app.tgz -C /tmp/chk octopus && /tmp/chk/octopus version
#   Version: v0.13.12 / Commit ID: 3d1ff13 / Built At: 2026-10-14 11:22:42 +0800
grep -c OCTOPUS_LISTEN_HOST /tmp/chk/cmd/main   # 监听开关逻辑确在包内（>0）
diff /tmp/chk/cmd/main /path/to/fnos/cmd/main   # 内容级一致（存在性检查挡不住旧内容）
```

- fpk 外层 19 项、app.tgz 14 项，与 0.13.10-1 逐目录项对齐，无多余文件、无 `__pycache__`。
- 体积 22,683,210 B（0.13.10-1 为 22,666,365 B），正常范围。
- 注意：app/ 下需保留一份 `config/`（privilege/resource 冗余拷贝），0.13.7 及以前包内皆有，缺了会与已验证结构不一致。

## src/fnos/ 重组装

```bash
# 结构：cmd/ config/ + app/(二进制+ui+gateway) + manifest + wizard + ICON*
# 组装：解包上一版 fpk 作骨架 → 换 app/octopus 官方新二进制 + zip 附带 LICENSE/README/THIRD_PARTY
#       → 更新 manifest（version/desc/changelog，删 checksum 行）→ fnpack build -d .
```
