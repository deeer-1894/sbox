//! Run against a live stack (see Phase 0 plan, Task 8 header).
//!   cargo test -p aep-itest --test security_chain -- --ignored
const INGRESS: &str = "http://localhost:8080";
const COUNTER: &str = "http://localhost:9090";

async fn counter() -> u64 {
    reqwest::get(format!("{COUNTER}/count")).await.unwrap().json().await.unwrap()
}

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn permitted_tool_runs_once() {
    let key = format!("permit-{}", uuid::Uuid::new_v4());
    let before = counter().await;
    let r: serde_json::Value = reqwest::Client::new()
        .post(format!("{INGRESS}/AgentService/agent-1/handle"))
        .json(&serde_json::json!({ "idempotency_key": key, "content": "hello" }))
        .send().await.unwrap().error_for_status().unwrap()
        .json().await.unwrap();
    assert_eq!(r["denied"], serde_json::json!(false), "echo must be permitted");
    assert_eq!(counter().await, before + 1, "permitted tool runs the side effect once");
}

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn denied_tool_does_not_run() {
    let key = format!("deny-{}", uuid::Uuid::new_v4());
    let before = counter().await;
    let r: serde_json::Value = reqwest::Client::new()
        .post(format!("{INGRESS}/AgentService/agent-1/handle"))
        .json(&serde_json::json!({ "idempotency_key": key, "content": "x", "requested_tool": "shell" }))
        .send().await.unwrap().error_for_status().unwrap()
        .json().await.unwrap();
    assert_eq!(r["denied"], serde_json::json!(true), "shell must be denied by policy");
    assert!(r["reason"].is_string());
    assert_eq!(counter().await, before, "denied tool must NOT run the side effect");
}

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn forged_capability_is_rejected_at_tool_boundary() {
    // Bypass the agent; call ToolService directly with a bogus capability.
    let key = format!("forge-{}", uuid::Uuid::new_v4());
    let before = counter().await;
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
    assert_eq!(counter().await, before, "rejected call must NOT run the side effect");
}
