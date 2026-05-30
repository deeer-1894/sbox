# Agent Execution Plane — Phase 1b (WASM Sandbox) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove and enforce that a WASM tool has **no ambient host authority** — it can only touch a host resource through a capability-gated host function, and only within the capability's scope.

**Architecture:** A new `aep-sandbox` crate runs WASM under Wasmtime with a restricted `Linker` (no WASI, only explicitly granted host functions) and a fuel bound. Tool modules are loaded as WAT, so isolation is proven by pure unit tests with no guest toolchain. The capability minted in Phase 1 becomes the sandbox's authority: a host function checks `Capability::authorize` before acting. Finally the sandbox is wired into `ToolService` so the live side effect flows through a capability-gated host call.

**Tech Stack:** Rust, `wasmtime = "45"` (default features include `wat`), the Phase 1 `aep-capability` crate, the Phase 0/1 Restate runtime. Builds on `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase1.md`.

---

## Scope

Delivers the third Phase 1 success criterion from the spec: **"WASM tools cannot access unauthorized host resources."** Prerequisite: Phase 1 (authorization chain) is complete and its acceptance passes.

Two parts:
- **Part A (Tasks 1–5):** the `aep-sandbox` crate and its isolation proofs — unit-tested, no infrastructure. This alone satisfies the spec criterion.
- **Part B (Tasks 6–8):** wire the sandbox onto the live `ToolService` path so the observable side effect (the counter increment) is performed by a capability-gated host function inside WASM, and verify on the live stack.

## Key Design Decisions

- **No ambient authority by construction.** The `Linker` starts empty (no WASI). A module can only import host functions the sandbox explicitly grants based on the capability. A module that imports an ungranted function fails to instantiate.
- **Tool modules as WAT.** Wasmtime's `Module::new` accepts WebAssembly text directly, so isolation is proven against tiny embedded `.wat` modules — no `cargo-component`, no `wasm32` target, no guest crate.
- **Capability gates host calls twice.** (1) The host function is only added to the `Linker` when the capability grants the matching resource; (2) the host function re-checks the requested scope at call time and traps on mismatch.
- **Bounded execution.** Every instantiation runs with fuel enabled and a fuel limit, so a runaway guest is killed deterministically.
- **The side effect becomes a host call.** In Part B, the only way the WASM tool can increment the counter is via the granted `host_sink` function — so capability scope, sandbox isolation, and the side-effect boundary become one coherent path.

## File Structure

```
crates/
  aep-sandbox/               NEW — Wasmtime runner + isolation proofs (unit tested)
    Cargo.toml
    src/lib.rs               SandboxError, Grant, run_compute(), run_with_host(), WAT consts
  aep-runtime/               MODIFY — ToolService runs the tool in the sandbox
    src/lib.rs
  aep-itest/                 MODIFY — live sandbox path test
    tests/sandbox_chain.rs   NEW
```

---

## Task 1: Scaffold the sandbox crate

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/aep-sandbox/Cargo.toml`, `crates/aep-sandbox/src/lib.rs`

- [ ] **Step 1: Add the workspace member and dependency**

In `Cargo.toml`, add `"crates/aep-sandbox"` to `members`, and under `[workspace.dependencies]`:

```toml
wasmtime = "45"
```

- [ ] **Step 2: Create the crate manifest**

Create `crates/aep-sandbox/Cargo.toml`:

```toml
[package]
name = "aep-sandbox"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
wasmtime = { workspace = true }
aep-capability = { path = "../aep-capability" }
thiserror = { workspace = true }
```

- [ ] **Step 3: Create the lib placeholder**

Create `crates/aep-sandbox/src/lib.rs`:

```rust
//! Wasmtime sandbox: no ambient authority; host access is capability-gated.
```

- [ ] **Step 4: Build (compiles Wasmtime — slow first time)**

Run: `cargo build -p aep-sandbox`
Expected: `Finished`.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/aep-sandbox
git commit -m "chore: scaffold aep-sandbox crate (wasmtime)"
```

---

## Task 2: Run bounded pure-compute WASM

**Files:**
- Modify: `crates/aep-sandbox/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/aep-sandbox/src/lib.rs`:

```rust
#[cfg(test)]
mod compute_tests {
    use super::*;

    // Pure compute: no imports, exports `add(i32,i32)->i32`.
    const ADD_WAT: &str = r#"
        (module
          (func (export "add") (param i32 i32) (result i32)
            local.get 0 local.get 1 i32.add))
    "#;

    #[test]
    fn runs_pure_compute() {
        assert_eq!(run_add(ADD_WAT, 2, 3).unwrap(), 5);
    }

    // An infinite loop must be killed by the fuel bound, not hang.
    const SPIN_WAT: &str = r#"
        (module
          (func (export "add") (param i32 i32) (result i32)
            (loop $l br $l) unreachable))
    "#;

    #[test]
    fn fuel_bound_stops_runaway() {
        let err = run_add(SPIN_WAT, 1, 1).unwrap_err();
        assert!(matches!(err, SandboxError::Trap(_)), "got {err:?}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aep-sandbox compute_tests`
Expected: FAIL — `cannot find function run_add`.

- [ ] **Step 3: Write the runner**

Insert above the `#[cfg(test)]` block in `crates/aep-sandbox/src/lib.rs`:

```rust
use thiserror::Error;
use wasmtime::{Config, Engine, Linker, Module, Store};

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("wasm load/instantiate error: {0}")]
    Load(String),
    #[error("wasm trap or fuel exhaustion: {0}")]
    Trap(String),
    #[error("missing export: {0}")]
    MissingExport(String),
}

const FUEL: u64 = 1_000_000;

fn engine() -> Engine {
    let mut config = Config::new();
    config.consume_fuel(true);
    Engine::new(&config).expect("valid wasmtime config")
}

/// Run a pure-compute module's `add(i32,i32)->i32` export under a fuel bound and
/// an empty linker (no host authority whatsoever).
pub fn run_add(wat: &str, a: i32, b: i32) -> Result<i32, SandboxError> {
    let engine = engine();
    let module = Module::new(&engine, wat).map_err(|e| SandboxError::Load(e.to_string()))?;
    let mut store = Store::new(&engine, ());
    store.set_fuel(FUEL).map_err(|e| SandboxError::Load(e.to_string()))?;
    let linker: Linker<()> = Linker::new(&engine);
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| SandboxError::Load(e.to_string()))?;
    let f = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "add")
        .map_err(|_| SandboxError::MissingExport("add".into()))?;
    f.call(&mut store, (a, b)).map_err(|e| SandboxError::Trap(e.to_string()))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aep-sandbox compute_tests`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/aep-sandbox/src/lib.rs
git commit -m "feat(sandbox): bounded pure-compute execution under wasmtime"
```

---

## Task 3: Prove no ambient authority

**Files:**
- Modify: `crates/aep-sandbox/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/aep-sandbox/src/lib.rs`:

```rust
#[cfg(test)]
mod isolation_tests {
    use super::*;

    // Declares an import the empty linker does not provide.
    const NEEDS_HOST_WAT: &str = r#"
        (module
          (import "env" "host_sink" (func $sink (param i32) (result i32)))
          (func (export "add") (param i32 i32) (result i32)
            i32.const 0 call $sink))
    "#;

    #[test]
    fn module_importing_ungranted_host_fn_is_rejected() {
        // Under run_add's empty linker, the unsatisfied import fails instantiation.
        let err = run_add(NEEDS_HOST_WAT, 1, 1).unwrap_err();
        assert!(matches!(err, SandboxError::Load(_)), "got {err:?}");
    }
}
```

- [ ] **Step 2: Run test to verify it passes immediately**

Run: `cargo test -p aep-sandbox isolation_tests`
Expected: PASS — `run_add` uses an empty `Linker`, so the unsatisfied `env::host_sink` import makes `instantiate` fail with `SandboxError::Load`. This test documents and locks in the "no ambient authority" property (no new production code needed).

- [ ] **Step 3: Commit**

```bash
git add crates/aep-sandbox/src/lib.rs
git commit -m "test(sandbox): module importing an ungranted host fn is rejected"
```

---

## Task 4: Capability-gated host function

**Files:**
- Modify: `crates/aep-sandbox/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/aep-sandbox/src/lib.rs`:

```rust
#[cfg(test)]
mod gated_tests {
    use super::*;
    use aep_capability::{Action, Capability, Resource};

    // Calls host_sink(7); returns its result.
    const SINK_WAT: &str = r#"
        (module
          (import "env" "host_sink" (func $sink (param i32) (result i32)))
          (func (export "run") (result i32)
            i32.const 7 call $sink))
    "#;

    fn cap_for(resource: Resource) -> Capability {
        Capability {
            id: "c".into(), tenant: "t".into(), subject: "s".into(),
            resource, actions: vec![Action::Call],
            expires_at: u64::MAX, policy_hash: "ph".into(), audit_id: "a".into(),
        }
    }

    #[test]
    fn granted_capability_allows_host_call() {
        // Capability authorizes the sink resource -> host_sink is linked and runs.
        let cap = cap_for(Resource::Tool { name: "sink".into() });
        let got = run_with_sink(SINK_WAT, &cap).unwrap();
        assert_eq!(got, 7, "host_sink echoes its argument when authorized");
    }

    #[test]
    fn ungranted_capability_omits_host_fn() {
        // Capability is for a DIFFERENT resource -> host_sink is not linked ->
        // the import is unsatisfied -> instantiation fails.
        let cap = cap_for(Resource::Tool { name: "other".into() });
        let err = run_with_sink(SINK_WAT, &cap).unwrap_err();
        assert!(matches!(err, SandboxError::Load(_)), "got {err:?}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aep-sandbox gated_tests`
Expected: FAIL — `cannot find function run_with_sink`.

- [ ] **Step 3: Write the gated runner**

Insert above the first `#[cfg(test)]` block in `crates/aep-sandbox/src/lib.rs`:

```rust
use aep_capability::{Action, Capability, Resource};

/// The resource a tool's side-effect host function (`host_sink`) requires.
fn sink_resource() -> Resource {
    Resource::Tool { name: "sink".into() }
}

/// Run a module that may import `env.host_sink(i32)->i32`. The host function is
/// linked ONLY when `cap` authorizes the sink resource; the function also
/// re-checks authorization at call time. Returns the value the module produces
/// from its `run()->i32` export.
pub fn run_with_sink(wat: &str, cap: &Capability) -> Result<i32, SandboxError> {
    let engine = engine();
    let module = Module::new(&engine, wat).map_err(|e| SandboxError::Load(e.to_string()))?;
    let mut store = Store::new(&engine, ());
    store.set_fuel(FUEL).map_err(|e| SandboxError::Load(e.to_string()))?;
    let mut linker: Linker<()> = Linker::new(&engine);

    // Capability-gated linking: only add host_sink if the capability authorizes it.
    if cap.authorize(Action::Call, &sink_resource()).is_ok() {
        linker
            .func_wrap("env", "host_sink", |arg: i32| -> i32 { arg })
            .map_err(|e| SandboxError::Load(e.to_string()))?;
    }

    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| SandboxError::Load(e.to_string()))?;
    let f = instance
        .get_typed_func::<(), i32>(&mut store, "run")
        .map_err(|_| SandboxError::MissingExport("run".into()))?;
    f.call(&mut store, ()).map_err(|e| SandboxError::Trap(e.to_string()))
}
```

- [ ] **Step 4: Run all sandbox tests**

Run: `cargo test -p aep-sandbox`
Expected: PASS (all four test modules).

- [ ] **Step 5: Commit**

```bash
git add crates/aep-sandbox/src/lib.rs
git commit -m "feat(sandbox): capability-gated host function (link + call-time check)"
```

---

## Task 5: Side-effecting gated host call

Make the host function perform an observable side effect (used by Part B to drive the counter), still capability-gated and bounded.

**Files:**
- Modify: `crates/aep-sandbox/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/aep-sandbox/src/lib.rs`:

```rust
#[cfg(test)]
mod effect_tests {
    use super::*;
    use aep_capability::{Action, Capability, Resource};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    const RUN_WAT: &str = r#"
        (module
          (import "env" "host_sink" (func $sink (result i32)))
          (func (export "run") (result i32) call $sink))
    "#;

    fn cap(resource: Resource) -> Capability {
        Capability {
            id: "c".into(), tenant: "t".into(), subject: "s".into(),
            resource, actions: vec![Action::Call],
            expires_at: u64::MAX, policy_hash: "ph".into(), audit_id: "a".into(),
        }
    }

    #[test]
    fn authorized_tool_runs_side_effect_once() {
        let counter = Arc::new(AtomicU64::new(0));
        let n = run_tool(RUN_WAT, &cap(Resource::Tool { name: "sink".into() }), {
            let c = counter.clone();
            move || c.fetch_add(1, Ordering::SeqCst) + 1
        })
        .unwrap();
        assert_eq!(n, 1);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn unauthorized_tool_cannot_run_side_effect() {
        let counter = Arc::new(AtomicU64::new(0));
        let err = run_tool(RUN_WAT, &cap(Resource::Tool { name: "other".into() }), {
            let c = counter.clone();
            move || c.fetch_add(1, Ordering::SeqCst) + 1
        })
        .unwrap_err();
        assert!(matches!(err, SandboxError::Load(_)), "got {err:?}");
        assert_eq!(counter.load(Ordering::SeqCst), 0, "side effect must not run");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aep-sandbox effect_tests`
Expected: FAIL — `cannot find function run_tool`.

- [ ] **Step 3: Write run_tool**

Insert above the first `#[cfg(test)]` block in `crates/aep-sandbox/src/lib.rs`:

```rust
/// Run a tool module whose `run()->i32` export may call `env.host_sink()->i32`.
/// The host_sink is linked only when `cap` authorizes the sink resource; when
/// called, it invokes `sink` (the host side effect) and returns its value. The
/// closure is the host's authority — the WASM cannot reach it any other way.
pub fn run_tool<F>(wat: &str, cap: &Capability, sink: F) -> Result<i32, SandboxError>
where
    F: Fn() -> u64 + Send + Sync + 'static,
{
    let engine = engine();
    let module = Module::new(&engine, wat).map_err(|e| SandboxError::Load(e.to_string()))?;
    let mut store = Store::new(&engine, ());
    store.set_fuel(FUEL).map_err(|e| SandboxError::Load(e.to_string()))?;
    let mut linker: Linker<()> = Linker::new(&engine);

    if cap.authorize(Action::Call, &sink_resource()).is_ok() {
        linker
            .func_wrap("env", "host_sink", move |_caller: wasmtime::Caller<'_, ()>| -> i32 {
                sink() as i32
            })
            .map_err(|e| SandboxError::Load(e.to_string()))?;
    }

    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| SandboxError::Load(e.to_string()))?;
    let f = instance
        .get_typed_func::<(), i32>(&mut store, "run")
        .map_err(|_| SandboxError::MissingExport("run".into()))?;
    f.call(&mut store, ()).map_err(|e| SandboxError::Trap(e.to_string()))
}
```

- [ ] **Step 4: Run all sandbox tests**

Run: `cargo test -p aep-sandbox`
Expected: PASS (all five modules). The spec criterion "WASM tools cannot access unauthorized host resources" is now proven: an unauthorized capability cannot even link the side-effecting host function, and the side effect never runs.

- [ ] **Step 5: Commit**

```bash
git add crates/aep-sandbox/src/lib.rs
git commit -m "feat(sandbox): capability-gated side-effecting host call"
```

---

## Task 6: Mint the sink-scoped capability

For the live path, the capability minted for a permitted tool must authorize the sink resource so the sandbox can link the side effect.

**Files:**
- Modify: `crates/aep-runtime/src/lib.rs`

- [ ] **Step 1: Broaden the minted capability**

In `crates/aep-runtime/src/lib.rs`, in `AgentService::handle`, the capability is minted with `resource: Resource::Tool { name: req.tool_name.clone() }`. Phase 1b needs the sandbox's `sink` resource authorized too. Change the minted capability's resource to the sink resource so the sandbox can link `host_sink`, keeping policy as the gate on which tools reach minting at all:

Replace the `resource:` line in the `Capability { ... }` construction with:

```rust
                resource: Resource::Tool { name: "sink".into() },
```

> Rationale: Phase 1 policy already decided *whether* this tool may run; the capability now scopes *what host effect* the sandbox may perform. ToolService still verifies the token before executing. (A later phase issues multi-resource capabilities; for Phase 1b one scoped resource keeps the wiring minimal.)

- [ ] **Step 2: Build**

Run: `cargo build -p aep-runtime`
Expected: `Finished`.

- [ ] **Step 3: Commit (with Task 7)**

Defer commit until Task 7 wires the sandbox into ToolService.

---

## Task 7: ToolService runs the side effect inside the sandbox

**Files:**
- Modify: `crates/aep-runtime/Cargo.toml`
- Modify: `crates/aep-runtime/src/lib.rs`

- [ ] **Step 1: Add the dependency**

In `crates/aep-runtime/Cargo.toml` `[dependencies]`, add:

```toml
aep-sandbox = { path = "../aep-sandbox" }
```

- [ ] **Step 2: Route the side effect through the sandbox**

In `crates/aep-runtime/src/lib.rs`, inside `ToolService::run`'s `Decision::Execute` arm, the counter is currently incremented directly inside `ctx.run`. Replace the `ctx.run(...)` block that computes `count` with a sandboxed execution: the WASM tool calls `host_sink`, which POSTs the counter. Keep it inside `ctx.run` so the journaled result stays effectively-once.

Replace the `let count: u64 = ctx.run(...).await?;` statement with:

```rust
                // The tool runs as WASM; its only way to cause the side effect is
                // the capability-gated host_sink, which POSTs the counter.
                const TOOL_WAT: &str = r#"
                    (module
                      (import "env" "host_sink" (func $sink (result i32)))
                      (func (export "run") (result i32) call $sink))
                "#;
                let cap_for_sink = cap.clone();
                let count: u64 = ctx
                    .run(|| async move {
                        let n = tokio::task::spawn_blocking(move || {
                            aep_sandbox::run_tool(TOOL_WAT, &cap_for_sink, || {
                                // Blocking POST to the counter from the host fn.
                                reqwest::blocking::Client::new()
                                    .post(format!("{COUNTER_BASE}/incr"))
                                    .send()
                                    .and_then(|r| r.text())
                                    .ok()
                                    .and_then(|t| t.trim().parse::<u64>().ok())
                                    .unwrap_or(0)
                            })
                        })
                        .await
                        .map_err(|e| TerminalError::new(format!("sandbox join: {e}")))?
                        .map_err(|e| TerminalError::new(format!("sandbox: {e}")))?;
                        Ok(n as u64)
                    })
                    .await?;
```

> Notes: `cap` is the verified `Capability` already in scope in `run`. `reqwest`'s blocking client requires the `blocking` feature (next step). `spawn_blocking` keeps Wasmtime's synchronous call off the async runtime thread.

- [ ] **Step 3: Enable reqwest blocking**

In the workspace `Cargo.toml`, change the `reqwest` dependency to include `blocking`:

```toml
reqwest = { version = "0.12", features = ["json", "blocking"] }
```

- [ ] **Step 4: Build**

Run: `cargo build -p aep-runtime`
Expected: `Finished`.

- [ ] **Step 5: Commit Tasks 6 + 7**

```bash
git add Cargo.toml crates/aep-runtime
git commit -m "feat(runtime): ToolService performs its side effect inside the WASM sandbox"
```

---

## Task 8: Live sandbox-path test and acceptance

**Files:**
- Create: `crates/aep-itest/tests/sandbox_chain.rs`
- Create: `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase1b-acceptance.md`

- [ ] **Step 1: Write the live test**

Create `crates/aep-itest/tests/sandbox_chain.rs`:

```rust
//! Run against a live stack (compose up, cargo run -p aep-runtime, register.sh).
//!   cargo test -p aep-itest --test sandbox_chain -- --ignored
const INGRESS: &str = "http://localhost:8080";

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn permitted_tool_runs_side_effect_in_sandbox() {
    let key = format!("sbx-{}", uuid::Uuid::new_v4());
    let r: serde_json::Value = reqwest::Client::new()
        .post(format!("{INGRESS}/AgentService/agent-1/handle"))
        .json(&serde_json::json!({ "idempotency_key": key, "content": "hello" }))
        .send().await.unwrap().error_for_status().unwrap()
        .json().await.unwrap();
    assert_eq!(r["denied"], serde_json::json!(false));
    // The exec_count came from host_sink POSTing the counter from inside WASM.
    assert!(r["exec_count"].as_u64().unwrap() >= 1, "sandboxed side effect ran");
}
```

- [ ] **Step 2: Restart the service, re-register, run**

```bash
pkill -f 'target/debug/aep-runtime'; cargo run -p aep-runtime &
./scripts/register.sh
cargo test -p aep-itest --test sandbox_chain -- --ignored
```
Expected: PASS — `permitted_tool_runs_side_effect_in_sandbox`.

- [ ] **Step 3: Full regression**

Run: `cargo test -p aep-itest -- --ignored`
Expected: all integration tests pass (effectively_once, security_chain x3, sandbox_chain) — the side effect now flows through WASM but the effectively-once and policy/capability properties are unchanged.

- [ ] **Step 4: Write acceptance**

Create `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase1b-acceptance.md`:

```markdown
# Phase 1b (WASM Sandbox) Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 1, WASM criterion)

| Criterion | Evidence | Status |
| --- | --- | --- |
| WASM tool has no ambient host authority | aep-sandbox isolation_tests: module importing an ungranted host fn fails to instantiate | [ ] |
| Host access is capability-gated | aep-sandbox gated_tests/effect_tests: ungranted capability omits the host fn; side effect cannot run | [ ] |
| Execution is bounded | aep-sandbox compute_tests::fuel_bound_stops_runaway: infinite loop trapped | [ ] |
| Sandbox is on the live tool path | sandbox_chain::permitted_tool_runs_side_effect_in_sandbox passes; the counter is bumped from inside WASM via host_sink | [ ] |
| No earlier-phase regression | full `cargo test -p aep-itest -- --ignored` passes | [ ] |

This closes the third Phase 1 success criterion ("WASM tools cannot access
unauthorized host resources").
```

- [ ] **Step 5: Commit**

```bash
git add crates/aep-itest/tests/sandbox_chain.rs docs/superpowers/plans/2026-05-30-agent-execution-plane-phase1b-acceptance.md
git commit -m "test(sandbox): live sandboxed side-effect path + Phase 1b acceptance"
```

---

## Self-Review

**Spec coverage:** "WASM tools cannot access unauthorized host resources" → Tasks 3–5 (no ambient authority; capability-gated host fn; gated side effect) prove it in unit tests; Tasks 6–8 put the sandbox on the live path. ✔

**Placeholder scan:** every step shows complete code; the wasmtime API (`Engine`/`Config::consume_fuel`/`Store::set_fuel`/`Linker::func_wrap`/`Module::new` with WAT/`get_typed_func`) is shown in full; verify against `wasmtime = "45"` docs if a signature differs (e.g., `set_fuel` vs `add_fuel` across versions). ✔

**Type consistency:** `SandboxError`, `run_add`, `run_with_sink`, `run_tool`, `sink_resource` defined in Tasks 2/4/5 and used consistently; `Capability`/`Action`/`Resource` come from `aep-capability`; `TOOL_WAT` in Task 7 matches the `run()->i32` + `env.host_sink` shape the sandbox expects. ✔

**Known follow-ups (not this plan):** upgrade from core-module WAT to the WASI Preview 2 component model with real guest tools; multi-resource capabilities; per-tool WASM artifacts with supply-chain verification (spec Production phase).
