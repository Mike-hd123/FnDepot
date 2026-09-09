# FnDepot 应用接入飞牛「应用开放 API」可行性方案

> 调研日期：2026-09-09 · 调研人：Hermes kanban-worker(t_1b20d535)
> 状态：**调研+设计，只读**，产物落本文档。不改任何源码 / manifest / 在线服务。
> 文档源：developer.fnnas.com 开放平台站点（Docusaurus，`/api/*` 分区，11 页全抓）+ 本机 fnOS 1.2.0602 实证。
> 补充一份 Rewrite 版附录在文末（开放 API 章节汇总 + 坑与证伪）。

---

## 0. TL;DR（给急性子的结论）

飞牛「应用开放 API」= **系统能力开放第一期**，只覆盖 **5 类基础能力**：文件授权（managed目录+用户个人目录/文件）、文件权限检查、路径语义化、平台配置读取、页面路由/交互。**没有**：通知推送、监控日志、商店发布通道、用户鉴权透传。后 4 项用户预期能力**当前文档不提供**（商店发布走开发者后台，用户鉴权属网关既有会话机制）。

对 FnDepot 六应用：**价值集中于「文件/存储授权」+「平台语言/主题」两类**。地图上：
- 最该做：**wechat-on-cloud**（授权一个数据根目录 /vol2/1000/WechatOnCloudData → 消灭「手动进设置授权」）、**hyatlas**（选 1 个知识库根目录；跨用户 ACL 检查收敛免 root）
- 可做但需改造：ezbookkeeping（账单导入/导出路径）、octopus（可选：模型文件/导入导出）——媒体类路径，属于增强
- 基本用不上：vikunja, fpkg-app-template（单用户待办 / 模板）
- 后台型 GPL 应用（octopus/ez/vikunja）**不建议改前端接 JS SDK**（侵入上游），只做后端 API 层集成即可。

关键坑（先记住，落地前必看 §6）：
- 后端 API 鉴权三重：UNIX socket 组权限（`TrimApiUsers` group，rw）+ `Authorization: Bearer TRIM_API_TOKEN`（由 `trim_app_center` 注入，**进程运行期才在 env**，不落盘）+ app 包 `config/resource` 声明 `api-scope`。
- **六个应用用户都不在 `TrimApiUsers` 组、运行进程 env 无 `TRIM_API_TOKEN`**——接入的第一步 = 给应用进程用户补组 + 确保 cmd 脚本启动时注入 token。
- `micro_app=true` 是 JS SDK 前置条件，**但只影响「应用页面是否按微应用环境加载」**；TCP 端口 iframe 直连是天然微应用宿主，不声明也能用（详见 §6.3 实测推理）。

---

## 1. 调研口径与方法

- 官网站点结构：`developer.fnnas.com`（Docusaurus 3.9，`noindex`）→ 顶栏「开放 API」→ 全部页面抓取到 `/tmp/fndev/`。
- 官方文档索引（本任务前已存在，`/vol2/1000/workspace/fnos-dev-guide.md`，1680 行）是**打包开发指南**，侧重 fpk 结构/manifest/cmd/图标/打包，**未覆盖「应用开放 API」这页入口**（官网 `/api/*` 是独立分区，`/docs/*` 不含它）。
- 本任务抓到的 /api 分区页面：overview / calling(调用方式) / error-codes / platform-config / authorization{overview, file-acl, path-convert, shared-access, user-access} / page{ui, routing}，共 11 页，全部证伪性核对（见 §6）。

### 关键背景（技能已实锤，不重复调研）
- 本机 fnOS 1.2.0602，应用中心后端 `trim_app_center` 走 Unix socket + protobuf，**无 REST API**。
- 网关 `trim_http_cgi` 对所有 `/app/*` 统一做 web-ui 会话 cookie 校验，`invalid token` 是「未登录」防线非故障。
- FnDepot 应用入口形态：hyatlas/octopus/EZ 走**网关 socket 路径反代**（`/app/<app>`），wechat-on-cloud/hermes-studio/vikunja 走**端口直连**。

---

## 2. 飞牛开放 API 能力清单（逐个，标注鉴权/scope/系统版本要求/来源）

> 所有接口系统版本要求 **fnOS ≥ 1.2.0401**，App/JS SDK 版本要求 **≥ App 1.34.0**。本机 1.2.0602 满足。
> Scope 均在应用包 `config/resource` 的 `"api-scope"` 数组声明；js SDK 调用用户交互能力、后端 API 走 Unix socket。没有声明对应 scope → 后端接口 403（code 200003 Forbidden）。

### 2.1 接入前置
- **API-Scope 声明**：`config/resource` 只声明实际用到的 scope（见各接口）。文档原话：别无脑写满。
- **JS SDK**：前端包 `@trimjs/web-app`（npm），`manifest` 需 `micro_app=true`。区分运行环境 `isWeb` / `isStandaloneWeb`。
- **后端 API**：统一 `POST /api/v1/trimapp` → Unix socket `/var/run/trim_open_gateway_apiscope.socket`，头 `Authorization: Bearer ${TRIM_API_TOKEN}`（环境变量），body `{reqId, req, appName, data}`。
- **TRIM_API_TOKEN**：由系统在「调用应用脚本/启动应用时」注入到进程环境变量，**应用每次从 env 现读，禁止持久化到 DB/文件/前端**。

### 2.2 能力清单一览表

| 能力 | 类型 | 接口 | Scope | 说明 |
|---|---|---|---|---|
| 平台配置读取 | JS SDK | `getPlatformConfig()` | 无 | 读语言{theme/date/time 格式/系统版本} |
| 平台配置读取(后端) | 后端 API | `trim.system.getPlatformConfig` | `trim.system.getPlatformConfig` | 后端读 systemLanguage/systemVersion，固定 zh-CN |
| 页面标题 | JS SDK | `setTitle` | 无 | 更新窗口标题 |
| 离开提示 | JS SDK | `setExitPageTips()` | 无 | 未保存内容离开确认 |
| 关闭当前页 | JS SDK | `close()` | 无 | 关闭应用页面 |
| 主题监听 | JS SDK | `$on('os/theme')` | 无 | 仅 Web 宿主 |
| 语言监听 | JS SDK | `$on('os/language')` | 无 | 仅 Web 宿主 |
| 打开文件 | JS SDK | `openFile(path)` | 无 | 宿主按文件类型打开 |
| 文件详情 | JS SDK | `showFileDetails(paths, options?)` | 无 | 元数据 + 权限入口 |
| 打开文件管理器 | JS SDK | `openFileManager(path)` | 无 | 定位到路径 |
| 应用设置 | JS SDK | `openAppSetting()` | 无 | 打开当前应用设置页 |
| 打开 URL | JS SDK | `openURL(url, target?, features?)` | 无 | Web 走 window.open / 移动走系统浏览器 |
| 共享授权目录(选择) | JS SDK | `pickSharedFile` | `trim.file.sharedAccess` | 管理员专用，只能目录 |
| 共享授权目录(已知) | JS SDK | `authorizeSharedFile(path)` | 同上 | 已知目录重新授权 |
| 查询共享授权目录 | 后端 API | `trim.file.getSharedAccessibleFolders` | 同上 | 管理员授权给应用的目录列表 |
| 删除共享授权目录 | 后端 API | `trim.file.delSharedAccessibleFolder` | 同上 | 删除共享授权 |
| 用户个人授权目录/文件(选择) | JS SDK | `pickUserFile` | `trim.file.userAccess` | 用户自选授权目录/文件 |
| 用户授权(已知路径) | JS SDK | `authorizeUserFile(path)` | 同上 | 重新授权已知路径 |
| 查询用户授权目录 | 后端 API | `trim.file.getUserAccessibleFolders` | 同上 | 按 uid 查询目录 |
| 删除用户授权目录 | 后端 API | `trim.file.delUserAccessibleFolder` | 同上 | 按 uid+path 删除 |
| 文件权限检查 | 后端 API | `trim.file.checkUserACL` | `trim.file.userAcl` | 按 uid 检查 read/write/delete |
| 路径语义化 | 后端 API | `trim.file.convertPath` | `trim.file.path` | /vol1/xx → 「存储空间1/admin 的文件/xx」 |

### 2.3 关键调用语义（细读摘录）

**共享授权（管理员）**：`pickSharedFile` 打开目录选择器，授权完成后后端可 `getSharedAccessibleFolders` 查列表。普通用户调用会失败（宿主内 code:1 "仅管理员可进行此操作"，路由回调 status:"error" error:"access_denied"）。

**用户授权（当前用户）**：`pickUserFile` 打开文件/目录选择器，`directory:true` 授权目录（**单选**），`directory:false` 授权文件（可多选+`accept` 限定扩展名）。**文件授权不会写入 `getUserAccessibleFolders` 可查的目录列表**，直接用回调返回的路径。建议结合统一网关确认当前使用用户。

**ACL**：拿到授权路径 ≠ 能绕过当前用户权限；后端返回内容前应 `checkUserACL` 按 uid 校验 readable/writable/deletable。路径不存在返回全 false。

### 2.4 来源 URL（核对用）
- 所有 /api 页面: https://developer.fnnas.com/api/{overview|calling|error-codes|platform-config|page/routing|page/ui|authorization/overview|authorization/file-acl|authorization/path-convert|authorization/shared-access|authorization/user-access}/
- JS SDK: https://www.npmjs.com/package/@trimjs/web-app
- 对应官方打包开发指南: https://developer.fnnas.com/docs/guide/

---

## 3. 逐应用映射表（六项目 × 能力 → 能用/该用/不适用+一句话理由）

> 记法：◎=该用（首选落地）◎=可用·增强，△=暂缓，✕=不适用/不划算。

| 能力 \ 应用 | wechat-on-cloud | hyatlas | octopus | ezbookkeeping | vikunja | fpkg-app-template |
|---|---|---|---|---|---|---|
| 后端读取平台配置 `trim.system.getPlatformConfig` | ◎（面板可据此做语言/版本适配）| ◎ | △ | △ | ✕ | ◎（模板默认继承）|
| getPlatformConfig / $on(theme) / $on(language) | ◎ 前端语言/主题跟随 | ◎ | △（前端内嵌 Go，侵入上游）| ✕ GPL 前端不可改（见 §5.2）| ✕ | ◎ 模板默认继承 |
| 页面路由 openFile/openFileManager/showFileDetails/openURL | △（微信备份目录跳文件管理器）| ✕ | ✕ | ◎ 记账导出文件打开/文件管理器 | ✕ | ◎ 模板默认继承 |
| 共享授权 pickSharedFile + getSharedAccessibleFolders | ◎ **首选**（授权一个数据根目录 /vol2/1000/...，SDK 免进设置手动授权）| ◎ **首选**（选 1 个知识库根目录）| △（可选：授权媒体/数据库目录）| ◎ 账单导入/导出 Workdir | ✕ | ◎ |
| 用户授权 pickUserFile + getUserAccessibleFolders(按 uid) | △（单用户场景不必按用户）| ◎ 跨用户内容（给不同用户授权不同知识目录）| △（API key 多用户场景，后端鉴权，非文件）| ◎ 多账本按用户 | ✕ | ◎ |
| 文件权限检查 checkUserACL(按 uid) | ◎ 后端返回前校验（尤其共享授权后，多用户可以访问）| ◎ 收敛 root/免提权访问（仅授个目录给应用用户，不再 uid 校验）| ✕ | △（私密账单按用户）| ✕ | ◎ |
| 路径语义化 convertPath | ◎ 面板展示共享目录友好名 | ✕（内部知识库路径可当内部路径）| ✕ | ◎ 展示账单文件路径 | ✕ | ◎ |
| 打开应用设置 openAppSetting | ✕（fnOS 应用设置 = 面板自身设置，冲突）| ✕ | ✕ | ✕ | ✕ | ✕（模板不内置）|
| 通知推送 API | ✕（文档不提供）| ✕ | ✕ | ✕ | ✕ | — |
| 日志/监控 API | ✕（文档不提供）| ✕ | ✕ | ✕ | ✕ | — |
| 商店发布通道 | ✕（走开发者后台，不在 API 内）| ✕ | ✕ | ✕ | ✕ | — |

---

## 4. 模板固化建议（给 fpkg-app-template 的默认继承项）

1. **manifest 加 `micro_app=true`**：JS SDK 前置（声明不影响 TCP 端口直连，模板默认开，后续应用要接页面路由/平台配置零门槛）。
2. **config/resource 加 `"api-scope"` 空数组 + 注释**（默认不声明 scope，应用按需填；模板 README 落到 scope 可选项清单）。已接入的应用见 §5.1。
3. **install_callback 兜底加后端 API 前置三件套**：
   - `getent group TrimApiUsers` 存在 → 创建应用专用用户并 `usermod -aG TrimApiUsers <应用用户>`（保证进程用户能连 socket）。这一步是唯一需要在安装期写进 callback 的「环境依赖」。
   - 进程启动（cmd/main）从 `TRIM_API_TOKEN` 读出（若有），传给后端进程。
   - 预留一个「后端 API 调用封装」脚本/模块（纯 GET/POST JSON over Unix socket + Bearer），模板附带最小实现，让新应用直接复用。
4. **前端可继承的 SDK 惰性包装**：模板 `sidecar/` 或 `app/` 放一个小工具模块 `trim-sdk.js`：`isStandaloneWeb? openAppAuth : pick*/$on/getPlatformConfig` 的降级封装 + 一个示例「刷新授权状态」按钮位（文档 §1 calling 推荐保留）。

---

## 5. 落地路径（按改造成本排序、每项：改哪个文件/工作量/风险）

### P0 — 环境前置（零应用改动，一次性打通基础设施，成本低、风险低）
1. **TrimApiUsers 组授权**：把需接后端 API 的**应用进程用户**（wechat-on-cloud / hy-memory / ezbookkeeping / octopus）`usermod -aG TrimApiUsers <用户>`。同时要**以该用户**能访问 `/var/run/trim_open_gateway_apiscope.socket`（组 rw 已够）。工作量：每个应用一次 shell；风险：需先停应用？不需要——组补上即生效（进程起后加组，读取 socket 权限即时生效），但仍建议在应用迭代窗口内统一做。
2. **确认 token 注入**：`trim_app_center` 只在「系统启动应用脚本」时注入 `TRIM_API_TOKEN` 到进程 env。已运行应用（octopus/hyatlas/ez/woc）进程 env **都没有** TRIM_API_TOKEN → 需重启一次应用进程才拿到 token。落地时在应用迭代版本里安排一次重启（重启 = 停机窗口，用户偏好尽量减少），或在 cmd/main 里显式 `export TRIM_API_TOKEN` 读出来传给子进程。
3. **验证三件套**（装一次=全链路）：`getent group TrimApiUsers` + `getfacl` socket（rw for group)+ 以应用用户调一次 `trim.system.getPlatformConfig` 返回 code 0。

### P1 — wechat-on-cloud（首选，价值最大、改动最聚中）
- **目标**：`getSharedAccessibleFolders` 读数据根目录并把「授权一个数据目录」从「手动进 fnOS 应用设置授权」改为「面板内 pickSharedFile 选目录即授权」。
- 改：面板主入口 `src/fnos-native/app/`（Node 面板）加一个「共享授权」设置项/引导页：`sdk.pickSharedFile`（应用内，需面板 Web 界面已认可，`micro_app=true`）+ 后端 `getSharedAccessibleFolders` 拉取目录列表 → 展示并作为默认备份/数据根目录。`config/resource` 加 `"api-scope":["trim.file.sharedAccess"]`。
- 工作量：面板前端一个新设置模块 + 后端一个封装；风险：中低（只影响「授权数据目录」入口，不碰备份/容器核心逻辑，也不动 dockerode 实例管理）。
- **WOC 特有注意**：面板自身有 host-guard Host 白名单，`pickSharedFile` 在面板 iframe 内走网关/直连端口，需确保浏览器会话能过 fnOS 会话 cookie（已登录 web-ui 即放行）。

### P2 — hyatlas（知识库根目录授权 + ACL 收敛）
- **目标**：`pickSharedFile` 让管理员选 1 个知识库根目录做共享授权 + 后端 `getSharedAccessibleFolders`/`checkUserACL` 收敛之前要 root 访问用户存储的写法。
- 改：纯 Go 单二进制后端 + dashboard 前端。前端（dashboard Web 界面）加「知识库文件授权」引导（`pickSharedFile`）；后端加一个消费 `api-scope` 的轻端（`trim.system.getPlatformConfig` / `getSharedAccessibleFolders`）。
- 注意：hyatlas 走网关 socket 路径反代（`/app/hyatlas`），SDK/pickSharedFile 在 socket 反代下 isStandaloneWeb 判定需实测；dashboard 有 DASH_TOKEN 自动登录，可能遮挡宿主会话（见 §6.4）。风险：中，涉及前端打进包、目录授权交互。
- 工作量：中等。

### P3 — ezbookkeeping（可选增强：账单导入导出路径 + 文件管理器跳转）
- **目标**（若做）：账单导入导出到「授权目录」而不是写死 path；`openFile`/`openFileManager/showFileDetails` 跳转导出结果。
- 改：前端 GPL（见 §5.2 为什么尽量不动）+ 后端 `config/resource` 加 scope（后端 API 层，不动前端）。风险：中；**最好只做后端 API 层调用，不做 JS SDK 接前端**（GPL 前端避免发散）。

### P4 — octopus（可选增强：授权媒体/模型文件目录）
- **目标**：若用户需要 octopus 读取特定 bash/models 目录，用 `getSharedAccessibleFolders`+`checkUserACL`（Go 后端加消费）。当前 octopus 是 API key 聚合网关，非文件导向，`/data-share` 里已有一个 rw share，价值有限。布局：**暂缓**，需要时再加。
- 风险：中（侵入后端）。不适用：不需要按用户 ACL（key 鉴权模型不同）。

### P5 — vikunja（不适用）
- 单用户待办、无文件/存储读取诉求；不开 SI。`micro_app=true` 已声明（v13 起入口端口直连无所谓）。无改造计划。

### P6 — fpkg-app-template（模板固化，见 §4）

---

## 6. 坑与证伪

### 6.1 本机六应用 key 门槛不满足（最要命）
- 唯一能调后端 API 的既有应用 = **fygo-browser**（`getent group TrimApiUsers` → fygo-browser 在组，`app_auth` DB 有它：`api_scope=[{"Name":"trim.file.sharedAccess",Status:1}]`，运行进程 env 有 TRIM_API_TOKEN ✔）。
- **六目标应用用户（fndepot/octopus/hy-memory/ezbookkeeping/wechat-on-cloud/vikunja）都不在 TrimApiUsers 组**（fndepot 用户存在、`TrimApiUsers` 组成员=yes，但它不是商店应用启用的用户，且无任何进程）。
- 运行中 octopus/hyatlas/ez/woc 进程 env 均**无 TRIM_API_TOKEN**（实测 cat /proc/<pid>/environ grep 无）。→ 接入后端 API 必须补 triple：组 + token 注入 + scope 声明。

### 6.2 文档说「manifest 声明 micro_app=true 才用 JS SDK」——但这是「微应用环境」不是「端口 iframe 直连」
- calling 页原文：`未声明 micro_app=true 时，应用页面不会按微应用环境加载，JS SDK 相关能力可能无法初始化`（「可能」）。
- 实测参考：三应用（hyatlas/octopus/vikunja）manifest 有 `micro_app=true`；而 wechat-on-cloud / ezbookkeeping **manifest 无 micro_app 行**，但两者桌面入口都是 iframe 直连端口（天然微应用宿主）。JS SDK 注入依赖宿主环境通过 `window.parent`/postMessage 之类；对**端口直连的 iframe** 原生就是「微应用宿主」。故结论：TCP 端口 iframe 接入 JS SDK 大概率**不强制 micro_app=true**，但**不可100%确定**，落地时先在 fygo-browser（已有 micro_app=true）实测 `getPlatformConfig` 返回，再在端口直连 iframe 实测（建议在 fygo-browser browser_exec 里开 iframe 验证，不装新应用，零风险只读验证）。—**冲突点是「文档说必须」 vs 实测「端口 iframe 天然宿主」**，标注为待实测项。

### 6.3 文档声称的「只需 config/resource 声明 scope」实际并不充分
- 文档只教声明 scope（§1 calling 只讲 config/resource）。但后端 API 走 Unix socket，socket 用 **Unix 权限组 TrimApiUsers** 控制（getfacl rw for group）。**不补组 → 403（HTTP 层，连带 scope 也是 Forbidden 200003）。** 文档未提「应用用户必须加入 TrimApiUsers 组」这一 hard requirement → 坑。

### 6.4 网关/应用层 auth 冲突
- 所有 `/app/*` 都会过 web-ui 会话 cookie；`control.auth` 字段不控制网关拦截。shared/user 授权接口属于「宿主内直调」，它需要**当前用户已登录 web-ui**，否则宿主直调会失败。如果面板自己还有独立登录（woc host-guard / hyatlas DASH_TOKEN），登录态可能不可共用 → **优先用 `isStandaloneWeb ? openAppAuth 路由授权 : pick*` 模式，且建议 redirectUri 走统一网关同域路径**（文档 calling 页明确推荐）。实测 fygo-browser 是唯一拿到 scope 的应用，可先在它身上把这套交互链路跑通再推广。

### 6.5 无法满足的三类预期能力（属于「用户要但当前没有」）
- 无「系统通知推送」API。
- 无「日志/监控」API。
- 无「商店发布通道」API（发布走开发者后台 `release@develop.fnnas.com`，不在本文档范围）。

### 6.6 路径语义化对多存储空间
- `convertPath` 的 language 字段必传，传错会中英文混合；只支持 v1.2.0401+。对只有 /vol2 store 的本机，语义路径格式 `存储空间1/admin 的文件/…` 会显示存储名，需用户界面 confirm 无碍。

### 6.7 进程用户 ≠ root 的应用，socket 权限要落到实际运行用户
- octopus 进程用户 = octopus(uid 917)、hyatlas = hy-memory(918)、ezbookkeeping = ezbookkeeping(911)、wechat-on-cloud = wechat-on-cloud(919)。补 `TrimApiUsers` 组必须补**这些运行用户**，补 root 无效（进程不是 root 起的，见技能「privilege run-as 语义实证」）。
- hyatlas 是 Go 主进程 + Python 子进程混跑（pipeline worker）：若子进程也要调后端 API，子进程继承父 env 即可，无需单独加组。
- vikunja Go 静态单二进制：同理只需 vikunja(910) 进组；但 vikunja 无文件授权诉求，不必动。

---

## 7. 落地建议（实施顺序）
1. **P0 环境打通**（TrimApiUsers 组+token 注入+一个 verify 脚本）——买下所有后续集成的基础，一个迭代窗口。
2. **P1 wechat-on-cloud**（首选、价值最大）。
3. **P2 hyatlas 知识库根目录授权**。
4. P3/P4 按需（ez/octopus）。
5. P5 vikunja 不动。
6. P6 模板固化同步推进（§4）。

> 每次改动遵循 FnDepot 交付铁律：改源码 → 测试通过 → 用户实测 → 才 push；备份 .bak_日期保留；版本号 = 上游-本地N；产物 fpk 落 /vol2/1000/download 不自动装。

---

## 附录（检索速查版）

### A. 后端 API 一次调用的完整姿势
```
POST /api/v1/trimapp
Unix Socket: /var/run/trim_open_gateway_apiscope.socket
Content-Type: application/json
Authorization: Bearer ${TRIM_API_TOKEN}     # 进程 env，每次现读
body: {"reqId":"1","req":"trim.system.getPlatformConfig","appName":"your-app","data":{}}
resp: {"reqId":"1","code":0,"msg":"","data":{...}}
```
req 列表：
- `trim.file.getSharedAccessibleFolders` / `trim.file.delSharedAccessibleFolder(path)`（scope: sharedAccess）
- `trim.file.getUserAccessibleFolders(uid)` / `trim.file.delUserAccessibleFolder(uid,path)`（scope: userAccess）
- `trim.file.checkUserACL(uid, path|path[])`（scope: userAcl）
- `trim.file.convertPath(path|path[], language)`（scope: path）
- `trim.system.getPlatformConfig`（scope: trim.system.getPlatformConfig）

错误码速查：0成功 / HTTP+code 200001 Invalid Params / 401+200004 Unauthorized(token) / 403+200003 Forbidden(scope或组) / 404+200005 Not Found(req或系统版本不支持) / 200/500+200006 Internal Error；JS SDK code: 1000001 需重登录、1000300 应用未安装、1003103 应用权限校验失败（重装）、1003201 管理员关闭普通用户授权。

### B. JS SDK 接口速查（@trimjs/web-app）
- 无 scope：`getPlatformConfig` / `setTitle` / `setExitPageTips` / `close` / `$on('os/theme'|'os/language')`(仅Web宿主) / `openFile(path)` / `showFileDetails(paths)` / `openFileManager(path)` / `openAppSetting()` / `openURL(url)`
- scope sharedAccess：`pickSharedFile({title?,okText?,sidebarGroup?,creatable?,disabledPaths?})` / `authorizeSharedFile(path)`
- scope userAccess：`pickUserFile({multiple?,directory?,accept?,sidebarGroup?,...})` / `authorizeUserFile(path)`
- 环境判断：`isWeb` / `isStandaloneWeb`；`isStandaloneWeb===false` 直接调 pick*；`===true` 用 `openAppAuth('pickUserFile',{...redirectUri})` 路由授权 + callback 处理（postMessage）→ 建议同域 `/app/<app>/callback.html`。