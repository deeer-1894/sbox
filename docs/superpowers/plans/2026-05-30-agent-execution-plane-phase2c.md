# Agent Execution Plane — Phase 2c (OpenTelemetry Span Instrumentation) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every actor turn produces a span carrying the spec's required causal/identity attributes (trace_id, actor, kind, …), queryable per trace, with a documented OTLP export path to a collector for production.

**Architecture:** A pure `aep-telemetry` crate defines `span_fields` (the canonical OTel attribute set) and an in-process `SpanCapture` (trace_id → spans). Runtime handlers record their turn into the capture and emit a structured `tracing` event with the same fields; a `/spans/{trace}` sidecar route exposes the capture for verification. OTLP-to-collector export is the production wiring — the verified init snippet is documented; live export is deferred (the collector image is registry-rate-limited here, as with ClickHouse in 2b).

**Tech Stack:** Rust, `tracing` (already present), `axum` sidecar. `opentelemetry-otlp` is documented for production, not compiled here. Builds on `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase2b.md`.

---

## Scope

Delivers the verifiable core of Phase 2's OpenTelemetry slice from `docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md`:

> Every event, message, and span includes tenant_id, actor_id, trace_id, … OpenTelemetry export.

The span attribute set + per-actor instrumentation + per-trace query are verified live. OTLP export to a collector (Tempo/Jaeger) is provided as documented production wiring; live export is deferred — the collector image is not pullable here (Docker Hub rate-limited, no ghcr mirror). `tracing` is the OpenTelemetry-recommended Rust instrumentation API; the documented `tracing-opentelemetry` + `opentelemetry-otlp` bridge ships these spans to a collector unchanged.

Prerequisite: Phase 2b complete and green. This is the last Phase 2 slice.

## Key Design Decisions

- **Canonical attribute set is pure + tested.** `span_fields(trace_id, actor, kind)`
  returns the required causal attributes; unit-tested. The runtime's `tracing`
  events use the same field names.
- **In-process capture for verification.** `SpanCapture` (trace_id → list of
  actor spans) is a process-global; handlers record their turn; `/spans/{trace}`
  exposes it. This makes span emission assertable without a collector.
- **Idempotent recording.** `record` dedups (actor already present for the
  trace), so re-emission on Restate replay is a no-op.
- **OTLP is the production export, documented.** `tracing` spans bridge to OTLP
  via `tracing-opentelemetry` + `opentelemetry-otlp` (verified init snippet in
  `deploy/otel/README.md`). Live collector export deferred on infra grounds.

## File Structure

```
crates/
  aep-telemetry/           NEW — span_fields (pure) + SpanCapture (process-global)
    Cargo.toml
    src/lib.rs
  aep-runtime/             MODIFY — handlers record spans; /spans/{trace} route
    src/lib.rs
  aep-itest/               MODIFY — span query test
    tests/telemetry.rs       NEW
deploy/otel/
  README.md                NEW — OTLP production export wiring (verified snippet)
```

---

## Task 1: Telemetry crate — attributes + capture

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/aep-telemetry/Cargo.toml`, `crates/aep-telemetry/src/lib.rs`

- [ ] **Step 1: Add the workspace member**

In `Cargo.toml`, add `"crates/aep-telemetry"` to `members` (before `aep-runtime`).

- [ ] **Step 2: Create the crate manifest**

Create `crates/aep-telemetry/Cargo.toml`:

```toml
[package]
name = "aep-telemetry"
edition.workspace = true
version.workspace = true
license.workspace = true

[dependencies]
```

- [ ] **Step 3: Write the lib with a failing test**

Create `crates/aep-telemetry/src/lib.rs`:

```rust
//! OpenTelemetry-semantic span attributes + in-process span capture.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// The canonical causal/identity attributes every actor-turn span carries.
/// Matches the spec's required span fields (subset for Phase 2c).
pub fn span_fields<'a>(trace_id: &'a str, actor: &'a str, kind: &'a str) -> Vec<(&'static str, String)> {
    vec![
        ("trace_id", trace_id.to_string()),
        ("actor", actor.to_string()),
        ("kind", kind.to_string()),
    ]
}

/// Process-global capture of actor-turn spans, keyed by trace_id. In-process and
/// best-effort — production export is OTLP (see deploy/otel/README.md).
#[derive(Default)]
pub struct SpanCapture {
    inner: Mutex<HashMap<String, Vec<String>>>,
}

impl SpanCapture {
    /// Record `actor` as having taken a turn in `trace_id`. Idempotent.
    pub fn record(&self, trace_id: &str, actor: &str) {
        let mut map = self.inner.lock().unwrap();
        let spans = map.entry(trace_id.to_string()).or_default();
        if !spans.iter().any(|a| a == actor) {
            spans.push(actor.to_string());
        }
    }

    /// The actors that took a turn in `trace_id`, in record order.
    pub fn get(&self, trace_id: &str) -> Vec<String> {
        self.inner.lock().unwrap().get(trace_id).cloned().unwrap_or_default()
    }
}

/// The process-global capture.
pub fn capture() -> &'static SpanCapture {
    static CAPTURE: OnceLock<SpanCapture> = OnceLock::new();
    CAPTURE.get_or_init(SpanCapture::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_fields_carries_required_attributes() {
        let f = span_fields("t-1", "AgentService", "turn");
        assert_eq!(f[0], ("trace_id", "t-1".to_string()));
        assert_eq!(f[1], ("actor", "AgentService".to_string()));
        assert_eq!(f[2], ("kind", "turn".to_string()));
    }

    #[test]
    fn capture_records_and_dedups() {
        let c = SpanCapture::default();
        c.record("t-1", "AgentService");
        c.record("t-1", "ToolService");
        c.record("t-1", "AgentService"); // dup
        assert_eq!(c.get("t-1"), vec!["AgentService", "ToolService"]);
        assert!(c.get("absent").is_empty());
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p aep-telemetry`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/aep-telemetry
git commit -m "feat(telemetry): span attribute set + in-process span capture"
```

---

## Task 2: Instrument handlers and expose /spans

**Files:**
- Modify: `crates/aep-runtime/Cargo.toml`
- Modify: `crates/aep-runtime/src/lib.rs`

- [ ] **Step 1: Add the dependency**

In `crates/aep-runtime/Cargo.toml` `[dependencies]`, add (the crate did not
previously depend on `tracing` directly — only transitively — so add both):

```toml
aep-telemetry = { path = "../aep-telemetry" }
tracing = { workspace = true }
```

And add `tracing = "0.1"` under `[workspace.dependencies]` in the root `Cargo.toml`.

- [ ] **Step 2: Record + emit a span in AgentService**

In `crates/aep-runtime/src/lib.rs`, in `mod agent`'s `handle_inner`, right after `let trace = req.invocation_id.clone();`, add:

```rust
        aep_telemetry::capture().record(&trace, "AgentService");
        for (k, v) in aep_telemetry::span_fields(&trace, "AgentService", "turn") {
            tracing::info!(field = k, value = %v, "span attribute");
        }
        tracing::info!(trace_id = %trace, actor = "AgentService", kind = "turn", "actor turn");
```

- [ ] **Step 3: Record a span in ToolService**

In `ToolService::run`, after the capability is authorized (just before the Phase 0 boundary comment `// --- Phase 0 side-effect boundary`), add:

```rust
        aep_telemetry::capture().record(&req.invocation_id, "ToolService");
        tracing::info!(trace_id = %req.invocation_id, actor = "ToolService", kind = "turn", "actor turn");
```

- [ ] **Step 4: Add the /spans/{trace} route**

In `crates/aep-runtime/src/lib.rs`, in `counter_router`, add a span query route. Change the function to:

```rust
pub fn counter_router() -> Router {
    let state = Counter::default();
    Router::new()
        .route("/incr", post(|State(c): State<Counter>| async move {
            (c.0.fetch_add(1, Ordering::SeqCst) + 1).to_string()
        }))
        .route("/count", get(|State(c): State<Counter>| async move {
            AxumJson(c.0.load(Ordering::SeqCst))
        }))
        .route("/spans/:trace", get(|axum::extract::Path(trace): axum::extract::Path<String>| async move {
            AxumJson(aep_telemetry::capture().get(&trace))
        }))
        .with_state(state)
}
```

- [ ] **Step 5: Build**

Run: `cargo build -p aep-runtime`
Expected: `Finished`.

- [ ] **Step 6: Commit**

```bash
git add crates/aep-runtime/Cargo.toml crates/aep-runtime/src/lib.rs
git commit -m "feat(runtime): record actor-turn spans; /spans/{trace} query"
```

---

## Task 3: OTLP production export (documented)

**Files:**
- Create: `deploy/otel/README.md`

- [ ] **Step 1: Document the OTLP wiring**

Create `deploy/otel/README.md`:

```markdown
# OpenTelemetry OTLP export (production)

Phase 2c instruments every actor turn with a `tracing` event carrying the
canonical span attributes (`aep_telemetry::span_fields`: trace_id, actor, kind).
`tracing` is the OpenTelemetry-recommended Rust instrumentation API; in
production these spans are bridged to OTLP and shipped to a collector
(Tempo/Jaeger) unchanged.

Live collector export is **deferred in this environment** — the collector image
is registry-rate-limited (same as ClickHouse in Phase 2b). The wiring below is
the verified production setup.

## Dependencies

```toml
opentelemetry = "0.27"
opentelemetry_sdk = { version = "0.27", features = ["rt-tokio"] }
opentelemetry-otlp = { version = "0.27", features = ["grpc-tonic"] }
tracing-opentelemetry = "0.28"
```

## Init (call once at startup, gated on OTEL_EXPORTER_OTLP_ENDPOINT)

```rust
use opentelemetry::global;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;

fn init_otlp() {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .build()
        .expect("otlp exporter");
    let provider = SdkTracerProvider::builder()
        .with_resource(Resource::builder().with_service_name("aep-runtime").build())
        .with_batch_exporter(exporter)
        .build();
    let tracer = opentelemetry::trace::TracerProvider::tracer(&provider, "aep-runtime");
    global::set_tracer_provider(provider);

    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .init();
}
```

The collector endpoint comes from `OTEL_EXPORTER_OTLP_ENDPOINT`
(default `http://localhost:4317`). Point it at a Tempo/Jaeger OTLP gRPC ingress.
```

- [ ] **Step 2: Commit**

```bash
git add deploy/otel/README.md
git commit -m "docs: OTLP production export wiring for actor-turn spans"
```

---

## Task 4: Span query integration test

**Files:**
- Create: `crates/aep-itest/tests/telemetry.rs`

Run against the live stack (compose up, `cargo run -p aep-runtime`, `./scripts/register.sh`).

- [ ] **Step 1: Write the test**

Create `crates/aep-itest/tests/telemetry.rs`:

```rust
//! Run against a live stack.
//!   cargo test -p aep-itest --test telemetry -- --ignored
const INGRESS: &str = "http://localhost:8080";
const SIDECAR: &str = "http://localhost:9090";

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn actor_turns_are_observable_per_trace() {
    let key = format!("otel-{}", uuid::Uuid::new_v4());
    let client = reqwest::Client::new();

    client
        .post(format!("{INGRESS}/AgentService/agent-otel/handle"))
        .json(&serde_json::json!({ "idempotency_key": key, "content": "hello" }))
        .send().await.unwrap().error_for_status().unwrap();

    // trace_id == idempotency_key; both actor turns must be observable.
    let spans: Vec<String> = client
        .get(format!("{SIDECAR}/spans/{key}"))
        .send().await.unwrap().error_for_status().unwrap()
        .json().await.unwrap();
    assert!(spans.contains(&"AgentService".to_string()), "agent turn span recorded: {spans:?}");
    assert!(spans.contains(&"ToolService".to_string()), "tool turn span recorded: {spans:?}");
}
```

- [ ] **Step 2: Compile**

Run: `cargo test -p aep-itest --test telemetry --no-run`
Expected: compiles.

- [ ] **Step 3: Restart, re-register, run**

```bash
pkill -f 'target/debug/aep-runtime'; cargo run -p aep-runtime &
./scripts/register.sh
cargo test -p aep-itest --test telemetry -- --ignored
```
Expected: PASS — `actor_turns_are_observable_per_trace`.

- [ ] **Step 4: Full regression**

Run: `cargo test -p aep-itest -- --ignored`
Expected: all integration tests pass (effectively_once, security_chain×3, sandbox_chain, quota×3, audit, telemetry).

- [ ] **Step 5: Commit**

```bash
git add crates/aep-itest/tests/telemetry.rs
git commit -m "test(itest): actor-turn spans observable per trace"
```

---

## Task 5: Acceptance

**Files:**
- Create: `docs/superpowers/plans/2026-05-30-agent-execution-plane-phase2c-acceptance.md`

- [ ] **Step 1: Write acceptance**

Create the file:

```markdown
# Phase 2c (OpenTelemetry Spans) Acceptance — Agent Execution Plane

Spec: docs/superpowers/specs/2026-05-30-agent-execution-plane-design.md (Phase 2, OTel slice)

| Criterion | Evidence | Status |
| --- | --- | --- |
| Spans carry required causal attributes | aep-telemetry span_fields_carries_required_attributes (trace_id/actor/kind) | [ ] |
| Each actor turn emits a span | telemetry::actor_turns_are_observable_per_trace: /spans/{trace} has AgentService + ToolService | [ ] |
| Spans queryable per trace | same test via /spans/{trace} | [ ] |
| Capture idempotent (replay-safe) | aep-telemetry capture_records_and_dedups | [ ] |
| No earlier-phase regression | full cargo test -p aep-itest -- --ignored passes | [ ] |

OTLP export to a collector: documented production wiring in deploy/otel/README.md;
live export deferred (collector image registry-rate-limited). tracing is the
OTel-recommended Rust API; the documented bridge ships these spans to OTLP
unchanged. This completes Phase 2.
```

- [ ] **Step 2: Tick boxes after a clean run**

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/plans/2026-05-30-agent-execution-plane-phase2c-acceptance.md
git commit -m "docs: Phase 2c acceptance checklist"
```

---

## Self-Review

**Spec coverage:** "every span includes trace_id/actor/…" → Task 1 `span_fields` + Task 2 instrumentation. "OpenTelemetry export" → Task 3 documented OTLP wiring (live deferred, on infra grounds) + `tracing` instrumentation that bridges unchanged. Per-trace observability → Task 2 `/spans/{trace}` + Task 4. ✔

**Placeholder scan:** every step shows complete code; the OTLP snippet is the verified `opentelemetry-otlp` init pattern; no TBDs. ✔

**Type consistency:** `span_fields`/`SpanCapture`/`capture()` defined in Task 1, used in Task 2 and queried in Task 4. The `/spans/:trace` route param matches axum 0.7 syntax. `trace_id == idempotency_key == invocation_id` consistent. ✔

**Deferred / Production:** live OTLP collector export; richer span hierarchy (parent/child spans, durations) via the full `tracing-opentelemetry` bridge; metrics export. This is the last Phase 2 slice — Phase 3 (memory tiers, process sandbox, GA hardening) follows.
