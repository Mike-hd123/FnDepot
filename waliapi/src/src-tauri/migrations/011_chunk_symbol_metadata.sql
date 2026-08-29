-- ═══════════════════════════════════════════════════════
-- 知识库 chunk 符号感知：新增 symbol_name / symbol_kind 列
-- 用于 tree-sitter AST 解析出的符号信息
-- ═══════════════════════════════════════════════════════

ALTER TABLE kb_chunks ADD COLUMN symbol_name TEXT;
ALTER TABLE kb_chunks ADD COLUMN symbol_kind TEXT;

CREATE INDEX IF NOT EXISTS idx_chunks_symbol ON kb_chunks(kb_id, symbol_kind)
    WHERE symbol_name IS NOT NULL;
