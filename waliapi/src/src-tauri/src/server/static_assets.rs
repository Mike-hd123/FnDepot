//! 内嵌 Web 管理面板静态资源（`embed-web` feature）。
//!
//! 启用 `embed-web` 时把 `web/dist` 编译进二进制并以 SPA fallback 方式提供；
//! 未启用时所有路径返回 404（桌面开发模式不受影响）。

use axum::{
    extract::Request,
    http::StatusCode,
    response::{IntoResponse, Response},
    Router,
};

#[cfg(feature = "embed-web")]
use axum::{body::Body, http::header};

use super::router::SharedState;

#[cfg(feature = "embed-web")]
#[derive(rust_embed::RustEmbed)]
#[folder = "../web/dist"]
struct WebAssets;

pub fn static_router() -> Router<SharedState> {
    Router::new().fallback(serve_embedded)
}

async fn serve_embedded(req: Request) -> Response {
    #[cfg(feature = "embed-web")]
    {
        let path = req.uri().path().trim_start_matches('/');
        // SPA fallback：未匹配到文件时返回 index.html（MIME 也按 index.html 计算）
        let (served_path, asset) = match WebAssets::get(path) {
            Some(content) => (path.to_string(), Some(content)),
            None => ("index.html".to_string(), WebAssets::get("index.html")),
        };
        return match asset {
            Some(content) => {
                let mime = mime_guess::from_path(&served_path).first_or_octet_stream();
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, mime.as_ref())
                    .body(Body::from(content.data.into_owned()))
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
            }
            None => StatusCode::NOT_FOUND.into_response(),
        };
    }
    #[cfg(not(feature = "embed-web"))]
    {
        let _ = req;
        StatusCode::NOT_FOUND.into_response()
    }
}
