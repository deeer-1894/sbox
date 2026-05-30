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
