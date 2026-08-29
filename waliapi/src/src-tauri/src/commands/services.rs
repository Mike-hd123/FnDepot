use crate::services::ServiceRegistry;
use crate::AppState;
use std::sync::Arc;
use tauri::State;

/// Get all service statuses (Knowledge Base, MCP, etc.)
#[tauri::command]
pub async fn get_service_statuses(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<serde_json::Value>, String> {
    get_service_statuses_impl(&*state).await
}

pub async fn get_service_statuses_impl(
    state: &Arc<AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let registry = ServiceRegistry::new();
    let statuses = registry.list_status(state).await;
    Ok(statuses
        .into_iter()
        .map(|s| serde_json::to_value(s).unwrap_or_default())
        .collect())
}
