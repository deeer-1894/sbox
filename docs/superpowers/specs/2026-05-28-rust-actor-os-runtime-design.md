# Rust Actor OS Runtime / Sandbox / Agent Runtime Design

Date: 2026-05-28

## Status

Approved design direction: Actor OS.

This specification captures the agreed architecture for a Rust-based runtime, sandbox, and agent execution environment. It is a design document, not an implementation plan. The first implementation target is a single-node MVP that proves durable actor execution, capability-secured tool calls, WASM sandboxing, checkpoint/replay, and causal observability.

## Goals

- Build a high-concurrency Rust runtime for long-running AI agents.
- Treat agents, tools, memory, policy, workflow, model calls, and sandboxes as isolated actors.
- Make execution durable, auditable, replayable, and recoverable after process crashes.
- Enforce least-privilege tool execution through a capability broker.
- Support sandboxed tools with a WASM-first isolation model and a clear path to process and microVM isolation.
- Provide runtime-level observability through causal message graphs, actor turn traces, and replay debugging.
- Keep the MVP single-node while preserving clean boundaries for distributed placement and shard-level replication.

## Non-Goals

- Do not build a custom async executor for the MVP.
- Do not implement distributed scheduling, Raft, microVM orchestration, or actor migration in the MVP.
- Do not serialize or migrate in-flight Rust futures.
- Do not allow LLM output to directly execute privileged operations.
- Do not use Docker alone as the sandbox security boundary.
- Do not treat vector memory, snapshots, or materialized views as the authoritative execution state.

## Architectural Principles

```text
Everything important is an Actor.
Everything across boundaries is a Message.
Everything durable is an Event.
Everything privileged is a Capability.
Everything replayable stops at a Side-effect Boundary.
```

The system is not a generic workflow engine and not merely a Tokio service. It is a durable actor execution fabric for AI agents. Runtime tasks are disposable; actor event logs and committed messages are authoritative.

## Reference Architecture

```text
                           ┌───────────────────────────────────────┐
                           │             Control Plane              │
                           │ API / Auth / Tenant / Policy / Quota   │
                           │ Deploy / Config / Actor Directory      │
                           └───────────────────┬───────────────────┘
                                               │
┌──────────────────────────────────────────────▼──────────────────────────────────────────────┐
│                                      Actor Runtime OS                                       │
│                                                                                             │
│  ┌──────────────┐   ┌───────────────┐   ┌────────────────┐   ┌──────────────────────────┐ │
│  │ Actor Kernel │──▶│ Actor Router  │──▶│ Actor Scheduler│──▶│ ActorRunner / Executor   │ │
│  │ lifecycle    │   │ path/mailbox  │   │ local+cluster  │   │ Tokio-backed turn engine │ │
│  └──────┬───────┘   └───────┬───────┘   └───────┬────────┘   └───────────┬──────────────┘ │
│         │                   │                   │                        │                │
│         ▼                   ▼                   ▼                        ▼                │
│  ┌──────────────┐   ┌───────────────┐   ┌────────────────┐   ┌──────────────────────────┐ │
│  │ Event Log    │   │ Mailbox Store │   │ Capability     │   │ Sandbox Actors           │ │
│  │ actor seq    │   │ durable inbox │   │ Broker/Policy  │   │ WASM / proc / microVM    │ │
│  └──────┬───────┘   └───────────────┘   └────────────────┘   └───────────┬──────────────┘ │
│         │                                                                 │                │
│         ▼                                                                 ▼                │
│  ┌───────────────────────────────────────────────────────────────────────────────────────┐ │
│  │ AgentActor / WorkflowActor / ToolActor / MemoryActor / ModelActor / TenantActor       │ │
│  └───────────────────────────────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────┬──────────────────────────────────────────────┘
                                               │
                           ┌───────────────────▼───────────────────┐
                           │               Data Plane               │
                           │ Log / KV / Blob / Vector / WAL / LSM   │
                           │ Snapshot / Queue / Object / Metrics    │
                           └───────────────────────────────────────┘
```

## System Planes

### Control Plane

The control plane owns slow-moving global decisions:

- tenant identity and quotas;
- user and service authentication;
- policy configuration;
- deployment configuration;
- actor directory metadata;
- cluster placement intent;
- billing and audit configuration.

It does not run actor business logic and is not in the hot path for each actor message.

### Runtime Plane

The runtime plane owns actor execution:

- actor lifecycle;
- message routing;
- mailbox scheduling;
- durable event commits;
- checkpoint and replay;
- capability enforcement;
- sandbox orchestration;
- runtime-level observability.

The runtime plane is the core of the Actor OS.

### Data Plane

The data plane owns storage and derived indexes:

- actor event log;
- durable mailbox;
- snapshots;
- blob artifacts;
- vector memory indexes;
- materialized views;
- metrics and traces.

The event log is authoritative. Snapshots, vectors, and query views are derived state.

## Core Actor Types

### TenantActor

Owns tenant-level admission control, fairness, quota, budget accounting, and high-level safety policy.

### SessionActor

Owns user interaction state, streaming response delivery, user input events, and front-end connection state.

### AgentActor

Owns agent lifecycle, goals, current execution intent, memory references, and coordination with workflow, model, memory, and tool actors.

### WorkflowActor

Owns workflow graph state, DAG dependencies, retries, joins, fanout limits, and compensation.

### ModelActor

Owns LLM provider calls, streaming chunks, retries, budget accounting, model fallback, and model call tracing.

### ToolActor

Owns tool invocation semantics, idempotency keys, policy gates, side-effect boundaries, and tool result archival.

### SandboxActor

Owns isolated execution environments: WASM instances for MVP, later process sandboxes and microVMs.

### MemoryActor

Owns episodic memory, semantic memory references, memory compaction, retrieval, trust labels, and contamination boundaries.

### PolicyActor

Owns policy evaluation, approval workflows, risk scoring, and audit records.

### ArtifactActor

Owns files, blobs, reports, browser traces, and external job handles.

## Actor Execution Model

Actors are durable identities, not permanent tasks. A dormant actor should consume no thread and no resident future. Execution happens in bounded actor turns.

```text
1. Acquire actor lease.
2. Load actor snapshot.
3. Replay event log tail if required.
4. Pull a bounded mailbox batch.
5. Process messages within turn budget.
6. Emit events and outgoing messages.
7. Atomically commit events, outgoing messages, and mailbox acknowledgements.
8. Write checkpoint opportunistically.
9. Release lease or reschedule actor if more work remains.
```

Each actor turn has explicit limits:

- maximum messages per turn;
- maximum wall-clock time;
- maximum CPU budget;
- maximum outgoing messages;
- maximum memory delta;
- maximum tool invocations;
- deadline and cancellation token.

Only one turn for a given actor may execute at a time.

## Runtime Core

The MVP uses Tokio as the local async substrate. Tokio provides reactor, timers, network I/O, async primitives, and executor work stealing.

The Actor OS layer provides:

- `ActorRunner`;
- `TurnBudget`;
- `RuntimeContext`;
- `CancellationTree`;
- durable message commit;
- causal tracing;
- capability context;
- actor turn scheduling.

Business code must not call `tokio::spawn` directly. Background work must be attached to a structured runtime scope:

```text
RunScope
ActorTurnScope
ToolInvocationScope
SandboxScope
```

Each scope carries:

```text
tenant_id
actor_id
run_id
trace_id
deadline
cancel_token
quota_handle
capability_context
```

## Mailbox Model

The mailbox is durable and bounded. It is not an in-memory channel.

Messages include:

```text
message_id
actor_id
sender_actor_id
causal_parent_id
trace_id
tenant_id
deadline
priority
idempotency_key
capability_token_ref
payload_hash
schema_version
```

Mailbox lanes:

- control: cancellation, pause, policy revoke, quota update;
- command: run step, call tool, resume, plan;
- event: user replied, tool completed, model chunk received, timer fired.

Required behaviors:

- per-actor mailbox limit;
- per-tenant mailbox limit;
- deduplication by idempotency key;
- dead letter queue;
- poison message handling;
- priority aging;
- visibility timeout or equivalent recovery mechanism.

## Durable State Model

The authoritative state is the actor event log:

```text
(actor_id, seq) -> event
```

Physical storage may group many actors into shard-level append-only segments.

State stores:

- `ActorEventLog(actor_id, seq)`;
- `ActorSnapshot(actor_id, revision)`;
- `Mailbox(actor_id, message_id, priority)`;
- `ActorDirectory(actor_id -> shard_id -> node_id)`;
- `BlobStore` for large payloads;
- `VectorStore` as a derived semantic memory index.

Snapshots accelerate recovery but do not replace the event log.

## Side-Effect Boundary

Tool and model calls are nondeterministic or externally observable. They must be recorded as durable boundaries.

Before an external side effect:

```text
ToolRequested {
  invocation_id,
  idempotency_key,
  actor_id,
  capability_ref,
  input_hash
}
```

After an external side effect:

```text
ToolCompleted {
  invocation_id,
  output_ref,
  external_reference,
  output_hash
}
```

Replay rules:

- if `ToolCompleted` exists, reuse the recorded result;
- if an external reference exists but completion was not committed, reconcile the external system first;
- retry only when the operation is proven idempotent;
- never replay a side effect just because the process crashed.

## Sandbox and Isolation

Isolation is tiered:

```text
Level 0: trusted in-process actor
Level 1: WASM sandbox
Level 2: process sandbox with namespace/seccomp/cgroup
Level 3: microVM through Firecracker or Kata
Level 4: hostile tenant isolation with dedicated VM/network/secret domain
```

The MVP implements Level 1 with Wasmtime and WASI component model where practical.

Default mapping:

- plugin and normal tool execution: WASM;
- POSIX-heavy tools: process sandbox in production;
- browser, shell, arbitrary user code: microVM in later phases.

Tool execution path:

```text
AgentActor emits ToolIntent
PolicyActor evaluates intent
CapabilityBroker mints scoped capability
ToolActor starts invocation
SandboxActor executes with capability
ToolActor records sanitized result
AgentActor consumes result
```

LLM output is untrusted intent. Only the policy engine and capability broker can grant execution authority.

## Capability and Security Model

Capabilities are explicit, scoped, short-lived authorities.

```text
Capability {
  subject: tenant/user/agent/tool
  resource: file/network/secret/browser/model/api
  actions: read/write/call/spawn
  scope: path/domain/secret_name/table
  constraints: ttl/rate/budget/region
  policy_hash
  audit_id
}
```

Policy evaluation considers:

- actor identity;
- tenant policy;
- user consent;
- requested resource;
- tool trust level;
- sandbox level;
- memory trust labels;
- risk score;
- historical behavior;
- regional and compliance constraints.

Security requirements:

- no ambient host authority;
- no plugin direct access to secrets, network, filesystem, or shell;
- no model-granted authority;
- all capability use is audited;
- tool output is classified and sanitized before entering agent memory;
- supply-chain verification is required before production plugin execution.

## Agent Runtime Model

Agent lifecycle:

```text
Created
  -> Runnable
  -> RunningTurn
  -> WaitingModel
  -> WaitingTool
  -> WaitingUser
  -> Checkpointed
  -> Resumed
  -> Completed / Failed / Cancelled / Compensating
```

Agent memory is split into:

- working memory: current prompt/context window;
- episodic memory: event-sourced interaction history;
- semantic memory: vector-indexed derived facts;
- operational memory: tool outputs, files, artifacts, handles;
- policy memory: permission and approval decisions.

Long-running agents are execution slices over durable state:

```text
wake event -> load snapshot -> replay log tail -> execute bounded turn
-> commit events -> checkpoint if needed -> sleep
```

Millions of agents are represented as durable identities. Only runnable actors consume active scheduler capacity.

## Scheduling and Backpressure

The MVP has a local scheduler. Production adds a cluster scheduler.

Node scheduler responsibilities:

- runnable actor queues;
- per-tenant fairness;
- per-actor dedupe;
- mailbox batch sizing;
- sandbox pool limits;
- CPU, memory, and I/O budgets;
- priority lanes;
- cancellation handling.

Backpressure exists at multiple levels:

- actor mailbox;
- tenant runnable queue;
- shard queue;
- node CPU/memory/I/O;
- sandbox pool;
- model provider;
- external tool provider.

The scheduler must avoid runnable storms by coalescing wakeups and deduplicating runnable actor IDs.

## Distributed Evolution

The MVP is single-node. Production evolves toward sharded actor ownership.

Distributed structure:

```text
ActorId -> ShardId -> NodeId
```

Cluster scheduler responsibilities:

- shard placement;
- actor directory;
- shard leases;
- failover;
- hot actor detection;
- tenant fairness;
- regional policy.

Actor migration happens only at turn boundaries:

```text
1. Stop scheduling actor on source.
2. Commit current turn.
3. Seal mailbox cursor.
4. Transfer placement lease.
5. Target loads snapshot and log tail.
6. Target resumes mailbox processing.
```

In-flight futures, processes, and microVMs are not migrated. Tool execution is reconciled through ToolActor state and external job references.

## Observability

The runtime must expose causal observability, not just logs.

Every event, message, and span includes:

```text
tenant_id
actor_id
actor_type
message_id
causal_parent_id
trace_id
turn_id
seq
capability_id
sandbox_id
checkpoint_revision
replay_epoch
```

Required timelines:

- wall-clock timeline;
- logical actor event timeline;
- execution timeline;
- causal message graph.

The runtime debugger must answer:

- why is this agent stuck;
- which actor mailbox is blocked;
- which capability authorized this side effect;
- which tool output wrote this memory;
- which cancellation failed to propagate;
- which tenant exhausted sandbox capacity;
- which poison message caused retry loops.

## Rust Workspace Architecture

Proposed crates:

```text
crates/
  actor-kernel/
  actor-router/
  actor-scheduler/
  actor-mailbox/
  event-log/
  checkpoint/
  capability/
  policy/
  sandbox/
  wasm-runtime/
  process-sandbox/
  microvm/
  agent-core/
  workflow/
  memory/
  observability/
  storage/
  api-types/
  plugin-sdk/
```

MVP crate subset:

```text
actor-kernel
actor-mailbox
actor-scheduler
event-log
checkpoint
capability
policy
sandbox
wasm-runtime
agent-core
memory
observability
storage
api-types
plugin-sdk
```

Trait boundaries are appropriate for storage, event log, mailbox, sandbox backends, policy engines, model providers, vector stores, and blob stores. The actor kernel state machine and scheduler hot path should not be over-abstracted early.

`unsafe` is allowed only in leaf crates with documented invariants and safe public APIs. Candidate areas are shared memory rings, zero-copy buffers, custom slabs, mmap log segments, and low-level sandbox FFI. Orchestration code must remain safe Rust.

## MVP Scope

Build a single-node durable actor runtime with:

- Tokio-backed ActorRunner;
- durable mailbox;
- actor event log;
- actor snapshot and replay;
- AgentActor;
- ToolActor;
- MemoryActor;
- PolicyActor;
- SandboxActor;
- WASM tool sandbox;
- CapabilityBroker;
- causal tracing;
- local scheduler with quotas and backpressure;
- replay by `actor_id` and sequence range.

MVP excludes:

- distributed scheduler;
- Raft;
- microVM;
- process sandbox;
- custom executor;
- global actor migration;
- NUMA or kernel-bypass optimization.

## MVP Success Criteria

- An agent run can recover after process crash from event log and snapshot.
- Tool calls require capability approval.
- WASM tools cannot access unauthorized host resources.
- A committed side effect is not repeated during replay.
- Actor turns obey bounded budgets.
- Mailbox limits apply backpressure.
- Poison messages are isolated into a dead letter path.
- Causal tracing reconstructs user input to model call to tool call to memory write to final response.
- Replay can reconstruct actor state for a given actor sequence range.

## Testing Strategy

### Unit Tests

- actor state transitions;
- mailbox ordering and deduplication;
- capability checks;
- event encoding and decoding;
- snapshot apply and replay;
- poison message handling.

### Integration Tests

- full AgentActor to ToolActor to SandboxActor flow;
- crash after `ToolRequested` before `ToolCompleted`;
- crash after external tool completion before event commit;
- cancellation propagation from AgentActor to SandboxActor;
- mailbox backpressure under fanout;
- replay of a completed run.

### Property and Concurrency Tests

- event log append ordering;
- idempotency key behavior;
- actor single-turn exclusivity;
- cancellation race cases;
- mailbox visibility timeout behavior.

Use `loom` for small concurrent state machines where feasible. Use fuzzing for message decoding and event replay inputs.

## High-Risk Areas

- Side-effect replay can duplicate external actions if idempotency and reconciliation are weak.
- Actor messages can degrade into hidden synchronous RPC if request/response patterns are not controlled.
- Unbounded mailboxes can exhaust memory and hide backpressure problems.
- Prompt injection can become tool abuse if runtime authority is not separated from model output.
- Memory poisoning can corrupt future agent decisions.
- Message schema evolution can break long-running actors with old messages.
- Causal debugging will be impossible if message IDs, parent IDs, and actor turn IDs are not mandatory from the start.

## Evolution Roadmap

### MVP

- single-node runtime;
- durable actor turn protocol;
- WASM tools;
- capability broker;
- local scheduler;
- causal tracing;
- replay debugger.

### Production

- process sandbox with namespace, seccomp, and cgroup;
- persistent LSM/WAL storage;
- shard-level replication;
- policy engine integration;
- tenant quota and admission control;
- OpenTelemetry export;
- idempotent tool registry;
- dead letter and poison message operations;
- snapshot compaction.

### Planet Scale

- regional control planes;
- distributed actor directory;
- shard-level Raft groups;
- actor migration at turn boundaries;
- microVM pools;
- hierarchical scheduler;
- cross-region data residency;
- causal graph query engine;
- multi-region event log strategy;
- supply-chain verification pipeline.

## Key Architectural Decision

The system should start from durable actor semantics, not from task execution. Tokio is an execution substrate. The core product boundary is:

```text
durable actor kernel
+ capability-secured tool execution
+ replayable side-effect boundaries
+ sandboxed execution
+ causal observability
```

That boundary is the difference between a regular agent workflow service and a Rust Agent OS runtime.
