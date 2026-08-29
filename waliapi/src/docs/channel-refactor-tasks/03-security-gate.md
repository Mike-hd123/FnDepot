# T03：统一安全审计闸门

## 目标

让所有会接收或转发模型内容的公开推理入口在协议转换和访问上游前审计原始下游请求，覆盖 Chat、Legacy Completions、Responses、Messages、Count Tokens、Embeddings，以及启用后的 Images/Audio，生成独立的脱敏转发体与日志体，修复 Confirm fail-open、日志泄密和扫描预算风险。

## 依赖

- T00。
- 可与 T01/T02 并行；与 T05 集成前冻结 `AuditedRequest` 接口。

## 文件所有权

- 修改 `src-tauri/src/security/mod.rs`、`scanner.rs`、`redact.rs` 及 security tests。
- 新增 `src-tauri/src/security/gate.rs`。
- 可新增共享 `RequestEnvelope/AuditedRequest` 类型文件；若 T05 已建立 core request 模块，只实现 security-owned 部分。
- 对 handlers/proxy 的调用点只做最小接入；大规模 handler 重构由 T05/T06 完成。

## 安全闸门接口

输入：下游协议、endpoint、原始 JSON、允许转发的非凭证 headers/query、raw 长度/hash、API/安全设置。

输出：

- `forward_json`：按安全策略脱敏、供原生转发或 codec 使用。
- `sanitized_log_json`：始终适合持久化，不含原始 secret。
- `audit_result`：风险级别、规则命中、动作、定位。
- `request_features`：路由与 codec 预检需要的特征集合。
- body hash/len 与扫描预算状态。

禁止将原始 request body 交给日志层。原始字节只做长度、hash 和解析错误取证，不持久化全文。

## 覆盖规则

- 扫描原始协议 JSON 全树，不能用转换后的 Chat JSON替代。
- Responses built-in tools、图片 URL、文件元数据、未知内容块必须可定位。
- Anthropic beta headers 和允许转发的 query 纳入审计；Authorization、x-api-key、Cookie 等凭证 header 不进入上游复制或普通日志。
- Base64 图片/附件只扫描 media type、声明长度、实际长度和 hash；设置独立大小上限，不按文本规则完整扫描。
- 转换器或 provider execution profile 新增/改写的字符串做 delta scan。
- 响应非流做完整响应审计；流式逐完整事件做增量审计，命中 Block 时发送目标协议 error event 并关闭，无法撤回已下发内容。

## 动作语义

- Block：访问上游前拒绝。
- Redact：修改原始协议 JSON 中对应 JSON pointer；转换使用脱敏后原体。
- Audit/Allow：上游可收到允许的原始内容，但日志仍使用日志专用脱敏体。
- Confirm：HTTP 无交互审批令牌时返回 409 或 403，错误 code 为 `approval_required`，不得访问上游。

## 扫描预算

预算按整个请求累计：字节、字符串节点数、JSON 深度、扫描耗时。UTF-8 截断使用字符边界。超预算 fail-closed，返回明确 `security_scan_budget_exceeded`，不标记为 clean。

路由 body limit 与扫描预算分别配置；32MB/50MB 请求不能通过拆分大量小字符串绕过 CPU 预算。

## 实施步骤

1. 定义 `SecurityGateInput/Output` 与安全错误类型。
2. 重构 scanner 为全请求 budget context，消除直接字节切片 panic。
3. 扩展 JSON tree walker 与 JSON pointer 定位。
4. 实现原始协议 redaction 和日志专用 redaction。
5. 明确 Confirm fail-closed。
6. 枚举 `server/router.rs` 的公开推理路由并建立覆盖表；接入 Chat、Legacy Completions、Responses、Messages、Count Tokens、Embeddings。当前 501 Images/Audio 保持早拒绝，但为其写“启用后必须接 gate”的测试或显式 guard。
7. 修复 request log 调用，禁止传原始 request_body。
8. 为转换后 delta scan 提供窄接口，不让 provider 任意改写绕过。

## 验收标准

- 危险内容只存在于 Responses built-in tool、未知字段或图片 URL 时仍命中。
- redact 后原生和转换 mock upstream 都只看到脱敏值。
- Chat、Legacy Completions、Responses、Messages、Count Tokens、Embeddings 日志均无原始 secret；任何新启用的 Images/Audio handler 未接 gate 时测试必须失败。
- Confirm、预算超限和解析失败均不上游。
- UTF-8 任意边界输入不 panic。
- 安全 findings 的 phase、pointer 和动作可追踪。

## 测试命令

- `cargo test security --manifest-path src-tauri/Cargo.toml`
- `cargo test security_gate --manifest-path src-tauri/Cargo.toml`

## 交接输出

提供 gate API、动作表、预算默认值、各入口接入清单、脱敏日志样例和 mock upstream 零调用测试结果。
