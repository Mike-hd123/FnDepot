-- Add response_choices column for logging AI response content
ALTER TABLE request_logs ADD COLUMN response_choices TEXT;

-- Note: seq column already exists from 001_init.sql (INTEGER NOT NULL DEFAULT 0)
-- 007_fix_log_seq.sql handles backfilling seq with rowid values
