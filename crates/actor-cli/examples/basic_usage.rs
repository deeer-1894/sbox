use actor_cli::runtime::{RuntimeContext, RuntimeConfig};
use api_types::*;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let ctx = RuntimeContext::new(RuntimeConfig::default());

    // 创建 Actor
    ctx.create_actor(ActorId::new("agent-1"), ActorKind::Agent).await?;
    println!("Created actor: agent-1");

    // 发送消息
    let message = ActorMessage {
        to: ActorId::new("agent-1"),
        from: ActorId::new("session-1"),
        priority: MessagePriority::Command,
        idempotency_key: Arc::from("example-key-1"),
        meta: Arc::new(CausalMeta::root(TenantId::new("default"))),
        payload: MessagePayload::UserInput { content: "hello".to_string() },
    };

    ctx.send_message(message).await?;
    println!("Message sent to agent-1");

    // 查看邮箱状态
    let depth = ctx.mailbox_depth(&ActorId::new("agent-1")).await;
    println!("Mailbox depth: {}", depth);

    // 列出 Actor
    let actors = ctx.list_actors().await;
    println!("Actors: {:?}", actors);

    Ok(())
}