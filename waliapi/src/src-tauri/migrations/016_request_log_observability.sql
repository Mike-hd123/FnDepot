-- 016: Request-log observability columns (T09)
--
-- Extend `request_logs` so every attempt can carry the routing/observability
-- context that the T05 RoutePlan / PreparedAttempt produce, plus stream
-- lifecycle flags.  ALL new columns are NULLABLE so existing queries and the
-- legacy request-log UI keep working unchanged (old rows simply have NULLs).
--
-- Semantics (task 09 spec, design 6.0.1 / 11.4):
--   * downstream_protocol  — the downstream wire protocol string, e.g. chat_completions
--   * downstream_endpoint  — the downstream endpoint string (chat_completions/responses/...)
--   * route_group          — RoutePlan group id, e.g. "chat_completions_g1_native"
--   * upstream_protocol    — upstream wire protocol actually used (openai/anthropic/ollama)
--   * upstream_endpoint    — upstream native endpoint used (chat_completions/responses/messages/api_chat)
--   * provider             — channel provider string (openai/deepseek/custom/...)
--   * codec_version        — versioned codec label when a conversion ran, e.g. chat_to_messages_v1
--   * failure_class        — T00 decision 5 failure class, e.g. retryable / caller_terminal
--   * identity_revision    — channel identity_revision at request time (0 = legacy-inferred)
--   * client_cancelled     — streaming: client disconnected before completion (0/1/NULL)
--   * stream_committed     -- streaming: first downstream byte was already written (0/1/NULL)
--
-- `upstream_model` already exists (T00-era column); the new columns are purely
-- additive.  No old column is altered or dropped.

ALTER TABLE request_logs ADD COLUMN downstream_protocol TEXT;
ALTER TABLE request_logs ADD COLUMN downstream_endpoint TEXT;
ALTER TABLE request_logs ADD COLUMN route_group TEXT;
ALTER TABLE request_logs ADD COLUMN upstream_protocol TEXT;
ALTER TABLE request_logs ADD COLUMN upstream_endpoint TEXT;
ALTER TABLE request_logs ADD COLUMN provider TEXT;
ALTER TABLE request_logs ADD COLUMN codec_version TEXT;
ALTER TABLE request_logs ADD COLUMN failure_class TEXT;
ALTER TABLE request_logs ADD COLUMN identity_revision INTEGER;
ALTER TABLE request_logs ADD COLUMN client_cancelled INTEGER;
ALTER TABLE request_logs ADD COLUMN stream_committed INTEGER;

CREATE INDEX IF NOT EXISTS idx_logs_route_group ON request_logs(route_group);
CREATE INDEX IF NOT EXISTS idx_logs_upstream_protocol ON request_logs(upstream_protocol);
CREATE INDEX IF NOT EXISTS idx_logs_failure_class ON request_logs(failure_class);
