use super::handlers;
use crate::server::router::SharedState;
use crate::AppState;
use axum::routing::{delete, get, post};
use axum::Router;
use std::sync::Arc;

pub fn create_router(_state: Arc<AppState>) -> Router<SharedState> {
    Router::new()
        // ── Project management ──
        .route(
            "/api/wiki/projects",
            get(handlers::list_projects).post(handlers::create_project),
        )
        .route(
            "/api/wiki/projects/{id}",
            get(handlers::get_project)
                .put(handlers::update_project)
                .delete(handlers::delete_project),
        )
        .route(
            "/api/wiki/projects/{id}/stats",
            get(handlers::get_project_stats),
        )
        // ── Sources ──
        .route(
            "/api/wiki/projects/{id}/sources",
            get(handlers::list_sources).post(handlers::add_source),
        )
        .route(
            "/api/wiki/projects/{id}/sources/{sid}",
            delete(handlers::delete_source),
        )
        .route(
            "/api/wiki/projects/{id}/sources/{sid}/ingest",
            post(handlers::ingest_source),
        )
        .route(
            "/api/wiki/projects/{id}/rescan",
            post(handlers::rescan_sources),
        )
        // ── Wiki pages ──
        .route("/api/wiki/projects/{id}/pages", get(handlers::list_pages))
        .route(
            "/api/wiki/projects/{id}/pages/{*path}",
            get(handlers::get_page)
                .put(handlers::update_page)
                .delete(handlers::delete_page),
        )
        // ── Search & Ask ──
        .route("/api/wiki/projects/{id}/search", get(handlers::search))
        .route("/api/wiki/projects/{id}/ask", post(handlers::ask))
        // ── Graph ──
        .route("/api/wiki/projects/{id}/graph", get(handlers::get_graph))
        // ── Sessions ──
        .route(
            "/api/wiki/projects/{id}/sessions",
            get(handlers::list_sessions).delete(handlers::clear_sessions),
        )
        // ── Queue ──
        .route(
            "/api/wiki/projects/{id}/queue",
            get(handlers::get_queue_status),
        )
}
