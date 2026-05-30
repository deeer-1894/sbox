# Agent Execution Plane — Phase 2b (Audit Stream & Causal Query) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every agent run reconstructable as a causal chain — answer "what happened in this trace, in causal order?" and "which capability authorized this tool call?" — from a durable audit stream.

**Architecture:** A pure `aep-audit` crate defines the audit event model and the causal-query logic (ordering by `causal_parent_id`, capability lookup), unit-tested with no infrastructure. A Restate `AuditService` (keyed by `trace_id`) is the durable, idempotent sink and the query API (reachable over the ingress). `AgentService` emits the causal chain for each request. ClickHouse is the production analytics sink — its table schema is provided and the `aep-audit` model maps to it 1:1; live ClickHouse wiring is deferred (the image is not pullable in this environment).

**Tech Stack:** Rust, the Phase 0–2a `restate-sdk = "0.8"` runtime, `serde`. Builds on `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase2a.md`.

---

## Scope

Delivers the **audit + causal query** slice of Phase 2 from `docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md`:

> ClickHouse audit + causal query API ... the causal chain (user input → model → tool → memory write → response) is fully reconstructable.

The criterion is satisfied by the `aep-audit` model + the durable `AuditService` query API, verified live. ClickHouse is provided as the production sink (schema + 1:1 model mapping) but not run here — the registry image is rate-limited/unavailable. Deferred: Phase 2c (OpenTelemetry).

Prerequisite: Phase 2a complete and green.

## Key Design Decisions

- **Causal model is pure and unit-tested.** Events link by `message_id` ←
  `causal_parent_id`. `causal_chain` orders a trace from its root; `capability_for`
  answers "which capability authorized invocation X". No infra to test it.
- **Durable, idempotent sink as a Restate object.** `AuditService` is keyed by
  `trace_id`; `record` dedups by `message_id`, so re-emission on replay is a
  no-op. The query API is just its `chain` / `capability` handlers over the
  ingress — no separate database is required to satisfy the criterion.
- **Single emitter for the first cut.** `AgentService` knows the whole flow
  (policy decision, minted capability, tool result) and emits the chain in one
  place. (In production each actor emits its own events; the model and queries
  are identical — this only reduces wiring.)
- **Deterministic IDs and time.** `trace_id = idempotency_key`; each event's
  `message_id` is `"{trace}:{kind}"` (one per kind per trace); `ts` comes from
  the already-journaled `now` (via `ctx.run`). Replay re-emits identical events,
  which dedup.
- **ClickHouse is the analytics sink, not the source of truth.** The Restate
  journal + AuditService remain authoritative; ClickHouse is for high-volume
  query at scale. Schema provided; live wiring deferred.

## File Structure

```
crates/
  aep-audit/                NEW — event model + causal query (unit tested)
    Cargo.toml
    src/lib.rs              AuditEvent, EventKind, causal_chain(), capability_for()
  aep-runtime/              MODIFY — AuditService object + AgentService emission
    src/lib.rs
    src/main.rs
  aep-itest/                MODIFY — causal reconstruction test
    tests/audit.rs           NEW
deploy/
  clickhouse/
    audit_schema.sql        NEW — production ClickHouse table (documented target)
```

---

## Task 1: Audit model and causal query

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/aep-audit/Cargo.toml`, `crates/aep-audit/src/lib.rs`

- [ ] **Step 1: Add the workspace member**

In `Cargo.toml`, add `"crates/aep-audit"` to `members` (before `aep-runtime`).

- [ ] **Step 2: Create the crate manifest**

Create `crates/aep-audit/Cargo.toml`:

```toml
[package]
name = "aep-audit"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
```

- [ ] **Step 3: Write the failing test**

Create `crates/aep-audit/src/lib.rs`:

```rust
//! Causal audit event model and query logic (pure, infra-free).

use serde::{Deserialize, Serialize};

/// One audit fact in a trace. Events form a causal chain via
/// `message_id` <- `causal_parent_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub trace_id: String,
    pub message_id: String,
    pub causal_parent_id: Option<String>,
    pub actor: String,
    pub kind: String,
    #[serde(default)]
    pub capability_id: Option<String>,
    #[serde(default)]
    pub invocation_id: Option<String>,
    #[serde(default)]
    pub detail: serde_json::Value,
    pub ts: u64,
}

/// Order a trace's events causally: start at the root (no parent) and follow
/// each event to the child whose parent is its message_id.
pub fn causal_chain(events: &[AuditEvent]) -> Vec<AuditEvent> {
    let mut ordered = Vec::new();
    let mut current = events.iter().find(|e| e.causal_parent_id.is_none());
    while let Some(ev) = current {
        ordered.push(ev.clone());
        let id = &ev.message_id;
        current = events
            .iter()
            .find(|e| e.causal_parent_id.as_deref() == Some(id.as_str()));
    }
    ordered
}

/// Which capability authorized a given tool invocation in this trace.
pub fn capability_for(events: &[AuditEvent], invocation_id: &str) -> Option<String> {
    events
        .iter()
        .find(|e| e.kind == "tool_requested" && e.invocation_id.as_deref() == Some(invocation_id))
        .and_then(|e| e.capability_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(msg: &str, parent: Option<&str>, kind: &str) -> AuditEvent {
        AuditEvent {
            trace_id: "t".into(),
            message_id: msg.into(),
            causal_parent_id: parent.map(|s| s.to_string()),
            actor: "AgentService".into(),
            kind: kind.into(),
            capability_id: None,
            invocation_id: None,
            detail: serde_json::Value::Null,
            ts: 0,
        }
    }

    #[test]
    fn orders_a_trace_from_root() {
        // Deliberately scrambled input order.
        let events = vec![
            ev("t:tool_req", Some("t:capability"), "tool_requested"),
            ev("t:input", None, "input"),
            ev("t:capability", Some("t:policy"), "capability_minted"),
            ev("t:policy", Some("t:input"), "policy_permit"),
        ];
        let chain: Vec<String> = causal_chain(&events).into_iter().map(|e| e.kind).collect();
        assert_eq!(chain, vec!["input", "policy_permit", "capability_minted", "tool_requested"]);
    }

    #[test]
    fn finds_authorizing_capability() {
        let mut req = ev("t:tool_req", Some("t:capability"), "tool_requested");
        req.invocation_id = Some("inv-1".into());
        req.capability_id = Some("cap-1".into());
        let events = vec![ev("t:input", None, "input"), req];
        assert_eq!(capability_for(&events, "inv-1"), Some("cap-1".into()));
        assert_eq!(capability_for(&events, "nope"), None);
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p aep-audit`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/aep-audit
git commit -m "feat(audit): causal audit event model + chain/capability queries"
```

---

## Task 2: AuditService durable sink + query API

**Files:**
- Modify: `crates/aep-runtime/Cargo.toml`
- Modify: `crates/aep-runtime/src/lib.rs`

- [ ] **Step 1: Add the dependency**

In `crates/aep-runtime/Cargo.toml` `[dependencies]`, add:

```toml
aep-audit = { path = "../aep-audit" }
```

- [ ] **Step 2: Add the AuditService**

In `crates/aep-runtime/src/lib.rs`, add an import and the service near `TenantService`:

```rust
use aep_audit::{capability_for, causal_chain, AuditEvent};
```

```rust
/// AuditService: keyed by trace_id. Durable, idempotent audit sink + query API.
#[restate_sdk::object]
pub trait AuditService {
    async fn record(event: Json<AuditEvent>) -> Result<(), HandlerError>;
    async fn chain() -> Result<Json<Vec<AuditEvent>>, HandlerError>;
    async fn capability(invocation_id: Json<String>) -> Result<Json<Option<String>>, HandlerError>;
}

pub struct AuditServiceImpl;

impl AuditService for AuditServiceImpl {
    async fn record(&self, ctx: ObjectContext<'_>, Json(event): Json<AuditEvent>) -> Result<(), HandlerError> {
        let mut events = ctx.get::<Json<Vec<AuditEvent>>>("events").await?.map(|j| j.0).unwrap_or_default();
        // Idempotent: re-emission on replay is a no-op.
        if !events.iter().any(|e| e.message_id == event.message_id) {
            events.push(event);
            ctx.set("events", Json(events));
        }
        Ok(())
    }

    async fn chain(&self, ctx: ObjectContext<'_>) -> Result<Json<Vec<AuditEvent>>, HandlerError> {
        let events = ctx.get::<Json<Vec<AuditEvent>>>("events").await?.map(|j| j.0).unwrap_or_default();
        Ok(Json(causal_chain(&events)))
    }

    async fn capability(&self, ctx: ObjectContext<'_>, Json(invocation_id): Json<String>) -> Result<Json<Option<String>>, HandlerError> {
        let events = ctx.get::<Json<Vec<AuditEvent>>>("events").await?.map(|j| j.0).unwrap_or_default();
        Ok(Json(capability_for(&events, &invocation_id)))
    }
}
```

- [ ] **Step 3: Bind it in main**

In `crates/aep-runtime/src/main.rs`, add `AuditService, AuditServiceImpl` to the `aep_runtime::{...}` import and add `.bind(AuditServiceImpl.serve())` to the `Endpoint::builder()` chain.

- [ ] **Step 4: Build**

Run: `cargo build -p aep-runtime`
Expected: `Finished`.

- [ ] **Step 5: Commit**

```bash
git add crates/aep-runtime/Cargo.toml crates/aep-runtime/src/lib.rs crates/aep-runtime/src/main.rs
git commit -m "feat(runtime): AuditService durable causal audit sink + query"
```

---

## Task 3: AgentService emits the causal chain

**Files:**
- Modify: `crates/aep-runtime/src/lib.rs`

- [ ] **Step 1: Emit events in handle_inner**

In `crates/aep-runtime/src/lib.rs`, in `mod agent`'s `handle_inner`, emit audit events to `AuditService` keyed by the trace (the idempotency key). Add a small helper and emit at each step. Replace `handle_inner` with:

```rust
    async fn handle_inner(
        ctx: &ObjectContext<'_>,
        input: &UserInput,
    ) -> Result<AgentReply, HandlerError> {
        let agent_id = ctx.key().to_string();
        let req: ToolRequest = plan_user_input(input);
        let trace = req.invocation_id.clone();
        let now: u64 = ctx.run(|| async { now_unix() }).await?;

        // Root event.
        emit(ctx, &trace, "input", None, "AgentService", now, None, None,
             serde_json::json!({ "content": input.content })).await?;

        if let PolicyDecision::Deny(reason) = evaluate(&agent_id, &req.tool_name) {
            emit(ctx, &trace, "policy_deny", Some("input"), "AgentService", now, None, None,
                 serde_json::json!({ "reason": reason.clone() })).await?;
            return Ok(AgentReply {
                output: serde_json::Value::Null, exec_count: 0, denied: true, reason: Some(reason),
            });
        }
        emit(ctx, &trace, "policy_permit", Some("input"), "AgentService", now, None, None,
             serde_json::json!({ "tool": req.tool_name })).await?;

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
        emit(ctx, &trace, "capability_minted", Some("policy_permit"), "AgentService", now,
             Some(cap.id.clone()), None, serde_json::json!({ "resource": req.tool_name })).await?;
        let token = sign(&cap_secret(), &cap);

        emit(ctx, &trace, "tool_requested", Some("capability_minted"), "AgentService", now,
             Some(cap.id.clone()), Some(req.invocation_id.clone()),
             serde_json::json!({ "tool": req.tool_name })).await?;

        let tool_key = req.tool_name.clone();
        let invocation_id = req.invocation_id.clone();
        let Json(out) = ctx
            .object_client::<ToolServiceClient>(tool_key)
            .run(Json(ToolCall { request: req, capability_token: token }))
            .call()
            .await?;

        emit(ctx, &trace, "tool_completed", Some("tool_requested"), "ToolService", now,
             None, Some(invocation_id), serde_json::json!({ "exec_count": out.exec_count })).await?;

        Ok(AgentReply { output: out.output, exec_count: out.exec_count, denied: false, reason: None })
    }

    /// Emit one audit event to AuditService (keyed by trace). message_id is
    /// deterministic ("{trace}:{kind}"), so replay re-emits identically and dedups.
    #[allow(clippy::too_many_arguments)]
    async fn emit(
        ctx: &ObjectContext<'_>,
        trace: &str,
        kind: &str,
        parent_kind: Option<&str>,
        actor: &str,
        ts: u64,
        capability_id: Option<String>,
        invocation_id: Option<String>,
        detail: serde_json::Value,
    ) -> Result<(), HandlerError> {
        let event = AuditEvent {
            trace_id: trace.to_string(),
            message_id: format!("{trace}:{kind}"),
            causal_parent_id: parent_kind.map(|p| format!("{trace}:{p}")),
            actor: actor.to_string(),
            kind: kind.to_string(),
            capability_id,
            invocation_id,
            detail,
            ts,
        };
        // A Restate client `.call().await` resolves to Result<_, TerminalError>;
        // `?` converts it into the HandlerError this helper returns.
        ctx.object_client::<AuditServiceClient>(trace.to_string())
            .record(Json(event))
            .call()
            .await?;
        Ok(())
    }
```

> `AuditService`, `AuditServiceClient`, `AuditEvent` are in scope via the module's `use super::*;`. The `emit` helper lives in `mod agent`.

- [ ] **Step 2: Build**

Run: `cargo build -p aep-runtime`
Expected: `Finished`.

- [ ] **Step 3: Commit**

```bash
git add crates/aep-runtime/src/lib.rs
git commit -m "feat(runtime): AgentService emits the causal audit chain per request"
```

---

## Task 4: ClickHouse production schema (documented target)

**Files:**
- Create: `deploy/clickhouse/audit_schema.sql`

- [ ] **Step 1: Write the schema**

Create `deploy/clickhouse/audit_schema.sql`:

```sql
-- Production analytics sink for the audit stream. The aep-audit AuditEvent maps
-- 1:1 to these columns. Restate (journal + AuditService) remains authoritative;
-- ClickHouse is for high-volume causal/audit queries at scale.
--
-- Not run in this environment (image registry unavailable). To use:
--   clickhouse-client < deploy/clickhouse/audit_schema.sql
CREATE TABLE IF NOT EXISTS audit_events (
    trace_id         String,
    message_id       String,
    causal_parent_id Nullable(String),
    actor            LowCardinality(String),
    kind             LowCardinality(String),
    capability_id    Nullable(String),
    invocation_id    Nullable(String),
    detail           String,            -- JSON
    ts               UInt64
)
ENGINE = MergeTree
ORDER BY (trace_id, ts, message_id);

-- Example causal queries:
--   SELECT kind, capability_id, invocation_id FROM audit_events
--     WHERE trace_id = {trace:String} ORDER BY ts, message_id;
--   SELECT capability_id FROM audit_events
--     WHERE trace_id = {trace:String} AND kind = 'tool_requested'
--       AND invocation_id = {inv:String};
```

- [ ] **Step 2: Commit**

```bash
git add deploy/clickhouse/audit_schema.sql
git commit -m "docs: ClickHouse audit table schema (production analytics sink)"
```

---

## Task 5: Causal reconstruction integration test

**Files:**
- Create: `crates/aep-itest/tests/audit.rs`

Run against the live stack (compose up, `cargo run -p aep-runtime`, `./scripts/register.sh` — `force:true` picks up AuditService).

- [ ] **Step 1: Write the test**

Create `crates/aep-itest/tests/audit.rs`:

```rust
//! Run against a live stack.
//!   cargo test -p aep-itest --test audit -- --ignored
const INGRESS: &str = "http://localhost:8080";

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn causal_chain_is_reconstructable() {
    let key = format!("aud-{}", uuid::Uuid::new_v4());
    let client = reqwest::Client::new();

    // Drive one agent run.
    let r: serde_json::Value = client
        .post(format!("{INGRESS}/AgentService/agent-aud/handle"))
        .json(&serde_json::json!({ "idempotency_key": key, "content": "hello" }))
        .send().await.unwrap().error_for_status().unwrap()
        .json().await.unwrap();
    assert_eq!(r["denied"], serde_json::json!(false));

    // Reconstruct the causal chain for this trace (trace_id == idempotency_key).
    let chain: Vec<serde_json::Value> = client
        .post(format!("{INGRESS}/AuditService/{key}/chain"))
        .send().await.unwrap().error_for_status().unwrap()
        .json().await.unwrap();
    let kinds: Vec<&str> = chain.iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert_eq!(
        kinds,
        vec!["input", "policy_permit", "capability_minted", "tool_requested", "tool_completed"],
        "events reconstruct in causal order",
    );

    // "Which capability authorized this tool invocation?"
    let cap: serde_json::Value = client
        .post(format!("{INGRESS}/AuditService/{key}/capability"))
        .json(&key) // invocation_id == idempotency_key
        .send().await.unwrap().error_for_status().unwrap()
        .json().await.unwrap();
    assert_eq!(cap, serde_json::json!(format!("cap-{key}")), "capability lookup resolves");
}
```

- [ ] **Step 2: Compile**

Run: `cargo test -p aep-itest --test audit --no-run`
Expected: compiles.

- [ ] **Step 3: Restart, re-register, run**

```bash
pkill -f 'target/debug/aep-runtime'; cargo run -p aep-runtime &
./scripts/register.sh      # force:true picks up AuditService
cargo test -p aep-itest --test audit -- --ignored
```
Expected: PASS — `causal_chain_is_reconstructable`.

- [ ] **Step 4: Full regression**

Run: `cargo test -p aep-itest -- --ignored`
Expected: all integration tests pass (effectively_once, security_chain×3, sandbox_chain, quota×3, audit).

- [ ] **Step 5: Commit**

```bash
git add crates/aep-itest/tests/audit.rs
git commit -m "test(itest): causal chain reconstruction + capability lookup"
```

---

## Task 6: Acceptance

**Files:**
- Create: `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase2b-acceptance.md`

- [ ] **Step 1: Write acceptance**

Create the file:

```markdown
# Phase 2b (Audit & Causal Query) Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 2, audit slice)

| Criterion | Evidence | Status |
| --- | --- | --- |
| Causal chain reconstructable | audit::causal_chain_is_reconstructable: chain = input -> policy_permit -> capability_minted -> tool_requested -> tool_completed | [ ] |
| "Which capability authorized this side effect?" answerable | same test: AuditService/{trace}/capability returns cap-{trace} | [ ] |
| Causal query logic pure + tested | aep-audit tests (causal_chain ordering, capability_for) | [ ] |
| Audit sink is durable + idempotent | AuditService keyed by trace, record dedups by message_id (replay-safe) | [ ] |
| No earlier-phase regression | full cargo test -p aep-itest -- --ignored passes | [ ] |

ClickHouse: production analytics sink; schema in deploy/clickhouse/audit_schema.sql;
live wiring deferred (registry image unavailable in this environment). Deferred:
Phase 2c (OpenTelemetry).
```

- [ ] **Step 2: Tick boxes after a clean run**

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/plans/2026-05-30-agent-execution-plane-phase2b-acceptance.md
git commit -m "docs: Phase 2b acceptance checklist"
```

---

## Self-Review

**Spec coverage:** "causal chain reconstructable" → Task 1 (`causal_chain`) + Task 3 (emission) + Task 5 (live reconstruction). "which capability authorized this side effect" → `capability_for` + AuditService.capability + Task 5. ClickHouse audit → Task 4 schema + 1:1 model mapping (live deferred, documented). ✔

**Placeholder scan:** every step shows complete code; the only external surface is the Restate handler shape, proven since Phase 0. ✔

**Type consistency:** `AuditEvent`/`causal_chain`/`capability_for` defined in Task 1, used in Tasks 2–3. `AuditService`/`AuditServiceClient`/`AuditServiceImpl` defined in Task 2, used in Tasks 2–3 and called over the ingress in Task 5. `emit` helper signature matches its call sites. `trace_id == idempotency_key == invocation_id` is consistent across Tasks 3 and 5. ✔

**Deferred:** Phase 2c (OpenTelemetry). Production: live ClickHouse sink behind the same model; per-actor emission (ToolService emits its own tool_completed); memory-write audit events once MemoryService exists (Phase 3).
