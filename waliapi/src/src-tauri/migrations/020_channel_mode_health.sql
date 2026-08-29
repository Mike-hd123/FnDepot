-- 020: per-channel transport-mode health
--
-- A provider can be healthy for Chat SSE while returning an invalid body for
-- non-stream Chat Completions (or vice versa).  Keep those capabilities
-- independent so one broken transport mode does not disable the whole channel.

CREATE TABLE IF NOT EXISTS channel_mode_health (
    channel_id TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    is_stream INTEGER NOT NULL CHECK (is_stream IN (0, 1)),
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    cooldown_until TEXT,
    last_failure_at TEXT,
    last_failure_reason TEXT,
    PRIMARY KEY (channel_id, endpoint, is_stream),
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_channel_mode_health_cooldown
    ON channel_mode_health(endpoint, is_stream, cooldown_until);
