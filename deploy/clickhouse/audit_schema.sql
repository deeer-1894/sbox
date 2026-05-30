-- Production analytics sink for the audit stream. The aep-audit AuditEvent maps
-- 1:1 to these columns. Restate (journal + AuditService) remains authoritative;
-- ClickHouse is for high-volume causal/audit queries at scale.
--
-- Not run in this environment (image registry unavailable). To use:
--   clickhouse-client < deploy/clickhouse/audit_schema.sql
CREATE TABLE IF NOT EXISTS audit_events (
    trace_id         String,
    message_id       String,
    causal_parent_id Nullable(String),
    actor            LowCardinality(String),
    kind             LowCardinality(String),
    capability_id    Nullable(String),
    invocation_id    Nullable(String),
    detail           String,            -- JSON
    ts               UInt64
)
ENGINE = MergeTree
ORDER BY (trace_id, ts, message_id);

-- Example causal queries:
--   SELECT kind, capability_id, invocation_id FROM audit_events
--     WHERE trace_id = {trace:String} ORDER BY ts, message_id;
--   SELECT capability_id FROM audit_events
--     WHERE trace_id = {trace:String} AND kind = 'tool_requested'
--       AND invocation_id = {inv:String};
