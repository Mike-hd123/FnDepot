use super::handlers::*;
use crate::AppState;
use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

#[cfg(feature = "desktop-ui")]
use tauri::{AppHandle, Manager};
#[cfg(not(feature = "desktop-ui"))]
use tauri::Manager;

/// 构建 axum 路由。`desktop_app` 仅桌面端传入（对话框等桌面专属命令需要真实 AppHandle）；
/// headless 传 None，此时通过 tauri MockRuntime 获得 `State<'static, Arc<AppState>>` 供命令分发。
///
/// 注意：任何对 `AppHandle<Wry>` 的方法调用都会把整套 wry/webkit 链接进二进制，
/// 因此 desktop 分支代码全部用 `desktop-ui` feature 门控。
#[cfg(feature = "desktop-ui")]
pub fn create_router(state: Arc<AppState>, desktop_app: Option<AppHandle>) -> Router {
    // 桌面端：泄漏真实 AppHandle 换取 'static 引用（App 生命周期即进程生命周期）。
    let (state_static, desktop_app_static) = match desktop_app {
        Some(app) => {
            let leaked: &'static AppHandle = Box::leak(Box::new(app));
            (leaked.state::<Arc<AppState>>(), Some(leaked))
        }
        None => (mock_state_handle(&state), None),
    };

    let shared = SharedState {
        state: state.clone(),
        state_static,
        desktop_app: desktop_app_static,
    };

    build_router(state, shared)
}

/// headless（无 desktop-ui）：仅 mock State，无任何 AppHandle<Wry> 引用。
#[cfg(not(feature = "desktop-ui"))]
pub fn create_router(state: Arc<AppState>) -> Router {
    let shared = SharedState {
        state: state.clone(),
        state_static: mock_state_handle(&state),
    };

    build_router(state, shared)
}

/// 通过 MockRuntime 获得 `State<'static, Arc<AppState>>`（无窗口/事件循环，仅作状态容器）。
fn mock_state_handle(state: &Arc<AppState>) -> tauri::State<'static, Arc<AppState>> {
    let mock = Box::leak(Box::new(tauri::test::mock_app()));
    mock.manage(state.clone());
    let handle: &'static tauri::AppHandle<tauri::test::MockRuntime> =
        Box::leak(Box::new(mock.handle().clone()));
    handle.state::<Arc<AppState>>()
}

fn build_router(state: Arc<AppState>, shared: SharedState) -> Router {
    // 宽松 CORS 仅作用于 LLM 网关与服务 API（API Key 鉴权、供跨域调用）；
    // /admin/api 与静态资源不附带 CORS 头（仅同源可用，配合 CSRF 中间件）。
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers(Any);

    // Service registry — merge all service routes
    let registry = crate::services::ServiceRegistry::new();
    let service_router = registry.merge_routes(state.clone());

    // Web 管理面板（/admin/api/*，自带会话鉴权 + CSRF 防护）
    let admin = super::admin_routes::router(shared.clone());

    let gateway_router = Router::new()
        // OpenAI Chat Completions
        .route("/v1/chat/completions", post(handle_chat_completions))
        // OpenAI Completions (legacy)
        .route("/v1/completions", post(handle_completions))
        // OpenAI Responses API
        .route("/v1/responses", post(handle_responses))
        // OpenAI Embeddings
        .route("/v1/embeddings", post(handle_embeddings))
        // OpenAI Models
        .route("/v1/models", get(handle_list_models))
        // OpenAI Images
        .route("/v1/images/generations", post(handle_images))
        // OpenAI Audio
        .route(
            "/v1/audio/transcriptions",
            post(handle_audio_transcriptions),
        )
        .route("/v1/audio/speech", post(handle_audio_speech))
        // Anthropic Messages API
        .route(
            "/v1/messages",
            post(handle_messages).layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .route(
            "/v1/messages/count_tokens",
            post(handle_messages_count_tokens).layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        // Health check
        .route("/health", get(handle_health))
        // Service routes (Knowledge Base, MCP, etc.)
        .merge(service_router)
        .layer(cors);

    Router::new()
        .merge(gateway_router)
        // Web 管理面板 API（无宽松 CORS）
        .nest("/admin/api", admin)
        // 内嵌 Web 静态资源（SPA fallback，须放在所有 API 路由之后）
        .merge(super::static_assets::static_router())
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024))
        .with_state(shared)
}

#[derive(Clone)]
pub struct SharedState {
    pub state: Arc<AppState>,
    /// 供 /admin/api/invoke 分发用的 'static State（桌面：真实 handle；headless：mock handle）。
    pub state_static: tauri::State<'static, Arc<AppState>>,
    /// 桌面端真实 AppHandle（文件对话框、自动启动等桌面专属命令）；headless 编译期不存在该字段。
    #[cfg(feature = "desktop-ui")]
    pub desktop_app: Option<&'static AppHandle>,
}
