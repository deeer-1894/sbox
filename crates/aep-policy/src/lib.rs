//! Cedar-backed policy evaluation for tool intents.

use cedar_policy::{Authorizer, Context, Decision, Entities, PolicySet, Request};
use std::sync::OnceLock;

/// Embedded tool authorization policy (default-deny allowlist).
const POLICY_SRC: &str = include_str!("../policies/tools.cedar");

/// Parse the embedded policy once and reuse it (parsing on every request is
/// wasteful). The policy is a compile-time const, validated by the unit tests,
/// so a parse failure here is a programming error.
fn policies() -> &'static PolicySet {
    static POLICIES: OnceLock<PolicySet> = OnceLock::new();
    POLICIES.get_or_init(|| POLICY_SRC.parse().expect("embedded tools.cedar is a valid policy set"))
}

/// The outcome of evaluating a tool intent against policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Permit,
    Deny(String),
}

/// Evaluate whether `agent_id` may call `tool_name`. Deterministic and infra-free.
pub fn evaluate(agent_id: &str, tool_name: &str) -> PolicyDecision {
    let principal = match format!("Agent::\"{agent_id}\"").parse() {
        Ok(p) => p,
        Err(e) => return PolicyDecision::Deny(format!("bad principal: {e}")),
    };
    let action = match r#"Action::"CallTool""#.parse() {
        Ok(a) => a,
        Err(e) => return PolicyDecision::Deny(format!("bad action: {e}")),
    };
    let resource = match format!("Tool::\"{tool_name}\"").parse() {
        Ok(r) => r,
        Err(e) => return PolicyDecision::Deny(format!("bad resource: {e}")),
    };
    let request = match Request::new(principal, action, resource, Context::empty(), None) {
        Ok(r) => r,
        Err(e) => return PolicyDecision::Deny(format!("bad request: {e}")),
    };
    let answer = Authorizer::new().is_authorized(&request, policies(), &Entities::empty());
    match answer.decision() {
        Decision::Allow => PolicyDecision::Permit,
        Decision::Deny => PolicyDecision::Deny(format!("denied tool '{tool_name}' by policy")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permits_allowlisted_tool() {
        assert_eq!(evaluate("agent-1", "echo"), PolicyDecision::Permit);
        assert_eq!(evaluate("agent-1", "upper"), PolicyDecision::Permit);
    }

    #[test]
    fn denies_unlisted_tool() {
        match evaluate("agent-1", "shell") {
            PolicyDecision::Deny(reason) => assert!(!reason.is_empty()),
            other => panic!("expected Deny, got {other:?}"),
        }
    }
}
