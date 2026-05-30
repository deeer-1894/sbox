# Phase 3a (Memory Trust & Sanitization) Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 3, memory slice)

Verified on 2026-05-30 against the live stack (`ghcr.io/restatedev/restate:latest`,
`restate-sdk-rust/0.8.0`).

| Criterion | Evidence | Status |
| --- | --- | --- |
| Tool output sanitized before entering memory | `memory::tool_output_is_sanitized_and_labeled_untrusted`: the injection marker is `[REDACTED]` in the stored value, "ignore previous instructions" absent | [x] |
| Tool output classified as untrusted | same test: `entry.trust == "Untrusted"`, `entry.sanitized == true` | [x] |
| Memory carries provenance | same test: `source_capability` starts with `cap-` | [x] |
| Contamination boundary queryable | same test: `MemoryService/{agent}/by_trust("Untrusted")` lists the entry | [x] |
| Sanitize/classify pure + tested | `aep-memory` tests: redaction (case-insensitive), clean passthrough, classify untrusted | [x] |
| No earlier-phase regression | full `cargo test -p aep-itest -- --ignored` passes — 11 tests (effectively_once, security_chain×3, sandbox_chain, quota×3, audit, telemetry, memory) | [x] |

## Findings during execution (folded into the plan)

- `req` is moved into the `ToolCall` before the memory store, so the entry key
  uses `trace.clone()` (== `req.invocation_id` == idempotency_key), not
  `req.invocation_id`.

## Deferred

- **Phase 3b — process-sandbox tier (Level 2):** Linux namespaces/seccomp/cgroups;
  not exercisable on the macOS dev host. Needs a Linux target.
- **Phase 3c — supply-chain verification + SLO dashboards.**
- Production: vector-indexed semantic tier, memory compaction, per-tier retention.

## Reproduce

```bash
docker compose -f deploy/docker-compose.yml up -d
cargo test -p aep-memory
cargo run -p aep-runtime &
./scripts/register.sh
cargo test -p aep-itest -- --ignored
```
