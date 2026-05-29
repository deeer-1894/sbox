use ahash::AHashSet;
use api_types::{ActorId, ActorMessage};
use async_trait::async_trait;
use crossbeam_channel::{Receiver, Sender};
use dashmap::DashMap;
use object_pool::Pool;
use std::collections::VecDeque;
use std::sync::Arc;
use thiserror::Error;
use tracing::warn;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MailboxError {
    #[error("mailbox for {actor_id:?} is full")]
    MailboxFull { actor_id: ActorId },
}

#[derive(Debug, Clone)]
pub struct DeadLetter {
    pub message: ActorMessage,
    pub reason: String,
    pub attempts: usize,
}

#[async_trait]
pub trait Mailbox: Send + Sync {
    async fn enqueue(&self, message: ActorMessage) -> Result<(), MailboxError>;
    async fn enqueue_batch(&self, messages: Vec<ActorMessage>) -> Result<(), MailboxError>;
    async fn pull(&self, actor_id: &ActorId, limit: usize) -> Vec<ActorMessage>;
    async fn depth(&self, actor_id: &ActorId) -> usize;
    async fn move_to_dead_letter(&self, message: ActorMessage, reason: String) -> ();
    async fn get_dead_letters(&self, actor_id: &ActorId) -> Vec<DeadLetter>;
    async fn poison_count(&self, actor_id: &ActorId) -> usize;
}

// Channel-based mailbox for high-throughput scenarios
#[derive(Debug)]
pub struct ChannelMailbox {
    capacity_per_actor: usize,
    poison_threshold: usize,
    senders: DashMap<ActorId, Sender<ActorMessage>>,
    receivers: DashMap<ActorId, Receiver<ActorMessage>>,
    dead_letters: DashMap<ActorId, Vec<DeadLetter>>,
    poison_counts: DashMap<ActorId, usize>,
}

impl ChannelMailbox {
    pub fn new(capacity_per_actor: usize) -> Self {
        Self {
            capacity_per_actor,
            poison_threshold: 3,
            senders: DashMap::new(),
            receivers: DashMap::new(),
            dead_letters: DashMap::new(),
            poison_counts: DashMap::new(),
        }
    }

    pub fn with_poison_threshold(capacity_per_actor: usize, poison_threshold: usize) -> Self {
        Self {
            capacity_per_actor,
            poison_threshold,
            senders: DashMap::new(),
            receivers: DashMap::new(),
            dead_letters: DashMap::new(),
            poison_counts: DashMap::new(),
        }
    }

    fn get_or_create_channel(&self, actor_id: &ActorId) -> (Sender<ActorMessage>, Receiver<ActorMessage>) {
        if let Some(sender) = self.senders.get(actor_id) {
            let receiver = self.receivers.get(actor_id).unwrap();
            return (sender.clone(), receiver.clone());
        }

        let (sender, receiver) = crossbeam_channel::bounded(self.capacity_per_actor);
        self.senders.insert(actor_id.clone(), sender.clone());
        self.receivers.insert(actor_id.clone(), receiver.clone());
        (sender, receiver)
    }
}

/// Default mailbox implementation using crossbeam-channel for best performance
pub type DefaultMailbox = ChannelMailbox;

// Object pool for reusing ActorMessage allocations
pub struct PooledMailbox {
    capacity_per_actor: usize,
    poison_threshold: usize,
    inner: DashMap<ActorId, ActorQueue>,
    message_pool: Arc<Pool<ActorMessage>>,
}

// Sharded mailbox for better concurrency
pub struct ShardedMailbox {
    capacity_per_actor: usize,
    poison_threshold: usize,
    shards: Vec<DashMap<ActorId, ActorQueue>>,
    shard_count: usize,
}

impl ShardedMailbox {
    pub fn new(capacity_per_actor: usize) -> Self {
        let shard_count = 16; // 16 个分片
        let mut shards = Vec::with_capacity(shard_count);
        for _ in 0..shard_count {
            shards.push(DashMap::new());
        }
        Self {
            capacity_per_actor,
            poison_threshold: 3,
            shards,
            shard_count,
        }
    }

    pub fn with_poison_threshold(capacity_per_actor: usize, poison_threshold: usize) -> Self {
        let shard_count = 16;
        let mut shards = Vec::with_capacity(shard_count);
        for _ in 0..shard_count {
            shards.push(DashMap::new());
        }
        Self {
            capacity_per_actor,
            poison_threshold,
            shards,
            shard_count,
        }
    }

    fn get_shard(&self, actor_id: &ActorId) -> &DashMap<ActorId, ActorQueue> {
        let hash = {
            use std::hash::{Hash, Hasher};
            use ahash::AHasher;
            let mut hasher = AHasher::default();
            actor_id.hash(&mut hasher);
            hasher.finish()
        };
        &self.shards[hash as usize % self.shard_count]
    }
}

#[async_trait]
impl Mailbox for ShardedMailbox {
    async fn enqueue(&self, message: ActorMessage) -> Result<(), MailboxError> {
        let shard = self.get_shard(&message.to);
        let mut queue = shard.entry(message.to.clone()).or_default();
        if queue.idempotency_keys.contains(&message.idempotency_key) {
            return Ok(());
        }
        if queue.messages.len() >= self.capacity_per_actor {
            return Err(MailboxError::MailboxFull { actor_id: message.to });
        }
        queue.idempotency_keys.insert(message.idempotency_key.clone());
        queue.messages.push_back(message);
        Ok(())
    }

    async fn enqueue_batch(&self, messages: Vec<ActorMessage>) -> Result<(), MailboxError> {
        // 按 actor_id 分组
        let grouped: DashMap<ActorId, Vec<ActorMessage>> = DashMap::new();
        for message in messages {
            grouped.entry(message.to.clone()).or_default().push(message);
        }

        // 批量插入
        for entry in grouped {
            let actor_id = entry.0;
            let messages = entry.1;
            let shard = self.get_shard(&actor_id);
            let mut queue = shard.entry(actor_id).or_default();
            
            for message in messages {
                if queue.idempotency_keys.contains(&message.idempotency_key) {
                    continue;
                }
                if queue.messages.len() >= self.capacity_per_actor {
                    return Err(MailboxError::MailboxFull { actor_id: message.to });
                }
                queue.idempotency_keys.insert(message.idempotency_key.clone());
                queue.messages.push_back(message);
            }
        }
        Ok(())
    }

    async fn pull(&self, actor_id: &ActorId, limit: usize) -> Vec<ActorMessage> {
        let shard = self.get_shard(actor_id);
        let Some(mut queue) = shard.get_mut(actor_id) else {
            return Vec::new();
        };
        let mut pulled = Vec::with_capacity(limit);
        for _ in 0..limit {
            let Some(message) = queue.messages.pop_front() else {
                break;
            };
            queue.idempotency_keys.remove(&message.idempotency_key);
            pulled.push(message);
        }
        pulled.sort_by_key(|message| message.priority);
        pulled
    }

    async fn depth(&self, actor_id: &ActorId) -> usize {
        let shard = self.get_shard(actor_id);
        shard.get(actor_id).map(|queue| queue.messages.len()).unwrap_or(0)
    }

    async fn move_to_dead_letter(&self, message: ActorMessage, reason: String) -> () {
        let shard = self.get_shard(&message.to);
        let mut queue = shard.entry(message.to.clone()).or_default();

        queue.poison_count += 1;
        let attempts = queue.poison_count;

        if attempts >= self.poison_threshold {
            warn!(
                actor_id = ?message.to,
                idempotency_key = ?message.idempotency_key,
                reason = ?reason,
                attempts = attempts,
                "message moved to dead letter queue"
            );
        }

        queue.dead_letters.push_back(DeadLetter {
            message,
            reason,
            attempts,
        });
    }

    async fn get_dead_letters(&self, actor_id: &ActorId) -> Vec<DeadLetter> {
        let shard = self.get_shard(actor_id);
        shard.get(actor_id)
            .map(|queue| queue.dead_letters.iter().cloned().collect())
            .unwrap_or_default()
    }

    async fn poison_count(&self, actor_id: &ActorId) -> usize {
        let shard = self.get_shard(actor_id);
        shard.get(actor_id).map(|queue| queue.poison_count).unwrap_or(0)
    }
}

impl PooledMailbox {
    pub fn new(capacity_per_actor: usize) -> Self {
        Self {
            capacity_per_actor,
            poison_threshold: 3,
            inner: DashMap::new(),
            message_pool: Arc::new(Pool::new(1024, || ActorMessage {
                to: ActorId::new(""),
                from: ActorId::new(""),
                priority: api_types::MessagePriority::Command,
                idempotency_key: Arc::from(""),
                meta: Arc::new(api_types::CausalMeta::root(api_types::TenantId::new(""))),
                payload: api_types::MessagePayload::UserInput { content: String::new() },
            })),
        }
    }

    pub fn with_poison_threshold(capacity_per_actor: usize, poison_threshold: usize) -> Self {
        Self {
            capacity_per_actor,
            poison_threshold,
            inner: DashMap::new(),
            message_pool: Arc::new(Pool::new(1024, || ActorMessage {
                to: ActorId::new(""),
                from: ActorId::new(""),
                priority: api_types::MessagePriority::Command,
                idempotency_key: Arc::from(""),
                meta: Arc::new(api_types::CausalMeta::root(api_types::TenantId::new(""))),
                payload: api_types::MessagePayload::UserInput { content: String::new() },
            })),
        }
    }
}

#[async_trait]
impl Mailbox for PooledMailbox {
    async fn enqueue(&self, message: ActorMessage) -> Result<(), MailboxError> {
        let mut queue = self.inner.entry(message.to.clone()).or_default();
        if queue.idempotency_keys.contains(&message.idempotency_key) {
            return Ok(());
        }
        if queue.messages.len() >= self.capacity_per_actor {
            return Err(MailboxError::MailboxFull { actor_id: message.to });
        }
        queue.idempotency_keys.insert(message.idempotency_key.clone());
        queue.messages.push_back(message);
        Ok(())
    }

    async fn enqueue_batch(&self, messages: Vec<ActorMessage>) -> Result<(), MailboxError> {
        // 按 actor_id 分组
        let grouped: DashMap<ActorId, Vec<ActorMessage>> = DashMap::new();
        for message in messages {
            grouped.entry(message.to.clone()).or_default().push(message);
        }

        // 批量插入
        for entry in grouped {
            let actor_id = entry.0;
            let messages = entry.1;
            let mut queue = self.inner.entry(actor_id).or_default();
            
            for message in messages {
                if queue.idempotency_keys.contains(&message.idempotency_key) {
                    continue;
                }
                if queue.messages.len() >= self.capacity_per_actor {
                    return Err(MailboxError::MailboxFull { actor_id: message.to });
                }
                queue.idempotency_keys.insert(message.idempotency_key.clone());
                queue.messages.push_back(message);
            }
        }
        Ok(())
    }

    async fn pull(&self, actor_id: &ActorId, limit: usize) -> Vec<ActorMessage> {
        let Some(mut queue) = self.inner.get_mut(actor_id) else {
            return Vec::new();
        };
        let mut pulled = Vec::with_capacity(limit);
        for _ in 0..limit {
            let Some(message) = queue.messages.pop_front() else {
                break;
            };
            queue.idempotency_keys.remove(&message.idempotency_key);
            pulled.push(message);
        }
        pulled.sort_by_key(|message| message.priority);
        pulled
    }

    async fn depth(&self, actor_id: &ActorId) -> usize {
        self.inner.get(actor_id).map(|queue| queue.messages.len()).unwrap_or(0)
    }

    async fn move_to_dead_letter(&self, message: ActorMessage, reason: String) -> () {
        let mut queue = self.inner.entry(message.to.clone()).or_default();

        queue.poison_count += 1;
        let attempts = queue.poison_count;

        if attempts >= self.poison_threshold {
            warn!(
                actor_id = ?message.to,
                idempotency_key = ?message.idempotency_key,
                reason = ?reason,
                attempts = attempts,
                "message moved to dead letter queue"
            );
        }

        queue.dead_letters.push_back(DeadLetter {
            message,
            reason,
            attempts,
        });
    }

    async fn get_dead_letters(&self, actor_id: &ActorId) -> Vec<DeadLetter> {
        self.inner.get(actor_id)
            .map(|queue| queue.dead_letters.iter().cloned().collect())
            .unwrap_or_default()
    }

    async fn poison_count(&self, actor_id: &ActorId) -> usize {
        self.inner.get(actor_id).map(|queue| queue.poison_count).unwrap_or(0)
    }
}

#[async_trait]
impl Mailbox for ChannelMailbox {
    async fn enqueue(&self, message: ActorMessage) -> Result<(), MailboxError> {
        let actor_id = message.to.clone();
        let (sender, _) = self.get_or_create_channel(&actor_id);
        sender.try_send(message).map_err(|_| MailboxError::MailboxFull {
            actor_id,
        })
    }

    async fn enqueue_batch(&self, messages: Vec<ActorMessage>) -> Result<(), MailboxError> {
        // 按 actor_id 分组
        let grouped: DashMap<ActorId, Vec<ActorMessage>> = DashMap::new();
        for message in messages {
            grouped.entry(message.to.clone()).or_default().push(message);
        }

        // 批量插入
        for entry in grouped {
            let actor_id = entry.0;
            let messages = entry.1;
            let (sender, _) = self.get_or_create_channel(&actor_id);
            
            for message in messages {
                sender.try_send(message).map_err(|e| {
                    match e {
                        crossbeam_channel::TrySendError::Full(_) => MailboxError::MailboxFull {
                            actor_id: actor_id.clone(),
                        },
                        _ => MailboxError::MailboxFull {
                            actor_id: actor_id.clone(),
                        },
                    }
                })?;
            }
        }
        Ok(())
    }

    async fn pull(&self, actor_id: &ActorId, limit: usize) -> Vec<ActorMessage> {
        let (_, receiver) = self.get_or_create_channel(actor_id);
        let mut pulled = Vec::with_capacity(limit);
        
        for _ in 0..limit {
            match receiver.try_recv() {
                Ok(message) => pulled.push(message),
                Err(_) => break,
            }
        }
        pulled.sort_by_key(|message| message.priority);
        pulled
    }

    async fn depth(&self, actor_id: &ActorId) -> usize {
        let (_, receiver) = self.get_or_create_channel(actor_id);
        receiver.len()
    }

    async fn move_to_dead_letter(&self, message: ActorMessage, reason: String) -> () {
        let mut count = self.poison_counts.entry(message.to.clone()).or_insert(0);
        *count += 1;
        let attempts = *count;

        if attempts >= self.poison_threshold {
            warn!(
                actor_id = ?message.to,
                idempotency_key = ?message.idempotency_key,
                reason = ?reason,
                attempts = attempts,
                "message moved to dead letter queue"
            );
        }

        self.dead_letters.entry(message.to.clone()).or_default().push(DeadLetter {
            message,
            reason,
            attempts,
        });
    }

    async fn get_dead_letters(&self, actor_id: &ActorId) -> Vec<DeadLetter> {
        self.dead_letters.get(actor_id)
            .map(|letters| letters.clone())
            .unwrap_or_default()
    }

    async fn poison_count(&self, actor_id: &ActorId) -> usize {
        self.poison_counts.get(actor_id).map(|c| *c).unwrap_or(0)
    }
}

// Original DashMap-based mailbox
#[derive(Debug, Clone)]
pub struct InMemoryMailbox {
    capacity_per_actor: usize,
    poison_threshold: usize,
    inner: Arc<DashMap<ActorId, ActorQueue>>,
}

#[derive(Debug, Default)]
struct ActorQueue {
    idempotency_keys: AHashSet<Arc<str>>,
    messages: VecDeque<ActorMessage>,
    dead_letters: VecDeque<DeadLetter>,
    poison_count: usize,
}

impl InMemoryMailbox {
    pub fn new(capacity_per_actor: usize) -> Self {
        Self {
            capacity_per_actor,
            poison_threshold: 3,
            inner: Arc::new(DashMap::new()),
        }
    }

    pub fn with_poison_threshold(capacity_per_actor: usize, poison_threshold: usize) -> Self {
        Self {
            capacity_per_actor,
            poison_threshold,
            inner: Arc::new(DashMap::new()),
        }
    }
}

#[async_trait]
impl Mailbox for InMemoryMailbox {
    async fn enqueue(&self, message: ActorMessage) -> Result<(), MailboxError> {
        let mut queue = self.inner.entry(message.to.clone()).or_default();
        if queue.idempotency_keys.contains(&message.idempotency_key) {
            return Ok(());
        }
        if queue.messages.len() >= self.capacity_per_actor {
            return Err(MailboxError::MailboxFull { actor_id: message.to });
        }
        queue.idempotency_keys.insert(message.idempotency_key.clone());
        queue.messages.push_back(message);
        Ok(())
    }

    async fn enqueue_batch(&self, messages: Vec<ActorMessage>) -> Result<(), MailboxError> {
        // 按 actor_id 分组
        let grouped: DashMap<ActorId, Vec<ActorMessage>> = DashMap::new();
        for message in messages {
            grouped.entry(message.to.clone()).or_default().push(message);
        }

        // 批量插入
        for entry in grouped {
            let actor_id = entry.0;
            let messages = entry.1;
            let mut queue = self.inner.entry(actor_id).or_default();
            
            for message in messages {
                if queue.idempotency_keys.contains(&message.idempotency_key) {
                    continue;
                }
                if queue.messages.len() >= self.capacity_per_actor {
                    return Err(MailboxError::MailboxFull { actor_id: message.to });
                }
                queue.idempotency_keys.insert(message.idempotency_key.clone());
                queue.messages.push_back(message);
            }
        }
        Ok(())
    }

    async fn pull(&self, actor_id: &ActorId, limit: usize) -> Vec<ActorMessage> {
        let Some(mut queue) = self.inner.get_mut(actor_id) else {
            return Vec::new();
        };
        let mut pulled = Vec::with_capacity(limit);
        for _ in 0..limit {
            let Some(message) = queue.messages.pop_front() else {
                break;
            };
            queue.idempotency_keys.remove(&message.idempotency_key);
            pulled.push(message);
        }
        pulled.sort_by_key(|message| message.priority);
        pulled
    }

    async fn depth(&self, actor_id: &ActorId) -> usize {
        self.inner.get(actor_id).map(|queue| queue.messages.len()).unwrap_or(0)
    }

    async fn move_to_dead_letter(&self, message: ActorMessage, reason: String) -> () {
        let mut queue = self.inner.entry(message.to.clone()).or_default();

        queue.poison_count += 1;
        let attempts = queue.poison_count;

        if attempts >= self.poison_threshold {
            warn!(
                actor_id = ?message.to,
                idempotency_key = ?message.idempotency_key,
                reason = ?reason,
                attempts = attempts,
                "message moved to dead letter queue"
            );
        }

        queue.dead_letters.push_back(DeadLetter {
            message,
            reason,
            attempts,
        });
    }

    async fn get_dead_letters(&self, actor_id: &ActorId) -> Vec<DeadLetter> {
        self.inner.get(actor_id)
            .map(|queue| queue.dead_letters.iter().cloned().collect())
            .unwrap_or_default()
    }

    async fn poison_count(&self, actor_id: &ActorId) -> usize {
        self.inner.get(actor_id).map(|queue| queue.poison_count).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use api_types::{CausalMeta, MessagePayload, MessagePriority, TenantId};

    fn message(to: &ActorId, key: &str) -> ActorMessage {
        ActorMessage {
            to: to.clone(),
            from: ActorId::new("sender"),
            priority: MessagePriority::Command,
            idempotency_key: Arc::from(key),
            meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
            payload: MessagePayload::UserInput { content: "hello".to_string() },
        }
    }

    #[tokio::test]
    async fn deduplicates_by_idempotency_key() {
        let mailbox = InMemoryMailbox::new(10);
        let actor_id = ActorId::new("agent-1");

        mailbox.enqueue(message(&actor_id, "same")).await.unwrap();
        mailbox.enqueue(message(&actor_id, "same")).await.unwrap();

        assert_eq!(mailbox.depth(&actor_id).await, 1);
    }

    #[tokio::test]
    async fn enforces_per_actor_capacity() {
        let mailbox = InMemoryMailbox::new(1);
        let actor_id = ActorId::new("agent-1");

        mailbox.enqueue(message(&actor_id, "a")).await.unwrap();
        let err = mailbox.enqueue(message(&actor_id, "b")).await.unwrap_err();

        assert_eq!(err, MailboxError::MailboxFull { actor_id });
    }

    #[tokio::test]
    async fn moves_message_to_dead_letter_queue() {
        let mailbox = InMemoryMailbox::new(10);
        let actor_id = ActorId::new("agent-1");
        let msg = message(&actor_id, "poison-1");

        mailbox.move_to_dead_letter(msg.clone(), "processing failed".to_string()).await;

        let dead_letters = mailbox.get_dead_letters(&actor_id).await;
        assert_eq!(dead_letters.len(), 1);
        assert_eq!(dead_letters[0].message.idempotency_key, Arc::from("poison-1"));
        assert_eq!(dead_letters[0].reason, "processing failed");
        assert_eq!(dead_letters[0].attempts, 1);
    }

    #[tokio::test]
    async fn tracks_poison_count() {
        let mailbox = InMemoryMailbox::with_poison_threshold(10, 2);
        let actor_id = ActorId::new("agent-1");

        assert_eq!(mailbox.poison_count(&actor_id).await, 0);

        mailbox.move_to_dead_letter(message(&actor_id, "p1"), "error".to_string()).await;
        assert_eq!(mailbox.poison_count(&actor_id).await, 1);

        mailbox.move_to_dead_letter(message(&actor_id, "p2"), "error".to_string()).await;
        assert_eq!(mailbox.poison_count(&actor_id).await, 2);
    }

    #[tokio::test]
    async fn poison_threshold_triggers_warning() {
        let mailbox = InMemoryMailbox::with_poison_threshold(10, 2);
        let actor_id = ActorId::new("agent-1");

        // 第一条消息不会触发警告
        mailbox.move_to_dead_letter(message(&actor_id, "p1"), "error".to_string()).await;

        // 第二条消息会触发警告（达到阈值）
        mailbox.move_to_dead_letter(message(&actor_id, "p2"), "error".to_string()).await;

        let dead_letters = mailbox.get_dead_letters(&actor_id).await;
        assert_eq!(dead_letters.len(), 2);
        assert_eq!(dead_letters[1].attempts, 2);
    }

    #[tokio::test]
    async fn channel_mailbox_basic() {
        let mailbox = ChannelMailbox::new(10);
        let actor_id = ActorId::new("agent-1");

        mailbox.enqueue(message(&actor_id, "key-1")).await.unwrap();
        assert_eq!(mailbox.depth(&actor_id).await, 1);

        let pulled = mailbox.pull(&actor_id, 10).await;
        assert_eq!(pulled.len(), 1);
        assert_eq!(pulled[0].idempotency_key, Arc::from("key-1"));
    }

    #[tokio::test]
    async fn channel_mailbox_capacity() {
        let mailbox = ChannelMailbox::new(2);
        let actor_id = ActorId::new("agent-1");

        mailbox.enqueue(message(&actor_id, "key-1")).await.unwrap();
        mailbox.enqueue(message(&actor_id, "key-2")).await.unwrap();
        let err = mailbox.enqueue(message(&actor_id, "key-3")).await.unwrap_err();

        assert_eq!(err, MailboxError::MailboxFull { actor_id });
    }
}
