# T10 发布与灰度说明（Release Notes & Gray Rollout）

状态：待发布（T10 不执行真实发布；本文件为发布材料）

## 1. 发布内容概述

渠道协议重构（T01–T09）已集成到当前工作树，T10 完成了 mock-upstream 集成测试套件、灰度演练与发布门槛验证。发布不改变下游公开 `/v1/*` 网关地址；改变的是渠道的协议/提供商/原生端点建模、模型优先 RoutePlan、严格 codec、统一安全审计、导入导出 v2 与请求日志可观测性。

## 2. 发布门槛（本任务已逐项验证）

| # | 门槛 | 命令 | 结果 |
|---|------|------|------|
| 1 | `cargo fmt --check` 通过 | `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | PASS |
| 2 | clippy `-D warnings`（记录基线、新代码零新增） | `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings` | 基线记录 117 条（详见下方），**新代码（T05–T10 文件）0 条** |
| 3 | `cargo test --all-targets --all-features` 通过 | 同上 | PASS：289 lib + 21 migration + 4 request_log = **314 用例 0 失败** |
| 4 | `pnpm build` 通过 | `pnpm build` | PASS（1.5s） |
| 5 | 集成测试无真实外部请求 | 见测试矩阵 | PASS（全部走本地 mock，仅 loopback） |
| 6 | migration 备份/恢复演练 | `drill_backup_and_restore_file_db_preserves_everything` | PASS |
| 7 | 安全负向测试上游零调用 | `security_*` 系列 | PASS（全部断言 `call_count == 0`） |
| 8 | 功能开关回退演练 | `flag_*` 系列 | PASS |
| 9 | 文档一致性 | 本文 + 任务交接 + 导出格式 v2.0 + 设计文档 | 一致 |

### 2.1 Clippy 基线（记录，不在本期清理）

- 当前 `cargo clippy --all-targets --all-features` 全树 **121 条原始 warning**（lib 115 + 测试重复）。
- 其中 **T05–T10 新建文件 0 条**（本次已清理：attempt/route_plan/driver/sse/channel_test/channel_identity/channel_migration/request_log 共 31 条）。
- **既有基线 117 条**（dedup）分布在：`protocol/codec/*`（unused imports / never-read 字段，T04 遗留）、`commands/app_config.rs`（9）、`services/knowledge/*`（retriever 12、splitter 6、code_parser 5、rag 4 等）、`security/*`（gate/scanner too-many-args）、`server/handlers.rs`（10，全部为审计前/原生 Anthropic/Responses 流既有代码）、`protocol/responses.rs`（6）等。这些是重构前已存在或 T04 codec 已合入的历史警告，**不属于本次新代码**，按 brief 要求记录为基线，不在 T10 扩大 diff 清理。

## 3. 灰度顺序（每步异常先关闭对应 feature flag，不删除数据）

| 步 | 动作 | 打开开关 | 验证 |
|----|------|---------|------|
| 1 | 发布 migration 015/016、presets、DTO、安全闸门，保持新 RoutePlan 关闭 | 全部关闭（`new_routeplan=false`） | 旧扁平路由不变；`flag_*` 负向测试证明零上游调用、安全闸门独立 |
| 2 | 内部开启 new_routeplan，仅原生组 | `new_routeplan=true` | 原生 Chat/Messages/Responses 直通；`routing_native_g1_before_conversion_g2_priority` 证明原生优先 |
| 3 | 开启 Chat↔Messages codec 小流量 | `cross_protocol_codec=true` | 观察 failure class 与 codec reject；`security_codec_reject_zero_upstream` 证明不支持字段 4xx 且零上游 |
| 4 | 开启原生 Responses | `native_responses=true` | `/v1/responses` 原生直通；`flag_native_responses_off_blocks_responses` 证明关闭即回退 503 |
| 5 | 开启 Ollama native | `ollama_native=true` | 原生 `/api/chat` 进入 RoutePlan；`flag_ollama_native_off_blocks_ollama` 证明关闭即 503 |
| 6 | UI 对全部用户开放新建入口 | 全部开启 | 渠道列表双标签、三协议自定义默认、逐端点测试 |

**回滚原则**：任一步异常 → 仅关闭对应 feature flag（`settings.json` 中 `features.*`），数据库无需破坏性回滚。回滚旧二进制前备份 SQLite（`waliapi.db`）并导出 v2 备份。旧二进制对新增列按 `type/base_url` 兼容读取；新组合（如 Anthropic/DeepSeek）在旧版仅保证 legacy alias 的基本能力。

## 4. 监控指标（灰度期观察）

- **RequestLog 新列**：`route_group`、`upstream_protocol`、`upstream_endpoint`、`provider`、`codec_version`、`failure_class`、`identity_revision`、`client_cancelled`、`stream_committed`。
- **失败分类分布**：`caller_terminal / channel_auth_terminal / endpoint_unsupported / retryable / upstream_protocol_error / committed_stream_error`。
- **codec reject 计数**：`status_code=400` + `codec_version != null` 的日志行。
- **跨组降级计数**：`route_group` 含 `_g2_conversion` 的日志行比例。
- **客户端取消**：`status_code=499` + `client_cancelled=1` 的行（T10 修复了重复写入缺陷，现在恰好一次）。
- **流式提交**：`stream_committed=1` 的行比例。
- **安全闸门**：`status_code=451` + `security_action=block` 的行（应保持零上游调用）。

## 5. 功能开关说明（产品行为）

| 开关 | OFF 时行为 | 测试证据 |
|------|-----------|---------|
| `features.new_routeplan` | 旧扁平 Dispatcher 路由 | handlers `maybe_route_plan` 返回 `Ok(None)` |
| `features.cross_protocol_codec` | 只走原生组；Anthropic 渠道不参与 Chat 降级 | `flag_cross_protocol_codec_off_blocks_conversion_zero_upstream` |
| `features.native_responses` | 原生 `/responses` 组不可用；仅显式 `responses_via_chat_v1` 债务渠道保留 | `flag_native_responses_off_blocks_responses` |
| `features.ollama_native` | 原生 Ollama `/api/chat` 不进入 RoutePlan | `flag_ollama_native_off_blocks_ollama` |

**安全闸门独立**：`authorize_request`（status/expires/quota/allowed_models/allowed_channels）在任何业务开关之前执行；`flag_security_gate_never_disabled_by_business_flags` 证明全关闭时禁用 key 仍被拒且零上游调用。

## 6. 严格 codec 的破坏性说明（必读）

启用 `cross_protocol_codec` 后，Chat↔Messages 双向转换对**不支持字段 fail-closed**：`response_format`/JSON-schema、`thinking`、built-in tools、未知内容块、无效工具参数等返回 4xx 且**上游零调用**（`security_codec_reject_zero_upstream`）。这会把旧版可能“静默容忍”的请求改为明确 4xx；灰度第 3 步的小流量阶段用于观察此类 rejection 比例，业务方需同步客户端。

## 7. 正式发布检查单

- [ ] `cargo fmt --check` 通过
- [ ] `cargo clippy --all-targets --all-features` 新代码 0 警告（基线 117 已记录）
- [ ] `cargo test --all-targets --all-features` 314 用例全绿
- [ ] `pnpm build` 通过
- [ ] 备份生产 SQLite + 导出 v2
- [ ] 执行 015/016 migration，记录迁移摘要（渠道数、identity 推断数）
- [ ] 确认安全闸门默认开启（`security.enabled=true`）
- [ ] 灰度第 1 步上线（新 RoutePlan 关闭），观察 24h 无异常
- [ ] 依次开启 new_routeplan → cross_protocol_codec → native_responses → ollama_native → UI
- [ ] 每步观察监控指标（失败分类、codec reject、跨组降级、client_cancelled）
- [ ] 保留 feature flag 关闭路径（回滚无需破坏性 DB 操作）
