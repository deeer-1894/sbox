//! Run against a live stack.
//!   cargo test -p aep-itest --test quota -- --ignored
const INGRESS: &str = "http://localhost:8080";

/// POST a JSON body (for handlers that take an argument).
async fn post(path: &str, body: serde_json::Value) -> (reqwest::StatusCode, serde_json::Value) {
    let resp = reqwest::Client::new()
        .post(format!("{INGRESS}{path}"))
        .json(&body)
        .send().await.unwrap();
    let status = resp.status();
    let json = resp.json().await.unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// POST with no body and no content-type — required by Restate for no-argument
/// handlers (acquire/release/in_flight).
async fn post_empty(path: &str) -> (reqwest::StatusCode, serde_json::Value) {
    let resp = reqwest::Client::new()
        .post(format!("{INGRESS}{path}"))
        .send().await.unwrap();
    let status = resp.status();
    let json = resp.json().await.unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn quota_admits_then_rejects_then_recovers() {
    let tenant = format!("t-{}", uuid::Uuid::new_v4());
    let base = format!("/TenantService/{tenant}");

    post(&format!("{base}/set_limit"), serde_json::json!(1)).await;

    let (_, a1) = post_empty(&format!("{base}/acquire")).await;
    assert_eq!(a1, serde_json::json!(true), "first acquire admitted");

    let (_, a2) = post_empty(&format!("{base}/acquire")).await;
    assert_eq!(a2, serde_json::json!(false), "second acquire over limit -> rejected");

    post_empty(&format!("{base}/release")).await;

    let (_, a3) = post_empty(&format!("{base}/acquire")).await;
    assert_eq!(a3, serde_json::json!(true), "acquire admitted again after release");
}

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn agent_request_is_backpressured_when_tenant_exhausted() {
    let tenant = format!("t-{}", uuid::Uuid::new_v4());
    // Limit 0 -> every admission is rejected.
    post(&format!("/TenantService/{tenant}/set_limit"), serde_json::json!(0)).await;

    let (_, r) = post(
        "/AgentService/agent-q/handle",
        serde_json::json!({ "idempotency_key": format!("q-{}", uuid::Uuid::new_v4()), "content": "hi", "tenant": tenant }),
    )
    .await;
    assert_eq!(r["denied"], serde_json::json!(true), "exhausted tenant is backpressured");
    assert_eq!(r["reason"], serde_json::json!("tenant quota exceeded"));
}

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn default_tenant_request_succeeds() {
    let (_, r) = post(
        "/AgentService/agent-q/handle",
        serde_json::json!({ "idempotency_key": format!("ok-{}", uuid::Uuid::new_v4()), "content": "hi" }),
    )
    .await;
    assert_eq!(r["denied"], serde_json::json!(false), "default tenant (limit 1000) admits");
}
