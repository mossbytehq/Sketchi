//! Errors returned while validating or applying document operations.

use thiserror::Error;

/// Errors that prevent an operation or snapshot from being accepted.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum CrdtError {
    /// The operation contains invalid or contradictory metadata.
    #[error("invalid operation: {0}")]
    InvalidOperation(String),
    /// A geometry value is not finite or violates its bounds.
    #[error("invalid geometry: {0}")]
    InvalidGeometry(String),
    /// Text exceeds the document's bounded size.
    #[error("text exceeds the maximum size")]
    TextTooLong,
    /// A point list exceeds the document's bounded size.
    #[error("point list exceeds the maximum size")]
    TooManyPoints,
    /// A client attempted to reuse an operation ID for different content.
    #[error("operation id was reused with different content: {0}")]
    OperationIdReuse(String),
    /// The document has reached its configured element bound.
    #[error("document exceeds the maximum number of elements")]
    TooManyElements,
    /// A serialized snapshot is internally inconsistent.
    #[error("invalid snapshot: {0}")]
    InvalidSnapshot(String),
}
