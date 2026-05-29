use api_types::{ActorId, CausalMeta};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceFields {
    pub tenant_id: String,
    pub actor_id: String,
    pub trace_id: String,
    pub message_id: String,
    pub causal_parent_id: Option<String>,
}

pub fn fields_for(actor_id: &ActorId, meta: &CausalMeta) -> TraceFields {
    TraceFields {
        tenant_id: meta.tenant_id.0.to_string(),
        actor_id: actor_id.0.to_string(),
        trace_id: meta.trace_id.0.to_string(),
        message_id: meta.message_id.0.to_string(),
        causal_parent_id: meta.causal_parent_id.as_ref().map(|id| id.0.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_types::TenantId;

    #[test]
    fn exposes_required_causal_fields() {
        let actor_id = ActorId::new("agent-1");
        let meta = CausalMeta::root(TenantId::new("tenant-a"));

        let fields = fields_for(&actor_id, &meta);

        assert_eq!(fields.tenant_id, "tenant-a");
        assert_eq!(fields.actor_id, "agent-1");
        assert_eq!(fields.trace_id, meta.trace_id.0.to_string());
        assert_eq!(fields.message_id, meta.message_id.0.to_string());
        assert_eq!(fields.causal_parent_id, None);
    }
}
