# Agent Execution Plane — Product Design

Date: 2026-05-30

## Status

**Implemented and verified** on a live stack (single-node). Every phase in the
roadmap below that is exercisable from a single host has been built test-first
and verified end-to-end against Restate — see the per-phase plans and acceptance
records in `docs/superpowers/plans/` and the project [`README`](../../../README.md).
Delivered: durable execution + effectively-once side effects (Phase 0), Cedar
policy + capability tokens + WASM sandbox (Phase 1/1b), tenant quota + ClickHouse
audit + OTLP→Jaeger traces (Phase 2), memory trust/sanitization + seccomp process
sandbox + supply-chain verification (Phase 3a/3b/3c). Not built: distributed
"planet scale" and microVM/namespace isolation (noted in the roadmap).

Approved design direction: **Agent Execution Plane on a bought durable substrate.**

This document supersedes the build-from-scratch direction in
`2026-05-28-rust-actor-os-runtime-design.md`. That earlier spec set the right
*conceptual* north star (durable actors, capability-secured tools, replayable
side-effect boundaries, sandboxing, causal observability), and the MVP code
proved the concepts. This revision re-scopes the work for a **product / internal
infrastructure** target rather than a concept proof.

The central change: **we do not build the durable-execution substrate
ourselves.** We buy/reuse it (Restate) and invest our engineering budget in the
layer that no existing system provides — **capability-secured, sandboxed,
auditable tool execution for AI agents.**

This is a design document, not an implementation plan. The first delivery target
is a self-hostable single-region product that proves durable agent execution,
policy-gated tool calls, WASM sandboxing, effectively-once side effects, and
causal audit.

## One-Line Thesis

> Buy the substrate (durable execution). Build the moat (capability + sandbox +
> audit + agent semantics).

Everything in the prior spec's "durable actor + replay + side-effect boundary +
single-turn serialization" maps one-to-one onto Restate Virtual Objects, which
are Rust-native and self-hostable. Re-implementing a Temporal-grade reliable
substrate is 6–18 months of systems work that produces **zero product
differentiation** — users never pay for "we wrote our own event log," they only
pay attention when it loses data. The differentiation, and therefore the build
budget, belongs in the security and audit layer.

## Goals

- Run long-running AI agents that survive process and node crashes with zero
  loss of committed state (RPO = 0).
- Guarantee **effectively-once** external side effects across crashes and
  replays.
- Enforce the chain *LLM output → policy evaluation → capability mint → sandbox
  execution* so that model output can never directly exercise privilege.
- Provide complete causal audit: which capability authorized which side effect,
  which tool output wrote which memory, why an agent is stuck.
- Support multi-tenancy with quotas, fairness, and multi-level backpressure.
- Be self-hostable in a single region, with clean seams for multi-region later.

## Non-Goals

- Do not build a durable-execution engine, event log, mailbox, scheduler, or
  checkpoint store. These are delegated to Restate.
- Do not build a custom async executor or serialize in-flight futures.
- Do not allow LLM output to directly execute privileged operations.
- Do not use Docker alone as the sandbox security boundary.
- Do not treat vector memory, snapshots, or materialized views as authoritative
  execution state.
- Do not implement multi-region, microVM isolation, or actor migration in the
  first delivery (designed for, not built).

## Build vs. Buy — The Core Decision

The prior MVP re-implemented, in memory, the overlapping cores of three mature
systems simultaneously:

| Prior "Actor OS" concept | Mature system that already solves it |
| --- | --- |
| Durable actor identity, dormant consumes nothing, wake on message | Virtual actors / grains (Orleans, Dapr Actors) |
| Event log + replay, crash recovery | Durable execution (Temporal, Restate, DBOS) |
| Side-effect boundary: skip replay if `ToolCompleted` exists | Idempotent, deterministic replay (Temporal Activities, Restate journaling) |
| WASM sandbox + capability-gated tool calls | Capability security + WASM components (wasmCloud, WASI P2) |
| Causal message graph, turn traces | Distributed tracing + event-sourced audit (OpenTelemetry + columnar store) |

**Decision rule (fixed, to avoid re-litigation):**

- **Default: Restate.** Rust core; Virtual Objects are keyed durable actors that
  serialize access per key (free single-turn-per-actor); lightweight per-invocation
  journaling avoids the workflow-history growth pain that hurts long-lived,
  many-turn agents; self-hostable.
- **Choose Temporal instead** only if the organization lacks Rust operations
  capability, is already heavily invested in JVM/.NET, and requires the longest
  production track record.
- **Self-build the substrate (Appendix A)** only if a hard constraint forces it:
  air-gap, licensing, or extreme scale demanding owned storage. Confirm the
  constraint is real before taking this path.

## Architectural Principles

The original five laws still hold; the difference is *who* implements each one.

```text
Everything important is an Actor.        -> Restate Virtual Object (per agent/tool/...)
Everything across boundaries is a Message. -> Restate durable messaging / awakeables
Everything durable is an Event.          -> Restate journal (truth) + audit stream (derived, append-only)
Everything privileged is a Capability.   -> WE BUILD: CapabilityBroker + Cedar policy
Everything replayable stops at a Side-effect Boundary. -> WE BUILD: tool invocation protocol over Restate journaling
```

## Reference Architecture

```text
┌──────────────────────── Control Plane (build, slow path) ────────────────────────┐
│  API Gateway (axum/gRPC) · OIDC auth · Tenant/Quota · Agent Registry · Policy Pub │
│  Store: Postgres                                                                   │
└─────────────────────────────────────┬─────────────────────────────────────────────┘
                                       │
┌──────────────── Agent Execution Plane (the differentiated layer we build) ─────────┐
│                                                                                    │
│   AgentService   PolicyService   ToolService   MemoryService   ModelService        │
│   (semantics)    (Cedar policy)  (side-effect)  (memory tiers)  (model gateway)     │
│        │              │              │              │              │                │
│        └──────── all implemented as Restate Virtual Objects (keyed = single turn) ──┘
│                                                                                    │
│   Cross-cutting:  CapabilityBroker  ·  SandboxRunner  ·  Causal/Audit Emitter       │
└─────────────────────────────────────┬─────────────────────────────────────────────┘
                                       │
┌──────────────── Durable Substrate (reuse Restate, do not build) ───────────────────┐
│  Durable execution · deterministic replay · journaling · durable timers/messaging  │
│  · idempotency · durable state                                                     │
└─────────────────────────────────────┬─────────────────────────────────────────────┘
                                       │
┌────────────────────────────────── Data Plane ─────────────────────────────────────┐
│  ClickHouse (audit/causal events) · S3/MinIO (blobs/tool artifacts)                │
│  · pgvector|Qdrant (semantic memory) · OTel → Tempo/Prometheus/Grafana             │
└────────────────────────────────────────────────────────────────────────────────────┘
```

## System Planes

### Control Plane

Owns slow-moving global state, never on the per-message hot path:

- tenant identity, quotas, budgets;
- user/service authentication (OIDC);
- policy bundle publication and versioning;
- agent registry and deployment config;
- billing and audit configuration.

Storage: **Postgres**.

### Agent Execution Plane

The product. Owns agent execution semantics and the security chain. Each actor
type is a stateless Rust service exposing one or more **Restate Virtual
Objects** keyed by actor id:

- `AgentService` — agent lifecycle, goals, planning, memory references,
  coordination.
- `PolicyService` — evaluates tool intents via Cedar; emits explainable
  Permit/Deny decisions and audit records.
- `ToolService` — owns the tool invocation side-effect boundary, idempotency,
  result classification and sanitization.
- `MemoryService` — episodic/semantic/operational memory with trust labels and
  contamination boundaries.
- `ModelService` — LLM provider calls, streaming, retries, budget, fallback;
  model calls are journaled side effects.
- `TenantService` — admission control, quota, fairness, backpressure.

Cross-cutting components used by the services:

- `CapabilityBroker` — mints short-lived, scoped capabilities.
- `SandboxRunner` — executes tool code in tiered isolation.
- `Audit Emitter` — dual-writes causal/audit events to ClickHouse and OTel.

### Data Plane

- **Restate journal** is the authoritative execution state.
- **ClickHouse** holds the immutable, append-only audit/causal event stream
  (derived from execution, queried for compliance and debugging).
- **S3/MinIO** holds blobs, tool outputs, artifacts, large payloads.
- **pgvector / Qdrant** holds semantic memory as a derived index.
- **OTel → Tempo/Prometheus/Grafana** holds operational traces and metrics.

Snapshots, vectors, and views are derived state. The journal is truth.

## Technology Selection

| Concern | Choice | Rationale |
| --- | --- | --- |
| Durable substrate | **Restate** (self-hosted) | Rust core; Virtual Object = keyed durable actor; light per-invocation journaling; good fit for long-lived multi-turn agents |
| Substrate alternative | Temporal (JVM/.NET orgs, max maturity); Golem (WASM-everything) | See decision rule |
| Policy engine | **Cedar** (`cedar-policy` crate) | Rust-native fine-grained authorization; policy-as-code; explainable, auditable decisions |
| Capability broker | In-house Rust | Short-lived scoped capability mint; part of the moat |
| Sandbox L1 | **Wasmtime + WASI Preview 2** component model | Capabilities injected via host functions; no ambient authority |
| Sandbox L2 | Process sandbox: Linux namespaces + seccomp + cgroups v2 (`youki`/`bubblewrap`) or gVisor | POSIX-heavy tools |
| Sandbox L3 (later) | Firecracker microVM | Browser/arbitrary user code |
| Control-plane store | Postgres | Tenants, quotas, registry, policy versions |
| Audit/causal store | **ClickHouse** | High-frequency writes + complex causal queries |
| Blob/artifact store | S3 / MinIO | Large payloads, files, reports |
| Semantic memory | pgvector (start) / Qdrant (scale) | Derived index, not authoritative |
| Model gateway | ModelService + LiteLLM/in-house proxy | Budget, rate limiting, provider fallback; calls journaled |
| Observability | OpenTelemetry → Tempo + Prometheus + Grafana | Standard stack; no bespoke causal-graph engine |
| Service framework | Rust + axum/tonic + Restate Rust SDK | Reuse existing Rust investment |

## Data and State Model

Authoritative state is Restate-managed Virtual Object state plus its journal. We
do not maintain a separate source-of-truth event log.

The audit/causal stream is an independent, immutable, append-only stream of facts
dual-written to ClickHouse. Every `AuditEvent` carries the full causal context
required by the prior spec:

```text
AuditEvent {
  tenant_id, agent_id, actor_type, run_id, turn_id, seq,
  message_id, causal_parent_id, trace_id,
  capability_id?, sandbox_id?, tool_invocation_id?,
  event_type, payload_hash, ts
}
```

Memory tiers (owned by `MemoryService`, each item carries a **trust label** and
the **source capability id** for provenance):

- working — current context window (transient);
- episodic — event-sourced interaction history;
- semantic — vector-indexed derived facts;
- operational — tool outputs, files, handles;
- policy — permission and approval decisions.

## Durable Turn Protocol (expressed in Restate)

Each actor type is a Virtual Object keyed by `actor_id`. Restate guarantees that
at most one invocation runs per key at a time — the single-turn-per-actor
exclusivity the prior spec required, obtained for free.

```rust
// AgentService as a Restate Virtual Object (illustrative)
#[restate::object]
impl AgentService {
    // Restate guarantees: serialized per agent_id; replayed from journal on
    // crash; all ctx.* calls are deterministic on replay (recorded results are
    // reused, side effects are not re-executed).
    async fn handle_message(&self, ctx: ObjectContext, msg: UserInput) -> Result<()> {
        // 1. Pure planning (deterministic, replayable).
        let intent = self.plan(&ctx, &msg).await?;

        // 2. Tool call = side-effect boundary via ToolService (idempotency key
        //    = invocation_id). Restate journaling makes this effectively-once.
        let result = ctx
            .object::<ToolService>(intent.tool_actor)
            .run(intent, idempotency_key)
            .await?;

        // 3. Write memory with trust label + source capability.
        ctx.object::<MemoryService>(self.memory_id)
            .store(result.sanitized, trust_label, result.capability_id)
            .await?;
        Ok(())
    }
}
```

### Determinism Requirement (mandatory)

Actor logic must be a pure function of `(state, inbox, journaled_effects)`. All
nondeterminism — time, randomness, tool results, model output — must flow
through the Restate context (`ctx.rand()`, `ctx.time()`, `ctx.run(...)`) so it is
journaled and replayed, never re-executed.

This is the single most important correction over the prior MVP, whose actor
logic called `Uuid::new_v4()` and `OffsetDateTime::now_utc()` directly. Under
non-deterministic logic, replay produces different results and the side-effect
boundary is meaningless. Determinism is a non-negotiable invariant here.

## Tool Invocation Side-Effect Protocol (the product's core)

This is the one part that cannot be outsourced and must be exactly right.

```text
AgentService emits ToolIntent          (LLM output = untrusted intent)
        │
        ▼
PolicyService.evaluate(intent)          Cedar decision
        │  inputs: agent identity, tenant policy, resource, tool trust level,
        │          sandbox level, memory trust labels, risk score, compliance
        ▼  output: Permit/Deny + explanation + audit record
CapabilityBroker.mint(scope, ttl)       short-lived scoped capability
        │  Capability{ subject, resource, actions, scope, ttl, policy_hash, audit_id }
        ▼
ToolService.run(invocation_id):         side-effect boundary
        ├─ 1. journal ToolRequested{ invocation_id, idempotency_key, input_hash, capability_ref }
        ├─ 2. SandboxRunner.execute(capability)   capability is the sole authority
        ├─ 3. classify + sanitize result          (anti-injection / anti-poisoning) -> trust label
        └─ 4. journal ToolCompleted{ invocation_id, output_ref, external_ref, output_hash }
        ▼
AgentService consumes the sanitized result
```

### Replay and Reconciliation Rules

- If `ToolCompleted` exists, reuse the recorded result; do not re-execute.
- If `ToolRequested` exists without `ToolCompleted` (crash mid-flight), use the
  `external_reference` to reconcile with the external system *first*, then decide.
- Retry only when the operation is proven idempotent.
- Never replay a side effect merely because the process crashed.

Restate journaling provides the mechanism for the first two steps. The **external
reconciliation logic** and the **result classification / trust labeling** are our
business code — and they are the moat.

### Security Invariants (hard requirements)

- No ambient host authority.
- Plugins cannot directly access secrets, network, filesystem, or shell.
- The model cannot grant authority — only PolicyService + CapabilityBroker can.
- Every capability use is audited.
- Tool output is classified and sanitized before entering agent memory.
- Production plugins require supply-chain verification before execution.

## Capability and Policy Model

Capabilities are explicit, scoped, short-lived authorities:

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

Policy evaluation runs in `PolicyService` via **Cedar** policies (policy-as-code,
versioned and published from the control plane). Inputs: actor identity, tenant
policy, user consent, requested resource, tool trust level, sandbox level, memory
trust labels, risk score, historical behavior, regional/compliance constraints.
Cedar's explainable decisions feed directly into the audit stream.

## Sandbox and Isolation

Tiered, default-deny:

```text
Level 0: trusted in-process actor
Level 1: WASM sandbox (Wasmtime + WASI Preview 2)        <- first delivery
Level 2: process sandbox (namespaces/seccomp/cgroups v2) <- first delivery (POSIX tools)
Level 3: microVM (Firecracker)                            <- later
Level 4: hostile-tenant isolation (dedicated VM/network/secret domain) <- later
```

Default mapping: plugins and normal tools → WASM; POSIX-heavy tools → process
sandbox; browser/shell/arbitrary code → microVM later. Sandboxes run on isolated
node pools, partitioned per tenant.

## Multi-Tenancy, Quota, Backpressure

- **Isolation:** Restate isolates per key; sandboxes are pooled per tenant; L4
  tenants get dedicated VM/network/secret domains.
- **Quota:** `TenantService` (a Virtual Object) tracks `max_concurrent`, budget,
  and token quotas; model and tool calls debit quota before executing.
- **Backpressure (multi-level):** actor mailbox (Restate), tenant runnable queue,
  sandbox pool, model provider, external tool provider. When any level is full,
  reject with an explicit error rather than buffering unboundedly.
- **Fairness:** per-tenant fair scheduling; coalesce wakeups to avoid runnable
  storms.

## Observability and Audit

- **Operational:** OpenTelemetry traces with `CausalMeta` fields as span
  attributes, exported to Tempo; metrics to Prometheus; structured logs.
- **Audit/causal:** ClickHouse plus a query API that answers the runtime-debugger
  questions: why is this agent stuck; which mailbox is blocked; which capability
  authorized this side effect; which tool output wrote this memory; which tenant
  exhausted sandbox capacity; which poison message caused a retry loop.
- **Replay debugger:** Restate journal + ClickHouse audit reconstruct actor state
  for an `agent_id` and sequence range.

## Deployment Topology

```text
            ┌── API Gateway (axum) ──┐
Clients ───▶│  OIDC, rate limit, route│
            └────────────┬───────────┘
                         ▼
        ┌──── Rust service cluster (k8s) ────┐
        │ AgentSvc PolicySvc ToolSvc          │  stateless, scale horizontally
        │ MemorySvc ModelSvc TenantSvc        │
        └──────┬───────────────┬──────────────┘
               │               │
        ┌──────▼───┐    ┌──────▼─────────────┐
        │ Restate  │    │ Sandbox node pool   │  isolated, separate nodes
        │ (HA)     │    │ Wasmtime/process/VM │
        └──────────┘    └─────────────────────┘
               │
   ┌───────────┼─────────────┬──────────────┐
 Postgres  ClickHouse    S3/MinIO      Qdrant/pgvector
```

Restate provides persistence and HA; services are stateless and roll forward;
sandbox nodes are physically and network isolated.

## Migration From the MVP

The MVP code has been removed. When reconstituted, components map as follows:

| Prior crate | Disposition | Note |
| --- | --- | --- |
| `api-types` | Keep | Solid type design; shared library |
| `capability` | Keep + enhance | The moat; policy evaluation moves to Cedar |
| `sandbox` | Rewrite | `FakeSandbox` → Wasmtime/WASI P2 + process tier |
| `agent-core` | Keep logic, change execution | Reuse actor business logic as Restate services; remove `now()`/`uuid()` nondeterminism |
| `observability` | Keep + extend | `fields_for` → OTel span attributes + ClickHouse schema |
| `event-log` | Delete | Restate journaling replaces it |
| `actor-mailbox` | Delete (all four impls) | Restate durable messaging replaces it; the channel impl also dropped dedup/DLQ — a latent bug |
| `actor-kernel` | Delete | Restate turn model replaces it |
| `actor-scheduler` | Reshape into `TenantService` | Scheduling to Restate; keep quota/fairness/backpressure semantics |
| `checkpoint` | Delete | Restate state persistence replaces it |
| `actor-cli` | Keep as ops/debug CLI | Re-point at the control-plane API |

Net effect: delete roughly half the code (the in-memory re-implementations of a
substrate), keep all differentiated logic.

## Phased Roadmap and Success Criteria

### Phase 0 — Substrate validation (2–3 weeks)

Stand up Restate; run one `AgentService` + `ToolService` with a tool call through
the idempotent side-effect boundary.

- Success: kill the process and the agent recovers from the journal; a committed
  `ToolCompleted` side effect is not re-executed on replay.

### Phase 1 — Security chain (4–6 weeks)

Wire `PolicyService` (Cedar) + `CapabilityBroker` + Wasmtime sandbox.

- Success: tool calls require policy approval; WASM tools cannot access
  unauthorized host resources; LLM output cannot directly execute privileged
  operations.

### Phase 2 — Multi-tenancy + audit (4–6 weeks)

`TenantService` quota/backpressure; ClickHouse audit + causal query API; OTel.

- Success: tenant quotas apply backpressure; the causal chain (user input → model
  → tool → memory write → response) is fully reconstructable; poison messages go
  to a dead-letter path without infinite retries.

### Phase 3 — Memory + process sandbox + GA hardening (6–8 weeks)

`MemoryService` tiers + trust labels + sanitization; process-sandbox tier;
supply-chain verification; SLO dashboards.

- Success: tool output is classified and sanitized before entering memory;
  hostile-tenant isolation holds; RPO = 0 for committed events; effectively-once
  side effects; defined recovery-time SLO.

## High-Risk Areas and Mitigations

| Risk | Mitigation in this design |
| --- | --- |
| Side-effect replay duplicates external actions | Restate journaling + idempotency key + external reconciliation logic |
| Actor messaging degrades into hidden synchronous RPC | Stay message-driven; use Restate awakeables for request/response without blocking the turn |
| Unbounded mailboxes exhaust memory | Restate bounded messaging + multi-level backpressure |
| Prompt injection becomes tool abuse | Runtime authority physically separated from model output: only Policy + Capability can authorize |
| Memory poisoning corrupts future decisions | Memory trust labels + tool-output sanitization + source-capability provenance |
| Schema evolution breaks long-running actors | `schema_version` on messages/events; Restate state migration |
| Causal debugging impossible | `message_id`/`causal_parent_id`/`turn_id` mandatory from day one |

## Key Architectural Decision

The product boundary is:

```text
capability-secured tool execution
+ sandboxed execution
+ replayable side-effect boundaries (on a bought substrate)
+ causal audit
+ agent semantics
```

The durable-execution substrate is a commodity to be reused, not a product to be
built. Building it confers no advantage; getting the security and audit layer
right is the entire difference between a generic agent workflow service and a
trustworthy Agent Execution Plane.

## Appendix A — Self-Built Substrate (fallback only)

Taken only if a hard constraint (air-gap, licensing, owned-storage scale) rules
out Restate/Temporal. This is an independent 6–18 month systems project:

- Event log + snapshots on Postgres or an embedded LSM/WAL (`fjall`/`redb`).
- Single-writer-per-actor via advisory locks or a sharded owner.
- A deterministic replay engine where actor logic is strictly
  `f(state, inbox, journaled_effects)`.
- A first-class side-effect journal with idempotency keys and external
  reconciliation.
- Coalesced wakeups and dedup of runnable actor ids.

Confirm the constraint is genuine before committing to this path.
