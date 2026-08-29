//! Channel import/export (T09 rewrite).
//!
//! * **Export v2** (`version: "2.0"`) writes BOTH the new protocol identity
//!   (protocol/provider/native_base_url/native_endpoints/identity+preset
//!   revision) AND the legacy compat fields (`type`/`base_url`), plus every
//!   business field.  The API key follows the existing product semantics: it is
//!   the user's explicit export action and the file is plaintext JSON — never
//!   written to diagnostic logs.
//! * **Import** accepts v1 and v2.  v1 (and any v2 whose identity does not
//!   validate) goes through the SAME unified resolver
//!   [`resolve_channel_identity`], never the old URL-guessing type inference.
//!   Unknown protocol/provider/endpoint degrades to legacy/custom WITHOUT
//!   losing URL/model/key.  Import uses the dedicated `Repository::import_channel`
//!   write path so `status`/`timeout_secs` are preserved verbatim (the old
//!   create-channel path hard-coded status=1 and a default timeout).

use crate::core::channel_identity::{
    resolve_channel_identity, ChannelIdentity, ChannelIdentityRow,
};
use crate::db::models::{Channel, ImportChannelInput};
use crate::db::repository::Repository;
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

// ─── Export types ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct WaliapiExport {
    pub version: String,
    pub exported_at: String,
    pub r#type: String,
    pub channels: Vec<ExportedChannel>,
}

/// A channel in the export/import file.
///
/// Serialization (export v2): every field is written.
/// Deserialization (import): the legacy business fields are `Option` so both
/// v1 files (which carry them all) and v2 files parse; the new identity fields
/// are `Option` so a v1 file (missing them) still parses and falls back to the
/// resolver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedChannel {
    pub name: String,
    #[serde(rename = "type")]
    pub channel_type: String,
    pub base_url: String,
    /// API key in plaintext.  This matches the existing product semantics: the
    /// user explicitly triggers the export and the file is their backup.  The
    /// key is never written to diagnostic logs.
    pub api_key: String,
    pub models: Vec<String>,
    #[serde(default)]
    pub status: Option<i64>,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub weight: Option<i64>,
    #[serde(default)]
    pub config: Option<Value>,
    #[serde(default)]
    pub model_mapping: Option<Value>,
    #[serde(default)]
    pub timeout_secs: Option<i64>,
    #[serde(default)]
    pub last_test_at: Option<String>,
    #[serde(default)]
    pub last_test_ok: Option<i64>,
    // --- New protocol identity (v2; absent in v1) ---
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub native_base_url: Option<String>,
    #[serde(default)]
    pub native_endpoints: Option<Vec<String>>,
    #[serde(default)]
    pub preset_revision: Option<String>,
    #[serde(default)]
    pub identity_revision: Option<i64>,
    #[serde(default)]
    pub legacy_executor_override: Option<String>,
}

impl From<Channel> for ExportedChannel {
    fn from(c: Channel) -> Self {
        let identity: ChannelIdentity = resolve_channel_identity(&ChannelIdentityRow::from(&c));
        ExportedChannel {
            name: c.name,
            channel_type: c.channel_type,
            base_url: c.base_url,
            api_key: c.api_key,
            models: serde_json::from_str(&c.models).unwrap_or_default(),
            status: Some(c.status),
            priority: Some(c.priority),
            weight: Some(c.weight),
            config: Some(
                serde_json::from_str(&c.config).unwrap_or(Value::Object(Default::default())),
            ),
            model_mapping: Some(
                serde_json::from_str(&c.model_mapping).unwrap_or(Value::Object(Default::default())),
            ),
            timeout_secs: Some(c.timeout_secs),
            last_test_at: c.last_test_at,
            last_test_ok: c.last_test_ok,
            protocol: Some(identity.protocol),
            provider: Some(identity.provider),
            native_base_url: Some(identity.native_base_url),
            native_endpoints: Some(identity.native_endpoints),
            preset_revision: c.preset_revision.clone(),
            identity_revision: Some(c.identity_revision),
            legacy_executor_override: identity.legacy_executor_override,
        }
    }
}

// ─── Walicode backup types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct WalicodeBackup {
    pub version: serde_json::Value,
    pub r#type: Option<String>,
    #[serde(default)]
    pub ai_settings: Option<WalicodeAiSettings>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct WalicodeAiSettings {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub provider_type: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub custom_models: Option<Vec<String>>,
    #[serde(default)]
    pub custom_providers: Option<Vec<WalicodeProvider>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct WalicodeProvider {
    pub name: String,
    #[serde(default)]
    pub api_key: Option<String>,
    pub base_url: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub custom_models: Option<Vec<String>>,
    #[serde(default)]
    pub api_format: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

// ─── Scan result types ──────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ScanResult {
    pub sources: Vec<ScannedSource>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScannedSource {
    pub source: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
    pub api_format: String,
    pub raw: serde_json::Value,
}

// ─── Import result types ────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ImportResult {
    pub imported: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

// ─── Commands ───────────────────────────────────────────────────────────────

/// Export all channels as a waliapi v2 JSON backup.
///
/// v2 carries BOTH the new protocol identity AND the legacy compat fields
/// (design 5.2).  The API key is included in plaintext by design — this is the
/// user's explicit backup action (the product's existing export semantics); it
/// is never written to diagnostic logs.
#[tauri::command]
pub async fn export_channels(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<String, String> {
    export_channels_impl(state.inner()).await
}

pub async fn export_channels_impl(state: &std::sync::Arc<AppState>) -> Result<String, String> {
    let repo = Repository::new(state.db.pool.clone());
    let channels = repo.get_all_channels().await.map_err(|e| e.to_string())?;

    let export = WaliapiExport {
        version: "2.0".to_string(),
        exported_at: chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string(),
        r#type: "waliapi-export".to_string(),
        channels: channels.into_iter().map(ExportedChannel::from).collect(),
    };

    serde_json::to_string_pretty(&export).map_err(|e| e.to_string())
}

/// Import channels from a walicode-full-backup.json file content.
///
/// The legacy `type` is derived from the walicode hints/URL (unchanged product
/// behavior), but the row is written through `Repository::import_channel` with
/// identity_revision 0 and NULL identity columns so the unified resolver
/// live-infers the protocol identity on next read (task 09: "Walicode/local
/// scan marks revision 0").
#[tauri::command]
pub async fn import_walicode_backup(
    content: String,
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<ImportResult, String> {
    import_walicode_backup_impl(&content, state.inner()).await
}

pub async fn import_walicode_backup_impl(
    content: &str,
    state: &std::sync::Arc<AppState>,
) -> Result<ImportResult, String> {
    let backup: WalicodeBackup =
        serde_json::from_str(&content).map_err(|e| format!("解析 walicode 备份文件失败: {}", e))?;

    let repo = Repository::new(state.db.pool.clone());
    let existing = repo.get_all_channels().await.map_err(|e| e.to_string())?;
    let existing_names: std::collections::HashSet<String> =
        existing.iter().map(|c| c.name.clone()).collect();

    let mut imported = 0;
    let mut skipped = 0;
    let mut errors: Vec<String> = Vec::new();

    // Import main aiSettings as a channel
    if let Some(ai) = &backup.ai_settings {
        if let (Some(api_key), Some(base_url)) = (ai.api_key.as_ref(), ai.base_url.as_ref()) {
            if !api_key.is_empty() && !base_url.is_empty() {
                let name = "walicode-default".to_string();
                if existing_names.contains(&name) {
                    skipped += 1;
                } else {
                    let models = ai.custom_models.clone().unwrap_or_default();
                    let models = if models.is_empty() {
                        ai.model.clone().into_iter().collect()
                    } else {
                        models
                    };

                    let channel_type = guess_channel_type(base_url, ai.provider_type.as_deref());

                    let input = ImportChannelInput {
                        name,
                        channel_type,
                        base_url: base_url.clone(),
                        api_key: api_key.clone(),
                        models,
                        status: 1,
                        priority: 0,
                        weight: 1,
                        config: Value::Object(Default::default()),
                        model_mapping: Value::Object(Default::default()),
                        timeout_secs: 300,
                        // identity_revision 0 + NULL identity => resolver infers on read
                        identity_revision: 0,
                        ..Default::default()
                    };

                    match repo.import_channel(&input).await {
                        Ok(_) => imported += 1,
                        Err(e) => errors.push(format!("导入 walicode 默认渠道失败: {}", e)),
                    }
                }
            }
        }

        // Import custom providers
        if let Some(providers) = &ai.custom_providers {
            for p in providers {
                let name = p.name.clone();
                if existing_names.contains(&name) {
                    skipped += 1;
                    continue;
                }

                let api_key = p.api_key.clone().unwrap_or_default();
                if api_key.is_empty()
                    && !p.base_url.contains("localhost")
                    && !p.base_url.contains("127.0.0.1")
                {
                    skipped += 1;
                    continue;
                }

                let models = p.custom_models.clone().unwrap_or_default();
                let models = if models.is_empty() {
                    p.model.clone().into_iter().collect()
                } else {
                    models
                };

                let channel_type = guess_channel_type(&p.base_url, p.api_format.as_deref());

                let input = ImportChannelInput {
                    name,
                    channel_type,
                    base_url: p.base_url.clone(),
                    api_key,
                    models,
                    status: 1,
                    priority: 0,
                    weight: 1,
                    config: Value::Object(Default::default()),
                    model_mapping: Value::Object(Default::default()),
                    timeout_secs: 300,
                    identity_revision: 0,
                    ..Default::default()
                };

                match repo.import_channel(&input).await {
                    Ok(_) => imported += 1,
                    Err(e) => errors.push(format!("导入渠道 '{}' 失败: {}", p.name, e)),
                }
            }
        }
    }

    Ok(ImportResult {
        imported,
        skipped,
        errors,
    })
}

/// Import channels from a waliapi export JSON file (v1 or v2).
///
/// * v1 → routed through the unified `resolve_channel_identity` (never the old
///   URL-guessing inference) and written with identity_revision 0.
/// * v2 → the new+old field combination is validated against the resolver and
///   the known protocol/provider/endpoint enum strings; unknown values degrade
///   to legacy/custom WITHOUT losing URL/model/key.
/// * `status`/`timeout_secs` (and every other business field) are preserved
///   verbatim via `Repository::import_channel` (round-trip contract 11.4).
#[tauri::command]
pub async fn import_waliapi_export(
    content: String,
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<ImportResult, String> {
    import_waliapi_export_impl(&content, state.inner()).await
}

pub async fn import_waliapi_export_impl(
    content: &str,
    state: &std::sync::Arc<AppState>,
) -> Result<ImportResult, String> {
    let export: WaliapiExport =
        serde_json::from_str(&content).map_err(|e| format!("解析 waliapi 导出文件失败: {}", e))?;

    let repo = Repository::new(state.db.pool.clone());
    let existing = repo.get_all_channels().await.map_err(|e| e.to_string())?;
    let existing_names: std::collections::HashSet<String> =
        existing.iter().map(|c| c.name.clone()).collect();

    let mut imported = 0;
    let mut skipped = 0;
    let mut errors: Vec<String> = Vec::new();

    for ch in export.channels {
        if existing_names.contains(&ch.name) {
            skipped += 1;
            continue;
        }

        let input = exported_channel_to_import(&ch);
        match repo.import_channel(&input).await {
            Ok(_) => imported += 1,
            Err(e) => errors.push(format!("导入渠道 '{}' 失败: {}", ch.name, e)),
        }
    }

    Ok(ImportResult {
        imported,
        skipped,
        errors,
    })
}

/// Convert one exported channel (v1 or v2) into an import-write input,
/// applying the identity validation/degradation rules described on
/// [`import_waliapi_export`].
pub fn exported_channel_to_import(ch: &ExportedChannel) -> ImportChannelInput {
    let config = ch
        .config
        .clone()
        .unwrap_or(Value::Object(Default::default()));
    let model_mapping = ch
        .model_mapping
        .clone()
        .unwrap_or(Value::Object(Default::default()));
    let legacy_type = ch.channel_type.clone();
    let legacy_base = ch.base_url.clone();
    let file_rev = ch.identity_revision.unwrap_or(0);

    // v2 identity is trusted verbatim ONLY when every new field is present,
    // revision > 0, and protocol/provider/endpoints are known enum strings.
    let trusted = is_trusted_v2_identity(ch);

    let (protocol, provider, native_base_url, native_endpoints, identity_revision, legacy_override) =
        if trusted {
            (
                ch.protocol.clone(),
                ch.provider.clone(),
                ch.native_base_url.clone(),
                ch.native_endpoints.clone(),
                file_rev,
                ch.legacy_executor_override.clone(),
            )
        } else {
            // v1 / degraded: force legacy inference (revision 0, NULL identity)
            // so the resolver re-infers on read.  URL/model/key are untouched.
            let row = ChannelIdentityRow {
                channel_type: legacy_type.clone(),
                base_url: legacy_base.clone(),
                config: config.clone(),
                protocol: None,
                provider: None,
                native_base_url: None,
                native_endpoints: None,
                preset_revision: None,
                identity_revision: 0,
                legacy_executor_override: None,
            };
            let identity = resolve_channel_identity(&row);
            (
                Some(identity.protocol),
                Some(identity.provider),
                Some(identity.native_base_url),
                Some(identity.native_endpoints),
                0,
                identity.legacy_executor_override,
            )
        };

    let endpoints = native_endpoints.map(|eps| {
        eps.into_iter()
            .filter(|e| is_known_endpoint(e))
            .collect::<Vec<_>>()
    });

    ImportChannelInput {
        name: ch.name.clone(),
        channel_type: legacy_type,
        base_url: legacy_base,
        api_key: ch.api_key.clone(),
        models: ch.models.clone(),
        status: ch.status.unwrap_or(1),
        priority: ch.priority.unwrap_or(0),
        weight: ch.weight.unwrap_or(1),
        config,
        model_mapping,
        timeout_secs: ch.timeout_secs.unwrap_or(300),
        protocol,
        provider,
        native_base_url,
        native_endpoints: endpoints,
        preset_revision: if trusted {
            ch.preset_revision.clone()
        } else {
            None
        },
        identity_revision,
        legacy_executor_override: legacy_override,
        last_test_at: ch.last_test_at.clone(),
        last_test_ok: ch.last_test_ok,
    }
}

/// A v2 identity is trusted verbatim only when the new+old combination is
/// coherent: every new field present, revision > 0, and the values are known
/// enum strings.  Anything else falls back to the resolver's legacy inference.
fn is_trusted_v2_identity(ch: &ExportedChannel) -> bool {
    let protocol_ok = ch
        .protocol
        .as_deref()
        .map(is_known_protocol)
        .unwrap_or(false);
    let provider_ok = ch
        .provider
        .as_deref()
        .map(is_known_provider)
        .unwrap_or(false);
    let base_ok = ch
        .native_base_url
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let eps_ok = ch
        .native_endpoints
        .as_ref()
        .map(|v| !v.is_empty() && v.iter().all(|e| is_known_endpoint(e)))
        .unwrap_or(false);
    let rev_ok = ch.identity_revision.unwrap_or(0) > 0;
    protocol_ok && provider_ok && base_ok && eps_ok && rev_ok
}

fn is_known_protocol(s: &str) -> bool {
    matches!(s, "openai" | "anthropic" | "ollama")
}

fn is_known_provider(s: &str) -> bool {
    matches!(
        s,
        "openai"
            | "google"
            | "deepseek"
            | "qwen"
            | "zhipu"
            | "doubao"
            | "doubao_coding_plan"
            | "moonshot"
            | "anthropic"
            | "ollama"
            | "custom"
    )
}

fn is_known_endpoint(s: &str) -> bool {
    matches!(
        s,
        "chat_completions" | "responses" | "messages" | "count_tokens" | "embeddings" | "api_chat"
    )
}

/// Scan local AI CLI tool configs (Claude Code, Codex, Cursor, etc.)
#[tauri::command]
pub async fn scan_local_ai_configs() -> Result<ScanResult, String> {
    let home = std::env::var("WALIAPI_TARGET_HOME")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .ok_or("无法获取用户主目录")?;
    let mut sources: Vec<ScannedSource> = Vec::new();

    // 1. Claude Code: ~/.claude/settings.json
    let claude_settings = home.join(".claude").join("settings.json");
    if claude_settings.exists() {
        match std::fs::read_to_string(&claude_settings) {
            Ok(content) => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(env) = json.get("env").and_then(|v| v.as_object()) {
                        let base_url = env
                            .get("ANTHROPIC_BASE_URL")
                            .and_then(|v| v.as_str())
                            .unwrap_or("https://api.anthropic.com");
                        let api_key = env
                            .get("ANTHROPIC_AUTH_TOKEN")
                            .or_else(|| env.get("ANTHROPIC_API_KEY"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let model = env
                            .get("ANTHROPIC_MODEL")
                            .and_then(|v| v.as_str())
                            .unwrap_or("claude-sonnet-4-20250514");

                        if !api_key.is_empty() {
                            sources.push(ScannedSource {
                                source: "claude-code".to_string(),
                                name: "Claude Code".to_string(),
                                base_url: base_url.to_string(),
                                api_key: api_key.to_string(),
                                models: vec![model.to_string()],
                                api_format: "anthropic".to_string(),
                                raw: json,
                            });
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to read Claude Code settings: {}", e);
            }
        }
    }

    // 2. Codex CLI: ~/.codex/config.toml or ~/.codex/config.json
    let codex_dir = home.join(".codex");
    let codex_json = codex_dir.join("config.json");
    let codex_toml = codex_dir.join("config.toml");

    if codex_json.exists() {
        if let Ok(content) = std::fs::read_to_string(&codex_json) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                let base_url = json
                    .get("base_url")
                    .or_else(|| json.get("baseUrl"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("https://api.openai.com/v1");
                let api_key = json
                    .get("api_key")
                    .or_else(|| json.get("apiKey"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let model = json
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("gpt-4o");

                if !api_key.is_empty() {
                    sources.push(ScannedSource {
                        source: "codex".to_string(),
                        name: "Codex CLI".to_string(),
                        base_url: base_url.to_string(),
                        api_key: api_key.to_string(),
                        models: vec![model.to_string()],
                        api_format: "openai".to_string(),
                        raw: json,
                    });
                }
            }
        }
    } else if codex_toml.exists() {
        if let Ok(content) = std::fs::read_to_string(&codex_toml) {
            // Simple TOML parsing for known fields
            let mut base_url = String::new();
            let mut api_key = String::new();
            let mut model = String::new();

            for line in content.lines() {
                let line = line.trim();
                if let Some(val) = line.strip_prefix("base_url") {
                    base_url = val
                        .trim_start_matches('=')
                        .trim()
                        .trim_matches('"')
                        .to_string();
                } else if let Some(val) = line.strip_prefix("api_key") {
                    api_key = val
                        .trim_start_matches('=')
                        .trim()
                        .trim_matches('"')
                        .to_string();
                } else if let Some(val) = line.strip_prefix("model") {
                    model = val
                        .trim_start_matches('=')
                        .trim()
                        .trim_matches('"')
                        .to_string();
                }
            }

            if !api_key.is_empty() {
                let mut raw_map = serde_json::Map::new();
                raw_map.insert(
                    "base_url".to_string(),
                    serde_json::Value::String(base_url.clone()),
                );
                raw_map.insert(
                    "api_key".to_string(),
                    serde_json::Value::String(api_key.clone()),
                );
                raw_map.insert(
                    "model".to_string(),
                    serde_json::Value::String(model.clone()),
                );

                sources.push(ScannedSource {
                    source: "codex".to_string(),
                    name: "Codex CLI".to_string(),
                    base_url: if base_url.is_empty() {
                        "https://api.openai.com/v1".to_string()
                    } else {
                        base_url
                    },
                    api_key,
                    models: if model.is_empty() {
                        vec!["gpt-4o".to_string()]
                    } else {
                        vec![model]
                    },
                    api_format: "openai".to_string(),
                    raw: serde_json::Value::Object(raw_map),
                });
            }
        }
    }

    // 3. Cursor: ~/.cursor/config or ~/Library/Application Support/Cursor/User/settings.json
    let cursor_settings = [
        home.join(".config")
            .join("Cursor")
            .join("User")
            .join("settings.json"),
        home.join("Library")
            .join("Application Support")
            .join("Cursor")
            .join("User")
            .join("settings.json"),
        home.join("AppData")
            .join("Roaming")
            .join("Cursor")
            .join("User")
            .join("settings.json"),
    ]
    .into_iter()
    .find(|path| path.exists())
    .unwrap_or_else(|| home.join(".config/Cursor/User/settings.json"));
    if cursor_settings.exists() {
        if let Ok(content) = std::fs::read_to_string(&cursor_settings) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                // Cursor may store API keys in various locations
                let base_url = json.pointer("/cursorai.baseUrl").and_then(|v| v.as_str());
                let api_key = json.pointer("/cursorai.apiKey").and_then(|v| v.as_str());

                if let (Some(base_url), Some(api_key)) = (base_url, api_key) {
                    if !api_key.is_empty() {
                        sources.push(ScannedSource {
                            source: "cursor".to_string(),
                            name: "Cursor".to_string(),
                            base_url: base_url.to_string(),
                            api_key: api_key.to_string(),
                            models: vec![],
                            api_format: "openai".to_string(),
                            raw: json,
                        });
                    }
                }
            }
        }
    }

    // 4. OpenAI CLI: ~/.openai/config.json (if exists)
    let openai_config = home.join(".openai").join("config.json");
    if openai_config.exists() {
        if let Ok(content) = std::fs::read_to_string(&openai_config) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                let base_url = json
                    .get("base_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("https://api.openai.com/v1");
                let api_key = json.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
                let model = json
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("gpt-4o");

                if !api_key.is_empty() {
                    sources.push(ScannedSource {
                        source: "openai-cli".to_string(),
                        name: "OpenAI CLI".to_string(),
                        base_url: base_url.to_string(),
                        api_key: api_key.to_string(),
                        models: vec![model.to_string()],
                        api_format: "openai".to_string(),
                        raw: json,
                    });
                }
            }
        }
    }

    Ok(ScanResult { sources })
}

/// Import scanned sources into channels.
///
/// The legacy `type` is derived from the scan hints (unchanged product
/// behavior); rows are written with identity_revision 0 and NULL identity so
/// the unified resolver live-infers (task 09: local scan marks revision 0).
#[tauri::command]
pub async fn import_scanned_sources(
    sources: Vec<ScannedSource>,
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<ImportResult, String> {
    import_scanned_sources_impl(sources, state.inner()).await
}

pub async fn import_scanned_sources_impl(
    sources: Vec<ScannedSource>,
    state: &std::sync::Arc<AppState>,
) -> Result<ImportResult, String> {
    let repo = Repository::new(state.db.pool.clone());
    let existing = repo.get_all_channels().await.map_err(|e| e.to_string())?;
    let existing_names: std::collections::HashSet<String> =
        existing.iter().map(|c| c.name.clone()).collect();

    let mut imported = 0;
    let mut skipped = 0;
    let mut errors: Vec<String> = Vec::new();

    for src in sources {
        let name = src.name.clone();
        if existing_names.contains(&name) {
            skipped += 1;
            continue;
        }

        let channel_type = guess_channel_type(&src.base_url, Some(&src.api_format));

        let input = ImportChannelInput {
            name,
            channel_type,
            base_url: src.base_url,
            api_key: src.api_key,
            models: if src.models.is_empty() {
                vec!["auto".to_string()]
            } else {
                src.models
            },
            status: 1,
            priority: 0,
            weight: 1,
            config: Value::Object(Default::default()),
            model_mapping: Value::Object(Default::default()),
            timeout_secs: 300,
            identity_revision: 0,
            ..Default::default()
        };

        match repo.import_channel(&input).await {
            Ok(_) => imported += 1,
            Err(e) => errors.push(format!("导入扫描源失败: {}", e)),
        }
    }

    Ok(ImportResult {
        imported,
        skipped,
        errors,
    })
}

/// Open a file dialog and return the file content (for import)
#[tauri::command]
pub async fn pick_import_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    #[cfg(not(feature = "desktop-ui"))]
    {
        let _ = &app;
        return Err("文件对话框仅桌面版可用".to_string());
    }
    #[cfg(feature = "desktop-ui")]
    {
        use tauri_plugin_dialog::DialogExt;

        let (tx, rx) = tokio::sync::oneshot::channel();
        app.dialog()
            .file()
            .add_filter("JSON files", &["json"])
            .pick_file(move |file_path| {
                let result = file_path.and_then(|f| {
                    let path = f.into_path().ok()?;
                    std::fs::read_to_string(&path).ok()
                });
                let _ = tx.send(result);
            });

        let result = rx.await.map_err(|_| "对话框取消".to_string())?;
        Ok(result)
    }
}

/// Save a file dialog and return whether save was successful (for export)
#[tauri::command]
pub async fn save_export_file(
    app: tauri::AppHandle,
    content: String,
    default_name: String,
) -> Result<bool, String> {
    #[cfg(not(feature = "desktop-ui"))]
    {
        let _ = (&app, &content, &default_name);
        return Err("文件对话框仅桌面版可用".to_string());
    }
    #[cfg(feature = "desktop-ui")]
    {
        use tauri_plugin_dialog::DialogExt;

        let (tx, rx) = tokio::sync::oneshot::channel();
        app.dialog()
            .file()
            .set_file_name(&default_name)
            .add_filter("JSON files", &["json"])
            .save_file(move |file_path| {
                if let Some(path) = file_path {
                    if let Some(p) = path.as_path() {
                        match std::fs::write(p, &content) {
                            Ok(_) => {
                                let _ = tx.send(true);
                            }
                            Err(e) => {
                                tracing::error!("Failed to save export file: {}", e);
                                let _ = tx.send(false);
                            }
                        }
                        return;
                    }
                }
                let _ = tx.send(false);
            });

        let result = rx.await.map_err(|_| "对话框取消".to_string())?;
        Ok(result)
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Legacy URL/hint -> old `type` mapping.  Used ONLY for the walicode backup
/// and local-scan paths (which do not carry a waliapi v1/v2 identity).  Waliapi
/// v1 import goes through `resolve_channel_identity`, NOT this function.
fn guess_channel_type(base_url: &str, api_format: Option<&str>) -> String {
    let url = base_url.to_lowercase();

    // Check by API format first
    if let Some(fmt) = api_format {
        match fmt {
            "anthropic" => return "claude".to_string(),
            "ollama" => return "ollama".to_string(),
            _ => {}
        }
    }

    // Check by URL
    if url.contains("anthropic.com") {
        return "claude".to_string();
    }
    if url.contains("deepseek.com") {
        return "deepseek".to_string();
    }
    if url.contains("generativelanguage.googleapis.com") || url.contains("gemini") {
        return "gemini".to_string();
    }
    if url.contains("dashscope.aliyuncs.com") {
        return "qwen".to_string();
    }
    if url.contains("bigmodel.cn") {
        return "zhipu".to_string();
    }
    if url.contains("moonshot.cn") || url.contains("kimi") {
        return "moonshot".to_string();
    }
    if url.contains("volces.com") {
        return "doubao".to_string();
    }
    if url.contains("localhost:11434") || url.contains("/api/chat") {
        return "ollama".to_string();
    }

    "custom".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;

    /// In-memory SQLite with the full migration set applied.
    async fn test_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");
        pool
    }

    /// A channel written by the new dual-write path (revision > 0) carrying an
    /// unknown config key and an ARRAY model mapping — the two round-trip
    /// hazards the contract calls out (design 11.4).
    fn v2_channel_fixture() -> Channel {
        Channel {
            id: "ch-1".into(),
            name: "Anthropic-DS".into(),
            channel_type: "claude".into(),
            base_url: "https://api.deepseek.com/anthropic/v1".into(),
            api_key: "sk-secret-1234567890".into(),
            models: serde_json::to_string(&["m1".to_string(), "m2".to_string()]).unwrap(),
            status: 0, // disabled — must survive the round-trip
            priority: 7,
            weight: 3,
            config: json!({ "custom_unknown_key": "keep-me", "legacy_capabilities": ["responses_via_chat_v1"] })
                .to_string(),
            model_mapping: json!({ "alias": ["up-a", "up-b", "up-c"] }).to_string(),
            timeout_secs: 123, // non-default — must survive
            protocol: Some("anthropic".into()),
            provider: Some("deepseek".into()),
            native_base_url: Some("https://api.deepseek.com/anthropic/v1".into()),
            native_endpoints: Some("[\"messages\"]".into()),
            preset_revision: Some("2026-08-04".into()),
            identity_revision: 1,
            legacy_executor_override: None,
            created_at: "2026-08-01T00:00:00.000Z".into(),
            updated_at: "2026-08-01T00:00:00.000Z".into(),
            last_test_at: Some("2026-08-01T00:00:00.000Z".into()),
            last_test_ok: Some(1),
        }
    }

    /// A legacy v1 row (identity_revision 0, NULL identity fields) that the
    /// resolver must infer at read time.
    fn v1_channel_fixture() -> Channel {
        Channel {
            id: "ch-2".into(),
            name: "Legacy-OpenAI".into(),
            channel_type: "openai".into(),
            base_url: "https://gw.example.com/v1".into(),
            api_key: "sk-legacy-0987654321".into(),
            models: serde_json::to_string(&["gpt-4o".to_string()]).unwrap(),
            status: 1,
            priority: 5,
            weight: 2,
            config: json!({ "preserve": true, "custom_unknown_key": "x" }).to_string(),
            model_mapping: json!({ "auto": ["a", "b"] }).to_string(),
            timeout_secs: 45,
            protocol: None,
            provider: None,
            native_base_url: None,
            native_endpoints: None,
            preset_revision: None,
            identity_revision: 0,
            legacy_executor_override: None,
            created_at: "2026-07-01T00:00:00.000Z".into(),
            updated_at: "2026-07-01T00:00:00.000Z".into(),
            last_test_at: None,
            last_test_ok: None,
        }
    }

    fn assert_business_fields_equal(written: &Channel, expected: &Channel) {
        assert_eq!(written.name, expected.name, "name");
        assert_eq!(written.channel_type, expected.channel_type, "type");
        assert_eq!(written.base_url, expected.base_url, "base_url");
        assert_eq!(written.api_key, expected.api_key, "api_key");
        assert_eq!(
            written.status, expected.status,
            "status (must not be reset to 1)"
        );
        assert_eq!(written.priority, expected.priority, "priority");
        assert_eq!(written.weight, expected.weight, "weight");
        assert_eq!(written.timeout_secs, expected.timeout_secs, "timeout_secs");
        let written_models: Vec<String> = serde_json::from_str(&written.models).unwrap();
        let expected_models: Vec<String> = serde_json::from_str(&expected.models).unwrap();
        assert_eq!(written_models, expected_models, "models");
        assert_eq!(written.last_test_at, expected.last_test_at, "last_test_at");
        assert_eq!(written.last_test_ok, expected.last_test_ok, "last_test_ok");
    }

    /// config unknown keys + array model_mapping survive verbatim (per-field).
    fn assert_config_and_mapping_preserved(written: &Channel, expected: &Channel) {
        let wcfg: Value = serde_json::from_str(&written.config).unwrap();
        let ecfg: Value = serde_json::from_str(&expected.config).unwrap();
        assert_eq!(wcfg, ecfg, "config must round-trip including unknown keys");
        let wmm: Value = serde_json::from_str(&written.model_mapping).unwrap();
        let emm: Value = serde_json::from_str(&expected.model_mapping).unwrap();
        assert_eq!(wmm, emm, "model_mapping must round-trip including arrays");
        // Explicit assertion that an unknown config key and an array mapping
        // survive (the round-trip contract's named hazards).
        assert_eq!(wcfg["custom_unknown_key"], ecfg["custom_unknown_key"]);
        if let Some(arr) = emm.as_object().and_then(|o| o.values().next()) {
            if arr.is_array() {
                assert!(wmm
                    .as_object()
                    .and_then(|o| o.values().next())
                    .unwrap()
                    .is_array());
            }
        }
    }

    /// v2 channel -> export -> import -> DB, per-field equal, identity trusted
    /// verbatim (revision preserved).
    #[tokio::test]
    async fn round_trip_v2_preserves_every_field_and_identity() {
        let pool = test_pool().await;
        let repo = Repository::new(pool);
        let c = v2_channel_fixture();

        let exported = ExportedChannel::from(c.clone());
        // The v2 export carries BOTH new identity and legacy compat fields.
        assert_eq!(exported.protocol.as_deref(), Some("anthropic"));
        assert_eq!(exported.provider.as_deref(), Some("deepseek"));
        assert_eq!(
            exported.native_base_url.as_deref(),
            Some("https://api.deepseek.com/anthropic/v1")
        );
        assert_eq!(
            exported.native_endpoints.as_deref(),
            Some(&["messages".to_string()][..])
        );
        assert_eq!(exported.identity_revision, Some(1));
        assert_eq!(exported.channel_type, "claude");
        assert_eq!(exported.base_url, "https://api.deepseek.com/anthropic/v1");

        let input = exported_channel_to_import(&exported);
        assert_eq!(input.identity_revision, 1, "v2 identity must be trusted");
        let written = repo.import_channel(&input).await.unwrap();

        assert_business_fields_equal(&written, &c);
        assert_config_and_mapping_preserved(&written, &c);
        assert_eq!(written.protocol.as_deref(), Some("anthropic"));
        assert_eq!(written.provider.as_deref(), Some("deepseek"));
        assert_eq!(
            written.native_base_url.as_deref(),
            Some("https://api.deepseek.com/anthropic/v1")
        );
        assert_eq!(written.identity_revision, 1);
    }

    /// v1 channel -> export (identity inferred, revision 0) -> import -> DB.
    /// status/timeout (the historical v1-import bugs) must survive.
    #[tokio::test]
    async fn round_trip_v1_preserves_status_and_timeout_and_unknown_keys() {
        let pool = test_pool().await;
        let repo = Repository::new(pool);
        let c = v1_channel_fixture();

        let exported = ExportedChannel::from(c.clone());
        // v1 row is resolved (not URL-guessed): openai/custom for the gateway.
        assert_eq!(exported.protocol.as_deref(), Some("openai"));
        assert_eq!(exported.provider.as_deref(), Some("custom"));
        assert_eq!(exported.identity_revision, Some(0));

        let input = exported_channel_to_import(&exported);
        // The resolver re-infers on read, so revision stays 0 and the identity
        // columns are the inferred values.
        assert_eq!(input.identity_revision, 0);
        assert_eq!(input.status, 1);
        assert_eq!(input.timeout_secs, 45);
        let written = repo.import_channel(&input).await.unwrap();

        assert_business_fields_equal(&written, &c);
        assert_eq!(written.timeout_secs, 45, "v1 import must keep timeout");
        assert_eq!(written.status, 1, "v1 import must keep status");
        assert_eq!(written.priority, 5);
        assert_eq!(written.weight, 2);
        assert_config_and_mapping_preserved(&written, &c);
        // The resolver re-infers openai/custom at read time.
        let identity = resolve_channel_identity(&ChannelIdentityRow::from(&written));
        assert_eq!(identity.protocol, "openai");
        assert_eq!(identity.provider, "custom");
        assert_eq!(identity.native_base_url, "https://gw.example.com/v1");
        assert_eq!(identity.identity_revision, 0);
    }

    /// v1 import with a status=0 (disabled) channel must NOT be force-enabled.
    #[tokio::test]
    async fn round_trip_v1_keeps_disabled_status() {
        let pool = test_pool().await;
        let repo = Repository::new(pool);
        let mut c = v1_channel_fixture();
        c.status = 0;

        let exported = ExportedChannel::from(c.clone());
        let input = exported_channel_to_import(&exported);
        assert_eq!(input.status, 0, "import must not reset disabled -> enabled");
        let written = repo.import_channel(&input).await.unwrap();
        assert_eq!(written.status, 0);
    }

    /// A v2 file whose identity does NOT validate (unknown protocol) degrades to
    /// legacy/custom without losing URL/model/key.
    #[test]
    fn unknown_v2_protocol_degrades_to_legacy_without_data_loss() {
        let mut exported = ExportedChannel::from(v2_channel_fixture());
        exported.protocol = Some("gemini".to_string()); // not a known protocol
        assert!(!is_trusted_v2_identity(&exported));

        let input = exported_channel_to_import(&exported);
        // URL / key / models / mapping / status / timeout all preserved.
        assert_eq!(input.base_url, "https://api.deepseek.com/anthropic/v1");
        assert_eq!(input.api_key, "sk-secret-1234567890");
        assert_eq!(input.models, vec!["m1".to_string(), "m2".to_string()]);
        assert_eq!(input.status, 0);
        assert_eq!(input.timeout_secs, 123);
        assert_eq!(
            input.model_mapping["alias"],
            json!(["up-a", "up-b", "up-c"])
        );
        assert_eq!(input.config["custom_unknown_key"], "keep-me");
        // Degraded to a re-inferred identity, revision 0.
        assert_eq!(input.identity_revision, 0);
        // channel_type=claude + deepseek anthropic URL（带 /v1）-> anthropic/deepseek。
        assert_eq!(input.protocol.as_deref(), Some("anthropic"));
        assert_eq!(input.provider.as_deref(), Some("deepseek"));
        assert_eq!(
            input.native_base_url.as_deref(),
            Some("https://api.deepseek.com/anthropic/v1")
        );
    }

    /// A v1 file (new identity fields absent) goes through the resolver, not the
    /// URL-guessing inference: an "openai"-typed row with a private gateway host
    /// resolves to provider=custom (never a fabricated vendor).
    #[test]
    fn v1_missing_identity_fields_uses_resolver_inference() {
        let mut exported = ExportedChannel::from(v1_channel_fixture());
        exported.protocol = None;
        exported.provider = None;
        exported.native_base_url = None;
        exported.native_endpoints = None;
        exported.identity_revision = None;
        assert!(!is_trusted_v2_identity(&exported));

        let input = exported_channel_to_import(&exported);
        assert_eq!(input.identity_revision, 0);
        assert_eq!(input.protocol.as_deref(), Some("openai"));
        assert_eq!(input.provider.as_deref(), Some("custom"));
        assert_eq!(
            input.native_base_url.as_deref(),
            Some("https://gw.example.com/v1")
        );
        assert_eq!(
            input.native_endpoints.as_deref(),
            Some(&["chat_completions".to_string()][..])
        );
    }

    /// Unknown v2 provider/endpoint values are filtered/degraded the same way.
    #[test]
    fn unknown_provider_or_endpoint_degrades() {
        let mut exported = ExportedChannel::from(v2_channel_fixture());
        exported.provider = Some("not-a-provider".into());
        assert!(!is_trusted_v2_identity(&exported));
        let input = exported_channel_to_import(&exported);
        // Resolver maps the claude/deepseek URL to deepseek; no data loss.
        assert_eq!(input.provider.as_deref(), Some("deepseek"));
        assert_eq!(input.api_key, "sk-secret-1234567890");

        // An unknown endpoint inside an otherwise-coherent v2 identity makes
        // the WHOLE identity untrusted (design: "不信任未知 endpoint，降为
        // legacy/custom") — the row degrades to a resolver re-inference with
        // revision 0, never losing URL/model/key.
        let mut exported2 = ExportedChannel::from(v2_channel_fixture());
        exported2
            .native_endpoints
            .as_mut()
            .unwrap()
            .push("fancy_endpoint".to_string());
        assert!(
            !is_trusted_v2_identity(&exported2),
            "unknown endpoint must invalidate identity"
        );
        let input2 = exported_channel_to_import(&exported2);
        assert_eq!(
            input2.identity_revision, 0,
            "degraded to resolver re-inference"
        );
        assert_eq!(input2.api_key, "sk-secret-1234567890", "key preserved");
        assert_eq!(
            input2.base_url, "https://api.deepseek.com/anthropic/v1",
            "URL preserved"
        );
        // Resolver maps the claude/deepseek URL to anthropic/deepseek.
        assert_eq!(input2.protocol.as_deref(), Some("anthropic"));
        assert_eq!(input2.provider.as_deref(), Some("deepseek"));
        // T06 I-4: legacy claude infers [messages, count_tokens].
        assert_eq!(
            input2.native_endpoints.as_deref(),
            Some(&["messages".to_string(), "count_tokens".to_string()][..])
        );
    }

    /// The export file itself round-trips through the JSON serialization that a
    /// user would write to disk and re-import.
    #[tokio::test]
    async fn json_file_round_trip_v1_and_v2() {
        let pool = test_pool().await;
        let repo = Repository::new(pool);

        let v2 = v2_channel_fixture();
        let v1 = v1_channel_fixture();
        let export = WaliapiExport {
            version: "2.0".to_string(),
            exported_at: "2026-08-05T00:00:00.000Z".to_string(),
            r#type: "waliapi-export".to_string(),
            channels: vec![
                ExportedChannel::from(v2.clone()),
                ExportedChannel::from(v1.clone()),
            ],
        };
        let file = serde_json::to_string_pretty(&export).unwrap();

        // Parse as an incoming import (both v1 and v2 channels in one file).
        let parsed: WaliapiExport = serde_json::from_str(&file).unwrap();
        assert_eq!(parsed.version, "2.0");
        let first = exported_channel_to_import(&parsed.channels[0]);
        let second = exported_channel_to_import(&parsed.channels[1]);
        let w2 = repo.import_channel(&first).await.unwrap();
        let w1 = repo.import_channel(&second).await.unwrap();

        assert_business_fields_equal(&w2, &v2);
        assert_business_fields_equal(&w1, &v1);
        assert_eq!(w2.identity_revision, 1);
        assert_eq!(w1.identity_revision, 0);
        // No raw secret leaks into any diagnostic representation of the export
        // *except* the explicit plaintext api_key field (product semantics).
        assert_eq!(w2.api_key, "sk-secret-1234567890");
    }
}
