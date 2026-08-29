use super::models::*;
use super::project;
use super::repository::WikiRepository;
use crate::core::proxy;
use crate::db::repository::Repository;
use crate::server::event_bridge::EventSink;
use crate::settings_store::SettingsStore;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::Arc;

/// Ingest a source file: read → parse → generate wiki pages via LLM → write to disk+DB.
pub async fn ingest_source(
    events: &EventSink,
    settings: &SettingsStore,
    pool: &sqlx::SqlitePool,
    project_id: &str,
    source_id: &str,
) -> Result<IngestResult, String> {
    let repo = WikiRepository::new(pool.clone());
    let db_repo = Arc::new(Repository::new(pool.clone()));

    // 1. Load source record
    let sources = repo.list_sources(project_id).await?;
    let source = sources
        .iter()
        .find(|s| s.id == source_id)
        .ok_or_else(|| format!("Source {} not found", source_id))?;

    // 2. Update task status
    let task_id = repo
        .create_task(project_id, Some(source_id), "ingest")
        .await?;
    repo.update_task_status(&task_id, "running", 0, 0, 3, None, None)
        .await?;
    emit_wiki_progress(
        events,
        source_id,
        project_id,
        &source.filename,
        "processing",
        0,
        "准备摄入",
    );

    // 3. Read source file content
    let content = if let Some(ref file_path) = source.file_path {
        // Read from disk path
        tokio::fs::read_to_string(file_path)
            .await
            .map_err(|e| format!("Failed to read source file: {}", e))?
    } else {
        // Read from project raw/sources/ dir
        let raw = project::read_source_file(project_id, &source.filename)
            .await
            .map_err(|e| format!("Failed to read source: {}", e))?;
        String::from_utf8_lossy(&raw).to_string()
    };

    let source_filename = &source.filename;
    let file_ext = source_filename
        .rsplit('.')
        .next()
        .unwrap_or("txt")
        .to_lowercase();

    // 4. Parse content into sections/chunks
    repo.update_task_status(&task_id, "running", 10, 0, 3, None, None)
        .await?;
    emit_wiki_progress(
        events,
        source_id,
        project_id,
        &source.filename,
        "parsing",
        10,
        "解析文档",
    );
    let sections = parse_content(&content, &file_ext);

    // 5. Get project config for LLM
    let proj = repo.get_project(project_id).await?;
    let ingest_model = proj.ingest_model.as_deref().unwrap_or("gpt-4o");
    let ingest_channel_id = match proj.ingest_channel_id.as_deref() {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => {
            // Fallback: find first active channel from DB
            let row: Option<(String,)> = sqlx::query_as(
                "SELECT id FROM channels WHERE status = 1 ORDER BY priority DESC LIMIT 1",
            )
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("DB error: {}", e))?;
            let id = row.map(|(id,)| id).ok_or_else(|| "No active channel configured. Please create a channel first or set ingest_channel_id in Wiki project settings.".to_string())?;
            id
        }
    };

    // 6. Generate wiki pages via LLM
    repo.update_task_status(&task_id, "running", 30, 1, 3, None, None)
        .await?;
    emit_wiki_progress(
        events,
        source_id,
        project_id,
        &source.filename,
        "generating",
        30,
        "LLM 生成页面",
    );
    let pages = generate_wiki_pages(
        settings,
        &db_repo,
        ingest_model,
        &ingest_channel_id,
        project_id,
        source_filename,
        &sections,
        proj.schema_text
            .as_deref()
            .unwrap_or(super::repository::DEFAULT_SCHEMA),
    )
    .await?;

    // 7. Write pages to disk + DB
    repo.update_task_status(&task_id, "running", 60, 2, 3, None, None)
        .await?;
    emit_wiki_progress(
        events,
        source_id,
        project_id,
        &source.filename,
        "writing",
        60,
        "写入页面",
    );
    let mut written_pages = Vec::new();
    for page in &pages {
        let page_path = &page.path;

        // Write to disk
        project::write_page(project_id, page_path, &page.content).await?;

        // Compute hash
        let mut hasher = Sha256::new();
        hasher.update(page.content.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        let token_count = (page.content.len() / 4) as i64;

        // Extract wikilinks
        let wikilinks = extract_wikilinks(&page.content);
        let wikilinks_json = serde_json::to_string(&wikilinks).unwrap_or_else(|_| "[]".to_string());

        // Extract tags from frontmatter
        let tags = extract_tags_from_frontmatter(&page.content);
        let tags_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string());

        // Upsert into DB
        repo.upsert_page(
            project_id,
            page_path,
            &page.title,
            &page.page_type,
            &hash,
            token_count,
            &wikilinks_json,
            "{}",
            &tags_json,
        )
        .await?;

        written_pages.push(WrittenPage {
            path: page_path.clone(),
            title: page.title.clone(),
            page_type: page.page_type.clone(),
            wikilinks,
        });
    }

    // 8. Update graph edges from wikilinks
    repo.update_task_status(&task_id, "running", 80, 2, 3, None, None)
        .await?;
    emit_wiki_progress(
        events,
        source_id,
        project_id,
        &source.filename,
        "linking",
        80,
        "更新知识图谱",
    );
    update_graph_edges(pool, project_id, &written_pages).await?;

    // 9. Update source status
    repo.update_source_status(source_id, "ingested", written_pages.len() as i64, None)
        .await?;

    // Update project counts
    let page_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM wiki_pages WHERE project_id = ? AND status = 'active'",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let source_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM wiki_sources WHERE project_id = ? AND status = 'ingested'",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE wiki_projects SET page_count=?, source_count=?, last_ingest_at=?, updated_at=? WHERE id=?"
    )
    .bind(page_count).bind(source_count).bind(&now).bind(&now).bind(project_id)
    .execute(pool).await.map_err(|e| format!("DB error: {}", e))?;

    // Append log
    let _ = project::append_log(
        project_id,
        &format!(
            "ingest | {} → {} pages",
            source_filename,
            written_pages.len()
        ),
    )
    .await;

    // Update task
    let result_json = serde_json::json!({
        "pages_created": written_pages.len(),
        "source": source_filename,
    })
    .to_string();
    repo.update_task_status(&task_id, "done", 100, 3, 3, Some(&result_json), None)
        .await?;
    emit_wiki_progress(
        events,
        source_id,
        project_id,
        &source.filename,
        "done",
        100,
        &format!("完成，生成 {} 个页面", written_pages.len()),
    );

    Ok(IngestResult {
        pages_created: written_pages.len(),
        page_paths: written_pages.iter().map(|p| p.path.clone()).collect(),
    })
}

#[derive(Debug, Clone)]
pub struct IngestResult {
    pub pages_created: usize,
    pub page_paths: Vec<String>,
}

struct GeneratedPage {
    path: String,
    title: String,
    page_type: String,
    content: String,
}

/// Emit wiki source ingest progress event to frontend.
fn emit_wiki_progress(
    events: &EventSink,
    source_id: &str,
    project_id: &str,
    filename: &str,
    stage: &str,
    progress: u8,
    detail: &str,
    ) {
    events.emit(
        "wiki-source-progress",
        serde_json::json!({
            "source_id": source_id,
            "project_id": project_id,
            "filename": filename,
            "stage": stage,
            "progress": progress,
            "detail": detail,
        }),
    );
}

struct WrittenPage {
    path: String,
    title: String,
    page_type: String,
    wikilinks: Vec<String>,
}

/// Parse content into sections based on file type.
fn parse_content(content: &str, file_ext: &str) -> Vec<ContentSection> {
    match file_ext.as_ref() {
        "md" | "markdown" => parse_markdown(content),
        "txt" => parse_plain_text(content),
        "json" => parse_json(content),
        _ => parse_plain_text(content),
    }
}

#[derive(Debug, Clone)]
struct ContentSection {
    heading: String,
    content: String,
}

fn parse_markdown(content: &str) -> Vec<ContentSection> {
    let mut sections = Vec::new();
    let mut current_heading = String::from("Overview");
    let mut current_content = String::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            if !current_content.is_empty() {
                sections.push(ContentSection {
                    heading: current_heading.clone(),
                    content: current_content.clone(),
                });
                current_content.clear();
            }
            current_heading = trimmed[2..].to_string();
        } else if trimmed.starts_with("## ") {
            if !current_content.is_empty() {
                sections.push(ContentSection {
                    heading: current_heading.clone(),
                    content: current_content.clone(),
                });
                current_content.clear();
            }
            current_heading = trimmed[3..].to_string();
        } else {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    if !current_content.is_empty() {
        sections.push(ContentSection {
            heading: current_heading,
            content: current_content,
        });
    }

    // If only one small section, split by paragraphs
    if sections.len() <= 1 && content.len() > 2000 {
        return parse_plain_text(content);
    }

    if sections.is_empty() {
        sections.push(ContentSection {
            heading: "Document".to_string(),
            content: content.to_string(),
        });
    }

    sections
}

fn parse_plain_text(content: &str) -> Vec<ContentSection> {
    let mut sections = Vec::new();
    let mut chunk = String::new();
    let mut chunk_idx = 1;
    let max_chunk = 3000;

    for line in content.lines() {
        if chunk.len() + line.len() > max_chunk {
            if !chunk.is_empty() {
                sections.push(ContentSection {
                    heading: format!("Section {}", chunk_idx),
                    content: chunk.clone(),
                });
                chunk_idx += 1;
                chunk.clear();
            }
        }
        chunk.push_str(line);
        chunk.push('\n');
    }

    if !chunk.is_empty() {
        sections.push(ContentSection {
            heading: format!("Section {}", chunk_idx),
            content: chunk,
        });
    }

    if sections.is_empty() {
        sections.push(ContentSection {
            heading: "Document".to_string(),
            content: content.to_string(),
        });
    }

    sections
}

fn parse_json(content: &str) -> Vec<ContentSection> {
    // Try to parse and flatten JSON into readable sections
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(json) => {
            let pretty =
                serde_json::to_string_pretty(&json).unwrap_or_else(|_| content.to_string());
            parse_plain_text(&pretty)
        }
        Err(_) => parse_plain_text(content),
    }
}

/// Generate wiki pages from content sections via LLM.
async fn generate_wiki_pages(
    settings: &SettingsStore,
    db_repo: &Arc<Repository>,
    model: &str,
    channel_id: &str,
    project_id: &str,
    source_filename: &str,
    sections: &[ContentSection],
    schema: &str,
) -> Result<Vec<GeneratedPage>, String> {
    // Build a combined context from all sections
    let combined: String = sections
        .iter()
        .map(|s| format!("## {}\n{}", s.heading, s.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    // Truncate to fit context window (rough estimate)
    let max_chars = 24000;
    let truncated = if combined.len() > max_chars {
        let mut t = combined[..max_chars].to_string();
        t.push_str("\n\n[... content truncated ...]");
        t
    } else {
        combined
    };

    let system_prompt = format!(
        r#"You are a Wiki maintainer. Read the source document and generate structured wiki pages in Markdown.

## Wiki Schema
{}

## Instructions
1. Analyze the source document and identify key entities, concepts, and topics.
2. For each key item, generate a wiki page in Markdown format.
3. Use `[[wikilinks]]` to connect related pages.
4. Each page should have YAML frontmatter with: title, type (entity/concept/summary), tags, source.
5. Separate pages with a delimiter: ---PAGE---
6. The first line of each page should be the file path (e.g., `entities/my-item.md`).

## Output Format
```
entities/page-name.md
---
title: Page Name
type: entity
tags: [tag1, tag2]
source: {}
---
# Page Name

Content here with [[wikilinks]] to other pages.

## Details
...
---PAGE---
concepts/another-concept.md
---
title: Another Concept
type: concept
tags: [tag1]
source: {}
---
# Another Concept
...
```

Generate 3-8 pages depending on document complexity. Focus on the most important entities and concepts."#,
        schema, source_filename, source_filename
    );

    let user_prompt = format!(
        "Source document: {}\n\nContent:\n{}",
        source_filename, truncated
    );

    let chat_request = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt}
        ],
        "stream": false,
        "temperature": 0.3
    });

    let chat_request_str: String = serde_json::to_string(&chat_request).unwrap_or_default();

    let proxy_result = proxy::handle_request(
        db_repo,
        settings,
        channel_id,
        "Wiki Ingest",
        chat_request,
        false,
        Some(chat_request_str),
        Some(format!("wiki-ingest_{}", project_id)),
        None,
    )
    .await;

    let response_body = match proxy_result {
        Ok(result) => result.body,
        Err((code, msg)) => {
            return Err(format!("LLM request failed ({}): {}", code, msg));
        }
    };

    // Extract text from response
    let raw_text = response_body
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|t| t.as_str())
        .unwrap_or("");

    if raw_text.is_empty() {
        // Fallback: create a single summary page from the source
        return Ok(vec![GeneratedPage {
            path: format!("summaries/{}.md", sanitize_filename(source_filename)),
            title: source_filename.to_string(),
            page_type: "summary".to_string(),
            content: format!(
                "---\ntitle: {}\ntype: summary\ntags: []\nsource: {}\n---\n\n# {}\n\n{}",
                source_filename, source_filename, source_filename, truncated
            ),
        }]);
    }

    // Parse the LLM output into pages
    Ok(parse_generated_pages(raw_text, source_filename))
}

/// Parse LLM-generated pages from the response text.
fn parse_generated_pages(text: &str, source_filename: &str) -> Vec<GeneratedPage> {
    let mut pages = Vec::new();
    let mut current_path = String::new();
    let mut current_content = String::new();
    let mut in_content = false;

    let mut lines = text.lines().peekable();

    // Skip any preamble before first path
    while let Some(line) = lines.peek() {
        let trimmed = line.trim();
        if trimmed.ends_with(".md") || (trimmed.contains('/') && trimmed.ends_with(".md")) {
            break;
        }
        // Also check for path-like patterns
        if trimmed.contains(".md") && !trimmed.starts_with("#") {
            break;
        }
        lines.next();
    }

    for line in text.lines() {
        let trimmed = line.trim();

        // Check for page delimiter
        if trimmed == "---PAGE---" {
            if !current_content.is_empty() {
                if let Some(path) = extract_path_from_content(&current_content) {
                    pages.push(build_page(&path, &current_content, source_filename));
                }
            }
            current_content.clear();
            current_path.clear();
            in_content = false;
            continue;
        }

        // Check if line looks like a file path (ends with .md and has no spaces in path part)
        if !in_content && (trimmed.ends_with(".md") && trimmed.len() < 200) {
            current_path = trimmed.to_string();
            in_content = true;
            continue;
        }

        if in_content {
            current_content.push_str(line);
            current_content.push('\n');
        }
    }

    // Handle last page
    if !current_content.is_empty() {
        if current_path.is_empty() {
            // Try to extract path from content
            if let Some(path) = extract_path_from_content(&current_content) {
                current_path = path;
            }
        }
        if !current_path.is_empty() {
            pages.push(build_page(&current_path, &current_content, source_filename));
        } else if let Some(page) = build_page_from_content(&current_content, source_filename) {
            pages.push(page);
        }
    }

    // Deduplicate by path
    let mut seen = HashSet::new();
    pages.retain(|p| {
        if seen.contains(&p.path) {
            false
        } else {
            seen.insert(p.path.clone());
            true
        }
    });

    if pages.is_empty() {
        // Fallback: create a summary page
        pages.push(GeneratedPage {
            path: format!("summaries/{}.md", sanitize_filename(source_filename)),
            title: source_filename.to_string(),
            page_type: "summary".to_string(),
            content: format!(
                "---\ntitle: {}\ntype: summary\ntags: []\nsource: {}\n---\n\n# {}\n\n{}",
                source_filename,
                source_filename,
                source_filename,
                text.chars().take(8000).collect::<String>()
            ),
        });
    }

    pages
}

fn extract_path_from_content(content: &str) -> Option<String> {
    // Look for first line that looks like a path
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.ends_with(".md") && trimmed.len() < 200 {
            // Clean up markdown code fences
            let clean = trimmed
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim();
            if clean.ends_with(".md") {
                return Some(clean.to_string());
            }
        }
    }
    None
}

fn build_page(path: &str, raw_content: &str, source_filename: &str) -> GeneratedPage {
    // Remove path line from content if present
    let content = raw_content
        .lines()
        .filter(|l| {
            let t = l.trim();
            !(t.ends_with(".md") && t.len() < 200)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let content = content.trim();

    // Extract title from frontmatter or first heading
    let title = extract_title_from_content(content, path);

    // Determine page type from frontmatter or path
    let page_type = if path.starts_with("entities/") {
        "entity"
    } else if path.starts_with("concepts/") {
        "concept"
    } else if path.starts_with("summaries/") {
        "summary"
    } else if path.ends_with("index.md") {
        "index"
    } else if path.ends_with("log.md") {
        "log"
    } else {
        "entity"
    };

    // Ensure content has frontmatter
    let final_content = if content.starts_with("---") {
        content.to_string()
    } else {
        format!(
            "---\ntitle: {}\ntype: {}\ntags: []\nsource: {}\n---\n\n{}",
            title, page_type, source_filename, content
        )
    };

    GeneratedPage {
        path: path.to_string(),
        title,
        page_type: page_type.to_string(),
        content: final_content,
    }
}

fn build_page_from_content(content: &str, source_filename: &str) -> Option<GeneratedPage> {
    let title = extract_title_from_content(content, "");
    if title.is_empty() {
        return None;
    }
    let path = format!("entities/{}.md", sanitize_filename(&title));
    Some(GeneratedPage {
        path,
        title,
        page_type: "entity".to_string(),
        content: content.to_string(),
    })
}

pub fn extract_title_from_content(content: &str, fallback_path: &str) -> String {
    // Try frontmatter title
    if content.starts_with("---") {
        let end = content[3..].find("---");
        if let Some(e) = end {
            let frontmatter = &content[3..3 + e];
            for line in frontmatter.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("title:") {
                    let title = trimmed[6..].trim().trim_matches('"').trim_matches('\'');
                    if !title.is_empty() {
                        return title.to_string();
                    }
                }
            }
        }
    }
    // Try first heading
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            return trimmed[2..].trim().to_string();
        }
        if trimmed.starts_with("## ") {
            return trimmed[3..].trim().to_string();
        }
    }
    // Fallback to filename from path
    if !fallback_path.is_empty() {
        return fallback_path
            .split('/')
            .last()
            .unwrap_or(fallback_path)
            .trim_end_matches(".md")
            .to_string();
    }
    "Untitled".to_string()
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

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

/// Rebuild graph edges for a project based on current page wikilinks.
/// Called after page save/delete to keep the knowledge graph up-to-date.
pub async fn rebuild_graph_edges(pool: &sqlx::SqlitePool, project_id: &str) -> Result<(), String> {
    // Load all pages from DB
    let existing_pages: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT path, wikilinks, title FROM wiki_pages WHERE project_id = ? AND status = 'active'",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("DB error: {}", e))?;

    let mut all_links: Vec<(String, String)> = Vec::new();

    for (path, wikilinks_json, _title) in &existing_pages {
        let links: Vec<String> = serde_json::from_str(wikilinks_json).unwrap_or_default();
        for link in links {
            let target = resolve_wikilink_to_path(pool, project_id, &link).await;
            // Check if target exists
            if existing_pages.iter().any(|(p, _, _)| p == &target) {
                all_links.push((path.clone(), target));
            }
        }
    }

    // Clear old edges and insert new ones
    sqlx::query("DELETE FROM wiki_graph_edges WHERE project_id = ?")
        .bind(project_id)
        .execute(pool)
        .await
        .map_err(|e| format!("DB error: {}", e))?;

    let now = chrono::Utc::now().to_rfc3339();
    for (source, target) in all_links {
        let id = uuid::Uuid::new_v4().to_string();
        let _ = sqlx::query(
            "INSERT OR IGNORE INTO wiki_graph_edges (id, project_id, source_page, target_page, edge_type, weight, created_at)
             VALUES (?, ?, ?, ?, 'wikilink', 1.0, ?)"
        )
        .bind(&id).bind(project_id).bind(&source).bind(&target).bind(&now)
        .execute(pool).await;
    }

    Ok(())
}

/// Update graph_edges table based on wikilinks in pages.
async fn update_graph_edges(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    pages: &[WrittenPage],
) -> Result<(), String> {
    // Collect all valid page paths
    let valid_paths: HashSet<String> = pages.iter().map(|p| p.path.clone()).collect();

    // Also load existing pages from DB
    let existing_pages: Vec<(String, String)> = sqlx::query_as(
        "SELECT path, wikilinks FROM wiki_pages WHERE project_id = ? AND status = 'active'",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("DB error: {}", e))?;

    let mut all_links: Vec<(String, String)> = Vec::new();

    // From new pages
    for page in pages {
        for link in &page.wikilinks {
            // Resolve wikilink to actual page path via title matching
            let target = resolve_wikilink_to_path(pool, project_id, link).await;
            if valid_paths.contains(&target) || existing_pages.iter().any(|(p, _)| p == &target) {
                all_links.push((page.path.clone(), target));
            }
        }
    }

    // From existing pages (re-scan to catch new targets)
    for (path, wikilinks_json) in &existing_pages {
        let links: Vec<String> = serde_json::from_str(wikilinks_json).unwrap_or_default();
        for link in links {
            let target = resolve_wikilink_to_path(pool, project_id, &link).await;
            // Check if target exists (in new pages or existing)
            if valid_paths.contains(&target) || existing_pages.iter().any(|(p, _)| p == &target) {
                all_links.push((path.clone(), target));
            }
        }
    }

    // Clear old edges and insert new ones
    sqlx::query("DELETE FROM wiki_graph_edges WHERE project_id = ?")
        .bind(project_id)
        .execute(pool)
        .await
        .map_err(|e| format!("DB error: {}", e))?;

    let now = chrono::Utc::now().to_rfc3339();
    for (source, target) in all_links {
        let id = uuid::Uuid::new_v4().to_string();
        let _ = sqlx::query(
            "INSERT OR IGNORE INTO wiki_graph_edges (id, project_id, source_page, target_page, edge_type, weight, created_at)
             VALUES (?, ?, ?, ?, 'wikilink', 1.0, ?)"
        )
        .bind(&id).bind(project_id).bind(&source).bind(&target).bind(&now)
        .execute(pool).await;
    }

    Ok(())
}

/// Extract tags from YAML frontmatter of a wiki page.
pub fn extract_tags_from_frontmatter(content: &str) -> Vec<String> {
    if !content.starts_with("---") {
        return vec![];
    }
    let rest = &content[3..];
    let end = match rest.find("---") {
        Some(e) => e,
        None => return vec![],
    };
    let frontmatter = &rest[..end];
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("tags:") {
            let tags_part = trimmed[5..].trim();
            // Parse array format: [tag1, tag2] or ["tag1", "tag2"]
            let cleaned = tags_part
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim();
            if cleaned.is_empty() {
                return vec![];
            }
            return cleaned
                .split(',')
                .map(|t| {
                    t.trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .trim()
                        .to_string()
                })
                .filter(|t| !t.is_empty())
                .collect();
        }
    }
    vec![]
}

fn normalize_wikilink(link: &str) -> String {
    let link = link.trim();
    // If it already looks like a path, use as-is
    if link.contains('/') && link.ends_with(".md") {
        return link.to_string();
    }
    // Otherwise, assume it's an entity name
    let slug: String = link
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    format!("entities/{}.md", slug)
}

/// Resolve a wikilink to an actual page path by matching against known page titles.
/// Falls back to `normalize_wikilink` if no title match is found.
async fn resolve_wikilink_to_path(pool: &sqlx::SqlitePool, project_id: &str, link: &str) -> String {
    let link = link.trim();
    // If it already looks like a path, use as-is
    if link.contains('/') && link.ends_with(".md") {
        return link.to_string();
    }
    // Try exact title match (case-insensitive)
    let pattern = format!("%{}%", link);
    let title_match: Option<(String,)> = sqlx::query_as(
        "SELECT path FROM wiki_pages WHERE project_id = ? AND status = 'active' AND LOWER(title) = LOWER(?) LIMIT 1"
    )
    .bind(project_id)
    .bind(link)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    if let Some((path,)) = title_match {
        return path;
    }
    // Fallback to slug-based normalization
    normalize_wikilink(link)
}
