# WaLiAPI Wiki 模块完整架构设计

> 目标：在 WaLiAPI【服务】Tab 中，与现有【知识库】并排新增【Wiki】模块，集成 LLM Wiki 的完整能力，并对外暴露给 MCP 和 Skills 使用。

---

## 一、现有架构分析

### 1.1 WaLiAPI 当前架构

```
WaLiAPI/
├── src/                          # React 前端
│   ├── App.tsx                   # 路由：/services → KnowledgeBasePage
│   ├── pages/
│   │   └── KnowledgeBasePage.tsx # 服务页（Tab: 知识库 | MCP | Skills）
│   ├── components/layout/
│   │   └── Sidebar.tsx           # 侧边栏导航
│   └── lib/api.ts                # Tauri invoke 封装
└── src-tauri/                    # Rust Tauri 后端
    ├── src/
    │   ├── lib.rs                # App 入口，AppState，invoke_handler 注册
    │   ├── server/router.rs      # Axum HTTP 路由（OpenAI 兼容 + 服务路由合并）
    │   ├── services/             # 服务注册中心
    │   │   ├── mod.rs            # Service trait + ServiceRegistry
    │   │   ├── knowledge/        # 知识库服务（RAG/HNSW/MCP 工具）
    │   │   ├── mcp/              # MCP Server（JSON-RPC over SSE）
    │   │   └── upstream_models.rs
    │   ├── db/                   # SQLite（sqlx 迁移）
    │   ├── core/                 # 代理/路由/调度核心
    │   ├── adaptor/              # 协议适配器（OpenAI/Anthropic/Gemini/DeepSeek）
    │   └── commands/             # Tauri 命令层
    └── migrations/               # SQL 迁移文件 001-016
```

**关键设计模式**：`Service` trait + `ServiceRegistry` — 所有服务统一注册，路由合并到一个 Axum Router，状态通过 `SharedState` 共享。

### 1.2 现有知识库（KB）能力

| 能力 | 实现 |
|------|------|
| 文档管理 | 上传/删除/重索引，支持 Git/URL/本地目录导入 |
| 向量化 | embedding via Channel proxy，存储在 kb_chunks.embedding BLOB |
| 检索 | HNSW 向量索引 + FTS5 关键词 + 混合搜索 |
| RAG 问答 | 检索 → prompt 拼接 → Channel proxy 转发 → 回答 + 来源引用 |
| MCP 暴露 | 12 个 MCP 工具（search/ask/list/upload/import 等） |

**局限**：每次查询都从头检索+生成，知识不积累、不交叉引用、不维护持久化结构。

### 1.3 LLM Wiki 核心能力（material/llm_wiki 参考）

| 能力 | 说明 |
|------|------|
| 三层架构 | Raw sources（不可变）→ Wiki（LLM 生成 Markdown）→ Schema（规则） |
| 摄入（Ingest） | LLM 读文档 → 分析 → 生成/更新 Wiki 页面 → 交叉引用 → 写 log.md |
| 查询（Query） | LLM 读 index.md → 定位相关页面 → 综合回答 + 引用 |
| 检查（Lint） | 矛盾检测/孤儿页面/缺失页面/过时信息 |
| 知识图谱 | 四信号关联 + Louvain 社区检测 + 惊奇连接/知识空白 |
| 深度研究 | LLM 自动生成搜索主题 → 网络搜索 → 结果摄入 |
| Chat Agent | Rust 后端工具调用运行时（wiki/source/graph/web 检索） |
| MCP Server | 10 个工具（files/read_file/search/chat/graph/reviews/rescan 等） |
| 浏览器扩展 | Chrome 网页剪藏 → 自动摄入 |
| 多格式解析 | PDF/Office/EPUB/音视频/网页，内置 pdfium |

---

## 二、Wiki 模块架构设计

### 2.1 核心设计原则

1. **复用 WaLiAPI 基础设施**：Channel proxy 做模型调用、SQLite 做持久化、Axum 做 HTTP、ServiceRegistry 做注册
2. **与现有知识库互补不替代**：KB = 向量检索 + RAG（片段级），Wiki = LLM 增量构建（页面级 + 图谱级）
3. **统一 MCP 入口**：Wiki 工具挂载到现有 MCP Server，Agent/Skills 一个端点访问全部
4. **项目隔离**：每个 Wiki 项目对应一个独立目录（raw/ + wiki/ + schema），元数据存 SQLite

### 2.2 整体架构图

```
┌─────────────────────────────────────────────────────────────────┐
│                        React Frontend                            │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  服务 Page (KnowledgeBasePage → ServicePage)             │    │
│  │  ┌─────────┬──────────┬──────────┬─────────────────┐    │    │
│  │  │ 知识库  │   Wiki   │   MCP    │     Skills      │    │    │
│  │  │ (KB)    │  (NEW)   │  Server  │    技能包       │    │    │
│  │  └─────────┴──────────┴──────────┴─────────────────┘    │    │
│  └──────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
                              │ invoke / HTTP
┌─────────────────────────────┴────────────────────────────────────┐
│                      Tauri Rust Backend                          │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │              ServiceRegistry (services/mod.rs)           │    │
│  │  ┌────────────┬────────────┬────────────┬────────────┐   │    │
│  │  │ Knowledge  │   Wiki     │    MCP     │  ...future  │   │    │
│  │  │ Service    │  Service   │  Service   │             │   │    │
│  │  │ (现有)     │  (NEW)     │  (扩展)    │             │   │    │
│  │  └────────────┴────────────┴────────────┴────────────┘   │    │
│  └──────────────────────────────────────────────────────────┘    │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │                    Wiki Service                           │    │
│  │  ┌──────────┬───────────┬──────────┬─────────────────┐  │    │
│  │  │ Project  │  Ingest   │  Query   │  Graph           │  │    │
│  │  │ Manager  │  Engine   │  Engine  │  Engine          │  │    │
│  │  ├──────────┼───────────┼──────────┼─────────────────┤  │    │
│  │  │ Lint     │  Deep     │  Chat    │  Source          │  │    │
│  │  │ Engine   │  Research │  Agent   │  Watcher         │  │    │
│  │  └──────────┴───────────┴──────────┴─────────────────┘  │    │
│  │                                                           │    │
│  │  共享：Channel Proxy (模型调用) · SQLite (元数据)         │    │
│  │  文件系统：~/Library/Application Support/WaLiAPI/wiki/    │    │
│  └──────────────────────────────────────────────────────────┘    │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │              MCP Server (扩展 Wiki 工具)                  │    │
│  │  现有 12 个 KB 工具 + 新增 10 个 Wiki 工具                 │    │
│  └──────────────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────────────┘
```

### 2.3 文件系统布局

```
~/Library/Application Support/WaLiAPI/wiki/
├── projects/
│   ├── {project_id}/                    # 一个 Wiki 项目
│   │   ├── raw/
│   │   │   ├── sources/                 # 不可变原始资料
│   │   │   │   ├── pdf/
│   │   │   │   ├── docs/
│   │   │   │   ├── urls/
│   │   │   │   └── clips/
│   │   │   └── assets/                  # 图片等附件
│   │   ├── wiki/                        # LLM 生成的 Wiki 页面
│   │   │   ├── index.md                 # 内容目录
│   │   │   ├── log.md                   # 操作日志
│   │   │   ├── entities/                # 实体页面
│   │   │   ├── concepts/                # 概念页面
│   │   │   ├── summaries/               # 摘要页面
│   │   │   └── reviews/                 # 审核页面
│   │   ├── schema/
│   │   │   └── CLAUDE.md                # Wiki 维护规则
│   │   ├── graph/
│   │   │   └── graph.json               # 知识图谱数据
│   │   └── .meta.json                   # 项目元数据
│   └── {project_id}/
├── skills/                              # Wiki Agent Skills
│   └── SKILL.md
└── config.json                         # Wiki 全局配置
```

### 2.4 数据库设计（SQLite 迁移）

新增迁移文件 `017_wiki_module.sql`：

```sql
-- Wiki 项目
CREATE TABLE IF NOT EXISTS wiki_projects (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    description   TEXT,
    status        INTEGER NOT NULL DEFAULT 1,  -- 0=inactive 1=active
    schema_text   TEXT,                       -- CLAUDE.md 内容
    wiki_dir      TEXT NOT NULL,              -- 项目文件目录路径
    ingest_model  TEXT,                       -- 摄入用的模型
    chat_model    TEXT,                       -- 查询用的模型
    ingest_channel_id TEXT,                   -- 摄入用的 Channel
    chat_channel_id   TEXT,                    -- 查询用的 Channel
    mcp_enabled   INTEGER NOT NULL DEFAULT 1,
    source_count  INTEGER NOT NULL DEFAULT 0,
    page_count    INTEGER NOT NULL DEFAULT 0,
    last_ingest_at TEXT,
    last_lint_at   TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

-- Wiki 页面
CREATE TABLE IF NOT EXISTS wiki_pages (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL,
    path          TEXT NOT NULL,              -- wiki/ 相对路径
    title         TEXT NOT NULL,
    page_type     TEXT NOT NULL,              -- entity|concept|summary|review|index|log
    content_hash  TEXT NOT NULL,
    token_count   INTEGER NOT NULL DEFAULT 0,
    wikilinks     TEXT NOT NULL DEFAULT '[]', -- JSON array of [[links]]
    frontmatter   TEXT NOT NULL DEFAULT '{}',
    status        TEXT NOT NULL DEFAULT 'active', -- active|stale|orphan
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES wiki_projects(id) ON DELETE CASCADE,
    UNIQUE(project_id, path)
);

CREATE INDEX IF NOT EXISTS idx_wiki_pages_project ON wiki_pages(project_id);
CREATE INDEX IF NOT EXISTS idx_wiki_pages_type ON wiki_pages(project_id, page_type);

-- Wiki 源资料记录
CREATE TABLE IF NOT EXISTS wiki_sources (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL,
    source_type   TEXT NOT NULL,              -- pdf|office|epub|url|clip|audio|video|dir
    filename      TEXT NOT NULL,
    file_path     TEXT,
    source_url    TEXT,
    content_hash  TEXT,
    file_size     INTEGER NOT NULL DEFAULT 0,
    status        TEXT NOT NULL DEFAULT 'pending', -- pending|ingested|failed
    page_count    INTEGER NOT NULL DEFAULT 0,  -- 由此源触发生成的页面数
    error_message TEXT,
    created_at    TEXT NOT NULL,
    ingested_at   TEXT,
    FOREIGN KEY (project_id) REFERENCES wiki_projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_wiki_sources_project ON wiki_sources(project_id);

-- 摄入任务队列
CREATE TABLE IF NOT EXISTS wiki_ingest_queue (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL,
    source_id     TEXT,
    task_type     TEXT NOT NULL,              -- ingest|lint|reindex|deep_research
    status        TEXT NOT NULL DEFAULT 'pending', -- pending|running|done|failed|cancelled
    progress      INTEGER NOT NULL DEFAULT 0,
    total_steps   INTEGER NOT NULL DEFAULT 0,
    done_steps    INTEGER NOT NULL DEFAULT 0,
    result_json   TEXT,                       -- 任务结果
    error_message TEXT,
    created_at    TEXT NOT NULL,
    started_at    TEXT,
    completed_at  TEXT,
    FOREIGN KEY (project_id) REFERENCES wiki_projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_wiki_queue_project ON wiki_ingest_queue(project_id);
CREATE INDEX IF NOT EXISTS idx_wiki_queue_status ON wiki_ingest_queue(status);

-- 审核项
CREATE TABLE IF NOT EXISTS wiki_reviews (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL,
    review_type   TEXT NOT NULL,              -- contradiction|orphan|missing_page|duplicate|stale|suggestion
    title         TEXT NOT NULL,
    description   TEXT,
    source_path   TEXT,
    affected_pages TEXT NOT NULL DEFAULT '[]', -- JSON array of page paths
    search_queries TEXT NOT NULL DEFAULT '[]', -- JSON array of suggested searches
    options_json  TEXT NOT NULL DEFAULT '[]', -- JSON array of {action,label}
    resolved      INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL,
    resolved_at   TEXT,
    FOREIGN KEY (project_id) REFERENCES wiki_projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_wiki_reviews_project ON wiki_reviews(project_id);
CREATE INDEX IF NOT EXISTS idx_wiki_reviews_resolved ON wiki_reviews(project_id, resolved);

-- Wiki 会话历史
CREATE TABLE IF NOT EXISTS wiki_sessions (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL,
    role          TEXT NOT NULL,
    content       TEXT NOT NULL,
    sources_json  TEXT,                       -- 引用的 Wiki 页面
    model         TEXT,
    tokens_used   INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES wiki_projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_wiki_sessions_project ON wiki_sessions(project_id);

-- 知识图谱边
CREATE TABLE IF NOT EXISTS wiki_graph_edges (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL,
    source_page   TEXT NOT NULL,
    target_page   TEXT NOT NULL,
    edge_type     TEXT NOT NULL,              -- direct|source_overlap|adamic_adar|type_affinity
    weight        REAL NOT NULL DEFAULT 0.0,
    created_at    TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES wiki_projects(id) ON DELETE CASCADE,
    UNIQUE(project_id, source_page, target_page, edge_type)
);

CREATE INDEX IF NOT EXISTS idx_wiki_edges_project ON wiki_graph_edges(project_id);
CREATE INDEX IF NOT EXISTS idx_wiki_edges_source ON wiki_graph_edges(project_id, source_page);
```

---

## 三、Rust 后端模块设计

### 3.1 目录结构

```
src-tauri/src/services/
├── mod.rs                  # ServiceRegistry 新增 WikiService 注册
├── knowledge/              # 现有知识库（不动）
├── mcp/                    # MCP Server（扩展 Wiki 工具）
├── upstream_models.rs      # 现有
└── wiki/                   # ★ 新增 Wiki 服务
    ├── mod.rs              # WikiService impl Service
    ├── routes.rs           # HTTP 路由
    ├── models.rs           # 数据模型
    ├── repository.rs       # SQLite CRUD
    ├── project.rs          # 项目管理（创建/导入/导出/删除）
    ├── ingest.rs           # 摄入引擎（LLM 分析 → 生成 Wiki 页面）
    ├── query.rs            # 查询引擎（index.md 导航 → 页面合成 → 回答）
    ├── lint.rs             # 检查引擎（矛盾/孤儿/缺失/过时）
    ├── graph.rs            # 知识图谱（四信号 + Louvain + 洞察）
    ├── deep_research.rs    # 深度研究（多查询网络搜索 → 摄入）
    ├── chat_agent.rs       # Wiki Chat Agent（工具调用运行时）
    ├── source_watcher.rs   # Source 文件夹监听
    ├── parser/             # 多格式文档解析
    │   ├── mod.rs
    │   ├── pdf.rs          # PDF 解析（复用现有 pdf-extract + pdfium）
    │   ├── office.rs       # Office 文档解析
    │   ├── ebook.rs        # EPUB/MOBI 解析
    │   ├── web.rs          # 网页 → Markdown
    │   └── image.rs        # 图片 → 视觉模型描述
    └── schema.rs           # Wiki Schema 模板管理
```

### 3.2 Service trait 实现

```rust
// services/wiki/mod.rs
pub mod routes;
pub mod models;
pub mod repository;
pub mod project;
pub mod ingest;
pub mod query;
pub mod lint;
pub mod graph;
pub mod deep_research;
pub mod chat_agent;
pub mod source_watcher;
pub mod parser;
pub mod schema;

use super::{Service, ServiceStatus};
use crate::server::router::SharedState;
use crate::AppState;
use async_trait::async_trait;
use axum::Router;
use std::sync::Arc;

pub struct WikiService;

#[async_trait]
impl Service for WikiService {
    fn id(&self) -> &'static str { "wiki" }
    fn name(&self) -> &'static str { "Wiki" }
    fn description(&self) -> &'static str {
        "LLM 增量知识库：自动摄入文档 → 生成结构化 Wiki 页面 → 知识图谱 → 深度研究 → MCP 工具暴露"
    }

    async fn status(&self, state: &Arc<AppState>) -> ServiceStatus {
        let pool = &state.db.pool;
        let project_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM wiki_projects WHERE status = 1"
        ).fetch_one(pool).await.unwrap_or(0);
        let page_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM wiki_pages WHERE status = 'active'"
        ).fetch_one(pool).await.unwrap_or(0);
        let source_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM wiki_sources WHERE status = 'ingested'"
        ).fetch_one(pool).await.unwrap_or(0);

        ServiceStatus {
            id: self.id().to_string(),
            name: self.name().to_string(),
            description: self.description().to_string(),
            enabled: true,
            running: true,
            stats: serde_json::json!({
                "projects": project_count,
                "pages": page_count,
                "sources": source_count,
            }),
        }
    }

    fn routes(&self, state: Arc<AppState>) -> Router<SharedState> {
        routes::create_router(state)
    }
}
```

### 3.3 HTTP API 路由设计

```rust
// services/wiki/routes.rs
pub fn create_router(_state: Arc<AppState>) -> Router<SharedState> {
    Router::new()
        // ── 项目管理 ──
        .route("/api/wiki/projects",
            get(handlers::list_projects).post(handlers::create_project))
        .route("/api/wiki/projects/{id}",
            get(handlers::get_project).put(handlers::update_project)
                .delete(handlers::delete_project))
        .route("/api/wiki/projects/{id}/export",
            post(handlers::export_project))
        .route("/api/wiki/projects/import",
            post(handlers::import_project))

        // ── 源资料 ──
        .route("/api/wiki/projects/{id}/sources",
            get(handlers::list_sources).post(handlers::add_source))
        .route("/api/wiki/projects/{id}/sources/{sid}",
            delete(handlers::delete_source))
        .route("/api/wiki/projects/{id}/sources/{sid}/ingest",
            post(handlers::ingest_source))
        .route("/api/wiki/projects/{id}/rescan",
            post(handlers::rescan_sources))

        // ── Wiki 页面 ──
        .route("/api/wiki/projects/{id}/pages",
            get(handlers::list_pages))
        .route("/api/wiki/projects/{id}/pages/{*path}",
            get(handlers::get_page).put(handlers::update_page)
                .delete(handlers::delete_page))

        // ── 查询 ──
        .route("/api/wiki/projects/{id}/search",
            get(handlers::search))
        .route("/api/wiki/projects/{id}/ask",
            post(handlers::ask))

        // ── 知识图谱 ──
        .route("/api/wiki/projects/{id}/graph",
            get(handlers::get_graph))
        .route("/api/wiki/projects/{id}/graph/insights",
            get(handlers::get_insights))

        // ── 检查 ──
        .route("/api/wiki/projects/{id}/lint",
            post(handlers::run_lint))
        .route("/api/wiki/projects/{id}/reviews",
            get(handlers::list_reviews))
        .route("/api/wiki/projects/{id}/reviews/{rid}/resolve",
            post(handlers::resolve_review))

        // ── 深度研究 ──
        .route("/api/wiki/projects/{id}/deep-research",
            post(handlers::deep_research))

        // ── 会话 ──
        .route("/api/wiki/projects/{id}/sessions",
            get(handlers::list_sessions).delete(handlers::clear_sessions))

        // ── 摄入队列 ──
        .route("/api/wiki/projects/{id}/queue",
            get(handlers::get_queue_status))
}
```

### 3.4 摄入引擎（Ingest Engine）核心流程

```
用户添加源文件
    │
    ▼
┌─────────────────────────────────────┐
│ 1. 文档解析 (parser/)               │
│    PDF/Office/EPUB/URL → Markdown    │
│    提取图片 → 视觉模型描述            │
└─────────────────┬───────────────────┘
                  │
                  ▼
┌─────────────────────────────────────┐
│ 2. LLM 分析 (ingest.rs)              │
│    调用 Channel proxy（ingest_model）│
│    Prompt: "分析以下文档，提取关键    │
│    实体、概念、主题，生成 Wiki 页面"  │
└─────────────────┬───────────────────┘
                  │
                  ▼
┌─────────────────────────────────────┐
│ 3. 页面生成与更新                     │
│    - 生成新页面 → wiki/entities/*.md  │
│    - 更新已有页面 → 合并新信息         │
│    - 更新 index.md                   │
│    - 追加 log.md                     │
│    - 维护 [[wikilink]] 交叉引用       │
└─────────────────┬───────────────────┘
                  │
                  ▼
┌─────────────────────────────────────┐
│ 4. 图谱更新 (graph.rs)               │
│    - 直接链接：[[wikilink]] 边        │
│    - 来源重叠：共引文献              │
│    - Adamic-Adar 相似度              │
│    - 类型亲和：entity↔concept       │
│    - Louvain 社区检测                │
└─────────────────┬───────────────────┘
                  │
                  ▼
┌─────────────────────────────────────┐
│ 5. 审核标记 (lint.rs 触发)           │
│    - 矛盾检测：新旧信息冲突          │
│    - 缺失页面：提及但无独立页面       │
│    - 孤儿页面：无入链                │
│    - 过时信息：新源覆盖旧结论         │
└─────────────────────────────────────┘
```

**摄入 Prompt 模板**（通过 Channel proxy 调用 LLM）：

```text
你是 Wiki 维护者。根据以下规则处理新文档：

## Schema (CLAUDE.md)
{schema_text}

## 现有 Wiki 结构
{index_md_content}

## 新文档内容
{document_content}

## 任务
1. 提取关键实体、概念、主题
2. 对每个新增/更新项，生成 Markdown 页面
3. 用 [[wikilink]] 链接相关页面
4. 更新 index.md（新增条目 + 一行摘要）
5. 追加 log.md（`## [{date}] ingest | {source_name}`）
6. 标注潜在矛盾/缺失

输出 JSON:
{
  "pages": [{"path": "...", "content": "...", "action": "create|update"}],
  "index_update": "...",
  "log_entry": "...",
  "reviews": [{"type": "...", "title": "...", "description": "..."}]
}
```

### 3.5 查询引擎（Query Engine）

与现有 KB RAG 的区别：

| 维度 | 现有 KB RAG | Wiki Query |
|------|------------|------------|
| 检索单元 | 文本切片（chunk） | Wiki 页面（page） |
| 检索方式 | HNSW 向量 + FTS5 | index.md 导航 + 关键词 + 可选 embedding |
| 回答来源 | 切片拼接 | Wiki 页面综合 |
| 知识积累 | 无 | 增量构建，持续更新 |
| 交叉引用 | 无 | [[wikilink]] 关联 |

**查询流程**：

```
用户提问
    │
    ▼
LLM 读 index.md → 定位相关页面路径
    │
    ▼
读取 3-5 个 Wiki 页面 → 综合回答 + [[wikilink]] 引用
    │
    ▼
（可选）回答归档为新 Wiki 页面 → 知识积累
```

### 3.6 Chat Agent 设计

参考 llm_wiki 的 `agent/` 模块，但复用 WaLiAPI 的 Channel proxy：

```rust
// services/wiki/chat_agent.rs

/// Wiki Chat Agent 工具集
pub enum WikiAgentTool {
    WikiSearch { query: String, project_id: String },
    WikiRead { path: String, project_id: String },
    WikiList { root: String, project_id: String },
    SourceSearch { query: String, project_id: String },
    GraphQuery { project_id: String, filter: Option<String> },
    WebSearch { query: String, provider: String },
    WorkspaceWrite { path: String, content: String },
    ShellExec { command: String }, // 带权限审批
}

/// Agent 运行时
pub struct WikiAgentRuntime {
    channel_proxy: Arc<ChannelProxy>,  // 复用 WaLiAPI 现有 Channel
    pool: SqlitePool,
    project_id: String,
    session: AgentSession,
}

impl WikiAgentRuntime {
    pub async fn chat(&mut self, message: &str) -> Result<AgentResponse> {
        // 1. 构建系统 prompt（含 index.md 摘要）
        // 2. LLM 决定调用哪些工具（function calling）
        // 3. 执行工具 → 结果回传 LLM
        // 4. 循环直到 LLM 生成最终回答
        // 5. 保存会话历史
    }
}
```

---

## 四、MCP Server 扩展

在现有 `services/mcp/handlers.rs` 中新增 10 个 Wiki 工具：

```json
[
  {"name": "wiki_list_projects", "label": "Wiki 项目列表", "desc": "列出所有 Wiki 项目"},
  {"name": "wiki_get_project", "label": "Wiki 项目详情", "desc": "获取项目状态、统计、schema"},
  {"name": "wiki_files", "label": "Wiki 文件树", "desc": "列出 wiki/ 或 raw/sources/ 目录文件"},
  {"name": "wiki_read_file", "label": "Wiki 读取文件", "desc": "读取 Wiki 页面或原始文档"},
  {"name": "wiki_search", "label": "Wiki 搜索", "desc": "关键词 + 向量混合检索 Wiki 页面"},
  {"name": "wiki_ask", "label": "Wiki 问答", "desc": "基于 Wiki 的 LLM 问答，返回回答 + 引用页面"},
  {"name": "wiki_graph", "label": "知识图谱", "desc": "查询项目知识图谱，节点/边/社区"},
  {"name": "wiki_reviews", "label": "审核项", "desc": "列出未解决的矛盾/缺失/孤儿页面"},
  {"name": "wiki_ingest", "label": "摄入文档", "desc": "将新文档摄入到 Wiki 项目，触发 LLM 生成页面"},
  {"name": "wiki_rescan", "label": "重扫描源", "desc": "检测 raw/sources/ 目录变更并同步"}
]
```

MCP 工具调用流程（以 `wiki_ask` 为例）：

```
Agent (Claude Code / Cursor) 
    → MCP JSON-RPC POST /mcp
    → handlers::handle_mcp
    → 工具路由：wiki_ask
    → WikiQueryEngine::ask(project_id, question)
    → Channel proxy 调用 LLM
    → 返回 { answer, references, sources }
    → MCP 响应回 Agent
```

---

## 五、前端设计

### 5.1 路由变更

```tsx
// App.tsx 新增路由
<Route path="/services/wiki" element={<KnowledgeBasePage />} />
```

### 5.2 服务页 Tab 扩展

```tsx
// KnowledgeBasePage.tsx (重命名为 ServicePage 或保持)
type ServiceTab = "knowledge" | "wiki" | "mcp" | "skills";

const serviceTabs = [
  { key: "knowledge", label: "知识库", icon: BookOpen },
  { key: "wiki",       label: "Wiki",  icon: Network },     // ★ 新增
  { key: "mcp",        label: "MCP 服务", icon: Terminal },
  { key: "skills",     label: "Skills 技能", icon: Puzzle },
];
```

### 5.3 Wiki 前端页面结构

```
WikiSection (组件)
├── 项目选择器（Sidebar 或 Dropdown）
├── Tab 导航
│   ├── 概览 (Overview)     — 项目统计、最近摄入、审核待办
│   ├── 页面 (Pages)        — Wiki 页面列表 + 预览 + 编辑
│   ├── 源 (Sources)        — 原始文档管理 + 摄入触发
│   ├── 搜索 (Search)       — Wiki 搜索 + 问答
│   ├── 图谱 (Graph)        — Sigma 知识图谱可视化
│   ├── 审核 (Reviews)      — 矛盾/缺失/孤儿页面管理
│   ├── 设置 (Settings)     — 项目配置、模型选择、Schema 编辑
│   └── 队列 (Queue)        — 摄入任务进度
└── 状态栏 — Wiki 服务状态 + MCP 端点
```

### 5.4 新增前端依赖

```json
{
  "dependencies": {
    // 图谱可视化
    "sigma": "^3.0.2",
    "graphology": "^0.26.0",
    "graphology-communities-louvain": "^2.0.2",
    "graphology-layout-forceatlas2": "^0.10.1",
    "@react-sigma/core": "^5.0.6",
    // Markdown 渲染（已有 react-markdown）
    // Mermaid 渲染
    "mermaid": "^11.14.0",
    // Milkdown 编辑器（Wiki 页面编辑）
    "@milkdown/kit": "^7.20.0",
    "@milkdown/react": "^7.20.0",
    "@milkdown/theme-nord": "^7.20.0"
  }
}
```

---

## 六、Skills 集成

### 6.1 Wiki Skill 包

发布 `waliapi-wiki-skills` npm 包，一行命令接入 Agent：

```json
{
  "name": "waliapi-wiki",
  "version": "1.0.0",
  "mcpEndpoint": "http://127.0.0.1:8777/mcp",
  "tools": [
    "wiki_list_projects", "wiki_get_project", "wiki_files",
    "wiki_read_file", "wiki_search", "wiki_ask",
    "wiki_graph", "wiki_reviews", "wiki_ingest", "wiki_rescan"
  ]
}
```

### 6.2 使用示例

```bash
# Claude Code 接入
npx skills add https://github.com/fuzhengwei/waliapi-wiki-skills

# Cursor / Windsurf 接入
# MCP 配置指向 http://127.0.0.1:8777/mcp
```

Agent 使用后在对话中可以直接调用：
- "搜索我的 Wiki 中关于 XX 的内容" → `wiki_search`
- "把这篇文档摄入到 Wiki" → `wiki_ingest`
- "Wiki 中有哪些未解决的矛盾？" → `wiki_reviews`
- "画出知识图谱" → `wiki_graph`

---

## 七、实施计划

### Phase 1: 基础框架（1-2 周）

| 任务 | 产出 |
|------|------|
| 数据库迁移 `017_wiki_module.sql` | 7 张新表 |
| `services/wiki/` 骨架代码 | mod.rs, routes.rs, models.rs, repository.rs |
| `WikiService` 注册到 `ServiceRegistry` | 服务状态可见 |
| 前端 Wiki Tab + 空白页面 | UI 框架 |
| 项目 CRUD（创建/删除/配置） | 基础项目管理 |

### Phase 2: 摄入引擎（2-3 周）

| 任务 | 产出 |
|------|------|
| 多格式文档解析器（parser/） | PDF/Office/EPUB/URL → Markdown |
| LLM 摄入 Prompt 模板 + 调用 | 文档 → Wiki 页面 |
| 页面生成 + index.md 更新 + log.md 追加 | 增量构建 |
| [[wikilink]] 交叉引用维护 | 页面关联 |
| 摄入队列 + 进度可视化 | 异步摄入 + 状态跟踪 |
| Source 文件夹监听 | 自动检测变更 |

### Phase 3: 查询与图谱（2 周）

| 任务 | 产出 |
|------|------|
| 查询引擎（index.md 导航 → 页面合成） | Wiki 问答 |
| 知识图谱构建（四信号 + Louvain） | 图谱数据 |
| 图谱可视化（Sigma + ForceAtlas2） | 前端图谱 |
| 图谱洞察（惊奇连接/知识空白） | 智能建议 |
| 审核系统（矛盾/孤儿/缺失检测） | Review 列表 |

### Phase 4: MCP 与 Agent（1-2 周）

| 任务 | 产出 |
|------|------|
| MCP Server 扩展 10 个 Wiki 工具 | Agent 可调用 |
| Wiki Chat Agent（工具调用运行时） | Agent 对话 |
| Skills 包发布 | npm 可安装 |
| 浏览器扩展适配（Chrome 剪藏） | 网页摄入 |

### Phase 5: 深度研究与优化（1-2 周）

| 任务 | 产出 |
|------|------|
| 深度研究（多查询网络搜索 → 摄入） | 自动研究 |
| 向量语义搜索（可选 embedding 检索） | 大规模 Wiki 搜索 |
| 性能优化 + 缓存 | 稳定可用 |

---

## 八、与现有系统的关系

```
┌──────────────────────────────────────────────┐
│              WaLiAPI 服务层                   │
│                                              │
│  ┌──────────┐    ┌──────────┐    ┌────────┐ │
│  │ 知识库   │    │   Wiki   │    │  MCP   │ │
│  │ (KB)     │    │  (NEW)   │    │ Server │ │
│  │          │    │          │    │        │ │
│  │ 切片级   │    │ 页面级   │    │ KB 12  │ │
│  │ 向量检索 │    │ LLM 构建 │    │ +Wiki 10│ │
│  │ RAG 问答 │    │ 知识图谱 │    │ =22 工具│ │
│  └─────┬────┘    └────┬─────┘    └───┬────┘ │
│        │              │              │      │
│        └──────────────┴──────────────┘      │
│                       │                      │
│              Channel Proxy (模型调用)        │
│              SQLite (元数据持久化)            │
│              文件系统 (Wiki 项目目录)         │
└──────────────────────────────────────────────┘
```

**互补关系**：
- **KB**：精准片段检索，适合 "找到这句话在哪个文档的哪里"
- **Wiki**：结构化知识合成，适合 "总结这个主题的所有信息"
- Agent 可同时调用两者：KB 找原文出处，Wiki 找综合分析

---

## 九、关键决策点

### 9.1 为什么不直接在现有 KB 上扩展？

| 维度 | KB | Wiki |
|------|-----|------|
| 数据粒度 | chunk（段落级） | page（文档级） |
| 知识结构 | 扁平切片 | 层级 + 交叉引用 |
| 维护方式 | 重索引 | LLM 增量更新 |
| 查询方式 | 向量 top-k | index.md 导航 + 页面合成 |
| 知识积累 | 无 | 持续构建 |

架构差异太大，强行合并会破坏两者各自的优势。独立模块 + 共享基础设施是更合理的选择。

### 9.2 模型路由

复用 WaLiAPI Channel proxy，支持独立配置：
- `ingest_channel_id` + `ingest_model`：摄入时用强模型（如 Claude Sonnet）
- `chat_channel_id` + `chat_model`：查询时用快模型（如 GPT-4o-mini）
- 未配置时 fallback 到 WaLiAPI 默认路由

### 9.3 与 llm_wiki 参考项目的关系

- **参考但不依赖**：llm_wiki 是独立桌面应用（tiny_http + Tauri），我们提取其方法论和 Prompt 设计，用 WaLiAPI 的技术栈重新实现
- **不引入 llm_wiki 的依赖**：不引入 tiny_http、pdfium.js 等，复用 WaLiAPI 现有的 axum + pdf-extract + Channel proxy
- **MCP 协议兼容**：Wiki 工具命名参考 llm_wiki 的 MCP Server（`wiki_files`, `wiki_read_file`, `wiki_search` 等），保持 Agent 侧迁移成本低

---

## 十、风险与缓解

| 风险 | 缓解 |
|------|------|
| LLM 摄入成本高（每文档都调 LLM） | 增量缓存（content_hash 未变则跳过）+ 可选弱模型摄入 |
| Wiki 页面质量依赖 Prompt | Schema (CLAUDE.md) 可编辑 + 持续优化模板 |
| 大规模 Wiki 检索性能 | 小规模用 index.md 导航（<500 页），大规模可选 embedding |
| 与现有 KB 功能边界模糊 | 明确分工：KB = 片段检索，Wiki = 结构化知识 |
| 文件系统管理复杂 | 项目级隔离 + SQLite 元数据索引 + 导入导出 |
