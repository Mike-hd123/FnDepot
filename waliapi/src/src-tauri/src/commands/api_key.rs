use crate::db::models::{ApiKey, ApiKeyStats, CreateApiKeyInput};
use crate::db::repository::Repository;
use crate::AppState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyDto {
    pub id: String,
    pub name: String,
    pub key: String,
    pub status: i64,
    pub allowed_models: Vec<String>,
    pub allowed_channels: Vec<String>,
    pub denied_models: Vec<String>,
    pub denied_channels: Vec<String>,
    pub quota_limit: i64,
    pub quota_used: i64,
    pub expires_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<ApiKey> for ApiKeyDto {
    fn from(k: ApiKey) -> Self {
        ApiKeyDto {
            id: k.id,
            name: k.name,
            key: k.key,
            status: k.status,
            allowed_models: serde_json::from_str(&k.allowed_models).unwrap_or_default(),
            allowed_channels: serde_json::from_str(&k.allowed_channels).unwrap_or_default(),
            denied_models: serde_json::from_str(&k.denied_models).unwrap_or_default(),
            denied_channels: serde_json::from_str(&k.denied_channels).unwrap_or_default(),
            quota_limit: k.quota_limit,
            quota_used: k.quota_used,
            expires_at: k.expires_at,
            created_at: k.created_at,
            updated_at: k.updated_at,
        }
    }
}

#[tauri::command]
pub async fn get_api_keys(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<Vec<ApiKeyDto>, String> {
    get_api_keys_impl(&*state).await
}

pub async fn get_api_keys_impl(state: &std::sync::Arc<AppState>) -> Result<Vec<ApiKeyDto>, String> {
    let repo = Repository::new(state.db.pool.clone());
    repo.get_all_api_keys()
        .await
        .map_err(|e| e.to_string())
        .map(|ks| ks.into_iter().map(Into::into).collect())
}

#[tauri::command]
pub async fn create_api_key(
    input: CreateApiKeyInput,
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<ApiKeyDto, String> {
    create_api_key_impl(input, &*state).await
}

pub async fn create_api_key_impl(
    input: CreateApiKeyInput,
    state: &std::sync::Arc<AppState>,
) -> Result<ApiKeyDto, String> {
    let repo = Repository::new(state.db.pool.clone());
    repo.create_api_key(&input)
        .await
        .map_err(|e| e.to_string())
        .map(Into::into)
}

#[derive(Debug, Deserialize)]
pub struct UpdateApiKeyInput {
    pub id: String,
    pub name: Option<String>,
    pub quota_limit: Option<i64>,
    pub status: Option<i64>,
    pub allowed_models: Option<Vec<String>>,
    pub allowed_channels: Option<Vec<String>>,
    pub denied_models: Option<Vec<String>>,
    pub denied_channels: Option<Vec<String>>,
}

#[tauri::command]
pub async fn update_api_key(
    input: UpdateApiKeyInput,
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<(), String> {
    update_api_key_impl(input, &*state).await
}

pub async fn update_api_key_impl(
    input: UpdateApiKeyInput,
    state: &std::sync::Arc<AppState>,
) -> Result<(), String> {
    let repo = Repository::new(state.db.pool.clone());
    if let Some(name) = &input.name {
        repo.update_api_key_name(&input.id, name)
            .await
            .map_err(|e| e.to_string())?;
    }
    if let Some(quota_limit) = input.quota_limit {
        repo.update_api_key_quota(&input.id, quota_limit)
            .await
            .map_err(|e| e.to_string())?;
    }
    if let Some(status) = input.status {
        repo.update_api_key_status(&input.id, status)
            .await
            .map_err(|e| e.to_string())?;
    }
    if let Some(models) = &input.allowed_models {
        repo.update_api_key_allowed_models(&input.id, models)
            .await
            .map_err(|e| e.to_string())?;
    }
    if let Some(channels) = &input.allowed_channels {
        repo.update_api_key_allowed_channels(&input.id, channels)
            .await
            .map_err(|e| e.to_string())?;
    }
    if let Some(models) = &input.denied_models {
        repo.update_api_key_denied_models(&input.id, models)
            .await
            .map_err(|e| e.to_string())?;
    }
    if let Some(channels) = &input.denied_channels {
        repo.update_api_key_denied_channels(&input.id, channels)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_api_key(
    id: String,
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<(), String> {
    delete_api_key_impl(&id, &*state).await
}

pub async fn delete_api_key_impl(id: &str, state: &std::sync::Arc<AppState>) -> Result<(), String> {
    let repo = Repository::new(state.db.pool.clone());
    repo.delete_api_key(id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_api_key_stats(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<Vec<ApiKeyStats>, String> {
    get_api_key_stats_impl(&*state).await
}

pub async fn get_api_key_stats_impl(
    state: &std::sync::Arc<AppState>,
) -> Result<Vec<ApiKeyStats>, String> {
    let repo = Repository::new(state.db.pool.clone());
    repo.get_api_key_stats().await.map_err(|e| e.to_string())
}
