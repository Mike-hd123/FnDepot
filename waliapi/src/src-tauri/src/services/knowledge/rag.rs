use super::embedder;
use super::models::{ConversationMessage, RagAnswer, RetrievalDetail, SourceInfo, UsageInfo};
use super::repository::KbRepository;
use super::retriever;
use crate::core::proxy;
use crate::db::repository::Repository;
use crate::settings_store::SettingsStore;
use sqlx::SqlitePool;
use std::sync::Arc;

/// RAG: Retrieve relevant chunks, then generate answer via WaLiAPI proxy
/// Enhanced with conversation history, token limit fallback, and configurable search modes.
pub async fn ask(
    pool: &SqlitePool,
    kb_id: &str,
    query: &str,
    embedding_model: &str,
    chat_model: &str,
    top_k: usize,
    mcp_only: bool,
    history: &[ConversationMessage],
    settings: &SettingsStore,
) -> Result<RagAnswer, String> {
    ask_with_config(
        pool,
        kb_id,
        query,
        embedding_model,
        chat_model,
        top_k,
        mcp_only,
        history,
        settings,
        0.7,
        0.3,
        "hybrid",
    )
    .await
}

/// RAG with configurable search parameters.
pub async fn ask_with_config(
    pool: &SqlitePool,
    kb_id: &str,
    query: &str,
    embedding_model: &str,
    chat_model: &str,
    top_k: usize,
    mcp_only: bool,
    history: &[ConversationMessage],
    settings: &SettingsStore,
    vector_weight: f32,
    keyword_weight: f32,
    search_mode: &str,
) -> Result<RagAnswer, String> {
    let repo = Repository::new(pool.clone());
    let kb_repo = KbRepository::new(pool.clone());

    // 1. Embed the query (needed for vector and hybrid modes)
    let query_emb_opt = if search_mode != "keyword" {
        let embeddings = embedder::embed(&[query.to_string()], embedding_model, &repo)
            .await
            .map_err(|e| format!("Embedding failed: {}", e))?;
        if embeddings.is_empty() {
            return Err("Failed to embed query".to_string());
        }
        Some(embeddings[0].clone())
    } else {
        None
    };

    // 2. Search based on mode
    let scored_results = if search_mode == "keyword" {
        // Keyword-only search
        let kw_results = if kb_id.is_empty() {
            // For search_all with keyword mode, we still need embeddings for cross-KB search
            // Fallback: embed and use hybrid
            let embeddings = embedder::embed(&[query.to_string()], embedding_model, &repo)
                .await
                .map_err(|e| format!("Embedding failed: {}", e))?;
            retriever::hybrid_search_with_details(
                pool,
                kb_id,
                query,
                &embeddings[0],
                top_k,
                vector_weight,
                keyword_weight,
            )
            .await?
        } else {
            let kw = retriever::keyword_only_search(pool, kb_id, query, top_k).await?;
            kw.into_iter()
                .map(|r| {
                    let score = r.score;
                    retriever::ScoredSearchResult {
                        result: r,
                        vector_score: None,
                        keyword_score: Some(score),
                    }
                })
                .collect()
        };
        kw_results
    } else if search_mode == "vector" {
        // Vector-only search
        let query_emb = query_emb_opt
            .as_ref()
            .ok_or("Embedding required for vector search")?;
        let v_results = if kb_id.is_empty() {
            retriever::search_all(pool, query_emb, top_k, mcp_only).await?
        } else {
            retriever::search(pool, kb_id, query_emb, top_k).await?
        };
        v_results
            .into_iter()
            .map(|r| {
                let score = r.score;
                retriever::ScoredSearchResult {
                    result: r,
                    vector_score: Some(score),
                    keyword_score: None,
                }
            })
            .collect()
    } else {
        // Hybrid search (default)
        let query_emb = query_emb_opt
            .as_ref()
            .ok_or("Embedding required for hybrid search")?;
        if kb_id.is_empty() {
            // Cross-KB: use search_all then compute details
            let results = retriever::search_all(pool, query_emb, top_k, mcp_only).await?;
            results
                .into_iter()
                .map(|r| {
                    let score = r.score;
                    retriever::ScoredSearchResult {
                        result: r,
                        vector_score: Some(score),
                        keyword_score: None,
                    }
                })
                .collect()
        } else {
            retriever::hybrid_search_with_details(
                pool,
                kb_id,
                query,
                query_emb,
                top_k,
                vector_weight,
                keyword_weight,
            )
            .await?
        }
    };

    // Extract plain results for context building
    let results: Vec<super::models::SearchResult> =
        scored_results.iter().map(|s| s.result.clone()).collect();

    if results.is_empty() {
        // Save to conversation history
        if !kb_id.is_empty() {
            let answer = "RAG 中没有找到相关内容。".to_string();
            kb_repo
                .add_conversation(kb_id, "user", query, None, Some(chat_model), 0)
                .await
                .ok();
            kb_repo
                .add_conversation(kb_id, "assistant", &answer, None, Some(chat_model), 0)
                .await
                .ok();
            return Ok(RagAnswer {
                answer,
                sources: vec![],
                usage: None,
                retrieval_details: Some(vec![]),
            });
        }
        return Ok(RagAnswer {
            answer: "RAG 中没有找到相关内容。".to_string(),
            sources: vec![],
            usage: None,
            retrieval_details: Some(vec![]),
        });
    }

    // 3. Build context
    let context = build_context(&results);

    // 4. Build prompt with history
    let prompt = build_rag_prompt(&context, query, history);

    // 5. Token estimation and fallback
    let estimated_tokens = retriever::estimate_tokens(&prompt);
    let model_limit = retriever::get_model_context_limit(chat_model);
    let context_limit = (model_limit as f64 * 0.7) as usize; // Reserve 30% for response

    let (final_prompt, context_used) = if estimated_tokens > context_limit {
        // Stage 1: Trim context (remove lowest-scoring chunks)
        let trimmed = trim_context(&results, query, history, context_limit);
        if retriever::estimate_tokens(&trimmed.0) > context_limit {
            // Stage 2: Remove history, keep only latest message
            let no_history =
                build_rag_prompt(&context, query, &history[history.len().saturating_sub(2)..]);
            if retriever::estimate_tokens(&no_history) > context_limit {
                // Stage 3: Remove context entirely
                let bare = format!(
                    "注意：由于 token 限制，无法附上 RAG 上下文。\n\n问题: {}",
                    query
                );
                (bare, false)
            } else {
                (no_history, true)
            }
        } else {
            trimmed
        }
    } else {
        (prompt, true)
    };

    tracing::info!(
        "RAG prompt: estimated {} tokens, limit {}, context_used: {}",
        estimated_tokens,
        context_limit,
        context_used
    );

    // 6. Call LLM via proxy
    let chat_request = serde_json::json!({
        "model": chat_model,
        "messages": [
            {"role": "system", "content": "你是 RAG 助手。基于检索到的内容回答问题。回答要准确、简洁，并标注信息来源。如果没有相关信息，请明确说明。"},
            {"role": "user", "content": final_prompt}
        ],
        "stream": false
    });
    let chat_request_str: String = serde_json::to_string(&chat_request).unwrap_or_default();
    let proxy_result = proxy::handle_request(
        &Arc::new(repo),
        settings,
        "kb-internal",
        "RAG",
        chat_request,
        false,
        Some(chat_request_str),
        Some(format!("kb-internal_{}", kb_id)),
        None, // legacy RAG path: no security gate yet — proxy scans internally
    )
    .await;

    match proxy_result {
        Ok(result) => {
            let answer = result
                .body
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or("生成回答失败")
                .to_string();

            let usage = result.usage.map(|u| UsageInfo {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            });

            let sources: Vec<SourceInfo> = results
                .iter()
                .map(|r| SourceInfo {
                    filename: r.filename.clone(),
                    score: r.score,
                    snippet: r.content.chars().take(200).collect(),
                })
                .collect();

            // Build retrieval details for visualization
            let retrieval_details: Vec<RetrievalDetail> = scored_results
                .iter()
                .map(|s| {
                    let meta = &s.result.metadata;
                    RetrievalDetail {
                        chunk_id: s.result.chunk_id.clone(),
                        filename: s.result.filename.clone(),
                        score: s.result.score,
                        vector_score: s.vector_score,
                        keyword_score: s.keyword_score,
                        snippet: s.result.content.chars().take(200).collect(),
                        symbol_name: meta
                            .get("symbol_name")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        symbol_kind: meta
                            .get("symbol_kind")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                    }
                })
                .collect();

            // Save to conversation history
            if !kb_id.is_empty() {
                let sources_json = serde_json::to_string(&sources).ok();
                let tokens = usage.as_ref().map(|u| u.total_tokens as i64).unwrap_or(0);
                kb_repo
                    .add_conversation(kb_id, "user", query, None, Some(chat_model), 0)
                    .await
                    .ok();
                kb_repo
                    .add_conversation(
                        kb_id,
                        "assistant",
                        &answer,
                        sources_json.as_deref(),
                        Some(chat_model),
                        tokens,
                    )
                    .await
                    .ok();
            }

            Ok(RagAnswer {
                answer,
                sources,
                usage,
                retrieval_details: Some(retrieval_details),
            })
        }
        Err((code, msg)) => Err(format!("LLM request failed ({}): {}", code, msg)),
    }
}

/// Build context string from search results
/// Enhanced with symbol metadata (name, kind, signature)
fn build_context(results: &[super::models::SearchResult]) -> String {
    results
        .iter()
        .enumerate()
        .map(|(i, r)| {
            // 从 metadata 中提取符号信息
            let symbol_info = r
                .metadata
                .get("symbol_name")
                .and_then(|n| n.as_str())
                .map(|name| {
                    let kind = r
                        .metadata
                        .get("symbol_kind")
                        .and_then(|k| k.as_str())
                        .unwrap_or("");
                    let sig = r
                        .metadata
                        .get("signature")
                        .and_then(|s| s.as_str())
                        .unwrap_or("");
                    if sig.is_empty() {
                        format!(" [{}: {}]", kind, name)
                    } else {
                        format!(" [{}: {} {}]", kind, name, sig)
                    }
                })
                .unwrap_or_default();

            format!(
                "--- 文档 {} [{}] (相似度: {:.2}){} ---\n{}",
                i + 1,
                r.filename,
                r.score,
                symbol_info,
                r.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Build RAG prompt with conversation history
fn build_rag_prompt(context: &str, query: &str, history: &[ConversationMessage]) -> String {
    let history_str = if history.is_empty() {
        String::new()
    } else {
        let h: String = history
            .iter()
            .map(|msg| match msg.role.as_str() {
                "user" => format!("User: {}", msg.content),
                "assistant" => format!("Assistant: {}", msg.content),
                _ => msg.content.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        format!("<conversation_history>\n{}\n</conversation_history>\n\n", h)
    };

    format!(
        r#"基于以下 RAG 内容回答问题。如果没有相关信息，请明确说明。

规则：
1. 只基于 RAG 内容回答，不要编造信息
2. 如果是多轮对话，注意上下文连贯性
3. 回答要准确、简洁，标注信息来源

{history}<knowledge_base>
{context}
</knowledge_base>

问题: {query}
"#,
        history = history_str,
        context = context,
        query = query,
    )
}

/// Trim context to fit token limit (remove lowest-scoring chunks first)
fn trim_context(
    results: &[super::models::SearchResult],
    query: &str,
    history: &[ConversationMessage],
    target_tokens: usize,
) -> (String, bool) {
    // Sort by score ascending (remove lowest first)
    let mut indexed: Vec<(usize, &super::models::SearchResult)> =
        results.iter().enumerate().collect();
    indexed.sort_by(|a, b| {
        a.1.score
            .partial_cmp(&b.1.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut removed = std::collections::HashSet::new();
    let mut current_estimate =
        retriever::estimate_tokens(&build_rag_prompt(&build_context(results), query, history));

    for (idx, r) in &indexed {
        if current_estimate <= target_tokens {
            break;
        }
        removed.insert(*idx);
        current_estimate = current_estimate.saturating_sub(retriever::estimate_tokens(&r.content));
    }

    // Rebuild context without removed chunks
    let remaining: Vec<_> = results
        .iter()
        .enumerate()
        .filter(|(i, _)| !removed.contains(i))
        .map(|(_, r)| r.clone())
        .collect();

    let new_context = build_context(&remaining);
    let new_prompt = build_rag_prompt(&new_context, query, history);
    (new_prompt, !removed.is_empty())
}

/// Deep Research: multi-round iterative retrieval and analysis
pub async fn deep_research(
    pool: &SqlitePool,
    kb_id: &str,
    query: &str,
    embedding_model: &str,
    chat_model: &str,
    top_k: usize,
    max_rounds: usize,
    settings: &SettingsStore,
) -> Result<RagAnswer, String> {
    let repo = Repository::new(pool.clone());
    let kb_repo = KbRepository::new(pool.clone());

    let mut all_findings: Vec<String> = Vec::new();
    let mut history: Vec<ConversationMessage> = Vec::new();
    let mut all_sources: Vec<super::models::SearchResult> = Vec::new();

    for round in 0..max_rounds {
        // 1. Generate query for this round
        let round_query = if round == 0 {
            query.to_string()
        } else {
            // Ask LLM to generate a follow-up query based on findings so far
            let follow_up_prompt = format!(
                r#"基于原始问题和已有发现，生成一个简短的追问查询（只需要返回查询本身，不需要解释）。

原始问题: {query}

已有发现:
{findings}

请生成下一步需要搜索的关键词或问题（直接返回查询文本，不要加引号或其他格式）:"#,
                query = query,
                findings = all_findings
                    .iter()
                    .enumerate()
                    .map(|(i, f)| format!("第{}轮: {}", i + 1, f))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );

            let follow_up_request = serde_json::json!({
                "model": chat_model,
                "messages": [
                    {"role": "system", "content": "你是一个研究助手，根据已有发现生成下一步搜索查询。只返回查询本身。"},
                    {"role": "user", "content": follow_up_prompt}
                ],
                "stream": false
            });

            let follow_up_request_str: String =
                serde_json::to_string(&follow_up_request).unwrap_or_default();
            match proxy::handle_request(
                &Arc::new(Repository::new(pool.clone())),
                settings,
                "kb-research",
                "深度研究",
                follow_up_request,
                false,
                Some(follow_up_request_str),
                Some(format!("kb-research_{}", kb_id)),
                None, // legacy RAG path: no security gate yet — proxy scans internally
            )
            .await
            {
                Ok(result) => result
                    .body
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                    .unwrap_or(query)
                    .trim()
                    .to_string(),
                Err(_) => query.to_string(),
            }
        };

        // 2. Embed and search
        let embeddings = embedder::embed(&[round_query.clone()], embedding_model, &repo)
            .await
            .map_err(|e| format!("Embedding failed: {}", e))?;

        if embeddings.is_empty() {
            break;
        }

        let results = retriever::search(pool, kb_id, &embeddings[0], top_k)
            .await
            .unwrap_or_default();

        if results.is_empty() && round > 0 {
            break; // No more relevant content found
        }

        all_sources.extend(results.clone());

        // 3. Generate round answer
        let context = build_context(&results);
        let findings_str = all_findings
            .iter()
            .enumerate()
            .map(|(i, f)| format!("第{}轮发现: {}", i + 1, f))
            .collect::<Vec<_>>()
            .join("\n");

        let round_prompt = if round == 0 {
            format!(
                r#"你是一个深度研究助手。请分析以下 RAG 内容，并给出初步发现。

原始问题: {query}

<knowledge_base>
{context}
</knowledge_base>

请完成：
1. 理解问题的核心需求
2. 从 RAG 中提取相关信息
3. 给出初步发现
4. 如果信息不足，指出还需要哪些方面"#,
                query = query,
                context = context,
            )
        } else {
            format!(
                r#"继续深度研究。

原始问题: {query}

已有发现:
{findings}

新检索到的内容:
<knowledge_base>
{context}
</knowledge_base>

请完成：
1. 分析新内容与已有发现的关系
2. 补充或修正之前的发现
3. 指出是否需要继续研究"#,
                query = query,
                findings = findings_str,
                context = context,
            )
        };

        let chat_request = serde_json::json!({
            "model": chat_model,
            "messages": [
                {"role": "system", "content": "你是深度研究助手。基于 RAG 内容进行多轮迭代研究，逐步深入分析。"},
                {"role": "user", "content": round_prompt}
            ],
            "stream": false
        });

        let chat_request_str: String = serde_json::to_string(&chat_request).unwrap_or_default();
        let proxy_result = proxy::handle_request(
            &Arc::new(Repository::new(pool.clone())),
            settings,
            "kb-research",
            "深度研究",
            chat_request,
            false,
            Some(chat_request_str),
            Some(format!("kb-research_{}", kb_id)),
            None, // legacy RAG path: no security gate yet — proxy scans internally
        )
        .await;

        let round_answer = match proxy_result {
            Ok(result) => result
                .body
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or("分析失败")
                .to_string(),
            Err((code, msg)) => {
                tracing::warn!("Deep research round {} failed: {} {}", round + 1, code, msg);
                break;
            }
        };
        all_findings.push(round_answer.clone());
        history.push(ConversationMessage {
            role: "user".into(),
            content: round_query,
        });
        history.push(ConversationMessage {
            role: "assistant".into(),
            content: round_answer,
        });

        // Check if we have enough info (after round 2)
        if round >= 2 && round == max_rounds - 1 {
            break;
        }
    }

    // Final synthesis
    let findings_summary = all_findings
        .iter()
        .enumerate()
        .map(|(i, f)| format!("### 第{}轮发现\n{}", i + 1, f))
        .collect::<Vec<_>>()
        .join("\n\n");

    let final_prompt = format!(
        r#"基于多轮深度研究的发现，请综合回答原始问题。

原始问题: {query}

多轮研究发现:
{findings}

请综合所有发现，给出完整、准确的回答。标注信息来源。"#,
        query = query,
        findings = findings_summary,
    );

    let final_request = serde_json::json!({
        "model": chat_model,
        "messages": [
            {"role": "system", "content": "你是深度研究助手。综合多轮研究发现，给出完整准确的回答。"},
            {"role": "user", "content": final_prompt}
        ],
        "stream": false
    });

    let final_request_str: String = serde_json::to_string(&final_request).unwrap_or_default();
    let proxy_result = proxy::handle_request(
        &Arc::new(repo),
        settings,
        "kb-research",
        "深度研究",
        final_request,
        false,
        Some(final_request_str),
        Some(format!("kb-research_{}", kb_id)),
        None, // legacy RAG path: no security gate yet — proxy scans internally
    )
    .await;

    match proxy_result {
        Ok(result) => {
            let answer = result
                .body
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or("综合分析失败")
                .to_string();

            let usage = result.usage.map(|u| UsageInfo {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            });

            // Deduplicate sources
            let mut seen = std::collections::HashSet::new();
            let sources: Vec<SourceInfo> = all_sources
                .iter()
                .filter(|r| seen.insert(r.chunk_id.clone()))
                .map(|r| SourceInfo {
                    filename: r.filename.clone(),
                    score: r.score,
                    snippet: r.content.chars().take(200).collect(),
                })
                .collect();

            // Save to conversation history
            if !kb_id.is_empty() {
                let sources_json = serde_json::to_string(&sources).ok();
                let tokens = usage.as_ref().map(|u| u.total_tokens as i64).unwrap_or(0);
                kb_repo
                    .add_conversation(kb_id, "user", query, None, Some(chat_model), 0)
                    .await
                    .ok();
                kb_repo
                    .add_conversation(
                        kb_id,
                        "assistant",
                        &answer,
                        sources_json.as_deref(),
                        Some(chat_model),
                        tokens,
                    )
                    .await
                    .ok();
            }

            Ok(RagAnswer {
                answer,
                sources,
                usage,
                retrieval_details: None,
            })
        }
        Err((code, msg)) => Err(format!("Final synthesis failed ({}): {}", code, msg)),
    }
}
