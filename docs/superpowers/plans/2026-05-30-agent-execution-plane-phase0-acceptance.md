# Phase 0 Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 0)

| Criterion | Evidence | Status |
| --- | --- | --- |
| Agent → Tool call runs through a durable side-effect boundary on Restate | Task 7 smoke invoke returns exec_count=1 | [ ] |
| A committed side effect is NOT re-executed on resend | `cargo test -p aep-itest -- --ignored` passes; mutation check (Task 8 step 4) fails when guard removed | [ ] |
| Agent recovers after process crash from the journal | scripts/kill-recover.sh: counter advances by exactly 1 | [ ] |
| Actor logic is deterministic (no ambient now()/uuid() in domain) | `cargo test -p aep-domain` passes; aep-domain has no infra deps | [ ] |

Fill in the Status boxes after a clean run on a fresh `docker compose up`.
