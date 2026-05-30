# Phase 3c (Supply-Chain Verification & SLOs) Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 3, supply-chain + SLO)

Verified on 2026-05-31 against the live stack (`ghcr.io/restatedev/restate:latest`,
`restate-sdk-rust/0.8.0`).

| Criterion | Evidence | Status |
| --- | --- | --- |
| Tool artifact verified before execution | ToolService verifies `TOOL_WAT` against `TRUSTED_ECHO_DIGEST` before the sandbox; the full suite passes with the gate on the path | [x] |
| Tampered artifact rejected (live) | mutation check: adding `;; tampered` to `TOOL_WAT` makes the digest mismatch → the tool invocation returns non-2xx (sandbox_chain fails); reverting restores green | [x] |
| Tampered/unknown artifact rejected (unit) | `aep-supplychain` tests: digest mismatch + unknown artifact rejected | [x] |
| Verification pure + tested | `aep-supplychain` accept/tamper/unknown tests pass (3) | [x] |
| SLOs defined with metric sources | `deploy/slo/README.md` (targets + source signals + alert rules) | [x] |
| No earlier-phase regression | full `cargo test -p aep-itest -- --ignored` passes — 11 tests across 7 binaries | [x] |

## Notes

- `TRUSTED_ECHO_DIGEST` = `ab0c7fdb3a8908e6b70afe42e8c5738ccfbd028648437fa113e45d59008a3a5c`
  (SHA-256 of the audited `TOOL_WAT`). Regenerate if the artifact changes.
- Production replaces the pinned const with a signed artifact manifest
  (sigstore/cosign-style verification).

## Deferred

- **Phase 3b — process-sandbox tier (Level 2):** Linux namespaces/seccomp/cgroups;
  not exercisable on macOS.
- Live Grafana/Prometheus SLO dashboards (registry images rate-limited); the SLO
  definitions + alert rules are in `deploy/slo/README.md`.

## Reproduce

```bash
docker compose -f deploy/docker-compose.yml up -d
cargo test -p aep-supplychain
cargo run -p aep-runtime &
./scripts/register.sh
cargo test -p aep-itest -- --ignored
```
