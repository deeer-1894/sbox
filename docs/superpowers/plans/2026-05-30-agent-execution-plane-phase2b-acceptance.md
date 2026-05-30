# Phase 2b (Audit & Causal Query) Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 2, audit slice)

Verified on 2026-05-30 against the live stack (`ghcr.io/restatedev/restate:latest`,
`restate-sdk-rust/0.8.0`).

| Criterion | Evidence | Status |
| --- | --- | --- |
| Causal chain reconstructable | `audit::causal_chain_is_reconstructable`: `AuditService/{trace}/chain` returns `input → policy_permit → capability_minted → tool_requested → tool_completed` in causal order | [x] |
| "Which capability authorized this side effect?" answerable | same test: `AuditService/{trace}/capability` returns `cap-{trace}` | [x] |
| Causal query logic pure + tested | `aep-audit` tests: `causal_chain` ordering (scrambled input), `capability_for` lookup | [x] |
| Audit sink is durable + idempotent | `AuditService` keyed by trace; `record` dedups by `message_id` so replay re-emission is a no-op | [x] |
| No earlier-phase regression | full `cargo test -p aep-itest -- --ignored` passes — 9 tests (effectively_once, security_chain×3, sandbox_chain, quota×3, audit) | [x] |

## ClickHouse

Production analytics sink. The `aep-audit` `AuditEvent` maps 1:1 to the table in
`deploy/clickhouse/audit_schema.sql`. Restate (journal + AuditService) remains
authoritative; ClickHouse is for high-volume causal/audit queries at scale.
**Live wiring deferred:** the ClickHouse image is not pullable in this
environment (Docker Hub anon rate-limited, no ghcr mirror). The `AuditService`
seam makes a ClickHouse-backed sink a drop-in addition.

## Findings during execution (folded into the plan)

- A Restate client `.call().await` returns `Result<_, TerminalError>`; a helper
  returning `Result<_, HandlerError>` must `?`-convert it (`...call().await?; Ok(())`).

Deferred: Phase 2c (OpenTelemetry). Production: live ClickHouse sink; per-actor
emission (ToolService emits its own `tool_completed`); memory-write audit events
once MemoryService exists (Phase 3).

## Reproduce

```bash
docker compose -f deploy/docker-compose.yml up -d
cargo test -p aep-audit
cargo run -p aep-runtime &
./scripts/register.sh
cargo test -p aep-itest -- --ignored
```
