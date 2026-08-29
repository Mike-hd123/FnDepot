-- Channel API Keys: 多密钥负载均衡支持
CREATE TABLE IF NOT EXISTS channel_api_keys (
    id          TEXT PRIMARY KEY,
    channel_id  TEXT NOT NULL,
    api_key     TEXT NOT NULL,
    weight      INTEGER NOT NULL DEFAULT 1,
    status      INTEGER NOT NULL DEFAULT 1,  -- 1 = enabled, 0 = disabled
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    FOREIGN KEY (channel_id) REFERENCES channels(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_channel_api_keys_channel
    ON channel_api_keys(channel_id, status);
