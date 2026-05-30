# Phase 1 (Authorization Chain) Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 1)

Verified on 2026-05-30 against the live stack (`ghcr.io/restatedev/restate:latest`,
`restate-sdk-rust/0.8.0`, `cedar-policy 4.11`).

| Criterion | Evidence | Status |
| --- | --- | --- |
| Tool calls require policy approval | `security_chain::denied_tool_does_not_run` passes — `requested_tool=shell` returns `denied:true` and the counter does not advance | [x] |
| Model/agent intent cannot directly execute | `security_chain::forged_capability_is_rejected_at_tool_boundary` passes — a direct `ToolService.run` with `"not.a.valid.token"` returns non-2xx and the counter does not advance | [x] |
| Capability is scoped and short-lived | `aep-capability` tests (8): wrong-resource / wrong-action / expired all rejected; mint sets `expires_at = now + 300s` | [x] |
| Policy is declarative and default-deny | `aep-policy` tests (2): echo/upper Permit, shell Deny via `tools.cedar` allowlist | [x] |
| Determinism preserved | capability time injected via `ctx.run` (`now_unix`); `aep-capability` + `aep-policy` are infra-free and unit-tested | [x] |
| No Phase 0 regression | `effectively_once` still passes (new `UserInput`/`AgentReply` fields are `#[serde(default)]`) | [x] |

## Test evidence

```
cargo test -p aep-capability      # 8 passed
cargo test -p aep-policy          # 2 passed
cargo test -p aep-domain          # 6 passed
cargo test -p aep-itest --test security_chain -- --ignored
  permitted_tool_runs_once ... ok
  denied_tool_does_not_run ... ok
  forged_capability_is_rejected_at_tool_boundary ... ok
cargo test -p aep-itest --test effectively_once -- --ignored   # 1 passed (no regression)
```

## Deferred to Phase 1b

"WASM tools cannot access unauthorized host resources" — the Wasmtime/WASI
Preview 2 sandbox. The minted capability already carries the scope the sandbox
will enforce; Phase 1b replaces the fake echo tool with a real sandboxed tool.

## Notes

- Phase 1a audit is structured `tracing` (capability rejections / policy denials);
  durable audit to ClickHouse is Phase 2.
- Single-node capability secret is symmetric HMAC (`AEP_CAP_SECRET`, dev default).
  Production uses an asymmetric key so verifiers never hold the signing key.

## Reproduce

```bash
docker compose -f deploy/docker-compose.yml up -d
cargo test -p aep-capability -p aep-policy -p aep-domain
cargo run -p aep-runtime &
./scripts/register.sh
cargo test -p aep-itest --test security_chain -- --ignored
cargo test -p aep-itest --test effectively_once -- --ignored
```
