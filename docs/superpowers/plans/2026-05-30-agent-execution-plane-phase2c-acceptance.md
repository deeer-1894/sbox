# Phase 2c (OpenTelemetry Spans) Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 2, OTel slice)

Verified on 2026-05-30 against the live stack (`ghcr.io/restatedev/restate:latest`,
`restate-sdk-rust/0.8.0`).

| Criterion | Evidence | Status |
| --- | --- | --- |
| Spans carry required causal attributes | `aep-telemetry span_fields_carries_required_attributes` (trace_id/actor/kind) | [x] |
| Each actor turn emits a span | `telemetry::actor_turns_are_observable_per_trace`: `/spans/{trace}` contains `AgentService` + `ToolService` | [x] |
| Spans queryable per trace | same test via `/spans/{trace}` | [x] |
| Capture idempotent (replay-safe) | `aep-telemetry capture_records_and_dedups` | [x] |
| No earlier-phase regression | full `cargo test -p aep-itest -- --ignored` passes — 10 tests (effectively_once, security_chain×3, sandbox_chain, quota×3, audit, telemetry) | [x] |

## OTLP export

Documented production wiring in `deploy/otel/README.md` (verified
`opentelemetry-otlp` init snippet). Live collector export is deferred — the
collector image is registry-rate-limited (same as ClickHouse in Phase 2b).
`tracing` is the OTel-recommended Rust instrumentation API; the documented
`tracing-opentelemetry` bridge ships these spans to OTLP unchanged.

## Findings during execution (folded into the plan)

- `aep-runtime` did not depend on `tracing` directly (only used it transitively);
  added `tracing` to the workspace + crate deps for the instrumentation events.

This completes **Phase 2** (quota/backpressure + audit/causal query + OTel spans).

## Reproduce

```bash
docker compose -f deploy/docker-compose.yml up -d
cargo test -p aep-telemetry
cargo run -p aep-runtime &
./scripts/register.sh
cargo test -p aep-itest -- --ignored
```
