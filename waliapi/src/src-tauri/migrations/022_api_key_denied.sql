-- 022: Add denied_channels/denied_models to api_keys for blacklist support
ALTER TABLE api_keys ADD COLUMN denied_channels TEXT NOT NULL DEFAULT '[]';
ALTER TABLE api_keys ADD COLUMN denied_models TEXT NOT NULL DEFAULT '[]';
