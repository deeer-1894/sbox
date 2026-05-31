# Phase 2c-live (Jaeger / OTLP Export) Acceptance — Agent Execution Plane

Closes the "OTLP live export deferred" note from Phase 2c acceptance.

Verified 2026-05-31. Jaeger `jaegertracing/all-in-one:latest` (OTLP gRPC 4317,
UI/API 16686). Real OpenTelemetry SDK: `opentelemetry`/`opentelemetry_sdk` 0.27.1,
`opentelemetry-otlp` 0.27.0, `tracing-opentelemetry` 0.28.0.

| Criterion | Evidence | Status |
| --- | --- | --- |
| Runtime exports spans over OTLP | with `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317`, 3 requests → Jaeger lists service `aep-runtime` with traces | [x] |
| Our actor-turn span is exported | Jaeger `operations?service=aep-runtime` includes `AgentService.handle`; trace fetchable by that operation | [x] |
| Export is gated, default path unchanged | OTLP only installs when the env var is set; the Phase 2c `/spans` capture still verifies instrumentation without a collector | [x] |
| No regression | runtime builds + serves; integration suite unaffected | [x] |

## OTel 0.27 API notes (folded into the plan)

- Provider is `opentelemetry_sdk::trace::TracerProvider` (not `SdkTracerProvider`).
- `Resource::new(vec![KeyValue::new("service.name", ...)])` (no `builder()` yet).
- `with_endpoint` comes from the `opentelemetry_otlp::WithExportConfig` trait.
- `with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)` (two args).

## Reproduce

```bash
docker compose -f deploy/docker-compose.yml up -d jaeger
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 cargo run -p aep-runtime &
./scripts/register.sh
curl -s localhost:8080/AgentService/a/handle -H 'content-type: application/json' \
  -d '{"idempotency_key":"jg-1","content":"hi"}' >/dev/null
curl -s 'http://localhost:16686/api/operations?service=aep-runtime' | grep AgentService.handle
# Jaeger UI: http://localhost:16686
```

## Production follow-ups

Propagate trace context across Restate object calls (parent/child spans for
Tool/Audit/Memory turns); span durations + attributes for the SLO metrics;
tail-based sampling at the collector.
