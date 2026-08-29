use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

use crate::settings_store::SettingsStore;
use crate::AppState;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Settings {
    #[serde(default = "default_port")]
    pub server_port: u16,
    #[serde(default = "default_host")]
    pub server_host: String,
    #[serde(default = "default_theme")]
    pub ui_theme: String,
    #[serde(default = "default_language")]
    pub ui_language: String,
    #[serde(default = "default_true")]
    pub minimize_to_tray: bool,
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    #[serde(default = "default_false")]
    pub auto_start: bool,
    #[serde(default = "default_retry_enabled")]
    pub retry_enabled: bool,
    #[serde(default = "default_retry_times")]
    pub retry_times: i32,
    #[serde(default = "default_security_enabled")]
    pub security_enabled: bool,
    #[serde(default = "default_security_mode")]
    pub security_mode: String,
    #[serde(default = "default_true")]
    pub security_scan_unicode: bool,
    #[serde(default = "default_true")]
    pub security_scan_tools: bool,
    #[serde(default = "default_true")]
    pub security_scan_network: bool,
    #[serde(default = "default_false")]
    pub security_scan_response: bool,
    #[serde(default = "default_false")]
    pub security_redact_secrets: bool,
    #[serde(default = "default_false")]
    pub security_block_on_critical: bool,
    #[serde(default = "default_true")]
    pub routing_prefer_auth_accounts: bool,
    #[serde(default = "default_true")]
    pub routing_prefer_same_protocol: bool,
}

fn default_port() -> u16 {
    8777
}
fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_theme() -> String {
    "dark".to_string()
}
fn default_language() -> String {
    "zh-CN".to_string()
}
fn default_true() -> bool {
    true
}
fn default_false() -> bool {
    false
}
fn default_retry_enabled() -> bool {
    true
}
fn default_retry_times() -> i32 {
    2
}
fn default_security_enabled() -> bool {
    false
}
fn default_security_mode() -> String {
    "audit".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            server_port: default_port(),
            server_host: default_host(),
            ui_theme: default_theme(),
            ui_language: default_language(),
            minimize_to_tray: default_true(),
            close_to_tray: default_true(),
            auto_start: default_false(),
            retry_enabled: default_retry_enabled(),
            retry_times: default_retry_times(),
            security_enabled: default_security_enabled(),
            security_mode: default_security_mode(),
            security_scan_unicode: default_false(),
            security_scan_tools: default_false(),
            security_scan_network: default_false(),
            security_scan_response: default_false(),
            security_redact_secrets: default_false(),
            security_block_on_critical: default_false(),
            routing_prefer_auth_accounts: default_true(),
            routing_prefer_same_protocol: default_true(),
        }
    }
}

fn get_str(store: &SettingsStore, key: &str, default: &str) -> String {
    store.get_str(key, default)
}

fn get_u64(store: &SettingsStore, key: &str, default: u64) -> u64 {
    store.get_u64(key, default)
}

fn get_bool(store: &SettingsStore, key: &str, default: bool) -> bool {
    store.get_bool(key, default)
}

/// Feature-flag snapshot exposed to the UI (T00 decision 9 / T10 rollout).
///
/// The frontend uses these to disable/hide protocol tabs whose backend path is
/// not yet enabled (e.g. Ollama when `ollama_native` is OFF) so a user never
/// creates a channel that 503s at runtime.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FeatureFlagsDto {
    pub new_routeplan: bool,
    pub cross_protocol_codec: bool,
    pub native_responses: bool,
    pub ollama_native: bool,
    pub prefer_auth_accounts: bool,
    pub prefer_same_protocol: bool,
}

#[tauri::command]
pub fn get_feature_flags(state: tauri::State<'_, Arc<AppState>>) -> Result<FeatureFlagsDto, String> {
    let f = crate::core::feature_flags::read_feature_flags(&state.settings);
    Ok(FeatureFlagsDto {
        new_routeplan: f.new_routeplan,
        cross_protocol_codec: f.cross_protocol_codec,
        native_responses: f.native_responses,
        ollama_native: f.ollama_native,
        prefer_auth_accounts: f.prefer_auth_accounts,
        prefer_same_protocol: f.prefer_same_protocol,
    })
}

#[tauri::command]
pub async fn get_settings(state: tauri::State<'_, Arc<AppState>>) -> Result<Settings, String> {
    let store = &state.settings;
    let settings = Settings {
        server_port: get_u64(store, "server.port", 8777) as u16,
        server_host: get_str(store, "server.host", "127.0.0.1"),
        ui_theme: get_str(store, "ui.theme", "dark"),
        ui_language: get_str(store, "ui.language", "zh-CN"),
        minimize_to_tray: get_bool(store, "general.minimize_to_tray", true),
        close_to_tray: get_bool(store, "general.close_to_tray", true),
        auto_start: get_bool(store, "general.auto_start", false),
        retry_enabled: get_bool(store, "retry.enabled", true),
        retry_times: get_u64(store, "retry.times", 2) as i32,
        security_enabled: get_bool(store, "security.enabled", false),
        security_mode: get_str(store, "security.mode", "audit"),
        security_scan_unicode: get_bool(store, "security.scan_unicode", false),
        security_scan_tools: get_bool(store, "security.scan_tools", false),
        security_scan_network: get_bool(store, "security.scan_network", false),
        security_scan_response: get_bool(store, "security.scan_response", false),
        security_redact_secrets: get_bool(store, "security.redact_secrets", false),
        security_block_on_critical: get_bool(store, "security.block_on_critical", false),
        routing_prefer_auth_accounts: get_bool(store, "routing.prefer_auth_accounts", true),
        routing_prefer_same_protocol: get_bool(store, "routing.prefer_same_protocol", true),
    };
    Ok(settings)
}

#[tauri::command]
pub async fn save_settings(
    settings: Settings,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state.settings.set_many(&[
        ("server.port".to_string(), serde_json::json!(settings.server_port)),
        ("server.host".to_string(), serde_json::json!(settings.server_host)),
        ("ui.theme".to_string(), serde_json::json!(settings.ui_theme)),
        ("ui.language".to_string(), serde_json::json!(settings.ui_language)),
        ("general.minimize_to_tray".to_string(), serde_json::json!(settings.minimize_to_tray)),
        ("general.close_to_tray".to_string(), serde_json::json!(settings.close_to_tray)),
        ("general.auto_start".to_string(), serde_json::json!(settings.auto_start)),
        ("retry.enabled".to_string(), serde_json::json!(settings.retry_enabled)),
        ("retry.times".to_string(), serde_json::json!(settings.retry_times)),
        ("security.enabled".to_string(), serde_json::json!(settings.security_enabled)),
        ("security.mode".to_string(), serde_json::json!(settings.security_mode)),
        ("security.scan_unicode".to_string(), serde_json::json!(settings.security_scan_unicode)),
        ("security.scan_tools".to_string(), serde_json::json!(settings.security_scan_tools)),
        ("security.scan_network".to_string(), serde_json::json!(settings.security_scan_network)),
        ("security.scan_response".to_string(), serde_json::json!(settings.security_scan_response)),
        ("security.redact_secrets".to_string(), serde_json::json!(settings.security_redact_secrets)),
        ("security.block_on_critical".to_string(), serde_json::json!(settings.security_block_on_critical)),
        ("routing.prefer_auth_accounts".to_string(), serde_json::json!(settings.routing_prefer_auth_accounts)),
        ("routing.prefer_same_protocol".to_string(), serde_json::json!(settings.routing_prefer_same_protocol)),
    ])?;
    Ok(())
}

#[tauri::command]
pub async fn apply_theme(
    theme: String,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state
        .events
        .emit("theme-changed", serde_json::json!({ "theme": theme }));
    Ok(())
}

#[tauri::command]
pub async fn set_auto_start(enabled: bool, app: AppHandle) -> Result<(), String> {
    #[cfg(not(feature = "desktop-ui"))]
    {
        let _ = (enabled, &app);
        return Ok(());
    }
    #[cfg(feature = "desktop-ui")]
    {
        let autostart = app.autolaunch();
        if enabled {
            autostart.enable().map_err(|e| e.to_string())?;
        } else {
            autostart.disable().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
