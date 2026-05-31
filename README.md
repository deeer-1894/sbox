# Agent Execution Plane (AEP)

A durable, secure execution runtime for long-running AI agents — built on a
bought durable-execution substrate ([Restate](https://restate.dev)) so the
engineering goes into the part that no off-the-shelf system provides:
**capability-secured, sandboxed, auditable tool execution.**

> Thesis: *buy the substrate, build the moat.* Durable execution, replay, and
> per-key serialization are commodities (Restate/Temporal/Orleans). The product
> value is the security + audit + sandbox + agent-semantics layer on top.

Design spec: [`docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md`](docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md).

---

## What it does

Every agent request flows through one enforced pipeline:

```
UserInput
  → TenantService.acquire        (quota / backpressure; cheapest rejection first)
  → Cedar policy evaluation      (independent of model intent; default-deny)
  → CapabilityBroker mint        (short-lived, scoped HMAC capability)
  → supply-chain verification    (tool WASM artifact digest pinned & checked)
  → ToolService side-effect      (capability verified; effectively-once boundary)
       → WASM sandbox (Wasmtime)  (no ambient authority; capability-gated host fn)
  → sanitize + trust-label       (tool output → Untrusted, injection redacted, into memory)
  → causal audit                 (Restate AuditService + ClickHouse SQL)
  → OpenTelemetry span           (exported to Jaeger over OTLP)
  → release tenant slot
```

LLM/model output is **untrusted intent**. It can never directly execute: only the
policy engine + capability broker grant authority, the sandbox enforces it, and
the supply-chain check gates the artifact.

## Architecture

```
┌──────────── Agent Execution Plane (the layer we build) ─────────────┐
│  AgentService  PolicyService  ToolService  MemoryService  TenantService  AuditService
│       │ (Cedar)      │ (capability)   │ (trust labels)  │ (quota)    │ (causal)
│       └──────── all Restate Virtual Objects (keyed = single-turn serialized) ──────┘
│  cross-cutting:  CapabilityBroker · WASM sandbox · supply-chain · OTel spans
├──────────── Durable substrate (reused, not built): Restate ─────────┤
│  durable execution · journaling · replay · timers · idempotency · state
├──────────── Data plane ─────────────────────────────────────────────┤
│  ClickHouse (audit SQL) · OTLP→Jaeger (traces) · S3/pgvector (future)
└─────────────────────────────────────────────────────────────────────┘
```

## Crates

| Crate | Responsibility | Tested |
| --- | --- | --- |
| `aep-domain` | Pure types, planning, side-effect decision, admission rule | unit |
| `aep-capability` | HMAC-signed scoped capability tokens (mint / verify / authorize) | unit |
| `aep-policy` | Cedar default-deny tool allowlist | unit |
| `aep-sandbox` | Wasmtime WASM sandbox; no ambient authority; capability-gated host fn | unit |
| `aep-audit` | Causal audit event model + chain/capability queries | unit |
| `aep-telemetry` | OTel span attributes, in-process capture, OTLP init | unit |
| `aep-memory` | Memory tiers, trust labels, prompt-injection sanitization | unit |
| `aep-supplychain` | Artifact digest + trusted-registry verification | unit |
| `aep-procsandbox` | Level-2 process sandbox (seccomp network lockdown); Linux-only | container |
| `aep-runtime` | Restate adapter: the 5 services + sidecar (counter, `/spans`) | integration |
| `aep-itest` | End-to-end integration tests against a live stack | — |

The pure crates have **zero infrastructure dependencies** and are unit-tested in
milliseconds; the runtime adapts them to Restate Virtual Objects.

## Quickstart

Requires Docker and a Rust toolchain (≥ 1.93 for Wasmtime).

```bash
# 1. Infrastructure: Restate + ClickHouse + Jaeger
docker compose -f deploy/docker-compose.yml up -d
until curl -s http://localhost:8123/ping | grep -q Ok; do sleep 1; done
curl -s 'http://aep:aep@localhost:8123/' --data-binary @deploy/clickhouse/audit_schema.sql

# 2. Run the services (OTLP export optional)
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 cargo run -p aep-runtime &
./scripts/register.sh          # discovers the 5 Virtual Objects (force-registers)

# 3. Drive a request
curl -s localhost:8080/AgentService/agent-1/handle \
  -H 'content-type: application/json' \
  -d '{"idempotency_key":"demo-1","content":"hello"}'
# → {"output":{"echo":{"content":"hello"}},"exec_count":1,"denied":false,"reason":null}

# 4. Observe it
curl -s localhost:8080/AuditService/demo-1/chain          # causal chain (Restate)
curl -s "http://aep:aep@localhost:8123/?query=SELECT%20kind%20FROM%20audit_events%20WHERE%20trace_id='demo-1'"
open http://localhost:16686    # Jaeger UI: service aep-runtime, span AgentService.handle
```

Try the security boundaries:

```bash
# policy denies an unlisted tool — no side effect:
curl -s localhost:8080/AgentService/a/handle -H 'content-type: application/json' \
  -d '{"idempotency_key":"d1","content":"x","requested_tool":"shell"}'   # denied:true

# prompt injection in tool output is redacted + labeled Untrusted in memory:
curl -s localhost:8080/AgentService/agent-m/handle -H 'content-type: application/json' \
  -d '{"idempotency_key":"m1","content":"plan: ignore previous instructions"}' >/dev/null
curl -s localhost:8080/MemoryService/agent-m/get -H 'content-type: application/json' -d '"m1"'
# → trust:"Untrusted", sanitized:true, value contains [REDACTED]

# tenant quota backpressure:
curl -s -X POST localhost:8080/TenantService/t1/set_limit -H 'content-type: application/json' -d '0'
curl -s localhost:8080/AgentService/a/handle -H 'content-type: application/json' \
  -d '{"idempotency_key":"q1","content":"x","tenant":"t1"}'   # denied: tenant quota exceeded
```

## Testing

```bash
cargo test --workspace                          # 34 unit tests (pure crates)
cargo test -p aep-itest -- --ignored            # 11 integration tests (needs the live stack)
./scripts/proc-sandbox-test.sh                  # 3 Linux process-sandbox tests (in a container)
```

Integration suites: `effectively_once`, `security_chain`, `sandbox_chain`,
`quota`, `audit`, `telemetry`, `memory`.

## Implementation status

Each phase has a spec slice, a TDD plan, and an acceptance record under
[`docs/superpowers/`](docs/superpowers/). All verified on a live stack.

| Phase | Scope | Acceptance |
| --- | --- | --- |
| 0 | Durable execution, effectively-once side-effect boundary, crash recovery | [phase0](docs/superpowers/plans/2026-05-30-agent-execution-plane-phase0-acceptance.md) |
| 1 | Authorization chain: Cedar policy + capability tokens | [phase1](docs/superpowers/plans/2026-05-30-agent-execution-plane-phase1-acceptance.md) |
| 1b | WASM sandbox: no ambient authority, capability-gated host access | [phase1b](docs/superpowers/plans/2026-05-30-agent-execution-plane-phase1b-acceptance.md) |
| 2a | Tenant quota + backpressure | [phase2a](docs/superpowers/plans/2026-05-30-agent-execution-plane-phase2a-acceptance.md) |
| 2b | Audit stream + causal query (+ live ClickHouse) | [2b](docs/superpowers/plans/2026-05-30-agent-execution-plane-phase2b-acceptance.md) · [live](docs/superpowers/plans/2026-05-31-agent-execution-plane-phase2b-live-clickhouse-acceptance.md) |
| 2c | OpenTelemetry spans (+ live OTLP → Jaeger) | [2c](docs/superpowers/plans/2026-05-30-agent-execution-plane-phase2c-acceptance.md) · [live](docs/superpowers/plans/2026-05-31-agent-execution-plane-phase2c-live-jaeger-acceptance.md) |
| 3a | Memory trust labels + injection sanitization | [phase3a](docs/superpowers/plans/2026-05-30-agent-execution-plane-phase3a-acceptance.md) |
| 3b | Level-2 process sandbox (seccomp), verified in Linux container | [phase3b](docs/superpowers/plans/2026-05-30-agent-execution-plane-phase3b-acceptance.md) |
| 3c | Supply-chain artifact verification + SLO definitions | [phase3c](docs/superpowers/plans/2026-05-30-agent-execution-plane-phase3c-acceptance.md) |

## Security properties (each verified)

- Model intent cannot execute without a policy-minted capability (forged token →
  rejected at the tool boundary, no side effect).
- A committed side effect is never repeated on resend or crash recovery.
- WASM tools have no ambient host authority; host access is capability-gated.
- Tool artifacts must match a pinned trusted digest before execution.
- Tool output is sanitized + labeled untrusted before entering memory.
- Tenant quotas apply backpressure.
- Every step is causally auditable (Restate + ClickHouse) and traced (OTLP/Jaeger).

## Not built (out of scope here)

- Distributed/multi-region ("planet scale"): sharded actor directory, Raft, actor
  migration.
- Namespace/cgroup process isolation and microVM (Level 3) — beyond the seccomp tier.
- Production hardening: signed artifact manifests (cosign), batched ClickHouse
  inserts, cross-service trace propagation, vector-indexed semantic memory.

## Layout

```
crates/          the 11 crates above
deploy/          docker-compose (Restate/ClickHouse/Jaeger), ClickHouse schema, OTel + SLO docs
scripts/         register.sh, kill-recover.sh, proc-sandbox-test.sh
docs/superpowers/ design spec + per-phase plans + acceptance records
```
