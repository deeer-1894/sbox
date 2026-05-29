use ahash::AHashMap;
use api_types::{ActorEvent, ActorId, ActorSeq};
use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EventLogError {
    #[error("event sequence mismatch for actor {actor_id:?}: expected {expected:?}, got {actual:?}")]
    SequenceMismatch { actor_id: ActorId, expected: ActorSeq, actual: ActorSeq },
}

#[async_trait]
pub trait EventLog: Send + Sync {
    async fn append(&self, events: Vec<ActorEvent>) -> Result<(), EventLogError>;
    async fn replay(&self, actor_id: &ActorId, from: ActorSeq, to: Option<ActorSeq>) -> Vec<ActorEvent>;
}

#[derive(Debug, Default, Clone)]
pub struct InMemoryEventLog {
    inner: Arc<Mutex<AHashMap<ActorId, Vec<ActorEvent>>>>,
}

impl InMemoryEventLog {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl EventLog for InMemoryEventLog {
    async fn append(&self, events: Vec<ActorEvent>) -> Result<(), EventLogError> {
        let mut inner = self.inner.lock().await;
        for event in events {
            let stream = inner.entry(event.actor_id.clone()).or_default();
            let expected = ActorSeq(stream.len() as u64 + 1);
            if event.seq != expected {
                return Err(EventLogError::SequenceMismatch {
                    actor_id: event.actor_id,
                    expected,
                    actual: event.seq,
                });
            }
            stream.push(event);
        }
        Ok(())
    }

    async fn replay(&self, actor_id: &ActorId, from: ActorSeq, to: Option<ActorSeq>) -> Vec<ActorEvent> {
        let inner = self.inner.lock().await;
        let upper = to.unwrap_or(ActorSeq(u64::MAX));
        inner
            .get(actor_id)
            .into_iter()
            .flat_map(|events| events.iter())
            .filter(|event| event.seq >= from && event.seq <= upper)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_types::{ActorEventPayload, CausalMeta, TenantId};

    fn event(actor_id: &ActorId, seq: u64) -> ActorEvent {
        ActorEvent {
            actor_id: actor_id.clone(),
            seq: ActorSeq(seq),
            meta: CausalMeta::root(TenantId::new("tenant-a")),
            payload: ActorEventPayload::MemoryStored { key: format!("k-{seq}") },
        }
    }

    #[tokio::test]
    async fn appends_and_replays_actor_events_by_sequence_range() {
        let log = InMemoryEventLog::new();
        let actor_id = ActorId::new("agent-1");

        log.append(vec![event(&actor_id, 1), event(&actor_id, 2), event(&actor_id, 3)])
            .await
            .unwrap();

        let replayed = log.replay(&actor_id, ActorSeq(2), Some(ActorSeq(3))).await;
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[0].seq, ActorSeq(2));
        assert_eq!(replayed[1].seq, ActorSeq(3));
    }

    #[tokio::test]
    async fn rejects_non_contiguous_actor_sequence() {
        let log = InMemoryEventLog::new();
        let actor_id = ActorId::new("agent-1");

        let err = log.append(vec![event(&actor_id, 2)]).await.unwrap_err();

        assert_eq!(
            err,
            EventLogError::SequenceMismatch {
                actor_id,
                expected: ActorSeq(1),
                actual: ActorSeq(2)
            }
        );
    }
}
