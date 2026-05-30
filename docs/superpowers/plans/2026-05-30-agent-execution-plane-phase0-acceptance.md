# Phase 0 Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 0)

Verified on 2026-05-30 against a live stack: `ghcr.io/restatedev/restate:latest`
(Restate server), `restate-sdk-rust/0.8.0`, two registered Virtual Objects
(`AgentService.handle`, `ToolService.run`).

| Criterion | Evidence | Status |
| --- | --- | --- |
| Agent → Tool call runs through a durable side-effect boundary on Restate | Smoke invoke returned `{"output":{"echo":{"content":"hello"}},"exec_count":1}`; counter `0 → 1` | [x] |
| A committed side effect is NOT re-executed on resend | `cargo test -p aep-itest -- --ignored` → `resend_does_not_re_execute_side_effect ... ok`. Mutation check: with the journal guard removed, the same test FAILS on `resend must NOT re-run the side effect` (counter increments twice), then PASSES again after revert | [x] |
| Agent recovers after process crash from the journal | `./scripts/kill-recover.sh` → `PASS`: after `kill -9` + restart, resend reused the journaled completion (`exec_count=1`) and did not re-run the side effect (fresh in-process counter still `0`) | [x] |
| Actor logic is deterministic (no ambient now()/uuid() in domain) | `cargo test -p aep-domain` → 4 passed; `aep-domain` has zero infrastructure dependencies (serde/serde_json/sha2 only) | [x] |

## Notes / known Phase 0 limitations

- The "external" counter sidecar lives inside the `aep-runtime` process, so a
  `kill -9` resets it. This is intentional for the demo and makes "no
  re-execution after crash" unambiguous (the fresh counter staying at 0 proves
  the side effect was not POSTed again). A production external effect would
  persist via its own `external_reference`.
- Effectively-once holds across resends and across a clean crash/restart because
  the `ToolCompleted` is journaled in restate-server. The classic narrow window
  (external effect performed but its `ctx.run` journal entry not yet persisted
  when the process dies) is the inherent at-least-once boundary and is addressed
  in later phases via external reconciliation (spec §"Replay and Reconciliation
  Rules").
- Docker Hub anonymous pulls are rate-limited (HTTP 429); the compose file uses
  the `ghcr.io` mirror to avoid this.

## Reproduce

```bash
docker compose -f deploy/docker-compose.yml up -d        # Restate
cargo test -p aep-domain                                 # pure logic, 4 pass
cargo run -p aep-runtime &                               # service on :9080 + counter :9090
./scripts/register.sh                                    # discover the two objects
cargo test -p aep-itest -- --ignored                     # effectively-once, 1 pass
./scripts/kill-recover.sh                                 # crash recovery, PASS
```
