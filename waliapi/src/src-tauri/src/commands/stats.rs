use crate::db::repository::Repository;
use crate::AppState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct DashboardStatsDto {
    pub today_requests: i64,
    pub today_total_tokens: i64,
    pub active_channels: i64,
    pub avg_latency_ms: f64,
    pub total_channels: i64,
    pub total_api_keys: i64,
    pub total_requests: i64,
    pub total_tokens: i64,
    pub total_knowledge_bases: i64,
    pub total_kb_documents: i64,
    pub total_kb_chunks: i64,
    pub total_wiki_projects: i64,
    pub total_wiki_pages: i64,
}

#[tauri::command]
pub async fn get_dashboard_stats(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<DashboardStatsDto, String> {
    get_dashboard_stats_impl(&*state).await
}

pub async fn get_dashboard_stats_impl(
    state: &std::sync::Arc<AppState>,
) -> Result<DashboardStatsDto, String> {
    let repo = Repository::new(state.db.pool.clone());
    let s = repo
        .get_dashboard_stats()
        .await
        .map_err(|e| e.to_string())?;
    Ok(DashboardStatsDto {
        today_requests: s.today_requests,
        today_total_tokens: s.today_total_tokens,
        active_channels: s.active_channels,
        avg_latency_ms: s.avg_latency_ms,
        total_channels: s.total_channels,
        total_api_keys: s.total_api_keys,
        total_requests: s.total_requests,
        total_tokens: s.total_tokens,
        total_knowledge_bases: s.total_knowledge_bases,
        total_kb_documents: s.total_kb_documents,
        total_kb_chunks: s.total_kb_chunks,
        total_wiki_projects: s.total_wiki_projects,
        total_wiki_pages: s.total_wiki_pages,
    })
}
