use api_types::{ActorId, CapabilityId, TenantId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resource {
    Tool { name: String },
    Network { domain: String },
    File { path_prefix: String },
    Secret { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Call,
    Read,
    Write,
    Spawn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub id: CapabilityId,
    pub tenant_id: TenantId,
    pub subject: ActorId,
    pub resource: Resource,
    pub actions: Vec<Action>,
    pub expires_at: OffsetDateTime,
    pub audit_id: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CapabilityError {
    #[error("policy denied: {0}")]
    PolicyDenied(String),
    #[error("capability not found")]
    NotFound,
    #[error("capability expired")]
    Expired,
    #[error("capability does not authorize action")]
    Unauthorized,
}

#[derive(Debug, Default)]
pub struct CapabilityBroker {
    issued: HashMap<CapabilityId, Capability>,
}

impl CapabilityBroker {
    pub fn issue_tool_call(
        &mut self,
        tenant_id: TenantId,
        subject: ActorId,
        tool_name: &str,
        ttl: Duration,
    ) -> Result<Capability, CapabilityError> {
        if tool_name.trim().is_empty() {
            return Err(CapabilityError::PolicyDenied("tool name is empty".to_string()));
        }
        let capability = Capability {
            id: CapabilityId(Uuid::new_v4()),
            tenant_id,
            subject,
            resource: Resource::Tool { name: tool_name.to_string() },
            actions: vec![Action::Call],
            expires_at: OffsetDateTime::now_utc() + ttl,
            audit_id: Uuid::new_v4().to_string(),
        };
        self.issued.insert(capability.id.clone(), capability.clone());
        Ok(capability)
    }

    pub fn authorize(&self, capability_id: &CapabilityId, action: Action, resource: &Resource) -> Result<(), CapabilityError> {
        let capability = self.issued.get(capability_id).ok_or(CapabilityError::NotFound)?;
        if capability.expires_at <= OffsetDateTime::now_utc() {
            return Err(CapabilityError::Expired);
        }
        if &capability.resource != resource || !capability.actions.contains(&action) {
            return Err(CapabilityError::Unauthorized);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issues_and_authorizes_tool_call_capability() {
        let mut broker = CapabilityBroker::default();
        let capability = broker
            .issue_tool_call(
                TenantId::new("tenant-a"),
                ActorId::new("agent-1"),
                "echo",
                Duration::minutes(5),
            )
            .unwrap();

        broker
            .authorize(&capability.id, Action::Call, &Resource::Tool { name: "echo".to_string() })
            .unwrap();
    }

    #[test]
    fn rejects_wrong_resource() {
        let mut broker = CapabilityBroker::default();
        let capability = broker
            .issue_tool_call(
                TenantId::new("tenant-a"),
                ActorId::new("agent-1"),
                "echo",
                Duration::minutes(5),
            )
            .unwrap();

        let err = broker
            .authorize(&capability.id, Action::Call, &Resource::Tool { name: "shell".to_string() })
            .unwrap_err();

        assert_eq!(err, CapabilityError::Unauthorized);
    }
}
