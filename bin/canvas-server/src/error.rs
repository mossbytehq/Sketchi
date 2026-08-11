//! Server-specific errors.

use thiserror::Error;

/// Errors raised by room, storage, configuration, or transport layers.
#[derive(Debug, Error)]
pub enum ServerError {
    /// The requested room is not known by the store or manager.
    #[error("room not found")]
    RoomNotFound,
    /// The session is not a member of the room.
    #[error("client is not joined to the room")]
    NotInRoom,
    /// Capability or operation client identity is invalid.
    #[error("client authorization failed: {0}")]
    Unauthorized(String),
    /// A shared core operation was invalid.
    #[error("core operation failed: {0}")]
    Core(#[from] canvas_core::CrdtError),
    /// `SQLite` persistence failed.
    #[error("store failed: {0}")]
    Store(#[from] crate::store::StoreError),
    /// Server configuration is unsafe or incomplete.
    #[error("invalid server configuration: {0}")]
    InvalidConfig(String),
    /// TLS certificate generation or configuration failed.
    #[error("TLS setup failed: {0}")]
    Tls(String),
}
