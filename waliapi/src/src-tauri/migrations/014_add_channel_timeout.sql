-- 014: Add channel timeout configuration
-- Each channel can configure its own request timeout (in seconds).
-- Default 60 seconds; existing channels get this default automatically.
ALTER TABLE channels ADD COLUMN timeout_secs INTEGER NOT NULL DEFAULT 60;
