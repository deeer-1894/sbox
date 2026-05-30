//! Run against a live stack (see Phase 0 plan, Task 8 header).
//!   cargo test -p aep-itest --test security_chain -- --ignored
//!
//! These tests share one global counter sidecar and run concurrently, so they
//! assert on the per-request RESPONSE (which deterministically proves the
//! security property) rather than the shared counter value. "Runs exactly once"
//! is covered separately by the effectively_once test.
const INGRESS: &str = "http://localhost:8080";

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn permitted_tool_runs() {
    let key = format!("permit-{}", uuid::Uuid::new_v4());
    let r: serde_json::Value = reqwest::Client::new()
        .post(format!("{INGRESS}/AgentService/agent-1/handle"))
        .json(&serde_json::json!({ "idempotency_key": key, "content": "hello" }))
        .send().await.unwrap().error_for_status().unwrap()
        .json().await.unwrap();
    assert_eq!(r["denied"], serde_json::json!(false), "echo must be permitted");
    assert!(r["exec_count"].as_u64().unwrap() >= 1, "permitted tool ran the side effect");
}

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn denied_tool_does_not_run() {
    let key = format!("deny-{}", uuid::Uuid::new_v4());
    let r: serde_json::Value = reqwest::Client::new()
        .post(format!("{INGRESS}/AgentService/agent-1/handle"))
        .json(&serde_json::json!({ "idempotency_key": key, "content": "x", "requested_tool": "shell" }))
        .send().await.unwrap().error_for_status().unwrap()
        .json().await.unwrap();
    // A policy denial returns before any tool call is made, so the side effect
    // can never run for a denied tool.
    assert_eq!(r["denied"], serde_json::json!(true), "shell must be denied by policy");
    assert!(r["reason"].is_string());
    assert_eq!(r["exec_count"].as_u64().unwrap(), 0, "denied path performs no side effect");
}

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn forged_capability_is_rejected_at_tool_boundary() {
    // Bypass the agent; call ToolService directly with a bogus capability. The
    // capability is verified BEFORE the side-effect boundary, so a forged token
    // fails the invocation (non-2xx) without ever running the tool.
    let key = format!("forge-{}", uuid::Uuid::new_v4());
    let resp = reqwest::Client::new()
        .post(format!("{INGRESS}/ToolService/echo/run"))
        .json(&serde_json::json!({
            "request": {
                "invocation_id": key, "tool_name": "echo",
                "input": { "content": "hi" }, "input_hash": "x"
            },
            "capability_token": "not.a.valid.token"
        }))
        .send().await.unwrap();
    assert!(!resp.status().is_success(), "forged capability must be rejected");
}
