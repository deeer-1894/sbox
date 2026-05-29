# Actor CLI 工具设计

Date: 2026-05-28

## Status

Approved design direction: 单一 CLI crate，直接调用运行时 API

## 目标

为 Rust Actor OS Runtime 提供命令行管理工具，支持：
- Actor 管理（创建、查询、删除）
- 消息发送（单条、批量）
- 状态监控（邮箱深度、调度器状态）
- 日志查看（事件日志、因果追踪、死信队列）
- 性能测试（基准测试）
- 检查点管理（保存、加载、列表）

## 约束

- 使用 clap 4 作为 CLI 框架
- 直接调用运行时 API，无网络通信
- 支持多种输出格式（Text、JSON、Table）
- 所有操作都是异步的

## 成功标准

- CLI 工具可以管理 Actor 生命周期
- 可以发送和接收消息
- 可以查看运行时状态
- 可以运行性能测试
- 所有操作都有适当的错误处理

## 架构设计

### 整体架构

```
actor-cli/
├── Cargo.toml
└── src/
    ├── main.rs           # 入口点，解析命令行参数
    ├── cli.rs            # CLI 命令定义
    ├── runtime.rs        # 运行时上下文，封装所有 crate 的 API
    └── commands/
        ├── mod.rs
        ├── actor.rs      # Actor 管理命令
        ├── message.rs    # 消息发送命令
        ├── status.rs     # 状态监控命令
        ├── logs.rs       # 日志查看命令
        └── bench.rs      # 性能测试命令
```

### 核心组件

1. **CLI 命令层** - 使用 clap 定义命令结构
2. **运行时上下文** - 封装所有 crate 的 API，提供统一的接口
3. **命令执行层** - 实现各个命令的具体逻辑

### 依赖关系

```toml
[dependencies]
actor-kernel = { path = "../actor-kernel" }
actor-mailbox = { path = "../actor-mailbox" }
actor-scheduler = { path = "../actor-scheduler" }
agent-core = { path = "../agent-core" }
api-types = { path = "../api-types" }
capability = { path = "../capability" }
checkpoint = { path = "../checkpoint" }
event-log = { path = "../event-log" }
clap = { version = "4", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
```

## CLI 命令结构

### 命令层次

```
actor-cli
├── actor
│   ├── create <actor-id> [--kind <kind>]
│   ├── list [--tenant <tenant-id>]
│   ├── get <actor-id>
│   └── delete <actor-id>
├── message
│   ├── send <to> <from> <payload>
│   └── batch <file>
├── status
│   ├── mailbox <actor-id>
│   ├── scheduler
│   └── overview
├── logs
│   ├── events <actor-id> [--from <seq>] [--to <seq>]
│   ├── trace <trace-id>
│   └── dead-letters <actor-id>
├── bench
│   ├── enqueue [--concurrent <n>] [--count <n>]
│   ├── pull [--concurrent <n>] [--count <n>]
│   └── full [--concurrent <n>] [--count <n>]
└── checkpoint
    ├── save <actor-id>
    ├── load <actor-id>
    └── list
```

### 示例用法

```bash
# 创建 Actor
actor-cli actor create agent-1 --kind Agent

# 发送消息
actor-cli message send agent-1 session-1 --content "hello"

# 查看邮箱状态
actor-cli status mailbox agent-1

# 查看事件日志
actor-cli logs events agent-1 --from 1 --to 10

# 运行性能测试
actor-cli bench enqueue --concurrent 10 --count 1000

# 保存检查点
actor-cli checkpoint save agent-1
```

## 运行时上下文

### RuntimeContext 结构

```rust
pub struct RuntimeContext {
    // 核心组件
    event_log: Arc<InMemoryEventLog>,
    mailbox: Arc<ChannelMailbox>,
    scheduler: Arc<LocalScheduler>,
    checkpoint_store: Arc<InMemoryCheckpointStore>,
    capability_broker: Arc<Mutex<CapabilityBroker>>,
    
    // 配置
    config: RuntimeConfig,
}

pub struct RuntimeConfig {
    pub tenant_id: TenantId,
    pub mailbox_capacity: usize,
    pub poison_threshold: usize,
}
```

### 初始化流程

```rust
impl RuntimeContext {
    pub fn new(config: RuntimeConfig) -> Self {
        let event_log = Arc::new(InMemoryEventLog::new());
        let mailbox = Arc::new(ChannelMailbox::new(config.mailbox_capacity));
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
}
```

### API 封装

```rust
impl RuntimeContext {
    // Actor 管理
    pub async fn create_actor(&self, actor_id: ActorId, kind: ActorKind) -> Result<()>;
    pub async fn list_actors(&self) -> Vec<ActorId>;
    pub async fn get_actor(&self, actor_id: &ActorId) -> Option<ActorSnapshot>;
    pub async fn delete_actor(&self, actor_id: &ActorId) -> Result<()>;
    
    // 消息发送
    pub async fn send_message(&self, message: ActorMessage) -> Result<()>;
    pub async fn send_batch(&self, messages: Vec<ActorMessage>) -> Result<()>;
    
    // 状态查询
    pub async fn mailbox_depth(&self, actor_id: &ActorId) -> usize;
    pub async fn scheduler_status(&self) -> SchedulerStatus;
    
    // 日志查询
    pub async fn get_events(&self, actor_id: &ActorId, from: ActorSeq, to: Option<ActorSeq>) -> Vec<ActorEvent>;
    pub async fn get_trace(&self, trace_id: &TraceId) -> Vec<ActorEvent>;
    pub async fn get_dead_letters(&self, actor_id: &ActorId) -> Vec<DeadLetter>;
    
    // 检查点
    pub async fn save_checkpoint(&self, actor_id: &ActorId) -> Result<()>;
    pub async fn load_checkpoint(&self, actor_id: &ActorId) -> Result<ActorSnapshot>;
    
    // 性能测试
    pub async fn bench_enqueue(&self, concurrent: usize, count: usize) -> BenchResult;
    pub async fn bench_pull(&self, concurrent: usize, count: usize) -> BenchResult;
}
```

## 错误处理

### 错误类型

```rust
#[derive(Debug, Error)]
pub enum CliError {
    #[error("actor not found: {0}")]
    ActorNotFound(ActorId),
    
    #[error("mailbox full: {0}")]
    MailboxFull(ActorId),
    
    #[error("checkpoint not found: {0}")]
    CheckpointNotFound(ActorId),
    
    #[error("invalid input: {0}")]
    InvalidInput(String),
    
    #[error("runtime error: {0}")]
    Runtime(#[from] anyhow::Error),
}

pub type CliResult<T> = Result<T, CliError>;
```

### 输出格式

```rust
#[derive(clap::ValueEnum, Clone)]
pub enum OutputFormat {
    Text,  // 默认，人类可读
    Json,  // JSON 格式
    Table, // 表格格式
}
```

## 测试策略

### 单元测试

- 每个命令模块独立测试
- 测试命令解析和执行
- 测试错误处理

### 集成测试

- 测试完整的命令流程
- 测试多个命令的组合
- 测试运行时上下文

### 端到端测试

- 测试 CLI 与运行时的交互
- 测试真实场景
- 测试性能

### 示例测试

```rust
#[tokio::test]
async fn test_create_and_list_actor() {
    let ctx = RuntimeContext::new(RuntimeConfig::default());
    
    // 创建 Actor
    ctx.create_actor(ActorId::new("test-1"), ActorKind::Agent).await.unwrap();
    
    // 列出 Actor
    let actors = ctx.list_actors().await;
    assert_eq!(actors.len(), 1);
    assert_eq!(actors[0], ActorId::new("test-1"));
}
```

## 实施计划

### 阶段 1：基础框架（1-2 天）

1. 创建 actor-cli crate
2. 实现 CLI 命令解析
3. 实现 RuntimeContext 基础结构

### 阶段 2：核心功能（3-5 天）

1. 实现 Actor 管理命令
2. 实现消息发送命令
3. 实现状态监控命令

### 阶段 3：高级功能（1-2 周）

1. 实现日志查看命令
2. 实现性能测试命令
3. 实现检查点管理命令

## 参考资料

- [clap 文档](https://docs.rs/clap/)
- [Tokio 文档](https://tokio.rs/)
- [Rust CLI 工具开发](https://rust-cli.github.io/book/)
