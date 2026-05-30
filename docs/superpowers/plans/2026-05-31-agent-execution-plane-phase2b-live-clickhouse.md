# Agent Execution Plane — Phase 2b-live (ClickHouse Audit Sink) Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Checkbox steps.

**Goal:** Make the audit stream land in a real ClickHouse, queryable by SQL — closing the "live wiring deferred" gap from Phase 2b now that the registry image is pullable.

**Architecture:** ClickHouse runs in Docker (HTTP on 8123). `AuditService.record` best-effort dual-writes each new event to ClickHouse via its HTTP `INSERT ... FORMAT JSONEachRow` interface, inside `ctx.run` (journaled, effectively-once). Restate stays authoritative; a ClickHouse outage never fails a request. Causal/audit queries run as SQL.

**Tech Stack:** Rust (`reqwest`), ClickHouse `clickhouse/clickhouse-server:24.8`. Builds on Phase 2b.

---

## Task 1: ClickHouse service + schema

- [ ] **Step 1: Add ClickHouse to compose.** Append to `deploy/docker-compose.yml`:

```yaml
  clickhouse:
    image: clickhouse/clickhouse-server:24.8
    ports:
      - "8123:8123"
    ulimits:
      nofile: { soft: 262144, hard: 262144 }
```

- [ ] **Step 2: Start it and create the table.**

```bash
docker compose -f deploy/docker-compose.yml up -d clickhouse
# wait for HTTP, then apply schema:
until curl -s http://localhost:8123/ping | grep -q Ok; do sleep 1; done
curl -s 'http://localhost:8123/' --data-binary @deploy/clickhouse/audit_schema.sql
curl -s 'http://localhost:8123/?query=SHOW%20TABLES'   # expect audit_events
```

- [ ] **Step 3: Commit** the compose change.

---

## Task 2: Dual-write from AuditService

- [ ] **Step 1: Best-effort ClickHouse insert in `record`.** In `crates/aep-runtime/src/lib.rs`, in `AuditServiceImpl::record`, inside the `if !events.iter().any(...)` block (new event only), after `ctx.set(...)`, add a journaled best-effort insert:

```rust
            // Best-effort dual-write to ClickHouse (analytics sink). Restate state
            // is authoritative; a ClickHouse outage must not fail the request.
            let row = serde_json::json!({
                "trace_id": event.trace_id,
                "message_id": event.message_id,
                "causal_parent_id": event.causal_parent_id,
                "actor": event.actor,
                "kind": event.kind,
                "capability_id": event.capability_id,
                "invocation_id": event.invocation_id,
                "detail": event.detail.to_string(),
                "ts": event.ts,
            });
            ctx.run(|| async move {
                let url = std::env::var("CLICKHOUSE_URL")
                    .unwrap_or_else(|_| "http://localhost:8123".to_string());
                let body = format!("INSERT INTO audit_events FORMAT JSONEachRow\n{row}");
                let _ = reqwest::Client::new().post(&url).body(body).send().await;
                Ok(())
            })
            .await?;
```

- [ ] **Step 2: Build.** `cargo build -p aep-runtime` → Finished.
- [ ] **Step 3: Commit.**

---

## Task 3: Verify + acceptance

- [ ] **Step 1: Restart runtime, re-register, drive a request.**

```bash
pkill -f 'target/debug/aep-runtime'; cargo run -p aep-runtime &
./scripts/register.sh
KEY="chk-$(uuidgen)"
curl -s localhost:8080/AgentService/agent-ch/handle -H 'content-type: application/json' \
  -d "{\"idempotency_key\":\"$KEY\",\"content\":\"hello\"}" >/dev/null
```

- [ ] **Step 2: Query ClickHouse (SQL).**

```bash
curl -s "http://localhost:8123/?query=SELECT%20count()%20FROM%20audit_events%20WHERE%20trace_id='$KEY'"
# expect 5
curl -s "http://localhost:8123/?query=SELECT%20capability_id%20FROM%20audit_events%20WHERE%20trace_id='$KEY'%20AND%20kind='tool_requested'"
# expect cap-$KEY
```

- [ ] **Step 3: Full regression** still green (`cargo test -p aep-itest -- --ignored`).
- [ ] **Step 4: Acceptance doc + commit + push.**

---

## Notes
- Insert is best-effort + journaled, so replay does not double-insert and a
  ClickHouse outage never fails the agent request.
- Production: batch inserts / async buffer instead of per-event HTTP.
