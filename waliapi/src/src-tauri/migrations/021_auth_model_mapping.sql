-- 021: Add model_mapping column to auth_accounts
-- Allows auth accounts to define model name mappings (same as channels)
ALTER TABLE auth_accounts ADD COLUMN model_mapping_json TEXT NOT NULL DEFAULT '{}';
