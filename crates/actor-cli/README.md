# Actor CLI

Rust Actor OS Runtime 命令行管理工具。

## 安装

```bash
cargo install --path crates/actor-cli
```

## 使用

### Actor 管理

```bash
# 创建 Actor
actor-cli actor create agent-1 --kind Agent

# 列出 Actor
actor-cli actor list

# 获取 Actor 详情
actor-cli actor get agent-1

# 删除 Actor
actor-cli actor delete agent-1
```

### 消息发送

```bash
# 发送消息
actor-cli message send agent-1 session-1 --content "hello"

# 批量发送消息
actor-cli message batch messages.json
```

### 状态监控

```bash
# 查看邮箱状态
actor-cli status mailbox agent-1

# 查看调度器状态
actor-cli status scheduler

# 查看概览
actor-cli status overview
```

### 日志查看

```bash
# 查看事件日志
actor-cli logs events agent-1 --from 1 --to 10

# 查看因果追踪
actor-cli logs trace trace-id

# 查看死信队列
actor-cli logs dead-letters agent-1
```

### 性能测试

```bash
# 测试入队性能
actor-cli bench enqueue --concurrent 10 --count 1000

# 测试出队性能
actor-cli bench pull --concurrent 10 --count 1000

# 完整测试
actor-cli bench full --concurrent 10 --count 1000
```

### 检查点管理

```bash
# 保存检查点
actor-cli checkpoint save agent-1

# 加载检查点
actor-cli checkpoint load agent-1

# 列出检查点
actor-cli checkpoint list
```