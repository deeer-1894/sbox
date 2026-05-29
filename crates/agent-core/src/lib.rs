use actor_kernel::{Actor, ActorTurn, TurnOutcome};
use api_types::{
    ActorEvent, ActorEventPayload, ActorId, ActorMessage, ActorSeq, CausalMeta,
    MessagePayload, MessagePriority, TenantId,
};
use async_trait::async_trait;
use capability::{CapabilityBroker, CapabilityError};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use time::Duration;
use tokio::sync::Mutex;

pub struct AgentActor {
    pub tool_actor: ActorId,
}

#[async_trait]
impl Actor for AgentActor {
    async fn handle_turn(&mut self, turn: ActorTurn) -> TurnOutcome {
        let mut events = Vec::new();
        let mut outgoing = Vec::new();
        let mut seq = turn.budget.next_seq.0;

        for message in turn.messages {
            events.push(ActorEvent {
                actor_id: turn.actor_id.clone(),
                seq: ActorSeq(seq),
                meta: (*message.meta).clone(),
                payload: ActorEventPayload::MessageReceived { message_id: message.meta.message_id.clone() },
            });
            seq += 1;

            if let MessagePayload::UserInput { content } = message.payload {
                outgoing.push(ActorMessage {
                    to: self.tool_actor.clone(),
                    from: turn.actor_id.clone(),
                    priority: MessagePriority::Command,
                    idempotency_key: Arc::from(format!("tool-intent-{}", message.meta.message_id.0).as_str()),
                    meta: Arc::new(child_meta(&message.meta)),
                    payload: MessagePayload::ToolIntent {
                        tool_name: "echo".to_string(),
                        input: serde_json::json!({ "content": content }),
                    },
                });
            }
        }

        TurnOutcome { events, outgoing }
    }
}

pub struct PolicyActor {
    pub broker: Arc<Mutex<CapabilityBroker>>,
}

#[async_trait]
impl Actor for PolicyActor {
    async fn handle_turn(&mut self, turn: ActorTurn) -> TurnOutcome {
        let mut events = Vec::new();
        let mut outgoing = Vec::new();
        let mut seq = turn.budget.next_seq.0;

        for message in turn.messages {
            events.push(ActorEvent {
                actor_id: turn.actor_id.clone(),
                seq: ActorSeq(seq),
                meta: (*message.meta).clone(),
                payload: ActorEventPayload::MessageReceived { message_id: message.meta.message_id.clone() },
            });
            seq += 1;

            // PolicyActor 目前是 pass-through，批准所有请求
            // 未来可以添加风险评分、审计记录等逻辑
            match &message.payload {
                MessagePayload::ToolIntent { tool_name, input } => {
                    // 批准并转发
                    outgoing.push(ActorMessage {
                        to: message.from.clone(),
                        from: turn.actor_id.clone(),
                        priority: MessagePriority::Command,
                        idempotency_key: Arc::from(format!("policy-approved-{}", message.meta.message_id.0).as_str()),
                        meta: Arc::new(child_meta(&message.meta)),
                        payload: MessagePayload::ToolIntent {
                            tool_name: tool_name.clone(),
                            input: input.clone(),
                        },
                    });
                }
                _ => {
                    // 其他消息类型直接转发
                    outgoing.push(ActorMessage {
                        to: message.from.clone(),
                        from: turn.actor_id.clone(),
                        priority: message.priority,
                        idempotency_key: Arc::from(format!("policy-passthrough-{}", message.meta.message_id.0).as_str()),
                        meta: Arc::new(child_meta(&message.meta)),
                        payload: message.payload.clone(),
                    });
                }
            }
        }

        TurnOutcome { events, outgoing }
    }
}

pub struct ToolActor {
    pub sandbox_actor: ActorId,
    pub tenant_id: TenantId,
    pub broker: Arc<Mutex<CapabilityBroker>>,
}

#[async_trait]
impl Actor for ToolActor {
    async fn handle_turn(&mut self, turn: ActorTurn) -> TurnOutcome {
        let mut events = Vec::new();
        let mut outgoing = Vec::new();
        let mut seq = turn.budget.next_seq.0;

        for message in turn.messages {
            if let MessagePayload::ToolIntent { tool_name, input } = message.payload {
                // 真正向 CapabilityBroker 申请能力
                let capability_result = self.broker.lock().await.issue_tool_call(
                    self.tenant_id.clone(),
                    turn.actor_id.clone(),
                    &tool_name,
                    Duration::minutes(5),
                );

                match capability_result {
                    Ok(capability) => {
                        let input_hash = compute_hash(&input);
                        events.push(ActorEvent {
                            actor_id: turn.actor_id.clone(),
                            seq: ActorSeq(seq),
                            meta: (*message.meta).clone(),
                            payload: ActorEventPayload::ToolRequested {
                                tool_name: tool_name.clone(),
                                input_hash,
                                capability_id: capability.id.clone(),
                            },
                        });
                        seq += 1;

                        outgoing.push(ActorMessage {
                            to: self.sandbox_actor.clone(),
                            from: turn.actor_id.clone(),
                            priority: MessagePriority::Command,
                            idempotency_key: Arc::from(format!("run-tool-{}", message.meta.message_id.0).as_str()),
                            meta: Arc::new(child_meta(&message.meta)),
                            payload: MessagePayload::RunTool {
                                tool_name,
                                input,
                                capability_id: capability.id,
                            },
                        });
                    }
                    Err(CapabilityError::PolicyDenied(reason)) => {
                        events.push(ActorEvent {
                            actor_id: turn.actor_id.clone(),
                            seq: ActorSeq(seq),
                            meta: (*message.meta).clone(),
                            payload: ActorEventPayload::ActorFailed {
                                reason: format!("Policy denied: {}", reason),
                            },
                        });
                        seq += 1;
                    }
                    Err(e) => {
                        events.push(ActorEvent {
                            actor_id: turn.actor_id.clone(),
                            seq: ActorSeq(seq),
                            meta: (*message.meta).clone(),
                            payload: ActorEventPayload::ActorFailed {
                                reason: format!("Capability error: {}", e),
                            },
                        });
                        seq += 1;
                    }
                }
            }
        }

        TurnOutcome { events, outgoing }
    }
}

fn compute_hash(value: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn child_meta(parent: &CausalMeta) -> CausalMeta {
    CausalMeta {
        tenant_id: parent.tenant_id.clone(),
        trace_id: parent.trace_id.clone(),
        message_id: api_types::MessageId(uuid::Uuid::new_v4()),
        causal_parent_id: Some(parent.message_id.clone()),
        created_at: time::OffsetDateTime::now_utc(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_types::{CausalMeta, TenantId};
    use capability::CapabilityBroker;

    #[tokio::test]
    async fn agent_turn_emits_tool_intent_for_user_input() {
        let agent_id = ActorId::new("agent-1");
        let tool_id = ActorId::new("tool-1");
        let mut actor = AgentActor { tool_actor: tool_id.clone() };
        let message = ActorMessage {
            to: agent_id.clone(),
            from: ActorId::new("session-1"),
            priority: MessagePriority::Command,
            idempotency_key: Arc::from("user-1"),
            meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
            payload: MessagePayload::UserInput { content: "hello".to_string() },
        };

        let outcome = actor
            .handle_turn(ActorTurn {
                actor_id: agent_id,
                messages: vec![message],
                budget: actor_kernel::TurnBudget { max_messages: 10, next_seq: ActorSeq(1) },
            })
            .await;

        assert_eq!(outcome.events.len(), 1);
        assert_eq!(outcome.outgoing.len(), 1);
        assert_eq!(outcome.outgoing[0].to, tool_id);
    }

    #[tokio::test]
    async fn policy_actor_approves_tool_intent() {
        let broker = Arc::new(Mutex::new(CapabilityBroker::default()));
        let policy_id = ActorId::new("policy-1");
        let agent_id = ActorId::new("agent-1");
        let mut actor = PolicyActor { broker };

        let message = ActorMessage {
            to: policy_id.clone(),
            from: agent_id.clone(),
            priority: MessagePriority::Command,
            idempotency_key: Arc::from("tool-intent-1"),
            meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
            payload: MessagePayload::ToolIntent {
                tool_name: "echo".to_string(),
                input: serde_json::json!({"test": true}),
            },
        };

        let outcome = actor
            .handle_turn(ActorTurn {
                actor_id: policy_id,
                messages: vec![message],
                budget: actor_kernel::TurnBudget { max_messages: 10, next_seq: ActorSeq(1) },
            })
            .await;

        assert_eq!(outcome.events.len(), 1);
        assert_eq!(outcome.outgoing.len(), 1);
        assert_eq!(outcome.outgoing[0].to, agent_id);
    }
}
