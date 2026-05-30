# Phase 2a (Tenant Quota & Backpressure) Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 2, quota slice)

Verified on 2026-05-30 against the live stack (`ghcr.io/restatedev/restate:latest`,
`restate-sdk-rust/0.8.0`).

| Criterion | Evidence | Status |
| --- | --- | --- |
| Per-tenant concurrency quota enforced | `quota::quota_admits_then_rejects_then_recovers` passes — limit 1: acquire→true, acquire→false, release, acquire→true | [x] |
| Tenant quotas apply backpressure | `quota::agent_request_is_backpressured_when_tenant_exhausted` passes — tenant limit 0 → AgentService returns `denied:true`, reason "tenant quota exceeded" | [x] |
| Admission rule is pure + tested | `aep-domain admit_tests` pass (7 domain tests total) | [x] |
| No earlier-phase regression | full `cargo test -p aep-itest -- --ignored` passes — 8 tests (effectively_once, security_chain×3, sandbox_chain, quota×3) | [x] |

Deferred: Phase 2b (ClickHouse audit + causal query API), Phase 2c (OpenTelemetry).

## Findings during execution (folded back into the plan)

- **Re-registration needs `force:true`.** Restate ignores a re-registration of an
  already-known deployment URI, so a newly added service (TenantService) never
  appeared until `scripts/register.sh` was changed to send `{"force":true}`.
  Re-run `register.sh` after adding/changing any service.
- **No-argument Restate handlers require an empty body and no content-type.**
  `acquire`/`release`/`in_flight` reject a JSON body with HTTP 400
  ("Expected body and content-type to be empty"). Integration tests call them
  with a bodyless POST (`post_empty`); `set_limit` (which takes an arg) uses a
  JSON body.

## Known limitation

Release is a bare `saturating_sub` decrement; a retry after a lost release-ack
could over-release (free a slot early). A lease-id model is a later-phase fix;
`saturating_sub` prevents underflow meanwhile.

## Reproduce

```bash
docker compose -f deploy/docker-compose.yml up -d
cargo test -p aep-domain
cargo run -p aep-runtime &
./scripts/register.sh          # force:true picks up TenantService
cargo test -p aep-itest -- --ignored
```
