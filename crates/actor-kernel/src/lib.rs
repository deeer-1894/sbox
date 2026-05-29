use actor_mailbox::{Mailbox, MailboxError};
use api_types::{ActorEvent, ActorId, ActorMessage, ActorSeq};
use async_trait::async_trait;
use event_log::{EventLog, EventLogError};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct TurnBudget {
    pub max_messages: usize,
    pub next_seq: ActorSeq,
}

#[derive(Debug, Clone)]
pub struct ActorTurn {
    pub actor_id: ActorId,
    pub messages: Vec<ActorMessage>,
    pub budget: TurnBudget,
}

#[derive(Debug, Default)]
pub struct TurnOutcome {
    pub events: Vec<ActorEvent>,
    pub outgoing: Vec<ActorMessage>,
}

#[derive(Debug, Error)]
pub enum KernelError {
    #[error(transparent)]
    EventLog(#[from] EventLogError),
    #[error("mailbox backpressure: {0}")]
    MailboxBackpressure(#[from] MailboxError),
}

#[async_trait]
pub trait Actor: Send {
    async fn handle_turn(&mut self, turn: ActorTurn) -> TurnOutcome;
}

pub struct ActorRunner<L, M> {
    event_log: Arc<L>,
    mailbox: Arc<M>,
}

impl<L, M> ActorRunner<L, M>
where
    L: EventLog + 'static,
    M: Mailbox + 'static,
{
    pub fn new(event_log: Arc<L>, mailbox: Arc<M>) -> Self {
        Self { event_log, mailbox }
    }

    pub async fn run_once<A: Actor>(
        &self,
        actor_id: ActorId,
        actor: &mut A,
        budget: TurnBudget,
    ) -> Result<TurnOutcome, KernelError> {
        let messages = self.mailbox.pull(&actor_id, budget.max_messages).await;
        let turn = ActorTurn { actor_id, messages, budget };
        let outcome = actor.handle_turn(turn).await;
        self.event_log.append(outcome.events.clone()).await?;
        for message in &outcome.outgoing {
            self.mailbox.enqueue(message.clone()).await?;
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actor_mailbox::InMemoryMailbox;
    use api_types::{
        ActorEventPayload, ActorMessage, CausalMeta, MessagePayload, MessagePriority, TenantId,
    };
    use event_log::InMemoryEventLog;

    #[derive(Default)]
    struct RecordingActor;

    #[async_trait]
    impl Actor for RecordingActor {
        async fn handle_turn(&mut self, turn: ActorTurn) -> TurnOutcome {
            let events = turn
                .messages
                .into_iter()
                .enumerate()
                .map(|(idx, message)| {
                    let message_id = message.meta.message_id.clone();
                    ActorEvent {
                        actor_id: turn.actor_id.clone(),
                        seq: ActorSeq(turn.budget.next_seq.0 + idx as u64),
                        meta: (*message.meta).clone(),
                        payload: ActorEventPayload::MessageReceived { message_id },
                    }
                })
                .collect();
            TurnOutcome { events, outgoing: Vec::new() }
        }
    }

    fn message(to: &ActorId) -> ActorMessage {
        ActorMessage {
            to: to.clone(),
            from: ActorId::new("sender"),
            priority: MessagePriority::Command,
            idempotency_key: Arc::from("input-1"),
            meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
            payload: MessagePayload::UserInput { content: "hello".to_string() },
        }
    }

    #[tokio::test]
    async fn runner_pulls_messages_and_commits_events() {
        let log = Arc::new(InMemoryEventLog::new());
        let mailbox = Arc::new(InMemoryMailbox::new(10));
        let actor_id = ActorId::new("agent-1");
        mailbox.enqueue(message(&actor_id)).await.unwrap();

        let runner = ActorRunner::new(log.clone(), mailbox);
        let mut actor = RecordingActor::default();
        runner
            .run_once(
                actor_id.clone(),
                &mut actor,
                TurnBudget { max_messages: 10, next_seq: ActorSeq(1) },
            )
            .await
            .unwrap();

        let events = log.replay(&actor_id, ActorSeq(1), None).await;
        assert_eq!(events.len(), 1);
    }
}
