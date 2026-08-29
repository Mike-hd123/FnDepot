//! Protected browser-management command bridge.
use crate::{commands, db::repository::Repository, server::router::SharedState};
use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, Request, StatusCode},
    middleware::Next,
    response::{sse::Event, sse::KeepAlive, sse::Sse, IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
pub struct InvokeRequest {
    pub command: String,
    #[serde(default)]
    pub args: Value,
}

fn response(ok: bool, result: Option<Value>, error: Option<&str>) -> (StatusCode, Json<Value>) {
    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    (
        status,
        Json(json!({ "ok": ok, "result": result, "error": error })),
    )
}

fn token_matches(candidate: &str, expected: &str) -> bool {
    let candidate = candidate.as_bytes();
    let expected = expected.as_bytes();
    let max = candidate.len().max(expected.len());
    let mut different = candidate.len() ^ expected.len();
    for index in 0..max {
        different |=
            (*candidate.get(index).unwrap_or(&0) ^ *expected.get(index).unwrap_or(&0)) as usize;
    }
    different == 0
}

fn authorized(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(|candidate| token_matches(candidate, expected))
        .unwrap_or(false)
}

pub async fn require_admin(
    State(shared): State<SharedState>,
    headers: HeaderMap,
    request: Request<Body>,
    next: Next,
) -> Response {
    match shared.admin_token.as_deref() {
        Some(token) if authorized(&headers, token) => next.run(request).await,
        _ => (StatusCode::UNAUTHORIZED, "unauthorized").into_response(),
    }
}

/// Protect the externally-consumed MCP endpoint with a credential that has no
/// access to the browser administration bridge.  Keeping this separate from
/// `WALIAPI_ADMIN_TOKEN` lets users configure Agents without granting them
/// channel/key/settings management privileges.
pub async fn require_mcp(
    State(shared): State<SharedState>,
    headers: HeaderMap,
    request: Request<Body>,
    next: Next,
) -> Response {
    match shared.mcp_token.as_deref() {
        Some(token) if authorized(&headers, token) => next.run(request).await,
        _ => (StatusCode::UNAUTHORIZED, "unauthorized").into_response(),
    }
}

/// Authenticated server-sent event bridge for browser progress updates.
/// The admin token is accepted only as an Authorization header; it is never
/// placed in a query string where proxies and access logs could retain it.
pub async fn events(headers: HeaderMap, State(shared): State<SharedState>) -> Response {
    let Some(token) = shared.admin_token.as_deref() else {
        return (StatusCode::NOT_FOUND, "event bridge unavailable").into_response();
    };
    if !authorized(&headers, token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let Some(mut receiver) = shared.app.subscribe() else {
        return (StatusCode::NOT_FOUND, "event bridge unavailable").into_response();
    };
    let stream = async_stream::stream! {
        loop {
            match receiver.recv().await {
                Ok(runtime_event) => {
                    let event = Event::default()
                        .event(runtime_event.name)
                        .json_data(runtime_event.payload)
                        .unwrap_or_else(|_| Event::default().event("serialization-error").data("null"));
                    yield Ok::<_, std::convert::Infallible>(event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
        .into_response()
}

fn input<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, String> {
    serde_json::from_value(args.get("input").cloned().unwrap_or(args))
        .map_err(|e| format!("invalid command arguments: {e}"))
}
fn field<T: serde::de::DeserializeOwned>(
    args: &Value,
    snake: &str,
    camel: &str,
) -> Result<T, String> {
    args.get(snake)
        .or_else(|| args.get(camel))
        .cloned()
        .ok_or_else(|| format!("missing argument: {snake}"))
        .and_then(|value| {
            serde_json::from_value(value).map_err(|e| format!("invalid argument {snake}: {e}"))
        })
}
fn optional_field<T: serde::de::DeserializeOwned>(
    args: &Value,
    snake: &str,
    camel: &str,
) -> Result<Option<T>, String> {
    args.get(snake)
        .or_else(|| args.get(camel))
        .filter(|value| !value.is_null())
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| format!("invalid argument {snake}: {e}"))
}
fn value<T: serde::Serialize>(result: T) -> Result<Value, String> {
    serde_json::to_value(result).map_err(|e| e.to_string())
}

pub async fn invoke(
    headers: HeaderMap,
    State(shared): State<SharedState>,
    Json(request): Json<InvokeRequest>,
) -> impl IntoResponse {
    let Some(token) = shared.admin_token.as_deref() else {
        return response(false, None, Some("admin bridge unavailable"));
    };
    if !authorized(&headers, token) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"ok": false, "error": "unauthorized"})),
        );
    }
    let args = request.args;
    let result: Result<Value, String> = async {
        match request.command.as_str() {
            "get_server_status" => {
                value(commands::server::get_server_status_impl(&shared.state).await?)
            }
            "auth_accounts_list" => {
                value(commands::auth::auth_accounts_list_impl(&shared.state).await?)
            }
            "auth_providers_list" => value(commands::auth::auth_providers_list_impl().await?),
            "auth_login_start" => value(
                commands::auth::auth_login_start_impl(
                    field(&args, "provider", "provider")?,
                    optional_field(&args, "replace_account_id", "replaceAccountId")?,
                    &shared.state,
                )
                .await?,
            ),
            "auth_login_status" => value(
                commands::auth::auth_login_status_impl(
                    &field::<String>(&args, "session_id", "sessionId")?,
                    &shared.state,
                )
                .await?,
            ),
            "auth_login_callback" => value(
                commands::auth::auth_login_callback_impl(
                    &field::<String>(&args, "session_id", "sessionId")?,
                    &field::<String>(&args, "callback_url", "callbackUrl")?,
                    &shared.state,
                )
                .await?,
            ),
            "auth_login_cancel" => value(
                commands::auth::auth_login_cancel_impl(
                    &field::<String>(&args, "session_id", "sessionId")?,
                    &shared.state,
                )
                .await?,
            ),
            "auth_login_import" => value(
                commands::auth::auth_login_import_impl(
                    optional_field(&args, "provider", "provider")?,
                    optional_field(&args, "path", "path")?,
                    &shared.state,
                )
                .await?,
            ),
            "auth_login_import_content" => {
                let content: String = field(&args, "content", "content")?;
                value(
                    commands::auth::auth_login_import_content_impl(
                        optional_field(&args, "provider", "provider")?,
                        content.as_bytes(),
                        &shared.state,
                    )
                    .await?,
                )
            }
            "auth_default_import_path" => {
                value(commands::auth::auth_default_import_path_impl().await?)
            }
            "auth_logout" => value(
                commands::auth::auth_logout_impl(
                    &field::<String>(&args, "id", "id")?,
                    &shared.state,
                )
                .await?,
            ),
            "auth_refresh_token" => value(
                commands::auth::auth_refresh_token_impl(
                    &field::<String>(&args, "id", "id")?,
                    &shared.state,
                )
                .await?,
            ),
            "auth_sync_models" => value(
                commands::auth::auth_sync_models_impl(
                    &field::<String>(&args, "id", "id")?,
                    &shared.state,
                )
                .await?,
            ),
            "auth_export_json" => value(
                commands::auth::auth_export_json_impl(
                    &field::<String>(&args, "id", "id")?,
                    &field::<String>(&args, "path", "path")?,
                    &shared.state,
                )
                .await?,
            ),
            "auth_export_content" => value(
                commands::auth::auth_export_content_impl(
                    &field::<String>(&args, "id", "id")?,
                    &shared.state,
                )
                .await?,
            ),
            "auth_toggle" => value(
                commands::auth::auth_toggle_impl(
                    &field::<String>(&args, "id", "id")?,
                    field(&args, "disabled", "disabled")?,
                    &shared.state,
                )
                .await?,
            ),
            "auth_quota_status" => value(
                commands::auth::auth_quota_status_impl(
                    &field::<String>(&args, "id", "id")?,
                    &shared.state,
                )
                .await?,
            ),
            "auth_update" => {
                value(commands::auth::auth_update_impl(input(args)?, &shared.state).await?)
            }
            "export_channels" => {
                value(commands::import_export::export_channels_impl(&shared.state).await?)
            }
            "import_walicode_backup" => value(
                commands::import_export::import_walicode_backup_impl(
                    &field::<String>(&args, "content", "content")?,
                    &shared.state,
                )
                .await?,
            ),
            "import_waliapi_export" => value(
                commands::import_export::import_waliapi_export_impl(
                    &field::<String>(&args, "content", "content")?,
                    &shared.state,
                )
                .await?,
            ),
            "scan_local_ai_configs" => {
                value(commands::import_export::scan_local_ai_configs().await?)
            }
            "import_scanned_sources" => value(
                commands::import_export::import_scanned_sources_impl(
                    field(&args, "sources", "sources")?,
                    &shared.state,
                )
                .await?,
            ),
            "get_app_configs" => {
                value(commands::app_config::get_app_configs_impl(&shared.state).await?)
            }
            "apply_app_config" => value(
                commands::app_config::apply_app_config_impl(
                    &field::<String>(&args, "app_name", "appName")?,
                    &field::<String>(&args, "api_key", "apiKey")?,
                    &field::<String>(&args, "model", "model")?,
                    &shared.state,
                )
                .await?,
            ),
            "clear_app_config" => value(
                commands::app_config::clear_app_config_impl(&field::<String>(
                    &args, "app_name", "appName",
                )?)
                .await?,
            ),
            "get_app_config_content" => value(
                commands::app_config::get_app_config_content_impl(&field::<String>(
                    &args, "app_name", "appName",
                )?)
                .await?,
            ),
            "get_app_config_path" => value(
                commands::app_config::prepare_app_config_path_impl(&field::<String>(
                    &args, "app_name", "appName",
                )?)
                .await?,
            ),
            "get_dashboard_stats" => {
                value(commands::stats::get_dashboard_stats_impl(&shared.state).await?)
            }
            "get_channels" => value(commands::channel::get_channels_impl(&shared.state).await?),
            "get_channel" => value(
                commands::channel::get_channel_impl(
                    &field::<String>(&args, "id", "id")?,
                    &shared.state,
                )
                .await?,
            ),
            "get_channel_api_key" => value(
                commands::channel::get_channel_api_key_impl(
                    &field::<String>(&args, "id", "id")?,
                    &shared.state,
                )
                .await?,
            ),
            "get_channel_presets" => value(crate::channel_presets::groups_for_protocols()),
            "create_channel" => {
                value(commands::channel::create_channel_impl(input(args)?, &shared.state).await?)
            }
            "update_channel" => {
                value(commands::channel::update_channel_impl(input(args)?, &shared.state).await?)
            }
            "toggle_channel" => {
                commands::channel::toggle_channel_impl(
                    &field::<String>(&args, "id", "id")?,
                    field(&args, "status", "status")?,
                    &shared.state,
                )
                .await?;
                Ok(Value::Null)
            }
            "delete_channel" => {
                commands::channel::delete_channel_impl(
                    &field::<String>(&args, "id", "id")?,
                    &shared.state,
                )
                .await?;
                Ok(Value::Null)
            }
            "test_channel" => value(
                commands::channel::test_channel_impl(
                    &field::<String>(&args, "id", "id")?,
                    &shared.state,
                )
                .await?,
            ),
            "test_channel_draft" => value(
                commands::channel::test_channel_draft_impl(input(args)?, &shared.state).await?,
            ),
            "sync_upstream_models" => value(
                commands::channel::sync_upstream_models_impl(input(args)?, &shared.state).await?,
            ),
            "get_channel_stats" => {
                value(commands::channel::get_channel_stats_impl(&shared.state).await?)
            }
            "reorder_channels" => {
                let ids: Vec<String> = field(&args, "ordered_ids", "orderedIds")?;
                commands::channel::reorder_channels_impl(&ids, &shared.state).await?;
                Ok(Value::Null)
            }
            "get_channel_extra_keys" => {
                let repo = Repository::new(shared.state.db.pool.clone());
                let id: String = field(&args, "id", "id")?;
                value(
                    repo.get_channel_api_keys(&id)
                        .await
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .map(commands::channel::ChannelKeyDto::from)
                        .collect::<Vec<_>>(),
                )
            }
            "get_channel_extra_key_value" => {
                let id: String = field(&args, "key_id", "keyId")?;
                let row: Option<(String,)> =
                    sqlx::query_as("SELECT api_key FROM channel_api_keys WHERE id = ?")
                        .bind(id)
                        .fetch_optional(&shared.state.db.pool)
                        .await
                        .map_err(|e| e.to_string())?;
                row.map(|v| value(v.0))
                    .unwrap_or_else(|| Err("Key not found".into()))
            }
            "toggle_channel_extra_key" => {
                let repo = Repository::new(shared.state.db.pool.clone());
                repo.toggle_channel_api_key(
                    &field::<String>(&args, "key_id", "keyId")?,
                    field(&args, "status", "status")?,
                )
                .await
                .map_err(|e| e.to_string())?;
                Ok(Value::Null)
            }
            "delete_channel_extra_key" => {
                let repo = Repository::new(shared.state.db.pool.clone());
                repo.delete_channel_api_key(&field::<String>(&args, "key_id", "keyId")?)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(Value::Null)
            }
            "get_api_keys" => value(commands::api_key::get_api_keys_impl(&shared.state).await?),
            "create_api_key" => {
                value(commands::api_key::create_api_key_impl(input(args)?, &shared.state).await?)
            }
            "update_api_key" => {
                commands::api_key::update_api_key_impl(input(args)?, &shared.state).await?;
                Ok(Value::Null)
            }
            "delete_api_key" => {
                commands::api_key::delete_api_key_impl(
                    &field::<String>(&args, "id", "id")?,
                    &shared.state,
                )
                .await?;
                Ok(Value::Null)
            }
            "get_api_key_stats" => {
                value(commands::api_key::get_api_key_stats_impl(&shared.state).await?)
            }
            "get_logs" => value(commands::log::get_logs_impl(input(args)?, &shared.state).await?),
            "get_log" => value(
                commands::log::get_log_impl(&field::<String>(&args, "id", "id")?, &shared.state)
                    .await?,
            ),
            "get_log_security_findings" => value(
                commands::log::get_log_security_findings_impl(
                    &field::<String>(&args, "log_id", "logId")?,
                    &shared.state,
                )
                .await?,
            ),
            "delete_log" => {
                commands::log::delete_log_impl(&field::<String>(&args, "id", "id")?, &shared.state)
                    .await?;
                Ok(Value::Null)
            }
            "delete_logs_before" => value(
                commands::log::delete_logs_before_impl(
                    &field::<String>(&args, "before_date", "beforeDate")?,
                    &shared.state,
                )
                .await?,
            ),
            "delete_all_logs" => value(commands::log::delete_all_logs_impl(&shared.state).await?),
            "get_log_stats" => value(
                commands::log::get_log_stats_impl(
                    args.get("days")
                        .cloned()
                        .map(serde_json::from_value)
                        .transpose()
                        .map_err(|e| e.to_string())?,
                    &shared.state,
                )
                .await?,
            ),
            "get_settings" => {
                value(commands::settings::get_settings_impl(&shared.state.runtime).await?)
            }
            "get_feature_flags" => value(commands::settings::get_feature_flags_impl(
                &shared.state.runtime,
            )?),
            "apply_theme" => {
                shared.state.runtime.emit(
                    "theme-changed",
                    json!({"theme": field::<String>(&args, "theme", "theme")?}),
                )?;
                Ok(Value::Null)
            }
            "save_settings" => {
                // Tauri's generated JS command contract sends `{ settings }`,
                // not the `{ input }` envelope used by CRUD commands.
                let settings = field(&args, "settings", "settings")?;
                commands::settings::save_settings_impl(settings, &shared.state.runtime).await?;
                Ok(json!({"restart_required": true}))
            }
            "get_service_statuses" => {
                value(commands::services::get_service_statuses_impl(&shared.state).await?)
            }
            "get_kb_tags" => value(
                commands::knowledge_base::get_kb_tags_impl(
                    &shared.state,
                    &field::<String>(&args, "kb_id", "kbId")?,
                    optional_field(&args, "limit", "limit")?,
                )
                .await?,
            ),
            "get_wiki_tags" => value(
                commands::wiki::get_wiki_tags_impl(
                    &shared.state,
                    &field::<String>(&args, "project_id", "projectId")?,
                    optional_field(&args, "limit", "limit")?,
                )
                .await?,
            ),
            "get_builtin_security_rules" => {
                let pool = &shared.state.db.pool;
                let rules = crate::security::rules::BuiltinRuleRepository::get_all(pool)
                    .await
                    .map_err(|e| e.to_string())?;
                if rules.is_empty() {
                    crate::security::rules::seed_builtin_rules(pool)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                value(
                    crate::security::rules::BuiltinRuleRepository::get_all(pool)
                        .await
                        .map_err(|e| e.to_string())?,
                )
            }
            "update_builtin_security_rule" => {
                let id: String = field(&args, "id", "id")?;
                let update = serde_json::from_value(args.get("input").cloned().unwrap_or(args))
                    .map_err(|e| format!("invalid command arguments: {e}"))?;
                crate::security::rules::BuiltinRuleRepository::update(
                    &shared.state.db.pool,
                    &id,
                    &update,
                )
                .await
                .map_err(|e| e.to_string())?;
                Ok(Value::Null)
            }
            "delete_builtin_security_rule" => {
                crate::security::rules::BuiltinRuleRepository::delete(
                    &shared.state.db.pool,
                    &field::<String>(&args, "id", "id")?,
                )
                .await
                .map_err(|e| e.to_string())?;
                Ok(Value::Null)
            }
            "reset_builtin_security_rules" => {
                let pool = &shared.state.db.pool;
                crate::security::rules::BuiltinRuleRepository::reset_to_defaults(pool)
                    .await
                    .map_err(|e| e.to_string())?;
                value(
                    crate::security::rules::BuiltinRuleRepository::get_all(pool)
                        .await
                        .map_err(|e| e.to_string())?,
                )
            }
            "get_custom_security_rules" => value(
                crate::security::rules::CustomRuleRepository::get_all(&shared.state.db.pool)
                    .await
                    .map_err(|e| e.to_string())?,
            ),
            "create_custom_security_rule" => value(
                crate::security::rules::CustomRuleRepository::create(
                    &shared.state.db.pool,
                    &input(args)?,
                )
                .await
                .map_err(|e| e.to_string())?,
            ),
            "toggle_custom_security_rule" => {
                crate::security::rules::CustomRuleRepository::update_enabled(
                    &shared.state.db.pool,
                    &field::<String>(&args, "id", "id")?,
                    field(&args, "enabled", "enabled")?,
                )
                .await
                .map_err(|e| e.to_string())?;
                Ok(Value::Null)
            }
            "delete_custom_security_rule" => {
                crate::security::rules::CustomRuleRepository::delete(
                    &shared.state.db.pool,
                    &field::<String>(&args, "id", "id")?,
                )
                .await
                .map_err(|e| e.to_string())?;
                Ok(Value::Null)
            }
            _ => Err(format!("unsupported command: {}", request.command)),
        }
    }
    .await;
    match result {
        Ok(result) => response(true, Some(result), None),
        Err(error) => response(false, None, Some(&error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_comparison_requires_equal_values() {
        assert!(token_matches("abc", "abc"));
        assert!(!token_matches("abc", "abd"));
        assert!(!token_matches("abc", "abcd"));
    }

    #[test]
    fn authorization_requires_bearer_token() {
        let mut headers = HeaderMap::new();
        assert!(!authorized(&headers, "secret"));
        headers.insert(header::AUTHORIZATION, "Basic secret".parse().unwrap());
        assert!(!authorized(&headers, "secret"));
        headers.insert(header::AUTHORIZATION, "Bearer secret".parse().unwrap());
        assert!(authorized(&headers, "secret"));
        headers.insert(header::AUTHORIZATION, "Bearer other".parse().unwrap());
        assert!(!authorized(&headers, "secret"));
    }

    #[test]
    fn parser_accepts_tauri_input_envelope_and_direct_arguments() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct Payload {
            id: String,
        }
        assert_eq!(
            input::<Payload>(json!({"input": {"id": "one"}})).unwrap(),
            Payload { id: "one".into() }
        );
        assert_eq!(
            input::<Payload>(json!({"id": "two"})).unwrap(),
            Payload { id: "two".into() }
        );
        assert_eq!(
            field::<String>(&json!({"keyId": "three"}), "key_id", "keyId").unwrap(),
            "three"
        );
        assert_eq!(
            field::<Payload>(
                &json!({"settings": {"id": "wrapped"}}),
                "settings",
                "settings"
            )
            .unwrap(),
            Payload {
                id: "wrapped".into()
            }
        );
    }
}
