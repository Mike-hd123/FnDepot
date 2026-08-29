//! waliapi-web headless 启动流程：不创建 Tauri 窗口/事件循环，
//! 直接初始化数据库与服务状态并启动内嵌 HTTP 服务（网关 + Web 管理面板）。

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::db;
use crate::server;
use crate::settings_store;
use crate::AppState;

#[derive(Debug, Clone)]
pub struct WebServerConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub data_dir: PathBuf,
}

/// 解析 headless 数据目录：显式参数 > WALIAPI_DATA_DIR > 平台默认
/// （与桌面端 app_data_dir 一致，保证 Docker 卷数据延续）。
pub fn resolve_data_dir(explicit: Option<String>) -> PathBuf {
    if let Some(dir) = explicit {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Ok(dir) = std::env::var("WALIAPI_DATA_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    // 与 tauri app_data_dir 对齐：$XDG_DATA_HOME/<identifier>（Docker 中即 /data/<identifier>）
    let base = std::env::var("XDG_DATA_HOME").ok().filter(|v| !v.trim().is_empty());
    if let Some(base) = base {
        return PathBuf::from(base).join(crate::APP_IDENTIFIER);
    }
    platform_default_data_dir()
}

#[cfg(target_os = "windows")]
fn platform_default_data_dir() -> PathBuf {
    std::env::var("APPDATA")
        .map(|v| PathBuf::from(v).join(crate::APP_IDENTIFIER))
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(target_os = "macos")]
fn platform_default_data_dir() -> PathBuf {
    std::env::var("HOME")
        .map(|v| {
            PathBuf::from(v)
                .join("Library/Application Support")
                .join(crate::APP_IDENTIFIER)
        })
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn platform_default_data_dir() -> PathBuf {
    std::env::var("HOME")
        .map(|v| PathBuf::from(v).join(".local/share").join(crate::APP_IDENTIFIER))
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub async fn run(cfg: WebServerConfig) -> Result<(), String> {
    // CLI 参数提升为环境变量，沿用 start_server 的 env > settings > default 优先级
    if let Some(host) = &cfg.host {
        std::env::set_var("WALIAPI_SERVER_HOST", host);
    }
    if let Some(port) = &cfg.port {
        std::env::set_var("WALIAPI_SERVER_PORT", port.to_string());
    }

    let data_dir = cfg.data_dir;
    std::fs::create_dir_all(&data_dir).map_err(|e| format!("创建数据目录失败: {e}"))?;

    let db = db::Database::new_with_path(&data_dir).await;
    let db = Arc::new(db);
    server::admin_auth::ensure_initial_admin(&db.pool, &data_dir).await?;

    let auth_service = Arc::new(crate::auth_provider::service::AuthService::new(
        Arc::new(db::repository::Repository::new(db.pool.clone())),
        crate::auth_provider::ProviderRegistry::new(),
    ));
    let (event_tx, _) =
        tokio::sync::broadcast::channel(server::event_bridge::EVENT_CHANNEL_CAPACITY);

    let state = Arc::new(AppState {
        db,
        auth_service: auth_service.clone(),
        login_sessions: Arc::new(crate::commands::auth::LoginSessions::new()),
        server_port: Arc::new(RwLock::new(0)),
        server_running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        server_handle: Arc::new(RwLock::new(None)),
        test_receipts: Arc::new(crate::services::channel_test::TestReceiptStore::new(
            std::time::Duration::from_secs(30 * 60),
        )),
        admin_sessions: server::admin_auth::SessionStore::new(),
        events: server::event_bridge::EventSink::headless(event_tx),
        settings: settings_store::SettingsStore::file(
            settings_store::default_settings_path(&data_dir),
        ),
        data_dir,
    });

    tauri::async_runtime::spawn(crate::auth_provider::maintenance::run_maintenance_loop(
        auth_service,
    ));

    let state_clone = state.clone();
    let handle = tauri::async_runtime::spawn(async move {
        if let Err(e) = server::start_server(state_clone, None).await {
            log::error!("服务启动失败: {e}");
        }
    });
    *state.server_handle.write().await = Some(handle);

    // 等待终止信号
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("收到退出信号，正在关闭…");
    Ok(())
}
