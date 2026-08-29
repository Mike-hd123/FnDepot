-- 015: Channel protocol identity columns (T02)
--
-- Add protocol-identity columns to `channels`. This migration ONLY adds
-- columns; it never rewrites the legacy business fields (type/base_url/api_key/
-- models/model_mapping/priority/weight/status/timeout_secs/config/timestamps).
--
-- Semantics (T00 decisions 2/3/10, design 5.1/11.3):
--   * protocol             — openai | anthropic | ollama (NULL => legacy-uninitialized)
--   * provider             — openai|google|deepseek|qwen|zhipu|doubao|
--                            doubao_coding_plan|moonshot|anthropic|ollama|custom
--   * native_base_url      — new-protocol canonical root (UI root)
--   * native_endpoints     — upstream native capability list, JSON array
--   * preset_revision      — preset registry revision at save time (traceability only)
--   * identity_revision    — 0 = legacy/uninitialized => resolver must live-infer;
--                            >0 = written by the new dual-write UPDATE/INSERT
--   * legacy_executor_override — only 'gemini_native' for legacy Gemini
--
-- Old binaries keep writing only the legacy columns; rows they insert land at
-- identity_revision 0 and the new resolver infers identity at read time.
-- Old binaries that UPDATE legacy fields fire the invalidation trigger below,
-- which clears the new identity fields so a later upgrade re-infers (and never
-- trusts stale identity written by the pre-rollback code).

ALTER TABLE channels ADD COLUMN protocol TEXT;
ALTER TABLE channels ADD COLUMN provider TEXT;
ALTER TABLE channels ADD COLUMN native_base_url TEXT;
ALTER TABLE channels ADD COLUMN native_endpoints TEXT NOT NULL DEFAULT '[]';
ALTER TABLE channels ADD COLUMN preset_revision TEXT;
ALTER TABLE channels ADD COLUMN identity_revision INTEGER NOT NULL DEFAULT 0;
ALTER TABLE channels ADD COLUMN legacy_executor_override TEXT;

-- Invalidation trigger (T00 decision 10, design 11.3):
-- Whenever an old code path WRITES the legacy identity columns
-- (type/base_url/config) — SQLite fires AFTER UPDATE OF when the columns are
-- named in the UPDATE's SET list, regardless of whether the value changed —
-- clear the new identity and reset revision to 0 so the resolver re-infers
-- from the current legacy fields on next read.
-- This is what makes "upgrade -> rollback writes legacy -> re-upgrade" safe:
-- the rolled-back write invalidates any stale new-identity values.
CREATE TRIGGER IF NOT EXISTS trg_channels_legacy_invalidate_identity
AFTER UPDATE OF type, base_url, config ON channels
FOR EACH ROW
BEGIN
    UPDATE channels
       SET protocol = NULL,
           provider = NULL,
           native_base_url = NULL,
           native_endpoints = '[]',
           preset_revision = NULL,
           legacy_executor_override = NULL,
           identity_revision = 0
     WHERE id = NEW.id;
END;
