//! Short-lived, scoped capability tokens (HMAC-SHA256, stateless verification).

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

/// A protected resource a capability may authorize.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resource {
    Tool { name: String },
    Network { domain: String },
    File { path_prefix: String },
    Secret { name: String },
}

/// An action a capability may permit on a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Call,
    Read,
    Write,
    Spawn,
}

/// The claims carried by a capability token. `expires_at` is a Unix timestamp
/// (seconds); time is injected by callers, never read ambiently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    pub tenant: String,
    pub subject: String,
    pub resource: Resource,
    pub actions: Vec<Action>,
    pub expires_at: u64,
    pub policy_hash: String,
    pub audit_id: String,
}

impl Capability {
    /// Check this capability authorizes `action` on `resource`. Expiry is checked
    /// at `verify` time, not here.
    pub fn authorize(&self, action: Action, resource: &Resource) -> Result<(), CapError> {
        if &self.resource != resource || !self.actions.contains(&action) {
            return Err(CapError::Unauthorized);
        }
        Ok(())
    }
}

/// Errors from minting or verifying capability tokens.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CapError {
    #[error("malformed capability token")]
    Malformed,
    #[error("bad signature")]
    BadSignature,
    #[error("capability expired")]
    Expired,
    #[error("capability does not authorize this action/resource")]
    Unauthorized,
}

fn mac(secret: &[u8], claims_b64: &str) -> Vec<u8> {
    let mut m = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    m.update(claims_b64.as_bytes());
    m.finalize().into_bytes().to_vec()
}

/// Produce a `base64(claims).base64(hmac)` capability token.
pub fn sign(secret: &[u8], cap: &Capability) -> String {
    let claims_json = serde_json::to_vec(cap).expect("Capability serializes");
    let claims_b64 = URL_SAFE_NO_PAD.encode(claims_json);
    let mac_b64 = URL_SAFE_NO_PAD.encode(mac(secret, &claims_b64));
    format!("{claims_b64}.{mac_b64}")
}

/// Verify signature and expiry, returning the claims. Does not check authorization
/// against a specific action/resource — call `Capability::authorize` for that.
pub fn verify(secret: &[u8], token: &str, now: u64) -> Result<Capability, CapError> {
    let (claims_b64, mac_b64) = token.split_once('.').ok_or(CapError::Malformed)?;
    let provided = URL_SAFE_NO_PAD.decode(mac_b64).map_err(|_| CapError::Malformed)?;
    // Constant-time comparison via the HMAC crate.
    let mut m = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    m.update(claims_b64.as_bytes());
    m.verify_slice(&provided).map_err(|_| CapError::BadSignature)?;
    let claims_json = URL_SAFE_NO_PAD.decode(claims_b64).map_err(|_| CapError::Malformed)?;
    let cap: Capability = serde_json::from_slice(&claims_json).map_err(|_| CapError::Malformed)?;
    if now >= cap.expires_at {
        return Err(CapError::Expired);
    }
    Ok(cap)
}

#[cfg(test)]
mod type_tests {
    use super::*;

    #[test]
    fn capability_json_roundtrips() {
        let cap = Capability {
            id: "cap-1".into(),
            tenant: "tenant-a".into(),
            subject: "agent-1".into(),
            resource: Resource::Tool { name: "echo".into() },
            actions: vec![Action::Call],
            expires_at: 1_000,
            policy_hash: "ph".into(),
            audit_id: "aud-1".into(),
        };
        let s = serde_json::to_string(&cap).unwrap();
        let back: Capability = serde_json::from_str(&s).unwrap();
        assert_eq!(cap, back);
    }
}

#[cfg(test)]
mod sign_tests {
    use super::*;

    fn cap(expires_at: u64) -> Capability {
        Capability {
            id: "cap-1".into(),
            tenant: "tenant-a".into(),
            subject: "agent-1".into(),
            resource: Resource::Tool { name: "echo".into() },
            actions: vec![Action::Call],
            expires_at,
            policy_hash: "ph".into(),
            audit_id: "aud-1".into(),
        }
    }

    #[test]
    fn sign_then_verify_roundtrips() {
        let secret = b"dev-secret";
        let token = sign(secret, &cap(10_000));
        let got = verify(secret, &token, 9_000).unwrap();
        assert_eq!(got, cap(10_000));
    }

    #[test]
    fn rejects_wrong_secret() {
        let token = sign(b"secret-a", &cap(10_000));
        assert_eq!(verify(b"secret-b", &token, 9_000).unwrap_err(), CapError::BadSignature);
    }

    #[test]
    fn rejects_tampered_claims() {
        let token = sign(b"s", &cap(10_000));
        let (claims, mac) = token.split_once('.').unwrap();
        let mut bytes = claims.as_bytes().to_vec();
        bytes[0] ^= 0x01;
        let tampered = format!("{}.{}", String::from_utf8_lossy(&bytes), mac);
        assert!(matches!(
            verify(b"s", &tampered, 9_000).unwrap_err(),
            CapError::BadSignature | CapError::Malformed
        ));
    }
}

#[cfg(test)]
mod authz_tests {
    use super::*;

    fn cap() -> Capability {
        Capability {
            id: "cap-1".into(),
            tenant: "tenant-a".into(),
            subject: "agent-1".into(),
            resource: Resource::Tool { name: "echo".into() },
            actions: vec![Action::Call],
            expires_at: 10_000,
            policy_hash: "ph".into(),
            audit_id: "aud-1".into(),
        }
    }

    #[test]
    fn rejects_expired_on_verify() {
        let token = sign(b"s", &cap());
        assert_eq!(verify(b"s", &token, 10_000).unwrap_err(), CapError::Expired);
    }

    #[test]
    fn authorizes_matching_action_and_resource() {
        cap().authorize(Action::Call, &Resource::Tool { name: "echo".into() }).unwrap();
    }

    #[test]
    fn rejects_wrong_resource() {
        let err = cap()
            .authorize(Action::Call, &Resource::Tool { name: "shell".into() })
            .unwrap_err();
        assert_eq!(err, CapError::Unauthorized);
    }

    #[test]
    fn rejects_wrong_action() {
        let err = cap()
            .authorize(Action::Write, &Resource::Tool { name: "echo".into() })
            .unwrap_err();
        assert_eq!(err, CapError::Unauthorized);
    }
}
