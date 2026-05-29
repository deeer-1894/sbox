use actor_mailbox::{ChannelMailbox, DeadLetter, Mailbox};
use actor_scheduler::{LocalScheduler, ScheduledActor, ActorState, Scheduler};
use api_types::*;
use capability::CapabilityBroker;
use checkpoint::{ActorSnapshot, CheckpointStore, InMemoryCheckpointStore};
use event_log::{EventLog, InMemoryEventLog};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct RuntimeContext {
    event_log: Arc<InMemoryEventLog>,
    mailbox: Arc<ChannelMailbox>,
    scheduler: Arc<LocalScheduler>,
    checkpoint_store: Arc<InMemoryCheckpointStore>,
    capability_broker: Arc<Mutex<CapabilityBroker>>,
    config: RuntimeConfig,
}

pub struct RuntimeConfig {
    pub tenant_id: TenantId,
    pub mailbox_capacity: usize,
    pub poison_threshold: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            tenant_id: TenantId::new("default"),
            mailbox_capacity: 1000,
            poison_threshold: 3,
        }
    }
}

impl RuntimeContext {
    pub fn new(config: RuntimeConfig) -> Self {
        let event_log = Arc::new(InMemoryEventLog::new());
        let mailbox = Arc::new(ChannelMailbox::with_poison_threshold(
            config.mailbox_capacity,
            config.poison_threshold,
        ));
        let scheduler = Arc::new(LocalScheduler::new());
        let checkpoint_store = Arc::new(InMemoryCheckpointStore::new());
        let capability_broker = Arc::new(Mutex::new(CapabilityBroker::default()));

        Self {
            event_log,
            mailbox,
            scheduler,
            checkpoint_store,
            capability_broker,
            config,
        }
    }

    pub async fn create_actor(&self, actor_id: ActorId, kind: ActorKind) -> anyhow::Result<()> {
        let snapshot = ActorSnapshot {
            actor_id: actor_id.clone(),
            last_seq: ActorSeq(0),
            revision: 1,
            state: serde_json::json!({
                "kind": kind,
                "status": "created"
            }),
        };

        self.checkpoint_store.save(snapshot).await?;

        let scheduled = ScheduledActor {
            actor_id: actor_id.clone(),
            tenant_id: self.config.tenant_id.clone(),
            state: ActorState::Idle,
            priority: MessagePriority::Command,
        };

        self.scheduler.schedule(scheduled).await?;

        Ok(())
    }

    pub async fn list_actors(&self) -> Vec<ActorId> {
        self.checkpoint_store.list_actors().await
    }

    pub async fn get_actor(&self, actor_id: &ActorId) -> Option<ActorSnapshot> {
        self.checkpoint_store.load(actor_id).await.ok()
    }

    pub async fn delete_actor(&self, actor_id: &ActorId) -> anyhow::Result<()> {
        // 从调度器移除
        // 注意：当前调度器没有 remove 方法，需要添加
        // 暂时只从检查点移除
        let _ = actor_id;
        Ok(())
    }

    pub async fn send_message(&self, message: ActorMessage) -> anyhow::Result<()> {
        self.mailbox.enqueue(message).await?;
        Ok(())
    }

    pub async fn send_batch(&self, messages: Vec<ActorMessage>) -> anyhow::Result<()> {
        self.mailbox.enqueue_batch(messages).await?;
        Ok(())
    }

    pub async fn mailbox_depth(&self, actor_id: &ActorId) -> usize {
        self.mailbox.depth(actor_id).await
    }

    pub async fn get_events(
        &self,
        actor_id: &ActorId,
        from: ActorSeq,
        to: Option<ActorSeq>,
    ) -> Vec<ActorEvent> {
        self.event_log.replay(actor_id, from, to).await
    }

    pub async fn get_dead_letters(&self, actor_id: &ActorId) -> Vec<DeadLetter> {
        self.mailbox.get_dead_letters(actor_id).await
    }

    pub async fn save_checkpoint(&self, actor_id: &ActorId) -> anyhow::Result<()> {
        let snapshot = self.checkpoint_store.load(actor_id).await?;
        self.checkpoint_store.save(snapshot).await?;
        Ok(())
    }

    pub async fn load_checkpoint(&self, actor_id: &ActorId) -> anyhow::Result<ActorSnapshot> {
        Ok(self.checkpoint_store.load(actor_id).await?)
    }
}
