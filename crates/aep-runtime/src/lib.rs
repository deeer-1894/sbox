//! Restate adapter for the agent/tool domain logic.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use aep_capability::{sign, Action, Capability, Resource};
use aep_domain::{decide, AgentReply, Decision, ToolOutput, ToolRequest, UserInput};
use aep_policy::{evaluate, PolicyDecision};
use axum::{extract::State, routing::{get, post}, Json as AxumJson, Router};
use restate_sdk::prelude::*;

/// Address of the in-process "external" counter the tool side effect mutates.
const COUNTER_BASE: &str = "http://localhost:9090";

/// Process-shared capability signing secret. Single-node Phase 1 only; production
/// uses an asymmetric key so verifiers never hold the signing key.
pub fn cap_secret() -> Vec<u8> {
    std::env::var("AEP_CAP_SECRET").unwrap_or_else(|_| "dev-insecure-secret".into()).into_bytes()
}

/// Current Unix time (seconds). Call only inside `ctx.run` so the value is
/// journaled and stable across replay. The `?` converts the SystemTime error
/// (TerminalError) into the HandlerError that `ctx.run` expects.
fn now_unix() -> Result<u64, HandlerError> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| TerminalError::new(e.to_string()))?
        .as_secs())
}

/// A tool invocation carrying its authorizing capability token.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ToolCall {
    pub request: ToolRequest,
    pub capability_token: String,
}

/// ToolService: keyed by tool name. State key = invocation_id -> ToolOutput.
#[restate_sdk::object]
pub trait ToolService {
    async fn run(call: Json<ToolCall>) -> Result<Json<ToolOutput>, HandlerError>;
}

pub struct ToolServiceImpl;

impl ToolService for ToolServiceImpl {
    async fn run(
        &self,
        ctx: ObjectContext<'_>,
        Json(ToolCall { request: req, capability_token }): Json<ToolCall>,
    ) -> Result<Json<ToolOutput>, HandlerError> {
        // The capability is the sole authority. Deterministic time via ctx.run.
        let now: u64 = ctx.run(|| async { now_unix() }).await?;
        let cap = aep_capability::verify(&cap_secret(), &capability_token, now)
            .map_err(|e| TerminalError::new(format!("capability rejected: {e}")))?;
        cap.authorize(Action::Call, &Resource::Tool { name: req.tool_name.clone() })
            .map_err(|e| TerminalError::new(format!("capability not scoped to tool: {e}")))?;

        // --- Phase 0 side-effect boundary (unchanged) ---
        let existing = ctx.get::<Json<ToolOutput>>(&req.invocation_id).await?.map(|j| j.0);
        match decide(existing) {
            Decision::Reuse(output) => Ok(Json(output)),
            Decision::Execute => {
                let content = req.input.clone();
                let count: u64 = ctx
                    .run(|| async move {
                        let n = reqwest::Client::new()
                            .post(format!("{COUNTER_BASE}/incr"))
                            .send()
                            .await
                            .map_err(|e| TerminalError::new(format!("counter unreachable: {e}")))?
                            .text()
                            .await
                            .map_err(|e| TerminalError::new(format!("counter body: {e}")))?
                            .trim()
                            .parse::<u64>()
                            .map_err(|e| TerminalError::new(format!("counter parse: {e}")))?;
                        Ok(n)
                    })
                    .await?;
                let output = ToolOutput {
                    output: serde_json::json!({ "echo": content }),
                    exec_count: count,
                };
                ctx.set(&req.invocation_id, Json(output.clone()));
                Ok(Json(output))
            }
        }
    }
}

/// The external counter sidecar. Not part of Restate's journal — its mutation is
/// exactly what must happen once per committed side effect.
#[derive(Clone, Default)]
pub struct Counter(Arc<AtomicU64>);

pub fn counter_router() -> Router {
    let state = Counter::default();
    Router::new()
        .route("/incr", post(|State(c): State<Counter>| async move {
            (c.0.fetch_add(1, Ordering::SeqCst) + 1).to_string()
        }))
        .route("/count", get(|State(c): State<Counter>| async move {
            AxumJson(c.0.load(Ordering::SeqCst))
        }))
        .with_state(state)
}

pub use agent::*;
mod agent {
    use super::*;
    use aep_domain::plan_user_input;

    /// AgentService: keyed by agent id. Evaluates policy, mints a capability on
    /// Permit, then calls ToolService with the capability token.
    #[restate_sdk::object]
    pub trait AgentService {
        async fn handle(input: Json<UserInput>) -> Result<Json<AgentReply>, HandlerError>;
    }

    pub struct AgentServiceImpl;

    impl AgentService for AgentServiceImpl {
        async fn handle(
            &self,
            ctx: ObjectContext<'_>,
            Json(input): Json<UserInput>,
        ) -> Result<Json<AgentReply>, HandlerError> {
            let agent_id = ctx.key().to_string();
            let req: ToolRequest = plan_user_input(&input);

            // Policy is evaluated independently of the model's intent.
            if let PolicyDecision::Deny(reason) = evaluate(&agent_id, &req.tool_name) {
                return Ok(Json(AgentReply {
                    output: serde_json::Value::Null,
                    exec_count: 0,
                    denied: true,
                    reason: Some(reason),
                }));
            }

            // Deterministic time for the capability TTL (journaled via ctx.run).
            let now: u64 = ctx.run(|| async { now_unix() }).await?;

            // Mint a short-lived capability scoped to exactly this tool.
            let cap = Capability {
                id: format!("cap-{}", req.invocation_id),
                tenant: "default".into(),
                subject: agent_id,
                resource: Resource::Tool { name: req.tool_name.clone() },
                actions: vec![Action::Call],
                expires_at: now + 300, // 5 minutes
                policy_hash: "tools.cedar@v1".into(),
                audit_id: format!("aud-{}", req.invocation_id),
            };
            let token = sign(&cap_secret(), &cap);

            let tool_key = req.tool_name.clone();
            let Json(out) = ctx
                .object_client::<ToolServiceClient>(tool_key)
                .run(Json(ToolCall { request: req, capability_token: token }))
                .call()
                .await?;
            Ok(Json(AgentReply {
                output: out.output,
                exec_count: out.exec_count,
                denied: false,
                reason: None,
            }))
        }
    }
}
