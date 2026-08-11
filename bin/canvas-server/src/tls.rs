//! Built-in rustls identity and loopback certificate pinning.

use rcgen::generate_simple_self_signed;
use sha2::{Digest, Sha256};
use std::fmt::Write;
use thiserror::Error;

/// TLS setup failures.
#[derive(Debug, Error)]
pub enum TlsError {
    /// Certificate generation failed.
    #[error("certificate generation failed: {0}")]
    Rcgen(#[from] rcgen::Error),
    /// rustls could not construct a server configuration.
    #[error("rustls configuration failed: {0}")]
    Rustls(#[from] rustls::Error),
}

/// Self-signed certificate identity restricted to loopback names.
#[derive(Clone, Debug)]
pub struct LoopbackCertificate {
    cert_der: Vec<u8>,
    key_der: Vec<u8>,
    pin: String,
}

impl LoopbackCertificate {
    /// Generates a fresh localhost/127.0.0.1 certificate.
    ///
    /// # Errors
    ///
    /// Returns [`TlsError::Rcgen`] when certificate generation fails.
    pub fn generate() -> Result<Self, TlsError> {
        let certified = generate_simple_self_signed(vec![
            "localhost".to_owned(),
            "127.0.0.1".to_owned(),
            "::1".to_owned(),
        ])?;
        let cert_der = certified.cert.der().to_vec();
        let key_der = certified.signing_key.serialize_der();
        let pin = Self::pin_for_der(&cert_der);
        Ok(Self {
            cert_der,
            key_der,
            pin,
        })
    }

    /// Computes the lowercase SHA-256 certificate pin for DER bytes.
    #[must_use]
    pub fn pin_for_der(cert_der: &[u8]) -> String {
        let digest = Sha256::digest(cert_der);
        let mut pin = String::with_capacity(digest.len() * 2);
        for byte in digest {
            let _ = write!(pin, "{byte:02x}");
        }
        pin
    }

    /// Returns DER certificate bytes.
    #[must_use]
    pub fn cert_der(&self) -> &[u8] {
        &self.cert_der
    }

    /// Returns DER private-key bytes.
    #[must_use]
    pub fn key_der(&self) -> &[u8] {
        &self.key_der
    }

    /// Returns the lowercase SHA-256 certificate pin.
    #[must_use]
    pub fn pin(&self) -> &str {
        &self.pin
    }

    /// Checks a client-provided certificate pin.
    #[must_use]
    pub fn verify_pin(&self, candidate: &str) -> bool {
        self.pin == candidate
    }

    /// Builds a rustls server configuration from this identity.
    ///
    /// # Errors
    ///
    /// Returns [`TlsError::Rustls`] when rustls rejects the DER key or
    /// certificate.
    pub fn server_config(&self) -> Result<rustls::ServerConfig, TlsError> {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        let certificates = vec![CertificateDer::from(self.cert_der.clone())];
        let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone()));
        Ok(rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certificates, private_key)?)
    }
}
