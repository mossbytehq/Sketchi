//! Capability-token generation and hashing.

use sha2::{Digest, Sha256};
use std::fmt::Write;
use uuid::Uuid;

/// Opaque room capability kept in memory by the creator/client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityToken {
    secret: String,
}

impl CapabilityToken {
    /// Generates a fresh high-entropy token.
    #[must_use]
    pub fn generate() -> Self {
        Self {
            secret: Uuid::new_v4().to_string(),
        }
    }

    /// Wraps a known secret for fixtures or a received join token.
    #[must_use]
    pub fn from_secret(secret: impl Into<String>) -> Self {
        Self {
            secret: secret.into(),
        }
    }

    /// Returns the plaintext secret for a one-time room-created response.
    #[must_use]
    pub fn secret(&self) -> &str {
        &self.secret
    }

    /// Hashes the token for durable storage.
    #[must_use]
    pub fn hash(&self) -> String {
        let digest = Sha256::digest(self.secret.as_bytes());
        let mut hash = String::with_capacity(digest.len() * 2);
        for byte in digest {
            let _ = write!(hash, "{byte:02x}");
        }
        hash
    }

    /// Compares the token with a stored hash.
    #[must_use]
    pub fn verify(&self, expected_hash: &str) -> bool {
        self.hash() == expected_hash
    }
}
