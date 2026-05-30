//! Causal audit event model and query logic (pure, infra-free).

use serde::{Deserialize, Serialize};

/// One audit fact in a trace. Events form a causal chain via
/// `message_id` <- `causal_parent_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub trace_id: String,
    pub message_id: String,
    pub causal_parent_id: Option<String>,
    pub actor: String,
    pub kind: String,
    #[serde(default)]
    pub capability_id: Option<String>,
    #[serde(default)]
    pub invocation_id: Option<String>,
    #[serde(default)]
    pub detail: serde_json::Value,
    pub ts: u64,
}

/// Order a trace's events causally: start at the root (no parent) and follow
/// each event to the child whose parent is its message_id.
pub fn causal_chain(events: &[AuditEvent]) -> Vec<AuditEvent> {
    let mut ordered = Vec::new();
    let mut current = events.iter().find(|e| e.causal_parent_id.is_none());
    while let Some(ev) = current {
        ordered.push(ev.clone());
        let id = &ev.message_id;
        current = events
            .iter()
            .find(|e| e.causal_parent_id.as_deref() == Some(id.as_str()));
    }
    ordered
}

/// Which capability authorized a given tool invocation in this trace.
pub fn capability_for(events: &[AuditEvent], invocation_id: &str) -> Option<String> {
    events
        .iter()
        .find(|e| e.kind == "tool_requested" && e.invocation_id.as_deref() == Some(invocation_id))
        .and_then(|e| e.capability_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(msg: &str, parent: Option<&str>, kind: &str) -> AuditEvent {
        AuditEvent {
            trace_id: "t".into(),
            message_id: msg.into(),
            causal_parent_id: parent.map(|s| s.to_string()),
            actor: "AgentService".into(),
            kind: kind.into(),
            capability_id: None,
            invocation_id: None,
            detail: serde_json::Value::Null,
            ts: 0,
        }
    }

    #[test]
    fn orders_a_trace_from_root() {
        let events = vec![
            ev("t:tool_req", Some("t:capability"), "tool_requested"),
            ev("t:input", None, "input"),
            ev("t:capability", Some("t:policy"), "capability_minted"),
            ev("t:policy", Some("t:input"), "policy_permit"),
        ];
        let chain: Vec<String> = causal_chain(&events).into_iter().map(|e| e.kind).collect();
        assert_eq!(chain, vec!["input", "policy_permit", "capability_minted", "tool_requested"]);
    }

    #[test]
    fn finds_authorizing_capability() {
        let mut req = ev("t:tool_req", Some("t:capability"), "tool_requested");
        req.invocation_id = Some("inv-1".into());
        req.capability_id = Some("cap-1".into());
        let events = vec![ev("t:input", None, "input"), req];
        assert_eq!(capability_for(&events, "inv-1"), Some("cap-1".into()));
        assert_eq!(capability_for(&events, "nope"), None);
    }
}
