//! Run against a live stack:
//!   1. docker compose -f deploy/docker-compose.yml up -d
//!   2. cargo run -p aep-runtime           (terminal A)
//!   3. ./scripts/register.sh
//!   4. cargo test -p aep-itest -- --ignored
const INGRESS: &str = "http://localhost:8080";
const COUNTER: &str = "http://localhost:9090";

async fn counter() -> u64 {
    reqwest::get(format!("{COUNTER}/count")).await.unwrap().json().await.unwrap()
}

async fn invoke_agent(idempotency_key: &str) -> serde_json::Value {
    reqwest::Client::new()
        .post(format!("{INGRESS}/AgentService/agent-1/handle"))
        .json(&serde_json::json!({ "idempotency_key": idempotency_key, "content": "hello" }))
        .send().await.unwrap()
        .error_for_status().unwrap()
        .json().await.unwrap()
}

#[tokio::test]
#[ignore = "requires a live Restate stack; see file header"]
async fn resend_does_not_re_execute_side_effect() {
    // Unique key per run so prior runs' journaled state can't interfere.
    let key = format!("itest-{}", uuid::Uuid::new_v4());

    let before = counter().await;

    // First invocation: the side effect runs exactly once.
    let r1 = invoke_agent(&key).await;
    let after_first = counter().await;
    assert_eq!(after_first, before + 1, "first call must run the side effect once");
    assert_eq!(r1["exec_count"].as_u64().unwrap(), after_first);

    // Resend with the SAME idempotency key: ToolCompleted exists -> reuse, no re-run.
    let r2 = invoke_agent(&key).await;
    let after_second = counter().await;
    assert_eq!(after_second, after_first, "resend must NOT re-run the side effect");
    assert_eq!(
        r2["exec_count"].as_u64().unwrap(),
        r1["exec_count"].as_u64().unwrap(),
        "resend must return the originally committed result",
    );
}
