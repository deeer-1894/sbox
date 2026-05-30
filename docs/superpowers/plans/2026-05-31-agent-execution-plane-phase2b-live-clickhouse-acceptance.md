# Phase 2b-live (ClickHouse Audit Sink) Acceptance — Agent Execution Plane

Closes the "ClickHouse live wiring deferred" note from Phase 2b acceptance.

Verified 2026-05-31. ClickHouse `clickhouse/clickhouse-server:24.8` (HTTP 8123).

| Criterion | Evidence | Status |
| --- | --- | --- |
| Audit stream lands in ClickHouse | one agent run → `SELECT count() WHERE trace_id=K` returns 5 | [x] |
| Causal query answerable in SQL | `SELECT capability_id WHERE kind='tool_requested'` returns `cap-K` | [x] |
| All causal kinds present | input, policy_permit, capability_minted, tool_requested, tool_completed | [x] |
| Dual-write is best-effort + non-blocking | insert wrapped in ctx.run (journaled); errors ignored; a ClickHouse outage does not fail the agent request | [x] |
| No regression | full `cargo test -p aep-itest -- --ignored` passes (7 binaries, 0 failures) | [x] |

## How

- ClickHouse added to `deploy/docker-compose.yml` (user `aep`, the 24.8 image
  rejects the empty-password default user).
- Schema applied from `deploy/clickhouse/audit_schema.sql`.
- `AuditService.record` best-effort `INSERT ... FORMAT JSONEachRow` over HTTP,
  inside `ctx.run` so replay does not double-insert. `CLICKHOUSE_URL` overrides
  the default `http://aep:aep@localhost:8123`.

## Reproduce

```bash
docker compose -f deploy/docker-compose.yml up -d
until curl -s http://localhost:8123/ping | grep -q Ok; do sleep 1; done
curl -s 'http://aep:aep@localhost:8123/' --data-binary @deploy/clickhouse/audit_schema.sql
cargo run -p aep-runtime &
./scripts/register.sh
KEY="chk-$(uuidgen)"
curl -s localhost:8080/AgentService/a/handle -H 'content-type: application/json' \
  -d "{\"idempotency_key\":\"$KEY\",\"content\":\"hi\"}" >/dev/null
curl -s "http://aep:aep@localhost:8123/?query=SELECT%20count()%20FROM%20audit_events%20WHERE%20trace_id='$KEY'"
```

## Production follow-ups

Batch/async-buffered inserts instead of per-event HTTP; a ReplacingMergeTree or
dedup key for at-least-once edges; the same seam for an OTLP→Jaeger live export.
