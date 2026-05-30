//! Run against a live stack (compose up, cargo run -p aep-runtime, register.sh).
//!   cargo test -p aep-itest --test sandbox_chain -- --ignored
const INGRESS: &str = "http://localhost:8080";

#[tokio::test]
#[ignore = "requires a live Restate stack"]
async fn permitted_tool_runs_side_effect_in_sandbox() {
    let key = format!("sbx-{}", uuid::Uuid::new_v4());
    let r: serde_json::Value = reqwest::Client::new()
        .post(format!("{INGRESS}/AgentService/agent-1/handle"))
        .json(&serde_json::json!({ "idempotency_key": key, "content": "hello" }))
        .send().await.unwrap().error_for_status().unwrap()
        .json().await.unwrap();
    assert_eq!(r["denied"], serde_json::json!(false));
    // The exec_count came from host_sink POSTing the counter from inside WASM.
    assert!(r["exec_count"].as_u64().unwrap() >= 1, "sandboxed side effect ran");
}
