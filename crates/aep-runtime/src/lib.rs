//! Restate adapter for the agent/tool domain logic.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use aep_domain::{decide, AgentReply, Decision, ToolOutput, ToolRequest, UserInput};
use axum::{extract::State, routing::{get, post}, Json as AxumJson, Router};
use restate_sdk::prelude::*;

/// Address of the in-process "external" counter the tool side effect mutates.
const COUNTER_BASE: &str = "http://localhost:9090";

/// ToolService: keyed by tool name. State key = invocation_id -> ToolOutput.
#[restate_sdk::object]
pub trait ToolService {
    async fn run(req: Json<ToolRequest>) -> Result<Json<ToolOutput>, HandlerError>;
}

pub struct ToolServiceImpl;

impl ToolService for ToolServiceImpl {
    async fn run(
        &self,
        ctx: ObjectContext<'_>,
        Json(req): Json<ToolRequest>,
    ) -> Result<Json<ToolOutput>, HandlerError> {
        // Read any committed completion for this invocation (the durable journal).
        let existing = ctx.get::<Json<ToolOutput>>(&req.invocation_id).await?.map(|j| j.0);

        match decide(existing) {
            Decision::Reuse(output) => Ok(Json(output)),
            Decision::Execute => {
                // The side effect: POST the external counter. ctx.run journals the
                // result so a retry/replay reuses it instead of POSTing again.
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
                // Commit ToolCompleted: future resends of this invocation reuse it.
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

    /// AgentService: keyed by agent id. Plans the tool request and calls ToolService.
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
            let req: ToolRequest = plan_user_input(&input);
            let tool_key = req.tool_name.clone();
            let Json(out) = ctx
                .object_client::<ToolServiceClient>(tool_key)
                .run(Json(req))
                .call()
                .await?;
            Ok(Json(AgentReply { output: out.output, exec_count: out.exec_count }))
        }
    }
}
