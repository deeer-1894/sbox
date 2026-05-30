# Phase 1b (WASM Sandbox) Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 1, WASM criterion)

Verified on 2026-05-30. Toolchain: rustc 1.96.0 (wasmtime 45 / cranelift 0.132
require >= 1.93). Live stack: `ghcr.io/restatedev/restate:latest`,
`restate-sdk-rust/0.8.0`.

| Criterion | Evidence | Status |
| --- | --- | --- |
| WASM tool has no ambient host authority | `aep-sandbox isolation_tests`: a module importing an ungranted `env.host_sink` fails to instantiate under the empty linker | [x] |
| Host access is capability-gated | `aep-sandbox gated_tests` + `effect_tests`: an ungranted capability omits the host fn, so the side effect cannot even link, let alone run | [x] |
| Execution is bounded | `aep-sandbox compute_tests::fuel_bound_stops_runaway`: an infinite loop is trapped by the fuel bound | [x] |
| Sandbox is on the live tool path | `sandbox_chain::permitted_tool_runs_side_effect_in_sandbox` passes — the counter is incremented from inside WASM via `host_sink` | [x] |
| No earlier-phase regression | full `cargo test -p aep-itest -- --ignored` passes (effectively_once + security_chain x3 + sandbox_chain) | [x] |

This closes the third Phase 1 success criterion ("WASM tools cannot access
unauthorized host resources"). Phase 1 is now fully complete.

## Test evidence

```
cargo test -p aep-sandbox            # 7 passed (compute/isolation/gated/effect)
cargo test -p aep-itest -- --ignored
  resend_does_not_re_execute_side_effect ... ok    # Phase 0
  permitted_tool_runs ... ok                        # Phase 1
  denied_tool_does_not_run ... ok                   # Phase 1
  forged_capability_is_rejected_at_tool_boundary ... ok  # Phase 1
  permitted_tool_runs_side_effect_in_sandbox ... ok # Phase 1b
```

## Design notes

- Tool modules are loaded as WAT (Wasmtime `Module::new` accepts text), so the
  isolation properties are proven with no guest toolchain (`wasm32` target /
  `cargo-component` not required for this phase).
- The minted capability stays scoped to the tool resource (`Tool{name}`); the
  sandbox gates `host_sink` on that same resource via `run_tool(.., &cap,
  &Resource::Tool{name}, sink)`. (The plan's original Task 6 — re-scoping the
  capability to a separate `"sink"` resource — was dropped because it would have
  broken ToolService's existing tool-name authorization check.)
- The WASM call is synchronous, run on `spawn_blocking`, inside `ctx.run` so the
  journaled result keeps the side effect effectively-once.

## Known follow-ups (Production phase, not this plan)

- Upgrade core-module WAT to the WASI Preview 2 component model with real guest
  tools (the spec's stated sandbox target).
- Multi-resource capabilities and per-tool WASM artifacts with supply-chain
  verification.
