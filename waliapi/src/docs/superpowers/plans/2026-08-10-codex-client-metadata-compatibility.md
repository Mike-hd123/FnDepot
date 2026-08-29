# Codex `client_metadata` Compatibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Accept and preserve Codex `client_metadata` in account-backed Responses requests so GPT-5.6-luna requests match CLIProxyAPI's compatibility behavior instead of failing at `/client_metadata`.

**Architecture:** Extend the existing Codex boundary validator with one explicitly supported top-level field, `client_metadata`. The field will be copied as-is, including unknown nested keys, while the existing account-boundary behavior continues stripping `metadata` and `max_output_tokens` and forcing `stream: true` / `store: false`. No CPA identity-confusion rewriting is added because WaliAPI has no equivalent mechanism and the immediate compatibility requirement is preservation.

**Tech Stack:** Rust, `serde_json::Value`, Tokio tests, existing `src-tauri` test harness.

## Global Constraints

- Preserve `client_metadata` under its original top-level key; never rename it to `metadata`.
- Preserve the complete nested JSON value, including unknown future Codex subfields.
- Continue stripping public `metadata` and `max_output_tokens` before account-backend transmission.
- Continue rejecting unsupported non-null top-level fields and nulling unknown fields as currently implemented.
- Continue forcing `stream` to `true` and `store` to `false`.
- Do not add identity-confusion or client-metadata value rewriting in this change.

---

### Task 1: Pin the Codex metadata contract with focused tests

**Files:**
- Modify: `src-tauri/src/auth_provider/codex_backend.rs:761-792` (existing validator/account-boundary tests)
- Test: `src-tauri/src/auth_provider/codex_backend.rs` inline `#[cfg(test)]` module

**Interfaces:**
- Consumes: existing `validate_backend_request(&Value) -> Result<Value, ProviderError>` and `CodexProvider::outbound` test fixture.
- Produces: tests proving `client_metadata` is preserved through both pure validation and the account backend boundary.

- [ ] **Step 1: Add a failing pure-validation test**

Add a test beside `backend_request_strips_max_output_tokens`:

```rust
#[test]
fn backend_request_preserves_client_metadata() {
    let metadata = json!({
        "x-codex-installation-id": "install-1",
        "x-codex-window-id": "window-1",
        "ws_request_header_x_openai_internal_codex_responses_lite": "true",
        "future-field": {"nested": [1, 2, 3]}
    });
    let body = validate_backend_request(&json!({
        "model": "gpt-5.6-luna",
        "input": "hi",
        "client_metadata": metadata
    }))
    .unwrap();

    assert_eq!(body["client_metadata"]["x-codex-installation-id"], "install-1");
    assert_eq!(body["client_metadata"]["x-codex-window-id"], "window-1");
    assert_eq!(
        body["client_metadata"]["ws_request_header_x_openai_internal_codex_responses_lite"],
        "true"
    );
    assert_eq!(body["client_metadata"]["future-field"]["nested"], json!([1, 2, 3]));
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
}
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run:

```bash
cd src-tauri && cargo test backend_request_preserves_client_metadata --lib
```

Expected: FAIL with `UnsupportedFeatures` for `/client_metadata`.

- [ ] **Step 3: Add a failing account-boundary test**

Add a Tokio test beside `metadata_annotation_is_stripped_before_account_backend`:

```rust
#[tokio::test]
async fn client_metadata_is_preserved_before_account_backend() {
    let (provider, state) = provider(vec![]).await;
    let account = account();
    let payload = payload();
    provider
        .outbound(ProviderRequest {
            account: &account,
            payload: &payload,
            body: &json!({
                "model": "gpt-5.6-luna",
                "input": "hi",
                "client_metadata": {
                    "x-codex-window-id": "window-1",
                    "future-field": {"enabled": true}
                }
            }),
            headers: &HeaderMap::new(),
        })
        .await
        .unwrap();

    let requests = state.requests.lock().await;
    assert_eq!(requests[0].1["client_metadata"]["x-codex-window-id"], "window-1");
    assert_eq!(requests[0].1["client_metadata"]["future-field"]["enabled"], true);
    assert!(requests[0].1.get("metadata").is_none());
}
```

- [ ] **Step 4: Run the new account-boundary test and verify it fails**

Run:

```bash
cd src-tauri && cargo test client_metadata_is_preserved_before_account_backend --lib
```

Expected: FAIL before the network fixture receives a request because validation rejects `/client_metadata`.

- [ ] **Step 5: Commit the tests**

```bash
git add src-tauri/src/auth_provider/codex_backend.rs
git commit -m "test: cover Codex client metadata preservation"
```

### Task 2: Allow and preserve `client_metadata` in the Codex validator

**Files:**
- Modify: `src-tauri/src/auth_provider/codex_backend.rs:283-319`
- Test: `src-tauri/src/auth_provider/codex_backend.rs` inline tests from Task 1

**Interfaces:**
- Consumes: `validate_backend_request(body: &Value)` and its existing `ALLOWED` / `STRIPPED` policy.
- Produces: validated Responses body containing `client_metadata` unchanged when supplied.

- [ ] **Step 1: Add `client_metadata` to the allowed top-level field list**

Update the existing list to include the field next to the other request-body fields:

```rust
const ALLOWED: &[&str] = &[
    "model",
    "input",
    "instructions",
    "tools",
    "tool_choice",
    "client_metadata",
    "stream",
    "store",
];
```

Do not add it to `STRIPPED`; it must be copied into the encoded body.

- [ ] **Step 2: Run the focused tests and verify they pass**

Run:

```bash
cd src-tauri && cargo test backend_request_preserves_client_metadata --lib
cd src-tauri && cargo test client_metadata_is_preserved_before_account_backend --lib
```

Expected: PASS; the captured backend body contains the original nested `client_metadata`, while `metadata` remains absent.

- [ ] **Step 3: Run all Codex backend tests**

Run:

```bash
cd src-tauri && cargo test auth_provider::codex_backend --lib
```

Expected: PASS, including existing tests for unsupported public controls, `metadata` stripping, `max_output_tokens` stripping, forced stream/store, and account headers.

- [ ] **Step 4: Commit the implementation**

```bash
git add src-tauri/src/auth_provider/codex_backend.rs
git commit -m "fix: preserve Codex client metadata"
```

### Task 3: Verify regression coverage and request behavior

**Files:**
- Modify: none unless formatting requires it
- Test: `src-tauri/src/auth_provider/codex_backend.rs`, related `src-tauri` integration tests

**Interfaces:**
- Consumes: completed validator change and focused tests.
- Produces: verified evidence that only `client_metadata` compatibility changed and existing rejection/stripping behavior remains intact.

- [ ] **Step 1: Format the Rust source**

Run:

```bash
cd src-tauri && cargo fmt --check
```

Expected: PASS. If formatting fails, run `cargo fmt`, inspect the diff, and rerun `cargo fmt --check`.

- [ ] **Step 2: Run the complete Rust test suite**

Run:

```bash
cd src-tauri && cargo test
```

Expected: PASS. If an unrelated environment or pre-existing failure occurs, record the exact failing test and output rather than claiming full verification.

- [ ] **Step 3: Inspect the final diff**

Run:

```bash
git diff HEAD~2..HEAD -- src-tauri/src/auth_provider/codex_backend.rs
git status --short
```

Confirm the final change:

- adds `client_metadata` to `ALLOWED`;
- does not rename or rewrite the field;
- retains `metadata` and `max_output_tokens` stripping;
- retains forced `stream` and `store` values;
- adds focused preservation tests;
- does not alter unrelated routing or authentication code.

- [ ] **Step 4: Commit any formatting-only correction, if needed**

```bash
git add src-tauri/src/auth_provider/codex_backend.rs
git commit -m "chore: format Codex backend tests"
```

Skip this step when no correction was needed.
