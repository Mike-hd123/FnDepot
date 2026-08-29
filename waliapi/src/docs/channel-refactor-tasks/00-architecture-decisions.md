# T00：架构决策冻结

## 目的

本文件是所有执行 Agent 的强制前置约束。若实现发现必须违反其中任一决策，停止相关实现并形成 ADR 补充，不得在局部模块自行改变语义。

## 决策 1：最小请求包络，不建立全协议大 IR

内部仅定义用于审计、路由和 attempt 的最小结构：

```rust
RequestEnvelope {
    downstream_protocol,
    endpoint,
    original_json,
    safe_forward_headers,
    query,
    model,
    stream,
    trace_id,
}

AuditedRequest {
    envelope,
    forward_json,
    sanitized_log_json,
    body_hash,
    body_len,
    audit_result,
    request_features,
}

PreparedAttempt {
    channel_id,
    route_group,
    upstream_protocol,
    upstream_endpoint,
    upstream_model,
    codec_version,
    encoded_body,
    conversion_report,
}
```

原生直通以原始协议 JSON 为基础，只允许安全脱敏和模型映射产生变化。跨协议转换由有向 codec 完成，不依赖一个试图表达所有协议语义的公共消息格式。

## 决策 2：领域数据只有一份运行时真相

渠道新字段为：

- `protocol`
- `provider`（普通豆包为 `doubao`，Anthropic Coding Plan 为 `doubao_coding_plan`）
- `native_base_url`
- `native_endpoints`
- `identity_revision`
- `preset_revision`

`executor_kind` 由 `resolve_channel_identity()` 根据协议身份派生，不作为所有新渠道的第二份持久化真相。只有旧 Gemini 使用 `legacy_executor_override=gemini_native`。旧 Responses→Chat 债务统一存于 `config.legacy_capabilities=["responses_via_chat_v1"]`，不伪装成原生 Responses 能力；其他字符串均视为未知 capability。

## 决策 3：空数组兼容语义

为避免现有配置升级后大面积拒绝：

- `api_keys.allowed_models=[]` 表示不限制模型。
- `api_keys.allowed_channels=[]` 表示不限制渠道。
- 旧渠道 `models=[]` 保持 wildcard，表示该渠道接受任意请求模型。
- 新建渠道 UI 可以允许空模型列表，但必须显示“接受所有模型”的明确提示。
- `native_endpoints=[]` 不表示 wildcard；它表示身份尚未初始化或配置非法，必须通过 legacy identity resolver 推断或拒绝保存。

## 决策 4：路由次序

路由次序固定为：模型候选 → 原生协议分组 → 组内 priority tier → 同 tier weight 抽样 → 允许条件下进入转换组。转换组的 priority/weight 不与原生组比较。

每个 RouteGroup 有独立 `max_attempts_per_group`，请求有 `max_attempts_total`。同一请求中每个候选默认只尝试一次。

## 决策 5：错误分类与跨组条件

- `caller_terminal`：本地 schema、codec 不支持、上游返回 400/422；立即结束。
- `channel_auth_terminal`：上游 401/403；可继续同组下一渠道，但禁止跨协议组。
- `endpoint_unsupported`：405/501；404 只有响应明确表明端点路径不存在时才归类，模型不存在的 404 不得归类。
- `retryable`：连接失败、首帧超时、408、409、429、5xx、529；可在预算内继续同组并在本组耗尽后跨组。
- `upstream_protocol_error`：下游尚未 commit 且上游响应无法解码；可继续下一候选。
- `committed_stream_error`：下游已收到 header 或 body；不得重试，只能发送目标协议可表达的错误并结束。

带 `background`、`store` 或具有远端副作用的 Responses 请求默认禁用自动重试，除非后续 ADR 定义幂等键策略。

## 决策 6：流式 commit barrier

流状态固定为：

```text
Planned
→ Connecting
→ UpstreamHeadersReceived
→ FirstFrameBufferedAndValidated
→ DownstreamCommitted
→ Streaming
→ Completed | Aborted
```

只在 `DownstreamCommitted` 前允许换上游。转换流先缓冲一个有上限的完整 SSE record，并成功编码出首个下游事件后才能提交 200。原生 SSE 同样验证首个完整 record 后再释放原字节。

客户端取消时必须取消上游，并由 exactly-once finalizer 写入 `client_cancelled`。`[DONE]`、`message_stop`、`response.completed` 各方向只能终止一次。

## 决策 7：安全审计语义

认证与基础 schema 后，必须对下游原始协议 JSON 做安全扫描，再进入模型筛选、协议分组或转换。安全闸门返回：

- 上游使用的 `forward_json`
- 日志使用的 `sanitized_log_json`
- 审计结果与定位信息

允许转发的非凭证 header/query 纳入审计。Base64 附件只做类型、长度、hash 和元数据审计，不作为普通文本全量扫描。扫描预算按整个请求累计字节、节点、深度和耗时计算，UTF-8 截断必须在字符边界。

`SecurityAction::Confirm` 在 HTTP 网关中 fail-closed，返回 `approval_required`；没有一次性审批令牌时不得访问上游。

## 决策 8：codec 契约

codec 是 `(downstream_endpoint, upstream_endpoint, version)` 的有向实现，返回 `Result<Converted, UnsupportedFeatures>`。本期只强化 `chat_to_messages_v1` 与 `messages_to_chat_v1`。

支持：文本、保序 system/developer、user 图片、function tools/calls/results、可明确映射的采样参数和 stop、真实 usage、已定义 stop reason、流/非流 SSE。

拒绝：thinking/reasoning、structured output、built-in tools、documents/PDF、prompt cache annotations、未知 role/block/event/finish reason、非法 tool id/name/arguments、不能保真的 beta feature。拒绝必须发生在上游调用前。

Responses→Anthropic 属于下一阶段，不进入本期 RoutePlan。

## 决策 9：端点能力

`native_endpoints` 枚举至少包括：

- `chat_completions`
- `responses`
- `messages`
- `count_tokens`
- `embeddings`
- `api_chat`

OpenAI 表单只允许用户勾选 Chat 和 Responses；其他端点来自模板或高级配置。Ollama 原生配置在 UI 正式开放前，必须完成 `/api/chat` executor、模型枚举和下游 Chat 转换链；否则功能开关保持关闭。

## 决策 10：灰度和回滚

独立功能开关：

- `new_routeplan`
- `cross_protocol_codec`
- `native_responses`
- `ollama_native`

迁移只增列。新代码继续双写旧 `type/base_url`，并用 `identity_revision` 检测旧版回滚后的新写入。发生故障先关闭跨协议 codec，再关闭新 RoutePlan，不需要删除新字段。

本期固定采用 SQLite 失效触发器处理回滚写入：旧 `type/base_url/config` 变更时清空新身份并将 revision 置 0；新版更新事务必须先写旧字段，最后再重写完整新身份。不得由各任务自行选择另一套 source/revision 机制。

## 决策 11：原生直通的精确定义

“原生直通”表示不做跨协议语义转换，不承诺请求字节逐字相同。安全闸门可能对 JSON 做脱敏，模型映射会改写 `model`，实现也可能重新序列化 JSON。未知字段必须保留；若无法保证保留，则在上游调用前拒绝。原生 SSE 响应在首个完整 event 验证后保持 event 原始字节，不由 codec 重写。
