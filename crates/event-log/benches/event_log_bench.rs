use criterion::{black_box, criterion_group, criterion_main, Criterion};
use event_log::{InMemoryEventLog, EventLog};
use api_types::{ActorId, ActorEvent, ActorEventPayload, ActorSeq, CausalMeta, TenantId};

fn bench_append(c: &mut Criterion) {
    let log = InMemoryEventLog::new();
    let actor_id = ActorId::new("agent-1");

    c.bench_function("append", |b| {
        let mut seq = 1;
        b.iter(|| {
            let event = ActorEvent {
                actor_id: actor_id.clone(),
                seq: ActorSeq(seq),
                meta: CausalMeta::root(TenantId::new("tenant-a")),
                payload: ActorEventPayload::MemoryStored { key: format!("k-{}", seq) },
            };
            seq += 1;
            black_box(log.append(vec![event]))
        })
    });
}

fn bench_replay(c: &mut Criterion) {
    let log = InMemoryEventLog::new();
    let actor_id = ActorId::new("agent-1");

    // 填充事件日志
    for i in 1..=100 {
        let event = ActorEvent {
            actor_id: actor_id.clone(),
            seq: ActorSeq(i),
            meta: CausalMeta::root(TenantId::new("tenant-a")),
            payload: ActorEventPayload::MemoryStored { key: format!("k-{}", i) },
        };
        tokio::runtime::Runtime::new().unwrap().block_on(log.append(vec![event])).unwrap();
    }

    c.bench_function("replay", |b| {
        b.iter(|| {
            black_box(log.replay(&actor_id, ActorSeq(1), None))
        })
    });
}

criterion_group!(benches, bench_append, bench_replay);
criterion_main!(benches);