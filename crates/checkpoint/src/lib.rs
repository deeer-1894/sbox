use api_types::{ActorId, ActorSeq};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CheckpointError {
    #[error("checkpoint not found for actor {actor_id:?}")]
    NotFound { actor_id: ActorId },
    #[error("checkpoint revision mismatch for actor {actor_id:?}: expected {expected:?}, got {actual:?}")]
    RevisionMismatch { actor_id: ActorId, expected: u64, actual: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorSnapshot {
    pub actor_id: ActorId,
    pub last_seq: ActorSeq,
    pub revision: u64,
    pub state: serde_json::Value,
}

#[async_trait]
pub trait CheckpointStore: Send + Sync {
    async fn save(&self, snapshot: ActorSnapshot) -> Result<(), CheckpointError>;
    async fn load(&self, actor_id: &ActorId) -> Result<ActorSnapshot, CheckpointError>;
    async fn list_actors(&self) -> Vec<ActorId>;
}

#[derive(Debug, Default, Clone)]
pub struct InMemoryCheckpointStore {
    inner: Arc<Mutex<HashMap<ActorId, ActorSnapshot>>>,
}

impl InMemoryCheckpointStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl CheckpointStore for InMemoryCheckpointStore {
    async fn save(&self, snapshot: ActorSnapshot) -> Result<(), CheckpointError> {
        let mut inner = self.inner.lock().await;
        inner.insert(snapshot.actor_id.clone(), snapshot);
        Ok(())
    }

    async fn load(&self, actor_id: &ActorId) -> Result<ActorSnapshot, CheckpointError> {
        let inner = self.inner.lock().await;
        inner.get(actor_id).cloned().ok_or(CheckpointError::NotFound {
            actor_id: actor_id.clone(),
        })
    }

    async fn list_actors(&self) -> Vec<ActorId> {
        let inner = self.inner.lock().await;
        inner.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn saves_and_loads_checkpoint() {
        let store = InMemoryCheckpointStore::new();
        let actor_id = ActorId::new("agent-1");

        let snapshot = ActorSnapshot {
            actor_id: actor_id.clone(),
            last_seq: ActorSeq(42),
            revision: 1,
            state: serde_json::json!({"status": "running", "counter": 10}),
        };

        store.save(snapshot.clone()).await.unwrap();
        let loaded = store.load(&actor_id).await.unwrap();

        assert_eq!(loaded.actor_id, actor_id);
        assert_eq!(loaded.last_seq, ActorSeq(42));
        assert_eq!(loaded.revision, 1);
        assert_eq!(loaded.state, serde_json::json!({"status": "running", "counter": 10}));
    }

    #[tokio::test]
    async fn returns_error_for_missing_checkpoint() {
        let store = InMemoryCheckpointStore::new();
        let actor_id = ActorId::new("agent-1");

        let result = store.load(&actor_id).await;
        assert_eq!(result.unwrap_err(), CheckpointError::NotFound { actor_id });
    }

    #[tokio::test]
    async fn lists_all_actors() {
        let store = InMemoryCheckpointStore::new();

        let snapshot1 = ActorSnapshot {
            actor_id: ActorId::new("agent-1"),
            last_seq: ActorSeq(10),
            revision: 1,
            state: serde_json::json!({}),
        };
        let snapshot2 = ActorSnapshot {
            actor_id: ActorId::new("agent-2"),
            last_seq: ActorSeq(20),
            revision: 1,
            state: serde_json::json!({}),
        };

        store.save(snapshot1).await.unwrap();
        store.save(snapshot2).await.unwrap();

        let actors = store.list_actors().await;
        assert_eq!(actors.len(), 2);
        assert!(actors.contains(&ActorId::new("agent-1")));
        assert!(actors.contains(&ActorId::new("agent-2")));
    }

    #[tokio::test]
    async fn overwrites_existing_checkpoint() {
        let store = InMemoryCheckpointStore::new();
        let actor_id = ActorId::new("agent-1");

        let snapshot1 = ActorSnapshot {
            actor_id: actor_id.clone(),
            last_seq: ActorSeq(10),
            revision: 1,
            state: serde_json::json!({"version": 1}),
        };
        let snapshot2 = ActorSnapshot {
            actor_id: actor_id.clone(),
            last_seq: ActorSeq(20),
            revision: 2,
            state: serde_json::json!({"version": 2}),
        };

        store.save(snapshot1).await.unwrap();
        store.save(snapshot2).await.unwrap();

        let loaded = store.load(&actor_id).await.unwrap();
        assert_eq!(loaded.last_seq, ActorSeq(20));
        assert_eq!(loaded.revision, 2);
    }
}
