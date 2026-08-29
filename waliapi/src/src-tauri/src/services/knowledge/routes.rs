use super::handlers;
use crate::server::router::SharedState;
use crate::AppState;
#[allow(unused_imports)]
use axum::routing::{delete as _delete, put as _put};
use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;

pub fn create_router(_state: Arc<AppState>) -> Router<SharedState> {
    Router::new()
        // Knowledge Base CRUD
        .route(
            "/api/kb",
            get(handlers::list_knowledge_bases).post(handlers::create_knowledge_base),
        )
        .route(
            "/api/kb/{id}",
            get(handlers::get_knowledge_base)
                .put(handlers::update_knowledge_base)
                .delete(handlers::delete_knowledge_base),
        )
        .route("/api/kb/{id}/stats", get(handlers::kb_stats))
        // Documents
        .route(
            "/api/kb/{id}/documents",
            get(handlers::list_documents).post(handlers::upload_document),
        )
        .route(
            "/api/kb/{kb_id}/documents/{doc_id}",
            get(handlers::get_document).delete(handlers::delete_document),
        )
        .route(
            "/api/kb/{kb_id}/documents/{doc_id}/reindex",
            post(handlers::reindex_document),
        )
        // Search & RAG
        .route("/api/kb/search", get(handlers::search))
        .route("/api/kb/ask", post(handlers::ask))
        // Conversation History
        .route(
            "/api/kb/{kb_id}/conversations",
            get(handlers::list_conversations).delete(handlers::clear_conversations),
        )
        // Sources (Multi-source import)
        .route(
            "/api/kb/{kb_id}/sources",
            get(handlers::list_sources).post(handlers::import_source),
        )
        .route(
            "/api/kb/{kb_id}/sources/{source_id}",
            axum::routing::delete(handlers::delete_source),
        )
        // Index Management
        .route(
            "/api/kb/{kb_id}/index",
            get(handlers::get_index_status)
                .post(handlers::build_index)
                .delete(handlers::drop_index),
        )
}
