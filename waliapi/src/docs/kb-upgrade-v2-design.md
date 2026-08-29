# WaLiAPI 知识库升级方案 v2

> 基于现有架构 + DeepWiki 核心设计借鉴，涵盖多源录入、向量索引、对话历史、Token 降级、Deep Research 模式。

---

## 一、总体架构变更

```
┌─────────────────────────────────────────────────────────┐
│                    前端 (React + Tauri)                   │
│  KnowledgeBasePage                                       │
│  ├── 文档Tab   ← 本地上传 / URL抓取 / Git导入            │
│  ├── 检索Tab   ← 向量检索 + 过滤                         │
│  ├── 问答Tab   ← 多轮对话 + Deep Research 模式           │
│  ├── 设置Tab   ← 分块配置 / 过滤规则 / Embedding配置     │
│  └── MCP Tab   ← 不变                                    │
├─────────────────────────────────────────────────────────┤
│                    后端 (Rust + Axum)                     │
│  services/knowledge/                                     │
│  ├── mod.rs          ← 服务注册（不变）                    │
│  ├── models.rs       ← 新增模型定义                       │
│  ├── repository.rs   ← 新增 CRUD 方法                    │
│  ├── parser.rs       ← 新增 URL/HTML 解析                │
│  ├── splitter.rs     ← 增强：配置化 + 语言感知            │
│  ├── embedder.rs     ← 不变（渠道复用已够好）              │
│  ├── retriever.rs    ← 重写：HNSW 索引 + 维度校验         │
│  ├── processor.rs    ← 增强：多源处理 + 增量索引          │
│  ├── rag.rs          ← 重写：对话历史 + Token降级 + 多轮   │
│  ├── importer.rs     ← 新增：Git/URL/本地目录导入         │
│  ├── handlers.rs     ← 新增 API 端点                     │
│  └── routes.rs       ← 新增路由                          │
├─────────────────────────────────────────────────────────┤
│                    SQLite 数据库                          │
│  kb_knowledge_bases  ← 新增字段                          │
│  kb_documents        ← 新增字段                          │
│  kb_chunks           ← 不变                              │
│  kb_tasks            ← 不变                              │
│  kb_conversations    ← 新增表：对话历史                   │
│  kb_sources          ← 新增表：多源记录                   │
│  kb_index_meta       ← 新增表：索引元数据                 │
└─────────────────────────────────────────────────────────┘
```

---

## 二、数据库变更

### 2.1 迁移脚本：`010_kb_upgrade.sql`

```sql
-- ═══════════════════════════════════════════════════════
-- 知识库表：新增分块/过滤配置
-- ═══════════════════════════════════════════════════════
ALTER TABLE kb_knowledge_bases ADD COLUMN chunk_size INTEGER NOT NULL DEFAULT 512;
ALTER TABLE kb_knowledge_bases ADD COLUMN chunk_overlap INTEGER NOT NULL DEFAULT 64;
ALTER TABLE kb_knowledge_bases ADD COLUMN excluded_dirs TEXT NOT NULL DEFAULT '';
ALTER TABLE kb_knowledge_bases ADD COLUMN excluded_files TEXT NOT NULL DEFAULT '';
ALTER TABLE kb_knowledge_bases ADD COLUMN included_files TEXT NOT NULL DEFAULT '';
ALTER TABLE kb_knowledge_bases ADD COLUMN embedding_dim INTEGER NOT NULL DEFAULT 0;
ALTER TABLE kb_knowledge_bases ADD COLUMN index_status TEXT NOT NULL DEFAULT 'none';
-- index_status: none | building | ready | stale | error

-- ═══════════════════════════════════════════════════════
-- 文档表：新增来源信息
-- ═══════════════════════════════════════════════════════
ALTER TABLE kb_documents ADD COLUMN source_type TEXT NOT NULL DEFAULT 'upload';
-- source_type: upload | git | url | local_dir
ALTER TABLE kb_documents ADD COLUMN source_url TEXT;
ALTER TABLE kb_documents ADD COLUMN source_path TEXT;
ALTER TABLE kb_documents ADD COLUMN doc_meta TEXT NOT NULL DEFAULT '{}';
-- doc_meta: JSON，存储语言、行数、原始路径等额外信息

-- ═══════════════════════════════════════════════════════
-- 新增：对话历史表
-- ═══════════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS kb_conversations (
    id           TEXT PRIMARY KEY,
    kb_id        TEXT NOT NULL,
    role         TEXT NOT NULL,  -- user | assistant
    content      TEXT NOT NULL,
    sources      TEXT,           -- JSON array of sources (assistant messages)
    model        TEXT,
    tokens_used  INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT NOT NULL,
    FOREIGN KEY (kb_id) REFERENCES kb_knowledge_bases(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_conversations_kb ON kb_conversations(kb_id, created_at);

-- ═══════════════════════════════════════════════════════
-- 新增：导入源记录表
-- ═══════════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS kb_sources (
    id           TEXT PRIMARY KEY,
    kb_id        TEXT NOT NULL,
    source_type  TEXT NOT NULL,  -- git | url | local_dir
    source_url   TEXT,           -- git repo URL or web URL
    source_path  TEXT,           -- local path
    branch       TEXT,           -- git branch (optional)
    status       TEXT NOT NULL DEFAULT 'pending',
    -- pending | fetching | parsing | done | error
    file_count   INTEGER NOT NULL DEFAULT 0,
    error        TEXT,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    FOREIGN KEY (kb_id) REFERENCES kb_knowledge_bases(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_sources_kb ON kb_sources(kb_id);

-- ═══════════════════════════════════════════════════════
-- 新增：索引元数据表
-- ═══════════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS kb_index_meta (
    kb_id        TEXT PRIMARY KEY,
    index_type   TEXT NOT NULL DEFAULT 'hnsw',
    embedding_dim INTEGER NOT NULL DEFAULT 0,
    chunk_count  INTEGER NOT NULL DEFAULT 0,
    index_path   TEXT,           -- HNSW 索引文件路径
    built_at     TEXT,
    status       TEXT NOT NULL DEFAULT 'none',
    FOREIGN KEY (kb_id) REFERENCES kb_knowledge_bases(id) ON DELETE CASCADE
);
```

---

## 三、模块详细设计

### 3.1 多源录入 — `importer.rs`（新增）

#### 3.1.1 Git 仓库导入

```rust
pub struct GitImportConfig {
    pub repo_url: String,        // https://github.com/owner/repo
    pub branch: Option<String>,  // 默认 main/master
    pub token: Option<String>,   // 私有仓库访问令牌
    pub depth: u32,              // clone 深度，默认 1
    pub excluded_dirs: Vec<String>,  // node_modules, .git, dist, build...
    pub excluded_files: Vec<String>, // *.lock, *.log...
    pub included_files: Vec<String>, // *.md, *.rs, *.py... (空=全部)
    pub max_file_size: usize,    // 默认 1MB，跳过大文件
}

pub async fn import_git_repo(
    pool: &SqlitePool,
    app: &AppHandle,
    kb_id: &str,
    config: &GitImportConfig,
) -> Result<KbSource, String> {
    // 1. 创建 kb_sources 记录，状态 = fetching
    // 2. git clone --depth=1 到临时目录
    //    - 使用 std::process::Command 调用系统 git
    //    - 或嵌入 gix crate（纯 Rust git 实现）
    // 3. 遍历仓库文件，应用过滤规则
    // 4. 逐文件创建 kb_documents 记录
    // 5. 异步处理每个文档（parse → split → embed → store）
    // 6. 更新 kb_sources 状态 = done
    // 7. 全部完成后触发索引构建
}
```

**文件遍历逻辑**：
```rust
fn collect_files(
    root: &Path,
    config: &GitImportConfig,
) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !is_excluded(e, config))
    {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            let path = entry.path();
            let rel = path.strip_prefix(root).unwrap();
            // 检查文件扩展名
            if is_included(rel, config) && entry.metadata().len() <= config.max_file_size as u64 {
                files.push(path.to_path_buf());
            }
        }
    }
    files
}

fn is_excluded(entry: &DirEntry, config: &GitImportConfig) -> bool {
    let name = entry.file_name().to_string_lossy();
    // 默认排除
    if name.starts_with('.') && name != "." { return true; }
    if matches!(name.as_ref(), "node_modules" | "target" | "dist" | "build" | "__pycache__" | ".venv" | "vendor") {
        return true;
    }
    // 用户自定义排除
    config.excluded_dirs.iter().any(|d| name == d.as_str())
}
```

**依赖**：添加 `gix` crate（纯 Rust Git 实现）或使用系统 `git` 命令。

#### 3.1.2 URL 抓取

```rust
pub struct UrlImportConfig {
    pub url: String,
    pub selector: Option<String>,  // CSS selector，默认 body
    pub extract_mode: ExtractMode, // Markdown | PlainText
}

pub enum ExtractMode {
    Markdown,  // 保留标题、列表、代码块等结构
    PlainText, // 纯文本
}

pub async fn import_url(
    pool: &SqlitePool,
    app: &AppHandle,
    kb_id: &str,
    config: &UrlImportConfig,
) -> Result<KbDocument, String> {
    // 1. HTTP GET 抓取页面
    // 2. 使用 html2text 或 scraper crate 提取正文
    //    - 去除 nav/footer/script/style
    //    - 保留标题层级，转换为 Markdown
    // 3. 文件名 = URL 的域名+路径
    // 4. 创建 kb_documents 记录，source_type = 'url'
    // 5. 处理文档（parse → split → embed → store）
}
```

**依赖**：添加 `scraper` crate（HTML 解析）或 `html2text` crate。

#### 3.1.3 本地目录导入

```rust
pub struct LocalDirImportConfig {
    pub dir_path: String,
    pub recursive: bool,
    pub excluded_dirs: Vec<String>,
    pub included_files: Vec<String>,  // 扩展名过滤
    pub max_file_size: usize,
}

pub async fn import_local_dir(
    pool: &SqlitePool,
    app: &AppHandle,
    kb_id: &str,
    config: &LocalDirImportConfig,
) -> Result<Vec<KbDocument>, String> {
    // 1. 验证路径存在
    // 2. 遍历目录，应用过滤规则
    // 3. 逐文件创建记录并处理
    // 逻辑与 Git 导入的文件遍历相同
}
```

#### 3.1.4 统一入口 Handler

```rust
// POST /api/kb/{id}/import
pub async fn import_source(
    State(shared): State<SharedState>,
    Path(kb_id): Path<String>,
    Json(input): Json<ImportSourceInput>,
) -> Response {
    match input.source_type.as_str() {
        "git" => {
            let config = GitImportConfig { ... };
            tokio::spawn(importer::import_git_repo(pool, app, kb_id, config));
        }
        "url" => {
            let config = UrlImportConfig { ... };
            tokio::spawn(importer::import_url(pool, app, kb_id, config));
        }
        "local_dir" => {
            let config = LocalDirImportConfig { ... };
            tokio::spawn(importer::import_local_dir(pool, app, kb_id, config));
        }
        _ => return BadRequest,
    }
}

// GET /api/kb/{id}/sources — 列出所有导入源
// DELETE /api/kb/{id}/sources/{source_id} — 删除源及其文档
// POST /api/kb/{id}/sources/{source_id}/sync — 重新同步（拉取最新）
```

---

### 3.2 向量索引重写 — `retriever.rs`

#### 3.2.1 方案选型

| 方案 | Crate | 优点 | 缺点 |
|------|-------|------|------|
| HNSW | `hnsw_rs` | 纯 Rust，无外部依赖，性能好 | 内存索引，需持久化 |
| usearch | `usearch` | C++ 底层，极快 | 需要 C++ 依赖 |
| sqlite-vec | `sqlite-vec` | SQLite 扩展，与现有架构无缝集成 | 需要加载扩展 |
| 全量优化 | 无 | 零依赖 | 数据量大时仍然慢 |

**推荐：`hnsw_rs`**

理由：
- 纯 Rust，编译简单，适合 Tauri 打包
- 支持持久化（save/load）
- 支持增量插入
- 万级切片查询 <1ms

#### 3.2.2 索引管理器

```rust
use hnsw_rs::hnsw::{Hnsw, HnswBuilder, Params};
use hnsw_rs::distance::Distance;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// 全局索引缓存：kb_id → Hnsw 索引
pub struct IndexManager {
    indices: RwLock<HashMap<String, Arc<HnswIndex>>>,
    index_dir: PathBuf,
}

pub struct HnswIndex {
    hnsw: Hnsw<f32>,
    chunk_ids: RwLock<Vec<String>>,  // HNSW 内部 ID → chunk_id 映射
    embedding_dim: usize,
}

impl IndexManager {
    pub fn new(index_dir: PathBuf) -> Self {
        Self {
            indices: RwLock::new(HashMap::new()),
            index_dir,
        }
    }

    /// 获取或加载知识库的索引
    pub async fn get_or_load(&self, kb_id: &str) -> Option<Arc<HnswIndex>> {
        // 1. 先查缓存
        if let Some(idx) = self.indices.read().unwrap().get(kb_id) {
            return Some(idx.clone());
        }
        // 2. 从磁盘加载
        let path = self.index_dir.join(format!("{}.hnsw", kb_id));
        if path.exists() {
            let hnsw = Hnsw::load(&path).ok()?;
            let index = Arc::new(HnswIndex { hnsw, .. });
            self.indices.write().unwrap().insert(kb_id.into(), index.clone());
            Some(index)
        } else {
            None
        }
    }

    /// 构建知识库索引
    pub async fn build(
        &self,
        pool: &SqlitePool,
        kb_id: &str,
        expected_dim: usize,
    ) -> Result<(), String> {
        // 1. 从 DB 加载全部 chunks
        let repo = KbRepository::new(pool.clone());
        let chunks = repo.get_chunks_by_kb(kb_id).await?;

        // 2. 维度校验 — 过滤不一致的 embedding
        let valid: Vec<_> = chunks.into_iter()
            .filter(|c| decode_embedding(&c.3).len() == expected_dim)
            .collect();

        // 3. 构建 HNSW 索引
        let params = Params::new()
            .max_nb_connection(16)
            .layer(16)
            .distance(Distance::Cosine);
        let mut hnsw = HnswBuilder::new(params, expected_dim);

        let mut chunk_ids = Vec::with_capacity(valid.len());
        for (i, chunk) in valid.iter().enumerate() {
            let emb = decode_embedding(&chunk.3);
            hnsw.insert((&emb, i as u32));
            chunk_ids.push(chunk.0.clone()); // chunk_id
        }

        let index = Arc::new(HnswIndex {
            hnsw: hnsw.build(),
            chunk_ids: RwLock::new(chunk_ids),
            embedding_dim: expected_dim,
        });

        // 4. 持久化到磁盘
        let path = self.index_dir.join(format!("{}.hnsw", kb_id));
        index.hnsw.save(&path).map_err(|e| e.to_string())?;

        // 5. 更新缓存
        self.indices.write().unwrap().insert(kb_id.into(), index);

        // 6. 更新 DB 元数据
        sqlx::query("UPDATE kb_index_meta SET status='ready', built_at=?, chunk_count=? WHERE kb_id=?")
            .bind(now_iso())
            .bind(valid.len() as i64)
            .bind(kb_id)
            .execute(pool)
            .await
            .ok();

        Ok(())
    }

    /// 增量插入（新文档处理完后调用）
    pub async fn insert(
        &self,
        kb_id: &str,
        chunk_id: &str,
        embedding: &[f32],
    ) -> Result<(), String> {
        if let Some(index) = self.get_or_load(kb_id).await {
            let id = index.chunk_ids.read().unwrap().len();
            index.hnsw.insert((embedding, id as u32));
            index.chunk_ids.write().unwrap().push(chunk_id.to_string());
            // 标记索引为 stale（需要持久化）
        }
        Ok(())
    }

    /// 搜索
    pub async fn search(
        &self,
        kb_id: &str,
        query: &[f32],
        top_k: usize,
    ) -> Result<Vec<SearchResult>, String> {
        let index = self.get_or_load(kb_id).await
            .ok_or("Index not built")?;

        // 维度校验
        if query.len() != index.embedding_dim {
            return Err(format!(
                "Embedding dimension mismatch: query={} index={}",
                query.len(), index.embedding_dim
            ));
        }

        let results = index.hnsw.search(query, top_k);
        let chunk_ids = index.chunk_ids.read().unwrap();

        // 将 HNSW 结果映射回 chunk_id
        Ok(results.into_iter().filter_map(|r| {
            let chunk_id = chunk_ids.get(r.d_id as usize)?;
            Some(SearchResult {
                chunk_id: chunk_id.clone(),
                score: 1.0 - r.distance, // HNSW 返回距离，转换为相似度
                ..
            })
        }).collect())
    }
}
```

#### 3.2.3 检索流程

```rust
pub async fn search(
    pool: &SqlitePool,
    index_mgr: &IndexManager,
    kb_id: &str,
    query_embedding: &[f32],
    top_k: usize,
) -> Result<Vec<SearchResult>, String> {
    // 1. 尝试 HNSW 索引
    match index_mgr.search(kb_id, query_embedding, top_k).await {
        Ok(results) if !results.is_empty() => {
            // 从 DB 补全 content/filename/metadata
            let repo = KbRepository::new(pool.clone());
            let mut enriched = Vec::with_capacity(results.len());
            for r in results {
                if let Ok(Some(chunk)) = repo.get_chunk_by_id(&r.chunk_id).await {
                    enriched.push(SearchResult {
                        chunk_id: r.chunk_id,
                        doc_id: chunk.doc_id,
                        filename: chunk.filename,
                        content: chunk.content,
                        score: r.score,
                        metadata: chunk.metadata,
                    });
                }
            }
            return Ok(enriched);
        }
        _ => {}
    }

    // 2. 索引不存在或为空，降级为全量搜索（兼容旧数据）
    fallback_search(pool, kb_id, query_embedding, top_k).await
}
```

#### 3.2.4 Embedding 维度校验

```rust
pub fn validate_embeddings(
    chunks: &[(String, String, String, Vec<u8>, String, String)],
    expected_dim: usize,
) -> (Vec<ValidChunk>, Vec<InvalidChunk>) {
    let mut valid = Vec::new();
    let mut invalid = Vec::new();

    for chunk in chunks {
        let emb = decode_embedding(&chunk.3);
        if emb.len() == expected_dim {
            valid.push(chunk.clone());
        } else {
            invalid.push((chunk.0.clone(), emb.len(), expected_dim));
        }
    }

    if !invalid.is_empty() {
        tracing::warn!(
            "Found {} chunks with mismatched embedding dimensions (expected {}, got varying)",
            invalid.len(), expected_dim
        );
    }

    (valid, invalid)
}
```

---

### 3.3 对话历史管理 — `rag.rs` 重写

#### 3.3.1 数据模型

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub id: String,
    pub kb_id: String,
    pub role: String,       // user | assistant
    pub content: String,
    pub sources: Option<String>,  // JSON
    pub model: Option<String>,
    pub tokens_used: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskRequest {
    pub question: String,
    pub kb_id: Option<String>,
    pub top_k: Option<usize>,
    pub model: Option<String>,
    pub history: Option<Vec<(String, String)>>,  // (role, content)
    pub deep_research: Option<bool>,
    pub max_rounds: Option<usize>,  // Deep Research 最大轮数，默认 5
}
```

#### 3.3.2 RAG 流程（带对话历史 + Token 降级）

```rust
pub async fn ask(
    pool: &SqlitePool,
    index_mgr: &IndexManager,
    kb_id: &str,
    query: &str,
    embedding_model: &str,
    chat_model: &str,
    top_k: usize,
    history: &[(String, String)],  // 对话历史
    app: &AppHandle,
) -> Result<RagAnswer, String> {
    // 1. Embed query
    let embeddings = embedder::embed(&[query.to_string()], embedding_model, &repo).await?;
    let query_emb = &embeddings[0];

    // 2. Vector search
    let results = retriever::search(pool, index_mgr, kb_id, query_emb, top_k).await?;

    if results.is_empty() {
        return Ok(RagAnswer {
            answer: "知识库中没有找到相关内容。".to_string(),
            sources: vec![],
            usage: None,
        });
    }

    // 3. Build context
    let context = build_context(&results);

    // 4. Build prompt with history
    let prompt = build_rag_prompt(&context, query, history);

    // 5. Estimate tokens
    let estimated_tokens = estimate_tokens(&prompt);
    let model_limit = get_model_context_limit(chat_model);

    // 6. Token 降级策略
    let (final_prompt, context_used) = if estimated_tokens > model_limit {
        // 阶段1: 裁剪 context（按相似度从低到高移除）
        let trimmed = trim_context(&prompt, &results, model_limit);
        if estimate_tokens(&trimmed.0) > model_limit {
            // 阶段2: 去掉历史，只保留最近一轮
            let no_history = build_rag_prompt(&context, query, &history[history.len().saturating_sub(2)..]);
            if estimate_tokens(&no_history) > model_limit {
                // 阶段3: 去掉 context，直接回答
                let bare = format!(
                    "注意：由于 token 限制，无法附上知识库上下文。\n\n问题: {}",
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

    // 7. Call LLM
    let result = call_llm(&final_prompt, chat_model, app).await?;

    // 8. Save to conversation history
    save_conversation(pool, kb_id, "user", query, None, chat_model).await?;
    save_conversation(pool, kb_id, "assistant", &result.answer, Some(&result.sources), chat_model).await?;

    Ok(result)
}
```

#### 3.3.3 Prompt 构建

```rust
fn build_rag_prompt(
    context: &str,
    query: &str,
    history: &[(String, String)],
) -> String {
    let history_str = if history.is_empty() {
        String::new()
    } else {
        let h: String = history.iter()
            .map(|(role, content)| {
                match role.as_str() {
                    "user" => format!("User: {}", content),
                    "assistant" => format!("Assistant: {}", content),
                    _ => content.clone(),
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        format!("<conversation_history>\n{}\n</conversation_history>\n\n", h)
    };

    format!(
        r#"你是一个知识库助手。请基于以下检索到的知识库内容回答问题。

规则：
1. 只基于知识库内容回答，不要编造信息
2. 如果知识库中没有相关信息，明确说明
3. 回答要准确、简洁，标注信息来源
4. 如果是多轮对话，注意上下文连贯性

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
```

#### 3.3.4 Token 估算

```rust
fn estimate_tokens(text: &str) -> usize {
    // 启发式估算：英文 ~4 chars/token，中文 ~2 chars/token
    let ascii_chars = text.chars().filter(|c| c.is_ascii()).count();
    let non_ascii_chars = text.chars().filter(|c| !c.is_ascii()).count();
    (ascii_chars / 4) + (non_ascii_chars / 2) + 1
}

fn get_model_context_limit(model: &str) -> usize {
    match model {
        m if m.contains("gpt-4o") => 128_000,
        m if m.contains("gpt-4") => 8_192,
        m if m.contains("gpt-3.5") => 16_385,
        m if m.contains("claude-3") => 200_000,
        m if m.contains("gemini") => 1_000_000,
        m if m.contains("deepseek") => 64_000,
        m if m.contains("qwen") => 32_000,
        _ => 8_192,  // 默认保守值
    }
}
```

#### 3.3.5 Context 裁剪

```rust
fn trim_context(
    prompt: &str,
    results: &[SearchResult],
    target_tokens: usize,
) -> (String, bool) {
    // 按相似度从低到高移除 context 片段
    let mut sorted_results: Vec<_> = results.iter().enumerate().collect();
    sorted_results.sort_by(|a, b| a.1.score.partial_cmp(&b.1.score).unwrap());

    let mut current_tokens = estimate_tokens(prompt);
    let mut removed_indices = std::collections::HashSet::new();

    for (idx, _) in &sorted_results {
        if current_tokens <= target_tokens {
            break;
        }
        removed_indices.insert(*idx);
        // 重新计算 token（移除该 chunk 的内容）
        current_tokens -= estimate_tokens(&results[*idx].content) / 2;  // 粗略估算
    }

    // 重建 context
    let context: String = results.iter().enumerate()
        .filter(|(i, _)| !removed_indices.contains(i))
        .map(|(_, r)| format!("--- 文档 [{}] (相似度: {:.2}) ---\n{}", r.filename, r.score, r.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    // 重建 prompt
    let new_prompt = format!(
        r#"<knowledge_base>{}</knowledge_base>

问题: {}"#,
        context, query
    );

    (new_prompt, !removed_indices.is_empty())
}
```

---

### 3.4 Deep Research 模式

```rust
pub async fn deep_research(
    pool: &SqlitePool,
    index_mgr: &IndexManager,
    kb_id: &str,
    query: &str,
    embedding_model: &str,
    chat_model: &str,
    top_k: usize,
    max_rounds: usize,  // 默认 5
    app: &AppHandle,
) -> Result<RagAnswer, String> {
    let mut all_findings = Vec::new();
    let mut history: Vec<(String, String)> = Vec::new();

    for round in 0..max_rounds {
        // 1. 根据轮次生成不同的查询
        let round_query = if round == 0 {
            query.to_string()
        } else {
            // 基于前轮发现生成追问
            generate_follow_up_query(&all_findings, query, chat_model, app).await?
        };

        // 2. Embed + Search
        let embeddings = embedder::embed(&[round_query.clone()], embedding_model, &repo).await?;
        let results = retriever::search(pool, index_mgr, kb_id, &embeddings[0], top_k).await?;

        // 3. 生成本轮回答
        let round_prompt = build_research_prompt(
            round, query, &results, &all_findings, &history
        );
        let answer = call_llm(&round_prompt, chat_model, app).await?;

        // 4. 记录发现
        all_findings.push(ResearchFinding {
            round,
            query: round_query,
            answer: answer.answer.clone(),
            sources: answer.sources.clone(),
        });

        history.push(("user".into(), round_query));
        history.push(("assistant".into(), answer.answer));

        // 5. 检查是否已足够
        if round > 0 && should_stop_research(&all_findings, chat_model, app).await? {
            break;
        }
    }

    // 最终综合
    let final_prompt = build_final_synthesis(query, &all_findings);
    let final_answer = call_llm(&final_prompt, chat_model, app).await?;

    Ok(final_answer)
}
```

**轮次 Prompt 模板**：

```rust
fn build_research_prompt(
    round: usize,
    original_query: &str,
    results: &[SearchResult],
    findings: &[ResearchFinding],
    history: &[(String, String)],
) -> String {
    if round == 0 {
        // 第一轮：制定计划 + 初步发现
        format!(
            r#"你是一个深度研究助手。请分析以下检索到的知识库内容，并给出初步发现。

原始问题: {query}

<knowledge_base>
{context}
</knowledge_base>

请完成：
1. 理解问题的核心需求
2. 从知识库中提取相关信息
3. 给出初步发现
4. 指出还需要哪些方面的信息
"#,
            query = original_query,
            context = build_context(results),
        )
    } else {
        // 中间轮：深入分析
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
3. 指出是否需要继续研究
"#,
            query = original_query,
            findings = findings.iter().map(|f| format!("第{}轮: {}", f.round + 1, f.answer)).collect::<Vec<_>>().join("\n"),
            context = build_context(results),
        )
    }
}
```

---

### 3.5 分块配置化 — `splitter.rs` 增强

```rust
pub struct SplitConfig {
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub split_by: SplitStrategy,
    pub language: Option<String>,  // 语言提示，影响 token 估算
}

pub enum SplitStrategy {
    Auto,           // 根据文件类型自动选择
    ByParagraph,    // 按段落分
    ByHeader,       // 按 Markdown 标题分
    ByToken,        // 按 token 数分
    Fixed,          // 固定大小
}

impl SplitConfig {
    pub fn from_kb(kb: &KbKnowledgeBase) -> Self {
        Self {
            chunk_size: kb.chunk_size as usize,
            chunk_overlap: kb.chunk_overlap as usize,
            split_by: SplitStrategy::Auto,
            language: None,
        }
    }

    /// 根据文件类型自适应
    pub fn for_file_type(mut self, file_type: &str) -> Self {
        match file_type {
            "markdown" => self.split_by = SplitStrategy::ByHeader,
            "python" | "rust" | "typescript" | "javascript" => {
                self.split_by = SplitStrategy::ByParagraph;
                self.language = Some(file_type.to_string());
            }
            _ => self.split_by = SplitStrategy::ByToken,
        }
        self
    }
}
```

**Token 估算优化**（语言感知）：

```rust
fn estimate_token_count(text: &str, language: Option<&str>) -> usize {
    let ascii_chars = text.chars().filter(|c| c.is_ascii()).count();
    let non_ascii = text.chars().filter(|c| !c.is_ascii()).count();

    let chars_per_token = match language {
        Some("chinese") | Some("japanese") | Some("korean") => 1.5,
        Some("python") | Some("rust") | Some("typescript") => 3.5,
        _ => 4.0,
    };

    ((ascii_chars as f64 / chars_per_token) + (non_ascii as f64 / 1.5)) as usize + 1
}
```

---

### 3.6 新增 API 路由

```rust
pub fn create_router(_state: Arc<AppState>) -> Router<SharedState> {
    Router::new()
        // === 已有路由 ===
        .route("/api/kb", get(handlers::list_knowledge_bases).post(handlers::create_knowledge_base))
        .route("/api/kb/{id}", get(handlers::get_knowledge_base).put(handlers::update_knowledge_base).delete(handlers::delete_knowledge_base))
        .route("/api/kb/{id}/stats", get(handlers::kb_stats))
        .route("/api/kb/{id}/documents", get(handlers::list_documents).post(handlers::upload_document))
        .route("/api/kb/{kb_id}/documents/{doc_id}", get(handlers::get_document).delete(handlers::delete_document))
        .route("/api/kb/{kb_id}/documents/{doc_id}/reindex", post(handlers::reindex_document))
        .route("/api/kb/search", get(handlers::search))
        .route("/api/kb/ask", post(handlers::ask))

        // === 新增：多源导入 ===
        .route("/api/kb/{id}/import", post(handlers::import_source))
        .route("/api/kb/{id}/sources", get(handlers::list_sources))
        .route("/api/kb/{id}/sources/{source_id}", delete(handlers::delete_source))
        .route("/api/kb/{id}/sources/{source_id}/sync", post(handlers::sync_source))

        // === 新增：对话历史 ===
        .route("/api/kb/{id}/conversations", get(handlers::list_conversations).delete(handlers::clear_conversations))

        // === 新增：索引管理 ===
        .route("/api/kb/{id}/index", get(handlers::get_index_status).post(handlers::build_index).delete(handlers::drop_index))

        // === 新增：Deep Research ===
        .route("/api/kb/deep-research", post(handlers::deep_research))
}
```

---

## 四、前端变更

### 4.1 文档 Tab — 新增导入入口

在 `DocumentsTab` 上传区域旁边新增导入按钮组：

```tsx
// 三个导入入口：Git / URL / 本地目录
<div className="flex gap-2">
  <button onClick={() => setShowGitImport(true)}>
    <GitBranch size={16} /> Git 仓库
  </button>
  <button onClick={() => setShowUrlImport(true)}>
    <Link size={16} /> 网页 URL
  </button>
  <button onClick={() => setShowDirImport(true)}>
    <Folder size={16} /> 本地目录
  </button>
</div>

// Git 导入弹窗
<GitImportModal
  kbId={kb.id}
  onClose={() => setShowGitImport(false)}
  onImported={fetchDocs}
/>

// URL 导入弹窗
<UrlImportModal
  kbId={kb.id}
  onClose={() => setShowUrlImport(false)}
  onImported={fetchDocs}
/>

// 本地目录导入弹窗
<DirImportModal
  kbId={kb.id}
  onClose={() => setShowDirImport(false)}
  onImported={fetchDocs}
/>
```

#### Git 导入弹窗

```tsx
function GitImportModal({ kbId, onClose, onImported }) {
  const [repoUrl, setRepoUrl] = useState("");
  const [branch, setBranch] = useState("");
  const [token, setToken] = useState("");
  const [excludedDirs, setExcludedDirs] = useState("node_modules,.git,dist,build,target,__pycache__,.venv,vendor");
  const [includedFiles, setIncludedFiles] = useState(".md,.txt,.rs,.py,.ts,.tsx,.js,.jsx,.go,.java,.json,.yaml,.yml,.toml");
  const [importing, setImporting] = useState(false);

  const handleImport = async () => {
    setImporting(true);
    await kbApi.importSource(kbId, {
      source_type: "git",
      repo_url: repoUrl,
      branch: branch || undefined,
      token: token || undefined,
      excluded_dirs: excludedDirs.split(",").map(s => s.trim()),
      included_files: includedFiles.split(",").map(s => s.trim()),
    });
    onImported();
    onClose();
  };
  // ... UI
}
```

### 4.2 问答 Tab — 增加 Deep Research 模式

```tsx
function AskTab({ kb }) {
  const [deepResearch, setDeepResearch] = useState(false);
  const [maxRounds, setMaxRounds] = useState(5);

  const handleAsk = async () => {
    const result = deepResearch
      ? await kbApi.deepResearch({
          question: userMsg,
          kb_id: kb.id,
          top_k: 10,
          model: selectedModel,
          max_rounds: maxRounds,
        })
      : await kbApi.ask({
          question: userMsg,
          kb_id: kb.id,
          top_k: 5,
          model: selectedModel,
        });
  };

  return (
    <div>
      {/* 模式切换 */}
      <div className="flex items-center gap-2">
        <button
          onClick={() => setDeepResearch(!deepResearch)}
          className={deepResearch ? "bg-violet-50 text-violet-600" : "text-slate-500"}
        >
          <Sparkles size={14} />
          深度研究模式
        </button>
        {deepResearch && (
          <select value={maxRounds} onChange={e => setMaxRounds(+e.target.value)}>
            <option value={3}>3 轮</option>
            <option value={5}>5 轮</option>
            <option value={7}>7 轮</option>
          </select>
        )}
      </div>

      {/* 对话区域 — 不变 */}
      {/* ... */}
    </div>
  );
}
```

### 4.3 设置 Tab — 增加分块配置

```tsx
function SettingsTab({ kb }) {
  const [chunkSize, setChunkSize] = useState(kb.chunk_size || 512);
  const [chunkOverlap, setChunkOverlap] = useState(kb.chunk_overlap || 64);
  const [excludedDirs, setExcludedDirs] = useState(kb.excluded_dirs || "");

  return (
    <div>
      {/* 分块配置 */}
      <div className="surface data-card">
        <h3>分块配置</h3>
        <div>
          <label>Chunk Size (tokens)</label>
          <input type="number" value={chunkSize} onChange={e => setChunkSize(+e.target.value)} />
          <p className="text-xs text-slate-400">目标切片大小，建议 256-1024</p>
        </div>
        <div>
          <label>Chunk Overlap (tokens)</label>
          <input type="number" value={chunkOverlap} onChange={e => setChunkOverlap(+e.target.value)} />
          <p className="text-xs text-slate-400">切片重叠区域，建议 10%-20% 的 chunk_size</p>
        </div>
        <div>
          <label>排除目录（Git/目录导入用）</label>
          <input value={excludedDirs} onChange={e => setExcludedDirs(e.target.value)} />
          <p className="text-xs text-slate-400">逗号分隔，如：node_modules,.git,dist</p>
        </div>
      </div>

      {/* 索引管理 */}
      <div className="surface data-card">
        <h3>向量索引</h3>
        <div>状态: {kb.index_status}</div>
        <div>维度: {kb.embedding_dim}</div>
        <button onClick={handleBuildIndex}>构建索引</button>
        <button onClick={handleDropIndex}>删除索引</button>
        {kb.index_status === 'stale' && (
          <span className="text-amber-600">索引过期，建议重建</span>
        )}
      </div>
    </div>
  );
}
```

### 4.4 API 层扩展

```typescript
// api.ts 新增
export const kbApi = {
  // ... 已有方法 ...

  importSource: (kbId: string, input: {
    source_type: "git" | "url" | "local_dir";
    repo_url?: string;
    branch?: string;
    token?: string;
    url?: string;
    dir_path?: string;
    excluded_dirs?: string[];
    included_files?: string[];
  }) => invoke<KbSource>("import_source", { kbId, input }),

  getSources: (kbId: string) => invoke<KbSource[]>("get_kb_sources", { kbId }),
  deleteSource: (kbId: string, sourceId: string) => invoke<void>("delete_kb_source", { kbId, sourceId }),
  syncSource: (kbId: string, sourceId: string) => invoke<void>("sync_kb_source", { kbId, sourceId }),

  getConversations: (kbId: string) => invoke<ConversationMessage[]>("get_kb_conversations", { kbId }),
  clearConversations: (kbId: string) => invoke<void>("clear_kb_conversations", { kbId }),

  buildIndex: (kbId: string) => invoke<void>("build_kb_index", { kbId }),
  dropIndex: (kbId: string) => invoke<void>("drop_kb_index", { kbId }),
  getIndexStatus: (kbId: string) => invoke<IndexStatus>("get_kb_index_status", { kbId }),

  deepResearch: (input: {
    question: string;
    kb_id?: string;
    top_k?: number;
    model?: string;
    max_rounds?: number;
  }) => invoke<KbRagAnswer>("deep_research", { input }),
};
```

---

## 五、Tauri Commands 桥接

```rust
// src-tauri/src/commands/kb.rs
use crate::services::knowledge::{handlers, importer, models};

#[tauri::command]
async fn import_source(
    kb_id: String,
    input: ImportSourceInput,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    // 调用 importer 对应函数
    match input.source_type.as_str() {
        "git" => importer::import_git_repo(&state.db.pool, &app, &kb_id, &config).await,
        "url" => importer::import_url(&state.db.pool, &app, &kb_id, &config).await,
        "local_dir" => importer::import_local_dir(&state.db.pool, &app, &kb_id, &config).await,
        _ => Err("Unknown source type".into()),
    }
}

#[tauri::command]
async fn get_kb_conversations(kb_id: String, state: tauri::State<'_, AppState>) -> Result<Vec<ConversationMessage>, String> {
    let repo = KbRepository::new(state.db.pool.clone());
    repo.get_conversations(&kb_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn build_kb_index(kb_id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let index_mgr = state.kb_index_mgr.clone();
    let kb_repo = KbRepository::new(state.db.pool.clone());
    let kb = kb_repo.get_kb(&kb_id).await.map_err(|e| e.to_string())?;
    let dim = kb.embedding_dim as usize;
    if dim == 0 {
        return Err("Embedding dimension not set".into());
    }
    index_mgr.build(&state.db.pool, &kb_id, dim).await
}

#[tauri::command]
async fn deep_research(
    input: DeepResearchInput,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> Result<KbRagAnswer, String> {
    rag::deep_research(
        &state.db.pool,
        &state.kb_index_mgr,
        &input.kb_id.unwrap_or_default(),
        &input.question,
        &emb_model,
        &input.model,
        input.top_k.unwrap_or(10),
        input.max_rounds.unwrap_or(5),
        &app,
    ).await
}
```

---

## 六、依赖清单

### Cargo.toml 新增

```toml
[dependencies]
# 已有依赖省略...

# 向量索引
hnsw_rs = "0.3"

# Git 仓库克隆
gix = { version = "0.7", default-features = false, features = ["blocking-network-client"] }
# 或使用系统 git 命令（无需额外依赖）

# HTML 解析（URL 导入）
scraper = "0.20"

# 目录遍历
walkdir = "2.5"

# URL 解析
url = "2.5"
```

### package.json 新增

```json
{
  "dependencies": {
    "lucide-react": "已有",
    "@tauri-apps/api": "已有"
  }
}
```

前端不需要新增依赖，所有 UI 组件用现有的 lucide-react 图标 + 原生 HTML。

---

## 七、实现顺序

### Phase 1：基础能力（1-2天）

| 步骤 | 模块 | 内容 |
|------|------|------|
| 1 | 迁移脚本 | `010_kb_upgrade.sql` — 新增字段和表 |
| 2 | models.rs | 新增 ConversationMessage、KbSource、ImportSourceInput 等模型 |
| 3 | repository.rs | 新增对话历史和源的 CRUD 方法 |
| 4 | rag.rs | 重写 ask()，加入对话历史 + Token 降级 |
| 5 | handlers.rs + routes.rs | 新增对话历史 API |
| 6 | 前端 api.ts | 新增 API 方法 |
| 7 | 前端 AskTab | 后端返回对话历史时注入到前端对话列表 |

### Phase 2：多源录入（2-3天）

| 步骤 | 模块 | 内容 |
|------|------|------|
| 8 | importer.rs | Git 仓库导入（clone + 遍历 + 批量创建文档） |
| 9 | importer.rs | URL 抓取（HTTP GET + HTML→Markdown） |
| 10 | importer.rs | 本地目录导入（遍历 + 批量创建） |
| 11 | handlers.rs + routes.rs | 新增导入 API |
| 12 | 前端 DocumentsTab | 新增三个导入弹窗 |
| 13 | 前端 SettingsTab | 新增分块配置和过滤规则 |

### Phase 3：向量索引（1-2天）

| 步骤 | 模块 | 内容 |
|------|------|------|
| 14 | Cargo.toml | 添加 hnsw_rs 依赖 |
| 15 | retriever.rs | IndexManager 实现 |
| 16 | retriever.rs | 维度校验 + 降级搜索 |
| 17 | handlers.rs | 索引管理 API |
| 18 | 前端 SettingsTab | 索引管理 UI |
| 19 | processor.rs | 文档处理完成后触发增量索引 |

### Phase 4：Deep Research（1-2天）

| 步骤 | 模块 | 内容 |
|------|------|------|
| 20 | rag.rs | deep_research() 实现 |
| 21 | handlers.rs | Deep Research API |
| 22 | 前端 AskTab | Deep Research 模式切换 UI |
| 23 | 前端 api.ts | deepResearch 方法 |

---

## 八、测试计划

### 8.1 功能测试

| 场景 | 测试点 |
|------|--------|
| Git 导入 | 公开仓库 clone、私有仓库（带 token）、大仓库（文件过滤） |
| URL 导入 | 普通网页、技术文档页、含代码块的页面 |
| 目录导入 | 代码项目目录、文档目录、嵌套目录 |
| 对话历史 | 多轮对话上下文连贯性、清空历史 |
| Token 降级 | 超长 context 自动裁剪、极端情况去掉 context |
| 向量索引 | 首次构建、增量更新、重建、维度不匹配 |
| Deep Research | 3/5/7 轮迭代、提前终止、最终综合 |

### 8.2 性能测试

| 指标 | 目标 |
|------|------|
| 向量检索（1000 chunks） | <10ms（HNSW）vs 当前 ~100ms（全量） |
| 向量检索（10000 chunks） | <50ms（HNSW）vs 当前 ~1000ms（全量） |
| Git 导入（中等仓库） | <30s（clone + 处理 100 文件） |
| RAG 问答端到端 | <5s（检索 + LLM 生成） |
| Deep Research（5 轮） | <60s |

---

## 九、兼容性

- **数据库**：增量迁移，不破坏现有数据
- **API**：所有现有端点保持不变，新功能通过新端点添加
- **前端**：渐进式增强，旧知识库不构建索引也能用（降级为全量搜索）
- **渠道**：Embedding 继续复用渠道调度，不变
- **MCP**：现有 MCP 端点不变，新增的源和索引对 MCP 透明
