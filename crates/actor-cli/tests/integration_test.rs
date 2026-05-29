use actor_cli::runtime::{RuntimeConfig, RuntimeContext};
use api_types::*;
use std::sync::Arc;

#[tokio::test]
async fn test_create_and_list_actor() {
    let ctx = RuntimeContext::new(RuntimeConfig::default());

    ctx.create_actor(ActorId::new("test-1"), ActorKind::Agent)
        .await
        .unwrap();

    let actors = ctx.list_actors().await;
    assert_eq!(actors.len(), 1);
    assert_eq!(actors[0], ActorId::new("test-1"));
}

#[tokio::test]
async fn test_send_message() {
    let ctx = RuntimeContext::new(RuntimeConfig::default());

    ctx.create_actor(ActorId::new("agent-1"), ActorKind::Agent)
        .await
        .unwrap();

    let message = ActorMessage {
        to: ActorId::new("agent-1"),
        from: ActorId::new("session-1"),
        priority: MessagePriority::Command,
        idempotency_key: Arc::from("test-key-1"),
        meta: Arc::new(CausalMeta::root(TenantId::new("default"))),
        payload: MessagePayload::UserInput {
            content: "hello".to_string(),
        },
    };

    ctx.send_message(message).await.unwrap();

    let depth = ctx.mailbox_depth(&ActorId::new("agent-1")).await;
    assert_eq!(depth, 1);
}

#[tokio::test]
async fn test_checkpoint() {
    let ctx = RuntimeContext::new(RuntimeConfig::default());

    ctx.create_actor(ActorId::new("agent-1"), ActorKind::Agent)
        .await
        .unwrap();

    ctx.save_checkpoint(&ActorId::new("agent-1"))
        .await
        .unwrap();

    let snapshot = ctx
        .load_checkpoint(&ActorId::new("agent-1"))
        .await
        .unwrap();
    assert_eq!(snapshot.actor_id, ActorId::new("agent-1"));
}
