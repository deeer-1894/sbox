use actor_kernel::{ActorRunner, TurnBudget};
use actor_mailbox::{InMemoryMailbox, Mailbox};
use actor_scheduler::{LocalScheduler, Scheduler, ScheduledActor, ActorState};
use agent_core::{AgentActor, ToolActor};
use api_types::{ActorId, ActorMessage, ActorSeq, CausalMeta, MessagePayload, MessagePriority, TenantId};
use capability::CapabilityBroker;
use checkpoint::{InMemoryCheckpointStore, CheckpointStore, ActorSnapshot};
use event_log::{EventLog, InMemoryEventLog};
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::test]
async fn agent_user_input_flows_to_tool_actor_and_is_replayable() {
    let log = Arc::new(InMemoryEventLog::new());
    let mailbox = Arc::new(InMemoryMailbox::new(16));
    let runner = ActorRunner::new(log.clone(), mailbox.clone());
    let broker = Arc::new(Mutex::new(CapabilityBroker::default()));

    let agent_id = ActorId::new("agent-1");
    let tool_id = ActorId::new("tool-1");
    let sandbox_id = ActorId::new("sandbox-1");
    let tenant_id = TenantId::new("tenant-a");

    mailbox
        .enqueue(ActorMessage {
            to: agent_id.clone(),
            from: ActorId::new("session-1"),
            priority: MessagePriority::Command,
            idempotency_key: Arc::from("user-input-1"),
            meta: Arc::new(CausalMeta::root(tenant_id.clone())),
            payload: MessagePayload::UserInput { content: "hello".to_string() },
        })
        .await
        .unwrap();

    let mut agent = AgentActor { tool_actor: tool_id.clone() };
    runner
        .run_once(
            agent_id.clone(),
            &mut agent,
            TurnBudget { max_messages: 8, next_seq: ActorSeq(1) },
        )
        .await
        .unwrap();

    assert_eq!(mailbox.depth(&tool_id).await, 1);

    let mut tool = ToolActor {
        sandbox_actor: sandbox_id,
        tenant_id,
        broker,
    };
    runner
        .run_once(
            tool_id.clone(),
            &mut tool,
            TurnBudget { max_messages: 8, next_seq: ActorSeq(1) },
        )
        .await
        .unwrap();

    let agent_events = log.replay(&agent_id, ActorSeq(1), None).await;
    let tool_events = log.replay(&tool_id, ActorSeq(1), None).await;

    assert_eq!(agent_events.len(), 1);
    assert_eq!(tool_events.len(), 1);
    assert_eq!(mailbox.depth(&tool_id).await, 0);
}

#[tokio::test]
async fn checkpoint_and_recovery_flow() {
    let checkpoint_store = InMemoryCheckpointStore::new();
    let actor_id = ActorId::new("agent-1");

    // 保存 checkpoint
    let snapshot = ActorSnapshot {
        actor_id: actor_id.clone(),
        last_seq: ActorSeq(42),
        revision: 1,
        state: serde_json::json!({"status": "running", "counter": 10}),
    };
    checkpoint_store.save(snapshot).await.unwrap();

    // 模拟崩溃恢复
    let recovered = checkpoint_store.load(&actor_id).await.unwrap();
    assert_eq!(recovered.last_seq, ActorSeq(42));
    assert_eq!(recovered.revision, 1);
    assert_eq!(recovered.state, serde_json::json!({"status": "running", "counter": 10}));
}

#[tokio::test]
async fn scheduler_tenant_quota_enforcement() {
    let scheduler = LocalScheduler::with_tenant_quota(TenantId::new("tenant-a"), 1).await;

    let actor1 = ScheduledActor {
        actor_id: ActorId::new("agent-1"),
        tenant_id: TenantId::new("tenant-a"),
        state: ActorState::Idle,
        priority: MessagePriority::Command,
    };
    let actor2 = ScheduledActor {
        actor_id: ActorId::new("agent-2"),
        tenant_id: TenantId::new("tenant-a"),
        state: ActorState::Idle,
        priority: MessagePriority::Command,
    };

    scheduler.schedule(actor1.clone()).await.unwrap();
    scheduler.schedule(actor2.clone()).await.unwrap();

    scheduler.mark_runnable(&actor1.actor_id).await.unwrap();
    let result = scheduler.mark_runnable(&actor2.actor_id).await;
    assert!(result.is_err()); // 应该被配额限制拒绝
}

#[tokio::test]
async fn dead_letter_queue_for_poison_messages() {
    let mailbox = InMemoryMailbox::with_poison_threshold(10, 2);
    let actor_id = ActorId::new("agent-1");

    // 模拟毒消息
    let msg = ActorMessage {
        to: actor_id.clone(),
        from: ActorId::new("attacker"),
        priority: MessagePriority::Command,
        idempotency_key: Arc::from("poison-1"),
        meta: Arc::new(CausalMeta::root(TenantId::new("tenant-a"))),
        payload: MessagePayload::UserInput { content: "malicious".to_string() },
    };

    // 移动到死信队列
    mailbox.move_to_dead_letter(msg, "processing failed".to_string()).await;

    // 验证死信队列
    let dead_letters = mailbox.get_dead_letters(&actor_id).await;
    assert_eq!(dead_letters.len(), 1);
    assert_eq!(dead_letters[0].reason, "processing failed");

    // 验证毒消息计数
    assert_eq!(mailbox.poison_count(&actor_id).await, 1);
}

#[tokio::test]
async fn capability_authorization_in_tool_flow() {
    let broker = Arc::new(Mutex::new(CapabilityBroker::default()));
    let tenant_id = TenantId::new("tenant-a");
    let tool_actor_id = ActorId::new("tool-1");

    // 申请能力
    let capability = broker.lock().await.issue_tool_call(
        tenant_id.clone(),
        tool_actor_id.clone(),
        "echo",
        time::Duration::minutes(5),
    ).unwrap();

    // 验证能力已授予
    assert_eq!(capability.tenant_id, tenant_id);
    assert_eq!(capability.subject, tool_actor_id);
}
