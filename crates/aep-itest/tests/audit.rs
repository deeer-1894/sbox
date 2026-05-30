//! Run against a live stack.
//!   cargo test -p aep-itest --test audit -- --ignored
const INGRESS: &str = "http://localhost:8080";

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn causal_chain_is_reconstructable() {
    let key = format!("aud-{}", uuid::Uuid::new_v4());
    let client = reqwest::Client::new();

    // Drive one agent run.
    let r: serde_json::Value = client
        .post(format!("{INGRESS}/AgentService/agent-aud/handle"))
        .json(&serde_json::json!({ "idempotency_key": key, "content": "hello" }))
        .send().await.unwrap().error_for_status().unwrap()
        .json().await.unwrap();
    assert_eq!(r["denied"], serde_json::json!(false));

    // Reconstruct the causal chain for this trace (trace_id == idempotency_key).
    let chain: Vec<serde_json::Value> = client
        .post(format!("{INGRESS}/AuditService/{key}/chain"))
        .send().await.unwrap().error_for_status().unwrap()
        .json().await.unwrap();
    let kinds: Vec<&str> = chain.iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert_eq!(
        kinds,
        vec!["input", "policy_permit", "capability_minted", "tool_requested", "tool_completed"],
        "events reconstruct in causal order",
    );

    // "Which capability authorized this tool invocation?"
    let cap: serde_json::Value = client
        .post(format!("{INGRESS}/AuditService/{key}/capability"))
        .json(&key) // invocation_id == idempotency_key
        .send().await.unwrap().error_for_status().unwrap()
        .json().await.unwrap();
    assert_eq!(cap, serde_json::json!(format!("cap-{key}")), "capability lookup resolves");
}
