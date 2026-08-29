# 设计：下游接口 /v1/models（兼容 OpenAI + Anthropic）

日期：2026-08-07
状态：已实现

## 背景

WaLiAPI 作为网关对外提供下游接口。当前 `/v1/models` 路由已存在（`src-tauri/src/server/router.rs:38` → `handle_list_models`），但只返回 OpenAI 格式、无鉴权。需要让该接口同时兼容 OpenAI 与 Anthropic 两种下游客户端。

## 需求

- 下游通过 `/v1/models` 查询当前应用**配置的模型**列表。
- **只返回应用配置的模型**，与上游无关（绝不动态拉取上游模型列表）。
- 同时兼容 OpenAI 客户端（`Authorization: Bearer`）与 Anthropic 客户端（`x-api-key`），各自拿到原生格式响应。
- 鉴权与配额校验对齐现有端点（`/v1/messages`、`/v1/chat/completions`）。

## 方案（确认中）

**纯后端改动**：不改路由（`/v1/models` 已注册），只改 `handle_list_models` 一个函数。

### 鉴权（对齐 /v1/messages）

复用 `protocol::extract_api_key(&headers)`（`src-tauri/src/protocol/mod.rs:7`），它已同时支持：
- `Authorization: Bearer <key>`（OpenAI 客户端）
- `x-api-key: <key>`（Anthropic 客户端）

校验流程（与 `handle_chat_completions` / `handle_messages` 一致）：
- 无 key → 401
- key 无效（`get_api_key_by_key` 失败）→ 401
- 配额超限（`quota_limit > 0 && quota_used >= quota_limit`）→ 429

### 响应格式：按请求鉴权方式自动区分

| 请求特征 | 返回格式 | 结构 |
|---|---|---|
| 请求头含 `x-api-key`（Anthropic 客户端） | **Anthropic 格式** | `{"data":[{"type":"model","id":"...","display_name":"...","created_at":"..."}]}` |
| 仅 `Authorization: Bearer`（OpenAI 客户端） | **OpenAI 格式**（现状） | `{"object":"list","data":[{"id":"...","object":"model","created":...,"owned_by":"..."}]}` |

格式判定规则：**请求头含 `x-api-key` → Anthropic 格式；否则 → OpenAI 格式**。极端情况（两法都带 key）时 key 从 Bearer 取、格式按 OpenAI（保持现有行为）。

### 模型聚合（沿用现有逻辑，只返回应用配置）

来源为 enabled channels 的本地配置，**不访问上游**：
1. 每个 channel 的 `models` 字段（渠道表单配置的模型列表）
2. 每个 channel 的 `model_mapping` 的 **keys**（对下游暴露的模型别名；value 是上游实际模型，不参与列出）

两处去重合并。此逻辑与现有 `handle_list_models`（`handlers.rs:2908-2932`）一致，本设计不改动聚合来源。

### 错误响应格式：匹配下游协议

- OpenAI 格式错误：`{"error":{"message","type","code"}}`（现状）
- Anthropic 格式错误：复用现有 `anthropic_error` 辅助函数 → `{"type":"error","error":{"type":"authentication_error"|"rate_limit_error"|...,"message":...}}`

## 不做的事（YAGNI）

- 不动态拉取上游模型列表。
- 不过滤 allowed_models（鉴权但不按 Key 过滤模型列表；如需过滤属后续需求）。
- 不改路由注册、不改 DB schema、不改前端。
- 不新增依赖。

## 验证

- `cargo test` 新增后端集成测试：
  - OpenAI Bearer → 200 + OpenAI 格式
  - Anthropic x-api-key → 200 + Anthropic 格式
  - 无 key / 无效 key → 401；配额超限 → 429（各协议格式）
  - 模型聚合正确（models + mapping keys 去重）
- `cargo test` 全量回归通过（40 项 lib 测试全绿）。
- 与当前分支（`codex/channel-protocol-refactor-plan`）无冲突：`handle_list_models` 函数体在该分支未改动。

## 实现记录

- 2026-08-07 在 `codex/downstream-models-endpoint` 分支实现。核心逻辑抽到 `list_models_impl(pool, headers)`（`src-tauri/src/server/handlers.rs`），`handle_list_models` 新增 `headers: HeaderMap` 提取器，路由不变。实现计划见 `docs/superpowers/plans/2026-08-07-downstream-models-endpoint.md`。
