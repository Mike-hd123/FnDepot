# dsh-hy-memory — 混元记忆 (HyAtlas) 接入 DSH

把本机 **混元记忆 HyAtlas**（`http://127.0.0.1:19528`，面板 `/app/hyatlas`）接入 DeepSeek Harness：
对话中可检索/写入/删除长期记忆，每轮自动召回、回合结束自动落库，并把画像与工作记忆蒸馏成
两份人可读的常驻 md（对齐 Hermes 的核心记忆格式）。

## 能力

| 能力 | 实现 |
|---|---|
| 7 个工具 | `memory_search` / `memory_add` / `memory_list` / `memory_stats` / `memory_digest` / `memory_forget` / `memory_mirror` |
| 核心记忆 md | `memories/USER.md`（用户画像）+ `memories/MEMORY.md`（工作记忆），条目以单独一行的 `§` 分隔，按字符预算整条装箱 |
| 常驻注入 | `ctx.systemPrompt.section('hy-memory:core-memory')` 每轮把两份 md 拼进系统提示（`interpolate:false`，与检索式召回互补、不替代） |
| 每轮自动召回 | 监听 `agent/inbox/claimed`，检索后 `agent.inject()` 注入 `【混元记忆·自动召回】` 块 |
| 回合自动落库 | 监听 `session/event`（`user/message` + `assistant/message`），`turn/end` 时写入 L2 原始层，交给 HyAtlas digest 管线提炼 |

## 工具

### `memory_forget` — 按 id 删除记忆

| 入参 | 类型 | 说明 |
|---|---|---|
| `memoryIds` | `string[]`（必填） | 要删除的 memory id，来自 `memory_search` / `memory_list`。描述约定 1–50 条（代码不按条数硬拦，只校验形态） |
| `reason` | `string`（可选） | 删除理由，回显在工具结果里便于事后追责 |

- 走 `POST /api/v1/delete_all?id=<id[,id…]>`，返回 `{deleted_count, error}`。
- **id 白名单**：只接受 `/^m[0-9a-f]{20,26}$/i`。数组为空、缺参、或全部 id 非法时**不发起任何 HTTP 请求**，直接 `ok:false`；
  因此不可能触发「不带 id 的 delete_all = 清库」这条路径（服务端 09-07 事故后也加了 400 护栏，这里是双保险）。
- 非法 id 进 `rejected` 回显；重复的合法 id 只在内部去重，不会被误报成非法。
- 删除不可逆：先用 `memory_list` / `memory_search` 核对 id，再调本工具。

### `memory_mirror` — 重建核心记忆 md

| 入参 | 类型 | 说明 |
|---|---|---|
| `targets` | `('user'\|'memory')[]`（可选） | 留空 = 两份都重建 |
| `limit` | `number`（可选） | 每个源层最多取多少条，默认取 `mirrorLimit` |
| `mergeDirs` | `string[]`（可选） | 本次临时覆盖 `mirrorMergeDirs`；传空数组 = 本次只蒸馏、不合并固定源 |

流程：**固定源条目（优先占位）→ HyAtlas 按源层抓取（分页 + 时间倒序定序）→ 噪声过滤 →
归一化去重（含「短条目被长条目涵盖则丢」）→ 按字符预算整条装箱**（绝不截断单条，否则 `§` 结构会碎）
→ 覆盖前留 `.bak-<UTC>` 备份 → 写盘。

- **合并固定源**（`mirrorMergeDirs`）：每个目录取与目标同名的文件（`user` → `USER.md`，`memory` → `MEMORY.md`），
  按 `\n§\n` 切条目后当作高优先前缀排在蒸馏条目之前，再走同一套去重与装箱。
  因此重建**不会再冲掉人工/外部权威源维护的条目**；重叠时保留固定源那份（先到先得）。
  顺序固定 = 固定源文件内原顺序 → HyAtlas 定序结果，整体可复现。
  目录或文件不存在、读失败（含把文件误当目录）一律**静默跳过**，不报错也不影响蒸馏部分。
- 抓取过滤的噪声：`Current runtime context`、`This snapshot supersedes`、`<system-reminder>`、
  `<available_skills>`、`scheduled cron job`、`【混元记忆`、`测试占位`、`内容可忽略`、含自指 memory id 的行、
  一次性会话态（`当前会话最新请求`）；固定源条目同样过这套过滤。
- 固定源与源层都没有可用条目时**跳过覆盖**（保留现有文件），不会把常驻记忆清成空文件。
- 顺带的收益：HyAtlas 挂了也不断档——实测端点不可达时蒸馏 0 条、固定源条目照常写盘（`merged=19/29`），
  同时仍逐层给出 `错误（user / l1_profile）：fetch failed` 行；此时若无固定源就退回「跳过覆盖、不写文件」。
- 内容没有变化时不留 `.bak`（按写盘后的形态比对）。
- 结果里每条报告分别给出 `merged`（固定源条数）与 `distilled`（HyAtlas 蒸馏条数），`kept = merged + distilled`。
- 这两份文件每轮常驻注入，改动会立刻影响后续所有回合；用户手工编辑过就先读文件再决定是否覆盖。

## 核心记忆文件

| 文件 | 优先来源 | 补充来源（源层） | 预算 | 形态 |
|---|---|---|---|---|
| `memories/USER.md` | `mirrorMergeDirs` 各目录的 `USER.md` | `mirrorSources`（默认 `l1_profile`） | `userCharLimit` = 2300 | 正文 = 条目以 `\n§\n` 分隔，无表头 |
| `memories/MEMORY.md` | `mirrorMergeDirs` 各目录的 `MEMORY.md` | `mirrorMemorySources`（默认 `l6_schema` + `l3_fact`） | `memoryCharLimit` = 2800 | 同上 |

表头（`USER PROFILE (who the user is)` / `MEMORY (your personal notes)`）和「非本轮指令」的说明行由注入侧
`renderCoreMemory()` 现拼，不落盘——所以手工编辑 md 只需要维护 `§` 分隔的条目本身。

### 分支作用域标签

条目正文可以带一个 `[branch:a,b]`，声明它只在指定 git 分支上注入/召回（借鉴 dsh-memory-evolve 的
`keyBranchFilter`，用于把 dev 上做的实验判断和 main 上的正式结论隔开）：

```
身份:中文简体交流。                      ← 无标记，所有分支可见
[branch:main] 30万目标 2027-03 达成      ← 只有 main 上可见
[branch:dev] 换机方案 A 实验结论          ← 只有 dev 上可见
[branch:main,dev] 双分支都要看            ← main / dev 都可见
[branch:*] 显式全分支                    ← 等价无标记，但能在正文里表达意图
```

规则与已知取舍：

- **无标记 = 全分支可见**，存量条目全部属于这一类，所以在开始打标之前行为与改造前完全一致。
- **exact 匹配**，不做子串/前缀通配：`[branch:main]` 不会匹配 `main-2`。分支名大小写敏感（git 本身如此），
  标签本身也必须写小写 `[branch:...]`。
- 标签在渲染前被剥掉，**不进入模型上下文**；被过滤掉的条目也不占装箱预算。
- 作用于**两个**入口：常驻核心记忆（`renderCoreMemory`）和每轮自动召回（`searchMemories` 结果）。
- **分支未知时放行**：目录不是 git 仓库、detached HEAD、空仓库、`git` 缺失或读超时，一律当作
  「分支未知」并**注入所有条目**。宁可多注入一条，也不能因为读不到 git 就把用户明确标注的记忆整批藏掉。
  实测本机 `/vol2/1000/workspace` 不是 git 仓库（受文件系统边界限制），默认 `branchDir` 下就是这种情况；
  要让过滤真的生效，需把 `branchDir` 指到一个真实 git 仓库。
- **只过滤，不写**：标签由你在编辑 md 或写 `memory_add` 时手工加，插件不会自动补标，也不会迁移存量条目。

## 配置（`cordis.patch.yml` 中该行的 `config`）

| 键 | 默认值 | 说明 |
|---|---|---|
| `endpoint` | `http://127.0.0.1:19528` | HyAtlas HTTP 端点 |
| `userId` | `default` | 与 Hermes 共享的用户记忆池 |
| `agentId` | `dsh` | 写入来源标识（与 `hermes` 区分） |
| `autoRecall` | `true` | 每轮自动召回 |
| `recallLimit` / `recallMinScore` / `recallMinChars` | `5` / `0.25` / `4` | 召回条数、相似度下限、短于此字数的输入不召回 |
| `recallTimeoutMs` | `2000` | 召回超时；超时即放弃，不拖慢主链路 |
| `autoWrite` | `true` | 回合结束写入 L2 原始层 |
| `maxWriteChars` | `3000` | 用户/助手各自截断长度 |
| `writeTimeoutMs` | `5000` | 写入超时 |
| `searchAgentIds` | `[]` | 空 = 不限来源 Agent（即能看到 hermes 的历史记忆） |
| `coreMemory` | `true` | 是否注册 `hy-memory:core-memory` 常驻 section（关掉则两份 md 只落盘不注入） |
| `memoriesDir` | `memories` | 核心记忆目录，相对路径按 DSH 进程 `cwd` 解析（本机解析为 `/vol1/@appshare/Harness/memories`） |
| `userCharLimit` | `2300` | `USER.md` 装箱预算（夹紧范围 200–20000） |
| `memoryCharLimit` | `2800` | `MEMORY.md` 装箱预算（200–40000） |
| `mirrorSources` | `['l1_profile']` | `USER.md` 的蒸馏源层（只接受 `l1_profile`…`l7_intention` 形态的层名） |
| `mirrorMemorySources` | `['l6_schema', 'l3_fact']` | `MEMORY.md` 的蒸馏源层；装箱按这里的先后顺序优先 |
| `mirrorMergeDirs` | `[]` | **固定源目录**：重建时先取这里同名 `USER.md` / `MEMORY.md` 的条目优先占位，蒸馏条目只补剩余预算。相对路径按 DSH 进程 `cwd` 解析；空数组 = 纯蒸馏（旧行为）；目录/文件缺失静默跳过 |
| `mirrorLimit` | `200` | 每个源层抓取条数上限（10–1000） |
| `mirrorOnWrite` | `false` | 每次自动落库后是否在后台按节流重建 md |
| `mirrorThrottleMs` | `900000`（15 分钟） | `mirrorOnWrite` 的节流窗口（10000–86400000） |
| `mirrorTimeoutMs` | `15000` | 镜像抓取每页请求的超时（1000–120000） |
| `redact` | `true` | **出网脱敏闸**：进入模型上下文前把身份/凭证形态替换成 `[REDACTED]`。作用于常驻注入、每轮自动召回、`memory_search` / `memory_list` 返回、`memory_mirror` 写盘内容 |
| `redactPatterns` | 内置 8 条 | 正则（字符串数组，逐条编译；无效正则跳过并记 debug 日志）。内置：邮箱、11 位大陆手机号、18 位身份证（带前后断言，防吃掉更长号码）、≥19 位连续数字、`sk-…`、`ghp_…`、JWT、`admin[:/]admin`。**故意不含内网 IP**——ssh / 运维要用 |
| `redactLiterals` | `[]` | 字面黑名单（长值优先替换，不受正则形态限制：真名、微信昵称、精确生日、农历、镇村与路口门牌等） |
| `redactMarker` | `[REDACTED]` | 替换标记；相邻重复标记会自动合并，避免 `[REDACTED][REDACTED]` |
| `branchScope` | `true` | **分支作用域**：条目正文可带 `[branch:a,b]` 声明，只在指定 git 分支上注入/召回。`false` = 整条链路短路，退回改造前行为 |
| `branchDir` | `''`（跟随 `process.cwd()`） | 读 git 分支名的目录；相对路径按 DSH 进程 `cwd` 解析。留空时探测的就是常驻记忆文件所在的仓库目录 |
| `branchCacheTtlMs` | `30000` | 分支探测结果按目录缓存的 TTL（0–600000），避免每轮注入都 fork 一次 git |
| `reviewEnabled` | `false` | **in-turn review 旁路**：turn/end 写入 L2 后异步审查，命中关键事实时当轮额外写一条独立 L2 记录，不等 digest 提炼。**默认关闭** |
| `reviewEndpoint` | `http://127.0.0.1:8081/v1` | 审查模型端点（同压缩，走 octopus 网关） |
| `reviewApiKeyEnv` | `OCTOPUS_API_KEY` | 从哪个 env 变量读 key |
| `reviewModel` | `default` | 审查模型。建议 `default`（走 glm-5.2，实测严格 JSON）；**不建议 `compression`** —— 该 9B 别名偏懒、会回吐原文，不适合做结构化判断 |
| `reviewTimeoutMs` | `12000` | 审查超时（1000–120000），超时静默放弃 |
| `reviewMinSignalChars` | `40` | 本轮用户输入短于此字符数就不跑审查（0–2000），省掉寒暄轮的无效调用 |
| `reviewMaxItems` | `3` | 单轮最多固化条数（0–10），防模型失控 |
| `reviewMaxChars` | `60` | 单条最大字数（20–400） |

> 本机 `cordis.patch.yml` 已把 `memoriesDir` 钉成绝对路径 `/vol1/@appshare/Harness/memories`，
> 并把 `mirrorMergeDirs` 钉成 `['/vol1/@appshare/Harness/memories_from_hermes']`（Hermes 维护的记忆文件当权威源）；
> 其余键目前与上表默认值一致。profile 用 `link:` 指向本源码目录，所以改这个 patch 就是改运行配置（需重启 DSH 生效）。

## in-turn review 旁路（B2）

动机：关键事实原来只走 `turn/end` → L2 → HyAtlas digest 提炼成 L3，中间隔一轮，当轮拿不到。
旁路在 L2 写入之后**异步**再问一次审查模型：这一轮里有没有值得当轮就固化的关键事实（用户偏好、
明确决策、不可逆结论）？有就立刻多写一条带 `[KEY]` 前缀的独立 L2 记录。

与 digest 的关系：**不替代、不阻断、不覆盖**。审查是纯增量——该轮内容无论如何都已经在 L2 里了
（由 `autoWrite` 写入），审查只是多给 digest 一份高信号输入。任何失败（超时/网络/非 JSON/模型偷懒）
一律静默跳过，不影响主链路。

防误写三道闸，按顺序执行，任一拦下都不写：

1. **结构化输出**：模型必须返回严格 JSON `{"items":[...]}`，非 JSON 直接丢弃（会剥离 markdown 代码块围栏）。
2. **自包含度校验**：命中指代词（这个/那个/上述/上面那个/刚才/刚刚/前面说过/那条/那台/那件事）或纯寒暄
   （好的/对/嗯/继续…）直接拒绝。理由：脱敏后的 key 条目要能脱离对话独立阅读，带指代词就是废条目。
3. **去重**：与本轮已写入的 L2 文本做归一化去重，防止把整轮转录又原样固化成 key 条目。

实测（本地假模型 5 场景 + 3 短路场景）：

| 场景 | 输入 items | 结果 |
|---|---|---|
| 正常 | 2 条合格 | 全部保留 |
| 含坏项 | 1 好 + 「记住这个方案」+「按照上述配置来」 | 只留 1 条，坏项被自包含度闸拦下 |
| 非 JSON | 字符串 | 丢弃（返回 null） |
| 代码块围栏 | ` ```json … ``` ` | 剥围栏后正常解析 |
| 空结果 | `{"items":[]}` | 返回 `[]`，不写 |
| 关闭 / 短句 / 无 key | — | **0 次 HTTP 请求**（短路，不浪费调用） |

开启前建议先确认 `reviewModel` 指向的模型能稳定返回严格 JSON —— 这是唯一会写库的环节，
模型质量差等于往 HyAtlas 里灌噪声。

## 用到的 HyAtlas 接口

| 方法 | 路径 | 用途 |
|---|---|---|
| POST | `/api/v1/search` | `{query, user_ids, agent_ids?, reader, limit}` → `{memories:{profile,proactive,normal}}` |
| POST | `/api/v1/add` | `{text, user_id, agent_id, layer?}` → `{memory_id, extraction_status}`（**实测 `layer` 被服务端忽略，恒落 `l2_raw`**） |
| GET | `/api/v1/list` | `?limit=&offset=&user_id=&layer=` → `{memories:[{memory_id, content, layer, ts, …}], total}` |
| POST | `/api/v1/delete_all` | `?id=<id[,id…]>` → `{deleted_count, error}`；**不带 id 会清库**，本插件强制带 id |
| GET | `/api/v1/status` `/api/status` `/api/layer-counts` | 健康度与各层条目数 |
| POST | `/api/v1/digest` | `{user_id}` 触发提炼管线（LLM，耗时较长） |

## 实测记录（2026-09-30 收尾验证）

离线驱动：`node /tmp/hy-mirror-bootstrap.mjs`（假 `ctx` 调 `apply`，打真实 HyAtlas）+ `/tmp/hy-unit2.mjs`（stub `fetch`，33 项断言全过）。
库内规模 `l1_profile` 597 / `l3_fact` 17429 / `l6_schema` 3245 / `l2_raw` 约 7760。

- **生成结果**：`USER.md` 写入 27 条 / 正文 1368 字符（预算 1375，丢弃 145 条）；`MEMORY.md` 写入 38 条 / 正文 2192 字符
  （预算 2200，丢弃 309 条）；备份 `USER.md.bak-20260930T064224`、`MEMORY.md.bak-20260930T064224`。
- **常驻注入**：section 渲染 3671 字符，`interpolate:false`；`order` 取 `getSectionOrder('DEPLOYMENT_PERSONA_PREFIX') + 100`，
  离线 stub 里该查询返回 0 所以观测到 `order=100`，真实 DSH 里以前缀段的序为基准。
- **质量**：两份文件里 memory id / 平台快照 / `<system-reminder>` / 召回哨兵 / cron 前导指令 **命中 0**；
  完全重复 0、归一化重复 0、涵盖式重复 0；最长单条 119 / 123 字符（远小于预算，即没有半条截断）。
- **幂等**：紧接着第二次 `memory_mirror` 得到 `kept/chars/dropped` 完全相同且 `backup=(无)`。
  （修复前：每次运行都恒留一份同内容 `.bak`，因为拿带尾换行的旧文本比不带尾换行的新文本。）
- **`memory_forget`**：`memory_add` 写入自检条目 → 返回 id `m20260930063820070abeaa`（23 位，形态合法）→
  `memory_forget{memoryIds:[id]}` 返回 `{ok:true, requested:1, deleted_count:1}`，
  透传 `{"deleted_count":1,"error":""}` → 全量扫描 `l2_raw` 7759 行、按自检标记检索 **0 命中**；
  再次删同一 id 返回 `deleted_count:0`（幂等，且仍是合法请求）。
- **防御分支**（用 `globalThis.fetch` 计数器验证「未发起任何请求」）：
  `{memoryIds:[]}`、缺参、`memoryIds` 非数组、`['123']`、`["'; DROP"]`、`['../../admin']`、`['id=1 OR 1=1']`、
  `['m'+'a'×300]` → 全部 `ok:false` 且 **HTTP 次数 = 0**；合法但库里不存在的 id → 发 1 次请求、`deleted_count:0`。
- **源层噪声统计**：`l1_profile` / `l3_fact` / `l6_schema` 扫描内平台快照与 cron 前导 **0 命中**；
  但 `l2_raw` 有 158 条 cron 前导指令 + 3 条平台快照残留（尚未被 digest 提进上层，会顺着源层流进常驻 md，故镜像侧一并兜住）。

### 本次验证顺手修掉的问题

| 位置 | 问题 | 处理 |
|---|---|---|
| `writeCoreFile` | 备份判断拿「带尾换行的旧内容」比「不带尾换行的新内容」，恒不等 → 每次镜像都留同内容 `.bak` | 按写盘后的形态比较 |
| `rowsToEntries` | 去重只看原文前 48 字符 → 标点变体（`人话版` vs `「人话版」`）重复占预算；长句 + 涵盖它的短句也同时装箱 | 归一化（去标点/符号/空白/大小写）后前缀去重 + 涵盖式收敛 |
| `rowsToEntries` | 缺少会话态与 cron 类噪声 | 新增 `TRANSIENT_NOISE` 与扩充 `MIRROR_NOISE` |
| `fetchLayer` | 服务端同秒并列返回顺序不稳定（实测同层两次抓取集合相同、顺序不同），装箱先到先得 → 常驻内容每次漂移 | 抓取后按 `ts` 倒序 + `memory_id` 升序定序 |
| `toolForget` | 重复传入的合法 id 被塞进 `rejected`，渲染成「忽略的非法 id：m…」，误导调用方以为没删掉 | 合法 id 只在内部去重，`rejected` 只装真非法项 |
| `toolMirror` 的 `render` | 层抓取失败的报告形如 `{target, layer, error}`，旧条件 `item.error && !item.target` 恒假 → 失败被渲染成 `→ undefined：写入 undefined 条` 的成功行 | 先判 `error`，带上 target/layer 输出 |
| `toolAdd` | 描述串与 `parameters` 挤在同一行（排版塌陷，语义不变） | 拆行 |

## 实测记录（2026-09-30 合并固定源 `mirrorMergeDirs`）

离线驱动（**全部写到 `/tmp/hy-m*` 临时目录，未碰线上 `/vol1/@appshare/Harness/memories`，未调 `memory_digest`，未删任何记忆**）：
`/tmp/hy-mirror-merge-test.mjs <目录> <mergeDirsJSON>`（跑 mirror + 打 report/render）、
`/tmp/hy-mirror-verify.mjs`（14 项断言全过：条目齐全 / 最长条 / 三类重复 / 预算 / 前缀顺序）、
`/tmp/hy-mirror-edge.mjs`（5 组场景 × 4 项断言全过）、`/tmp/hy-mirror-patchcfg.mjs`（直接读 `cordis.patch.yml` 的 config 端到端跑）。
固定源用 `/vol1/@appshare/Harness/memories_from_hermes`（只读，未改动）。

- **合并生效**：`USER.md` 写入 33 条 / 2300 字符（`merged=19` + `distilled=14`，丢弃 158）；
  `MEMORY.md` 写入 29 条 / 2798 字符（`merged=29` + `distilled=0`，丢弃 366）。
  Hermes 的 19 / 29 条**一条不少**地进了输出，含最长两条（181 字、264 字）**原样无截断**。
- **去重与顺序**：完全重复 0、归一化前缀重复 0、涵盖式重复 0；输出前 N 条与固定源文件内原顺序逐条相等。
- **可复现**：连跑两次（各自新进程）`md5sum` 完全一致（`USER.md 27a2eb23…`、`MEMORY.md 84265a6a…`），
  且**不产生新 `.bak`**；先纯蒸馏再开合并、内容变了 → 各留 1 份 `.bak`（备份逻辑没被合并功能破坏）。
  `dropped` 在几次运行间会漂（366→368→373），那是 HyAtlas `l3_fact` 正被别的会话写入，与装箱无关：
  `kept / merged / distilled / md5` 几次都对得上。
- **退回旧行为**：`mirrorMergeDirs: []` → `merged=0, distilled=43`、无报错；且「配置关合并」「目录不存在」「入参显式空数组」
  「把文件误当目录」四种写出的 `USER.md` md5 **完全相同**（`cc5d1bc6…`）= 逐字节回到纯蒸馏。
- **边界静默**：目录不存在 `/tmp/definitely-not-here-xyz`、把文件当目录（`…/USER.md`）、
  参数显式 `mergeDirs: []` 覆盖掉配置里的合并目录 —— 三种都 `merged=0`、蒸馏照常、无 `error` 项。
- **参数覆盖**：配置留空但本次传 `mergeDirs: ['/vol1/@appshare/Harness/memories_from_hermes']` → `merged=19`；
  判定只看「是不是数组」——传了数组（含空数组）就用它覆盖配置，没传（`undefined`）才回落 `mirrorMergeDirs`。
- **附带收益（HyAtlas 挂了也不裸奔）**：端点指向死端口重跑 —— 开了合并时 `USER.md` / `MEMORY.md`
  仍能只靠固定源写出 19 / 29 条（层抓取失败照常报 `错误（user / l1_profile）：fetch failed`）；
  没开合并时保持旧行为 `skipped`、**不覆盖**现有文件。

### 本次改动顺手修掉的问题

| 位置 | 问题 | 处理 |
|---|---|---|
| `README` 配置表 | `userCharLimit` / `memoryCharLimit` 仍写 1375 / 2200（Hermes 出厂默认），与代码 `DEFAULTS` 的 2300 / 2800 不符 | 文档对齐代码 |

## 实测记录（2026-09-30 出网脱敏闸）

单元（`/tmp/hy-redact-unit.mjs`，只用自造假值）：**18 通过 / 0 失败** —
11 类命中（邮箱、手机号、18 位身份证、带 X 身份证、20 位保单号、`sk-`、`ghp_`、JWT、`admin:admin`、字面姓名、字面日期）
+ 7 类必须原样保留（`/vol2/1000/…` 路径、`127.0.0.1:8081` / `13080` / `3079`、`custom:Octopus-8081`、
`.credentials-vault`、`192.168` 段、`2026-09-30` 这类工作日期、`54 首` / `¥135` 等业务数字）。

端到端：把**未脱敏的 Hermes 原文**当常驻文件 + 固定源，逐条路径扫 10 项字面黑名单：

| 路径 | 结果 |
|---|---|
| 常驻 section 注入 | 0 泄漏（12 处替换成 `[REDACTED]`） |
| `memory_mirror` 重建后 USER.md | 0 泄漏，2292/2300 |
| `memory_mirror` 重建后 MEMORY.md | 0 泄漏，2794/2800 |
| `memory_search` 返回 | 0 泄漏 |
| `memory_list` 返回（`l1_profile` 全原值） | 0 泄漏 |

过程中抓到并修掉一个真 bug：18 位身份证规则原先没有前后断言，会先吃掉 20 位保单号的前 18 位，
输出 `10[REDACTED]` 这种残缺串；改成 `(?<![0-9])[0-9]{17}[0-9Xx](?![0-9])` + 独立的 `(?<![0-9])[0-9]{19,}(?![0-9])`。

另有一条踩坑记录：**别拿线上 `memories/` 目录当测试输出**——一次 `memory_mirror({targets:['user']})`
把策展版 USER.md（25 条）重写成 32 条并留下 `.bak`，已按备份还原。所有镜像测试都要先把
`memoriesDir` 指到 `/tmp` 下。

## 已知限制

- ~~固定源会把 PII 原值灌回常驻文件~~ **已由 `redact` 出网脱敏闸关闭**：`memories_from_hermes/USER.md` 里存着真名 /
  精确生日 / 邮箱 / 保单号 / 后台口令原值，任何进入模型上下文的文本（常驻注入、每轮召回、`memory_search` /
  `memory_list` 返回、`memory_mirror` 写盘内容）都先过闸替换成 `[REDACTED]`，所以固定源可以保持原样不动。
  HyAtlas 库里同样存原值（本机可读、不出网），要彻底清得走 supersede + 冷存。
- **固定源吃满预算**：Hermes 的 `MEMORY.md` 自身 2798/2800 字符，合并后 `distilled=0`，
  现网人工条目有 8 条（USER 有 11 条）在新输出里不出现——它们不是被去重干掉，是预算放不下。
  想让蒸馏条目也进来，要么删/并薄固定源条目，要么上调 `memoryCharLimit`。
- 装箱按「源层顺序 + 时间新到旧」先到先得，`MEMORY.md` 的 `l6_schema` 会挤掉 `l3_fact`：想调侧重改 `mirrorMemorySources` 顺序。
- 语义级重复（同一件事换了措辞，如 Telegram 登录那几条）文本层去不掉，得在库里用 `memory_forget` 清源头。
- `mirrorOnWrite` 默认关闭：常驻文件被每轮注入，写太频繁会让提示不稳定。

## 加载与卸载

```bash
# 安装（在会话里由 plugin_manager 执行）
install_bundle target=/vol1/@appshare/Harness/hy-memory
```

卸载：`plugin_manager action=remove_bundle target=dsh-hy-memory`。

> **改完 `index.js` 需要重启 DSH 才会加载**（profile 安装的插件不在热更新路径上）。
> 调试可用 `DSH_HY_MEMORY_DEBUG=1` 打开 `logDebug` 输出。
