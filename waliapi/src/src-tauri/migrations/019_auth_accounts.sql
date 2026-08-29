CREATE TABLE auth_accounts (
    id                   TEXT PRIMARY KEY,
    provider             TEXT NOT NULL,
    label                TEXT NOT NULL,
    account_id           TEXT NOT NULL,
    status               TEXT NOT NULL DEFAULT 'active',
    disabled             INTEGER NOT NULL DEFAULT 0,
    priority             INTEGER NOT NULL DEFAULT 0,
    weight               INTEGER NOT NULL DEFAULT 1,
    quota_json           TEXT,
    model_states_json    TEXT NOT NULL DEFAULT '{"version":1,"models":[]}',
    attributes_json      TEXT NOT NULL DEFAULT '{}',
    payload_json         TEXT NOT NULL,
    last_refreshed_at    TEXT,
    last_models_sync_at  TEXT,
    next_refresh_after   TEXT,
    next_retry_after     TEXT,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    UNIQUE(provider, account_id),
    CHECK (disabled IN (0, 1)),
    CHECK (priority >= 0),
    CHECK (weight >= 1),
    CHECK (status IN ('active', 'invalid'))
);

CREATE INDEX idx_auth_accounts_route
    ON auth_accounts(disabled, status, priority, provider);

ALTER TABLE request_logs
    ADD COLUMN upstream_type TEXT NOT NULL DEFAULT 'channel';
