# Rust Actor OS Runtime MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a single-node Rust Actor OS Runtime MVP with durable actor turns, event log replay, bounded mailbox scheduling, capability-gated tool execution, WASM sandbox abstraction, and causal tracing.

**Architecture:** The MVP uses Tokio as the async substrate and implements runtime semantics above it: actors are durable identities, actor turns are bounded execution slices, and the event log is the authority. The first storage backend is in-memory for deterministic tests, with traits shaped so a WAL/LSM backend can replace it later.

**Tech Stack:** Rust 2021, Tokio, serde, thiserror, uuid, time, tracing, async-trait, wasmtime behind an optional feature, cargo test.

---

## Scope

This plan implements the single-node MVP from `docs/superpowers/specs/2026-05-28-rust-actor-os-runtime-design.md`.

Included:

- Rust workspace scaffold.
- Core actor/message/event/capability types.
- In-memory event log and mailbox.
- Actor turn protocol.
- Local scheduler.
- Capability broker and policy engine.
- WASM sandbox abstraction with a fake sandbox for tests and optional Wasmtime backend stub.
- AgentActor -> ToolActor -> SandboxActor flow.
- Replay by actor ID and sequence range.
- Structured tracing fields and causal metadata.

Excluded:

- Distributed actor directory.
- Raft or replicated storage.
- Process sandbox.
- MicroVM sandbox.
- Custom executor.
- Cross-node actor migration.
- Production storage backend.

## File Structure

Create:

```text
Cargo.toml
crates/api-types/Cargo.toml
crates/api-types/src/lib.rs
crates/event-log/Cargo.toml
crates/event-log/src/lib.rs
crates/actor-mailbox/Cargo.toml
crates/actor-mailbox/src/lib.rs
crates/capability/Cargo.toml
crates/capability/src/lib.rs
crates/sandbox/Cargo.toml
crates/sandbox/src/lib.rs
crates/actor-kernel/Cargo.toml
crates/actor-kernel/src/lib.rs
crates/observability/Cargo.toml
crates/observability/src/lib.rs
crates/agent-core/Cargo.toml
crates/agent-core/src/lib.rs
crates/runtime-tests/Cargo.toml
crates/runtime-tests/tests/actor_flow.rs
```

Responsibilities:

- `api-types`: stable shared IDs, messages, events, actor refs, causal metadata.
- `event-log`: append-only actor event log trait and in-memory implementation.
- `actor-mailbox`: durable mailbox trait and in-memory bounded implementation.
- `capability`: capability token, policy decisions, broker.
- `sandbox`: sandbox trait, fake backend, optional Wasmtime backend boundary.
- `actor-kernel`: actor trait, actor turn runner, local scheduler.
- `observability`: tracing helpers and causal field conventions.
- `agent-core`: MVP AgentActor, ToolActor, MemoryActor, PolicyActor, SandboxActor.
- `runtime-tests`: cross-crate integration tests.

## Task 1: Workspace Scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `crates/api-types/Cargo.toml`
- Create: `crates/api-types/src/lib.rs`

- [ ] **Step 1: Write the root workspace manifest**

Create `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
  "crates/api-types",
  "crates/event-log",
  "crates/actor-mailbox",
  "crates/capability",
  "crates/sandbox",
  "crates/actor-kernel",
  "crates/observability",
  "crates/agent-core",
  "crates/runtime-tests",
]

[workspace.package]
edition = "2021"
license = "Apache-2.0"
version = "0.1.0"

[workspace.dependencies]
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
time = { version = "0.3", features = ["serde", "macros"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "sync", "time"] }
tracing = "0.1"
uuid = { version = "1", features = ["v4", "serde"] }
```

- [ ] **Step 2: Create the api-types manifest**

Create `crates/api-types/Cargo.toml`:

```toml
[package]
name = "api-types"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
time.workspace = true
uuid.workspace = true
```

- [ ] **Step 3: Add placeholder-free shared type module**

Create `crates/api-types/src/lib.rs`:

```rust
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActorId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub Uuid);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId(pub Uuid);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CapabilityId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ActorSeq(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalMeta {
    pub tenant_id: TenantId,
    pub trace_id: TraceId,
    pub message_id: MessageId,
    pub causal_parent_id: Option<MessageId>,
    pub created_at: OffsetDateTime,
}

impl CausalMeta {
    pub fn root(tenant_id: TenantId) -> Self {
        Self {
            tenant_id,
            trace_id: TraceId(Uuid::new_v4()),
            message_id: MessageId(Uuid::new_v4()),
            causal_parent_id: None,
            created_at: OffsetDateTime::now_utc(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActorKind {
    Agent,
    Tool,
    Memory,
    Policy,
    Sandbox,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessagePayload {
    UserInput { content: String },
    ToolIntent { tool_name: String, input: serde_json::Value },
    PolicyApproved { capability_id: CapabilityId },
    PolicyDenied { reason: String },
    RunTool { tool_name: String, input: serde_json::Value, capability_id: CapabilityId },
    ToolCompleted { output: serde_json::Value },
    StoreMemory { key: String, value: serde_json::Value },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorMessage {
    pub to: ActorId,
    pub from: ActorId,
    pub priority: MessagePriority,
    pub idempotency_key: String,
    pub meta: CausalMeta,
    pub payload: MessagePayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MessagePriority {
    Control = 0,
    Command = 1,
    Event = 2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActorEventPayload {
    MessageReceived { message_id: MessageId },
    ToolRequested { tool_name: String, input_hash: String, capability_id: CapabilityId },
    ToolCompleted { output_hash: String },
    MemoryStored { key: String },
    ActorFailed { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorEvent {
    pub actor_id: ActorId,
    pub seq: ActorSeq,
    pub meta: CausalMeta,
    pub payload: ActorEventPayload,
}
```

- [ ] **Step 4: Run the workspace check**

Run: `cargo check -p api-types`

Expected: command succeeds.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/api-types
git commit -m "chore: scaffold actor os workspace"
```

If the working directory is not a Git repository, skip the commit and record that in the execution notes.

## Task 2: Event Log

**Files:**
- Create: `crates/event-log/Cargo.toml`
- Create: `crates/event-log/src/lib.rs`
- Test: `crates/event-log/src/lib.rs`

- [ ] **Step 1: Create manifest**

Create `crates/event-log/Cargo.toml`:

```toml
[package]
name = "event-log"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
api-types = { path = "../api-types" }
async-trait.workspace = true
thiserror.workspace = true
tokio.workspace = true
```

- [ ] **Step 2: Write failing tests for append and replay**

Create `crates/event-log/src/lib.rs` with tests first:

```rust
use api_types::{ActorEvent, ActorId, ActorSeq};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EventLogError {
    #[error("event sequence mismatch for actor {actor_id:?}: expected {expected:?}, got {actual:?}")]
    SequenceMismatch { actor_id: ActorId, expected: ActorSeq, actual: ActorSeq },
}

#[async_trait]
pub trait EventLog: Send + Sync {
    async fn append(&self, events: Vec<ActorEvent>) -> Result<(), EventLogError>;
    async fn replay(&self, actor_id: &ActorId, from: ActorSeq, to: Option<ActorSeq>) -> Vec<ActorEvent>;
}

#[derive(Debug, Default, Clone)]
pub struct InMemoryEventLog {
    inner: Arc<Mutex<HashMap<ActorId, Vec<ActorEvent>>>>,
}

impl InMemoryEventLog {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl EventLog for InMemoryEventLog {
    async fn append(&self, events: Vec<ActorEvent>) -> Result<(), EventLogError> {
        let mut inner = self.inner.lock().await;
        for event in events {
            let stream = inner.entry(event.actor_id.clone()).or_default();
            let expected = ActorSeq(stream.len() as u64 + 1);
            if event.seq != expected {
                return Err(EventLogError::SequenceMismatch {
                    actor_id: event.actor_id,
                    expected,
                    actual: event.seq,
                });
            }
            stream.push(event);
        }
        Ok(())
    }

    async fn replay(&self, actor_id: &ActorId, from: ActorSeq, to: Option<ActorSeq>) -> Vec<ActorEvent> {
        let inner = self.inner.lock().await;
        let upper = to.unwrap_or(ActorSeq(u64::MAX));
        inner
            .get(actor_id)
            .into_iter()
            .flat_map(|events| events.iter())
            .filter(|event| event.seq >= from && event.seq <= upper)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_types::{ActorEventPayload, CausalMeta, TenantId};

    fn event(actor_id: &ActorId, seq: u64) -> ActorEvent {
        ActorEvent {
            actor_id: actor_id.clone(),
            seq: ActorSeq(seq),
            meta: CausalMeta::root(TenantId("tenant-a".to_string())),
            payload: ActorEventPayload::MemoryStored { key: format!("k-{seq}") },
        }
    }

    #[tokio::test]
    async fn appends_and_replays_actor_events_by_sequence_range() {
        let log = InMemoryEventLog::new();
        let actor_id = ActorId("agent-1".to_string());

        log.append(vec![event(&actor_id, 1), event(&actor_id, 2), event(&actor_id, 3)])
            .await
            .unwrap();

        let replayed = log.replay(&actor_id, ActorSeq(2), Some(ActorSeq(3))).await;
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[0].seq, ActorSeq(2));
        assert_eq!(replayed[1].seq, ActorSeq(3));
    }

    #[tokio::test]
    async fn rejects_non_contiguous_actor_sequence() {
        let log = InMemoryEventLog::new();
        let actor_id = ActorId("agent-1".to_string());

        let err = log.append(vec![event(&actor_id, 2)]).await.unwrap_err();

        assert_eq!(
            err,
            EventLogError::SequenceMismatch {
                actor_id,
                expected: ActorSeq(1),
                actual: ActorSeq(2)
            }
        );
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p event-log`

Expected: both tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/event-log Cargo.toml
git commit -m "feat: add actor event log"
```

Skip commit if not in a Git repository.

## Task 3: Durable Mailbox

**Files:**
- Create: `crates/actor-mailbox/Cargo.toml`
- Create: `crates/actor-mailbox/src/lib.rs`
- Test: `crates/actor-mailbox/src/lib.rs`

- [ ] **Step 1: Create manifest**

Create `crates/actor-mailbox/Cargo.toml`:

```toml
[package]
name = "actor-mailbox"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
api-types = { path = "../api-types" }
async-trait.workspace = true
thiserror.workspace = true
tokio.workspace = true
```

- [ ] **Step 2: Implement bounded in-memory mailbox with tests**

Create `crates/actor-mailbox/src/lib.rs`:

```rust
use api_types::{ActorId, ActorMessage};
use async_trait::async_trait;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MailboxError {
    #[error("mailbox for {actor_id:?} is full")]
    MailboxFull { actor_id: ActorId },
}

#[async_trait]
pub trait Mailbox: Send + Sync {
    async fn enqueue(&self, message: ActorMessage) -> Result<(), MailboxError>;
    async fn pull(&self, actor_id: &ActorId, limit: usize) -> Vec<ActorMessage>;
    async fn depth(&self, actor_id: &ActorId) -> usize;
}

#[derive(Debug, Clone)]
pub struct InMemoryMailbox {
    capacity_per_actor: usize,
    inner: Arc<Mutex<HashMap<ActorId, ActorQueue>>>,
}

#[derive(Debug, Default)]
struct ActorQueue {
    idempotency_keys: HashSet<String>,
    messages: VecDeque<ActorMessage>,
}

impl InMemoryMailbox {
    pub fn new(capacity_per_actor: usize) -> Self {
        Self {
            capacity_per_actor,
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl Mailbox for InMemoryMailbox {
    async fn enqueue(&self, message: ActorMessage) -> Result<(), MailboxError> {
        let mut inner = self.inner.lock().await;
        let queue = inner.entry(message.to.clone()).or_default();
        if queue.idempotency_keys.contains(&message.idempotency_key) {
            return Ok(());
        }
        if queue.messages.len() >= self.capacity_per_actor {
            return Err(MailboxError::MailboxFull { actor_id: message.to });
        }
        queue.idempotency_keys.insert(message.idempotency_key.clone());
        queue.messages.push_back(message);
        Ok(())
    }

    async fn pull(&self, actor_id: &ActorId, limit: usize) -> Vec<ActorMessage> {
        let mut inner = self.inner.lock().await;
        let Some(queue) = inner.get_mut(actor_id) else {
            return Vec::new();
        };
        let mut pulled = Vec::new();
        for _ in 0..limit {
            let Some(message) = queue.messages.pop_front() else {
                break;
            };
            queue.idempotency_keys.remove(&message.idempotency_key);
            pulled.push(message);
        }
        pulled.sort_by_key(|message| message.priority);
        pulled
    }

    async fn depth(&self, actor_id: &ActorId) -> usize {
        let inner = self.inner.lock().await;
        inner.get(actor_id).map(|queue| queue.messages.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_types::{CausalMeta, MessagePayload, MessagePriority, TenantId};

    fn message(to: &ActorId, key: &str) -> ActorMessage {
        ActorMessage {
            to: to.clone(),
            from: ActorId("sender".to_string()),
            priority: MessagePriority::Command,
            idempotency_key: key.to_string(),
            meta: CausalMeta::root(TenantId("tenant-a".to_string())),
            payload: MessagePayload::UserInput { content: "hello".to_string() },
        }
    }

    #[tokio::test]
    async fn deduplicates_by_idempotency_key() {
        let mailbox = InMemoryMailbox::new(10);
        let actor_id = ActorId("agent-1".to_string());

        mailbox.enqueue(message(&actor_id, "same")).await.unwrap();
        mailbox.enqueue(message(&actor_id, "same")).await.unwrap();

        assert_eq!(mailbox.depth(&actor_id).await, 1);
    }

    #[tokio::test]
    async fn enforces_per_actor_capacity() {
        let mailbox = InMemoryMailbox::new(1);
        let actor_id = ActorId("agent-1".to_string());

        mailbox.enqueue(message(&actor_id, "a")).await.unwrap();
        let err = mailbox.enqueue(message(&actor_id, "b")).await.unwrap_err();

        assert_eq!(err, MailboxError::MailboxFull { actor_id });
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p actor-mailbox`

Expected: both tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/actor-mailbox Cargo.toml
git commit -m "feat: add bounded actor mailbox"
```

Skip commit if not in a Git repository.

## Task 4: Capability Broker

**Files:**
- Create: `crates/capability/Cargo.toml`
- Create: `crates/capability/src/lib.rs`
- Test: `crates/capability/src/lib.rs`

- [ ] **Step 1: Create manifest**

Create `crates/capability/Cargo.toml`:

```toml
[package]
name = "capability"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
api-types = { path = "../api-types" }
serde.workspace = true
thiserror.workspace = true
time.workspace = true
uuid.workspace = true
```

- [ ] **Step 2: Implement scoped capability issuance**

Create `crates/capability/src/lib.rs`:

```rust
use api_types::{ActorId, CapabilityId, TenantId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resource {
    Tool { name: String },
    Network { domain: String },
    File { path_prefix: String },
    Secret { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Call,
    Read,
    Write,
    Spawn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub id: CapabilityId,
    pub tenant_id: TenantId,
    pub subject: ActorId,
    pub resource: Resource,
    pub actions: Vec<Action>,
    pub expires_at: OffsetDateTime,
    pub audit_id: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CapabilityError {
    #[error("policy denied: {0}")]
    PolicyDenied(String),
    #[error("capability not found")]
    NotFound,
    #[error("capability expired")]
    Expired,
    #[error("capability does not authorize action")]
    Unauthorized,
}

#[derive(Debug, Default)]
pub struct CapabilityBroker {
    issued: HashMap<CapabilityId, Capability>,
}

impl CapabilityBroker {
    pub fn issue_tool_call(
        &mut self,
        tenant_id: TenantId,
        subject: ActorId,
        tool_name: &str,
        ttl: Duration,
    ) -> Result<Capability, CapabilityError> {
        if tool_name.trim().is_empty() {
            return Err(CapabilityError::PolicyDenied("tool name is empty".to_string()));
        }
        let capability = Capability {
            id: CapabilityId(Uuid::new_v4()),
            tenant_id,
            subject,
            resource: Resource::Tool { name: tool_name.to_string() },
            actions: vec![Action::Call],
            expires_at: OffsetDateTime::now_utc() + ttl,
            audit_id: Uuid::new_v4().to_string(),
        };
        self.issued.insert(capability.id.clone(), capability.clone());
        Ok(capability)
    }

    pub fn authorize(&self, capability_id: &CapabilityId, action: Action, resource: &Resource) -> Result<(), CapabilityError> {
        let capability = self.issued.get(capability_id).ok_or(CapabilityError::NotFound)?;
        if capability.expires_at <= OffsetDateTime::now_utc() {
            return Err(CapabilityError::Expired);
        }
        if &capability.resource != resource || !capability.actions.contains(&action) {
            return Err(CapabilityError::Unauthorized);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issues_and_authorizes_tool_call_capability() {
        let mut broker = CapabilityBroker::default();
        let capability = broker
            .issue_tool_call(
                TenantId("tenant-a".to_string()),
                ActorId("agent-1".to_string()),
                "echo",
                Duration::minutes(5),
            )
            .unwrap();

        broker
            .authorize(&capability.id, Action::Call, &Resource::Tool { name: "echo".to_string() })
            .unwrap();
    }

    #[test]
    fn rejects_wrong_resource() {
        let mut broker = CapabilityBroker::default();
        let capability = broker
            .issue_tool_call(
                TenantId("tenant-a".to_string()),
                ActorId("agent-1".to_string()),
                "echo",
                Duration::minutes(5),
            )
            .unwrap();

        let err = broker
            .authorize(&capability.id, Action::Call, &Resource::Tool { name: "shell".to_string() })
            .unwrap_err();

        assert_eq!(err, CapabilityError::Unauthorized);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p capability`

Expected: both tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/capability Cargo.toml
git commit -m "feat: add capability broker"
```

Skip commit if not in a Git repository.

## Task 5: Sandbox Abstraction

**Files:**
- Create: `crates/sandbox/Cargo.toml`
- Create: `crates/sandbox/src/lib.rs`
- Test: `crates/sandbox/src/lib.rs`

- [ ] **Step 1: Create manifest**

Create `crates/sandbox/Cargo.toml`:

```toml
[package]
name = "sandbox"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
api-types = { path = "../api-types" }
capability = { path = "../capability" }
async-trait.workspace = true
serde_json.workspace = true
thiserror.workspace = true
```

- [ ] **Step 2: Implement fake sandbox backend**

Create `crates/sandbox/src/lib.rs`:

```rust
use api_types::CapabilityId;
use async_trait::async_trait;
use capability::{Action, CapabilityBroker, CapabilityError, Resource};
use serde_json::{json, Value};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error(transparent)]
    Capability(#[from] CapabilityError),
    #[error("tool not found: {0}")]
    ToolNotFound(String),
}

#[derive(Debug, Clone)]
pub struct SandboxRequest {
    pub tool_name: String,
    pub input: Value,
    pub capability_id: CapabilityId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SandboxOutput {
    pub output: Value,
}

#[async_trait]
pub trait SandboxBackend: Send + Sync {
    async fn execute(&self, request: SandboxRequest) -> Result<SandboxOutput, SandboxError>;
}

#[derive(Debug, Clone)]
pub struct FakeSandbox {
    broker: Arc<Mutex<CapabilityBroker>>,
}

impl FakeSandbox {
    pub fn new(broker: Arc<Mutex<CapabilityBroker>>) -> Self {
        Self { broker }
    }
}

#[async_trait]
impl SandboxBackend for FakeSandbox {
    async fn execute(&self, request: SandboxRequest) -> Result<SandboxOutput, SandboxError> {
        self.broker
            .lock()
            .await
            .authorize(
                &request.capability_id,
                Action::Call,
                &Resource::Tool { name: request.tool_name.clone() },
            )?;

        match request.tool_name.as_str() {
            "echo" => Ok(SandboxOutput { output: request.input }),
            "upper" => {
                let value = request.input.get("text").and_then(Value::as_str).unwrap_or_default();
                Ok(SandboxOutput { output: json!({ "text": value.to_uppercase() }) })
            }
            other => Err(SandboxError::ToolNotFound(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_types::{ActorId, TenantId};
    use capability::CapabilityBroker;
    use time::Duration;

    #[tokio::test]
    async fn executes_authorized_echo_tool() {
        let broker = Arc::new(Mutex::new(CapabilityBroker::default()));
        let capability = broker
            .lock()
            .await
            .issue_tool_call(
                TenantId("tenant-a".to_string()),
                ActorId("agent-1".to_string()),
                "echo",
                Duration::minutes(5),
            )
            .unwrap();

        let sandbox = FakeSandbox::new(broker);
        let output = sandbox
            .execute(SandboxRequest {
                tool_name: "echo".to_string(),
                input: json!({ "ok": true }),
                capability_id: capability.id,
            })
            .await
            .unwrap();

        assert_eq!(output.output, json!({ "ok": true }));
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p sandbox`

Expected: tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/sandbox Cargo.toml
git commit -m "feat: add sandbox backend abstraction"
```

Skip commit if not in a Git repository.

## Task 6: Actor Kernel Turn Runner

**Files:**
- Create: `crates/actor-kernel/Cargo.toml`
- Create: `crates/actor-kernel/src/lib.rs`
- Test: `crates/actor-kernel/src/lib.rs`

- [ ] **Step 1: Create manifest**

Create `crates/actor-kernel/Cargo.toml`:

```toml
[package]
name = "actor-kernel"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
actor-mailbox = { path = "../actor-mailbox" }
api-types = { path = "../api-types" }
async-trait.workspace = true
event-log = { path = "../event-log" }
thiserror.workspace = true
tokio.workspace = true
```

- [ ] **Step 2: Implement actor trait and turn runner**

Create `crates/actor-kernel/src/lib.rs`:

```rust
use actor_mailbox::Mailbox;
use api_types::{ActorEvent, ActorId, ActorMessage, ActorSeq};
use async_trait::async_trait;
use event_log::{EventLog, EventLogError};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct TurnBudget {
    pub max_messages: usize,
    pub next_seq: ActorSeq,
}

#[derive(Debug, Clone)]
pub struct ActorTurn {
    pub actor_id: ActorId,
    pub messages: Vec<ActorMessage>,
    pub budget: TurnBudget,
}

#[derive(Debug, Default)]
pub struct TurnOutcome {
    pub events: Vec<ActorEvent>,
    pub outgoing: Vec<ActorMessage>,
}

#[derive(Debug, Error)]
pub enum KernelError {
    #[error(transparent)]
    EventLog(#[from] EventLogError),
}

#[async_trait]
pub trait Actor: Send {
    async fn handle_turn(&mut self, turn: ActorTurn) -> TurnOutcome;
}

pub struct ActorRunner<L, M> {
    event_log: Arc<L>,
    mailbox: Arc<M>,
}

impl<L, M> ActorRunner<L, M>
where
    L: EventLog + 'static,
    M: Mailbox + 'static,
{
    pub fn new(event_log: Arc<L>, mailbox: Arc<M>) -> Self {
        Self { event_log, mailbox }
    }

    pub async fn run_once<A: Actor>(
        &self,
        actor_id: ActorId,
        actor: &mut A,
        budget: TurnBudget,
    ) -> Result<TurnOutcome, KernelError> {
        let messages = self.mailbox.pull(&actor_id, budget.max_messages).await;
        let turn = ActorTurn { actor_id, messages, budget };
        let outcome = actor.handle_turn(turn).await;
        self.event_log.append(outcome.events.clone()).await?;
        for message in &outcome.outgoing {
            let _ = self.mailbox.enqueue(message.clone()).await;
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actor_mailbox::InMemoryMailbox;
    use api_types::{
        ActorEventPayload, ActorMessage, CausalMeta, MessagePayload, MessagePriority, TenantId,
    };
    use event_log::InMemoryEventLog;

    #[derive(Default)]
    struct RecordingActor;

    #[async_trait]
    impl Actor for RecordingActor {
        async fn handle_turn(&mut self, turn: ActorTurn) -> TurnOutcome {
            let events = turn
                .messages
                .into_iter()
                .enumerate()
                .map(|(idx, message)| ActorEvent {
                    actor_id: turn.actor_id.clone(),
                    seq: ActorSeq(turn.budget.next_seq.0 + idx as u64),
                    meta: message.meta,
                    payload: ActorEventPayload::MessageReceived { message_id: message.meta.message_id },
                })
                .collect();
            TurnOutcome { events, outgoing: Vec::new() }
        }
    }

    fn message(to: &ActorId) -> ActorMessage {
        ActorMessage {
            to: to.clone(),
            from: ActorId("sender".to_string()),
            priority: MessagePriority::Command,
            idempotency_key: "input-1".to_string(),
            meta: CausalMeta::root(TenantId("tenant-a".to_string())),
            payload: MessagePayload::UserInput { content: "hello".to_string() },
        }
    }

    #[tokio::test]
    async fn runner_pulls_messages_and_commits_events() {
        let log = Arc::new(InMemoryEventLog::new());
        let mailbox = Arc::new(InMemoryMailbox::new(10));
        let actor_id = ActorId("agent-1".to_string());
        mailbox.enqueue(message(&actor_id)).await.unwrap();

        let runner = ActorRunner::new(log.clone(), mailbox);
        let mut actor = RecordingActor::default();
        runner
            .run_once(
                actor_id.clone(),
                &mut actor,
                TurnBudget { max_messages: 10, next_seq: ActorSeq(1) },
            )
            .await
            .unwrap();

        let events = log.replay(&actor_id, ActorSeq(1), None).await;
        assert_eq!(events.len(), 1);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p actor-kernel`

Expected: tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/actor-kernel Cargo.toml
git commit -m "feat: add actor turn runner"
```

Skip commit if not in a Git repository.

## Task 7: Agent Core Flow

**Files:**
- Create: `crates/agent-core/Cargo.toml`
- Create: `crates/agent-core/src/lib.rs`
- Test: `crates/agent-core/src/lib.rs`

- [ ] **Step 1: Create manifest**

Create `crates/agent-core/Cargo.toml`:

```toml
[package]
name = "agent-core"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
actor-kernel = { path = "../actor-kernel" }
api-types = { path = "../api-types" }
async-trait.workspace = true
serde_json.workspace = true
time.workspace = true
uuid.workspace = true
```

- [ ] **Step 2: Implement AgentActor intent emission**

Create `crates/agent-core/src/lib.rs`:

```rust
use actor_kernel::{Actor, ActorTurn, TurnOutcome};
use api_types::{
    ActorEvent, ActorEventPayload, ActorId, ActorMessage, ActorSeq, CausalMeta, CapabilityId,
    MessagePayload, MessagePriority,
};
use async_trait::async_trait;

pub struct AgentActor {
    pub tool_actor: ActorId,
}

#[async_trait]
impl Actor for AgentActor {
    async fn handle_turn(&mut self, turn: ActorTurn) -> TurnOutcome {
        let mut events = Vec::new();
        let mut outgoing = Vec::new();
        let mut seq = turn.budget.next_seq.0;

        for message in turn.messages {
            events.push(ActorEvent {
                actor_id: turn.actor_id.clone(),
                seq: ActorSeq(seq),
                meta: message.meta.clone(),
                payload: ActorEventPayload::MessageReceived { message_id: message.meta.message_id.clone() },
            });
            seq += 1;

            if let MessagePayload::UserInput { content } = message.payload {
                outgoing.push(ActorMessage {
                    to: self.tool_actor.clone(),
                    from: turn.actor_id.clone(),
                    priority: MessagePriority::Command,
                    idempotency_key: format!("tool-intent-{}", message.meta.message_id.0),
                    meta: child_meta(&message.meta),
                    payload: MessagePayload::ToolIntent {
                        tool_name: "echo".to_string(),
                        input: serde_json::json!({ "content": content }),
                    },
                });
            }
        }

        TurnOutcome { events, outgoing }
    }
}

pub struct ToolActor {
    pub sandbox_actor: ActorId,
}

#[async_trait]
impl Actor for ToolActor {
    async fn handle_turn(&mut self, turn: ActorTurn) -> TurnOutcome {
        let mut events = Vec::new();
        let mut outgoing = Vec::new();
        let mut seq = turn.budget.next_seq.0;

        for message in turn.messages {
            if let MessagePayload::ToolIntent { tool_name, input } = message.payload {
                let capability_id = CapabilityId(uuid::Uuid::new_v4());
                events.push(ActorEvent {
                    actor_id: turn.actor_id.clone(),
                    seq: ActorSeq(seq),
                    meta: message.meta.clone(),
                    payload: ActorEventPayload::ToolRequested {
                        tool_name: tool_name.clone(),
                        input_hash: format!("{:x}", input.to_string().len()),
                        capability_id: capability_id.clone(),
                    },
                });
                seq += 1;

                outgoing.push(ActorMessage {
                    to: self.sandbox_actor.clone(),
                    from: turn.actor_id.clone(),
                    priority: MessagePriority::Command,
                    idempotency_key: format!("run-tool-{}", message.meta.message_id.0),
                    meta: child_meta(&message.meta),
                    payload: MessagePayload::RunTool { tool_name, input, capability_id },
                });
            }
        }

        TurnOutcome { events, outgoing }
    }
}

fn child_meta(parent: &CausalMeta) -> CausalMeta {
    CausalMeta {
        tenant_id: parent.tenant_id.clone(),
        trace_id: parent.trace_id.clone(),
        message_id: api_types::MessageId(uuid::Uuid::new_v4()),
        causal_parent_id: Some(parent.message_id.clone()),
        created_at: time::OffsetDateTime::now_utc(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_types::{CausalMeta, TenantId};

    #[tokio::test]
    async fn agent_turn_emits_tool_intent_for_user_input() {
        let agent_id = ActorId("agent-1".to_string());
        let tool_id = ActorId("tool-1".to_string());
        let mut actor = AgentActor { tool_actor: tool_id.clone() };
        let message = ActorMessage {
            to: agent_id.clone(),
            from: ActorId("session-1".to_string()),
            priority: MessagePriority::Command,
            idempotency_key: "user-1".to_string(),
            meta: CausalMeta::root(TenantId("tenant-a".to_string())),
            payload: MessagePayload::UserInput { content: "hello".to_string() },
        };

        let outcome = actor
            .handle_turn(ActorTurn {
                actor_id: agent_id,
                messages: vec![message],
                budget: actor_kernel::TurnBudget { max_messages: 10, next_seq: ActorSeq(1) },
            })
            .await;

        assert_eq!(outcome.events.len(), 1);
        assert_eq!(outcome.outgoing.len(), 1);
        assert_eq!(outcome.outgoing[0].to, tool_id);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p agent-core`

Expected: tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/agent-core Cargo.toml
git commit -m "feat: add agent and tool actors"
```

Skip commit if not in a Git repository.

## Task 8: Observability Helpers

**Files:**
- Create: `crates/observability/Cargo.toml`
- Create: `crates/observability/src/lib.rs`
- Test: `crates/observability/src/lib.rs`

- [ ] **Step 1: Create manifest**

Create `crates/observability/Cargo.toml`:

```toml
[package]
name = "observability"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
api-types = { path = "../api-types" }
tracing.workspace = true
```

- [ ] **Step 2: Implement causal tracing field formatter**

Create `crates/observability/src/lib.rs`:

```rust
use api_types::{ActorId, CausalMeta};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceFields {
    pub tenant_id: String,
    pub actor_id: String,
    pub trace_id: String,
    pub message_id: String,
    pub causal_parent_id: Option<String>,
}

pub fn fields_for(actor_id: &ActorId, meta: &CausalMeta) -> TraceFields {
    TraceFields {
        tenant_id: meta.tenant_id.0.clone(),
        actor_id: actor_id.0.clone(),
        trace_id: meta.trace_id.0.to_string(),
        message_id: meta.message_id.0.to_string(),
        causal_parent_id: meta.causal_parent_id.as_ref().map(|id| id.0.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_types::TenantId;

    #[test]
    fn exposes_required_causal_fields() {
        let actor_id = ActorId("agent-1".to_string());
        let meta = CausalMeta::root(TenantId("tenant-a".to_string()));

        let fields = fields_for(&actor_id, &meta);

        assert_eq!(fields.tenant_id, "tenant-a");
        assert_eq!(fields.actor_id, "agent-1");
        assert_eq!(fields.trace_id, meta.trace_id.0.to_string());
        assert_eq!(fields.message_id, meta.message_id.0.to_string());
        assert_eq!(fields.causal_parent_id, None);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p observability`

Expected: tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/observability Cargo.toml
git commit -m "feat: add causal observability helpers"
```

Skip commit if not in a Git repository.

## Task 9: End-to-End Runtime Test

**Files:**
- Create: `crates/runtime-tests/Cargo.toml`
- Create: `crates/runtime-tests/tests/actor_flow.rs`

- [ ] **Step 1: Create manifest**

Create `crates/runtime-tests/Cargo.toml`:

```toml
[package]
name = "runtime-tests"
edition.workspace = true
license.workspace = true
version.workspace = true

[dev-dependencies]
actor-kernel = { path = "../actor-kernel" }
actor-mailbox = { path = "../actor-mailbox" }
agent-core = { path = "../agent-core" }
api-types = { path = "../api-types" }
event-log = { path = "../event-log" }
tokio.workspace = true
```

- [ ] **Step 2: Add integration test for AgentActor to ToolActor message flow**

Create `crates/runtime-tests/tests/actor_flow.rs`:

```rust
use actor_kernel::{ActorRunner, TurnBudget};
use actor_mailbox::{InMemoryMailbox, Mailbox};
use agent_core::{AgentActor, ToolActor};
use api_types::{ActorId, ActorMessage, ActorSeq, CausalMeta, MessagePayload, MessagePriority, TenantId};
use event_log::{EventLog, InMemoryEventLog};
use std::sync::Arc;

#[tokio::test]
async fn agent_user_input_flows_to_tool_actor_and_is_replayable() {
    let log = Arc::new(InMemoryEventLog::new());
    let mailbox = Arc::new(InMemoryMailbox::new(16));
    let runner = ActorRunner::new(log.clone(), mailbox.clone());

    let agent_id = ActorId("agent-1".to_string());
    let tool_id = ActorId("tool-1".to_string());
    let sandbox_id = ActorId("sandbox-1".to_string());

    mailbox
        .enqueue(ActorMessage {
            to: agent_id.clone(),
            from: ActorId("session-1".to_string()),
            priority: MessagePriority::Command,
            idempotency_key: "user-input-1".to_string(),
            meta: CausalMeta::root(TenantId("tenant-a".to_string())),
            payload: MessagePayload::UserInput { content: "hello".to_string() },
        })
        .await
        .unwrap();

    let mut agent = AgentActor { tool_actor: tool_id.clone() };
    runner
        .run_once(
            agent_id.clone(),
            &mut agent,
            TurnBudget { max_messages: 8, next_seq: ActorSeq(1) },
        )
        .await
        .unwrap();

    assert_eq!(mailbox.depth(&tool_id).await, 1);

    let mut tool = ToolActor { sandbox_actor: sandbox_id };
    runner
        .run_once(
            tool_id.clone(),
            &mut tool,
            TurnBudget { max_messages: 8, next_seq: ActorSeq(1) },
        )
        .await
        .unwrap();

    let agent_events = log.replay(&agent_id, ActorSeq(1), None).await;
    let tool_events = log.replay(&tool_id, ActorSeq(1), None).await;

    assert_eq!(agent_events.len(), 1);
    assert_eq!(tool_events.len(), 1);
    assert_eq!(mailbox.depth(&tool_id).await, 0);
}
```

- [ ] **Step 3: Run integration test**

Run: `cargo test -p runtime-tests`

Expected: test passes.

- [ ] **Step 4: Run full test suite**

Run: `cargo test --workspace`

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/runtime-tests Cargo.toml
git commit -m "test: add actor runtime integration flow"
```

Skip commit if not in a Git repository.

## Task 10: MVP Verification Notes

**Files:**
- Create: `docs/superpowers/plans/2026-05-28-rust-actor-os-runtime-verification.md`

- [ ] **Step 1: Create verification checklist**

Create `docs/superpowers/plans/2026-05-28-rust-actor-os-runtime-verification.md`:

```markdown
# Rust Actor OS Runtime MVP Verification

## Commands

- `cargo check --workspace`
- `cargo test --workspace`

## Expected MVP Properties

- Actor event log rejects non-contiguous sequence numbers.
- Actor mailbox deduplicates idempotency keys and enforces capacity.
- ActorRunner pulls bounded messages and commits events.
- CapabilityBroker authorizes only scoped tool calls.
- FakeSandbox refuses execution without a matching capability.
- AgentActor emits ToolIntent from UserInput.
- ToolActor records ToolRequested and emits RunTool.
- Integration flow is replayable by actor ID and sequence range.

## Known Non-MVP Areas

- No persistent WAL backend.
- No distributed scheduler.
- No process or microVM sandbox.
- No production Wasmtime component execution.
- No actor migration.
```

- [ ] **Step 2: Run verification commands**

Run: `cargo check --workspace`

Expected: succeeds.

Run: `cargo test --workspace`

Expected: succeeds.

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/plans/2026-05-28-rust-actor-os-runtime-verification.md
git commit -m "docs: add actor os runtime verification checklist"
```

Skip commit if not in a Git repository.

## Self-Review

Spec coverage:

- Durable actor execution: Tasks 2, 3, 6, 9.
- Event log replay: Tasks 2 and 9.
- Bounded mailbox scheduling: Tasks 3 and 6.
- Capability-secured tool execution: Tasks 4 and 5.
- WASM sandbox path: Task 5 defines the backend boundary; production Wasmtime execution remains outside MVP.
- Agent/tool actor flow: Tasks 7 and 9.
- Causal observability: Tasks 1 and 8.
- Single-node MVP: all tasks avoid distributed scheduling, Raft, microVMs, and custom executors.

Placeholder scan:

- The plan contains no `TBD`, `TODO`, or unspecified implementation steps.
- Production-only items are explicitly excluded from MVP scope.

Type consistency:

- Shared IDs and messages are defined in `api-types`.
- Event log, mailbox, kernel, sandbox, and agent-core use the same `ActorId`, `ActorSeq`, `ActorMessage`, and `ActorEvent` types.
- Test commands target the exact crates introduced in each task.

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-28-rust-actor-os-runtime.md`.

Two execution options:

1. **Subagent-Driven (recommended)** - dispatch a fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** - execute tasks in this session using executing-plans, batch execution with checkpoints.

Which approach?
