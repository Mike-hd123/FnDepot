# T10：集成测试、灰度与发布门槛

## 目标

整合 T01–T09，建立覆盖迁移、路由、转换、安全、流式和 UI 的发布门禁；提供按功能开关回退的灰度方案。本任务不重新设计已冻结模块。

## 依赖

- T01–T09 全部完成并提供交接输出。

## 文件所有权

- 集成测试、fixture、mock upstream、功能开关和发布说明。
- 可修复跨模块集成缺陷；不得未沟通重写已有模块接口。
- 不提交、推送或发布，除非另有明确授权。

## 测试基础设施

建立本地 mock upstream，支持：OpenAI Chat/Responses/Embeddings、Anthropic Messages/Count Tokens、Ollama api_chat、legacy Gemini。可配置 status、delay、headers、SSE 分片、malformed frame、断连和调用计数。

所有 provider URL/auth 测试使用 mock，不访问真实付费端点。

## 必测矩阵

### 路由

- 模型直接命中、mapping source 命中、旧空模型 wildcard、数组 mapping。
- 原生 G1 低 priority 与转换 G2 高 priority：G1 必须先调用。
- 同组多 priority tier 与同 tier weight。
- G1 无模型、disabled、连接失败、超时、429、5xx 后进入 G2。
- 400/422 不重试；401/403 只同组；404-model 不当 endpoint unsupported；405/501 可降级。
- 每组预算与总预算。

### 协议与流

- Chat、Responses、Messages 的流/非流原生路径。
- Chat↔Messages 支持矩阵和所有拒绝字段。
- 原生 Responses 未经过 Chat codec；旧标记渠道仍走 Responses→Chat。
- UTF-8 和 SSE delimiter 任意分片、并行 tool calls、未知事件、非法 JSON、EOF/终止一次。
- 首帧失败可换候选；commit 一字节后第二上游调用为零。
- 客户端取消触发上游取消与 client_cancelled 日志。

### 安全与权限

- Chat/Responses/Messages/Count/Embeddings 全部检查 status、expires、quota、allowed model/channel。
- secret 只存在于 built-in tool、未知字段、图片 URL、header/query 时可命中。
- redact 后原生/转换上游只见脱敏值；日志无原 secret。
- Confirm、预算超限、codec reject 上游零调用。
- Base64 大附件按元数据预算处理，UTF-8 不 panic。

### 迁移与数据

- 全新 DB、旧 DB 升级。
- 升级→旧版风格 INSERT/UPDATE→再次升级。
- 每个 legacy type identity、URL 和 auth。
- v1/v2 import/export round-trip。
- 新配置在上一版路由中的兼容调用。
- timeout/status/config 未知键/mapping 数组保真。

### UI

- 三协议 custom 默认。
- 各协议 provider 集合和分组。
- OpenAI 端点 0/1/2 选择、逐端点测试、失败强制保存。
- dirty preset switch 不丢字段。
- 渠道列表 `[协议] [提供商]` 双标签。
- 旧渠道编辑不改 Key、timeout、mapping、status。

## 功能开关验证

- `cross_protocol_codec=false`：只走原生组。
- `native_responses=false`：旧标记渠道保持 legacy Chat；新原生 Responses 不启用。
- `ollama_native=false`：原生 Ollama 不进入 RoutePlan，UI 明确不可用或隐藏。
- `new_routeplan=false`：回到旧路由，仅作为短期回滚；安全闸门不得关闭。

分别验证开关组合，确保关闭功能不会让数据不可读。

## 性能与资源

- 大请求扫描预算和 body limit。
- SSE 首 token 延迟只增加一个有界 frame。
- 长流 idle timeout 不受普通总 timeout 错杀。
- 客户端取消释放连接和任务。
- 100 个渠道的模型筛选/分组不产生显著阻塞。

## 发布门槛

1. `cargo fmt --check` 通过。
2. `cargo clippy --all-targets --all-features -- -D warnings` 通过；如项目当前已有 warning，记录基线且新代码零新增。
3. `cargo test --all-targets --all-features` 通过。
4. `pnpm build` 通过。
5. 所有新增集成测试通过且无真实外部请求。
6. migration 备份/恢复演练通过。
7. 安全审计负向测试上游调用为零。
8. 功能开关回退演练通过。
9. 设计文档、任务交接、导入导出格式和发布说明一致。

## 灰度顺序

1. 发布 migration、presets、DTO 和安全闸门，保持新 RoutePlan 关闭。
2. 内部开启 new_routeplan，仅原生组。
3. 开启 Chat↔Messages codec 小流量，观察 failure class 和 codec reject。
4. 开启 native Responses。
5. 开启 Ollama native。
6. UI 对全部用户开放新建入口。

任一步异常先关闭对应 feature flag，不删除数据。回滚旧二进制前备份 SQLite 和导出 v2；发布说明明确新组合在旧版只保证 legacy alias 的基本能力。

## 实施步骤

1. 合并各任务分支后先解决接口和 migration 编号冲突，不改变已冻结语义；记录每个冲突的最终选择。
2. 建立统一 mock-upstream fixture，覆盖所有原生端点、legacy executor、错误状态、延迟、断流和 SSE 分片。
3. 按“路由、协议与流、安全与权限、迁移与数据、UI”五组执行必测矩阵，并将每一条映射到具体测试名。
4. 执行全新安装、旧库升级、旧版回滚写入、再次升级和 v1/v2 导入导出五条数据演练。
5. 逐个验证四个功能开关及组合关闭行为，确认安全闸门不受业务开关影响。
6. 记录构建、lint、单元、集成、组件测试结果；失败必须修复或作为发布阻断，不得仅在报告中豁免。
7. 进行 migration 备份恢复、客户端取消、长流 idle timeout、100 渠道筛选和日志脱敏演练。
8. 形成发布说明、监控指标、开关回退步骤和正式发布检查单；本任务不执行真实发布。

## 测试命令

- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings`
- `cargo test --manifest-path src-tauri/Cargo.toml --all-targets --all-features`
- `pnpm build`
- 运行本任务新增的 mock-upstream 集成测试入口，并在交接报告中记录精确命令、用例数、耗时和失败重跑结果。

## 验收标准

- 所有门槛有可复现命令和结果。
- 没有未经覆盖的协议/流/迁移主路径。
- 开关回退不要求破坏性 DB 操作。
- 发布说明列出严格 codec 可能将旧静默容错请求改为 4xx。

## 交接输出

提供完整测试报告、失败矩阵、性能数据、功能开关演练、迁移备份恢复记录、灰度监控指标与正式发布检查单。
