-- Knowledge Base tables

-- 知识库
CREATE TABLE IF NOT EXISTS kb_knowledge_bases (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    description   TEXT,
    status        INTEGER NOT NULL DEFAULT 1,
    doc_count     INTEGER NOT NULL DEFAULT 0,
    chunk_count   INTEGER NOT NULL DEFAULT 0,
    total_tokens  INTEGER NOT NULL DEFAULT 0,
    embedding_model  TEXT,
    embedding_channel_id TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

-- 文档
CREATE TABLE IF NOT EXISTS kb_documents (
    id            TEXT PRIMARY KEY,
    kb_id         TEXT NOT NULL,
    filename      TEXT NOT NULL,
    file_path     TEXT,
    file_type     TEXT NOT NULL,
    file_size     INTEGER NOT NULL DEFAULT 0,
    content_hash  TEXT NOT NULL,
    chunk_count   INTEGER NOT NULL DEFAULT 0,
    token_count   INTEGER NOT NULL DEFAULT 0,
    status        TEXT NOT NULL DEFAULT 'pending',
    error_message TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    FOREIGN KEY (kb_id) REFERENCES kb_knowledge_bases(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_documents_kb ON kb_documents(kb_id);
CREATE INDEX IF NOT EXISTS idx_documents_hash ON kb_documents(content_hash);
CREATE INDEX IF NOT EXISTS idx_documents_status ON kb_documents(status);

-- 文档切片 + 向量
CREATE TABLE IF NOT EXISTS kb_chunks (
    id             TEXT PRIMARY KEY,
    doc_id         TEXT NOT NULL,
    kb_id          TEXT NOT NULL,
    chunk_index    INTEGER NOT NULL,
    content        TEXT NOT NULL,
    token_count    INTEGER NOT NULL DEFAULT 0,
    embedding      BLOB,
    embedding_dim  INTEGER NOT NULL DEFAULT 0,
    metadata       TEXT NOT NULL DEFAULT '{}',
    created_at     TEXT NOT NULL,
    FOREIGN KEY (doc_id) REFERENCES kb_documents(id) ON DELETE CASCADE,
    FOREIGN KEY (kb_id) REFERENCES kb_knowledge_bases(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_chunks_kb ON kb_chunks(kb_id);
CREATE INDEX IF NOT EXISTS idx_chunks_doc ON kb_chunks(doc_id);

-- 处理任务记录
CREATE TABLE IF NOT EXISTS kb_tasks (
    id            TEXT PRIMARY KEY,
    kb_id         TEXT NOT NULL,
    doc_id        TEXT,
    task_type     TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending',
    progress      INTEGER NOT NULL DEFAULT 0,
    total_items   INTEGER NOT NULL DEFAULT 0,
    done_items    INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    created_at    TEXT NOT NULL,
    completed_at  TEXT,
    FOREIGN KEY (kb_id) REFERENCES kb_knowledge_bases(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tasks_kb ON kb_tasks(kb_id);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON kb_tasks(status);
