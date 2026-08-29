pub mod admin_auth;
pub mod admin_routes;
pub mod event_bridge;
pub mod handlers;
pub mod router;
pub mod static_assets;

use crate::settings_store::SettingsStore;
use crate::AppState;
use std::sync::Arc;
use tauri::AppHandle;

/// 启动内嵌 HTTP 服务（LLM 网关 + Web 管理面板）。
/// `desktop_app` 仅桌面端传入（用于对话框等桌面专属命令）；headless 传 None。
pub async fn start_server(
    state: Arc<AppState>,
    desktop_app: Option<AppHandle>,
) -> Result<(), anyhow::Error> {
    let host = get_server_host(&state.settings);
    let port = get_server_port(&state.settings);

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let local_addr = listener.local_addr()?;
    let actual_port = local_addr.port();

    *state.server_port.write().await = actual_port;
    state
        .server_running
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let router = {
        #[cfg(feature = "desktop-ui")]
        {
            router::create_router(state.clone(), desktop_app)
        }
        #[cfg(not(feature = "desktop-ui"))]
        {
            let _ = &desktop_app; // headless 不使用 AppHandle
            router::create_router(state.clone())
        }
    };

    state.events.emit(
        "server-started",
        serde_json::json!({
            "port": actual_port,
            "url": format!("http://{}:{}", host, actual_port)
        }),
    );

    tracing::info!(
        "WaLiAPI server listening on http://{}:{}",
        host,
        actual_port
    );

    axum::serve(listener, router).await?;

    state
        .server_running
        .store(false, std::sync::atomic::Ordering::SeqCst);

    Ok(())
}

fn get_server_host(settings: &SettingsStore) -> String {
    if let Ok(host) = std::env::var("WALIAPI_SERVER_HOST") {
        let trimmed = host.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    let host = settings.get_str("server.host", "");
    if !host.trim().is_empty() {
        return host.trim().to_string();
    }
    "127.0.0.1".to_string()
}

fn get_server_port(settings: &SettingsStore) -> u16 {
    if let Ok(port) = std::env::var("WALIAPI_SERVER_PORT") {
        if let Ok(value) = port.trim().parse::<u16>() {
            if value != 0 {
                return value;
            }
        }
    }

    let port = settings.get_u64("server.port", 8777) as u16;
    if port == 0 {
        return 8777;
    }
    port
}
