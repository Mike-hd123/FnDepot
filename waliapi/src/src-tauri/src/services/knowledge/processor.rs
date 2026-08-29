use super::code_parser;
use super::embedder;
use super::parser;
use super::repository::{ChunkInsert, KbRepository};
use super::retriever;
use super::splitter;
use crate::db::models::now_iso;
use crate::db::repository::Repository;
use sqlx::SqlitePool;
use crate::server::event_bridge::EventSink;

/// Default embedding model
const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";

/// Emit progress event to frontend
fn emit_progress(
    events: &EventSink,
    doc_id: &str,
    kb_id: &str,
    filename: &str,
    stage: &str,
    progress: u8,
    detail: &str,
) {
    events.emit(
        "kb-document-progress",
        serde_json::json!({
            "doc_id": doc_id,
            "kb_id": kb_id,
            "filename": filename,
            "stage": stage,
            "progress": progress,
            "detail": detail,
        }),
    );
}

/// Process an uploaded document: parse → split → embed → store
pub async fn process_document(
    pool: &SqlitePool,
    events: &EventSink,
    kb_id: &str,
    doc_id: &str,
    filename: &str,
    content: &[u8],
    embedding_model: Option<&str>,
) -> Result<(), String> {
    let repo = KbRepository::new(pool.clone());

    // Update status to processing
    repo.update_document_status(doc_id, "processing", None)
        .await
        .map_err(|e| e.to_string())?;

    emit_progress(events, doc_id, kb_id, filename, "processing", 0, "开始处理");

    let result =
        process_document_inner(pool, events, kb_id, doc_id, filename, content, embedding_model).await;

    if let Err(ref e) = result {
        let err_msg = format!("文档「{}」处理失败: {}", filename, e);
        let _ = repo
            .update_document_status(doc_id, "failed", Some(&err_msg))
            .await;
        events.emit(
            "kb-document-error",
            serde_json::json!({
                "doc_id": doc_id,
                "kb_id": kb_id,
                "filename": filename,
                "error": e,
            }),
        );
    } else {
        emit_progress(events, doc_id, kb_id, filename, "done", 100, "处理完成");
    }

    result
}

async fn process_document_inner(
    pool: &SqlitePool,
    events: &EventSink,
    kb_id: &str,
    doc_id: &str,
    filename: &str,
    content: &[u8],
    embedding_model: Option<&str>,
) -> Result<(), String> {
    let repo = KbRepository::new(pool.clone());

    // 1. Parse file
    emit_progress(events, doc_id, kb_id, filename, "parsing", 5, "解析文件");
    let parsed = parser::parse_file(filename, content)?;

    let (text, file_type_label): (String, String) = match &parsed {
        parser::ParsedContent::PlainText(t) => (t.clone(), "text".to_string()),
        parser::ParsedContent::Markdown { text } => (text.clone(), "markdown".to_string()),
        parser::ParsedContent::Code { text, language } => (text.clone(), language.clone()),
        parser::ParsedContent::Structured(t) => (t.clone(), "structured".to_string()),
    };

    // 2. Split into chunks — use KB-level config if available
    emit_progress(events, doc_id, kb_id, filename, "splitting", 15, "文本分块");
    let kb = repo.get_kb(kb_id).await.map_err(|e| e.to_string())?;
    let config = splitter::SplitConfig {
        chunk_size: if kb.chunk_size > 0 {
            kb.chunk_size as usize
        } else {
            512
        },
        chunk_overlap: if kb.chunk_overlap > 0 {
            kb.chunk_overlap as usize
        } else {
            64
        },
    };
    let base_metadata = splitter::ChunkMetadata {
        file_path: Some(filename.to_string()),
        ..Default::default()
    };

    // 符号感知分块：代码文件且语言受支持时，按 AST 符号边界切分
    // 提前处理代码符号和进度更新（在 catch_unwind 外部，避免传递 EventSink）
    let chunks = if let parser::ParsedContent::Code { text, language } = &parsed {
        if code_parser::is_supported_language(language) {
            let symbols = code_parser::extract_symbols(filename, text);
            emit_progress(
                events,
                doc_id,
                kb_id,
                filename,
                "splitting",
                18,
                &format!("AST 解析：提取到 {} 个符号", symbols.len()),
            );
            // 使用 catch_unwind 保护分块操作
            std::panic::catch_unwind({
                let text = text.clone();
                let symbols = symbols.clone();
                let config = config.clone();
                let base_metadata = base_metadata.clone();
                move || splitter::split_code_by_symbols(&text, &symbols, &config, &base_metadata)
            })
            .map_err(|_| "文本分块过程发生严重错误".to_string())?
        } else {
            std::panic::catch_unwind({
                let text = text.clone();
                let file_type_label = file_type_label.clone();
                let config = config.clone();
                let base_metadata = base_metadata.clone();
                move || splitter::split(&text, &file_type_label, &config, &base_metadata)
            })
            .map_err(|_| "文本分块过程发生严重错误".to_string())?
        }
    } else {
        std::panic::catch_unwind({
            let text = text.clone();
            let file_type_label = file_type_label.clone();
            let config = config.clone();
            let base_metadata = base_metadata.clone();
            move || splitter::split(&text, &file_type_label, &config, &base_metadata)
        })
        .map_err(|_| "文本分块过程发生严重错误".to_string())?
    };

    if chunks.is_empty() {
        // 分块后为空状态改为失败,且失败信息给客户端提示
        events.emit(
            "kb-document-error",
            serde_json::json!({
                "doc_id": doc_id,
                "kb_id": kb_id,
                "filename": filename,
                "error": "分块后为空，无法继续处理".to_string(),
            }),
        );
        repo.update_document_status(doc_id, "failed", None)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    let total_chunks = chunks.len() as i64;
    let total_tokens: i64 = chunks.iter().map(|c| c.token_count as i64).sum();

    // 3. Embed chunks in batches
    let emb_model = embedding_model.unwrap_or(DEFAULT_EMBEDDING_MODEL);
    let main_repo = Repository::new(pool.clone());

    // Detect expected embedding dimension from KB config
    let expected_dim = if kb.embedding_dim > 0 {
        Some(kb.embedding_dim as usize)
    } else {
        None
    };

    let batch_size = if kb.embedding_batch_size > 0 {
        kb.embedding_batch_size as usize
    } else {
        32
    };
    println!("embedding_batch_size: {}", batch_size);
    let total_batches = ((chunks.len() as f64) / batch_size as f64).ceil() as usize;
    let mut all_embeddings: Vec<Vec<f32>> = Vec::with_capacity(chunks.len());
    let mut batch_done = 0usize;

    for batch in chunks.chunks(batch_size) {
        let texts: Vec<String> = batch.iter().map(|c| c.content.clone()).collect();
        let embeddings = embedder::embed(&texts, emb_model, &main_repo).await?;

        // Validate embedding dimensions
        if let Some(dim) = expected_dim {
            for (i, emb) in embeddings.iter().enumerate() {
                if emb.len() != dim {
                    tracing::warn!(
                        "Embedding dim mismatch in batch {}: expected {}, got {} (chunk {})",
                        batch_done,
                        dim,
                        emb.len(),
                        i
                    );
                }
            }
        }

        all_embeddings.extend(embeddings);
        batch_done += 1;
        // Embedding progress: 20% ~ 80%
        let pct = 20 + ((batch_done as f64 / total_batches as f64) * 60.0) as u8;
        emit_progress(
            events,
            doc_id,
            kb_id,
            filename,
            "embedding",
            pct,
            &format!("向量化 {}/{}", batch_done, total_batches),
        );
    }

    // Auto-detect and update KB embedding dimension if not set
    if expected_dim.is_none() && !all_embeddings.is_empty() {
        let detected_dim = all_embeddings[0].len() as i64;
        tracing::info!(
            "Auto-detected embedding dim {} for KB {}",
            detected_dim,
            kb_id
        );
        repo.update_kb_embedding_dim(kb_id, detected_dim).await.ok();
    }

    // 4. Store chunks with embeddings
    let chunks_total = chunks.len();
    for (i, chunk) in chunks.iter().enumerate() {
        // Storing progress: 80% ~ 95%
        if i % 10 == 0 || i == chunks_total - 1 {
            let pct = 80 + ((i as f64 + 1.0) / chunks_total as f64 * 15.0) as u8;
            emit_progress(
                events,
                doc_id,
                kb_id,
                filename,
                "storing",
                pct,
                &format!("存储切片 {}/{}", i + 1, chunks_total),
            );
        }
        let embedding_bytes = retriever::encode_embedding(&all_embeddings[i]);
        let chunk_insert = ChunkInsert {
            id: uuid::Uuid::new_v4().to_string(),
            doc_id: doc_id.to_string(),
            kb_id: kb_id.to_string(),
            chunk_index: i as i64,
            content: chunk.content.clone(),
            token_count: chunk.token_count as i64,
            embedding: embedding_bytes,
            embedding_dim: all_embeddings[i].len() as i64,
            metadata: serde_json::to_string(&chunk.metadata).unwrap_or_else(|_| "{}".to_string()),
            created_at: now_iso(),
        };
        repo.create_chunk(&chunk_insert)
            .await
            .map_err(|e| e.to_string())?;
    }

    // 5. Update document and KB counts
    emit_progress(events, doc_id, kb_id, filename, "finalizing", 98, "更新统计");
    repo.update_document_counts(doc_id, total_chunks, total_tokens)
        .await
        .map_err(|e| e.to_string())?;
    repo.update_document_status(doc_id, "ready", None)
        .await
        .map_err(|e| e.to_string())?;
    repo.update_kb_counts(kb_id)
        .await
        .map_err(|e| e.to_string())?;

    // 6. Rebuild HNSW index (best-effort, non-blocking on failure)
    emit_progress(events, doc_id, kb_id, filename, "indexing", 99, "更新向量索引");
    let pool_clone = pool.clone();
    let kb_id_clone = kb_id.to_string();
    let events_clone = events.clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            if let Err(e) = retriever::build_index(&pool_clone, &kb_id_clone, &events_clone).await {
                tracing::warn!(
                    "Failed to rebuild HNSW index for KB {} after doc: {}",
                    kb_id_clone,
                    e
                );
                events_clone.emit(
                    "kb-index-progress",
                    serde_json::json!({
                        "kb_id": &kb_id_clone,
                        "status": "error",
                        "message": format!("索引构建失败: {}", e)
                    }),
                );
            } else {
                events_clone.emit(
                    "kb-index-progress",
                    serde_json::json!({
                        "kb_id": &kb_id_clone,
                        "status": "ready",
                        "message": "索引构建完成"
                    }),
                );
            }
        });
    });

    Ok(())
}

/// Reindex a document (delete old chunks, reprocess)
pub async fn reindex_document(
    pool: &SqlitePool,
    events: &EventSink,
    doc_id: &str,
) -> Result<(), String> {
    let repo = KbRepository::new(pool.clone());
    let doc = repo.get_document(doc_id).await.map_err(|e| e.to_string())?;

    // Delete existing chunks
    repo.delete_chunks_by_doc(doc_id)
        .await
        .map_err(|e| e.to_string())?;

    // Read file content from path
    let content = if let Some(path) = &doc.file_path {
        std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?
    } else {
        return Err("No file path to reindex".to_string());
    };

    // Get KB for embedding model
    let kb = repo.get_kb(&doc.kb_id).await.map_err(|e| e.to_string())?;

    process_document(
        pool,
        events,
        &doc.kb_id,
        doc_id,
        &doc.filename,
        &content,
        kb.embedding_model.as_deref(),
    )
    .await
}
