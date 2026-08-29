# 渠道协议重构：任务总索引

分支：`codex/channel-protocol-refactor-plan`

主设计：[渠道配置重构设计](../channel-protocol-provider-refactor-design.md)

架构约束：[00-architecture-decisions.md](00-architecture-decisions.md)

复核基准：[T12 CLIProxyAPI 映射基准](12-cliproxy-anthropic-to-chat.md)

待评审方案：[T13 thinking fail-open 转换](13-thinking-fail-open-conversion.md)

已验收原型：[T14 同步上游模型（弹窗勾选应用）](14-model-upstream-sync.md) · [原型 HTML](14-model-sync-prototype.html)

## 交付目标

在不破坏现有 Chat、Responses、Anthropic Messages、模型映射、权重调度、导入导出和旧数据库的前提下，将渠道的协议、提供商和原生端点能力拆开，并交付：

- OpenAI、Anthropic、Ollama 三个协议 Tab；每个协议默认选择“自定义配置”。
- 后端唯一提供商模板 registry，前端不复制 URL、模型和能力常量。
- 模型第一、协议原生组第二、组内优先级/权重第三的 RoutePlan。
- Chat Completions ↔ Anthropic Messages 的严格、版本化、流/非流 codec。
- 原始协议请求统一安全审计、脱敏转发体和脱敏日志体。
- OpenAI 原生 Responses 与旧 Responses→Chat 兼容路径并存。
- 新旧数据、旧前端 payload、v1/v2 导入导出和旧二进制回滚兼容。
- 渠道列表名称后的 `[协议] [提供商]` 双标签。

## 不可破坏的路由不变量

```text
认证和原始请求审计
→ 请求模型授权与模型候选匹配
→ 原生协议组
→ 组内 priority tier
→ 同 priority 内按 weight 无放回抽样
→ 原生组没有候选或发生允许降级的故障
→ 转换协议组
```

转换组的高优先级渠道不得越过原生组的低优先级渠道。数组模型映射每个 attempt 只抽样一次，实际请求、日志和统计必须使用同一个上游模型。

## 阶段和任务

| 阶段 | Task | 名称 | 依赖 | 是否可并行 |
| --- | --- | --- | --- | --- |
| 0 | T00 | [架构决策冻结](00-architecture-decisions.md) | 无 | 所有 Agent 必读 |
| 1 | T01 | [提供商模板与领域类型](01-presets-and-domain-model.md) | T00 | 可与 T03 并行，避免修改同一文件 |
| 1 | T02 | [数据库迁移与身份兼容](02-migration-identity-compatibility.md) | T01 类型契约 | 与 T03/T04 并行 |
| 1 | T03 | [统一安全审计闸门](03-security-gate.md) | T00 | 可与 T01/T02 并行 |
| 2 | T04 | [Chat ↔ Messages 严格 codec](04-codec-chat-messages.md) | T00、T03 的审计契约 | 可与 T02 并行 |
| 2 | T05 | [模型优先 RoutePlan 与重试状态机](05-route-plan-and-retry.md) | T01、T02、T03 | T04 可先并行开发，集成时依赖 T04 |
| 2 | T06 | [协议端点执行器与 Responses/Ollama](06-endpoint-executors.md) | T01、T02、T05 | 不与 T05 同时修改 handlers |
| 3 | T07 | [草稿连通性与端点测试](07-channel-draft-testing.md) | T01、T06 | 可与 T08 前端静态布局并行 |
| 3 | T08 | [渠道表单与列表双标签](08-channel-ui.md) | T01 API、T02 DTO、T07 API | 前端独占任务 |
| 3 | T09 | [导入导出、日志和模型映射保真](09-import-export-observability.md) | T02、T05 | 可与 T07/T08 并行，协调 DTO 文件 |
| 4 | T10 | [集成测试、灰度与发布门槛](10-integration-rollout.md) | T01–T09 | 最终串行门禁 |

## 推荐 Agent 分工

- Agent A：T01，独占 `channel_presets` 与共享领域类型。
- Agent B：T02，独占 migration、DB model/repository 和身份解析。
- Agent C：T03，独占 security gate、scanner 预算和日志脱敏接口。
- Agent D：T04，独占 protocol codec 与表驱动 codec 测试。
- Agent E：T05→T06，串行负责 routeplan、stream supervisor、handlers 和 executors，避免核心路由冲突。
- Agent F：T07，负责草稿测试命令与测试结果分类。
- Agent G：T08，独占 React 表单、渠道列表和前端类型/API 接入。
- Agent H：T09，负责 import/export、日志字段和模型映射一致性；修改 DTO 前先与 B/G 对齐。
- 集成 Agent：T10，只做整合、测试补齐和缺陷修复，不重写已验收模块。

## 合并顺序

1. T01 与 T03 先合并，提供共享类型和安全闸门契约。
2. T02 合并后冻结 migration 与 DTO schema。
3. T04 独立通过 codec 测试后合并，但尚不接入生产路由。
4. T05 接入 RoutePlan；T06 接入 executor、原生 Responses、Ollama 和 codec。
5. T07、T08、T09 合并业务与 UI。
6. T10 通过全量门禁后才能开启功能开关。

## 跨任务工作规则

- 不回退其他 Agent 的修改；发现接口冲突先在任务交接记录中说明，再做最小兼容调整。
- 每个任务只修改“文件所有权”列出的文件。确需跨界修改时，先由拥有者提供接口或在串行阶段处理。
- 所有新数据字段必须有旧 payload 缺省语义、旧记录推断语义、导入导出语义和回滚语义。
- 所有 codec 和安全拒绝必须在访问 mock upstream 前失败，测试断言上游调用次数为零。
- 原生协议直通保留未知 JSON 字段；协议转换不支持的字段明确拒绝，不静默删除。
- 流式请求只有在首个完整、可解码、可转换的 frame 验证后才提交下游响应；提交后不得换上游。
- 每个任务完成时提供：修改文件、接口变更、测试命令及结果、兼容性说明、未决风险。任务文档没有授权提交、推送或发布。

## 全局完成定义

- `pnpm build`、Rust fmt/clippy/test 及新增集成测试全部通过。
- 三协议的模型筛选、分组、优先级、权重、重试和流式 commit barrier 有确定性测试。
- 新数据库、新版升级、回滚旧版写入、再次升级均不丢字段且能解析正确执行器。
- 所有公开入口执行权限检查和原始请求审计；日志不存在原始 secret。
- 旧 `/v1/responses` 经 Chat codec 的行为保留，原生 `/responses` 独立直通。
- Chat ↔ Messages 对支持矩阵内字段完成流/非流双向验证；不支持字段返回 4xx 且不上游。
- 自定义提供商默认选中，厂商预设可选择，渠道列表双标签正确。
- 功能开关关闭时可退回旧路由；数据库无需破坏性回滚。
