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

/// Install an OTLP span exporter + tracing bridge pointing at `endpoint`
/// (e.g. http://localhost:4317). Call once at startup. Real OpenTelemetry export.
pub fn init_otel(endpoint: &str) {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry::KeyValue;
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::runtime;
    use opentelemetry_sdk::trace::TracerProvider;
    use opentelemetry_sdk::Resource;
    use tracing_subscriber::prelude::*;

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
        .expect("build OTLP exporter");
    let provider = TracerProvider::builder()
        .with_resource(Resource::new(vec![KeyValue::new("service.name", "aep-runtime")]))
        .with_batch_exporter(exporter, runtime::Tokio)
        .build();
    let tracer = provider.tracer("aep-runtime");
    opentelemetry::global::set_tracer_provider(provider);

    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .try_init();
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
