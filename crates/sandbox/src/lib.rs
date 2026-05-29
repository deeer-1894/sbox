use api_types::CapabilityId;
use async_trait::async_trait;
use capability::{Action, CapabilityBroker, CapabilityError, Resource};
use serde_json::{json, Value};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error(transparent)]
    Capability(#[from] CapabilityError),
    #[error("tool not found: {0}")]
    ToolNotFound(String),
}

#[derive(Debug, Clone)]
pub struct SandboxRequest {
    pub tool_name: String,
    pub input: Value,
    pub capability_id: CapabilityId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SandboxOutput {
    pub output: Value,
}

#[async_trait]
pub trait SandboxBackend: Send + Sync {
    async fn execute(&self, request: SandboxRequest) -> Result<SandboxOutput, SandboxError>;
}

#[derive(Debug, Clone)]
pub struct FakeSandbox {
    broker: Arc<Mutex<CapabilityBroker>>,
}

impl FakeSandbox {
    pub fn new(broker: Arc<Mutex<CapabilityBroker>>) -> Self {
        Self { broker }
    }
}

#[async_trait]
impl SandboxBackend for FakeSandbox {
    async fn execute(&self, request: SandboxRequest) -> Result<SandboxOutput, SandboxError> {
        self.broker
            .lock()
            .await
            .authorize(
                &request.capability_id,
                Action::Call,
                &Resource::Tool { name: request.tool_name.clone() },
            )?;

        match request.tool_name.as_str() {
            "echo" => Ok(SandboxOutput { output: request.input }),
            "upper" => {
                let value = request.input.get("text").and_then(Value::as_str).unwrap_or_default();
                Ok(SandboxOutput { output: json!({ "text": value.to_uppercase() }) })
            }
            other => Err(SandboxError::ToolNotFound(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_types::{ActorId, TenantId};
    use capability::CapabilityBroker;
    use time::Duration;

    #[tokio::test]
    async fn executes_authorized_echo_tool() {
        let broker = Arc::new(Mutex::new(CapabilityBroker::default()));
        let capability = broker
            .lock()
            .await
            .issue_tool_call(
                TenantId::new("tenant-a"),
                ActorId::new("agent-1"),
                "echo",
                Duration::minutes(5),
            )
            .unwrap();

        let sandbox = FakeSandbox::new(broker);
        let output = sandbox
            .execute(SandboxRequest {
                tool_name: "echo".to_string(),
                input: json!({ "ok": true }),
                capability_id: capability.id,
            })
            .await
            .unwrap();

        assert_eq!(output.output, json!({ "ok": true }));
    }
}
