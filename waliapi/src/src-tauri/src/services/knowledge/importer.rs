use super::models::ImportSourceInput;
use super::parser;
use super::processor;
use super::repository::KbRepository;
use sha2::Digest;
use sqlx::SqlitePool;
use std::path::PathBuf;
use crate::server::event_bridge::EventSink;

/// Import a Git repository: clone → filter → process files
pub async fn import_git_repo(
    pool: &SqlitePool,
    events: &EventSink,
    kb_id: &str,
    source_id: &str,
    input: &ImportSourceInput,
) -> Result<usize, String> {
    let repo_url = input
        .repo_url
        .as_ref()
        .ok_or("repo_url is required for git import")?;

    let branch = input.branch.as_deref().unwrap_or("main");
    let token = input.token.as_deref();

    // Clone repo to temp dir
    let temp_dir = std::env::temp_dir().join(format!("kb_import_{}", uuid::Uuid::new_v4()));
    let clone_url = if let Some(t) = token {
        // Insert token into URL for auth
        if repo_url.starts_with("https://") {
            repo_url.replacen("https://", &format!("https://{}@", t), 1)
        } else if repo_url.starts_with("http://") {
            repo_url.replacen("http://", &format!("http://{}@", t), 1)
        } else {
            repo_url.clone()
        }
    } else {
        repo_url.clone()
    };

    emit_import_progress(events, kb_id, source_id, 0, "Cloning repository...");

    let clone_result = tokio::process::Command::new("git")
        .args(&[
            "clone",
            "--depth",
            "1",
            "--branch",
            branch,
            &clone_url,
            temp_dir.to_str().unwrap(),
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to run git clone: {}", e))?;

    if !clone_result.status.success() {
        let err = String::from_utf8_lossy(&clone_result.stderr);
        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
        return Err(format!(
            "Git clone failed: {}",
            err.chars().take(500).collect::<String>()
        ));
    }

    // Process files from cloned repo
    let excluded_dirs = input.excluded_dirs.clone().unwrap_or_default();
    let included_files = input.included_files.clone().unwrap_or_default();
    let max_file_size = input.max_file_size.unwrap_or(1024 * 1024); // 1MB default

    let result = process_directory_files(
        pool,
        events,
        kb_id,
        source_id,
        &temp_dir,
        &excluded_dirs,
        &included_files,
        max_file_size,
        "git",
        Some(repo_url),
        None,
    )
    .await;

    // Clean up temp dir
    std::fs::remove_dir_all(&temp_dir).ok();

    result
}

/// Import from a URL: fetch content → process
pub async fn import_url(
    pool: &SqlitePool,
    events: &EventSink,
    kb_id: &str,
    source_id: &str,
    input: &ImportSourceInput,
) -> Result<usize, String> {
    let url = input.url.as_ref().ok_or("url is required for url import")?;

    emit_import_progress(events, kb_id, source_id, 0, "Fetching URL...");

    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch URL: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}: {}", resp.status(), url));
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("text/plain")
        .to_string();

    let content = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed to read body: {}", e))?;

    // Determine filename from URL
    let filename = url.rsplit('/').next().unwrap_or("imported").to_string();
    let filename = if filename.contains('.') {
        filename
    } else if content_type.contains("html") {
        format!("{}.html", filename)
    } else if content_type.contains("markdown") || content_type.contains("text/plain") {
        format!("{}.md", filename)
    } else {
        format!("{}.txt", filename)
    };

    let file_type = parser::get_file_type(&filename);

    // Create document record
    let repo = KbRepository::new(pool.clone());
    let hash = sha2::Sha256::digest(&content);
    let hash_hex = hex::encode(hash);

    // Check duplicate
    if let Ok(Some(_)) = repo.find_document_by_hash(kb_id, &hash_hex).await {
        emit_import_progress(events, kb_id, source_id, 100, "URL content already exists");
        return Ok(0);
    }

    let doc = repo
        .create_document_with_source(
            kb_id,
            &filename,
            None,
            &file_type,
            content.len() as i64,
            &hash_hex,
            "url",
            Some(url),
            None,
        )
        .await
        .map_err(|e| e.to_string())?;

    // Get KB embedding model
    let kb = repo.get_kb(kb_id).await.map_err(|e| e.to_string())?;
    let emb_model = kb.embedding_model.clone();

    emit_import_progress(events, kb_id, source_id, 30, "Processing document...");

    processor::process_document(
        pool,
        events,
        kb_id,
        &doc.id,
        &filename,
        &content,
        emb_model.as_deref(),
    )
    .await?;

    emit_import_progress(events, kb_id, source_id, 100, "URL import complete");
    Ok(1)
}

/// Import from a local directory: scan → filter → process files
pub async fn import_local_dir(
    pool: &SqlitePool,
    events: &EventSink,
    kb_id: &str,
    source_id: &str,
    input: &ImportSourceInput,
) -> Result<usize, String> {
    let dir_path = input
        .dir_path
        .as_ref()
        .ok_or("dir_path is required for local_dir import")?;

    let path = PathBuf::from(dir_path);
    if !path.is_dir() {
        return Err(format!("Directory not found: {}", dir_path));
    }

    let excluded_dirs = input.excluded_dirs.clone().unwrap_or_default();
    let included_files = input.included_files.clone().unwrap_or_default();
    let max_file_size = input.max_file_size.unwrap_or(1024 * 1024);

    process_directory_files(
        pool,
        events,
        kb_id,
        source_id,
        &path,
        &excluded_dirs,
        &included_files,
        max_file_size,
        "local_dir",
        None,
        Some(dir_path),
    )
    .await
}

/// Common: process all files in a directory with filtering
async fn process_directory_files(
    pool: &SqlitePool,
    events: &EventSink,
    kb_id: &str,
    source_id: &str,
    dir: &PathBuf,
    excluded_dirs: &[String],
    included_files: &[String],
    max_file_size: usize,
    source_type: &str,
    source_url: Option<&str>,
    _source_path: Option<&str>,
) -> Result<usize, String> {
    emit_import_progress(events, kb_id, source_id, 5, "Scanning directory...");

    let files = scan_directory(dir, excluded_dirs, included_files, max_file_size)?;

    if files.is_empty() {
        emit_import_progress(
            events,
            kb_id,
            source_id,
            100,
            "No files found matching criteria",
        );
        return Ok(0);
    }

    let total = files.len();
    let repo = KbRepository::new(pool.clone());
    let kb = repo.get_kb(kb_id).await.map_err(|e| e.to_string())?;
    let emb_model = kb.embedding_model.clone();

    let mut processed = 0usize;
    let mut skipped = 0usize;

    for (i, file_path) in files.iter().enumerate() {
        let filename = file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("file_{}", i));

        let pct = 10 + ((i as f64 / total as f64) * 80.0) as u8;
        emit_import_progress(
            events,
            kb_id,
            source_id,
            pct,
            &format!("Processing {}/{}: {}", i + 1, total, filename),
        );

        // Read file
        let content = match std::fs::read(file_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read file {}: {}", filename, e);
                skipped += 1;
                continue;
            }
        };

        // Hash
        let hash = sha2::Sha256::digest(&content);
        let hash_hex = hex::encode(hash);

        // Check duplicate
        if let Ok(Some(_)) = repo.find_document_by_hash(kb_id, &hash_hex).await {
            skipped += 1;
            continue;
        }

        let file_type = parser::get_file_type(&filename);
        let file_size = content.len() as i64;

        // Create document record with source info
        let rel_path = file_path
            .strip_prefix(dir)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        let doc = match repo
            .create_document_with_source(
                kb_id,
                &filename,
                Some(&file_path.to_string_lossy()),
                &file_type,
                file_size,
                &hash_hex,
                source_type,
                source_url,
                Some(&rel_path),
            )
            .await
        {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("Failed to create document record for {}: {}", filename, e);
                skipped += 1;
                continue;
            }
        };

        // Process document
        if let Err(e) = processor::process_document(
            pool,
            events,
            kb_id,
            &doc.id,
            &filename,
            &content,
            emb_model.as_deref(),
        )
        .await
        {
            tracing::warn!("Failed to process document {}: {}", filename, e);
            skipped += 1;
        } else {
            processed += 1;
        }
    }

    // Update KB counts
    repo.update_kb_counts(kb_id).await.ok();

    emit_import_progress(
        events,
        kb_id,
        source_id,
        100,
        &format!("Done: {} processed, {} skipped", processed, skipped),
    );
    Ok(processed)
}

/// Recursively scan directory, applying filters
fn scan_directory(
    dir: &PathBuf,
    excluded_dirs: &[String],
    included_files: &[String],
    max_file_size: usize,
) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();

    // Default excluded dirs
    let default_excluded = vec![
        ".git",
        ".svn",
        ".hg",
        "node_modules",
        "__pycache__",
        ".venv",
        "venv",
        "env",
        ".env",
        "dist",
        "build",
        "target",
        ".next",
        ".nuxt",
        ".output",
        "vendor",
        "vendor",
        ".idea",
        ".vscode",
    ];

    let mut all_excluded: Vec<&str> = default_excluded.iter().copied().collect();
    for d in excluded_dirs {
        all_excluded.push(d.as_str());
    }

    scan_directory_recursive(
        dir,
        &all_excluded,
        included_files,
        max_file_size,
        &mut files,
    )?;

    files.sort();
    Ok(files)
}

fn scan_directory_recursive(
    dir: &PathBuf,
    excluded_dirs: &[&str],
    included_files: &[String],
    max_file_size: usize,
    files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("Failed to read dir: {}", e))?;

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            // Check if excluded
            if excluded_dirs.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            scan_directory_recursive(&path, excluded_dirs, included_files, max_file_size, files)?;
        } else if path.is_file() {
            // Check file size
            if let Ok(meta) = entry.metadata() {
                if meta.len() as usize > max_file_size {
                    continue;
                }
            }

            // Check extension
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            // Supported extensions
            let supported = is_supported_extension(&ext);

            // If included_files is specified, only include matching files
            let included = if included_files.is_empty() {
                true
            } else {
                included_files.iter().any(|f| {
                    let f_lower = f.to_lowercase();
                    name.to_lowercase().contains(&f_lower) || f_lower == ext
                })
            };

            if supported && included {
                files.push(path);
            }
        }
    }

    Ok(())
}

fn is_supported_extension(ext: &str) -> bool {
    matches!(
        ext,
        "md" | "markdown"
            | "txt"
            | "rst"
            | "log"
            | "rs"
            | "go"
            | "py"
            | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "java"
            | "c"
            | "cpp"
            | "h"
            | "hpp"
            | "cs"
            | "php"
            | "swift"
            | "kt"
            | "rb"
            | "scala"
            | "clj"
            | "sh"
            | "bash"
            | "vue"
            | "svelte"
            | "sql"
            | "proto"
            | "gradle"
            | "json"
            | "yaml"
            | "yml"
            | "toml"
            | "xml"
            | "html"
            | "csv"
            | "env"
            | "ini"
            | "conf"
            | "cfg"
            | "svg"
            | "pdf"
    )
}

fn emit_import_progress(events: &EventSink, kb_id: &str, source_id: &str, progress: u8, detail: &str) {
    events.emit(
        "kb-import-progress",
        serde_json::json!({
            "kb_id": kb_id,
            "source_id": source_id,
            "progress": progress,
            "detail": detail,
        }),
    );
}
