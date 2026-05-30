//! Pure, deterministic agent/tool domain logic. No infrastructure dependencies.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A user message delivered to an AgentService. `idempotency_key` is the dedup
/// anchor: the same key must drive the same tool invocation. `requested_tool`
/// stands in for the model's chosen tool; absent means the default `echo`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserInput {
    pub idempotency_key: String,
    pub content: String,
    #[serde(default)]
    pub requested_tool: Option<String>,
    #[serde(default)]
    pub tenant: Option<String>,
}

/// Admission rule for a per-tenant concurrency quota: admit while strictly
/// below the limit.
pub fn admit(in_flight: u32, limit: u32) -> bool {
    in_flight < limit
}

/// A request crossing the ToolService side-effect boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolRequest {
    pub invocation_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub input_hash: String,
}

/// The recorded result of a tool side effect. `exec_count` is the observed value
/// from the external counter at the moment the side effect ran — it lets tests
/// see whether the effect was executed or replayed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolOutput {
    pub output: serde_json::Value,
    pub exec_count: u64,
}

/// What the agent returns to the caller. On a policy denial, `denied` is true,
/// `reason` explains, and `output`/`exec_count` carry their zero values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentReply {
    pub output: serde_json::Value,
    pub exec_count: u64,
    #[serde(default)]
    pub denied: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

/// SHA-256 hex of a canonical JSON value.
pub fn hash_input(value: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Outcome of consulting the durable journal before a tool side effect.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// A committed ToolCompleted exists; reuse it, do not re-run the side effect.
    Reuse(ToolOutput),
    /// No completion recorded; run the side effect and record it.
    Execute,
}

/// Decide whether to run a tool side effect, given any previously committed
/// completion for the same invocation id.
pub fn decide(existing_completion: Option<ToolOutput>) -> Decision {
    match existing_completion {
        Some(output) => Decision::Reuse(output),
        None => Decision::Execute,
    }
}

/// Turn a user message into a tool request. Deterministic: the invocation id is
/// the user's idempotency key, so resends collapse to one tool invocation.
/// (Phase 0 always routes to the `echo` tool.)
pub fn plan_user_input(input: &UserInput) -> ToolRequest {
    let payload = serde_json::json!({ "content": input.content });
    ToolRequest {
        invocation_id: input.idempotency_key.clone(),
        tool_name: input.requested_tool.clone().unwrap_or_else(|| "echo".to_string()),
        input_hash: hash_input(&payload),
        input: payload,
    }
}

#[cfg(test)]
mod hash_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hash_is_stable_and_distinguishes_inputs() {
        let a = hash_input(&json!({"content": "hello"}));
        let b = hash_input(&json!({"content": "hello"}));
        let c = hash_input(&json!({"content": "world"}));
        assert_eq!(a, b, "same input must hash identically");
        assert_ne!(a, c, "different input must hash differently");
        assert_eq!(a.len(), 64, "sha256 hex is 64 chars");
    }
}

#[cfg(test)]
mod decide_tests {
    use super::*;
    use serde_json::json;

    fn output() -> ToolOutput {
        ToolOutput { output: json!({"echo": "hi"}), exec_count: 1 }
    }

    #[test]
    fn reuses_when_completion_exists() {
        let d = decide(Some(output()));
        assert_eq!(d, Decision::Reuse(output()));
    }

    #[test]
    fn executes_when_no_completion() {
        let d = decide(None);
        assert_eq!(d, Decision::Execute);
    }
}

#[cfg(test)]
mod admit_tests {
    use super::*;

    #[test]
    fn admits_below_limit_and_rejects_at_limit() {
        assert!(admit(0, 1), "first slot admitted");
        assert!(!admit(1, 1), "at limit -> rejected");
        assert!(admit(4, 5), "below limit admitted");
        assert!(!admit(5, 5), "at limit rejected");
        assert!(!admit(0, 0), "zero limit rejects everything");
    }
}

#[cfg(test)]
mod requested_tool_tests {
    use super::*;

    #[test]
    fn plan_routes_to_requested_tool_when_present() {
        let input = UserInput {
            idempotency_key: "k-1".into(),
            content: "hi".into(),
            requested_tool: Some("shell".into()),
            tenant: None,
        };
        assert_eq!(plan_user_input(&input).tool_name, "shell");
    }

    #[test]
    fn plan_defaults_to_echo_when_absent() {
        let input = UserInput {
            idempotency_key: "k-2".into(),
            content: "hi".into(),
            requested_tool: None,
            tenant: None,
        };
        assert_eq!(plan_user_input(&input).tool_name, "echo");
    }
}

#[cfg(test)]
mod plan_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plan_is_deterministic_and_keyed_by_idempotency() {
        let input = UserInput { idempotency_key: "k-1".into(), content: "hello".into(), requested_tool: None, tenant: None };
        let a = plan_user_input(&input);
        let b = plan_user_input(&input);
        assert_eq!(a, b, "planning must be deterministic");
        assert_eq!(a.invocation_id, "k-1", "invocation id anchors on idempotency key");
        assert_eq!(a.tool_name, "echo");
        assert_eq!(a.input, json!({"content": "hello"}));
        assert_eq!(a.input_hash, hash_input(&json!({"content": "hello"})));
    }
}
