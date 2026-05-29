use std::sync::Arc;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use actor_mailbox::{InMemoryMailbox, Mailbox};
use api_types::{ActorId, ActorMessage, CausalMeta, MessagePayload, MessagePriority, TenantId};

fn bench_enqueue(c: &mut Criterion) {
    let mailbox = InMemoryMailbox::new(1000);
    let actor_id = ActorId::new("agent-1");

    c.bench_function("enqueue", |b| {
        b.iter(|| {
            let message = ActorMessage {
                to: actor_id.clone(),
                from: ActorId::new("sender"),
                priority: MessagePriority::Command,
                idempotency_key: Arc::from(uuid::Uuid::new_v4().to_string().as_str()),
                meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
                payload: MessagePayload::UserInput { content: "hello".to_string() },
            };
            black_box(mailbox.enqueue(message))
        })
    });
}

fn bench_pull(c: &mut Criterion) {
    let mailbox = InMemoryMailbox::new(1000);
    let actor_id = ActorId::new("agent-1");

    // 填充邮箱
    for i in 0..100 {
        let message = ActorMessage {
            to: actor_id.clone(),
            from: ActorId::new("sender"),
            priority: MessagePriority::Command,
            idempotency_key: Arc::from(format!("key-{}", i).as_str()),
            meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
            payload: MessagePayload::UserInput { content: "hello".to_string() },
        };
        tokio::runtime::Runtime::new().unwrap().block_on(mailbox.enqueue(message)).unwrap();
    }

    c.bench_function("pull", |b| {
        b.iter(|| {
            black_box(mailbox.pull(&actor_id, 10))
        })
    });
}

criterion_group!(benches, bench_enqueue, bench_pull);
criterion_main!(benches);