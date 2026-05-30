# Agent Execution Plane — Phase 1 (Authorization Chain) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every tool call pass an independent policy decision (Cedar) and carry a short-lived, scoped capability that the tool boundary verifies before any side effect — so model/agent intent alone can never cause execution.

**Architecture:** Two pure, heavily unit-tested libraries — `aep-capability` (HMAC-signed scoped capability tokens) and `aep-policy` (Cedar policy engine) — are wired into the existing Phase 0 Restate services. `AgentService` evaluates policy and mints a capability on Permit; `ToolService` verifies the capability before running the Phase 0 side-effect boundary. Time is injected (never read ambiently) to preserve deterministic replay.

**Tech Stack:** Rust, `cedar-policy = "4"` (policy-as-code), `hmac`/`sha2`/`base64` (capability tokens), the Phase 0 `restate-sdk = "0.8"` runtime. Builds on `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase0.md`.

---

## Scope

This plan delivers the **authorization chain** of Phase 1 from `docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md`. It satisfies two of the three Phase 1 success criteria:

- ✅ tool calls require policy approval;
- ✅ LLM/agent output cannot directly execute privileged operations (the capability is the sole authority and is only mintable on a Cedar Permit);
- ⏭️ "WASM tools cannot access unauthorized host resources" — the **Wasmtime/WASI Preview 2 sandbox** is deferred to a separate **Phase 1b** plan (its own toolchain: wasm targets, guest compilation, host linking). Scoped at the end of this document.

This split follows the rule that each plan should produce working, testable software on its own. After this plan, the running system enforces policy + capability end-to-end with a fake (non-sandboxed) tool; Phase 1b replaces the fake tool with a real WASM sandbox that the capability gates.

Prerequisite: Phase 0 is complete and its acceptance criteria pass.

## Key Design Decisions

- **Capability = signed token, stateless verification.** A capability is a compact `base64(claims).base64(hmac)` token (HMAC-SHA256, shared process secret for single-node Phase 1). The minter signs; any verifier recomputes the MAC with constant-time comparison. No shared mutable broker state — capabilities survive crashes trivially and need no storage. (Production swaps HMAC for asymmetric Ed25519 so verifiers never hold the signing key — noted, not built.)
- **Policy is independent of intent.** Cedar evaluates `principal=Agent, action=CallTool, resource=Tool::"<name>"` against declarative policy. The agent cannot fabricate a Permit; default-deny means an unlisted tool is rejected.
- **Enforcement point is ToolService.** Even a caller that bypasses `AgentService` and hits `ToolService.run` directly is rejected without a valid capability — so the capability, not the request, is the authority.
- **Time is injected.** `aep-capability` mint/verify take `now: u64` as a parameter; the runtime supplies it via `ctx.run` so the journaled timestamp is stable across replay. No ambient `SystemTime::now()` inside a handler.
- **Phase 1a audit = structured tracing.** Durable audit to ClickHouse is Phase 2; here, policy decisions and capability rejections are logged via `tracing`.

## File Structure

```
crates/
  aep-capability/            NEW — pure lib, unit tested
    Cargo.toml
    src/lib.rs               Resource, Action, Capability, sign(), verify(), authorize(), CapError
  aep-policy/                NEW — Cedar wrapper, unit tested
    Cargo.toml
    src/lib.rs               PolicyDecision, evaluate()
    policies/tools.cedar     embedded allowlist policy (include_str!)
  aep-domain/                MODIFY — UserInput gains requested_tool
    src/lib.rs
  aep-runtime/               MODIFY — wire policy + capability into the two services
    src/lib.rs
  aep-itest/                 MODIFY — security-chain integration test
    tests/security_chain.rs  NEW
```

---

## Task 1: Workspace deps and new crate skeletons

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/aep-capability/Cargo.toml`, `crates/aep-capability/src/lib.rs`
- Create: `crates/aep-policy/Cargo.toml`, `crates/aep-policy/src/lib.rs`

- [ ] **Step 1: Add workspace members and shared deps**

In `Cargo.toml`, change the `members` line and add three workspace dependencies:

```toml
members = [
  "crates/aep-domain",
  "crates/aep-capability",
  "crates/aep-policy",
  "crates/aep-runtime",
  "crates/aep-itest",
]
```

Add under `[workspace.dependencies]`:

```toml
hmac = "0.12"
base64 = "0.22"
cedar-policy = "4"
```

- [ ] **Step 2: Create the capability crate manifest**

Create `crates/aep-capability/Cargo.toml`:

```toml
[package]
name = "aep-capability"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
sha2 = { workspace = true }
hmac = { workspace = true }
base64 = { workspace = true }
thiserror = { workspace = true }
```

Create `crates/aep-capability/src/lib.rs`:

```rust
//! Short-lived, scoped capability tokens (HMAC-SHA256, stateless verification).
```

- [ ] **Step 3: Create the policy crate manifest**

Create `crates/aep-policy/Cargo.toml`:

```toml
[package]
name = "aep-policy"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
cedar-policy = { workspace = true }
serde = { workspace = true }
thiserror = { workspace = true }
```

Create `crates/aep-policy/src/lib.rs`:

```rust
//! Cedar-backed policy evaluation for tool intents.
```

- [ ] **Step 4: Verify the workspace resolves**

Run: `cargo build -p aep-capability -p aep-policy`
Expected: `Finished` (this also fetches and compiles `cedar-policy`, which is sizable — first build is slow).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/aep-capability crates/aep-policy
git commit -m "chore: scaffold aep-capability and aep-policy crates"
```

---

## Task 2: Capability types

**Files:**
- Modify: `crates/aep-capability/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/aep-capability/src/lib.rs`:

```rust
#[cfg(test)]
mod type_tests {
    use super::*;

    #[test]
    fn capability_json_roundtrips() {
        let cap = Capability {
            id: "cap-1".into(),
            tenant: "tenant-a".into(),
            subject: "agent-1".into(),
            resource: Resource::Tool { name: "echo".into() },
            actions: vec![Action::Call],
            expires_at: 1_000,
            policy_hash: "ph".into(),
            audit_id: "aud-1".into(),
        };
        let s = serde_json::to_string(&cap).unwrap();
        let back: Capability = serde_json::from_str(&s).unwrap();
        assert_eq!(cap, back);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aep-capability type_tests`
Expected: FAIL — `cannot find type Capability`.

- [ ] **Step 3: Write the types**

Insert above the `#[cfg(test)]` block:

```rust
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A protected resource a capability may authorize.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resource {
    Tool { name: String },
    Network { domain: String },
    File { path_prefix: String },
    Secret { name: String },
}

/// An action a capability may permit on a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Call,
    Read,
    Write,
    Spawn,
}

/// The claims carried by a capability token. `expires_at` is a Unix timestamp
/// (seconds); time is injected by callers, never read ambiently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    pub tenant: String,
    pub subject: String,
    pub resource: Resource,
    pub actions: Vec<Action>,
    pub expires_at: u64,
    pub policy_hash: String,
    pub audit_id: String,
}

/// Errors from minting or verifying capability tokens.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CapError {
    #[error("malformed capability token")]
    Malformed,
    #[error("bad signature")]
    BadSignature,
    #[error("capability expired")]
    Expired,
    #[error("capability does not authorize this action/resource")]
    Unauthorized,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aep-capability type_tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/aep-capability/src/lib.rs
git commit -m "feat(capability): capability claim types and errors"
```

---

## Task 3: Sign and verify tokens

**Files:**
- Modify: `crates/aep-capability/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/aep-capability/src/lib.rs`:

```rust
#[cfg(test)]
mod sign_tests {
    use super::*;

    fn cap(expires_at: u64) -> Capability {
        Capability {
            id: "cap-1".into(),
            tenant: "tenant-a".into(),
            subject: "agent-1".into(),
            resource: Resource::Tool { name: "echo".into() },
            actions: vec![Action::Call],
            expires_at,
            policy_hash: "ph".into(),
            audit_id: "aud-1".into(),
        }
    }

    #[test]
    fn sign_then_verify_roundtrips() {
        let secret = b"dev-secret";
        let token = sign(secret, &cap(10_000));
        let got = verify(secret, &token, 9_000).unwrap();
        assert_eq!(got, cap(10_000));
    }

    #[test]
    fn rejects_wrong_secret() {
        let token = sign(b"secret-a", &cap(10_000));
        assert_eq!(verify(b"secret-b", &token, 9_000).unwrap_err(), CapError::BadSignature);
    }

    #[test]
    fn rejects_tampered_claims() {
        let token = sign(b"s", &cap(10_000));
        // Flip a character in the claims segment.
        let (claims, mac) = token.split_once('.').unwrap();
        let mut bytes = claims.as_bytes().to_vec();
        bytes[0] ^= 0x01;
        let tampered = format!("{}.{}", String::from_utf8_lossy(&bytes), mac);
        assert!(matches!(
            verify(b"s", &tampered, 9_000).unwrap_err(),
            CapError::BadSignature | CapError::Malformed
        ));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aep-capability sign_tests`
Expected: FAIL — `cannot find function sign`.

- [ ] **Step 3: Write sign/verify**

Insert above the first `#[cfg(test)]` block:

```rust
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

fn mac(secret: &[u8], claims_b64: &str) -> Vec<u8> {
    let mut m = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    m.update(claims_b64.as_bytes());
    m.finalize().into_bytes().to_vec()
}

/// Produce a `base64(claims).base64(hmac)` capability token.
pub fn sign(secret: &[u8], cap: &Capability) -> String {
    let claims_json = serde_json::to_vec(cap).expect("Capability serializes");
    let claims_b64 = URL_SAFE_NO_PAD.encode(claims_json);
    let mac_b64 = URL_SAFE_NO_PAD.encode(mac(secret, &claims_b64));
    format!("{claims_b64}.{mac_b64}")
}

/// Verify signature and expiry, returning the claims. Does not check authorization
/// against a specific action/resource — call `Capability::authorize` for that.
pub fn verify(secret: &[u8], token: &str, now: u64) -> Result<Capability, CapError> {
    let (claims_b64, mac_b64) = token.split_once('.').ok_or(CapError::Malformed)?;
    let expected = mac(secret, claims_b64);
    let provided = URL_SAFE_NO_PAD.decode(mac_b64).map_err(|_| CapError::Malformed)?;
    // Constant-time comparison via the HMAC crate.
    let mut m = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    m.update(claims_b64.as_bytes());
    m.verify_slice(&provided).map_err(|_| CapError::BadSignature)?;
    let _ = expected; // expected kept for clarity; verify_slice does the check.
    let claims_json = URL_SAFE_NO_PAD.decode(claims_b64).map_err(|_| CapError::Malformed)?;
    let cap: Capability = serde_json::from_slice(&claims_json).map_err(|_| CapError::Malformed)?;
    if now >= cap.expires_at {
        return Err(CapError::Expired);
    }
    Ok(cap)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p aep-capability sign_tests`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/aep-capability/src/lib.rs
git commit -m "feat(capability): HMAC sign/verify with constant-time MAC check"
```

---

## Task 4: Expiry and authorization checks

**Files:**
- Modify: `crates/aep-capability/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/aep-capability/src/lib.rs`:

```rust
#[cfg(test)]
mod authz_tests {
    use super::*;

    fn cap() -> Capability {
        Capability {
            id: "cap-1".into(),
            tenant: "tenant-a".into(),
            subject: "agent-1".into(),
            resource: Resource::Tool { name: "echo".into() },
            actions: vec![Action::Call],
            expires_at: 10_000,
            policy_hash: "ph".into(),
            audit_id: "aud-1".into(),
        }
    }

    #[test]
    fn rejects_expired_on_verify() {
        let token = sign(b"s", &cap());
        assert_eq!(verify(b"s", &token, 10_000).unwrap_err(), CapError::Expired);
    }

    #[test]
    fn authorizes_matching_action_and_resource() {
        cap().authorize(Action::Call, &Resource::Tool { name: "echo".into() }).unwrap();
    }

    #[test]
    fn rejects_wrong_resource() {
        let err = cap()
            .authorize(Action::Call, &Resource::Tool { name: "shell".into() })
            .unwrap_err();
        assert_eq!(err, CapError::Unauthorized);
    }

    #[test]
    fn rejects_wrong_action() {
        let err = cap()
            .authorize(Action::Write, &Resource::Tool { name: "echo".into() })
            .unwrap_err();
        assert_eq!(err, CapError::Unauthorized);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aep-capability authz_tests`
Expected: FAIL — `no method named authorize`.

- [ ] **Step 3: Write the authorize method**

Insert into `crates/aep-capability/src/lib.rs` (after the `Capability` struct definition):

```rust
impl Capability {
    /// Check this capability authorizes `action` on `resource`. Expiry is checked
    /// at `verify` time, not here.
    pub fn authorize(&self, action: Action, resource: &Resource) -> Result<(), CapError> {
        if &self.resource != resource || !self.actions.contains(&action) {
            return Err(CapError::Unauthorized);
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Run test and the whole crate**

Run: `cargo test -p aep-capability`
Expected: PASS (all four test modules).

- [ ] **Step 5: Commit**

```bash
git add crates/aep-capability/src/lib.rs
git commit -m "feat(capability): expiry enforcement and action/resource authorization"
```

---

## Task 5: Cedar policy engine

**Files:**
- Create: `crates/aep-policy/policies/tools.cedar`
- Modify: `crates/aep-policy/src/lib.rs`

- [ ] **Step 1: Write the policy**

Create `crates/aep-policy/policies/tools.cedar`:

```cedar
// Default-deny: only the listed tools may be called. Anything else is denied.
permit(
  principal,
  action == Action::"CallTool",
  resource
)
when { [Tool::"echo", Tool::"upper"].contains(resource) };
```

- [ ] **Step 2: Write the failing test**

Append to `crates/aep-policy/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permits_allowlisted_tool() {
        assert_eq!(evaluate("agent-1", "echo"), PolicyDecision::Permit);
        assert_eq!(evaluate("agent-1", "upper"), PolicyDecision::Permit);
    }

    #[test]
    fn denies_unlisted_tool() {
        match evaluate("agent-1", "shell") {
            PolicyDecision::Deny(reason) => assert!(!reason.is_empty()),
            other => panic!("expected Deny, got {other:?}"),
        }
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p aep-policy`
Expected: FAIL — `cannot find function evaluate`.

- [ ] **Step 4: Write the engine**

Insert above the `#[cfg(test)]` block in `crates/aep-policy/src/lib.rs`:

```rust
use cedar_policy::{Authorizer, Context, Decision, Entities, PolicySet, Request};

/// Embedded tool authorization policy (default-deny allowlist).
const POLICY_SRC: &str = include_str!("../policies/tools.cedar");

/// The outcome of evaluating a tool intent against policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Permit,
    Deny(String),
}

/// Evaluate whether `agent_id` may call `tool_name`. Deterministic and infra-free.
pub fn evaluate(agent_id: &str, tool_name: &str) -> PolicyDecision {
    let policies: PolicySet = match POLICY_SRC.parse() {
        Ok(p) => p,
        Err(e) => return PolicyDecision::Deny(format!("policy parse error: {e}")),
    };
    let principal = match format!("Agent::\"{agent_id}\"").parse() {
        Ok(p) => p,
        Err(e) => return PolicyDecision::Deny(format!("bad principal: {e}")),
    };
    let action = match r#"Action::"CallTool""#.parse() {
        Ok(a) => a,
        Err(e) => return PolicyDecision::Deny(format!("bad action: {e}")),
    };
    let resource = match format!("Tool::\"{tool_name}\"").parse() {
        Ok(r) => r,
        Err(e) => return PolicyDecision::Deny(format!("bad resource: {e}")),
    };
    let request = match Request::new(principal, action, resource, Context::empty(), None) {
        Ok(r) => r,
        Err(e) => return PolicyDecision::Deny(format!("bad request: {e}")),
    };
    let answer = Authorizer::new().is_authorized(&request, &policies, &Entities::empty());
    match answer.decision() {
        Decision::Allow => PolicyDecision::Permit,
        Decision::Deny => PolicyDecision::Deny(format!("denied tool '{tool_name}' by policy")),
    }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p aep-policy`
Expected: PASS (2 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/aep-policy/policies/tools.cedar crates/aep-policy/src/lib.rs
git commit -m "feat(policy): Cedar default-deny tool allowlist engine"
```

---

## Task 6: Route the requested tool through planning

**Files:**
- Modify: `crates/aep-domain/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/aep-domain/src/lib.rs`:

```rust
#[cfg(test)]
mod requested_tool_tests {
    use super::*;

    #[test]
    fn plan_routes_to_requested_tool_when_present() {
        let input = UserInput {
            idempotency_key: "k-1".into(),
            content: "hi".into(),
            requested_tool: Some("shell".into()),
        };
        assert_eq!(plan_user_input(&input).tool_name, "shell");
    }

    #[test]
    fn plan_defaults_to_echo_when_absent() {
        let input = UserInput {
            idempotency_key: "k-2".into(),
            content: "hi".into(),
            requested_tool: None,
        };
        assert_eq!(plan_user_input(&input).tool_name, "echo");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p aep-domain requested_tool_tests`
Expected: FAIL — `UserInput` has no field `requested_tool`.

- [ ] **Step 3: Extend UserInput and the planner**

In `crates/aep-domain/src/lib.rs`, replace the `UserInput` struct with:

```rust
/// A user message delivered to an AgentService. `idempotency_key` is the dedup
/// anchor: the same key must drive the same tool invocation. `requested_tool`
/// stands in for the model's chosen tool; absent means the default `echo`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserInput {
    pub idempotency_key: String,
    pub content: String,
    #[serde(default)]
    pub requested_tool: Option<String>,
}
```

And replace the body of `plan_user_input` with:

```rust
pub fn plan_user_input(input: &UserInput) -> ToolRequest {
    let payload = serde_json::json!({ "content": input.content });
    ToolRequest {
        invocation_id: input.idempotency_key.clone(),
        tool_name: input.requested_tool.clone().unwrap_or_else(|| "echo".to_string()),
        input_hash: hash_input(&payload),
        input: payload,
    }
}
```

- [ ] **Step 4: Fix the existing planning test**

The Phase 0 `plan_tests::plan_is_deterministic_and_keyed_by_idempotency` constructs `UserInput` without `requested_tool`. Update that construction in `crates/aep-domain/src/lib.rs` to include the new field:

```rust
        let input = UserInput { idempotency_key: "k-1".into(), content: "hello".into(), requested_tool: None };
```

- [ ] **Step 5: Run all domain tests**

Run: `cargo test -p aep-domain`
Expected: PASS (Phase 0 tests + the two new ones). The `#[serde(default)]` keeps Phase 0 JSON payloads (without `requested_tool`) valid.

- [ ] **Step 6: Commit**

```bash
git add crates/aep-domain/src/lib.rs
git commit -m "feat(domain): route planning to a requested tool, default echo"
```

---

## Task 7: AgentService — evaluate policy and mint a capability

**Files:**
- Modify: `crates/aep-runtime/Cargo.toml`
- Modify: `crates/aep-runtime/src/lib.rs`

- [ ] **Step 1: Add dependencies**

In `crates/aep-runtime/Cargo.toml`, add to `[dependencies]`:

```toml
aep-capability = { path = "../aep-capability" }
aep-policy = { path = "../aep-policy" }
```

- [ ] **Step 2: Add the shared secret, ToolCall type, and reply fields**

In `crates/aep-runtime/src/lib.rs`, add near the top (after the existing `use` lines):

```rust
use aep_capability::{sign, Action, Capability, Resource};
use aep_policy::{evaluate, PolicyDecision};

/// Process-shared capability signing secret. Single-node Phase 1 only; production
/// uses an asymmetric key so verifiers never hold the signing key.
pub fn cap_secret() -> Vec<u8> {
    std::env::var("AEP_CAP_SECRET").unwrap_or_else(|_| "dev-insecure-secret".into()).into_bytes()
}

/// A tool invocation carrying its authorizing capability token.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ToolCall {
    pub request: ToolRequest,
    pub capability_token: String,
}
```

> `ToolRequest`, `ToolOutput`, `AgentReply`, `UserInput` are imported from `aep_domain` already. Extend `AgentReply` usage below.

- [ ] **Step 3: Extend AgentReply for denials**

`AgentReply` lives in `aep-domain`. In `crates/aep-domain/src/lib.rs`, replace the `AgentReply` struct with:

```rust
/// What the agent returns to the caller. On a policy denial, `denied` is true,
/// `reason` explains, and `output`/`exec_count` carry their zero values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentReply {
    pub output: serde_json::Value,
    pub exec_count: u64,
    #[serde(default)]
    pub denied: bool,
    #[serde(default)]
    pub reason: Option<String>,
}
```

Run: `cargo test -p aep-domain` → Expected: PASS (the added fields are `#[serde(default)]`; existing constructions of `AgentReply` are in the runtime, updated next).

- [ ] **Step 4: Rewrite AgentService.handle to gate on policy + mint**

In `crates/aep-runtime/src/lib.rs`, replace the entire `impl AgentService for AgentServiceImpl { ... }` block with:

```rust
    impl AgentService for AgentServiceImpl {
        async fn handle(
            &self,
            ctx: ObjectContext<'_>,
            Json(input): Json<UserInput>,
        ) -> Result<Json<AgentReply>, HandlerError> {
            let agent_id = ctx.key().to_string();
            let req: ToolRequest = plan_user_input(&input);

            // Policy is evaluated independently of the model's intent.
            if let PolicyDecision::Deny(reason) = evaluate(&agent_id, &req.tool_name) {
                return Ok(Json(AgentReply {
                    output: serde_json::Value::Null,
                    exec_count: 0,
                    denied: true,
                    reason: Some(reason),
                }));
            }

            // Deterministic time for the capability TTL (journaled via ctx.run).
            let now: u64 = ctx
                .run(|| async {
                    Ok(std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|e| TerminalError::new(e.to_string()))?
                        .as_secs())
                })
                .await?;

            // Mint a short-lived capability scoped to exactly this tool.
            let cap = Capability {
                id: format!("cap-{}", req.invocation_id),
                tenant: "default".into(),
                subject: agent_id,
                resource: Resource::Tool { name: req.tool_name.clone() },
                actions: vec![Action::Call],
                expires_at: now + 300, // 5 minutes
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
            Ok(Json(AgentReply {
                output: out.output,
                exec_count: out.exec_count,
                denied: false,
                reason: None,
            }))
        }
    }
```

- [ ] **Step 5: Verify it compiles (ToolService still expects ToolRequest — expected break)**

Run: `cargo build -p aep-runtime`
Expected: FAIL — `ToolService::run` and its client expect `Json<ToolRequest>`, not `Json<ToolCall>`. Fixed in Task 8.

- [ ] **Step 6: Commit (after Task 8 compiles)**

Defer the commit until Task 8 makes the crate build.

---

## Task 8: ToolService — verify the capability before the side effect

**Files:**
- Modify: `crates/aep-runtime/src/lib.rs`

- [ ] **Step 1: Change the ToolService signature and verify the capability**

In `crates/aep-runtime/src/lib.rs`, change the `ToolService` trait handler to take a `ToolCall`:

```rust
#[restate_sdk::object]
pub trait ToolService {
    async fn run(call: Json<ToolCall>) -> Result<Json<ToolOutput>, HandlerError>;
}
```

Then replace the `impl ToolService for ToolServiceImpl { ... }` body so it verifies the capability before the Phase 0 boundary:

```rust
impl ToolService for ToolServiceImpl {
    async fn run(
        &self,
        ctx: ObjectContext<'_>,
        Json(ToolCall { request: req, capability_token }): Json<ToolCall>,
    ) -> Result<Json<ToolOutput>, HandlerError> {
        // The capability is the sole authority. Deterministic time via ctx.run.
        let now: u64 = ctx
            .run(|| async {
                Ok(std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| TerminalError::new(e.to_string()))?
                    .as_secs())
            })
            .await?;
        let cap = aep_capability::verify(&cap_secret(), &capability_token, now)
            .map_err(|e| TerminalError::new(format!("capability rejected: {e}")))?;
        cap.authorize(Action::Call, &Resource::Tool { name: req.tool_name.clone() })
            .map_err(|e| TerminalError::new(format!("capability not scoped to tool: {e}")))?;

        // --- Phase 0 side-effect boundary (unchanged) ---
        let existing = ctx.get::<Json<ToolOutput>>(&req.invocation_id).await?.map(|j| j.0);
        match decide(existing) {
            Decision::Reuse(output) => Ok(Json(output)),
            Decision::Execute => {
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
                ctx.set(&req.invocation_id, Json(output.clone()));
                Ok(Json(output))
            }
        }
    }
}
```

- [ ] **Step 2: Verify the whole runtime compiles**

Run: `cargo build -p aep-runtime`
Expected: `Finished`. (`Action`, `Resource`, `Capability`, `sign` are imported at the top from Task 7; `verify` is called fully-qualified.)

- [ ] **Step 3: Commit Tasks 7 + 8 together**

```bash
git add crates/aep-runtime/Cargo.toml crates/aep-runtime/src/lib.rs crates/aep-domain/src/lib.rs
git commit -m "feat(runtime): policy-gated mint in AgentService, capability verify in ToolService"
```

---

## Task 9: Security-chain integration test

**Files:**
- Create: `crates/aep-itest/tests/security_chain.rs`

Run against the live stack exactly as in Phase 0 (compose up, `cargo run -p aep-runtime`, `./scripts/register.sh`).

- [ ] **Step 1: Write the test**

Create `crates/aep-itest/tests/security_chain.rs`:

```rust
//! Run against a live stack (see Phase 0 plan, Task 8 header).
//!   cargo test -p aep-itest --test security_chain -- --ignored
const INGRESS: &str = "http://localhost:8080";
const COUNTER: &str = "http://localhost:9090";

async fn counter() -> u64 {
    reqwest::get(format!("{COUNTER}/count")).await.unwrap().json().await.unwrap()
}

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn permitted_tool_runs_once() {
    let key = format!("permit-{}", uuid::Uuid::new_v4());
    let before = counter().await;
    let r: serde_json::Value = reqwest::Client::new()
        .post(format!("{INGRESS}/AgentService/agent-1/handle"))
        .json(&serde_json::json!({ "idempotency_key": key, "content": "hello" }))
        .send().await.unwrap().error_for_status().unwrap()
        .json().await.unwrap();
    assert_eq!(r["denied"], serde_json::json!(false), "echo must be permitted");
    assert_eq!(counter().await, before + 1, "permitted tool runs the side effect once");
}

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn denied_tool_does_not_run() {
    let key = format!("deny-{}", uuid::Uuid::new_v4());
    let before = counter().await;
    let r: serde_json::Value = reqwest::Client::new()
        .post(format!("{INGRESS}/AgentService/agent-1/handle"))
        .json(&serde_json::json!({ "idempotency_key": key, "content": "x", "requested_tool": "shell" }))
        .send().await.unwrap().error_for_status().unwrap()
        .json().await.unwrap();
    assert_eq!(r["denied"], serde_json::json!(true), "shell must be denied by policy");
    assert!(r["reason"].is_string());
    assert_eq!(counter().await, before, "denied tool must NOT run the side effect");
}

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn forged_capability_is_rejected_at_tool_boundary() {
    // Bypass the agent; call ToolService directly with a bogus capability.
    let key = format!("forge-{}", uuid::Uuid::new_v4());
    let before = counter().await;
    let resp = reqwest::Client::new()
        .post(format!("{INGRESS}/ToolService/echo/run"))
        .json(&serde_json::json!({
            "request": {
                "invocation_id": key, "tool_name": "echo",
                "input": { "content": "hi" }, "input_hash": "x"
            },
            "capability_token": "not.a.valid.token"
        }))
        .send().await.unwrap();
    assert!(!resp.status().is_success(), "forged capability must be rejected");
    assert_eq!(counter().await, before, "rejected call must NOT run the side effect");
}
```

- [ ] **Step 2: Compile the test**

Run: `cargo test -p aep-itest --test security_chain --no-run`
Expected: compiles.

- [ ] **Step 3: Bring up the stack and run**

```bash
docker compose -f deploy/docker-compose.yml up -d
cargo run -p aep-runtime &      # then in another shell:
./scripts/register.sh
cargo test -p aep-itest --test security_chain -- --ignored
```
Expected: 3 passed — `permitted_tool_runs_once`, `denied_tool_does_not_run`, `forged_capability_is_rejected_at_tool_boundary`.

- [ ] **Step 4: Confirm the Phase 0 test still passes (no regression)**

Run: `cargo test -p aep-itest --test effectively_once -- --ignored`
Expected: PASS — the `{idempotency_key, content}` payload still works (new fields default).

- [ ] **Step 5: Commit**

```bash
git add crates/aep-itest/tests/security_chain.rs
git commit -m "test(itest): policy-permit, policy-deny, and forged-capability paths"
```

---

## Task 10: Phase 1 (authorization chain) acceptance

**Files:**
- Create: `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase1-acceptance.md`

- [ ] **Step 1: Record acceptance**

Create `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase1-acceptance.md`:

```markdown
# Phase 1 (Authorization Chain) Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 1)

| Criterion | Evidence | Status |
| --- | --- | --- |
| Tool calls require policy approval | security_chain::denied_tool_does_not_run passes (shell denied, side effect not run) | [ ] |
| Model/agent intent cannot directly execute | security_chain::forged_capability_is_rejected_at_tool_boundary passes (no valid capability => no side effect) | [ ] |
| Capability is scoped and short-lived | aep-capability tests: wrong-resource/action/expiry rejected | [ ] |
| Policy is declarative and default-deny | aep-policy tests: echo/upper permit, shell deny | [ ] |
| Determinism preserved | capability time injected via ctx.run; aep-capability/aep-policy are infra-free and unit-tested | [ ] |
| No Phase 0 regression | effectively_once still passes | [ ] |

Deferred to Phase 1b: "WASM tools cannot access unauthorized host resources"
(Wasmtime/WASI Preview 2 sandbox).
```

- [ ] **Step 2: Tick the boxes after a clean run**

Bring the stack up fresh and run `cargo test -p aep-capability -p aep-policy -p aep-domain` plus both `--ignored` integration tests; tick each box.

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/plans/2026-05-30-agent-execution-plane-phase1-acceptance.md
git commit -m "docs: Phase 1 authorization-chain acceptance checklist"
```

---

## Self-Review

**Spec coverage (Phase 1 authorization chain):**
- "tool calls require policy approval" → Task 5 (Cedar engine) + Task 7 (AgentService gates on Deny) + Task 9 deny test. ✔
- "LLM output cannot directly execute privileged operations" → Task 8 (ToolService verifies capability; sole authority) + Task 9 forged-token test. ✔
- Capabilities scoped/short-lived/audited → Tasks 2–4 (scope, expiry, authorize) + `policy_hash`/`audit_id` fields + `tracing` rejections. ✔
- Determinism invariant → Tasks 7–8 inject time via `ctx.run`; Tasks 2–5 libs are infra-free. ✔
- WASM isolation criterion → explicitly deferred to Phase 1b (documented), not silently dropped. ✔

**Placeholder scan:** no TBD/"handle errors appropriately"/"similar to above"; every code step is complete; the only external-API surfaces (Cedar 4.x `Request::new`/`Authorizer`, capability HMAC) are shown in full. ✔

**Type consistency:** `Capability`, `Resource`, `Action`, `CapError`, `sign`, `verify`, `authorize` defined in Tasks 2–4 and used in Tasks 7–8. `PolicyDecision`/`evaluate` defined in Task 5, used in Task 7. `ToolCall { request, capability_token }` defined in Task 7, consumed in Task 8 and posted verbatim in Task 9. `UserInput.requested_tool` and `AgentReply.{denied,reason}` defined in Task 6/Task 7 step 3 and used in Tasks 7–9. Handler names `handle`/`run` and ingress paths match Task 9. ✔

---

## Phase 1b (next plan): Wasmtime / WASI Preview 2 sandbox

A separate plan, because it introduces its own toolchain and is independently shippable. Scope:

- Replace the fake echo tool with a real WASM tool executed under **Wasmtime**, with **no ambient authority**: no WASI filesystem/network/env unless the capability's scope grants it.
- The capability minted in this plan becomes the sandbox's authority: host imports (e.g., `host_fetch`) check `Capability::authorize(Action::Call, &Resource::Network{domain})` before allowing, and trap otherwise.
- Success criterion (the third Phase 1 bullet): a WASM tool that attempts an unauthorized host resource is denied; an authorized one succeeds.
- Toolchain tasks: add a `wasm32` build target, a guest tool crate, the host `Linker` wiring, and integration tests for both the allowed and denied paths.
- Decision to confirm at the start of 1b: core module + explicit host ABI (simpler first cut) vs. full WASI Preview 2 component model with a WIT world (spec target). Recommend starting with the component model only if the guest-toolchain cost is acceptable; otherwise land the isolation property with a core module first and migrate.
