-- ═══════════════════════════════════════════════════════
-- 知识库表：新增 embedding_batch_size 列
-- ═══════════════════════════════════════════════════════
ALTER TABLE kb_knowledge_bases ADD COLUMN embedding_batch_size INTEGER NOT NULL DEFAULT 32;
