//! Pure Sketchi document and CRDT primitives.
//!
//! This crate intentionally has no graphics, user-interface, asynchronous,
//! networking, filesystem, process, or persistence dependencies. Both the
//! desktop client and collaboration server apply document changes through this
//! layer.

#![forbid(unsafe_code)]

mod clock;
mod command;
mod crdt;
mod document;
mod element;
mod error;
mod geometry;
mod ids;
mod operation;
mod version_vector;

pub use clock::{LamportClock, LamportTimestamp};
pub use command::EditorCommand;
pub use crdt::{
    ApplyResult, CrdtDocument, CrdtSnapshot, ElementSnapshot, MAX_ELEMENTS, OperationFingerprint,
    Register, RegisterMetadata,
};
pub use document::Document;
pub use element::{
    Color, EdgeStyle, Element, ElementKind, EmbeddedImage, FillStyle, MAX_IMAGE_BYTES,
    MAX_IMAGE_DIMENSION, MAX_IMAGE_PIXELS, Sloppiness, StrokeStyle, Style, StylePatch, TextAlign,
    TextFontFamily,
};
pub use error::CrdtError;
pub use geometry::{Point, Rect, Size, Transform};
pub use ids::{ClientId, ElementId, OperationId};
pub use operation::{MAX_POINTS, MAX_TEXT_BYTES, Operation, OperationKind};
pub use version_vector::VersionVector;
