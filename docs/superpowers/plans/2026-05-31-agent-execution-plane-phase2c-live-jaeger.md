# Agent Execution Plane — Phase 2c-live (Jaeger / OTLP Export) Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Checkbox steps.

**Goal:** Export real OpenTelemetry spans from the runtime to Jaeger over OTLP — closing the "OTLP live export deferred" note from Phase 2c.

**Architecture:** Jaeger all-in-one (OTLP ingest on 4317) runs in Docker. `aep-telemetry::init_otel` builds an OTLP span exporter + a `tracing-opentelemetry` layer, gated on `OTEL_EXPORTER_OTLP_ENDPOINT`. `AgentService.handle` is wrapped in a real `tracing` span, so each request produces a span exported to Jaeger. Verified via Jaeger's HTTP trace API.

**Tech Stack:** Rust (`opentelemetry` 0.27, `opentelemetry_sdk` 0.27 `rt-tokio`, `opentelemetry-otlp` 0.27 `grpc-tonic`, `tracing-opentelemetry` 0.28, `tracing-subscriber`), Jaeger `jaegertracing/all-in-one:latest`. Builds on Phase 2c.

---

## Task 1: Jaeger service

- [ ] **Step 1:** Append to `deploy/docker-compose.yml`:

```yaml
  jaeger:
    image: jaegertracing/all-in-one:latest
    environment:
      COLLECTOR_OTLP_ENABLED: "true"
    ports:
      - "16686:16686"  # UI / trace query API
      - "4317:4317"    # OTLP gRPC ingest
```

- [ ] **Step 2:** `docker compose -f deploy/docker-compose.yml up -d jaeger`; wait for `curl -s localhost:16686` to return HTML.

---

## Task 2: OTel init in aep-telemetry

- [ ] **Step 1:** Add to `crates/aep-telemetry/Cargo.toml`:

```toml
[dependencies]
opentelemetry = "0.27"
opentelemetry_sdk = { version = "0.27", features = ["rt-tokio"] }
opentelemetry-otlp = { version = "0.27", features = ["grpc-tonic"] }
tracing-opentelemetry = "0.28"
tracing-subscriber = "0.3"
```

- [ ] **Step 2:** Add `init_otel` to `crates/aep-telemetry/src/lib.rs`:

```rust
/// Install an OTLP span exporter + tracing bridge pointing at `endpoint`
/// (e.g. http://localhost:4317). Call once at startup.
pub fn init_otel(endpoint: &str) {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use opentelemetry_sdk::Resource;
    use tracing_subscriber::prelude::*;

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
        .expect("build OTLP exporter");
    let provider = SdkTracerProvider::builder()
        .with_resource(Resource::builder().with_service_name("aep-runtime").build())
        .with_batch_exporter(exporter)
        .build();
    let tracer = provider.tracer("aep-runtime");
    opentelemetry::global::set_tracer_provider(provider);

    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .try_init();
}
```

> Align crate versions during execution if cargo cannot resolve the set above.

- [ ] **Step 3:** Build `cargo build -p aep-telemetry`.

---

## Task 3: Span + startup gate

- [ ] **Step 1:** In `crates/aep-runtime/src/main.rs`, before serving, add:

```rust
    if let Ok(endpoint) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        aep_telemetry::init_otel(&endpoint);
    }
```

- [ ] **Step 2:** In `crates/aep-runtime/src/lib.rs`, `AgentService::handle`, wrap `handle_inner` in a span. Add `use tracing::Instrument;` near the top of `mod agent`, and change the call to:

```rust
            let outcome = handle_inner(&ctx, &input)
                .instrument(tracing::info_span!(
                    "AgentService.handle", trace_id = %input.idempotency_key, actor = "AgentService"
                ))
                .await;
```

- [ ] **Step 3:** Build `cargo build -p aep-runtime`.

---

## Task 4: Verify in Jaeger

- [ ] **Step 1:** Run the runtime with the endpoint set:

```bash
pkill -f 'target/debug/aep-runtime'
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 cargo run -p aep-runtime &
./scripts/register.sh
for i in 1 2 3; do
  curl -s localhost:8080/AgentService/agent-jaeger/handle -H 'content-type: application/json' \
    -d "{\"idempotency_key\":\"jg-$i-$(uuidgen)\",\"content\":\"hi\"}" >/dev/null
done
```

- [ ] **Step 2:** Query Jaeger (spans batch-export; poll a few seconds):

```bash
for i in $(seq 1 15); do
  n=$(curl -s "http://localhost:16686/api/traces?service=aep-runtime&limit=20" | grep -o '"traceID"' | wc -l)
  [ "$n" -gt 0 ] && { echo "traces in jaeger: $n"; break; }
  sleep 2
done
curl -s "http://localhost:16686/api/services" | grep aep-runtime
```
Expected: `aep-runtime` appears in services; trace count > 0.

- [ ] **Step 3:** Acceptance doc + commit + push.

---

## Notes
- OTLP export is gated on the env var, so the default/test path is unchanged
  (Phase 2c `/spans` capture still verifies instrumentation without a collector).
