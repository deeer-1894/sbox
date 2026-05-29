use actor_mailbox::MailboxError;
use api_types::{ActorId, MessagePriority, TenantId};
use async_trait::async_trait;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{debug, warn};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SchedulerError {
    #[error("tenant {tenant_id:?} quota exceeded")]
    TenantQuotaExceeded { tenant_id: TenantId },
    #[error("actor {actor_id:?} already scheduled")]
    ActorAlreadyScheduled { actor_id: ActorId },
    #[error("actor {actor_id:?} not found")]
    ActorNotFound { actor_id: ActorId },
    #[error("mailbox error: {0}")]
    Mailbox(#[from] MailboxError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorState {
    Idle,
    Runnable,
    Running,
    Waiting,
    Completed,
    Failed,
}

#[derive(Debug, Clone)]
pub struct ScheduledActor {
    pub actor_id: ActorId,
    pub tenant_id: TenantId,
    pub state: ActorState,
    pub priority: MessagePriority,
}

#[async_trait]
pub trait Scheduler: Send + Sync {
    async fn schedule(&self, actor: ScheduledActor) -> Result<(), SchedulerError>;
    async fn mark_runnable(&self, actor_id: &ActorId) -> Result<(), SchedulerError>;
    async fn next_runnable(&self) -> Option<ScheduledActor>;
    async fn complete(&self, actor_id: &ActorId) -> Result<(), SchedulerError>;
    async fn fail(&self, actor_id: &ActorId) -> Result<(), SchedulerError>;
    async fn get_state(&self, actor_id: &ActorId) -> Option<ActorState>;
}

#[derive(Debug, Clone)]
pub struct TenantQuota {
    pub max_concurrent: usize,
    pub current: usize,
}

#[derive(Debug, Default)]
struct SchedulerInner {
    actors: HashMap<ActorId, ScheduledActor>,
    runnable_queue: VecDeque<ActorId>,
    tenant_quotas: HashMap<TenantId, TenantQuota>,
    running: HashSet<ActorId>,
}

#[derive(Debug, Clone)]
pub struct LocalScheduler {
    inner: Arc<Mutex<SchedulerInner>>,
}

impl LocalScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn with_tenant_quota(tenant_id: TenantId, max_concurrent: usize) -> Self {
        let scheduler = Self::default();
        scheduler.inner.lock().await.tenant_quotas.insert(
            tenant_id,
            TenantQuota {
                max_concurrent,
                current: 0,
            },
        );
        scheduler
    }
}

impl Default for LocalScheduler {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(SchedulerInner::default())),
        }
    }
}

#[async_trait]
impl Scheduler for LocalScheduler {
    async fn schedule(&self, actor: ScheduledActor) -> Result<(), SchedulerError> {
        let mut inner = self.inner.lock().await;

        if inner.actors.contains_key(&actor.actor_id) {
            return Err(SchedulerError::ActorAlreadyScheduled {
                actor_id: actor.actor_id.clone(),
            });
        }

        inner.actors.insert(actor.actor_id.clone(), actor);
        Ok(())
    }

    async fn mark_runnable(&self, actor_id: &ActorId) -> Result<(), SchedulerError> {
        let mut inner = self.inner.lock().await;

        // First, get the tenant_id without holding a mutable borrow
        let tenant_id = inner.actors.get(actor_id)
            .ok_or(SchedulerError::ActorNotFound {
                actor_id: actor_id.clone(),
            })?
            .tenant_id
            .clone();

        // Check tenant quota
        if let Some(quota) = inner.tenant_quotas.get_mut(&tenant_id) {
            if quota.current >= quota.max_concurrent {
                debug!(actor_id = ?actor_id, tenant_id = ?tenant_id, "tenant quota exceeded");
                return Err(SchedulerError::TenantQuotaExceeded {
                    tenant_id,
                });
            }
            quota.current += 1;
        }

        // Now update the actor state
        let actor = inner.actors.get_mut(actor_id).unwrap();
        actor.state = ActorState::Runnable;
        inner.runnable_queue.push_back(actor_id.clone());

        debug!(actor_id = ?actor_id, "actor marked runnable");
        Ok(())
    }

    async fn next_runnable(&self) -> Option<ScheduledActor> {
        let mut inner = self.inner.lock().await;

        while let Some(actor_id) = inner.runnable_queue.pop_front() {
            if let Some(actor) = inner.actors.get(&actor_id) {
                if actor.state == ActorState::Runnable {
                    let mut actor = actor.clone();
                    actor.state = ActorState::Running;
                    inner.actors.insert(actor_id.clone(), actor.clone());
                    inner.running.insert(actor_id);
                    debug!(actor_id = ?actor.actor_id, "actor scheduled for execution");
                    return Some(actor);
                }
            }
        }

        None
    }

    async fn complete(&self, actor_id: &ActorId) -> Result<(), SchedulerError> {
        let mut inner = self.inner.lock().await;

        // First, get the tenant_id without holding a mutable borrow
        let tenant_id = inner.actors.get(actor_id)
            .ok_or(SchedulerError::ActorNotFound {
                actor_id: actor_id.clone(),
            })?
            .tenant_id
            .clone();

        // Release tenant quota
        if let Some(quota) = inner.tenant_quotas.get_mut(&tenant_id) {
            quota.current = quota.current.saturating_sub(1);
        }

        // Now update the actor state
        let actor = inner.actors.get_mut(actor_id).unwrap();
        actor.state = ActorState::Completed;
        inner.running.remove(actor_id);

        debug!(actor_id = ?actor_id, "actor completed");
        Ok(())
    }

    async fn fail(&self, actor_id: &ActorId) -> Result<(), SchedulerError> {
        let mut inner = self.inner.lock().await;

        // First, get the tenant_id without holding a mutable borrow
        let tenant_id = inner.actors.get(actor_id)
            .ok_or(SchedulerError::ActorNotFound {
                actor_id: actor_id.clone(),
            })?
            .tenant_id
            .clone();

        // Release tenant quota
        if let Some(quota) = inner.tenant_quotas.get_mut(&tenant_id) {
            quota.current = quota.current.saturating_sub(1);
        }

        // Now update the actor state
        let actor = inner.actors.get_mut(actor_id).unwrap();
        actor.state = ActorState::Failed;
        inner.running.remove(actor_id);

        warn!(actor_id = ?actor_id, "actor failed");
        Ok(())
    }

    async fn get_state(&self, actor_id: &ActorId) -> Option<ActorState> {
        let inner = self.inner.lock().await;
        inner.actors.get(actor_id).map(|a| a.state.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_actor(id: &str, tenant: &str) -> ScheduledActor {
        ScheduledActor {
            actor_id: ActorId::new(id),
            tenant_id: TenantId::new(tenant),
            state: ActorState::Idle,
            priority: MessagePriority::Command,
        }
    }

    #[tokio::test]
    async fn schedules_and_executes_actor() {
        let scheduler = LocalScheduler::new();
        let actor = make_actor("agent-1", "tenant-a");

        scheduler.schedule(actor.clone()).await.unwrap();
        scheduler.mark_runnable(&actor.actor_id).await.unwrap();

        let next = scheduler.next_runnable().await.unwrap();
        assert_eq!(next.actor_id, actor.actor_id);
        assert_eq!(next.state, ActorState::Running);

        scheduler.complete(&actor.actor_id).await.unwrap();
        assert_eq!(scheduler.get_state(&actor.actor_id).await, Some(ActorState::Completed));
    }

    #[tokio::test]
    async fn enforces_tenant_quota() {
        let scheduler = LocalScheduler::with_tenant_quota(TenantId::new("tenant-a"), 1).await;

        let actor1 = make_actor("agent-1", "tenant-a");
        let actor2 = make_actor("agent-2", "tenant-a");

        scheduler.schedule(actor1.clone()).await.unwrap();
        scheduler.schedule(actor2.clone()).await.unwrap();

        scheduler.mark_runnable(&actor1.actor_id).await.unwrap();
        let result = scheduler.mark_runnable(&actor2.actor_id).await;
        assert_eq!(result.unwrap_err(), SchedulerError::TenantQuotaExceeded {
            tenant_id: TenantId::new("tenant-a"),
        });
    }

    #[tokio::test]
    async fn rejects_duplicate_scheduling() {
        let scheduler = LocalScheduler::new();
        let actor = make_actor("agent-1", "tenant-a");

        scheduler.schedule(actor.clone()).await.unwrap();
        let result = scheduler.schedule(actor.clone()).await;
        assert_eq!(result.unwrap_err(), SchedulerError::ActorAlreadyScheduled {
            actor_id: ActorId::new("agent-1"),
        });
    }

    #[tokio::test]
    async fn handles_actor_failure() {
        let scheduler = LocalScheduler::new();
        let actor = make_actor("agent-1", "tenant-a");

        scheduler.schedule(actor.clone()).await.unwrap();
        scheduler.mark_runnable(&actor.actor_id).await.unwrap();
        scheduler.next_runnable().await;
        scheduler.fail(&actor.actor_id).await.unwrap();

        assert_eq!(scheduler.get_state(&actor.actor_id).await, Some(ActorState::Failed));
    }
}
