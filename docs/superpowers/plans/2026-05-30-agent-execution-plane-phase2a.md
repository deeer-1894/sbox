# Agent Execution Plane — Phase 2a (Tenant Quota & Backpressure) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Admit or reject each agent run against a per-tenant concurrency limit, so a tenant cannot exceed its slot budget and over-limit requests are shed fast (backpressure) instead of queueing unboundedly.

**Architecture:** A `TenantService` Restate Virtual Object (keyed by tenant) holds a durable in-flight counter and a limit. `AgentService` acquires a slot before doing work and releases it afterward (on every path, including errors — Restate's re-drive makes the release durable). The admit decision is a pure, unit-tested function.

**Tech Stack:** Rust, the Phase 0/1 `restate-sdk = "0.8"` runtime, `aep-domain`. Builds on `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase1b.md`.

---

## Scope

Delivers the **quota/backpressure** slice of Phase 2 from `docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md`:

> `TenantService` quota/backpressure ... tenant quotas apply backpressure.

Deferred to their own plans (each independently shippable):
- **Phase 2b:** ClickHouse audit stream + causal query API.
- **Phase 2c:** OpenTelemetry traces/metrics export.

Prerequisite: Phase 1 + 1b complete and green.

## Key Design Decisions

- **Concurrency quota, not rate.** `TenantService` tracks in-flight runs; `acquire` admits while `in_flight < limit`, else rejects. This matches the spec's "max_concurrent" and gives an immediate backpressure signal.
- **Admission is the first gate.** `AgentService` calls `acquire` before policy/mint/tool — the cheapest rejection happens first.
- **Release on every path.** The inner work returns a `Result`; the slot is released before returning regardless of outcome. If the process crashes mid-handler, Restate re-drives from the journal (acquire is journaled, not re-executed) and reaches the release — durability gives us the "finally".
- **Backpressure = fast rejection.** Over-limit returns a `denied` reply immediately; nothing queues.
- **Known limitation (noted, not fixed here):** release is a bare decrement (`saturating_sub`), so a retry after a release whose ack was lost could over-release (free a slot early). A lease-id model fixes this in a later phase; `saturating_sub` prevents underflow meanwhile.

## File Structure

```
crates/
  aep-domain/      MODIFY — admit() pure fn + UserInput.tenant
    src/lib.rs
  aep-runtime/     MODIFY — TenantService object + AgentService acquire/release
    src/lib.rs
  aep-itest/       MODIFY — quota semantics + backpressure tests
    tests/quota.rs   NEW
```

---

## Task 1: Admission rule and tenant field

**Files:**
- Modify: `crates/aep-domain/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/aep-domain/src/lib.rs`:

```rust
#[cfg(test)]
mod admit_tests {
    use super::*;

    #[test]
    fn admits_below_limit_and_rejects_at_limit() {
        assert!(admit(0, 1), "first slot admitted");
        assert!(!admit(1, 1), "at limit -> rejected");
        assert!(admit(4, 5), "below limit admitted");
        assert!(!admit(5, 5), "at limit rejected");
        assert!(!admit(0, 0), "zero limit rejects everything");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aep-domain admit_tests`
Expected: FAIL — `cannot find function admit`.

- [ ] **Step 3: Write admit + add the tenant field**

Add to `crates/aep-domain/src/lib.rs` (near the other free functions):

```rust
/// Admission rule for a per-tenant concurrency quota: admit while strictly
/// below the limit.
pub fn admit(in_flight: u32, limit: u32) -> bool {
    in_flight < limit
}
```

And add a `tenant` field to `UserInput` (replace the struct):

```rust
/// A user message delivered to an AgentService. `idempotency_key` is the dedup
/// anchor: the same key must drive the same tool invocation. `requested_tool`
/// stands in for the model's chosen tool; absent means the default `echo`.
/// `tenant` selects the quota bucket; absent means `default`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserInput {
    pub idempotency_key: String,
    pub content: String,
    #[serde(default)]
    pub requested_tool: Option<String>,
    #[serde(default)]
    pub tenant: Option<String>,
}
```

- [ ] **Step 4: Fix existing UserInput constructions**

The `requested_tool_tests` and `plan_tests` modules construct `UserInput`. Add `tenant: None` to each of the three constructions in those tests in `crates/aep-domain/src/lib.rs`.

- [ ] **Step 5: Run all domain tests**

Run: `cargo test -p aep-domain`
Expected: PASS (existing + `admit_tests`). The `#[serde(default)]` keeps older JSON valid.

- [ ] **Step 6: Commit**

```bash
git add crates/aep-domain/src/lib.rs
git commit -m "feat(domain): tenant admission rule + UserInput.tenant"
```

---

## Task 2: TenantService Virtual Object

**Files:**
- Modify: `crates/aep-runtime/src/lib.rs`

- [ ] **Step 1: Add the TenantService**

In `crates/aep-runtime/src/lib.rs`, add near the other service definitions (top level, not inside the `agent` module). First import `admit`: change the `aep_domain` use line to include it:

```rust
use aep_domain::{admit, decide, AgentReply, Decision, ToolOutput, ToolRequest, UserInput};
```

Then add:

```rust
/// Default per-tenant concurrency limit when none is configured.
const DEFAULT_TENANT_LIMIT: u32 = 1000;

/// TenantService: keyed by tenant id. Durable in-flight counter + limit.
#[restate_sdk::object]
pub trait TenantService {
    async fn set_limit(max: u32) -> Result<(), HandlerError>;
    async fn acquire() -> Result<bool, HandlerError>;
    async fn release() -> Result<(), HandlerError>;
    async fn in_flight() -> Result<u32, HandlerError>;
}

pub struct TenantServiceImpl;

impl TenantService for TenantServiceImpl {
    async fn set_limit(&self, ctx: ObjectContext<'_>, max: u32) -> Result<(), HandlerError> {
        ctx.set("limit", max);
        Ok(())
    }

    async fn acquire(&self, ctx: ObjectContext<'_>) -> Result<bool, HandlerError> {
        let limit = ctx.get::<u32>("limit").await?.unwrap_or(DEFAULT_TENANT_LIMIT);
        let current = ctx.get::<u32>("current").await?.unwrap_or(0);
        if admit(current, limit) {
            ctx.set("current", current + 1);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn release(&self, ctx: ObjectContext<'_>) -> Result<(), HandlerError> {
        let current = ctx.get::<u32>("current").await?.unwrap_or(0);
        ctx.set("current", current.saturating_sub(1));
        Ok(())
    }

    async fn in_flight(&self, ctx: ObjectContext<'_>) -> Result<u32, HandlerError> {
        Ok(ctx.get::<u32>("current").await?.unwrap_or(0))
    }
}
```

- [ ] **Step 2: Bind it in main**

In `crates/aep-runtime/src/main.rs`, import `TenantService` + `TenantServiceImpl` and bind it. Change the imports:

```rust
use aep_runtime::{
    counter_router, AgentService, AgentServiceImpl, TenantService, TenantServiceImpl,
    ToolService, ToolServiceImpl,
};
```

And add a `.bind` line in the `Endpoint::builder()` chain:

```rust
            .bind(TenantServiceImpl.serve())
```

- [ ] **Step 3: Build**

Run: `cargo build -p aep-runtime`
Expected: `Finished`.

- [ ] **Step 4: Commit**

```bash
git add crates/aep-runtime/src/lib.rs crates/aep-runtime/src/main.rs
git commit -m "feat(runtime): TenantService durable concurrency quota object"
```

---

## Task 3: AgentService acquires and releases a tenant slot

**Files:**
- Modify: `crates/aep-runtime/src/lib.rs`

- [ ] **Step 1: Extract the inner handler and wrap with acquire/release**

In `crates/aep-runtime/src/lib.rs`, inside `mod agent`, replace the entire `impl AgentService for AgentServiceImpl { ... }` block with a version that admits first, runs the existing logic as `handle_inner`, then releases on every path:

```rust
    impl AgentService for AgentServiceImpl {
        async fn handle(
            &self,
            ctx: ObjectContext<'_>,
            Json(input): Json<UserInput>,
        ) -> Result<Json<AgentReply>, HandlerError> {
            let tenant = input.tenant.clone().unwrap_or_else(|| "default".to_string());

            // Admission control: the cheapest rejection happens first.
            let admitted = ctx
                .object_client::<TenantServiceClient>(tenant.clone())
                .acquire()
                .call()
                .await?;
            if !admitted {
                return Ok(Json(AgentReply {
                    output: serde_json::Value::Null,
                    exec_count: 0,
                    denied: true,
                    reason: Some("tenant quota exceeded".to_string()),
                }));
            }

            // Run the real work, then release the slot regardless of outcome.
            let outcome = handle_inner(&ctx, &input).await;
            ctx.object_client::<TenantServiceClient>(tenant)
                .release()
                .call()
                .await?;
            outcome.map(Json)
        }
    }

    /// The policy + capability + tool logic, separated so the quota slot can be
    /// released on every return path.
    async fn handle_inner(
        ctx: &ObjectContext<'_>,
        input: &UserInput,
    ) -> Result<AgentReply, HandlerError> {
        let agent_id = ctx.key().to_string();
        let req: ToolRequest = plan_user_input(input);

        if let PolicyDecision::Deny(reason) = evaluate(&agent_id, &req.tool_name) {
            return Ok(AgentReply {
                output: serde_json::Value::Null,
                exec_count: 0,
                denied: true,
                reason: Some(reason),
            });
        }

        let now: u64 = ctx.run(|| async { now_unix() }).await?;
        let cap = Capability {
            id: format!("cap-{}", req.invocation_id),
            tenant: "default".into(),
            subject: agent_id,
            resource: Resource::Tool { name: req.tool_name.clone() },
            actions: vec![Action::Call],
            expires_at: now + 300,
            policy_hash: "tools.cedar@v1".into(),
            audit_id: format!("aud-{}", req.invocation_id),
        };
        let token = sign(&cap_secret(), &cap);

        let tool_key = req.tool_name.clone();
        let Json(out) = ctx
            .object_client::<ToolServiceClient>(tool_key)
            .run(Json(ToolCall { request: req, capability_token: token }))
            .call()
            .await?;
        Ok(AgentReply {
            output: out.output,
            exec_count: out.exec_count,
            denied: false,
            reason: None,
        })
    }
```

> This moves the former body of `handle` into `handle_inner` unchanged except for taking `&ctx`/`&input`. `plan_user_input`, `evaluate`, `now_unix`, `sign`, `Capability`, `Resource`, `Action`, `ToolCall`, `ToolServiceClient`, `PolicyDecision` are all already in scope in the module.

- [ ] **Step 2: Build**

Run: `cargo build -p aep-runtime`
Expected: `Finished`. If `handle_inner` cannot see a name, add the corresponding `use super::*` item (the `agent` module already does `use super::*;`).

- [ ] **Step 3: Commit**

```bash
git add crates/aep-runtime/src/lib.rs
git commit -m "feat(runtime): AgentService admits/releases a tenant quota slot"
```

---

## Task 4: Quota integration tests

**Files:**
- Create: `crates/aep-itest/tests/quota.rs`

Run against the live stack (compose up, `cargo run -p aep-runtime`, `./scripts/register.sh`).

- [ ] **Step 1: Write the tests**

Create `crates/aep-itest/tests/quota.rs`:

```rust
//! Run against a live stack.
//!   cargo test -p aep-itest --test quota -- --ignored
const INGRESS: &str = "http://localhost:8080";

async fn post(path: &str, body: serde_json::Value) -> (reqwest::StatusCode, serde_json::Value) {
    let resp = reqwest::Client::new()
        .post(format!("{INGRESS}{path}"))
        .json(&body)
        .send().await.unwrap();
    let status = resp.status();
    let json = resp.json().await.unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn quota_admits_then_rejects_then_recovers() {
    let tenant = format!("t-{}", uuid::Uuid::new_v4());
    let base = format!("/TenantService/{tenant}");

    post(&format!("{base}/set_limit"), serde_json::json!(1)).await;

    let (_, a1) = post(&format!("{base}/acquire"), serde_json::json!(null)).await;
    assert_eq!(a1, serde_json::json!(true), "first acquire admitted");

    let (_, a2) = post(&format!("{base}/acquire"), serde_json::json!(null)).await;
    assert_eq!(a2, serde_json::json!(false), "second acquire over limit -> rejected");

    post(&format!("{base}/release"), serde_json::json!(null)).await;

    let (_, a3) = post(&format!("{base}/acquire"), serde_json::json!(null)).await;
    assert_eq!(a3, serde_json::json!(true), "acquire admitted again after release");
}

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn agent_request_is_backpressured_when_tenant_exhausted() {
    let tenant = format!("t-{}", uuid::Uuid::new_v4());
    // Limit 0 -> every admission is rejected.
    post(&format!("/TenantService/{tenant}/set_limit"), serde_json::json!(0)).await;

    let (_, r) = post(
        "/AgentService/agent-q/handle",
        serde_json::json!({ "idempotency_key": format!("q-{}", uuid::Uuid::new_v4()), "content": "hi", "tenant": tenant }),
    )
    .await;
    assert_eq!(r["denied"], serde_json::json!(true), "exhausted tenant is backpressured");
    assert_eq!(r["reason"], serde_json::json!("tenant quota exceeded"));
}

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn default_tenant_request_succeeds() {
    let (_, r) = post(
        "/AgentService/agent-q/handle",
        serde_json::json!({ "idempotency_key": format!("ok-{}", uuid::Uuid::new_v4()), "content": "hi" }),
    )
    .await;
    assert_eq!(r["denied"], serde_json::json!(false), "default tenant (limit 1000) admits");
}
```

- [ ] **Step 2: Compile**

Run: `cargo test -p aep-itest --test quota --no-run`
Expected: compiles.

- [ ] **Step 3: Restart, re-register, run**

```bash
pkill -f 'target/debug/aep-runtime'; cargo run -p aep-runtime &
./scripts/register.sh        # picks up the new TenantService
cargo test -p aep-itest --test quota -- --ignored
```
Expected: 3 passed.

- [ ] **Step 4: Full regression**

Run: `cargo test -p aep-itest -- --ignored`
Expected: all integration tests pass (effectively_once, security_chain x3, sandbox_chain, quota x3). The default-tenant path is unaffected (limit 1000).

- [ ] **Step 5: Commit**

```bash
git add crates/aep-itest/tests/quota.rs
git commit -m "test(itest): tenant quota semantics + agent backpressure"
```

---

## Task 5: Acceptance

**Files:**
- Create: `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase2a-acceptance.md`

- [ ] **Step 1: Write acceptance**

Create `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase2a-acceptance.md`:

```markdown
# Phase 2a (Tenant Quota & Backpressure) Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 2, quota slice)

| Criterion | Evidence | Status |
| --- | --- | --- |
| Per-tenant concurrency quota enforced | quota::quota_admits_then_rejects_then_recovers passes (limit 1: admit, reject, release, admit) | [ ] |
| Tenant quotas apply backpressure | quota::agent_request_is_backpressured_when_tenant_exhausted passes (limit 0 -> denied) | [ ] |
| Admission rule is pure + tested | aep-domain admit_tests pass | [ ] |
| No earlier-phase regression | full cargo test -p aep-itest -- --ignored passes | [ ] |

Deferred: Phase 2b (ClickHouse audit + causal query), Phase 2c (OpenTelemetry).

Known limitation: release is a bare decrement; a lease-id model is a later-phase
fix. saturating_sub prevents underflow.
```

- [ ] **Step 2: Tick boxes after a clean run**

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/plans/2026-05-30-agent-execution-plane-phase2a-acceptance.md
git commit -m "docs: Phase 2a acceptance checklist"
```

---

## Self-Review

**Spec coverage:** "TenantService quota/backpressure" → Task 2 (durable quota object) + Task 3 (AgentService admission/release) + Task 4 (quota semantics + backpressure tests). ✔

**Placeholder scan:** every step shows complete code; the only new external surface is the Restate object handler shape, already proven in Phase 0/1. ✔

**Type consistency:** `admit` defined in Task 1, used in Task 2. `TenantService`/`TenantServiceClient`/`TenantServiceImpl` defined in Task 2, used in Tasks 2–3. `UserInput.tenant` defined in Task 1, used in Task 3 and posted in Task 4. `handle_inner` is the former `handle` body, names already in module scope. ✔

**Deferred:** Phase 2b (ClickHouse audit + causal query API), Phase 2c (OpenTelemetry) — separate plans.
