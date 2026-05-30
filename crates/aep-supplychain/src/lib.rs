//! Supply-chain verification: tool artifacts must match a pinned trusted digest.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use thiserror::Error;

/// SHA-256 hex digest of an artifact's bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SupplyChainError {
    #[error("artifact '{0}' is not in the trusted registry")]
    UnknownArtifact(String),
    #[error("artifact '{name}' digest mismatch (untrusted/tampered)")]
    DigestMismatch { name: String },
}

/// A registry of trusted artifact digests (the pinned manifest).
#[derive(Default)]
pub struct Registry {
    trusted: HashMap<String, String>,
}

impl Registry {
    /// Pin a trusted digest for a named artifact.
    pub fn register(&mut self, name: &str, digest: &str) {
        self.trusted.insert(name.to_string(), digest.to_string());
    }

    /// Verify `artifact` bytes match the pinned digest for `name`.
    pub fn verify(&self, name: &str, artifact: &[u8]) -> Result<(), SupplyChainError> {
        let expected = self
            .trusted
            .get(name)
            .ok_or_else(|| SupplyChainError::UnknownArtifact(name.to_string()))?;
        if &sha256_hex(artifact) != expected {
            return Err(SupplyChainError::DigestMismatch { name: name.to_string() });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_pinned_artifact() {
        let mut reg = Registry::default();
        reg.register("echo", &sha256_hex(b"wasm-bytes"));
        assert_eq!(reg.verify("echo", b"wasm-bytes"), Ok(()));
    }

    #[test]
    fn rejects_tampered_artifact() {
        let mut reg = Registry::default();
        reg.register("echo", &sha256_hex(b"wasm-bytes"));
        assert_eq!(
            reg.verify("echo", b"tampered-bytes"),
            Err(SupplyChainError::DigestMismatch { name: "echo".into() })
        );
    }

    #[test]
    fn rejects_unknown_artifact() {
        let reg = Registry::default();
        assert_eq!(
            reg.verify("ghost", b"x"),
            Err(SupplyChainError::UnknownArtifact("ghost".into()))
        );
    }
}
