use crate::AppState;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tauri::State;

// Re-export models for convenience
pub use crate::services::wiki::models::*;

// ── Wiki Project commands ──

#[tauri::command]
pub async fn get_wiki_projects(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<WikiProject>, String> {
    let pool = state.db.pool.clone();
    let rows =
        sqlx::query_as::<_, WikiProject>("SELECT * FROM wiki_projects ORDER BY created_at DESC")
            .fetch_all(&pool)
            .await
            .map_err(|e| format!("DB error: {}", e))?;
    Ok(rows)
}

#[tauri::command]
pub async fn create_wiki_project(
    state: State<'_, Arc<AppState>>,
    input: CreateProjectInput,
) -> Result<WikiProject, String> {
    let pool = state.db.pool.clone();
    let project_id = crate::services::wiki::project::new_uuid();
    let schema = input
        .schema_text
        .clone()
        .unwrap_or_else(|| crate::services::wiki::repository::DEFAULT_SCHEMA.to_string());

    // Create directory structure
    crate::services::wiki::project::init_project_dir(&project_id, &schema).await?;

    let dir = crate::services::wiki::project::project_wiki_dir(&project_id);
    let wiki_dir = dir.to_string_lossy().to_string();

    let repo = crate::services::wiki::repository::WikiRepository::new(pool);
    repo.create_project(&input, &wiki_dir).await
}

#[tauri::command]
pub async fn get_wiki_project(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<WikiProject, String> {
    let pool = state.db.pool.clone();
    let repo = crate::services::wiki::repository::WikiRepository::new(pool);
    repo.get_project(&id).await
}

#[tauri::command]
pub async fn update_wiki_project(
    state: State<'_, Arc<AppState>>,
    id: String,
    input: UpdateProjectInput,
) -> Result<WikiProject, String> {
    let pool = state.db.pool.clone();
    let repo = crate::services::wiki::repository::WikiRepository::new(pool);

    if let Some(ref schema) = input.schema_text {
        let dir = crate::services::wiki::project::project_wiki_dir(&id);
        let schema_path = dir.join("schema").join("CLAUDE.md");
        let _ = tokio::fs::write(schema_path, schema).await;
    }

    repo.update_project(&id, &input).await
}

#[tauri::command]
pub async fn delete_wiki_project(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    let pool = state.db.pool.clone();
    let repo = crate::services::wiki::repository::WikiRepository::new(pool);
    repo.delete_project(&id).await?;
    crate::services::wiki::project::remove_project_dir(&id).await?;
    Ok(())
}

// ── Wiki Pages ──

#[tauri::command]
pub async fn get_wiki_pages(
    state: State<'_, Arc<AppState>>,
    projectId: String,
) -> Result<Vec<WikiPage>, String> {
    let pool = state.db.pool.clone();
    let repo = crate::services::wiki::repository::WikiRepository::new(pool);
    repo.list_pages(&projectId).await
}

#[tauri::command]
pub async fn get_wiki_page(
    state: State<'_, Arc<AppState>>,
    projectId: String,
    path: String,
) -> Result<serde_json::Value, String> {
    let pool = state.db.pool.clone();
    let repo = crate::services::wiki::repository::WikiRepository::new(pool);

    if let Ok(Some(page)) = repo.get_page(&projectId, &path).await {
        if let Ok(content) = crate::services::wiki::project::read_page(&projectId, &path).await {
            return Ok(serde_json::json!({
                "id": page.id,
                "project_id": page.project_id,
                "path": page.path,
                "title": page.title,
                "page_type": page.page_type,
                "content": content,
                "wikilinks": page.wikilinks,
                "frontmatter": page.frontmatter,
                "status": page.status,
                "created_at": page.created_at,
                "updated_at": page.updated_at,
            }));
        }
    }

    // Try reading from disk
    match crate::services::wiki::project::read_page(&projectId, &path).await {
        Ok(content) => {
            let title = path
                .split('/')
                .last()
                .unwrap_or(&path)
                .trim_end_matches(".md")
                .to_string();
            Ok(serde_json::json!({
                "path": path,
                "title": title,
                "content": content,
                "page_type": "unknown",
            }))
        }
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub async fn save_wiki_page(
    state: State<'_, Arc<AppState>>,
    projectId: String,
    path: String,
    content: String,
) -> Result<(), String> {
    let pool = state.db.pool.clone();
    crate::services::wiki::project::write_page(&projectId, &path, &content).await?;

    let repo = crate::services::wiki::repository::WikiRepository::new(pool);
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let title = path
        .split('/')
        .last()
        .unwrap_or(&path)
        .trim_end_matches(".md")
        .to_string();
    let page_type = if path.contains("entities/") {
        "entity"
    } else if path.contains("concepts/") {
        "concept"
    } else if path.contains("summaries/") {
        "summary"
    } else if path.ends_with("index.md") {
        "index"
    } else if path.ends_with("log.md") {
        "log"
    } else {
        "entity"
    };
    let token_count = (content.len() / 4) as i64;
    let _ = repo
        .upsert_page(
            &projectId,
            &path,
            &title,
            page_type,
            &hash,
            token_count,
            "[]",
            "{}",
            "[]",
        )
        .await;
    let _ =
        crate::services::wiki::project::append_log(&projectId, &format!("update | {}", path)).await;
    Ok(())
}

// ── Wiki Sources ──

#[tauri::command]
pub async fn get_wiki_sources(
    state: State<'_, Arc<AppState>>,
    projectId: String,
) -> Result<Vec<WikiSource>, String> {
    let pool = state.db.pool.clone();
    let repo = crate::services::wiki::repository::WikiRepository::new(pool);
    repo.list_sources(&projectId).await
}

#[tauri::command]
pub async fn add_wiki_source(
    state: State<'_, Arc<AppState>>,
    projectId: String,
    input: AddSourceInput,
) -> Result<WikiSource, String> {
    let pool = state.db.pool.clone();
    let repo = crate::services::wiki::repository::WikiRepository::new(pool);

    let content_hash = input.content.as_ref().map(|c| {
        let mut hasher = Sha256::new();
        hasher.update(c.as_bytes());
        format!("{:x}", hasher.finalize())
    });
    let file_size = input.content.as_ref().map(|c| c.len() as i64).unwrap_or(0);

    if let Some(ref content) = input.content {
        crate::services::wiki::project::write_source_file(
            &projectId,
            &input.filename,
            content.as_bytes(),
        )
        .await?;
    }

    repo.add_source(&projectId, &input, content_hash.as_deref(), file_size)
        .await
}

#[tauri::command]
pub async fn delete_wiki_source(
    state: State<'_, Arc<AppState>>,
    sourceId: String,
) -> Result<(), String> {
    let pool = state.db.pool.clone();
    let repo = crate::services::wiki::repository::WikiRepository::new(pool);
    repo.delete_source(&sourceId).await
}

// ── Wiki Search ──

#[tauri::command]
pub async fn search_wiki(
    state: State<'_, Arc<AppState>>,
    projectId: String,
    query: String,
    topK: Option<usize>,
) -> Result<Vec<WikiSearchResult>, String> {
    let pool = state.db.pool.clone();
    let repo = crate::services::wiki::repository::WikiRepository::new(pool);
    repo.search_pages(&projectId, &query, topK.unwrap_or(10))
        .await
}

// ── Wiki Graph ──

#[tauri::command]
pub async fn get_wiki_graph(
    state: State<'_, Arc<AppState>>,
    projectId: String,
) -> Result<GraphData, String> {
    let pool = state.db.pool.clone();
    let repo = crate::services::wiki::repository::WikiRepository::new(pool);
    repo.get_graph(&projectId).await
}

// ── Wiki Tags ──

#[tauri::command]
pub async fn get_wiki_tags(
    state: State<'_, Arc<AppState>>,
    projectId: String,
    limit: Option<usize>,
) -> Result<Vec<WikiTag>, String> {
    let pool = state.db.pool.clone();
    let repo = crate::services::wiki::repository::WikiRepository::new(pool);
    repo.get_tags(&projectId, limit.unwrap_or(15)).await
}

// ── Wiki Stats ──

#[tauri::command]
pub async fn get_wiki_stats(
    state: State<'_, Arc<AppState>>,
    projectId: String,
) -> Result<serde_json::Value, String> {
    let pool = state.db.pool.clone();
    let repo = crate::services::wiki::repository::WikiRepository::new(pool);
    repo.get_stats(&projectId).await
}

// ── Wiki Ingest ──

#[tauri::command]
pub async fn ingest_wiki_source(
    state: State<'_, Arc<AppState>>,
    projectId: String,
    sourceId: String,
) -> Result<serde_json::Value, String> {
    let pool = state.db.pool.clone();
    crate::services::wiki::ingest::ingest_source(
        &state.events,
        &state.settings,
        &pool,
        &projectId,
        &sourceId,
    )
    .await
    .map(|r| {
        serde_json::json!({
            "status": "done",
            "pages_created": r.pages_created,
            "page_paths": r.page_paths,
        })
    })
    .map_err(|e| {
        let pool_clone = pool.clone();
        let sid = sourceId.clone();
        let pid = projectId.clone();
        let err = e.clone();
        let events = state.events.clone();
        tokio::spawn(async move {
            let repo = crate::services::wiki::repository::WikiRepository::new(pool_clone);
            let _ = repo
                .update_source_status(&sid, "failed", 0, Some(&err))
                .await;
            events.emit(
                "wiki-source-progress",
                serde_json::json!({
                    "source_id": sid,
                    "project_id": pid,
                    "filename": "",
                    "stage": "error",
                    "progress": 0,
                    "detail": &err,
                }),
            );
        });
        e
    })
}

#[tauri::command]
pub async fn rescan_wiki_sources(
    state: State<'_, Arc<AppState>>,
    projectId: String,
) -> Result<serde_json::Value, String> {
    let pool = state.db.pool.clone();
    let repo = crate::services::wiki::repository::WikiRepository::new(pool.clone());

    let sources = repo.list_sources(&projectId).await?;
    let pending: Vec<_> = sources.iter().filter(|s| s.status == "pending").collect();
    let mut results = Vec::new();

    for source in &pending {
        match crate::services::wiki::ingest::ingest_source(
            &state.events,
            &state.settings,
            &pool,
            &projectId,
            &source.id,
        )
        .await
        {
            Ok(r) => results.push(serde_json::json!({
                "source_id": source.id,
                "filename": source.filename,
                "status": "done",
                "pages": r.pages_created,
            })),
            Err(e) => results.push(serde_json::json!({
                "source_id": source.id,
                "filename": source.filename,
                "status": "failed",
                "error": e,
            })),
        }
    }

    Ok(serde_json::json!({
        "status": "done",
        "processed": pending.len(),
        "results": results,
    }))
}
