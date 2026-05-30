# Agent Execution Plane — Phase 0 (Substrate Validation) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove on real infrastructure that an agent's tool call runs through a durable, idempotent side-effect boundary on Restate — a committed side effect is executed exactly once and is not re-executed on resend or crash recovery.

**Architecture:** All actor business logic lives in a pure, deterministic `aep-domain` crate (unit-tested without any infrastructure). A thin `aep-runtime` crate adapts that logic to two Restate Virtual Objects (`AgentService`, `ToolService`) and exposes an "external" counter endpoint that observes whether the side effect actually ran. Restate (in Docker) provides durable execution, journaling, and per-key single-turn serialization; we never re-implement those.

**Tech Stack:** Rust (stable), `restate-sdk = "0.8"`, Tokio, axum (counter sidecar), reqwest (integration tests), serde/serde_json, sha2, Docker + Docker Compose (restate-server). Target platform: macOS/Linux dev host.

---

## Scope

This plan implements **only Phase 0** of `docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md`:

> Stand up Restate; run one `AgentService` + `ToolService` with a tool call through the idempotent side-effect boundary. **Success:** kill the process and the agent recovers from the journal; a committed `ToolCompleted` side effect is not re-executed on replay.

Out of scope here (each gets its own plan): PolicyService/Cedar, CapabilityBroker, WASM sandbox, multi-tenancy/quota, ClickHouse audit, MemoryService. Do not add them.

## Prerequisites & API Verification

- Docker Desktop (or Docker Engine) running. On macOS, `host.docker.internal` resolves to the host by default; the compose file below adds the mapping explicitly for Linux.
- **One-time API check before Task 6:** open <https://docs.rs/restate-sdk/0.8> and confirm three signatures used by this plan, since the macro-generated client surface is the only volatile dependency:
  1. handler payloads use the `restate_sdk::serde::Json<T>` wrapper for serde types;
  2. the generated client is reached via `ctx.object_client::<XxxClient>(key).handler(Json(arg)).call().await?`;
  3. `ctx.run(|| async { ... }).await?` returns the closure's `T` and journals it.
  If 0.8 differs, adjust the three call sites in Tasks 6–7 accordingly; the domain crate (Tasks 1–4) is SDK-independent and unaffected.

## File Structure

```
sadbox/
  Cargo.toml                       # workspace manifest
  rust-toolchain.toml              # pin stable toolchain
  .gitignore                       # ignore target/
  deploy/
    docker-compose.yml             # restate-server (ingress 8080, admin 9070)
  scripts/
    register.sh                    # register the host deployment with Restate
    kill-recover.sh                # scripted crash-recovery verification
  crates/
    aep-domain/                    # PURE deterministic logic — unit tested, no infra
      Cargo.toml
      src/lib.rs                   # types, hashing, decide(), plan_user_input()
    aep-runtime/                   # Restate adapter + counter sidecar + main
      Cargo.toml
      src/lib.rs                   # AgentService, ToolService, counter router
      src/main.rs                  # serve Restate endpoint + counter
    aep-itest/                     # integration tests against running stack
      Cargo.toml
      tests/effectively_once.rs    # #[ignore] — run against live Restate
```

Responsibility split: `aep-domain` owns *what to do* (pure, testable in milliseconds); `aep-runtime` owns *durable wiring* (Restate state, side effects, RPC). Keeping them separate is what makes the determinism invariant from the spec enforceable and unit-testable.

---

## Task 1: Workspace scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `.gitignore`
- Create: `crates/aep-domain/Cargo.toml`
- Create: `crates/aep-domain/src/lib.rs`

- [ ] **Step 1: Create the workspace manifest**

Create `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/aep-domain", "crates/aep-runtime", "crates/aep-itest"]

[workspace.package]
edition = "2021"
version = "0.1.0"
license = "Apache-2.0"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
thiserror = "1"
tokio = { version = "1", features = ["full"] }
restate-sdk = "0.8"
axum = "0.7"
reqwest = { version = "0.12", features = ["json"] }
uuid = { version = "1", features = ["v4"] }
```

- [ ] **Step 2: Pin the toolchain**

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
```

- [ ] **Step 3: Ignore build output**

Create `.gitignore`:

```
/target
```

- [ ] **Step 4: Create the domain crate manifest**

Create `crates/aep-domain/Cargo.toml`:

```toml
[package]
name = "aep-domain"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
sha2.workspace = true
```

- [ ] **Step 5: Create a placeholder lib so the workspace builds**

Create `crates/aep-domain/src/lib.rs`:

```rust
//! Pure, deterministic agent/tool domain logic. No infrastructure dependencies.
```

The `aep-runtime` and `aep-itest` members are declared but not yet created; create empty manifests so the workspace resolves. Create `crates/aep-runtime/Cargo.toml`:

```toml
[package]
name = "aep-runtime"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
aep-domain = { path = "../aep-domain" }
restate-sdk.workspace = true
tokio.workspace = true
axum.workspace = true
reqwest.workspace = true
serde.workspace = true
serde_json.workspace = true

[[bin]]
name = "aep-runtime"
path = "src/main.rs"
```

Create `crates/aep-runtime/src/lib.rs`:

```rust
//! Restate adapter for the agent/tool domain logic.
```

Create `crates/aep-runtime/src/main.rs`:

```rust
fn main() {}
```

Create `crates/aep-itest/Cargo.toml`:

```toml
[package]
name = "aep-itest"
edition.workspace = true
version.workspace = true
license.workspace = true

[dev-dependencies]
reqwest.workspace = true
tokio.workspace = true
serde_json.workspace = true
uuid.workspace = true
```

Create `crates/aep-itest/src/lib.rs`:

```rust
//! Integration tests against a live Restate stack.
```

- [ ] **Step 6: Verify the workspace builds**

Run: `cargo build`
Expected: `Finished` with no errors (warnings about unused crates are fine).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml rust-toolchain.toml .gitignore crates/
git commit -m "chore: scaffold AEP Phase 0 workspace"
```

---

## Task 2: Domain types and input hashing

**Files:**
- Modify: `crates/aep-domain/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/aep-domain/src/lib.rs`:

```rust
#[cfg(test)]
mod hash_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hash_is_stable_and_distinguishes_inputs() {
        let a = hash_input(&json!({"content": "hello"}));
        let b = hash_input(&json!({"content": "hello"}));
        let c = hash_input(&json!({"content": "world"}));
        assert_eq!(a, b, "same input must hash identically");
        assert_ne!(a, c, "different input must hash differently");
        assert_eq!(a.len(), 64, "sha256 hex is 64 chars");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aep-domain hash_is_stable -- --nocapture`
Expected: FAIL — `cannot find function hash_input`.

- [ ] **Step 3: Write the types and hashing**

Insert above the `#[cfg(test)]` block in `crates/aep-domain/src/lib.rs`:

```rust
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A user message delivered to an AgentService. `idempotency_key` is the dedup
/// anchor: the same key must drive the same tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserInput {
    pub idempotency_key: String,
    pub content: String,
}

/// A request crossing the ToolService side-effect boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolRequest {
    pub invocation_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub input_hash: String,
}

/// The recorded result of a tool side effect. `exec_count` is the observed value
/// from the external counter at the moment the side effect ran — it lets tests
/// see whether the effect was executed or replayed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolOutput {
    pub output: serde_json::Value,
    pub exec_count: u64,
}

/// What the agent returns to the caller.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentReply {
    pub output: serde_json::Value,
    pub exec_count: u64,
}

/// SHA-256 hex of a canonical JSON value.
pub fn hash_input(value: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aep-domain hash_is_stable -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/aep-domain/src/lib.rs
git commit -m "feat(domain): tool/agent types and input hashing"
```

---

## Task 3: Side-effect decision (the heart)

This is the pure expression of the spec's replay rule: *if `ToolCompleted` exists, reuse it; otherwise execute.* Keeping it pure makes the effectively-once guarantee unit-testable.

**Files:**
- Modify: `crates/aep-domain/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/aep-domain/src/lib.rs`:

```rust
#[cfg(test)]
mod decide_tests {
    use super::*;
    use serde_json::json;

    fn output() -> ToolOutput {
        ToolOutput { output: json!({"echo": "hi"}), exec_count: 1 }
    }

    #[test]
    fn reuses_when_completion_exists() {
        let d = decide(Some(output()));
        assert_eq!(d, Decision::Reuse(output()));
    }

    #[test]
    fn executes_when_no_completion() {
        let d = decide(None);
        assert_eq!(d, Decision::Execute);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aep-domain decide_tests`
Expected: FAIL — `cannot find type Decision` / `function decide`.

- [ ] **Step 3: Write the decision logic**

Insert above the first `#[cfg(test)]` block in `crates/aep-domain/src/lib.rs`:

```rust
/// Outcome of consulting the durable journal before a tool side effect.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// A committed ToolCompleted exists; reuse it, do not re-run the side effect.
    Reuse(ToolOutput),
    /// No completion recorded; run the side effect and record it.
    Execute,
}

/// Decide whether to run a tool side effect, given any previously committed
/// completion for the same invocation id.
pub fn decide(existing_completion: Option<ToolOutput>) -> Decision {
    match existing_completion {
        Some(output) => Decision::Reuse(output),
        None => Decision::Execute,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aep-domain decide_tests`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/aep-domain/src/lib.rs
git commit -m "feat(domain): side-effect reuse/execute decision"
```

---

## Task 4: Deterministic agent planning

**Files:**
- Modify: `crates/aep-domain/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/aep-domain/src/lib.rs`:

```rust
#[cfg(test)]
mod plan_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plan_is_deterministic_and_keyed_by_idempotency() {
        let input = UserInput { idempotency_key: "k-1".into(), content: "hello".into() };
        let a = plan_user_input(&input);
        let b = plan_user_input(&input);
        assert_eq!(a, b, "planning must be deterministic");
        assert_eq!(a.invocation_id, "k-1", "invocation id anchors on idempotency key");
        assert_eq!(a.tool_name, "echo");
        assert_eq!(a.input, json!({"content": "hello"}));
        assert_eq!(a.input_hash, hash_input(&json!({"content": "hello"})));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aep-domain plan_tests`
Expected: FAIL — `cannot find function plan_user_input`.

- [ ] **Step 3: Write the planner**

Insert above the first `#[cfg(test)]` block in `crates/aep-domain/src/lib.rs`:

```rust
/// Turn a user message into a tool request. Deterministic: the invocation id is
/// the user's idempotency key, so resends collapse to one tool invocation.
/// (Phase 0 always routes to the `echo` tool.)
pub fn plan_user_input(input: &UserInput) -> ToolRequest {
    let payload = serde_json::json!({ "content": input.content });
    ToolRequest {
        invocation_id: input.idempotency_key.clone(),
        tool_name: "echo".to_string(),
        input_hash: hash_input(&payload),
        input: payload,
    }
}
```

- [ ] **Step 4: Run all domain tests**

Run: `cargo test -p aep-domain`
Expected: PASS (all tests across the four modules).

- [ ] **Step 5: Commit**

```bash
git add crates/aep-domain/src/lib.rs
git commit -m "feat(domain): deterministic user-input planning"
```

---

## Task 5: Restate server via Docker Compose

**Files:**
- Create: `deploy/docker-compose.yml`
- Create: `scripts/register.sh`

- [ ] **Step 1: Write the compose file**

Create `deploy/docker-compose.yml`:

```yaml
services:
  restate:
    # ghcr.io mirror avoids Docker Hub anonymous pull rate limits
    # (docker.io/restatedev/restate returns HTTP 429 "toomanyrequests"
    # for unauthenticated pulls).
    image: ghcr.io/restatedev/restate:latest
    ports:
      - "8080:8080"   # ingress (invoke handlers)
      - "9070:9070"   # admin (register deployments)
    extra_hosts:
      - "host.docker.internal:host-gateway"
```

- [ ] **Step 2: Write the deployment registration script**

Create `scripts/register.sh`:

```bash
#!/usr/bin/env bash
# Register the host-run aep-runtime service endpoint with Restate.
# Run AFTER `cargo run -p aep-runtime` is listening on :9080.
set -euo pipefail
curl --fail --silent --show-error \
  http://localhost:9070/deployments \
  -H 'content-type: application/json' \
  -d '{"uri":"http://host.docker.internal:9080"}' | tee /dev/stderr
echo
echo "Registered. Services:"
curl --fail --silent http://localhost:9070/services | tee /dev/stderr
echo
```

- [ ] **Step 3: Make the script executable**

Run: `chmod +x scripts/register.sh`

- [ ] **Step 4: Start Restate and verify health**

Run: `docker compose -f deploy/docker-compose.yml up -d`
Then: `curl --fail --silent http://localhost:9070/health && echo OK`
Expected: prints `OK` (admin API is up). If `curl` returns non-zero, wait a few seconds and retry — the container needs a moment to start.

- [ ] **Step 5: Commit**

```bash
git add deploy/docker-compose.yml scripts/register.sh
git commit -m "chore: restate-server compose + deployment registration script"
```

---

## Task 6: ToolService Virtual Object + counter sidecar

This wires `decide()` to Restate state and runs the side effect through `ctx.run`. The counter is the "external" observable that proves whether the effect actually executed.

**Files:**
- Modify: `crates/aep-runtime/src/lib.rs`

> Apply the API check from "Prerequisites & API Verification" before writing this task.

- [ ] **Step 1: Write the ToolService and counter**

Replace the contents of `crates/aep-runtime/src/lib.rs`:

```rust
//! Restate adapter for the agent/tool domain logic.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use aep_domain::{decide, AgentReply, Decision, ToolOutput, ToolRequest, UserInput};
use axum::{extract::State, routing::{get, post}, Json as AxumJson, Router};
use restate_sdk::prelude::*;

/// Address of the in-process "external" counter the tool side effect mutates.
const COUNTER_BASE: &str = "http://localhost:9090";

/// ToolService: keyed by tool name. State key = invocation_id -> ToolOutput.
#[restate_sdk::object]
pub trait ToolService {
    async fn run(req: Json<ToolRequest>) -> Result<Json<ToolOutput>, HandlerError>;
}

pub struct ToolServiceImpl;

impl ToolService for ToolServiceImpl {
    async fn run(
        &self,
        ctx: ObjectContext<'_>,
        Json(req): Json<ToolRequest>,
    ) -> Result<Json<ToolOutput>, HandlerError> {
        // Read any committed completion for this invocation (the durable journal).
        let existing = ctx.get::<Json<ToolOutput>>(&req.invocation_id).await?.map(|j| j.0);

        match decide(existing) {
            Decision::Reuse(output) => Ok(Json(output)),
            Decision::Execute => {
                // The side effect: POST the external counter. ctx.run journals the
                // result so a retry/replay reuses it instead of POSTing again.
                let content = req.input.clone();
                let count: u64 = ctx
                    .run(|| async move {
                        let n = reqwest::Client::new()
                            .post(format!("{COUNTER_BASE}/incr"))
                            .send()
                            .await
                            .map_err(|e| TerminalError::new(format!("counter unreachable: {e}")))?
                            .text()
                            .await
                            .map_err(|e| TerminalError::new(format!("counter body: {e}")))?
                            .trim()
                            .parse::<u64>()
                            .map_err(|e| TerminalError::new(format!("counter parse: {e}")))?;
                        Ok(n)
                    })
                    .await?;

                let output = ToolOutput {
                    output: serde_json::json!({ "echo": content }),
                    exec_count: count,
                };
                // Commit ToolCompleted: future resends of this invocation reuse it.
                ctx.set(&req.invocation_id, Json(output.clone()));
                Ok(Json(output))
            }
        }
    }
}

/// The external counter sidecar. Not part of Restate's journal — its mutation is
/// exactly what must happen once per committed side effect.
#[derive(Clone, Default)]
pub struct Counter(Arc<AtomicU64>);

pub fn counter_router() -> Router {
    let state = Counter::default();
    Router::new()
        .route("/incr", post(|State(c): State<Counter>| async move {
            (c.0.fetch_add(1, Ordering::SeqCst) + 1).to_string()
        }))
        .route("/count", get(|State(c): State<Counter>| async move {
            AxumJson(c.0.load(Ordering::SeqCst))
        }))
        .with_state(state)
}

// Re-export AgentService bits added in Task 7.
pub use agent::*;
mod agent {
    use super::*;
    use aep_domain::plan_user_input;

    /// AgentService: keyed by agent id. Plans the tool request and calls ToolService.
    #[restate_sdk::object]
    pub trait AgentService {
        async fn handle(input: Json<UserInput>) -> Result<Json<AgentReply>, HandlerError>;
    }

    pub struct AgentServiceImpl;

    impl AgentService for AgentServiceImpl {
        async fn handle(
            &self,
            ctx: ObjectContext<'_>,
            Json(input): Json<UserInput>,
        ) -> Result<Json<AgentReply>, HandlerError> {
            let req: ToolRequest = plan_user_input(&input);
            let tool_key = req.tool_name.clone();
            let Json(out) = ctx
                .object_client::<ToolServiceClient>(tool_key)
                .run(Json(req))
                .call()
                .await?;
            Ok(Json(AgentReply { output: out.output, exec_count: out.exec_count }))
        }
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p aep-runtime`
Expected: `Finished`. If the generated client method or `Json` import differs in your 0.8 build, fix the three flagged call sites (`object_client::<ToolServiceClient>`, `.run(Json(req)).call()`, `ctx.run`) per docs.rs, then rebuild.

- [ ] **Step 3: Commit**

```bash
git add crates/aep-runtime/src/lib.rs
git commit -m "feat(runtime): ToolService side-effect boundary + AgentService + counter"
```

---

## Task 7: Serve the endpoint and the counter

**Files:**
- Modify: `crates/aep-runtime/src/main.rs`

- [ ] **Step 1: Write main()**

Replace the contents of `crates/aep-runtime/src/main.rs`:

```rust
// The `serve()` method comes from the macro-generated service traits, so the
// traits (not just the *Impl structs) must be in scope here.
use aep_runtime::{
    counter_router, AgentService, AgentServiceImpl, ToolService, ToolServiceImpl,
};
use restate_sdk::prelude::*;

#[tokio::main]
async fn main() {
    // External counter sidecar on :9090.
    tokio::spawn(async {
        let listener = tokio::net::TcpListener::bind("0.0.0.0:9090").await.unwrap();
        axum::serve(listener, counter_router()).await.unwrap();
    });

    // Restate service endpoint on :9080 (registered with the server in Task 5).
    HttpServer::new(
        Endpoint::builder()
            .bind(ToolServiceImpl.serve())
            .bind(AgentServiceImpl.serve())
            .build(),
    )
    .listen_and_serve("0.0.0.0:9080".parse().unwrap())
    .await;
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p aep-runtime`
Expected: `Finished`.

- [ ] **Step 3: Run the service and register it**

In terminal A: `cargo run -p aep-runtime`
Expected: process stays running, no panic.

In terminal B (with Restate already up from Task 5): `./scripts/register.sh`
Expected: JSON listing a deployment and the two services `AgentService` and `ToolService`.

- [ ] **Step 4: Smoke-invoke through the ingress**

In terminal B:

```bash
curl --fail --silent http://localhost:8080/AgentService/agent-1/handle \
  -H 'content-type: application/json' \
  -d '{"idempotency_key":"smoke-1","content":"hello"}'
echo
curl --fail --silent http://localhost:9090/count
```

Expected: first response is JSON like `{"output":{"echo":{"content":"hello"}},"exec_count":1}`; the counter prints `1`.

- [ ] **Step 5: Commit**

```bash
git add crates/aep-runtime/src/main.rs
git commit -m "feat(runtime): serve Restate endpoint and counter sidecar"
```

---

## Task 8: Effectively-once integration test

Proves the spec's success criterion in an automated, repeatable way: a resend of the same idempotency key does **not** re-run the side effect.

**Files:**
- Create: `crates/aep-itest/tests/effectively_once.rs`

- [ ] **Step 1: Write the test**

Create `crates/aep-itest/tests/effectively_once.rs`:

```rust
//! Run against a live stack:
//!   1. docker compose -f deploy/docker-compose.yml up -d
//!   2. cargo run -p aep-runtime           (terminal A)
//!   3. ./scripts/register.sh
//!   4. cargo test -p aep-itest -- --ignored
const INGRESS: &str = "http://localhost:8080";
const COUNTER: &str = "http://localhost:9090";

async fn counter() -> u64 {
    reqwest::get(format!("{COUNTER}/count")).await.unwrap().json().await.unwrap()
}

async fn invoke_agent(idempotency_key: &str) -> serde_json::Value {
    reqwest::Client::new()
        .post(format!("{INGRESS}/AgentService/agent-1/handle"))
        .json(&serde_json::json!({ "idempotency_key": idempotency_key, "content": "hello" }))
        .send().await.unwrap()
        .error_for_status().unwrap()
        .json().await.unwrap()
}

#[tokio::test]
#[ignore = "requires a live Restate stack; see file header"]
async fn resend_does_not_re_execute_side_effect() {
    // Unique key per run so prior runs' journaled state can't interfere.
    let key = format!("itest-{}", uuid::Uuid::new_v4());

    let before = counter().await;

    // First invocation: the side effect runs exactly once.
    let r1 = invoke_agent(&key).await;
    let after_first = counter().await;
    assert_eq!(after_first, before + 1, "first call must run the side effect once");
    assert_eq!(r1["exec_count"].as_u64().unwrap(), after_first);

    // Resend with the SAME idempotency key: ToolCompleted exists -> reuse, no re-run.
    let r2 = invoke_agent(&key).await;
    let after_second = counter().await;
    assert_eq!(after_second, after_first, "resend must NOT re-run the side effect");
    assert_eq!(
        r2["exec_count"].as_u64().unwrap(),
        r1["exec_count"].as_u64().unwrap(),
        "resend must return the originally committed result",
    );
}
```

- [ ] **Step 2: Ensure the stack is running**

Confirm Restate is up (`docker compose -f deploy/docker-compose.yml ps`), `cargo run -p aep-runtime` is running, and `./scripts/register.sh` has been run.

- [ ] **Step 3: Run the integration test**

Run: `cargo test -p aep-itest -- --ignored`
Expected: PASS — `resend_does_not_re_execute_side_effect`.

- [ ] **Step 4: Verify it actually guards (mutation check)**

Temporarily break the guard to confirm the test has teeth: in `crates/aep-runtime/src/lib.rs`, comment out the `ctx.get::<Json<ToolOutput>>(...)` line and hardcode `let existing = None;`, restart `cargo run -p aep-runtime`, re-register, and re-run the test.
Expected: FAIL on `resend must NOT re-run the side effect` (counter increments twice).
Then **revert** the change, restart, re-register, and confirm the test PASSes again.

- [ ] **Step 5: Commit**

```bash
git add crates/aep-itest/tests/effectively_once.rs
git commit -m "test(itest): effectively-once side effect on resend"
```

---

## Task 9: Crash-recovery verification

Proves a committed side effect survives a hard process crash: the `ToolCompleted` lives in Restate's journal (inside restate-server, not our stateless process), so after `kill -9` + restart, resending the same invocation reuses the journaled result and does **not** re-execute the side effect.

The Phase 0 "external" counter sidecar lives *inside* the aep-runtime process, so `kill -9` resets it to 0. That is intentional and makes the proof unambiguous: if recovery works, the post-crash resend returns the original `exec_count` and never POSTs the counter, so the fresh counter stays at 0. (A production external effect would persist via its own `external_reference` instead.) This makes the check deterministic rather than timing-racy.

**Files:**
- Create: `scripts/kill-recover.sh`

- [ ] **Step 1: Write the verification script**

Create `scripts/kill-recover.sh`:

```bash
#!/usr/bin/env bash
# Deterministic crash-recovery check for the side-effect boundary.
#
# Proves: a tool side effect committed before a hard crash is recovered from
# Restate's journal (which lives in restate-server, not our process) and is NOT
# re-executed when the same invocation is resent after restart.
#
# Note on the counter: the Phase 0 "external" counter sidecar lives INSIDE the
# aep-runtime process, so `kill -9` resets it to 0 on restart. That is fine — it
# makes "no re-execution" visually obvious: if recovery worked, the post-crash
# resend reuses the journaled ToolCompleted (returns the original exec_count) and
# never POSTs the counter, so the fresh counter stays at 0. A production external
# effect (S3, an API) would instead persist via its own external_reference.
#
# Prereqs: Restate up + deployment registered; run from the repo root with the
# service started via `cargo run -p aep-runtime`.
set -euo pipefail

PID=$(pgrep -f 'target/debug/aep-runtime' | head -1)
KEY="recover-$(date +%s)"
echo "service pid=$PID  key=$KEY"

# 1. Invoke; the side effect runs once and is journaled by Restate.
R1=$(curl -s http://localhost:8080/AgentService/agent-1/handle \
  -H 'content-type: application/json' \
  -d "{\"idempotency_key\":\"$KEY\",\"content\":\"hello\"}")
EXEC1=$(echo "$R1" | grep -o '"exec_count":[0-9]*' | grep -o '[0-9]*')
echo "first invoke committed exec_count=$EXEC1"

# 2. Hard crash.
kill -9 "$PID"
sleep 1
echo "killed (kill -9)"

# 3. Restart the stateless service at the same URI (no re-register needed).
nohup cargo run -p aep-runtime >/tmp/aep-runtime.log 2>&1 &
disown
for i in $(seq 1 30); do curl -s http://localhost:9090/count >/dev/null 2>&1 && break; sleep 2; done
echo "restarted; fresh in-process counter=$(curl -s http://localhost:9090/count)"

# 4. Resend the same key; must reuse the journaled completion.
R2=$(curl -s http://localhost:8080/AgentService/agent-1/handle \
  -H 'content-type: application/json' \
  -d "{\"idempotency_key\":\"$KEY\",\"content\":\"hello\"}")
EXEC2=$(echo "$R2" | grep -o '"exec_count":[0-9]*' | grep -o '[0-9]*')
CNT=$(curl -s http://localhost:9090/count)

echo "=== RESULT ==="
if [ "$EXEC2" = "$EXEC1" ] && [ "$CNT" = "0" ]; then
  echo "PASS: resend after crash reused journaled completion (exec_count=$EXEC2) and did not re-run the side effect (fresh counter=$CNT)"
  exit 0
else
  echo "FAIL: EXEC1=$EXEC1 EXEC2=$EXEC2 counter=$CNT"
  exit 1
fi
```

- [ ] **Step 2: Make it executable**

Run: `chmod +x scripts/kill-recover.sh`

- [ ] **Step 3: Run the verification**

Run: `./scripts/kill-recover.sh`
Expected: ends with `PASS: resend after crash reused journaled completion ...`.

- [ ] **Step 4: Commit**

```bash
git add scripts/kill-recover.sh
git commit -m "test(recovery): deterministic crash-recovery verification for side effects"
```

---

## Task 10: Phase 0 acceptance checklist

**Files:**
- Create: `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase0-acceptance.md`

- [ ] **Step 1: Record the acceptance evidence**

Create `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase0-acceptance.md`:

```markdown
# Phase 0 Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 0)

| Criterion | Evidence | Status |
| --- | --- | --- |
| Agent → Tool call runs through a durable side-effect boundary on Restate | Task 7 smoke invoke returns exec_count=1 | [ ] |
| A committed side effect is NOT re-executed on resend | `cargo test -p aep-itest -- --ignored` passes; mutation check (Task 8 step 4) fails when guard removed | [ ] |
| Agent recovers after process crash from the journal | scripts/kill-recover.sh: counter advances by exactly 1 | [ ] |
| Actor logic is deterministic (no ambient now()/uuid() in domain) | `cargo test -p aep-domain` passes; aep-domain has no infra deps | [ ] |

Fill in the Status boxes after a clean run on a fresh `docker compose up`.
```

- [ ] **Step 2: Run the full suite end to end**

```bash
docker compose -f deploy/docker-compose.yml down
docker compose -f deploy/docker-compose.yml up -d
cargo test -p aep-domain
# start service, register, then:
cargo test -p aep-itest -- --ignored
```
Expected: domain tests PASS; integration test PASSES. Tick the boxes in the acceptance file.

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/plans/2026-05-30-agent-execution-plane-phase0-acceptance.md
git commit -m "docs: Phase 0 acceptance checklist"
```

---

## Self-Review

**Spec coverage (Phase 0 section):**
- "Stand up Restate" → Task 5. ✔
- "one AgentService + ToolService" → Tasks 6–7. ✔
- "tool call through the idempotent side-effect boundary" → Task 6 (`decide` + `ctx.run` + committed completion), Task 8 (automated proof). ✔
- "kill the process and the agent recovers from the journal" → Task 9. ✔
- "committed ToolCompleted not re-executed on replay" → Task 8 (resend) + Task 9 (crash). ✔
- Determinism invariant (spec "Determinism Requirement") → Tasks 2–4 pure domain crate; Task 10 acceptance row. ✔

**Out-of-scope correctly excluded:** Cedar/policy, capability mint, WASM sandbox, multi-tenancy, ClickHouse — none appear; each is deferred to its own plan. ✔

**Type consistency:** `UserInput`, `ToolRequest`, `ToolOutput`, `AgentReply`, `Decision`, `hash_input`, `decide`, `plan_user_input` are defined in Tasks 2–4 and used unchanged in Tasks 6–8. `ToolServiceClient` / `AgentServiceImpl` / `ToolServiceImpl` names are consistent between lib (Task 6) and main (Task 7). Object handler names `run` / `handle` match the ingress paths used in Tasks 7–8 (`/ToolService/.../run` via client, `/AgentService/agent-1/handle` via curl). ✔

**Placeholder scan:** no TBD/“handle errors appropriately”/“similar to above”; every code step shows complete code; the single flagged uncertainty (0.8 client-call syntax) has an explicit verification step rather than a vague placeholder. ✔

---

## Next Plans (not in this document)

Each is a separate plan producing working, testable software, written only after Phase 0 acceptance passes:

- **Phase 1 — Security chain:** PolicyService (Cedar) + CapabilityBroker + Wasmtime/WASI P2 sandbox replacing the fake echo tool.
- **Phase 2 — Multi-tenancy + audit:** TenantService quota/backpressure; ClickHouse audit + causal query API; OpenTelemetry.
- **Phase 3 — Memory + process sandbox + GA hardening:** MemoryService tiers with trust labels/sanitization; process-sandbox tier; supply-chain verification; SLO dashboards.
