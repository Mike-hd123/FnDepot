-- ═══════════════════════════════════════════════════════
-- FTS5 全文索引：支持混合检索（向量 + 关键词）
-- ═══════════════════════════════════════════════════════

CREATE VIRTUAL TABLE IF NOT EXISTS kb_chunks_fts USING fts5(
    chunk_id UNINDEXED,
    content,
    symbol_name,
    tokenize = 'unicode61 remove_diacritics 2'
);

-- 触发器：chunk 插入时同步 FTS
CREATE TRIGGER IF NOT EXISTS kb_chunks_ai AFTER INSERT ON kb_chunks BEGIN
    INSERT INTO kb_chunks_fts(chunk_id, content, symbol_name)
    VALUES (NEW.id, NEW.content, COALESCE(NEW.symbol_name, ''));
END;

-- 触发器：chunk 删除时同步 FTS
CREATE TRIGGER IF NOT EXISTS kb_chunks_ad AFTER DELETE ON kb_chunks BEGIN
    DELETE FROM kb_chunks_fts WHERE chunk_id = OLD.id;
END;

-- 触发器：chunk 更新时同步 FTS
CREATE TRIGGER IF NOT EXISTS kb_chunks_au AFTER UPDATE ON kb_chunks BEGIN
    DELETE FROM kb_chunks_fts WHERE chunk_id = OLD.id;
    INSERT INTO kb_chunks_fts(chunk_id, content, symbol_name)
    VALUES (NEW.id, NEW.content, COALESCE(NEW.symbol_name, ''));
END;
