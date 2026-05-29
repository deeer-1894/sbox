use std::sync::Arc;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(pub Arc<str>);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActorId(pub Arc<str>);

impl TenantId {
    pub fn new(s: impl Into<Arc<str>>) -> Self {
        Self(s.into())
    }
}

impl ActorId {
    pub fn new(s: impl Into<Arc<str>>) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub Uuid);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId(pub Uuid);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CapabilityId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ActorSeq(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalMeta {
    pub tenant_id: TenantId,
    pub trace_id: TraceId,
    pub message_id: MessageId,
    pub causal_parent_id: Option<MessageId>,
    pub created_at: OffsetDateTime,
}

impl CausalMeta {
    pub fn root(tenant_id: TenantId) -> Self {
        Self {
            tenant_id,
            trace_id: TraceId(Uuid::new_v4()),
            message_id: MessageId(Uuid::new_v4()),
            causal_parent_id: None,
            created_at: OffsetDateTime::now_utc(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActorKind {
    Agent,
    Tool,
    Memory,
    Policy,
    Sandbox,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessagePayload {
    UserInput { content: String },
    ToolIntent { tool_name: String, input: serde_json::Value },
    PolicyApproved { capability_id: CapabilityId },
    PolicyDenied { reason: String },
    RunTool { tool_name: String, input: serde_json::Value, capability_id: CapabilityId },
    ToolCompleted { output: serde_json::Value },
    StoreMemory { key: String, value: serde_json::Value },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorMessage {
    pub to: ActorId,
    pub from: ActorId,
    pub priority: MessagePriority,
    pub idempotency_key: Arc<str>,
    pub meta: Arc<CausalMeta>,
    pub payload: MessagePayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MessagePriority {
    Control = 0,
    Command = 1,
    Event = 2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActorEventPayload {
    MessageReceived { message_id: MessageId },
    ToolRequested { tool_name: String, input_hash: String, capability_id: CapabilityId },
    ToolCompleted { output_hash: String },
    MemoryStored { key: String },
    ActorFailed { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorEvent {
    pub actor_id: ActorId,
    pub seq: ActorSeq,
    pub meta: CausalMeta,
    pub payload: ActorEventPayload,
}
