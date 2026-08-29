# T09：导入导出、日志和模型映射保真

## 目标

升级导入导出格式并修复现有字段丢失；统一 attempt 可观测性和模型映射，保证实际请求、日志、统计和回放信息一致。

## 依赖

- T02 schema/DTO。
- T05 PreparedAttempt、FailureClass、RouteGroup。
- 与 T08 共享 TypeScript DTO 时先遵循 T02 输出契约。

## 文件所有权

- 修改 `src-tauri/src/commands/import_export.rs`。
- 修改 request log model/repository 和必要 migration；若需新 migration 使用 T02 之后编号。
- 修改 `src-tauri/src/core/proxy.rs`/handlers 的日志调用点，仅接入 T05 已提供的 attempt context。
- 修改日志前端类型/展示仅限新增字段兼容。
- 不修改 route ordering、codec、provider templates、ChannelForm。

## 导出 v2

导出同时包含新身份和旧兼容字段：protocol、provider、native_base_url、native_endpoints、identity/preset revision、legacy type/base_url、models、status、priority、weight、config、model_mapping、timeout、测试状态。

API Key 按现有产品语义处理；若导出明文，必须保持现有明确用户动作和安全提示，不在诊断日志打印。

## 导入

- v1：通过同一个 identity resolver 推断，完整保留 status、timeout、priority、weight、config 未知键、数组 mapping、URL、Key、models。
- v2：验证新旧字段组合；不信任未知 protocol/provider/endpoint，降为 legacy/custom 且保留原始连接参数。
- Walicode/local scan：继续支持，所有猜测身份标 revision 0，由 resolver/用户确认。
- 导入使用能写 status/timeout 的专用 repository API，不复用固定 status=1/default timeout 的 create。

## 模型映射

数组 mapping 每个 attempt 只在 planner 抽样一次。adapter/codec/executor 不重新抽样。日志 `upstream_model`、请求 body、统计全部用 PreparedAttempt 值。

明确重试语义：每个新 attempt 可以重新抽样，但同一渠道同一请求默认只尝试一次，因此不会重复；若未来允许同渠道重试，配置决定是否保持模型。

## 日志字段

新增或记录：downstream protocol/endpoint、route group、upstream protocol/endpoint、provider、codec version、failure class、identity revision、upstream model、client_cancelled、stream committed。

所有 request body 使用 T03 `sanitized_log_json`。响应日志也做专用脱敏。原始 body 仅保存 hash/length。

## 实施步骤

1. 定义 WaliAPI export v2 和兼容 deserializer。
2. 修复 v1 import status/timeout 丢失。
3. 实现逐字段 round-trip comparator tests。
4. 扩展日志 schema/model/repository，保持旧查询兼容。
5. 将 PreparedAttempt 信息传入统一 log writer。
6. 删除 adapter/proxy 的第二次随机 mapping。
7. 更新日志前端类型，对旧日志字段缺失使用 nullable。

## 验收标准

- v1→DB、v2→DB、export→import 每个业务字段逐值相等。
- config 未知键和 mapping 数组不丢失。
- 实际 mock upstream model 等于日志 upstream_model。
- 原生 Anthropic 日志不再恒为 upstream_model=None。
- 流式取消、提交后错误和组间降级可从日志区分。
- 任何入口日志中不存在原始 secret。

## 测试命令

- `cargo test import_export --manifest-path src-tauri/Cargo.toml`
- `cargo test request_log --manifest-path src-tauri/Cargo.toml`
- `pnpm build`

## 交接输出

提供 v2 schema 示例、v1/v2 round-trip 报告、日志字段表、旧日志兼容说明和 mapping 一致性测试结果。
