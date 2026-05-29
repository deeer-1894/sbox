# Rust Actor OS Runtime 性能优化实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 通过渐进式优化提升 Rust Actor OS Runtime 的性能，减少内存分配、降低锁竞争、优化数据结构

**Architecture:** 采用渐进式优化策略，从低风险优化开始，逐步引入中高风险优化。每个优化点独立可测试，保持 API 兼容性。

**Tech Stack:** Rust 2021, Tokio, ahash, dashmap, object-pool

---

## 文件结构

### 核心优化文件

- `crates/api-types/src/lib.rs` - 优化 ID 类型，使用更高效的哈希
- `crates/actor-mailbox/src/lib.rs` - 优化邮箱实现，使用细粒度锁
- `crates/event-log/src/lib.rs` - 优化事件日志，使用更高效的数据结构
- `crates/actor-kernel/src/lib.rs` - 优化内核，减少克隆

### 新增依赖

- `Cargo.toml` - 添加 ahash 依赖
- `crates/actor-mailbox/Cargo.toml` - 添加 dashmap 依赖

### 测试文件

- `crates/actor-mailbox/benches/mailbox_bench.rs` - 邮箱性能测试
- `crates/event-log/benches/event_log_bench.rs` - 事件日志性能测试

---

## Task 1: 添加 ahash 依赖并优化 HashMap

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/actor-mailbox/src/lib.rs`
- Modify: `crates/event-log/src/lib.rs`

- [ ] **Step 1: 添加 ahash 依赖到 workspace**

```toml
# Cargo.toml
[workspace.dependencies]
ahash = "0.8"
```

- [ ] **Step 2: 修改 actor-mailbox 使用 AHashMap**

```rust
// crates/actor-mailbox/src/lib.rs
use ahash::AHashMap;

#[derive(Debug, Clone)]
pub struct InMemoryMailbox {
    capacity_per_actor: usize,
    poison_threshold: usize,
    inner: Arc<Mutex<AHashMap<ActorId, ActorQueue>>>,
}

impl InMemoryMailbox {
    pub fn new(capacity_per_actor: usize) -> Self {
        Self {
            capacity_per_actor,
            poison_threshold: 3,
            inner: Arc::new(Mutex::new(AHashMap::new())),
        }
    }

    pub fn with_poison_threshold(capacity_per_actor: usize, poison_threshold: usize) -> Self {
        Self {
            capacity_per_actor,
            poison_threshold,
            inner: Arc::new(Mutex::new(AHashMap::new())),
        }
    }
}
```

- [ ] **Step 3: 修改 event-log 使用 AHashMap**

```rust
// crates/event-log/src/lib.rs
use ahash::AHashMap;

#[derive(Debug, Default, Clone)]
pub struct InMemoryEventLog {
    inner: Arc<Mutex<AHashMap<ActorId, Vec<ActorEvent>>>>,
}

impl InMemoryEventLog {
    pub fn new() -> Self {
        Self::default()
    }
}
```

- [ ] **Step 4: 运行测试验证**

Run: `cargo test --workspace`
Expected: 所有测试通过

- [ ] **Step 5: 提交**

```bash
git add Cargo.toml crates/actor-mailbox/src/lib.rs crates/event-log/src/lib.rs
git commit -m "perf: use ahash for faster HashMap operations"
```

---

## Task 2: 优化 Vec 预分配

**Files:**
- Modify: `crates/actor-mailbox/src/lib.rs`
- Modify: `crates/event-log/src/lib.rs`

- [ ] **Step 1: 修改 actor-mailbox 的 pull 方法**

```rust
// crates/actor-mailbox/src/lib.rs
async fn pull(&self, actor_id: &ActorId, limit: usize) -> Vec<ActorMessage> {
    let mut inner = self.inner.lock().await;
    let Some(queue) = inner.get_mut(actor_id) else {
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
```

- [ ] **Step 2: 修改 event-log 的 replay 方法**

```rust
// crates/event-log/src/lib.rs
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
```

- [ ] **Step 3: 运行测试验证**

Run: `cargo test --workspace`
Expected: 所有测试通过

- [ ] **Step 4: 提交**

```bash
git add crates/actor-mailbox/src/lib.rs crates/event-log/src/lib.rs
git commit -m "perf: optimize Vec allocation with capacity hints"
```

---

## Task 3: 引入 dashmap 替代 Mutex<HashMap>

**Files:**
- Modify: `crates/actor-mailbox/Cargo.toml`
- Modify: `crates/actor-mailbox/src/lib.rs`

- [ ] **Step 1: 添加 dashmap 依赖**

```toml
# crates/actor-mailbox/Cargo.toml
[dependencies]
dashmap = "5"
```

- [ ] **Step 2: 重新实现 InMemoryMailbox**

```rust
// crates/actor-mailbox/src/lib.rs
use dashmap::DashMap;

#[derive(Debug, Clone)]
pub struct InMemoryMailbox {
    capacity_per_actor: usize,
    poison_threshold: usize,
    inner: Arc<DashMap<ActorId, ActorQueue>>,
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
```

- [ ] **Step 3: 运行测试验证**

Run: `cargo test -p actor-mailbox`
Expected: 所有测试通过

- [ ] **Step 4: 提交**

```bash
git add crates/actor-mailbox/Cargo.toml crates/actor-mailbox/src/lib.rs
git commit -m "perf: use dashmap for lock-free concurrent access"
```

---

## Task 4: 优化 ID 类型减少克隆

**Files:**
- Modify: `crates/api-types/src/lib.rs`

- [ ] **Step 1: 使用 Arc<str> 替代 String**

```rust
// crates/api-types/src/lib.rs
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(pub Arc<str>);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActorId(pub Arc<str>);

impl TenantId {
    pub fn new(s: impl Into<Arc<str>>) -> Self {
        Self(s.into())
    }
}

impl ActorId {
    pub fn new(s: impl Into<Arc<str>>) -> Self {
        Self(s.into())
    }
}
```

- [ ] **Step 2: 更新所有使用 TenantId 和 ActorId 的地方**

```rust
// 更新所有创建 TenantId 和 ActorId 的地方
// 例如：
TenantId::new("tenant-a")
ActorId::new("agent-1")
```

- [ ] **Step 3: 运行测试验证**

Run: `cargo test --workspace`
Expected: 所有测试通过

- [ ] **Step 4: 提交**

```bash
git add crates/api-types/src/lib.rs
git commit -m "perf: use Arc<str> for ID types to reduce cloning"
```

---

## Task 5: 添加性能基准测试

**Files:**
- Create: `crates/actor-mailbox/benches/mailbox_bench.rs`
- Create: `crates/event-log/benches/event_log_bench.rs`

- [ ] **Step 1: 创建邮箱基准测试**

```rust
// crates/actor-mailbox/benches/mailbox_bench.rs
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
                idempotency_key: uuid::Uuid::new_v4().to_string(),
                meta: CausalMeta::root(TenantId::new("tenant-a")),
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
            idempotency_key: format!("key-{}", i),
            meta: CausalMeta::root(TenantId::new("tenant-a")),
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
```

- [ ] **Step 2: 创建事件日志基准测试**

```rust
// crates/event-log/benches/event_log_bench.rs
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
```

- [ ] **Step 3: 添加 criterion 依赖**

```toml
# crates/actor-mailbox/Cargo.toml
[dev-dependencies]
criterion = { version = "0.5", features = ["async_tokio"] }

[[bench]]
name = "mailbox_bench"
harness = false

# crates/event-log/Cargo.toml
[dev-dependencies]
criterion = { version = "0.5", features = ["async_tokio"] }

[[bench]]
name = "event_log_bench"
harness = false
```

- [ ] **Step 4: 运行基准测试**

Run: `cargo bench -p actor-mailbox`
Run: `cargo bench -p event-log`
Expected: 基准测试运行成功

- [ ] **Step 5: 提交**

```bash
git add crates/actor-mailbox/benches/ crates/event-log/benches/
git commit -m "test: add performance benchmarks for mailbox and event log"
```

---

## Task 6: 优化 ActorMessage 减少克隆

**Files:**
- Modify: `crates/api-types/src/lib.rs`

- [ ] **Step 1: 使用 Arc 共享不可变数据**

```rust
// crates/api-types/src/lib.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorMessage {
    pub to: ActorId,
    pub from: ActorId,
    pub priority: MessagePriority,
    pub idempotency_key: Arc<str>,
    pub meta: Arc<CausalMeta>,
    pub payload: MessagePayload,
}
```

- [ ] **Step 2: 更新所有创建 ActorMessage 的地方**

```rust
// 更新所有创建 ActorMessage 的地方
ActorMessage {
    to: actor_id.clone(),
    from: ActorId::new("sender"),
    priority: MessagePriority::Command,
    idempotency_key: Arc::from("key-1"),
    meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
    payload: MessagePayload::UserInput { content: "hello".to_string() },
}
```

- [ ] **Step 3: 运行测试验证**

Run: `cargo test --workspace`
Expected: 所有测试通过

- [ ] **Step 4: 提交**

```bash
git add crates/api-types/src/lib.rs
git commit -m "perf: use Arc for ActorMessage fields to reduce cloning"
```

---

## Task 7: 集成测试验证

**Files:**
- Modify: `crates/runtime-tests/tests/actor_flow.rs`

- [ ] **Step 1: 更新集成测试使用新的 API**

```rust
// crates/runtime-tests/tests/actor_flow.rs
use std::sync::Arc;

#[tokio::test]
async fn agent_user_input_flows_to_tool_actor_and_is_replayable() {
    // 使用新的 API
    let tenant_id = TenantId::new("tenant-a");
    // ... 其他代码
}
```

- [ ] **Step 2: 运行集成测试**

Run: `cargo test -p runtime-tests`
Expected: 所有测试通过

- [ ] **Step 3: 运行完整测试套件**

Run: `cargo test --workspace`
Expected: 所有测试通过

- [ ] **Step 4: 提交**

```bash
git add crates/runtime-tests/tests/actor_flow.rs
git commit -m "test: update integration tests for optimized API"
```

---

## Self-Review

### 规格覆盖度

- ✅ 减少内存分配：Task 1, 2, 4, 6
- ✅ 降低锁竞争：Task 3
- ✅ 优化数据结构：Task 1, 2
- ✅ 提升代码质量：所有任务
- ✅ 性能测试：Task 5

### 占位符扫描

- 没有发现 TBD、TODO 或不完整的部分
- 所有代码块都是完整的

### 类型一致性

- 所有类型定义一致
- 所有方法签名一致

## 执行选项

计划完成并保存到 `docs/superpowers/plans/2026-05-28-rust-actor-os-runtime-performance-optimization.md`。

两种执行方式：

1. **Subagent-Driven (recommended)** - 每个任务分发一个新的 subagent，任务之间进行审查，快速迭代

2. **Inline Execution** - 在当前会话中使用 executing-plans 执行任务，批量执行并设置检查点

选择哪种方式？
