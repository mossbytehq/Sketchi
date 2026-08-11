//! Protocol validation and framing errors.

use thiserror::Error;

use canvas_core::CrdtError;

/// Errors returned while encoding, decoding, or validating a protocol message.
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// The frame exceeded the configured maximum size.
    #[error("protocol frame exceeds the maximum size")]
    FrameTooLarge,
    /// The JSON envelope could not be decoded or encoded.
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The sender requested a protocol version this build does not support.
    #[error("unsupported protocol version: {0}")]
    UnsupportedVersion(u16),
    /// The message shape or field values are invalid.
    #[error("invalid protocol message: {0}")]
    InvalidMessage(String),
    /// A single message contains too many durable operations.
    #[error("message contains too many operations")]
    TooManyOperations,
    /// Core operation or snapshot validation failed.
    #[error("invalid core payload: {0}")]
    Core(#[from] CrdtError),
}
