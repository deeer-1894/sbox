//! Restate adapter for the agent/tool domain logic.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use aep_audit::{capability_for, causal_chain, AuditEvent};
use aep_memory::{MemoryEntry, TrustLabel};
use aep_capability::{sign, Action, Capability, Resource};
use aep_domain::{admit, decide, AgentReply, Decision, ToolOutput, ToolRequest, UserInput};
use aep_policy::{evaluate, PolicyDecision};
use axum::{extract::State, routing::{get, post}, Json as AxumJson, Router};
use restate_sdk::prelude::*;

/// Address of the in-process "external" counter the tool side effect mutates.
const COUNTER_BASE: &str = "http://localhost:9090";

/// The audited tool WASM (WAT form). Its digest is pinned in TRUSTED_ECHO_DIGEST;
/// ToolService verifies it before the sandbox runs it.
const TOOL_WAT: &str = r#"
                    (module
                      (import "env" "host_sink" (func $sink (result i32)))
                      (func (export "run") (result i32) call $sink))
                "#;

/// Pinned SHA-256 of the audited TOOL_WAT. Regenerate if the artifact changes:
///   the value equals aep_supplychain::sha256_hex(TOOL_WAT.as_bytes()).
const TRUSTED_ECHO_DIGEST: &str =
    "ab0c7fdb3a8908e6b70afe42e8c5738ccfbd028648437fa113e45d59008a3a5c";

/// Process-shared capability signing secret. Single-node Phase 1 only; production
/// uses an asymmetric key so verifiers never hold the signing key.
pub fn cap_secret() -> Vec<u8> {
    std::env::var("AEP_CAP_SECRET").unwrap_or_else(|_| "dev-insecure-secret".into()).into_bytes()
}

/// Current Unix time (seconds). Call only inside `ctx.run` so the value is
/// journaled and stable across replay. The `?` converts the SystemTime error
/// (TerminalError) into the HandlerError that `ctx.run` expects.
fn now_unix() -> Result<u64, HandlerError> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| TerminalError::new(e.to_string()))?
        .as_secs())
}

/// A tool invocation carrying its authorizing capability token.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ToolCall {
    pub request: ToolRequest,
    pub capability_token: String,
}

/// ToolService: keyed by tool name. State key = invocation_id -> ToolOutput.
#[restate_sdk::object]
pub trait ToolService {
    async fn run(call: Json<ToolCall>) -> Result<Json<ToolOutput>, HandlerError>;
}

pub struct ToolServiceImpl;

impl ToolService for ToolServiceImpl {
    async fn run(
        &self,
        ctx: ObjectContext<'_>,
        Json(ToolCall { request: req, capability_token }): Json<ToolCall>,
    ) -> Result<Json<ToolOutput>, HandlerError> {
        // The capability is the sole authority. Deterministic time via ctx.run.
        let now: u64 = ctx.run(|| async { now_unix() }).await?;
        let cap = aep_capability::verify(&cap_secret(), &capability_token, now)
            .map_err(|e| TerminalError::new(format!("capability rejected: {e}")))?;
        cap.authorize(Action::Call, &Resource::Tool { name: req.tool_name.clone() })
            .map_err(|e| TerminalError::new(format!("capability not scoped to tool: {e}")))?;

        // OpenTelemetry-semantic span for this actor turn.
        aep_telemetry::capture().record(&req.invocation_id, "ToolService");
        tracing::info!(trace_id = %req.invocation_id, actor = "ToolService", kind = "turn", "actor turn");

        // Supply-chain: the tool artifact must match the pinned trusted digest
        // before the sandbox runs it. (Production loads a signed manifest.)
        let mut registry = aep_supplychain::Registry::default();
        registry.register("echo-tool", TRUSTED_ECHO_DIGEST);
        registry
            .verify("echo-tool", TOOL_WAT.as_bytes())
            .map_err(|e| TerminalError::new(format!("supply-chain verification failed: {e}")))?;

        // --- Phase 0 side-effect boundary (unchanged) ---
        let existing = ctx.get::<Json<ToolOutput>>(&req.invocation_id).await?.map(|j| j.0);
        match decide(existing) {
            Decision::Reuse(output) => Ok(Json(output)),
            Decision::Execute => {
                let content = req.input.clone();
                // The tool runs as WASM (module-level TOOL_WAT); its only path to
                // the side effect is the capability-gated host_sink. The sandbox
                // links host_sink only if `cap` authorizes this tool's resource.
                let cap_for_tool = cap.clone();
                let required = Resource::Tool { name: req.tool_name.clone() };
                let count: u64 = ctx
                    .run(|| async move {
                        let n = tokio::task::spawn_blocking(move || {
                            aep_sandbox::run_tool(TOOL_WAT, &cap_for_tool, &required, || {
                                reqwest::blocking::Client::new()
                                    .post(format!("{COUNTER_BASE}/incr"))
                                    .send()
                                    .and_then(|r| r.text())
                                    .ok()
                                    .and_then(|t| t.trim().parse::<u64>().ok())
                                    .unwrap_or(0)
                            })
                        })
                        .await
                        .map_err(|e| TerminalError::new(format!("sandbox join: {e}")))?
                        .map_err(|e| TerminalError::new(format!("sandbox: {e}")))?;
                        Ok(n as u64)
                    })
                    .await?;
                let output = ToolOutput {
                    output: serde_json::json!({ "echo": content }),
                    exec_count: count,
                };
                ctx.set(&req.invocation_id, Json(output.clone()));
                Ok(Json(output))
            }
        }
    }
}

/// Default per-tenant concurrency limit when none is configured.
const DEFAULT_TENANT_LIMIT: u32 = 1000;

/// TenantService: keyed by tenant id. Durable in-flight counter + limit.
#[restate_sdk::object]
pub trait TenantService {
    async fn set_limit(max: u32) -> Result<(), HandlerError>;
    async fn acquire() -> Result<bool, HandlerError>;
    async fn release() -> Result<(), HandlerError>;
    async fn in_flight() -> Result<u32, HandlerError>;
}

pub struct TenantServiceImpl;

impl TenantService for TenantServiceImpl {
    async fn set_limit(&self, ctx: ObjectContext<'_>, max: u32) -> Result<(), HandlerError> {
        ctx.set("limit", max);
        Ok(())
    }

    async fn acquire(&self, ctx: ObjectContext<'_>) -> Result<bool, HandlerError> {
        let limit = ctx.get::<u32>("limit").await?.unwrap_or(DEFAULT_TENANT_LIMIT);
        let current = ctx.get::<u32>("current").await?.unwrap_or(0);
        if admit(current, limit) {
            ctx.set("current", current + 1);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn release(&self, ctx: ObjectContext<'_>) -> Result<(), HandlerError> {
        let current = ctx.get::<u32>("current").await?.unwrap_or(0);
        ctx.set("current", current.saturating_sub(1));
        Ok(())
    }

    async fn in_flight(&self, ctx: ObjectContext<'_>) -> Result<u32, HandlerError> {
        Ok(ctx.get::<u32>("current").await?.unwrap_or(0))
    }
}

/// AuditService: keyed by trace_id. Durable, idempotent audit sink + query API.
#[restate_sdk::object]
pub trait AuditService {
    async fn record(event: Json<AuditEvent>) -> Result<(), HandlerError>;
    async fn chain() -> Result<Json<Vec<AuditEvent>>, HandlerError>;
    async fn capability(invocation_id: Json<String>) -> Result<Json<Option<String>>, HandlerError>;
}

pub struct AuditServiceImpl;

impl AuditService for AuditServiceImpl {
    async fn record(&self, ctx: ObjectContext<'_>, Json(event): Json<AuditEvent>) -> Result<(), HandlerError> {
        let mut events = ctx.get::<Json<Vec<AuditEvent>>>("events").await?.map(|j| j.0).unwrap_or_default();
        // Idempotent: re-emission on replay is a no-op.
        if !events.iter().any(|e| e.message_id == event.message_id) {
            // Best-effort dual-write to ClickHouse (analytics sink). Restate state
            // is authoritative; a ClickHouse outage must not fail the request.
            let row = serde_json::json!({
                "trace_id": event.trace_id,
                "message_id": event.message_id,
                "causal_parent_id": event.causal_parent_id,
                "actor": event.actor,
                "kind": event.kind,
                "capability_id": event.capability_id,
                "invocation_id": event.invocation_id,
                "detail": event.detail.to_string(),
                "ts": event.ts,
            });
            events.push(event);
            ctx.set("events", Json(events));
            ctx.run(|| async move {
                let url = std::env::var("CLICKHOUSE_URL")
                    .unwrap_or_else(|_| "http://aep:aep@localhost:8123".to_string());
                let body = format!("INSERT INTO audit_events FORMAT JSONEachRow\n{row}");
                let _ = reqwest::Client::new().post(&url).body(body).send().await;
                Ok(())
            })
            .await?;
        }
        Ok(())
    }

    async fn chain(&self, ctx: ObjectContext<'_>) -> Result<Json<Vec<AuditEvent>>, HandlerError> {
        let events = ctx.get::<Json<Vec<AuditEvent>>>("events").await?.map(|j| j.0).unwrap_or_default();
        Ok(Json(causal_chain(&events)))
    }

    async fn capability(&self, ctx: ObjectContext<'_>, Json(invocation_id): Json<String>) -> Result<Json<Option<String>>, HandlerError> {
        let events = ctx.get::<Json<Vec<AuditEvent>>>("events").await?.map(|j| j.0).unwrap_or_default();
        Ok(Json(capability_for(&events, &invocation_id)))
    }
}

/// MemoryService: keyed by agent id. Durable tiered memory with trust labels.
#[restate_sdk::object]
pub trait MemoryService {
    async fn store(entry: Json<MemoryEntry>) -> Result<(), HandlerError>;
    async fn get(key: Json<String>) -> Result<Json<Option<MemoryEntry>>, HandlerError>;
    async fn by_trust(trust: Json<TrustLabel>) -> Result<Json<Vec<MemoryEntry>>, HandlerError>;
}

pub struct MemoryServiceImpl;

impl MemoryService for MemoryServiceImpl {
    async fn store(&self, ctx: ObjectContext<'_>, Json(entry): Json<MemoryEntry>) -> Result<(), HandlerError> {
        let mut entries = ctx.get::<Json<Vec<MemoryEntry>>>("entries").await?.map(|j| j.0).unwrap_or_default();
        // Upsert by key (idempotent on replay).
        if let Some(slot) = entries.iter_mut().find(|e| e.key == entry.key) {
            *slot = entry;
        } else {
            entries.push(entry);
        }
        ctx.set("entries", Json(entries));
        Ok(())
    }

    async fn get(&self, ctx: ObjectContext<'_>, Json(key): Json<String>) -> Result<Json<Option<MemoryEntry>>, HandlerError> {
        let entries = ctx.get::<Json<Vec<MemoryEntry>>>("entries").await?.map(|j| j.0).unwrap_or_default();
        Ok(Json(entries.into_iter().find(|e| e.key == key)))
    }

    async fn by_trust(&self, ctx: ObjectContext<'_>, Json(trust): Json<TrustLabel>) -> Result<Json<Vec<MemoryEntry>>, HandlerError> {
        let entries = ctx.get::<Json<Vec<MemoryEntry>>>("entries").await?.map(|j| j.0).unwrap_or_default();
        Ok(Json(entries.into_iter().filter(|e| e.trust == trust).collect()))
    }
}

/// The external counter sidecar. Not part of Restate's journal — its mutation is
/// exactly what must happen once per committed side effect.
#[derive(Clone, Default)]
pub struct Counter(Arc<AtomicU64>);

pub fn counter_router() -> Router {
    let state = Counter::default();
    Router::new()
        .route("/incr", post(|State(c): State<Counter>| async move {
            (c.0.fetch_add(1, Ordering::SeqCst) + 1).to_string()
        }))
        .route("/count", get(|State(c): State<Counter>| async move {
            AxumJson(c.0.load(Ordering::SeqCst))
        }))
        .route("/spans/:trace", get(|axum::extract::Path(trace): axum::extract::Path<String>| async move {
            AxumJson(aep_telemetry::capture().get(&trace))
        }))
        .with_state(state)
}

pub use agent::*;
mod agent {
    use super::*;
    use aep_domain::plan_user_input;
    use tracing::Instrument;

    /// AgentService: keyed by agent id. Evaluates policy, mints a capability on
    /// Permit, then calls ToolService with the capability token.
    #[restate_sdk::object]
    pub trait AgentService {
        async fn handle(input: Json<UserInput>) -> Result<Json<AgentReply>, HandlerError>;
    }

    pub struct AgentServiceImpl;

    impl AgentService for AgentServiceImpl {
        async fn handle(
            &self,
            ctx: ObjectContext<'_>,
            Json(input): Json<UserInput>,
        ) -> Result<Json<AgentReply>, HandlerError> {
            let tenant = input.tenant.clone().unwrap_or_else(|| "default".to_string());

            // Admission control: the cheapest rejection happens first.
            let admitted = ctx
                .object_client::<TenantServiceClient>(tenant.clone())
                .acquire()
                .call()
                .await?;
            if !admitted {
                return Ok(Json(AgentReply {
                    output: serde_json::Value::Null,
                    exec_count: 0,
                    denied: true,
                    reason: Some("tenant quota exceeded".to_string()),
                }));
            }

            // Run the real work in an OpenTelemetry span, then release the slot
            // regardless of outcome.
            let outcome = handle_inner(&ctx, &input)
                .instrument(tracing::info_span!(
                    "AgentService.handle",
                    trace_id = %input.idempotency_key,
                    actor = "AgentService"
                ))
                .await;
            ctx.object_client::<TenantServiceClient>(tenant)
                .release()
                .call()
                .await?;
            outcome.map(Json)
        }
    }

    /// The policy + capability + tool logic, separated so the quota slot can be
    /// released on every return path.
    async fn handle_inner(
        ctx: &ObjectContext<'_>,
        input: &UserInput,
    ) -> Result<AgentReply, HandlerError> {
        let agent_id = ctx.key().to_string();
        let req: ToolRequest = plan_user_input(input);
        let trace = req.invocation_id.clone();
        let now: u64 = ctx.run(|| async { now_unix() }).await?;

        // OpenTelemetry-semantic span for this actor turn.
        aep_telemetry::capture().record(&trace, "AgentService");
        for (k, v) in aep_telemetry::span_fields(&trace, "AgentService", "turn") {
            tracing::info!(field = k, value = %v, "span attribute");
        }
        tracing::info!(trace_id = %trace, actor = "AgentService", kind = "turn", "actor turn");

        // Root event.
        emit(ctx, &trace, "input", None, "AgentService", now, None, None,
             serde_json::json!({ "content": input.content })).await?;

        // Policy is evaluated independently of the model's intent.
        if let PolicyDecision::Deny(reason) = evaluate(&agent_id, &req.tool_name) {
            emit(ctx, &trace, "policy_deny", Some("input"), "AgentService", now, None, None,
                 serde_json::json!({ "reason": reason.clone() })).await?;
            return Ok(AgentReply {
                output: serde_json::Value::Null, exec_count: 0, denied: true, reason: Some(reason),
            });
        }
        emit(ctx, &trace, "policy_permit", Some("input"), "AgentService", now, None, None,
             serde_json::json!({ "tool": req.tool_name })).await?;

        // Mint a short-lived capability scoped to exactly this tool.
        let cap = Capability {
            id: format!("cap-{}", req.invocation_id),
            tenant: "default".into(),
            subject: agent_id,
            resource: Resource::Tool { name: req.tool_name.clone() },
            actions: vec![Action::Call],
            expires_at: now + 300, // 5 minutes
            policy_hash: "tools.cedar@v1".into(),
            audit_id: format!("aud-{}", req.invocation_id),
        };
        emit(ctx, &trace, "capability_minted", Some("policy_permit"), "AgentService", now,
             Some(cap.id.clone()), None, serde_json::json!({ "resource": req.tool_name })).await?;
        let token = sign(&cap_secret(), &cap);

        emit(ctx, &trace, "tool_requested", Some("capability_minted"), "AgentService", now,
             Some(cap.id.clone()), Some(req.invocation_id.clone()),
             serde_json::json!({ "tool": req.tool_name })).await?;

        let tool_key = req.tool_name.clone();
        let invocation_id = req.invocation_id.clone();
        let Json(out) = ctx
            .object_client::<ToolServiceClient>(tool_key)
            .run(Json(ToolCall { request: req, capability_token: token }))
            .call()
            .await?;

        emit(ctx, &trace, "tool_completed", Some("tool_requested"), "ToolService", now,
             None, Some(invocation_id), serde_json::json!({ "exec_count": out.exec_count })).await?;

        // Classify + sanitize the tool output before it enters agent memory.
        let raw = out
            .output
            .get("echo")
            .and_then(|e| e.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string();
        let (sanitized_value, was_sanitized) = aep_memory::sanitize(&raw);
        let entry = MemoryEntry {
            key: trace.clone(),
            value: sanitized_value,
            tier: aep_memory::MemoryTier::Operational,
            trust: aep_memory::classify_tool_output(),
            source_capability: Some(cap.id.clone()),
            sanitized: was_sanitized,
            ts: now,
        };
        ctx.object_client::<MemoryServiceClient>(ctx.key().to_string())
            .store(Json(entry))
            .call()
            .await?;

        Ok(AgentReply {
            output: out.output,
            exec_count: out.exec_count,
            denied: false,
            reason: None,
        })
    }

    /// Emit one audit event to AuditService (keyed by trace). message_id is
    /// deterministic ("{trace}:{kind}"), so replay re-emits identically and dedups.
    #[allow(clippy::too_many_arguments)]
    async fn emit(
        ctx: &ObjectContext<'_>,
        trace: &str,
        kind: &str,
        parent_kind: Option<&str>,
        actor: &str,
        ts: u64,
        capability_id: Option<String>,
        invocation_id: Option<String>,
        detail: serde_json::Value,
    ) -> Result<(), HandlerError> {
        let event = AuditEvent {
            trace_id: trace.to_string(),
            message_id: format!("{trace}:{kind}"),
            causal_parent_id: parent_kind.map(|p| format!("{trace}:{p}")),
            actor: actor.to_string(),
            kind: kind.to_string(),
            capability_id,
            invocation_id,
            detail,
            ts,
        };
        ctx.object_client::<AuditServiceClient>(trace.to_string())
            .record(Json(event))
            .call()
            .await?;
        Ok(())
    }
}
