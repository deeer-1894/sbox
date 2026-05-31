use aep_runtime::{
    counter_router, AgentService, AgentServiceImpl, AuditService, AuditServiceImpl,
    MemoryService, MemoryServiceImpl, TenantService, TenantServiceImpl, ToolService,
    ToolServiceImpl,
};
use restate_sdk::prelude::*;

#[tokio::main]
async fn main() {
    // Real OpenTelemetry OTLP export when an endpoint is configured.
    if let Ok(endpoint) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        aep_telemetry::init_otel(&endpoint);
    }

    // External counter sidecar on :9090.
    tokio::spawn(async {
        let listener = tokio::net::TcpListener::bind("0.0.0.0:9090").await.unwrap();
        axum::serve(listener, counter_router()).await.unwrap();
    });

    // Restate service endpoint on :9080 (registered with the server in Task 5).
    HttpServer::new(
        Endpoint::builder()
            .bind(ToolServiceImpl.serve())
            .bind(AgentServiceImpl.serve())
            .bind(TenantServiceImpl.serve())
            .bind(AuditServiceImpl.serve())
            .bind(MemoryServiceImpl.serve())
            .build(),
    )
    .listen_and_serve("0.0.0.0:9080".parse().unwrap())
    .await;
}
