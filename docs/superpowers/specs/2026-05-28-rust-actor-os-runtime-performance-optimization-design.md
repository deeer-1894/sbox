# Rust Actor OS Runtime 性能优化设计

Date: 2026-05-28

## Status

Approved design direction: 渐进式性能优化

## 目标

通过渐进式优化提升 Rust Actor OS Runtime 的性能，重点关注：
- 减少内存分配和克隆
- 降低锁竞争
- 优化数据结构
- 提升代码质量

## 约束

- 保持 API 兼容性
- 不引入新的外部依赖（除非必要）
- 渐进式实施，每个优化点独立可测试

## 成功标准

- 代码更简洁、更易维护
- 减少不必要的内存分配
- 降低锁竞争
- 所有现有测试通过

## 优化点

### 1. 减少内存分配

#### 1.1 ActorMessage 和 ActorEvent 的优化

**问题**：频繁克隆和分配

**解决方案**：
- 使用 `Arc` 共享不可变数据
- 使用 `Cow<'static, str>` 替代 `String` 用于 ID
- 使用对象池减少分配

**实现**：
```rust
// 优化前
pub struct ActorMessage {
    pub to: ActorId,
    pub from: ActorId,
    pub priority: MessagePriority,
    pub idempotency_key: String,
    pub meta: CausalMeta,
    pub payload: MessagePayload,
}

// 优化后
pub struct ActorMessage {
    pub to: Arc<ActorId>,
    pub from: Arc<ActorId>,
    pub priority: MessagePriority,
    pub idempotency_key: Arc<str>,
    pub meta: Arc<CausalMeta>,
    pub payload: MessagePayload,
}
```

#### 1.2 Vec 的预分配

**问题**：`Vec::new()` 后动态增长

**解决方案**：
- 使用 `Vec::with_capacity()` 预分配
- 使用 `SmallVec` 替代小数组

**实现**：
```rust
// 优化前
let mut pulled = Vec::new();

// 优化后
let mut pulled = Vec::with_capacity(limit);
```

### 2. 降低锁竞争

#### 2.1 细粒度锁

**问题**：全局锁导致竞争

**解决方案**：
- 使用 `RwLock` 替代 `Mutex`（读多写少场景）
- 使用分片锁（Sharded Lock）
- 使用无锁数据结构

**实现**：
```rust
// 优化前
pub struct InMemoryMailbox {
    inner: Arc<Mutex<HashMap<ActorId, ActorQueue>>>,
}

// 优化后
pub struct InMemoryMailbox {
    inner: Arc<RwLock<HashMap<ActorId, ActorQueue>>>,
}
```

#### 2.2 使用 Dashmap

**问题**：手动管理锁复杂

**解决方案**：
- 使用 `dashmap` crate 提供的并发 HashMap

**实现**：
```rust
// 优化后
pub struct InMemoryMailbox {
    inner: Arc<DashMap<ActorId, ActorQueue>>,
}
```

### 3. 优化数据结构

#### 3.1 使用更高效的哈希函数

**问题**：默认哈希函数效率不高

**解决方案**：
- 使用 `ahash` 或 `fxhash` 替代默认哈希函数

**实现**：
```rust
// 优化前
use std::collections::HashMap;

// 优化后
use ahash::AHashMap;
```

#### 3.2 使用更高效的集合

**问题**：`VecDeque` 用于优先级队列

**解决方案**：
- 使用 `BinaryHeap` 实现优先级队列
- 使用 `IndexMap` 保持插入顺序

### 4. 减少不必要的克隆

#### 4.1 使用引用计数

**问题**：频繁克隆大型结构

**解决方案**：
- 使用 `Arc` 共享不可变数据
- 使用 `Rc` 在单线程场景

#### 4.2 使用借用

**问题**：不必要的所有权转移

**解决方案**：
- 使用引用而不是克隆
- 使用 `Cow` 延迟克隆

### 5. 引入对象池

#### 5.1 消息对象池

**问题**：频繁创建和销毁消息对象

**解决方案**：
- 使用 `object_pool` 或自定义对象池

**实现**：
```rust
use object_pool::Pool;

pub struct MessagePool {
    pool: Pool<ActorMessage>,
}

impl MessagePool {
    pub fn get(&self) -> PooledActorMessage {
        self.pool.get().unwrap_or_else(|| Box::new(ActorMessage::default()))
    }
}
```

### 6. 优化序列化

#### 6.1 使用更高效的序列化格式

**问题**：JSON 序列化效率不高

**解决方案**：
- 使用 `bincode` 或 `rkyv` 替代 JSON
- 使用零拷贝反序列化

**实现**：
```rust
// 优化前
serde_json::to_string(&message)

// 优化后
bincode::serialize(&message)
```

## 实施计划

### 阶段 1：低风险优化（1-2 天）

1. **Vec 预分配**
   - 修改所有 `Vec::new()` 为 `Vec::with_capacity()`
   - 添加性能测试

2. **使用更高效的哈希函数**
   - 引入 `ahash` 依赖
   - 替换所有 `HashMap` 和 `HashSet`

### 阶段 2：中等风险优化（3-5 天）

1. **细粒度锁**
   - 将 `Mutex` 替换为 `RwLock`
   - 添加并发测试

2. **减少克隆**
   - 使用 `Arc` 共享不可变数据
   - 修改 API 接口

### 阶段 3：高风险优化（1-2 周）

1. **无锁数据结构**
   - 引入 `dashmap`
   - 重新设计热点路径

2. **对象池**
   - 实现消息对象池
   - 添加内存使用监控

## 测试策略

### 单元测试

- 每个优化点独立测试
- 保持所有现有测试通过

### 集成测试

- 测试优化后的完整流程
- 验证功能正确性

### 性能测试

- 添加基准测试
- 比较优化前后的性能

## 风险评估

### 低风险

- Vec 预分配
- 使用更高效的哈希函数

### 中风险

- 细粒度锁
- 减少克隆

### 高风险

- 无锁数据结构
- 对象池

## 监控指标

### 性能指标

- 延迟（P50, P99）
- 吞吐量
- 内存使用
- CPU 使用率

### 代码质量指标

- 代码行数
- 复杂度
- 测试覆盖率

## 参考资料

- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Rust Optimization](https://rust-lang.github.io/rustc-optimization-guide/)
- [Tokio Performance](https://tokio.rs/tokio/tutorial/perf)
