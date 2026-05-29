use criterion::{black_box, criterion_group, criterion_main, Criterion};
use actor_mailbox::{InMemoryMailbox, ChannelMailbox, PooledMailbox, ShardedMailbox, Mailbox};
use api_types::{ActorId, ActorMessage, CausalMeta, MessagePayload, MessagePriority, TenantId};
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::task::JoinSet;

fn bench_enqueue_concurrent(c: &mut Criterion) {
    let mailbox = Arc::new(InMemoryMailbox::new(10000));
    
    c.bench_function("enqueue_concurrent_10", |b| {
        let rt = Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async {
                let mut set = JoinSet::new();
                for i in 0..10 {
                    let mailbox = mailbox.clone();
                    set.spawn(async move {
                        let actor_id = ActorId::new(format!("agent-{}", i));
                        for j in 0..100 {
                            let message = ActorMessage {
                                to: actor_id.clone(),
                                from: ActorId::new("sender"),
                                priority: MessagePriority::Command,
                                idempotency_key: Arc::from(format!("key-{}-{}", i, j)),
                                meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
                                payload: MessagePayload::UserInput { content: "hello".to_string() },
                            };
                            mailbox.enqueue(message).await.unwrap();
                        }
                    });
                }
                while let Some(_) = set.join_next().await {}
            })
        })
    });
}

fn bench_channel_enqueue_concurrent(c: &mut Criterion) {
    let mailbox = Arc::new(ChannelMailbox::new(10000));
    
    c.bench_function("channel_enqueue_concurrent_10", |b| {
        let rt = Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async {
                let mut set = JoinSet::new();
                for i in 0..10 {
                    let mailbox = mailbox.clone();
                    set.spawn(async move {
                        let actor_id = ActorId::new(format!("agent-{}", i));
                        for j in 0..100 {
                            let message = ActorMessage {
                                to: actor_id.clone(),
                                from: ActorId::new("sender"),
                                priority: MessagePriority::Command,
                                idempotency_key: Arc::from(format!("key-{}-{}", i, j)),
                                meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
                                payload: MessagePayload::UserInput { content: "hello".to_string() },
                            };
                            mailbox.enqueue(message).await.unwrap();
                        }
                    });
                }
                while let Some(_) = set.join_next().await {}
            })
        })
    });
}

fn bench_enqueue_batch_concurrent(c: &mut Criterion) {
    let mailbox = Arc::new(InMemoryMailbox::new(100000));
    
    c.bench_function("enqueue_batch_concurrent_10", |b| {
        let rt = Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async {
                let mut set = JoinSet::new();
                for i in 0..10 {
                    let mailbox = mailbox.clone();
                    set.spawn(async move {
                        let actor_id = ActorId::new(format!("agent-{}", i));
                        let mut messages = Vec::with_capacity(100);
                        for j in 0..100 {
                            messages.push(ActorMessage {
                                to: actor_id.clone(),
                                from: ActorId::new("sender"),
                                priority: MessagePriority::Command,
                                idempotency_key: Arc::from(format!("key-{}-{}", i, j)),
                                meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
                                payload: MessagePayload::UserInput { content: "hello".to_string() },
                            });
                        }
                        mailbox.enqueue_batch(messages).await.unwrap();
                    });
                }
                while let Some(_) = set.join_next().await {}
            })
        })
    });
}

fn bench_channel_enqueue_batch_concurrent(c: &mut Criterion) {
    let mailbox = Arc::new(ChannelMailbox::new(100000));
    
    c.bench_function("channel_enqueue_batch_concurrent_10", |b| {
        let rt = Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async {
                let mut set = JoinSet::new();
                for i in 0..10 {
                    let mailbox = mailbox.clone();
                    set.spawn(async move {
                        let actor_id = ActorId::new(format!("agent-{}", i));
                        let mut messages = Vec::with_capacity(100);
                        for j in 0..100 {
                            messages.push(ActorMessage {
                                to: actor_id.clone(),
                                from: ActorId::new("sender"),
                                priority: MessagePriority::Command,
                                idempotency_key: Arc::from(format!("key-{}-{}", i, j)),
                                meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
                                payload: MessagePayload::UserInput { content: "hello".to_string() },
                            });
                        }
                        mailbox.enqueue_batch(messages).await.unwrap();
                    });
                }
                while let Some(_) = set.join_next().await {}
            })
        })
    });
}

fn bench_enqueue_concurrent_100(c: &mut Criterion) {
    let mailbox = Arc::new(InMemoryMailbox::new(100000));
    
    c.bench_function("enqueue_concurrent_100", |b| {
        let rt = Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async {
                let mut set = JoinSet::new();
                for i in 0..100 {
                    let mailbox = mailbox.clone();
                    set.spawn(async move {
                        let actor_id = ActorId::new(format!("agent-{}", i));
                        for j in 0..100 {
                            let message = ActorMessage {
                                to: actor_id.clone(),
                                from: ActorId::new("sender"),
                                priority: MessagePriority::Command,
                                idempotency_key: Arc::from(format!("key-{}-{}", i, j)),
                                meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
                                payload: MessagePayload::UserInput { content: "hello".to_string() },
                            };
                            mailbox.enqueue(message).await.unwrap();
                        }
                    });
                }
                while let Some(_) = set.join_next().await {}
            })
        })
    });
}

fn bench_channel_enqueue_concurrent_100(c: &mut Criterion) {
    let mailbox = Arc::new(ChannelMailbox::new(100000));
    
    c.bench_function("channel_enqueue_concurrent_100", |b| {
        let rt = Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async {
                let mut set = JoinSet::new();
                for i in 0..100 {
                    let mailbox = mailbox.clone();
                    set.spawn(async move {
                        let actor_id = ActorId::new(format!("agent-{}", i));
                        for j in 0..100 {
                            let message = ActorMessage {
                                to: actor_id.clone(),
                                from: ActorId::new("sender"),
                                priority: MessagePriority::Command,
                                idempotency_key: Arc::from(format!("key-{}-{}", i, j)),
                                meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
                                payload: MessagePayload::UserInput { content: "hello".to_string() },
                            };
                            mailbox.enqueue(message).await.unwrap();
                        }
                    });
                }
                while let Some(_) = set.join_next().await {}
            })
        })
    });
}

fn bench_enqueue_batch_concurrent_100(c: &mut Criterion) {
    let mailbox = Arc::new(InMemoryMailbox::new(1000000));
    
    c.bench_function("enqueue_batch_concurrent_100", |b| {
        let rt = Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async {
                let mut set = JoinSet::new();
                for i in 0..100 {
                    let mailbox = mailbox.clone();
                    set.spawn(async move {
                        let actor_id = ActorId::new(format!("agent-{}", i));
                        let mut messages = Vec::with_capacity(100);
                        for j in 0..100 {
                            messages.push(ActorMessage {
                                to: actor_id.clone(),
                                from: ActorId::new("sender"),
                                priority: MessagePriority::Command,
                                idempotency_key: Arc::from(format!("key-{}-{}", i, j)),
                                meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
                                payload: MessagePayload::UserInput { content: "hello".to_string() },
                            });
                        }
                        mailbox.enqueue_batch(messages).await.unwrap();
                    });
                }
                while let Some(_) = set.join_next().await {}
            })
        })
    });
}

fn bench_pull_concurrent(c: &mut Criterion) {
    let mailbox = Arc::new(InMemoryMailbox::new(100000));
    let rt = Runtime::new().unwrap();
    
    // 填充邮箱
    rt.block_on(async {
        for i in 0..100 {
            let actor_id = ActorId::new(format!("agent-{}", i));
            for j in 0..1000 {
                let message = ActorMessage {
                    to: actor_id.clone(),
                    from: ActorId::new("sender"),
                    priority: MessagePriority::Command,
                    idempotency_key: Arc::from(format!("key-{}-{}", i, j)),
                    meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
                    payload: MessagePayload::UserInput { content: "hello".to_string() },
                };
                mailbox.enqueue(message).await.unwrap();
            }
        }
    });
    
    c.bench_function("pull_concurrent_10", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut set = JoinSet::new();
                for i in 0..10 {
                    let mailbox = mailbox.clone();
                    let actor_id = ActorId::new(format!("agent-{}", i));
                    set.spawn(async move {
                        for _ in 0..10 {
                            black_box(mailbox.pull(&actor_id, 100).await);
                        }
                    });
                }
                while let Some(_) = set.join_next().await {}
            })
        })
    });
}

fn bench_channel_pull_concurrent(c: &mut Criterion) {
    let mailbox = Arc::new(ChannelMailbox::new(100000));
    let rt = Runtime::new().unwrap();
    
    // 填充邮箱
    rt.block_on(async {
        for i in 0..100 {
            let actor_id = ActorId::new(format!("agent-{}", i));
            for j in 0..1000 {
                let message = ActorMessage {
                    to: actor_id.clone(),
                    from: ActorId::new("sender"),
                    priority: MessagePriority::Command,
                    idempotency_key: Arc::from(format!("key-{}-{}", i, j)),
                    meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
                    payload: MessagePayload::UserInput { content: "hello".to_string() },
                };
                mailbox.enqueue(message).await.unwrap();
            }
        }
    });
    
    c.bench_function("channel_pull_concurrent_10", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut set = JoinSet::new();
                for i in 0..10 {
                    let mailbox = mailbox.clone();
                    let actor_id = ActorId::new(format!("agent-{}", i));
                    set.spawn(async move {
                        for _ in 0..10 {
                            black_box(mailbox.pull(&actor_id, 100).await);
                        }
                    });
                }
                while let Some(_) = set.join_next().await {}
            })
        })
    });
}

fn bench_pooled_enqueue_concurrent(c: &mut Criterion) {
    let mailbox = Arc::new(PooledMailbox::new(10000));
    
    c.bench_function("pooled_enqueue_concurrent_10", |b| {
        let rt = Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async {
                let mut set = JoinSet::new();
                for i in 0..10 {
                    let mailbox = mailbox.clone();
                    set.spawn(async move {
                        let actor_id = ActorId::new(format!("agent-{}", i));
                        for j in 0..100 {
                            let message = ActorMessage {
                                to: actor_id.clone(),
                                from: ActorId::new("sender"),
                                priority: MessagePriority::Command,
                                idempotency_key: Arc::from(format!("key-{}-{}", i, j)),
                                meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
                                payload: MessagePayload::UserInput { content: "hello".to_string() },
                            };
                            mailbox.enqueue(message).await.unwrap();
                        }
                    });
                }
                while let Some(_) = set.join_next().await {}
            })
        })
    });
}

fn bench_pooled_enqueue_concurrent_100(c: &mut Criterion) {
    let mailbox = Arc::new(PooledMailbox::new(100000));
    
    c.bench_function("pooled_enqueue_concurrent_100", |b| {
        let rt = Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async {
                let mut set = JoinSet::new();
                for i in 0..100 {
                    let mailbox = mailbox.clone();
                    set.spawn(async move {
                        let actor_id = ActorId::new(format!("agent-{}", i));
                        for j in 0..100 {
                            let message = ActorMessage {
                                to: actor_id.clone(),
                                from: ActorId::new("sender"),
                                priority: MessagePriority::Command,
                                idempotency_key: Arc::from(format!("key-{}-{}", i, j)),
                                meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
                                payload: MessagePayload::UserInput { content: "hello".to_string() },
                            };
                            mailbox.enqueue(message).await.unwrap();
                        }
                    });
                }
                while let Some(_) = set.join_next().await {}
            })
        })
    });
}

fn bench_pooled_pull_concurrent(c: &mut Criterion) {
    let mailbox = Arc::new(PooledMailbox::new(100000));
    let rt = Runtime::new().unwrap();
    
    // 填充邮箱
    rt.block_on(async {
        for i in 0..100 {
            let actor_id = ActorId::new(format!("agent-{}", i));
            for j in 0..1000 {
                let message = ActorMessage {
                    to: actor_id.clone(),
                    from: ActorId::new("sender"),
                    priority: MessagePriority::Command,
                    idempotency_key: Arc::from(format!("key-{}-{}", i, j)),
                    meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
                    payload: MessagePayload::UserInput { content: "hello".to_string() },
                };
                mailbox.enqueue(message).await.unwrap();
            }
        }
    });
    
    c.bench_function("pooled_pull_concurrent_10", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut set = JoinSet::new();
                for i in 0..10 {
                    let mailbox = mailbox.clone();
                    let actor_id = ActorId::new(format!("agent-{}", i));
                    set.spawn(async move {
                        for _ in 0..10 {
                            black_box(mailbox.pull(&actor_id, 100).await);
                        }
                    });
                }
                while let Some(_) = set.join_next().await {}
            })
        })
    });
}

fn bench_sharded_enqueue_concurrent(c: &mut Criterion) {
    let mailbox = Arc::new(ShardedMailbox::new(10000));
    
    c.bench_function("sharded_enqueue_concurrent_10", |b| {
        let rt = Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async {
                let mut set = JoinSet::new();
                for i in 0..10 {
                    let mailbox = mailbox.clone();
                    set.spawn(async move {
                        let actor_id = ActorId::new(format!("agent-{}", i));
                        for j in 0..100 {
                            let message = ActorMessage {
                                to: actor_id.clone(),
                                from: ActorId::new("sender"),
                                priority: MessagePriority::Command,
                                idempotency_key: Arc::from(format!("key-{}-{}", i, j)),
                                meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
                                payload: MessagePayload::UserInput { content: "hello".to_string() },
                            };
                            mailbox.enqueue(message).await.unwrap();
                        }
                    });
                }
                while let Some(_) = set.join_next().await {}
            })
        })
    });
}

fn bench_sharded_enqueue_concurrent_100(c: &mut Criterion) {
    let mailbox = Arc::new(ShardedMailbox::new(100000));
    
    c.bench_function("sharded_enqueue_concurrent_100", |b| {
        let rt = Runtime::new().unwrap();
        b.iter(|| {
            rt.block_on(async {
                let mut set = JoinSet::new();
                for i in 0..100 {
                    let mailbox = mailbox.clone();
                    set.spawn(async move {
                        let actor_id = ActorId::new(format!("agent-{}", i));
                        for j in 0..100 {
                            let message = ActorMessage {
                                to: actor_id.clone(),
                                from: ActorId::new("sender"),
                                priority: MessagePriority::Command,
                                idempotency_key: Arc::from(format!("key-{}-{}", i, j)),
                                meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
                                payload: MessagePayload::UserInput { content: "hello".to_string() },
                            };
                            mailbox.enqueue(message).await.unwrap();
                        }
                    });
                }
                while let Some(_) = set.join_next().await {}
            })
        })
    });
}

fn bench_sharded_pull_concurrent(c: &mut Criterion) {
    let mailbox = Arc::new(ShardedMailbox::new(100000));
    let rt = Runtime::new().unwrap();
    
    // 填充邮箱
    rt.block_on(async {
        for i in 0..100 {
            let actor_id = ActorId::new(format!("agent-{}", i));
            for j in 0..1000 {
                let message = ActorMessage {
                    to: actor_id.clone(),
                    from: ActorId::new("sender"),
                    priority: MessagePriority::Command,
                    idempotency_key: Arc::from(format!("key-{}-{}", i, j)),
                    meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
                    payload: MessagePayload::UserInput { content: "hello".to_string() },
                };
                mailbox.enqueue(message).await.unwrap();
            }
        }
    });
    
    c.bench_function("sharded_pull_concurrent_10", |b| {
        b.iter(|| {
            rt.block_on(async {
                let mut set = JoinSet::new();
                for i in 0..10 {
                    let mailbox = mailbox.clone();
                    let actor_id = ActorId::new(format!("agent-{}", i));
                    set.spawn(async move {
                        for _ in 0..10 {
                            black_box(mailbox.pull(&actor_id, 100).await);
                        }
                    });
                }
                while let Some(_) = set.join_next().await {}
            })
        })
    });
}

criterion_group!(
    benches, 
    bench_enqueue_concurrent,
    bench_channel_enqueue_concurrent,
    bench_enqueue_batch_concurrent,
    bench_channel_enqueue_batch_concurrent,
    bench_enqueue_concurrent_100,
    bench_channel_enqueue_concurrent_100,
    bench_enqueue_batch_concurrent_100,
    bench_pull_concurrent,
    bench_channel_pull_concurrent,
    bench_pooled_enqueue_concurrent,
    bench_pooled_enqueue_concurrent_100,
    bench_pooled_pull_concurrent,
    bench_sharded_enqueue_concurrent,
    bench_sharded_enqueue_concurrent_100,
    bench_sharded_pull_concurrent
);
criterion_main!(benches);
