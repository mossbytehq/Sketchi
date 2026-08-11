//! Standalone and supervised server configuration.

use std::{net::SocketAddr, path::PathBuf};

use crate::error::ServerError;

/// TLS policy for a server endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TlsMode {
    /// Require configured certificate and private key.
    Required,
    /// Permit only loopback development without TLS.
    Disabled,
}

/// Server process configuration.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// Listening address.
    pub bind: SocketAddr,
    /// `SQLite` database path.
    pub database: PathBuf,
    /// TLS policy.
    pub tls_mode: TlsMode,
    /// PEM certificate path for standalone TLS.
    pub certificate: Option<PathBuf>,
    /// PEM private-key path for standalone TLS.
    pub private_key: Option<PathBuf>,
    /// Whether to emit a JSON readiness line for a supervised client.
    pub emit_readiness: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 3210)),
            database: PathBuf::from("sketchi.sqlite3"),
            tls_mode: TlsMode::Disabled,
            certificate: None,
            private_key: None,
            emit_readiness: false,
        }
    }
}

impl ServerConfig {
    /// Validates endpoint and TLS policy before opening a listener.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::InvalidConfig`] when a non-loopback endpoint does
    /// not have TLS or required certificate paths.
    pub fn validate(&self) -> Result<(), ServerError> {
        if self.bind.ip().is_loopback() && self.tls_mode == TlsMode::Disabled {
            return Ok(());
        }
        if self.tls_mode == TlsMode::Disabled {
            return Err(ServerError::InvalidConfig(
                "TLS is required for non-loopback endpoints".to_owned(),
            ));
        }
        if self.certificate.is_none() || self.private_key.is_none() {
            return Err(ServerError::InvalidConfig(
                "TLS certificate and private key are required".to_owned(),
            ));
        }
        Ok(())
    }
}
