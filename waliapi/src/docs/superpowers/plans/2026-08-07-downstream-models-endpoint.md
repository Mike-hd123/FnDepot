# 下游接口 /v1/models（兼容 OpenAI + Anthropic）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让现有 `/v1/models` 下游接口同时兼容 OpenAI 与 Anthropic 两种客户端（原生响应格式 + 对齐现有端点的鉴权/配额校验），纯后端改动。

**Architecture:** 不改路由（`/v1/models` 已注册）、不改 DB schema、不改前端、不新增依赖。只改 `handle_list_models` 一个入口函数：把「鉴权 + 配额 + 聚合配置模型 + 按请求头选格式」的核心逻辑抽到私有 `list_models_impl(pool, headers)`，使集成测试无需 Tauri `AppHandle`（只依赖 SQLite pool）即可调用；另抽出 4 个纯函数（格式判定 / 模型聚合 / OpenAI 响应 / Anthropic 响应）与 2 个错误构造辅助，便于单元测试。

**Tech Stack:** Rust, axum, sqlx (SQLite), serde_json, chrono；测试用 `#[cfg(test)]` 内联模块 + `sqlx::migrate!` + 内存 SQLite。

## Global Constraints

- 路由 `/v1/models` 保持已注册的 `get(handle_list_models)`（`src-tauri/src/server/router.rs:35`），不改路由注册。
- **只返回应用配置的模型**：来源仅两个 —— 每个 enabled channel 的 `models` 字段 + 每个 channel `model_mapping` 的 **keys**（value 是上游模型，**不参与列出**）。**绝不访问上游**。
- 两处来源去重合并（`HashSet` 判定，首个渠道胜出）。
- 鉴权/配额：复用 `protocol::extract_api_key`（`src-tauri/src/protocol/mod.rs:7`，同时支持 `Authorization: Bearer` 与 `x-api-key`）。无 key / key 无效 → 401；`quota_limit > 0 && quota_used >= quota_limit` → 429。
- 格式判定规则：**请求头含 `x-api-key` 且不含 `Authorization: Bearer` → Anthropic 格式；否则 → OpenAI 格式**。极端情况（两种都带 key）时 key 从 Bearer 取、格式按 OpenAI（保持现有行为）。
- 错误响应格式与下游协议匹配：
  - OpenAI：`{"error":{"message","type","code"}}`
  - Anthropic：复用 `anthropic_error` → `{"type":"error","error":{"type":...,"message":...}}`
- 不过滤 `allowed_models`（鉴权但不按 Key 过滤模型列表，属后续需求，YAGNI）。
- 不新增依赖；不改 DB schema；不改前端。
- 提交信息沿用仓库约定前缀 `feat（0.1.5）：`（见最近提交）。

---

### Task 1: `request_is_anthropic` —— 按请求头判定返回格式

**Files:**
- Modify: `src-tauri/src/server/handlers.rs`（在 `handle_health` 之后追加 `#[cfg(test)] mod list_models_tests` 测试模块 + 定义 `request_is_anthropic`）
- Test: `src-tauri/src/server/handlers.rs`（同模块内）

**Interfaces:**
- Consumes: `axum::http::HeaderMap`（已在文件顶部导入）。
- Produces: `fn request_is_anthropic(headers: &HeaderMap) -> bool` —— 后续 Task 4/5 用于选择成功与错误响应格式。

- [ ] **Step 1: 追加测试模块与失败测试**

在 `src-tauri/src/server/handlers.rs` 文件末尾（`handle_health` 的 `}` 之后）追加：

```rust
#[cfg(test)]
mod list_models_tests {
    use super::*;

    #[test]
    fn detects_anthropic_only_when_x_api_key_without_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "sk-test".parse().unwrap());
        assert!(request_is_anthropic(&headers));

        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer sk-test".parse().unwrap());
        assert!(!request_is_anthropic(&headers));

        // 极端情况：两法都带 key → 保持现有行为，按 OpenAI
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "sk-test".parse().unwrap());
        headers.insert("authorization", "Bearer sk-test".parse().unwrap());
        assert!(!request_is_anthropic(&headers));

        let headers = HeaderMap::new();
        assert!(!request_is_anthropic(&headers));
    }
}
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cd src-tauri && cargo test list_models_tests --lib 2>&1 | tail -20`
Expected: 编译错误 `cannot find function 'request_is_anthropic'`（失败即正确）。

- [ ] **Step 3: 写最小实现**

在 `src-tauri/src/server/handlers.rs` 中，`handle_list_models` 定义之前（例如 `handle_list_models` 函数上方）添加：

```rust
/// True when the request came from an Anthropic client: it authenticates with
/// `x-api-key` and sends no `Authorization: Bearer`. Matches the downstream
/// protocol selection rule (both present → OpenAI, keeping existing behavior).
fn request_is_anthropic(headers: &HeaderMap) -> bool {
    headers.contains_key("x-api-key") && !headers.contains_key("authorization")
}
```

- [ ] **Step 4: 运行测试验证通过**

Run: `cd src-tauri && cargo test list_models_tests --lib 2>&1 | tail -20`
Expected: `test list_models_tests::detects_anthropic_only_when_x_api_key_without_bearer ... ok`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/server/handlers.rs
git commit -m "feat（0.1.5）：/v1/models 支持 Anthropic 客户端（格式判定辅助）"
```

---

### Task 2: `ConfigModel` + `collect_config_models` —— 只聚合应用配置模型并去重

**Files:**
- Modify: `src-tauri/src/server/handlers.rs`
- Test: `src-tauri/src/server/handlers.rs`（`mod list_models_tests`）

**Interfaces:**
- Consumes: `crate::db::models::Channel`（现有 `handle_list_models` 已用 `serde_json::from_str(&ch.models)` / `&ch.model_mapping` 的解析方式）。
- Produces:
  - `struct ConfigModel { id: String, owned_by: String }`（`PartialEq` derive 供测试）
  - `fn collect_config_models(channels: &[crate::db::models::Channel]) -> Vec<ConfigModel>`
  - Task 3/5 依赖此函数：返回按 `channels` 顺序、去重后的配置模型（models 在前、mapping keys 在后；`owned_by` 为首个列出该模型的渠道类型）。

- [ ] **Step 1: 追加失败测试**

在 `mod list_models_tests` 中新增（紧接 Step 1 的测试之后）：

```rust
    fn channel(name: &str, ch_type: &str, models: &[&str], mapping: serde_json::Value) -> crate::db::models::Channel {
        crate::db::models::Channel {
            id: name.to_string(),
            name: name.to_string(),
            channel_type: ch_type.to_string(),
            base_url: "http://example.com".to_string(),
            api_key: "k".to_string(),
            models: serde_json::to_string(models).unwrap(),
            status: 1,
            priority: 0,
            weight: 1,
            config: "{}".to_string(),
            model_mapping: mapping.to_string(),
            timeout_secs: 60,
            created_at: "2026-01-01T00:00:00.000Z".to_string(),
            updated_at: "2026-01-01T00:00:00.000Z".to_string(),
            last_test_at: None,
            last_test_ok: None,
        }
    }

    #[test]
    fn aggregates_models_and_mapping_keys_with_dedup() {
        let channels = vec![
            channel("a", "openai", &["gpt-4o", "gpt-4o-mini"], serde_json::json!({"gpt-4o": "upstream-x"})),
            channel("b", "claude", &["claude-sonnet-4"], serde_json::json!({"gpt-4o": "claude-sonnet-5", "claude-35": "claude-3-5-sonnet"})),
        ];
        let models = collect_config_models(&channels);
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        // a.models → gpt-4o, gpt-4o-mini；a.mapping keys → gpt-4o(重复跳过)；
        // b.models → claude-sonnet-4；b.mapping keys → gpt-4o(重复跳过), claude-35
        assert_eq!(ids, vec!["gpt-4o", "gpt-4o-mini", "claude-sonnet-4", "claude-35"]);
        // 首个列出该模型的渠道胜出（owned_by 为 openai，而非 claude）
        assert_eq!(models[0].owned_by, "openai");
    }

    #[test]
    fn mapping_values_are_never_listed_as_models() {
        // value（"upstream-y"）是上游实际模型，不参与列出
        let channels = vec![channel("a", "openai", &["real-a"], serde_json::json!({"alias": "upstream-y"}))];
        let models = collect_config_models(&channels);
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["real-a", "alias"]);
    }
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cd src-tauri && cargo test list_models_tests --lib 2>&1 | tail -20`
Expected: 编译错误 `cannot find struct/variant/unit struct/union 'ConfigModel'` 或 `cannot find function 'collect_config_models'`。

- [ ] **Step 3: 写实现**

在 `request_is_anthropic` 函数下方添加：

```rust
/// One model exposed to downstream clients: its public `id` plus the channel
/// type of the first channel that listed it (kept for the OpenAI `owned_by`).
#[derive(Debug, Clone, PartialEq)]
struct ConfigModel {
    id: String,
    owned_by: String,
}

/// Aggregate configured models across enabled channels, deduped:
/// each channel's `models` list, then the keys of its `model_mapping`
/// (mapping values are upstream model names and are NOT exposed).
fn collect_config_models(channels: &[crate::db::models::Channel]) -> Vec<ConfigModel> {
    let mut out: Vec<ConfigModel> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for ch in channels {
        let ch_models: Vec<String> = serde_json::from_str(&ch.models).unwrap_or_default();
        for m in ch_models {
            if seen.insert(m.clone()) {
                out.push(ConfigModel { id: m, owned_by: ch.channel_type.clone() });
            }
        }
        let mapping: serde_json::Value = serde_json::from_str(&ch.model_mapping)
            .unwrap_or(serde_json::Value::Object(Default::default()));
        if let Some(obj) = mapping.as_object() {
            for key in obj.keys() {
                if seen.insert(key.clone()) {
                    out.push(ConfigModel { id: key.clone(), owned_by: ch.channel_type.clone() });
                }
            }
        }
    }
    out
}
```

- [ ] **Step 4: 运行测试验证通过**

Run: `cd src-tauri && cargo test list_models_tests --lib 2>&1 | tail -20`
Expected: `aggregates_models_and_mapping_keys_with_dedup ... ok` 与 `mapping_values_are_never_listed_as_models ... ok`。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/server/handlers.rs
git commit -m "feat（0.1.5）：/v1/models 模型聚合（models + mapping keys 去重）"
```

---

### Task 3: OpenAI / Anthropic 模型列表响应构造器

**Files:**
- Modify: `src-tauri/src/server/handlers.rs`
- Test: `src-tauri/src/server/handlers.rs`（`mod list_models_tests`）

**Interfaces:**
- Consumes: `ConfigModel`（Task 2）。
- Produces:
  - `fn openai_models_response(models: &[ConfigModel]) -> serde_json::Value` —— `{"object":"list","data":[{"id","object":"model","created","owned_by"}]}`
  - `fn anthropic_models_response(models: &[ConfigModel]) -> serde_json::Value` —— `{"data":[{"type":"model","id","display_name","created_at"}]}`
  - Task 5 依此构造成功响应体。

- [ ] **Step 1: 追加失败测试**

在 `mod list_models_tests` 中新增：

```rust
    #[test]
    fn builds_openai_models_response() {
        let models = vec![
            ConfigModel { id: "gpt-4o".to_string(), owned_by: "openai".to_string() },
            ConfigModel { id: "claude-35".to_string(), owned_by: "claude".to_string() },
        ];
        let value = openai_models_response(&models);
        assert_eq!(value["object"], "list");
        let data = value["data"].as_array().unwrap();
        assert_eq!(data.len(), 2);
        assert_eq!(data[0]["id"], "gpt-4o");
        assert_eq!(data[0]["object"], "model");
        assert_eq!(data[0]["owned_by"], "openai");
        assert!(data[0]["created"].is_number());
    }

    #[test]
    fn builds_anthropic_models_response() {
        let models = vec![ConfigModel { id: "claude-sonnet-4".to_string(), owned_by: "claude".to_string() }];
        let value = anthropic_models_response(&models);
        let data = value["data"].as_array().unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0]["type"], "model");
        assert_eq!(data[0]["id"], "claude-sonnet-4");
        assert_eq!(data[0]["display_name"], "claude-sonnet-4");
        assert!(data[0]["created_at"].as_str().is_some());
    }
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cd src-tauri && cargo test list_models_tests --lib 2>&1 | tail -20`
Expected: 编译错误 `cannot find function 'openai_models_response'` / `'anthropic_models_response'`。

- [ ] **Step 3: 写实现**

在 `collect_config_models` 函数下方添加：

```rust
/// OpenAI `/v1/models` response body: `{"object":"list","data":[...]}`.
fn openai_models_response(models: &[ConfigModel]) -> serde_json::Value {
    let data: Vec<serde_json::Value> = models
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "object": "model",
                "created": chrono::Utc::now().timestamp(),
                "owned_by": m.owned_by,
            })
        })
        .collect();
    serde_json::json!({ "object": "list", "data": data })
}

/// Anthropic `/v1/models` response body: `{"data":[{"type":"model","id",...}]}`.
fn anthropic_models_response(models: &[ConfigModel]) -> serde_json::Value {
    let data: Vec<serde_json::Value> = models
        .iter()
        .map(|m| {
            serde_json::json!({
                "type": "model",
                "id": m.id,
                "display_name": m.id,
                "created_at": crate::utils::time::now_iso(),
            })
        })
        .collect();
    serde_json::json!({ "data": data })
}
```

- [ ] **Step 4: 运行测试验证通过**

Run: `cd src-tauri && cargo test list_models_tests --lib 2>&1 | tail -20`
Expected: `builds_openai_models_response ... ok` 与 `builds_anthropic_models_response ... ok`。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/server/handlers.rs
git commit -m "feat（0.1.5）：/v1/models 响应构造器（OpenAI/Anthropic 格式）"
```

---

### Task 4: OpenAI 错误构造器 + 按协议分发错误

**Files:**
- Modify: `src-tauri/src/server/handlers.rs`
- Test: `src-tauri/src/server/handlers.rs`（`mod list_models_tests`）

**Interfaces:**
- Consumes: `anthropic_error`（`handlers.rs:539`，已存在，返回 `{"type":"error","error":{...}}`）、`request_is_anthropic`（Task 1）。
- Produces:
  - `fn openai_error(status: StatusCode, message: impl Into<String>, kind: &str) -> Response` —— `{"error":{"message","type","code"}}`
  - `fn models_error(anthropic: bool, status: StatusCode, kind: &str, message: impl Into<String>) -> Response` —— 按 `anthropic` 布尔分发到 `anthropic_error` 或 `openai_error`
  - Task 5 用它构造 401/429/500 错误。

- [ ] **Step 1: 追加失败测试**

在 `mod list_models_tests` 中新增：

```rust
    #[tokio::test]
    async fn builds_openai_style_errors() {
        let resp = openai_error(StatusCode::UNAUTHORIZED, "Missing API key", "authentication_error");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["type"], "authentication_error");
        assert_eq!(json["error"]["message"], "Missing API key");
        assert_eq!(json["error"]["code"], "401");
    }

    #[tokio::test]
    async fn models_error_dispatches_by_protocol() {
        let resp = models_error(true, StatusCode::UNAUTHORIZED, "authentication_error", "nope");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["type"], "error"); // Anthropic wrapper

        let resp = models_error(false, StatusCode::UNAUTHORIZED, "authentication_error", "nope");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["type"], "authentication_error");
        assert_eq!(json["error"]["code"], "401");
    }
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cd src-tauri && cargo test list_models_tests --lib 2>&1 | tail -20`
Expected: 编译错误 `cannot find function 'openai_error'` / `'models_error'`。

- [ ] **Step 3: 写实现**

在 `anthropic_models_response` 函数下方添加（`anthropic_error` 已在文件上方定义）：

```rust
/// OpenAI-style error body: `{"error":{"message","type","code"}}`.
fn openai_error(status: StatusCode, message: impl Into<String>, kind: &str) -> Response {
    let message = message.into();
    let code = status.as_u16().to_string();
    (
        status,
        Json(serde_json::json!({
            "error": { "message": message, "type": kind, "code": code }
        })),
    )
        .into_response()
}

/// Build an error in the caller's protocol format (Anthropic vs OpenAI).
fn models_error(anthropic: bool, status: StatusCode, kind: &str, message: impl Into<String>) -> Response {
    if anthropic {
        anthropic_error(status, kind, message)
    } else {
        openai_error(status, message, kind)
    }
}
```

- [ ] **Step 4: 运行测试验证通过**

Run: `cd src-tauri && cargo test list_models_tests --lib 2>&1 | tail -20`
Expected: `builds_openai_style_errors ... ok` 与 `models_error_dispatches_by_protocol ... ok`。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/server/handlers.rs
git commit -m "feat（0.1.5）：/v1/models 错误响应按协议分发"
```

---

### Task 5: `list_models_impl` 鉴权/配额/格式 + 重接 `handle_list_models` + 集成测试

**Files:**
- Modify: `src-tauri/src/server/handlers.rs`（`handle_list_models` 现有实现 `2600-2640` 替换）
- Test: `src-tauri/src/server/handlers.rs`（`mod list_models_tests`，含内存 DB 测试辅助）

**Interfaces:**
- Consumes: `protocol::extract_api_key`（`src-tauri/src/protocol/mod.rs:7`）、`Repository`（`src-tauri/src/db/repository.rs`）、`get_enabled_channels`、`get_api_key_by_key`、`increment_quota`（测试用）、Task 1–4 的 `request_is_anthropic` / `collect_config_models` / 两个响应构造器 / `models_error`。
- Produces:
  - `async fn list_models_impl(pool: SqlitePool, headers: &HeaderMap) -> Response` —— 鉴权 → 配额 → 聚合 → 按格式返回；集成测试直接调用（无需 `AppHandle`）。
  - 重写后的 `pub async fn handle_list_models(State(shared): State<SharedState>, headers: HeaderMap) -> Response`（新增 `headers` 提取器；路由无需改动）。
  - `async fn seed_test_db() -> (SqlitePool, ApiKey)`（测试辅助）。

- [ ] **Step 1: 追加失败测试（内存 DB + 集成断言）**

在 `mod list_models_tests` 中新增。需要给测试模块追加导入，把文件顶部的模块改为：

```rust
#[cfg(test)]
mod list_models_tests {
    use super::*;
    use crate::db::models::{ApiKey, CreateApiKeyInput, CreateChannelInput};
    use sqlx::sqlite::SqlitePoolOptions;
```

然后新增测试与辅助（Task 1–4 已有的测试保留不动）：

```rust
    async fn seed_test_db() -> (SqlitePool, ApiKey) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let repo = Repository::new(pool.clone());
        repo.create_channel(&CreateChannelInput {
            name: "ch-a".to_string(),
            channel_type: "openai".to_string(),
            base_url: "http://example.com".to_string(),
            api_key: "upstream".to_string(),
            models: vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()],
            priority: Some(0),
            weight: Some(1),
            config: None,
            model_mapping: Some(serde_json::json!({
                "alias-1": "gpt-4o",
                "alias-2": ["gpt-4o", "gpt-4o-mini"],
            })),
            timeout_secs: Some(60),
        }).await.unwrap();
        let api_key = repo.create_api_key(&CreateApiKeyInput {
            name: "test-key".to_string(),
            allowed_models: None,
            allowed_channels: None,
            quota_limit: Some(1000),
            expires_at: None,
        }).await.unwrap();
        (pool, api_key)
    }

    #[tokio::test]
    async fn openai_bearer_returns_openai_format() {
        let (pool, api_key) = seed_test_db().await;
        let mut headers = HeaderMap::new();
        headers.insert("authorization", format!("Bearer {}", api_key.key).parse().unwrap());
        let resp = list_models_impl(pool, &headers).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["object"], "list");
        let ids: Vec<&str> = json["data"].as_array().unwrap()
            .iter().map(|m| m["id"].as_str().unwrap()).collect();
        // models: gpt-4o, gpt-4o-mini；mapping keys: alias-1, alias-2 → 4 个，去重后无重复
        assert_eq!(ids.len(), 4);
        assert!(ids.contains(&"gpt-4o"));
        assert!(ids.contains(&"gpt-4o-mini"));
        assert!(ids.contains(&"alias-1"));
        assert!(ids.contains(&"alias-2"));
        assert!(json["data"].as_array().unwrap().iter().all(|m| m["object"] == "model"));
    }

    #[tokio::test]
    async fn anthropic_x_api_key_returns_anthropic_format() {
        let (pool, api_key) = seed_test_db().await;
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", api_key.key.parse().unwrap());
        let resp = list_models_impl(pool, &headers).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let data = json["data"].as_array().unwrap();
        assert!(!data.is_empty());
        assert!(data.iter().all(|m| m["type"] == "model"));
        assert!(data.iter().all(|m| m["id"].as_str().is_some()));
    }

    #[tokio::test]
    async fn missing_key_returns_401_openai_format() {
        let (pool, _) = seed_test_db().await;
        let resp = list_models_impl(pool, &HeaderMap::new()).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["type"], "authentication_error");
        assert_eq!(json["error"]["code"], "401");
    }

    #[tokio::test]
    async fn invalid_key_returns_401() {
        let (pool, _) = seed_test_db().await;
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer sk-wrong".parse().unwrap());
        let resp = list_models_impl(pool, &headers).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn invalid_key_anthropic_returns_anthropic_error_format() {
        let (pool, _) = seed_test_db().await;
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "sk-wrong".parse().unwrap());
        let resp = list_models_impl(pool, &headers).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["type"], "error");
        assert_eq!(json["error"]["type"], "authentication_error");
    }

    #[tokio::test]
    async fn quota_exceeded_returns_429() {
        let (pool, api_key) = seed_test_db().await;
        Repository::new(pool.clone()).increment_quota(&api_key.id, 2000).await.unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("authorization", format!("Bearer {}", api_key.key).parse().unwrap());
        let resp = list_models_impl(pool, &headers).await;
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"]["type"], "rate_limit_error");
        assert_eq!(json["error"]["code"], "429");
    }
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cd src-tauri && cargo test list_models_tests --lib 2>&1 | tail -30`
Expected: 编译错误 `cannot find function 'list_models_impl'`（或 `handle_list_models` 签名未变导致的错误）。失败即正确。

- [ ] **Step 3: 写实现**

把现有 `handle_list_models`（`src-tauri/src/server/handlers.rs:2600-2640`）整体替换为：

```rust
/// Core of `/v1/models`: auth, quota, configured-model aggregation, then a
/// response in the caller's protocol format. Extractable because it never
/// needs the Tauri `AppHandle` — only a SQLite pool.
async fn list_models_impl(pool: SqlitePool, headers: &HeaderMap) -> Response {
    let anthropic = request_is_anthropic(headers);
    let api_key = match protocol::extract_api_key(headers) {
        Some(key) => key,
        None => {
            return models_error(anthropic, StatusCode::UNAUTHORIZED, "authentication_error", "Missing API key")
        }
    };
    let repo = Repository::new(pool);
    let key = match repo.get_api_key_by_key(&api_key).await {
        Ok(key) => key,
        Err(_) => {
            return models_error(anthropic, StatusCode::UNAUTHORIZED, "authentication_error", "Invalid API key")
        }
    };
    if key.quota_limit > 0 && key.quota_used >= key.quota_limit {
        return models_error(anthropic, StatusCode::TOO_MANY_REQUESTS, "rate_limit_error", "Quota exceeded");
    }
    let channels = match repo.get_enabled_channels().await {
        Ok(channels) => channels,
        Err(_) => {
            return models_error(anthropic, StatusCode::INTERNAL_SERVER_ERROR, "api_error", "Failed to load channels")
        }
    };
    let models = collect_config_models(&channels);
    let body = if anthropic {
        anthropic_models_response(&models)
    } else {
        openai_models_response(&models)
    };
    (StatusCode::OK, Json(body)).into_response()
}

pub async fn handle_list_models(
    State(shared): State<SharedState>,
    headers: HeaderMap,
) -> Response {
    list_models_impl(shared.state.db.pool.clone(), &headers).await
}
```

> 注意：`Repository::new(pool)` 消费 `pool`，因此测试中调用 `list_models_impl(pool, ...)` 时若后续还要用 pool，需传 `pool.clone()`（上面测试里 `quota_exceeded` 先 `pool.clone()` 建 repo 再传 pool，符合此签名）。

- [ ] **Step 4: 运行测试验证通过**

Run: `cd src-tauri && cargo test list_models_tests --lib 2>&1 | tail -30`
Expected: 全部 5 个新集成测试 + 之前 5 个单元测试通过。

- [ ] **Step 5: 确认路由无需改动并编译**

Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: 编译成功（`router.rs:35` 的 `get(handle_list_models)` 不报错；`handle_list_models` 签名新增 `headers` 提取器后 axum 自动注入）。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/server/handlers.rs
git commit -m "feat（0.1.5）：/v1/models 鉴权、配额与 Anthropic 格式支持"
```

---

### Task 6: 全量回归 + 更新设计文档状态

**Files:**
- Modify: `docs/superpowers/specs/2026-08-07-downstream-models-endpoint-design.md`

- [ ] **Step 1: 全量测试回归**

Run: `cd src-tauri && cargo test 2>&1 | tail -40`
Expected: 所有测试通过，无回归（现有 `anthropic_handler_tests`、`protocol` 等全部 `ok`）。

- [ ] **Step 2: 更新设计文档状态**

把 `docs/superpowers/specs/2026-08-07-downstream-models-endpoint-design.md` 顶部 `状态：设计中` 改为 `状态：已实现`，并在「验证」一节下方追加一行实现记录（日期 + 关联实现分支）。

- [ ] **Step 3: 提交**

```bash
git add docs/superpowers/specs/2026-08-07-downstream-models-endpoint-design.md
git commit -m "feat（0.1.5）：更新 /v1/models 设计文档状态为已实现"
```

- [ ] **Step 4: 核对改动范围（YAGNI 检查）**

Run: `git diff main...HEAD --stat 2>/dev/null | tail -20` 或 `git log --oneline main..HEAD`
Expected: 只改动 `src-tauri/src/server/handlers.rs` 与设计文档；无路由、DB schema、前端、依赖改动。

---

## Self-Review

**Spec coverage:**
- 兼容 OpenAI `Authorization: Bearer` / Anthropic `x-api-key` → Task 1（格式判定）+ Task 5（`extract_api_key` 复用，天然支持两种头）。
- 只返回应用配置模型、不访问上游 → Task 2 `collect_config_models`（仅 `channels` 表本地字段）+ 测试断言 mapping value 不列出。
- 聚合来源 models + mapping keys 去重 → Task 2。
- 鉴权（无 key/无效 key → 401）、配额（`quota_limit > 0 && quota_used >= quota_limit` → 429）→ Task 5 集成测试覆盖 401/429 全路径。
- 按请求鉴权方式区分响应格式（含「两法都带 key → OpenAI」极端情况）→ Task 1 测试第 3 例 + Task 5 两种格式集成测试。
- 错误响应格式匹配下游协议 → Task 4 `models_error` + Task 5 `invalid_key_anthropic` 断言 Anthropic 错误包装。
- 不做的事（不动态拉上游 / 不过滤 allowed_models / 不改路由、DB、前端 / 不新增依赖）→ Task 6 Step 4 核对；全计划无此类改动。

**Placeholder scan:** 无 TBD/TODO；每个代码步骤都给出完整可粘贴代码。

**Type consistency:** `list_models_impl(pool: SqlitePool, headers: &HeaderMap)` 在所有测试与 handler 中调用方式一致（`pool` 直接传或 `pool.clone()`）；`collect_config_models` 返回 `Vec<ConfigModel>` 且 Task 3 两个构造器签名均为 `&[ConfigModel]`；`models_error(anthropic: bool, ...)` 在 Task 4 定义、Task 5 调用，参数顺序一致。
