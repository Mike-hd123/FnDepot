-- Wiki Module tables

-- Wiki 项目
CREATE TABLE IF NOT EXISTS wiki_projects (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    description   TEXT,
    status        INTEGER NOT NULL DEFAULT 1,
    schema_text   TEXT,
    wiki_dir      TEXT NOT NULL,
    ingest_model  TEXT,
    chat_model    TEXT,
    ingest_channel_id TEXT,
    chat_channel_id   TEXT,
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
    path          TEXT NOT NULL,
    title         TEXT NOT NULL,
    page_type     TEXT NOT NULL,
    content_hash  TEXT NOT NULL,
    token_count   INTEGER NOT NULL DEFAULT 0,
    wikilinks     TEXT NOT NULL DEFAULT '[]',
    frontmatter   TEXT NOT NULL DEFAULT '{}',
    status        TEXT NOT NULL DEFAULT 'active',
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
    source_type   TEXT NOT NULL,
    filename      TEXT NOT NULL,
    file_path     TEXT,
    source_url    TEXT,
    content_hash  TEXT,
    file_size     INTEGER NOT NULL DEFAULT 0,
    status        TEXT NOT NULL DEFAULT 'pending',
    page_count    INTEGER NOT NULL DEFAULT 0,
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
    task_type     TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending',
    progress      INTEGER NOT NULL DEFAULT 0,
    total_steps   INTEGER NOT NULL DEFAULT 0,
    done_steps    INTEGER NOT NULL DEFAULT 0,
    result_json   TEXT,
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
    review_type   TEXT NOT NULL,
    title         TEXT NOT NULL,
    description   TEXT,
    source_path   TEXT,
    affected_pages TEXT NOT NULL DEFAULT '[]',
    search_queries TEXT NOT NULL DEFAULT '[]',
    options_json  TEXT NOT NULL DEFAULT '[]',
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
    sources_json  TEXT,
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
    edge_type     TEXT NOT NULL,
    weight        REAL NOT NULL DEFAULT 0.0,
    created_at    TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES wiki_projects(id) ON DELETE CASCADE,
    UNIQUE(project_id, source_page, target_page, edge_type)
);

CREATE INDEX IF NOT EXISTS idx_wiki_edges_project ON wiki_graph_edges(project_id);
CREATE INDEX IF NOT EXISTS idx_wiki_edges_source ON wiki_graph_edges(project_id, source_page);
