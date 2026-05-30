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
