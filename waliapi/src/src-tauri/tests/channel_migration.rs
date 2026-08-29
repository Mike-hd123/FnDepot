//! T02 migration + storage + dual-write integration tests.
//!
//! These exercise the real migration SQL against an in-memory SQLite database
//! and the repository dual-write path, covering:
//!   * fresh DB (all migrations incl. 015) schema + trigger presence
//!   * legacy "upgrade" DB: schema at 014, insert legacy rows, apply 015,
//!     resolver live-infers identity (no backfill needed)
//!   * old-schema INSERT lands at identity_revision 0
//!   * trigger invalidates new identity when type/base_url/config change
//!   * new two-step UPDATE in a transaction writes a full identity + revision
//!   * a mid-transaction failure rolls back fully (no half-new/half-legacy)
//!   * old payload create/update does not fail and preserves business fields
//!   * explicit empty native_endpoints on update is rejected
//!   * clear_api_key patch semantics (leave-blank keep vs explicit clear)
//!   * per-preset native/legacy URL fixtures (T01 table)

use serde_json::json;

// Integration test: reference the lib crate by its lib name.
use waliapi_lib::{
    core::channel_identity::{resolve_channel_identity, ChannelIdentity, ChannelIdentityRow},
    db::{models, repository::Repository},
};

fn now() -> String {
    models::now_iso()
}

/// Build an in-memory sqlite pool with all migrations applied (fresh DB).
async fn fresh_db() -> sqlx::SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory db");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrate fresh db");
    pool
}

/// Build an in-memory DB that has the PRE-015 schema (as if upgraded from an
/// older version): 001..=014 migrations applied, then 015 applied.
/// This simulates the "upgrade" path on an existing deployment.
async fn upgraded_db() -> sqlx::SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory db");

    // Apply only migrations up to 014 (legacy schema).
    for sql in legacy_schema_migrations() {
        sqlx::raw_sql(sql)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("apply legacy schema migration: {e}"));
    }
    // Now apply 015 on top.
    sqlx::raw_sql(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations/015_channel_protocol_identity.sql"
    )))
    .execute(&pool)
    .await
    .expect("apply 015 migration on upgraded schema");
    pool
}

fn legacy_schema_migrations() -> Vec<&'static str> {
    vec![
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/001_init.sql"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/002_add_request_body.sql"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/003_security_audit.sql"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/004_security_rules.sql"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/005_add_response_choices_and_seq.sql"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/006_add_trace_id.sql"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/007_fix_log_seq.sql"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/008_knowledge_base.sql"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/009_add_mcp_enabled.sql"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/010_kb_upgrade.sql"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/011_chunk_symbol_metadata.sql"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/012_fts5_hybrid_search.sql"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/013_add_embedding_batch_size.sql"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/014_add_channel_timeout.sql"
        )),
    ]
}

/// Build an in-memory DB with ONLY the pre-015 (legacy) schema applied.
async fn legacy_db() -> sqlx::SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory db");
    for sql in legacy_schema_migrations() {
        sqlx::raw_sql(sql)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("apply legacy schema migration: {e}"));
    }
    pool
}

async fn insert_legacy_row(
    pool: &sqlx::SqlitePool,
    id: &str,
    channel_type: &str,
    base_url: &str,
    api_key: &str,
) {
    sqlx::query(
        "INSERT INTO channels (id, name, type, base_url, api_key, models, status, priority, weight, config, model_mapping, timeout_secs, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, 1, 0, 1, '{}', '{}', 60, ?, ?)",
    )
    .bind(id)
    .bind(format!("chan-{id}"))
    .bind(channel_type)
    .bind(base_url)
    .bind(api_key)
    .bind("[]")
    .bind(now())
    .bind(now())
    .execute(pool)
    .await
    .expect("insert legacy row");
}

async fn get_row(pool: &sqlx::SqlitePool, id: &str) -> models::Channel {
    sqlx::query_as::<_, models::Channel>("SELECT * FROM channels WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("fetch row")
}

fn row_to_identity(row: &models::Channel) -> ChannelIdentity {
    resolve_channel_identity(&ChannelIdentityRow::from(row))
}

// ===========================================================================
// Migration fresh + upgrade
// ===========================================================================

#[tokio::test]
async fn fresh_db_has_identity_columns_and_trigger() {
    let pool = fresh_db().await;
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM pragma_table_info('channels') WHERE name IN ('protocol','provider','native_base_url','native_endpoints','preset_revision','identity_revision','legacy_executor_override')",
    )
    .fetch_one(&pool)
    .await
    .expect("pragma");
    assert_eq!(row.0, 7, "all 7 identity columns must exist");

    let trg: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name='trg_channels_legacy_invalidate_identity'",
    )
    .fetch_one(&pool)
    .await
    .expect("trigger lookup");
    assert_eq!(trg.0, 1, "invalidation trigger must exist");
}

#[tokio::test]
async fn upgrade_db_applies_015_and_preserves_legacy_values() {
    // Start from the legacy schema (pre-015), insert a legacy row, then apply
    // 015 on top — exactly the "upgrade an existing deployment" path.
    let pool = legacy_db().await;

    sqlx::query(
        "INSERT INTO channels (id, name, type, base_url, api_key, models, status, priority, weight, config, model_mapping, timeout_secs, created_at, updated_at)
         VALUES ('pre1', 'old-deepseek', 'deepseek', 'https://api.deepseek.com', 'sk-old', '[\"deepseek-chat\"]', 1, 7, 3, '{\"custom\":1}', '{\"m\":\"n\"}', 45, '2024-01-01T00:00:00.000Z', '2024-01-01T00:00:00.000Z')",
    )
    .execute(&pool)
    .await
    .expect("pre-insert legacy row");
    sqlx::raw_sql(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations/015_channel_protocol_identity.sql"
    )))
    .execute(&pool)
    .await
    .expect("apply 015 to upgraded schema");

    let row = get_row(&pool, "pre1").await;
    // Business fields untouched.
    assert_eq!(row.channel_type, "deepseek");
    assert_eq!(row.base_url, "https://api.deepseek.com");
    assert_eq!(row.api_key, "sk-old");
    assert_eq!(row.priority, 7);
    assert_eq!(row.weight, 3);
    assert_eq!(row.timeout_secs, 45);
    assert_eq!(row.config, "{\"custom\":1}");
    assert_eq!(row.model_mapping, "{\"m\":\"n\"}");
    // New identity columns default to uninitialized.
    assert_eq!(row.identity_revision, 0);
    assert!(row.protocol.is_none());

    // Resolver live-infers identity from legacy fields (no backfill needed).
    let identity = row_to_identity(&row);
    assert_eq!(identity.protocol, "openai");
    assert_eq!(identity.provider, "deepseek");
    assert_eq!(identity.native_base_url, "https://api.deepseek.com");
    assert_eq!(identity.native_endpoints, vec!["chat_completions"]);
}

#[tokio::test]
async fn legacy_insert_lands_revision_zero_and_resolves_live() {
    let pool = upgraded_db().await;

    // "Old binary" INSERT: only legacy columns, no identity columns.
    insert_legacy_row(&pool, "c1", "ollama", "http://localhost:11434/v1", "").await;

    let row = get_row(&pool, "c1").await;
    assert_eq!(row.identity_revision, 0);
    assert!(row.protocol.is_none());

    let identity = row_to_identity(&row);
    assert_eq!(identity.protocol, "ollama");
    assert_eq!(identity.provider, "ollama");
    assert_eq!(identity.native_base_url, "http://localhost:11434");
    assert_eq!(identity.native_endpoints, vec!["api_chat"]);
    assert!(identity.inferred);
}

// ===========================================================================
// Trigger semantics
// ===========================================================================

#[tokio::test]
async fn trigger_invalidates_identity_on_legacy_type_change() {
    let pool = fresh_db().await;
    insert_legacy_row(&pool, "t1", "openai", "https://api.openai.com/v1", "sk").await;

    // Simulate a new-code write that set full identity + revision.
    sqlx::query(
        "UPDATE channels SET protocol='openai', provider='openai', native_base_url='https://api.openai.com/v1', native_endpoints='[\"chat_completions\",\"responses\"]', identity_revision=1 WHERE id='t1'",
    )
    .execute(&pool)
    .await
    .expect("set identity");
    let row = get_row(&pool, "t1").await;
    assert_eq!(row.identity_revision, 1);

    // Old binary changes `type` -> trigger must clear identity + revision 0.
    sqlx::query("UPDATE channels SET type='deepseek' WHERE id='t1'")
        .execute(&pool)
        .await
        .expect("old-binary type change");
    let row = get_row(&pool, "t1").await;
    assert_eq!(row.identity_revision, 0);
    assert!(row.protocol.is_none());
    assert!(row.native_base_url.is_none());
    assert_eq!(row.native_endpoints.as_deref(), Some("[]"));

    // Resolver re-infers from the NEW legacy type.
    let identity = row_to_identity(&row);
    assert_eq!(identity.provider, "deepseek");
}

#[tokio::test]
async fn trigger_invalidates_identity_on_base_url_change() {
    let pool = fresh_db().await;
    insert_legacy_row(&pool, "t2", "claude", "https://api.anthropic.com/v1", "sk").await;
    sqlx::query(
        "UPDATE channels SET protocol='anthropic', provider='anthropic', native_base_url='https://api.anthropic.com', native_endpoints='[\"messages\"]', identity_revision=1 WHERE id='t2'",
    )
    .execute(&pool)
    .await
    .expect("set identity");

    sqlx::query("UPDATE channels SET base_url='https://api.deepseek.com/anthropic' WHERE id='t2'")
        .execute(&pool)
        .await
        .expect("old-binary base_url change");
    let row = get_row(&pool, "t2").await;
    assert_eq!(row.identity_revision, 0);
    assert!(row.provider.is_none());
}

#[tokio::test]
async fn trigger_does_not_fire_on_non_identity_columns() {
    let pool = fresh_db().await;
    insert_legacy_row(&pool, "t3", "openai", "https://api.openai.com/v1", "sk").await;
    sqlx::query(
        "UPDATE channels SET protocol='openai', provider='openai', native_base_url='https://api.openai.com/v1', native_endpoints='[\"chat_completions\"]', identity_revision=1 WHERE id='t3'",
    )
    .execute(&pool)
    .await
    .expect("set identity");

    // Changing e.g. priority must NOT invalidate identity.
    sqlx::query("UPDATE channels SET priority=5 WHERE id='t3'")
        .execute(&pool)
        .await
        .expect("priority change");
    let row = get_row(&pool, "t3").await;
    assert_eq!(row.identity_revision, 1);
    assert_eq!(row.protocol.as_deref(), Some("openai"));
}

// ===========================================================================
// Repository dual-write create/update
// ===========================================================================

fn make_repo(pool: sqlx::SqlitePool) -> Repository {
    Repository::new(pool)
}

#[tokio::test]
async fn create_new_anthropic_dual_writes_type_and_compat_base() {
    let pool = fresh_db().await;
    let repo = make_repo(pool.clone());

    let input = models::CreateChannelInput {
        name: "anthropic-zhipu".into(),
        channel_type: "".into(),
        base_url: "".into(),
        api_key: "sk-zhipu".into(),
        models: vec!["glm-4.7".into()],
        priority: None,
        weight: None,
        config: None,
        model_mapping: None,
        timeout_secs: Some(120),
        protocol: Some("anthropic".into()),
        provider: Some("zhipu".into()),
        native_base_url: Some("https://open.bigmodel.cn/api/anthropic".into()),
        native_endpoints: Some(vec!["messages".into()]),
        preset_revision: Some("2026-08-04".into()),
        legacy_executor_override: None,
        ..Default::default()
    };

    let row = repo.create_channel(&input).await.expect("create anthropic");
    // Legacy dual-write: type=claude, base_url is the old adaptor compat root.
    assert_eq!(row.channel_type, "claude");
    assert_eq!(row.base_url, "https://open.bigmodel.cn/api/anthropic/v1");
    // New identity persisted.
    assert_eq!(row.identity_revision, 1);
    assert_eq!(row.protocol.as_deref(), Some("anthropic"));
    assert_eq!(row.provider.as_deref(), Some("zhipu"));
    assert_eq!(
        row.native_base_url.as_deref(),
        Some("https://open.bigmodel.cn/api/anthropic")
    );
    assert_eq!(row.native_endpoints.as_deref(), Some("[\"messages\"]"));
    // Business fields preserved.
    assert_eq!(row.timeout_secs, 120);
    assert_eq!(row.status, 1);

    // Old-code final URL from legacy fields must be correct.
    // Old claude adaptor appends /messages to base_url.
    let legacy_final = format!("{}/messages", row.base_url.trim_end_matches('/'));
    assert_eq!(
        legacy_final,
        "https://open.bigmodel.cn/api/anthropic/v1/messages"
    );
}

#[tokio::test]
async fn create_new_ollama_native_dual_writes_openai_compat() {
    let pool = fresh_db().await;
    let repo = make_repo(pool.clone());

    let input = models::CreateChannelInput {
        name: "ollama-native".into(),
        channel_type: "".into(),
        base_url: "".into(),
        api_key: "".into(),
        models: vec!["llama3.1".into()],
        priority: None,
        weight: None,
        config: None,
        model_mapping: None,
        timeout_secs: None,
        protocol: Some("ollama".into()),
        provider: Some("ollama".into()),
        native_base_url: Some("http://localhost:11434".into()),
        native_endpoints: Some(vec!["api_chat".into()]),
        preset_revision: Some("2026-08-04".into()),
        legacy_executor_override: None,
        ..Default::default()
    };

    let row = repo
        .create_channel(&input)
        .await
        .expect("create ollama native");
    assert_eq!(row.channel_type, "openai");
    assert_eq!(row.base_url, "http://localhost:11434/v1");
    assert_eq!(
        row.native_base_url.as_deref(),
        Some("http://localhost:11434")
    );
    assert_eq!(row.identity_revision, 1);
    // Old openai adaptor final URL.
    assert_eq!(
        format!("{}/chat/completions", row.base_url.trim_end_matches('/')),
        "http://localhost:11434/v1/chat/completions"
    );
    // Must never produce /v1/api/chat.
    let native_final = format!(
        "{}/api/chat",
        row.native_base_url
            .as_deref()
            .unwrap_or("")
            .trim_end_matches('/')
    );
    assert_eq!(native_final, "http://localhost:11434/api/chat");
    assert!(!native_final.contains("/v1/"));
}

#[tokio::test]
async fn create_new_openai_google_is_openai_type() {
    let pool = fresh_db().await;
    let repo = make_repo(pool.clone());

    let input = models::CreateChannelInput {
        name: "google-compat".into(),
        channel_type: "".into(),
        base_url: "".into(),
        api_key: "gkey".into(),
        models: vec!["gemini-3.6-flash".into()],
        priority: None,
        weight: None,
        config: None,
        model_mapping: None,
        timeout_secs: None,
        protocol: Some("openai".into()),
        provider: Some("google".into()),
        native_base_url: Some("https://generativelanguage.googleapis.com/v1beta/openai".into()),
        native_endpoints: Some(vec!["chat_completions".into()]),
        preset_revision: Some("2026-08-04".into()),
        legacy_executor_override: None,
        ..Default::default()
    };

    let row = repo
        .create_channel(&input)
        .await
        .expect("create google compat");
    // Must be type=openai, NOT gemini (avoids the legacy native adaptor).
    assert_eq!(row.channel_type, "openai");
    assert_eq!(row.identity_revision, 1);
}

#[tokio::test]
async fn create_old_payload_infers_and_preserves_fields() {
    let pool = fresh_db().await;
    let repo = make_repo(pool.clone());

    // Old frontend payload: only legacy fields (no protocol identity fields).
    let input = models::CreateChannelInput {
        name: "old-deepseek".into(),
        channel_type: "deepseek".into(),
        base_url: "https://api.deepseek.com".into(),
        api_key: "sk-old".into(),
        models: vec!["deepseek-chat".into()],
        priority: Some(3),
        weight: Some(2),
        config: Some(json!({"keep": true})),
        model_mapping: Some(json!({"a": "b"})),
        timeout_secs: Some(99),
        ..Default::default()
    };

    let row = repo
        .create_channel(&input)
        .await
        .expect("create old payload");
    // Legacy type/base preserved exactly.
    assert_eq!(row.channel_type, "deepseek");
    assert_eq!(row.base_url, "https://api.deepseek.com");
    assert_eq!(row.api_key, "sk-old");
    assert_eq!(row.priority, 3);
    assert_eq!(row.weight, 2);
    assert_eq!(row.timeout_secs, 99);
    assert_eq!(row.config, "{\"keep\":true}");

    // Identity inferred; revision stays 0 because it came from legacy fields.
    assert_eq!(row.identity_revision, 0);
    let identity = row_to_identity(&row);
    assert_eq!(identity.protocol, "openai");
    assert_eq!(identity.provider, "deepseek");
    assert_eq!(identity.native_base_url, "https://api.deepseek.com");
    assert!(identity.inferred);
}

#[tokio::test]
async fn create_legacy_gemini_keeps_override_and_original_url() {
    let pool = fresh_db().await;
    let repo = make_repo(pool.clone());

    let input = models::CreateChannelInput {
        name: "old-gemini".into(),
        channel_type: "gemini".into(),
        base_url: "https://generativelanguage.googleapis.com".into(),
        api_key: "gkey".into(),
        models: vec!["gemini-2.5-flash".into()],
        priority: None,
        weight: None,
        config: None,
        model_mapping: None,
        timeout_secs: None,
        ..Default::default()
    };

    let row = repo
        .create_channel(&input)
        .await
        .expect("create legacy gemini");
    // Legacy type/base preserved exactly (design 11.1).
    assert_eq!(row.channel_type, "gemini");
    assert_eq!(row.base_url, "https://generativelanguage.googleapis.com");
    // Resolver identity: openai/google + legacy native override.
    assert_eq!(
        row.legacy_executor_override.as_deref(),
        Some("gemini_native")
    );
    let identity = row_to_identity(&row);
    assert_eq!(identity.protocol, "openai");
    assert_eq!(identity.provider, "google");
    assert_eq!(identity.executor_kind, "gemini_native");
    assert_eq!(
        identity.native_base_url,
        "https://generativelanguage.googleapis.com"
    );
}

#[tokio::test]
async fn update_two_step_writes_full_identity_and_revision() {
    let pool = fresh_db().await;
    let repo = make_repo(pool.clone());
    insert_legacy_row(&pool, "u1", "openai", "https://api.openai.com/v1", "sk").await;

    let input = models::UpdateChannelInput {
        id: "u1".into(),
        name: None,
        channel_type: Some("claude".into()),
        base_url: Some("https://open.bigmodel.cn/api/anthropic".into()),
        api_key: None,
        models: None,
        status: None,
        priority: None,
        weight: None,
        config: None,
        model_mapping: None,
        timeout_secs: None,
        protocol: Some("anthropic".into()),
        provider: Some("zhipu".into()),
        native_base_url: Some("https://open.bigmodel.cn/api/anthropic".into()),
        native_endpoints: Some(vec!["messages".into()]),
        preset_revision: Some("2026-08-04".into()),
        legacy_executor_override: None,
        clear_api_key: None,
        ..Default::default()
    };

    let row = repo.update_channel(&input).await.expect("two-step update");
    assert_eq!(row.channel_type, "claude");
    // F1: the stored legacy base must be the DERIVED compat root (new_to_legacy),
    // NOT the raw input (which lacked "/v1") — old binaries must request
    // …/api/anthropic/v1/messages, never …/api/anthropic/messages.
    assert_eq!(row.base_url, "https://open.bigmodel.cn/api/anthropic/v1");
    assert_eq!(row.identity_revision, 1);
    assert_eq!(row.protocol.as_deref(), Some("anthropic"));
    assert_eq!(row.provider.as_deref(), Some("zhipu"));
    assert_eq!(
        row.native_base_url.as_deref(),
        Some("https://open.bigmodel.cn/api/anthropic")
    );
    assert_eq!(row.native_endpoints.as_deref(), Some("[\"messages\"]"));
    // api_key untouched (None = keep).
    assert_eq!(row.api_key, "sk");
}

#[tokio::test]
async fn update_new_payload_empty_legacy_fields_is_repaired() {
    // F1(b): a new-protocol payload that sends empty type/base_url (the new UI
    // only sends identity fields) must have its legacy dual-write pair derived
    // and stored, so old binaries can still route the channel.
    let pool = fresh_db().await;
    let repo = make_repo(pool.clone());
    insert_legacy_row(&pool, "u7", "openai", "https://api.openai.com/v1", "sk").await;

    let input = models::UpdateChannelInput {
        id: "u7".into(),
        name: Some("empty-legacy".into()),
        channel_type: Some(String::new()), // empty, not provided by new UI
        base_url: Some(String::new()),
        protocol: Some("anthropic".into()),
        provider: Some("zhipu".into()),
        native_base_url: Some("https://open.bigmodel.cn/api/anthropic".into()),
        native_endpoints: Some(vec!["messages".into()]),
        ..Default::default()
    };

    let row = repo
        .update_channel(&input)
        .await
        .expect("repair legacy pair");
    assert_eq!(row.channel_type, "claude");
    assert_eq!(row.base_url, "https://open.bigmodel.cn/api/anthropic/v1");
    assert_eq!(row.identity_revision, 1);
    assert_eq!(row.protocol.as_deref(), Some("anthropic"));
    assert_eq!(row.provider.as_deref(), Some("zhipu"));
}

#[tokio::test]
async fn update_old_payload_none_keeps_existing_identity() {
    let pool = fresh_db().await;
    let repo = make_repo(pool.clone());
    insert_legacy_row(&pool, "u2", "openai", "https://api.openai.com/v1", "sk").await;

    // New-code write first.
    let input = models::UpdateChannelInput {
        id: "u2".into(),
        name: Some("renamed".into()),
        ..Default::default()
    };
    let row = repo.update_channel(&input).await.expect("rename");
    assert_eq!(row.name, "renamed");
    // Only the name changed; legacy type/base unchanged => trigger didn't fire.
    // But plan falls back to legacy inference (no new fields) -> revision 0.
    assert_eq!(row.identity_revision, 0);
    let identity = row_to_identity(&row);
    assert_eq!(identity.provider, "openai");
}

#[tokio::test]
async fn update_explicit_empty_endpoints_rejected() {
    let pool = fresh_db().await;
    let repo = make_repo(pool.clone());
    insert_legacy_row(&pool, "u3", "openai", "https://api.openai.com/v1", "sk").await;

    let input = models::UpdateChannelInput {
        id: "u3".into(),
        native_endpoints: Some(vec![]),
        ..Default::default()
    };
    let res = repo.update_channel(&input).await;
    assert!(res.is_err(), "explicit empty endpoints must be rejected");
    let err = res.err().unwrap().to_string();
    assert!(err.contains("native_endpoints"), "{err}");

    // Row unchanged.
    let row = get_row(&pool, "u3").await;
    assert_eq!(row.api_key, "sk");
}

#[tokio::test]
async fn update_clear_api_key_semantics() {
    let pool = fresh_db().await;
    let repo = make_repo(pool.clone());
    insert_legacy_row(&pool, "u4", "openai", "https://api.openai.com/v1", "sk-old").await;

    // Leave-blank (None api_key, clear_api_key false) => keep key.
    let keep = models::UpdateChannelInput {
        id: "u4".into(),
        api_key: None,
        clear_api_key: Some(false),
        ..Default::default()
    };
    let row = repo.update_channel(&keep).await.expect("keep key");
    assert_eq!(row.api_key, "sk-old");

    // Explicit clear => empty key persisted.
    let clear = models::UpdateChannelInput {
        id: "u4".into(),
        api_key: None,
        clear_api_key: Some(true),
        ..Default::default()
    };
    let row = repo.update_channel(&clear).await.expect("clear key");
    assert_eq!(row.api_key, "");

    // Explicit new value => set key.
    let set = models::UpdateChannelInput {
        id: "u4".into(),
        api_key: Some("sk-new".into()),
        clear_api_key: None,
        ..Default::default()
    };
    let row = repo.update_channel(&set).await.expect("set key");
    assert_eq!(row.api_key, "sk-new");
}

#[tokio::test]
async fn mid_transaction_failure_rolls_back_fully() {
    let pool = fresh_db().await;
    let repo = make_repo(pool.clone());
    insert_legacy_row(&pool, "u5", "openai", "https://api.openai.com/v1", "sk").await;

    // This update writes a new identity but ALSO explicitly empties endpoints,
    // which is rejected. Because the validation happens before the tx begins,
    // nothing at all changes — proving the plan is atomic at the API boundary.
    let input = models::UpdateChannelInput {
        id: "u5".into(),
        channel_type: Some("claude".into()),
        base_url: Some("https://api.anthropic.com/v1".into()),
        protocol: Some("anthropic".into()),
        provider: Some("anthropic".into()),
        native_base_url: Some("https://api.anthropic.com".into()),
        native_endpoints: Some(vec![]), // rejected
        ..Default::default()
    };
    let res = repo.update_channel(&input).await;
    assert!(res.is_err());

    // The row must be fully unchanged: still legacy openai, revision 0, no new
    // identity (no half-new/half-legacy state).
    let row = get_row(&pool, "u5").await;
    assert_eq!(row.channel_type, "openai");
    assert_eq!(row.base_url, "https://api.openai.com/v1");
    assert_eq!(row.identity_revision, 0);
    assert!(row.protocol.is_none());
    assert!(row.native_base_url.is_none());

    // Resolver still sees the coherent legacy identity.
    let identity = row_to_identity(&row);
    assert_eq!(identity.provider, "openai");
    // Legacy openai rows are NOT natively /responses-capable without the
    // config legacy_capabilities debt marker (design 11.2).
    assert_eq!(identity.native_endpoints, vec!["chat_completions"]);
}

#[tokio::test]
async fn mid_transaction_sql_failure_rolls_back_fully() {
    // Directly drive the two-step UPDATE inside a transaction and force the
    // second statement to fail (invalid JSON written to native_endpoints which
    // a trigger/constraint would reject; here we force a NOT NULL violation by
    // writing NULL into native_endpoints). The whole transaction must roll back
    // leaving no half-new/half-legacy state.
    let pool = fresh_db().await;
    insert_legacy_row(&pool, "m1", "openai", "https://api.openai.com/v1", "sk").await;

    // Step 1: write legacy identity fields (fires the trigger, clears identity).
    let mut tx = pool.begin().await.expect("begin tx");
    sqlx::query("UPDATE channels SET type='claude', base_url='https://api.anthropic.com/v1', updated_at=? WHERE id='m1'")
        .bind(now())
        .execute(&mut *tx)
        .await
        .expect("step1 legacy write");

    // Step 2 (the "final" identity UPDATE): force a failure — native_endpoints
    // is NOT NULL DEFAULT '[]', writing NULL violates the constraint.
    let res = sqlx::query(
        "UPDATE channels SET protocol='anthropic', provider='anthropic', native_base_url='https://api.anthropic.com', native_endpoints=NULL, identity_revision=1 WHERE id='m1'",
    )
    .execute(&mut *tx)
    .await;
    assert!(res.is_err(), "step2 must fail");
    drop(tx); // rollback on drop

    // Row must be fully unchanged (legacy openai, revision 0, no new identity).
    let row = get_row(&pool, "m1").await;
    assert_eq!(row.channel_type, "openai");
    assert_eq!(row.base_url, "https://api.openai.com/v1");
    assert_eq!(row.identity_revision, 0);
    assert!(row.protocol.is_none());
    assert!(row.native_base_url.is_none());

    // Resolver still sees the coherent legacy identity.
    let identity = row_to_identity(&row);
    assert_eq!(identity.provider, "openai");
}

#[tokio::test]
async fn two_step_update_commits_both_legacy_and_identity() {
    let pool = fresh_db().await;
    let repo = make_repo(pool.clone());
    insert_legacy_row(&pool, "u6", "ollama", "http://localhost:11434/v1", "").await;

    // Convert to Anthropic/Ollama via the new API in one transaction.
    let input = models::UpdateChannelInput {
        id: "u6".into(),
        channel_type: Some("claude".into()),
        base_url: Some("http://localhost:11434/v1".into()),
        protocol: Some("anthropic".into()),
        provider: Some("ollama".into()),
        native_base_url: Some("http://localhost:11434".into()),
        native_endpoints: Some(vec!["messages".into()]),
        preset_revision: Some("2026-08-04".into()),
        ..Default::default()
    };
    let row = repo
        .update_channel(&input)
        .await
        .expect("convert to anthropic/ollama");
    assert_eq!(row.channel_type, "claude");
    assert_eq!(row.base_url, "http://localhost:11434/v1");
    assert_eq!(row.identity_revision, 1);
    assert_eq!(row.protocol.as_deref(), Some("anthropic"));
    assert_eq!(row.provider.as_deref(), Some("ollama"));
    assert_eq!(
        row.native_base_url.as_deref(),
        Some("http://localhost:11434")
    );
    // Legacy claude adaptor final URL.
    assert_eq!(
        format!("{}/messages", row.base_url.trim_end_matches('/')),
        "http://localhost:11434/v1/messages"
    );
}

// ===========================================================================
// Rollback -> re-upgrade simulation
// ===========================================================================

#[tokio::test]
async fn rollback_write_then_reupgrade_live_infers() {
    let pool = fresh_db().await;

    // New code writes a full identity.
    insert_legacy_row(&pool, "r1", "claude", "https://api.anthropic.com/v1", "sk").await;
    sqlx::query(
        "UPDATE channels SET protocol='anthropic', provider='anthropic', native_base_url='https://api.anthropic.com', native_endpoints='[\"messages\"]', identity_revision=1 WHERE id='r1'",
    )
    .execute(&pool)
    .await
    .expect("write identity");

    // "Rolled-back" old binary UPDATEs the legacy base_url -> trigger clears.
    // main 约定：deepseek anthropic 兼容 base 带 /v1。
    sqlx::query(
        "UPDATE channels SET base_url='https://api.deepseek.com/anthropic/v1' WHERE id='r1'",
    )
    .execute(&pool)
    .await
    .expect("rollback write");
    let row = get_row(&pool, "r1").await;
    assert_eq!(row.identity_revision, 0);
    assert!(row.provider.is_none());

    // Re-upgrade: resolver must live-infer from the current legacy fields.
    // T06 I-4 (leader adjudication): legacy claude live-infers
    // [messages, count_tokens] (the old type=="claude" predicate served
    // /v1/messages/count_tokens).
    let identity = row_to_identity(&row);
    assert_eq!(identity.protocol, "anthropic");
    assert_eq!(identity.provider, "deepseek");
    assert_eq!(identity.native_endpoints, vec!["messages", "count_tokens"]);
    assert!(identity.inferred);
}

// ===========================================================================
// Per-preset native/legacy URL fixtures (T01 contract)
// ===========================================================================

fn join(base: &str, path: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), path)
}

#[tokio::test]
async fn preset_url_fixtures_new_and_legacy() {
    let pool = fresh_db().await;
    let repo = make_repo(pool.clone());

    // Build each fixture via a new-code create and assert both the new-code
    // native final URL and the old-adaptor final URL from legacy fields.

    // 1) Anthropic / Anthropic
    let row = repo
        .create_channel(&models::CreateChannelInput {
            name: "fx-anthropic".into(),
            channel_type: "".into(),
            base_url: "".into(),
            api_key: "sk".into(),
            models: vec![],
            protocol: Some("anthropic".into()),
            provider: Some("anthropic".into()),
            native_base_url: Some("https://api.anthropic.com".into()),
            native_endpoints: Some(vec!["messages".into()]),
            ..Default::default()
        })
        .await
        .expect("create anthropic");
    assert_eq!(
        join(row.native_base_url.as_deref().unwrap(), "v1/messages"),
        "https://api.anthropic.com/v1/messages"
    );
    assert_eq!(row.channel_type, "claude");
    assert_eq!(
        join(&row.base_url, "messages"),
        "https://api.anthropic.com/v1/messages"
    );

    // 2) Anthropic / DeepSeek
    let row = repo
        .create_channel(&models::CreateChannelInput {
            name: "fx-deepseek".into(),
            channel_type: "".into(),
            base_url: "".into(),
            api_key: "sk".into(),
            models: vec![],
            protocol: Some("anthropic".into()),
            provider: Some("deepseek".into()),
            native_base_url: Some("https://api.deepseek.com/anthropic".into()),
            native_endpoints: Some(vec!["messages".into()]),
            ..Default::default()
        })
        .await
        .expect("create deepseek");
    assert_eq!(
        join(row.native_base_url.as_deref().unwrap(), "v1/messages"),
        "https://api.deepseek.com/anthropic/v1/messages"
    );
    assert_eq!(row.channel_type, "claude");
    assert_eq!(
        join(&row.base_url, "messages"),
        "https://api.deepseek.com/anthropic/v1/messages"
    );

    // 3) Anthropic / Zhipu
    let row = repo
        .create_channel(&models::CreateChannelInput {
            name: "fx-zhipu".into(),
            channel_type: "".into(),
            base_url: "".into(),
            api_key: "sk".into(),
            models: vec![],
            protocol: Some("anthropic".into()),
            provider: Some("zhipu".into()),
            native_base_url: Some("https://open.bigmodel.cn/api/anthropic".into()),
            native_endpoints: Some(vec!["messages".into()]),
            ..Default::default()
        })
        .await
        .expect("create zhipu");
    assert_eq!(
        join(row.native_base_url.as_deref().unwrap(), "v1/messages"),
        "https://open.bigmodel.cn/api/anthropic/v1/messages"
    );
    assert_eq!(
        join(&row.base_url, "messages"),
        "https://open.bigmodel.cn/api/anthropic/v1/messages"
    );

    // 4) Anthropic / Doubao Coding Plan
    let row = repo
        .create_channel(&models::CreateChannelInput {
            name: "fx-doubao".into(),
            channel_type: "".into(),
            base_url: "".into(),
            api_key: "sk".into(),
            models: vec![],
            protocol: Some("anthropic".into()),
            provider: Some("doubao_coding_plan".into()),
            native_base_url: Some("https://ark.cn-beijing.volces.com/api/coding".into()),
            native_endpoints: Some(vec!["messages".into()]),
            ..Default::default()
        })
        .await
        .expect("create doubao");
    assert_eq!(
        join(row.native_base_url.as_deref().unwrap(), "v1/messages"),
        "https://ark.cn-beijing.volces.com/api/coding/v1/messages"
    );
    assert_eq!(
        join(&row.base_url, "messages"),
        "https://ark.cn-beijing.volces.com/api/coding/v1/messages"
    );

    // 5) Anthropic / Ollama
    let row = repo
        .create_channel(&models::CreateChannelInput {
            name: "fx-anthropic-ollama".into(),
            channel_type: "".into(),
            base_url: "".into(),
            api_key: "".into(),
            models: vec![],
            protocol: Some("anthropic".into()),
            provider: Some("ollama".into()),
            native_base_url: Some("http://localhost:11434".into()),
            native_endpoints: Some(vec!["messages".into()]),
            ..Default::default()
        })
        .await
        .expect("create anthropic/ollama");
    assert_eq!(
        join(row.native_base_url.as_deref().unwrap(), "v1/messages"),
        "http://localhost:11434/v1/messages"
    );
    assert_eq!(row.base_url, "http://localhost:11434/v1");
    assert_eq!(
        join(&row.base_url, "messages"),
        "http://localhost:11434/v1/messages"
    );

    // 6) Ollama / Ollama (native)
    let row = repo
        .create_channel(&models::CreateChannelInput {
            name: "fx-ollama".into(),
            channel_type: "".into(),
            base_url: "".into(),
            api_key: "".into(),
            models: vec![],
            protocol: Some("ollama".into()),
            provider: Some("ollama".into()),
            native_base_url: Some("http://localhost:11434".into()),
            native_endpoints: Some(vec!["api_chat".into()]),
            ..Default::default()
        })
        .await
        .expect("create ollama native");
    assert_eq!(row.channel_type, "openai");
    assert_eq!(
        join(row.native_base_url.as_deref().unwrap(), "api/chat"),
        "http://localhost:11434/api/chat"
    );
    assert_eq!(row.base_url, "http://localhost:11434/v1");
    assert_eq!(
        join(&row.base_url, "chat/completions"),
        "http://localhost:11434/v1/chat/completions"
    );
    assert!(!join(row.native_base_url.as_deref().unwrap(), "api/chat").contains("/v1/"));

    // 7) OpenAI / OpenAI
    let row = repo
        .create_channel(&models::CreateChannelInput {
            name: "fx-openai".into(),
            channel_type: "".into(),
            base_url: "".into(),
            api_key: "sk".into(),
            models: vec![],
            protocol: Some("openai".into()),
            provider: Some("openai".into()),
            native_base_url: Some("https://api.openai.com/v1".into()),
            native_endpoints: Some(vec!["chat_completions".into()]),
            ..Default::default()
        })
        .await
        .expect("create openai");
    assert_eq!(row.channel_type, "openai");
    assert_eq!(row.base_url, "https://api.openai.com/v1");
    assert_eq!(
        join(&row.base_url, "chat/completions"),
        "https://api.openai.com/v1/chat/completions"
    );
}
