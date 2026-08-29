use super::ingest;
use super::models::*;
use super::project;
use super::repository::WikiRepository;
use crate::core::proxy;
use crate::db::repository::Repository;
use crate::server::router::SharedState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
}

fn default_top_k() -> usize {
    10
}

// ── Project handlers ──

pub async fn list_projects(State(shared): State<SharedState>) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());
    match repo.list_projects().await {
        Ok(projects) => Json(serde_json::json!({ "data": projects })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn create_project(
    State(shared): State<SharedState>,
    Json(input): Json<CreateProjectInput>,
) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());
    let project_id = project::new_uuid();
    let schema = input
        .schema_text
        .clone()
        .unwrap_or_else(|| super::repository::DEFAULT_SCHEMA.to_string());

    // Create directory structure
    if let Err(e) = project::init_project_dir(&project_id, &schema).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
    }

    let dir = project::project_wiki_dir(&project_id);
    let wiki_dir = dir.to_string_lossy().to_string();

    match repo.create_project(&input, &wiki_dir).await {
        Ok(p) => (StatusCode::CREATED, Json(p)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn get_project(State(shared): State<SharedState>, Path(id): Path<String>) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());
    match repo.get_project(&id).await {
        Ok(p) => Json(p).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, e).into_response(),
    }
}

pub async fn update_project(
    State(shared): State<SharedState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateProjectInput>,
) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());

    // If schema_text changed, write to disk
    if let Some(ref schema) = input.schema_text {
        let dir = project::project_wiki_dir(&id);
        let schema_path = dir.join("schema").join("CLAUDE.md");
        let _ = tokio::fs::write(schema_path, schema).await;
    }

    match repo.update_project(&id, &input).await {
        Ok(p) => Json(p).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn delete_project(State(shared): State<SharedState>, Path(id): Path<String>) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());
    if let Err(e) = repo.delete_project(&id).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
    }
    if let Err(e) = project::remove_project_dir(&id).await {
        tracing::warn!("Failed to remove project dir: {}", e);
    }
    (StatusCode::NO_CONTENT, "").into_response()
}

pub async fn get_project_stats(
    State(shared): State<SharedState>,
    Path(id): Path<String>,
) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());
    match repo.get_stats(&id).await {
        Ok(stats) => Json(stats).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

// ── Source handlers ──

pub async fn list_sources(State(shared): State<SharedState>, Path(id): Path<String>) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());
    match repo.list_sources(&id).await {
        Ok(sources) => Json(serde_json::json!({ "data": sources })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn add_source(
    State(shared): State<SharedState>,
    Path(id): Path<String>,
    Json(input): Json<AddSourceInput>,
) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());

    let content_hash = input.content.as_ref().map(|c| {
        let mut hasher = Sha256::new();
        hasher.update(c.as_bytes());
        format!("{:x}", hasher.finalize())
    });

    let file_size = input.content.as_ref().map(|c| c.len() as i64).unwrap_or(0);

    // If content provided, write to disk
    if let Some(ref content) = input.content {
        if let Err(e) = project::write_source_file(&id, &input.filename, content.as_bytes()).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
        }
    }

    match repo
        .add_source(&id, &input, content_hash.as_deref(), file_size)
        .await
    {
        Ok(s) => (StatusCode::CREATED, Json(s)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn delete_source(
    State(shared): State<SharedState>,
    Path((_id, sid)): Path<(String, String)>,
) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());
    if let Err(e) = repo.delete_source(&sid).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
    }
    (StatusCode::NO_CONTENT, "").into_response()
}

pub async fn ingest_source(
    State(shared): State<SharedState>,
    Path((id, sid)): Path<(String, String)>,
) -> Response {
    let events = shared.state.events.clone();
    let settings = shared.state.settings.clone();
    let pool = shared.state.db.pool.clone();

    // Spawn ingest in background, return immediately with task info
    let project_id = id.clone();
    let source_id = sid.clone();

    match ingest::ingest_source(&events, &settings, &pool, &project_id, &source_id).await {
        Ok(result) => Json(serde_json::json!({
            "status": "done",
            "pages_created": result.pages_created,
            "page_paths": result.page_paths,
        }))
        .into_response(),
        Err(e) => {
            // Update source status to failed
            let repo = WikiRepository::new(pool);
            let _ = repo
                .update_source_status(&source_id, "failed", 0, Some(&e))
                .await;
            (StatusCode::INTERNAL_SERVER_ERROR, e).into_response()
        }
    }
}

pub async fn rescan_sources(State(shared): State<SharedState>, Path(id): Path<String>) -> Response {
    let events = shared.state.events.clone();
    let settings = shared.state.settings.clone();
    let pool = shared.state.db.pool.clone();
    let repo = WikiRepository::new(pool.clone());

    // Get all pending sources and ingest them
    let sources = match repo.list_sources(&id).await {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };

    let pending: Vec<_> = sources.iter().filter(|s| s.status == "pending").collect();
    let mut results = Vec::new();

    for source in &pending {
        match ingest::ingest_source(&events, &settings, &pool, &id, &source.id).await {
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

    Json(serde_json::json!({
        "status": "done",
        "processed": pending.len(),
        "results": results,
    }))
    .into_response()
}

// ── Page handlers ──

pub async fn list_pages(State(shared): State<SharedState>, Path(id): Path<String>) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());
    match repo.list_pages(&id).await {
        Ok(pages) => Json(serde_json::json!({ "data": pages })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn get_page(
    State(shared): State<SharedState>,
    Path((id, path)): Path<(String, String)>,
) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());

    // Try DB first
    if let Ok(Some(page)) = repo.get_page(&id, &path).await {
        // Also read file content
        if let Ok(content) = project::read_page(&id, &path).await {
            return Json(serde_json::json!({
                "id": page.id,
                "project_id": page.project_id,
                "path": page.path,
                "title": page.title,
                "page_type": page.page_type,
                "content_hash": page.content_hash,
                "token_count": page.token_count,
                "wikilinks": page.wikilinks,
                "frontmatter": page.frontmatter,
                "status": page.status,
                "content": content,
                "created_at": page.created_at,
                "updated_at": page.updated_at,
            }))
            .into_response();
        }
    }

    // Try reading file directly from disk
    match project::read_page(&id, &path).await {
        Ok(content) => {
            let title = path
                .split('/')
                .last()
                .unwrap_or(&path)
                .trim_end_matches(".md")
                .to_string();
            Json(serde_json::json!({
                "path": path,
                "title": title,
                "content": content,
                "page_type": "unknown",
            }))
            .into_response()
        }
        Err(e) => (StatusCode::NOT_FOUND, e).into_response(),
    }
}

pub async fn update_page(
    State(shared): State<SharedState>,
    Path((id, path)): Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let content = body.get("content").and_then(|c| c.as_str()).unwrap_or("");

    let repo = WikiRepository::new(shared.state.db.pool.clone());
    match update_page_inner(
        &shared.state.db.pool,
        &repo,
        &id,
        &path,
        content,
    )
    .await
    {
        Ok(()) => Json(serde_json::json!({ "ok": true, "path": path })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

/// Inner logic for saving a wiki page — shared by HTTP handler and MCP handler.
pub async fn update_page_inner(
    pool: &sqlx::SqlitePool,
    repo: &WikiRepository,
    id: &str,
    path: &str,
    content: &str,
) -> Result<(), String> {
    if let Err(e) = project::write_page(id, path, content).await {
        return Err(e);
    }

    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let title = ingest::extract_title_from_content(content, path);
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

    let token_count = (content.len() / 4) as i64; // rough estimate

    // Extract wikilinks
    let wikilinks: Vec<String> = extract_wikilinks(content);
    let wikilinks_json = serde_json::to_string(&wikilinks).unwrap_or("[]".to_string());

    // Extract tags from frontmatter
    let tags = crate::services::wiki::ingest::extract_tags_from_frontmatter(content);
    let tags_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string());

    repo.upsert_page(
        id,
        path,
        &title,
        page_type,
        &hash,
        token_count,
        &wikilinks_json,
        "{}",
        &tags_json,
    )
    .await?;

    // Rebuild knowledge graph edges based on updated wikilinks
    let _ = ingest::rebuild_graph_edges(pool, id).await;

    // Append log
    let _ = project::append_log(id, &format!("update | {}", path)).await;

    Ok(())
}

pub async fn delete_page(
    State(shared): State<SharedState>,
    Path((id, path)): Path<(String, String)>,
) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());
    if let Err(e) = repo.delete_page(&id, &path).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
    }
    let _ = project::delete_page_file(&id, &path).await;
    // Rebuild graph edges after page deletion
    let _ = ingest::rebuild_graph_edges(&shared.state.db.pool, &id).await;
    (StatusCode::NO_CONTENT, "").into_response()
}

// ── Search & Ask ──

pub async fn search(
    State(shared): State<SharedState>,
    Path(id): Path<String>,
    Query(params): Query<SearchQuery>,
) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());
    match repo.search_pages(&id, &params.q, params.top_k).await {
        Ok(results) => {
            Json(serde_json::json!({ "data": results, "query": params.q })).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn ask(
    State(shared): State<SharedState>,
    Path(id): Path<String>,
    Json(input): Json<WikiAskInput>,
) -> Response {
    let top_k = input.top_k.unwrap_or(5);
    let model = input.model.as_deref();

    match ask_inner(&shared, &id, &input.question, top_k, model).await {
        Ok(json) => Json(json).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

/// Inner logic for Wiki Q&A — shared by HTTP handler and MCP handler.
pub async fn ask_inner(
    shared: &SharedState,
    id: &str,
    question: &str,
    top_k: usize,
    model_override: Option<&str>,
) -> Result<serde_json::Value, String> {
    let pool = &shared.state.db.pool;
    let repo = WikiRepository::new(pool.clone());
    let db_repo = Arc::new(Repository::new(pool.clone()));
    let settings = shared.state.settings.clone();

    // Search relevant pages
    let results = repo
        .search_pages(id, question, top_k)
        .await
        .unwrap_or_default();

    // Read page contents
    let mut contexts = Vec::new();
    for r in &results {
        if let Ok(content) = project::read_page(id, &r.path).await {
            let snippet: String = content.chars().take(2000).collect();
            contexts.push(format!("## {} ({})\n{}", r.title, r.path, snippet));
        }
    }

    if contexts.is_empty() {
        return Ok(serde_json::json!({
            "answer": "No relevant wiki pages found for your question. Please ingest some documents first.",
            "sources": []
        }));
    }

    let context_text = contexts.join("\n\n---\n\n");

    // Get project config
    let proj = repo.get_project(id).await?;

    let chat_model = model_override
        .or(proj.chat_model.as_deref())
        .unwrap_or("gpt-4o");
    let chat_channel_id = match proj.chat_channel_id.as_deref() {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => {
            let row: Option<(String,)> = sqlx::query_as(
                "SELECT id FROM channels WHERE status = 1 ORDER BY priority DESC LIMIT 1",
            )
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("DB error: {}", e))?;
            match row.map(|(id,)| id) {
                Some(id) => id,
                None => return Err("No active channel configured. Please create a channel first or set chat_channel_id in Wiki project settings.".to_string()),
            }
        }
    };

    let system_prompt = "You are a Wiki knowledge assistant. Answer questions based on the provided wiki pages. Be concise and cite source pages using [[wikilinks]] format.";
    let user_prompt = format!(
        "Based on the following wiki pages, answer the question.\n\nWiki pages:\n{}\n\nQuestion: {}\n\nAnswer:",
        context_text, question
    );

    // Save user message
    let _ = repo.add_session(id, "user", question, None, None).await;

    let chat_request = serde_json::json!({
        "model": chat_model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt}
        ],
        "stream": false,
        "temperature": 0.4
    });
    let chat_request_str: String = serde_json::to_string(&chat_request).unwrap_or_default();

    let proxy_result = proxy::handle_request(
        &db_repo,
        &settings,
        &chat_channel_id,
        "Wiki Chat",
        chat_request,
        false,
        Some(chat_request_str),
        Some(format!("wiki-chat_{}", id)),
        None,
    )
    .await;

    let (answer, usage) = match proxy_result {
        Ok(result) => {
            let answer_text = result
                .body
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|t| t.as_str())
                .unwrap_or("Failed to generate answer.");

            let usage = result.body.get("usage").map(|u| serde_json::json!({
                "prompt_tokens": u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                "completion_tokens": u.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                "total_tokens": u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            }));

            (answer_text.to_string(), usage)
        }
        Err((code, msg)) => {
            let err_answer = format!("LLM request failed ({}): {}", code, msg);
            (err_answer, None)
        }
    };

    let sources: Vec<WikiAnswerSource> = results
        .iter()
        .map(|r| WikiAnswerSource {
            path: r.path.clone(),
            title: r.title.clone(),
            score: r.score,
            snippet: r.snippet.clone(),
        })
        .collect();

    // Save assistant message
    let _ = repo
        .add_session(
            id,
            "assistant",
            &answer,
            Some(&serde_json::to_string(&sources).unwrap_or_default()),
            Some(chat_model),
        )
        .await;

    Ok(serde_json::json!({
        "answer": answer,
        "sources": sources,
        "usage": usage,
    }))
}

// ── Graph ──

pub async fn get_graph(State(shared): State<SharedState>, Path(id): Path<String>) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());
    match repo.get_graph(&id).await {
        Ok(graph) => Json(graph).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

// ── Sessions ──

pub async fn list_sessions(State(shared): State<SharedState>, Path(id): Path<String>) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());
    match repo.list_sessions(&id).await {
        Ok(sessions) => Json(serde_json::json!({ "data": sessions })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn clear_sessions(State(shared): State<SharedState>, Path(id): Path<String>) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());
    if let Err(e) = repo.clear_sessions(&id).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
    }
    (StatusCode::NO_CONTENT, "").into_response()
}

// ── Queue ──

pub async fn get_queue_status(
    State(shared): State<SharedState>,
    Path(id): Path<String>,
) -> Response {
    let repo = WikiRepository::new(shared.state.db.pool.clone());
    match repo.list_tasks(&id).await {
        Ok(tasks) => Json(serde_json::json!({ "data": tasks })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

// ── Helpers ──

fn extract_wikilinks(content: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut start = 0;
    loop {
        if let Some(s) = content[start..].find("[[") {
            let s = start + s + 2;
            if let Some(e) = content[s..].find("]]") {
                let link = &content[s..s + e];
                if !link.is_empty() {
                    links.push(link.to_string());
                }
                start = s + e + 2;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    links
}
