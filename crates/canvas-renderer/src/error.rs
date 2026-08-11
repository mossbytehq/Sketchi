//! Renderer-facing errors.

use thiserror::Error;

/// Errors raised while preparing a render scene.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum RendererError {
    /// A viewport or zoom value is invalid.
    #[error("invalid renderer viewport or zoom")]
    InvalidViewport,
}
