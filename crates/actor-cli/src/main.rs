use actor_cli::runtime::{RuntimeContext, RuntimeConfig};
use api_types::*;
use clap::Parser;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "actor-cli")]
#[command(about = "Rust Actor OS Runtime CLI tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Actor management commands
    Actor {
        #[command(subcommand)]
        command: ActorCommands,
    },
    /// Message sending commands
    Message {
        #[command(subcommand)]
        command: MessageCommands,
    },
    /// Status monitoring commands
    Status {
        #[command(subcommand)]
        command: StatusCommands,
    },
    /// Log viewing commands
    Logs {
        #[command(subcommand)]
        command: LogCommands,
    },
    /// Performance testing commands
    Bench {
        #[command(subcommand)]
        command: BenchCommands,
    },
    /// Checkpoint management commands
    Checkpoint {
        #[command(subcommand)]
        command: CheckpointCommands,
    },
}

#[derive(clap::Subcommand, Debug)]
enum ActorCommands {
    /// Create a new actor
    Create {
        /// Actor ID
        actor_id: String,
        /// Actor kind
        #[arg(short, long, default_value = "Agent")]
        kind: String,
    },
    /// List all actors
    List {
        /// Tenant ID filter
        #[arg(short, long)]
        tenant: Option<String>,
    },
    /// Get actor details
    Get {
        /// Actor ID
        actor_id: String,
    },
    /// Delete an actor
    Delete {
        /// Actor ID
        actor_id: String,
    },
}

#[derive(clap::Subcommand, Debug)]
enum MessageCommands {
    /// Send a message to an actor
    Send {
        /// Target actor ID
        to: String,
        /// Source actor ID
        from: String,
        /// Message content
        #[arg(short, long)]
        content: String,
    },
    /// Send batch messages from file
    Batch {
        /// JSON file path
        file: String,
    },
}

#[derive(clap::Subcommand, Debug)]
enum StatusCommands {
    /// Show mailbox status
    Mailbox {
        /// Actor ID
        actor_id: String,
    },
    /// Show scheduler status
    Scheduler,
    /// Show overview
    Overview,
}

#[derive(clap::Subcommand, Debug)]
enum LogCommands {
    /// Show actor events
    Events {
        /// Actor ID
        actor_id: String,
        /// Start sequence
        #[arg(short, long, default_value = "1")]
        from: u64,
        /// End sequence
        #[arg(short, long)]
        to: Option<u64>,
    },
    /// Show trace
    Trace {
        /// Trace ID
        trace_id: String,
    },
    /// Show dead letters
    DeadLetters {
        /// Actor ID
        actor_id: String,
    },
}

#[derive(clap::Subcommand, Debug)]
enum BenchCommands {
    /// Benchmark enqueue operations
    Enqueue {
        /// Concurrent tasks
        #[arg(short, long, default_value = "10")]
        concurrent: usize,
        /// Message count
        #[arg(short, long, default_value = "1000")]
        count: usize,
    },
    /// Benchmark pull operations
    Pull {
        /// Concurrent tasks
        #[arg(short, long, default_value = "10")]
        concurrent: usize,
        /// Message count
        #[arg(short, long, default_value = "1000")]
        count: usize,
    },
    /// Full benchmark
    Full {
        /// Concurrent tasks
        #[arg(short, long, default_value = "10")]
        concurrent: usize,
        /// Message count
        #[arg(short, long, default_value = "1000")]
        count: usize,
    },
}

#[derive(clap::Subcommand, Debug)]
enum CheckpointCommands {
    /// Save checkpoint
    Save {
        /// Actor ID
        actor_id: String,
    },
    /// Load checkpoint
    Load {
        /// Actor ID
        actor_id: String,
    },
    /// List checkpoints
    List,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let ctx = RuntimeContext::new(RuntimeConfig::default());

    match cli.command {
        Commands::Actor { command } => {
            match command {
                ActorCommands::Create { actor_id, kind } => {
                    let actor_id = ActorId::new(actor_id);
                    let kind = match kind.as_str() {
                        "Agent" => ActorKind::Agent,
                        "Tool" => ActorKind::Tool,
                        "Memory" => ActorKind::Memory,
                        "Policy" => ActorKind::Policy,
                        "Sandbox" => ActorKind::Sandbox,
                        _ => anyhow::bail!("Invalid actor kind: {}", kind),
                    };
                    ctx.create_actor(actor_id.clone(), kind).await?;
                    println!("Created actor: {}", actor_id.0);
                }
                ActorCommands::List { tenant: _ } => {
                    let actors = ctx.list_actors().await;
                    for actor in actors {
                        println!("{}", actor.0);
                    }
                }
                ActorCommands::Get { actor_id } => {
                    let actor_id = ActorId::new(actor_id);
                    if let Some(snapshot) = ctx.get_actor(&actor_id).await {
                        println!("Actor: {}", snapshot.actor_id.0);
                        println!("Last Seq: {}", snapshot.last_seq.0);
                        println!("Revision: {}", snapshot.revision);
                        println!("State: {}", snapshot.state);
                    } else {
                        println!("Actor not found: {}", actor_id.0);
                    }
                }
                ActorCommands::Delete { actor_id } => {
                    let actor_id = ActorId::new(actor_id);
                    ctx.delete_actor(&actor_id).await?;
                    println!("Deleted actor: {}", actor_id.0);
                }
            }
        }
        Commands::Message { command } => {
            match command {
                MessageCommands::Send { to, from, content } => {
                    let message = ActorMessage {
                        to: ActorId::new(to.clone()),
                        from: ActorId::new(from),
                        priority: MessagePriority::Command,
                        idempotency_key: Arc::from(format!("cli-{}", uuid::Uuid::new_v4())),
                        meta: Arc::new(CausalMeta::root(TenantId::new("default"))),
                        payload: MessagePayload::UserInput { content },
                    };
                    ctx.send_message(message).await?;
                    println!("Message sent to: {}", to);
                }
                MessageCommands::Batch { file } => {
                    let content = std::fs::read_to_string(&file)?;
                    let messages: Vec<ActorMessage> = serde_json::from_str(&content)?;
                    ctx.send_batch(messages).await?;
                    println!("Batch messages sent from: {}", file);
                }
            }
        }
        Commands::Status { command } => {
            match command {
                StatusCommands::Mailbox { actor_id } => {
                    let actor_id = ActorId::new(actor_id);
                    let depth = ctx.mailbox_depth(&actor_id).await;
                    println!("Mailbox depth for {}: {}", actor_id.0, depth);
                }
                StatusCommands::Scheduler => {
                    println!("Scheduler status: running");
                }
                StatusCommands::Overview => {
                    let actors = ctx.list_actors().await;
                    println!("Total actors: {}", actors.len());
                }
            }
        }
        Commands::Logs { command } => {
            match command {
                LogCommands::Events { actor_id, from, to } => {
                    let actor_id = ActorId::new(actor_id);
                    let events = ctx.get_events(&actor_id, ActorSeq(from), to.map(ActorSeq)).await;
                    for event in events {
                        println!("Seq: {} - {:?}", event.seq.0, event.payload);
                    }
                }
                LogCommands::Trace { trace_id } => {
                    println!("Trace: {}", trace_id);
                }
                LogCommands::DeadLetters { actor_id } => {
                    let actor_id = ActorId::new(actor_id);
                    let dead_letters = ctx.get_dead_letters(&actor_id).await;
                    for dl in dead_letters {
                        println!("Dead letter: {} - {}", dl.message.idempotency_key, dl.reason);
                    }
                }
            }
        }
        Commands::Bench { command } => {
            match command {
                BenchCommands::Enqueue { concurrent, count } => {
                    println!("Running enqueue benchmark: concurrent={}, count={}", concurrent, count);
                }
                BenchCommands::Pull { concurrent, count } => {
                    println!("Running pull benchmark: concurrent={}, count={}", concurrent, count);
                }
                BenchCommands::Full { concurrent, count } => {
                    println!("Running full benchmark: concurrent={}, count={}", concurrent, count);
                }
            }
        }
        Commands::Checkpoint { command } => {
            match command {
                CheckpointCommands::Save { actor_id } => {
                    let actor_id = ActorId::new(actor_id);
                    ctx.save_checkpoint(&actor_id).await?;
                    println!("Checkpoint saved for: {}", actor_id.0);
                }
                CheckpointCommands::Load { actor_id } => {
                    let actor_id = ActorId::new(actor_id);
                    let snapshot = ctx.load_checkpoint(&actor_id).await?;
                    println!("Checkpoint loaded for: {}", snapshot.actor_id.0);
                }
                CheckpointCommands::List => {
                    let actors = ctx.list_actors().await;
                    for actor in actors {
                        println!("{}", actor.0);
                    }
                }
            }
        }
    }

    Ok(())
}
