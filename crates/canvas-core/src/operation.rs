//! Durable operation definitions and validation.

use serde::{Deserialize, Serialize};

use crate::{
    LamportTimestamp, VersionVector,
    element::{Element, EmbeddedImage, StylePatch},
    error::CrdtError,
    geometry::{Point, Size},
    ids::{ElementId, OperationId},
};

/// Maximum UTF-8 byte length of durable text.
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
/// Maximum number of points in one durable point sequence.
pub const MAX_POINTS: usize = 100_000;

/// One durable mutation of the document.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Operation {
    /// Stable operation identity.
    pub id: OperationId,
    /// Lamport timestamp assigned by the originating replica.
    pub timestamp: LamportTimestamp,
    /// Causal knowledge held when the operation was created.
    pub deps: VersionVector,
    /// The document mutation.
    pub kind: OperationKind,
}

impl Operation {
    /// Creates an operation.
    #[must_use]
    pub const fn new(
        id: OperationId,
        timestamp: LamportTimestamp,
        deps: VersionVector,
        kind: OperationKind,
    ) -> Self {
        Self {
            id,
            timestamp,
            deps,
            kind,
        }
    }

    /// Validates metadata and operation payload bounds.
    ///
    /// # Errors
    ///
    /// Returns a [`CrdtError`] when identifiers, causal metadata, geometry,
    /// text, or point bounds are invalid.
    pub fn validate(&self) -> Result<(), CrdtError> {
        if self.id.client_id.is_nil() || self.id.sequence == 0 {
            return Err(CrdtError::InvalidOperation(
                "operation id must contain a client and non-zero sequence".to_owned(),
            ));
        }
        if self.timestamp.value() == 0 {
            return Err(CrdtError::InvalidOperation(
                "operation timestamp must be non-zero".to_owned(),
            ));
        }
        self.deps.validate()?;
        match &self.kind {
            OperationKind::Create { element } => element.validate(),
            OperationKind::Delete { element_id }
            | OperationKind::SetPosition { element_id, .. }
            | OperationKind::SetSize { element_id, .. }
            | OperationKind::SetRotation { element_id, .. }
            | OperationKind::SetStyle { element_id, .. }
            | OperationKind::SetText { element_id, .. }
            | OperationKind::SetImage { element_id, .. }
            | OperationKind::SetPoints { element_id, .. }
            | OperationKind::Reorder { element_id, .. } => {
                if element_id.is_nil() {
                    Err(CrdtError::InvalidOperation(
                        "element id cannot be nil".to_owned(),
                    ))
                } else {
                    self.kind.validate_payload()
                }
            }
        }
    }
}

/// The supported durable document mutations.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationKind {
    /// Creates or merges the initial state of an element.
    Create {
        /// Element state to create.
        element: Element,
    },
    /// Permanently tombstones an element.
    Delete {
        /// Element to tombstone.
        element_id: ElementId,
    },
    /// Updates position independently from other transform properties.
    SetPosition {
        /// Target element.
        element_id: ElementId,
        /// New world position.
        position: Point,
    },
    /// Updates dimensions independently from other transform properties.
    SetSize {
        /// Target element.
        element_id: ElementId,
        /// New world size.
        size: Size,
    },
    /// Updates rotation independently from position and size.
    SetRotation {
        /// Target element.
        element_id: ElementId,
        /// New rotation in radians.
        rotation: f32,
    },
    /// Updates selected style fields independently.
    SetStyle {
        /// Target element.
        element_id: ElementId,
        /// Partial style replacement.
        style: StylePatch,
    },
    /// Replaces an element's bounded text content.
    SetText {
        /// Target element.
        element_id: ElementId,
        /// New text content.
        text: String,
    },
    /// Replaces an image element's embedded payload.
    SetImage {
        /// Target element.
        element_id: ElementId,
        /// New bounded embedded image.
        image: EmbeddedImage,
    },
    /// Replaces an element's bounded local point sequence.
    SetPoints {
        /// Target element.
        element_id: ElementId,
        /// New local points.
        points: Vec<Point>,
    },
    /// Updates deterministic stacking order.
    Reorder {
        /// Target element.
        element_id: ElementId,
        /// New stacking position.
        z_index: i64,
    },
}

impl OperationKind {
    fn validate_payload(&self) -> Result<(), CrdtError> {
        match self {
            Self::Create { .. } | Self::Delete { .. } | Self::Reorder { .. } => Ok(()),
            Self::SetPosition { position, .. } => position.validate(),
            Self::SetSize { size, .. } => size.validate(),
            Self::SetRotation { rotation, .. } => {
                if rotation.is_finite() {
                    Ok(())
                } else {
                    Err(CrdtError::InvalidGeometry(
                        "rotation must be finite".to_owned(),
                    ))
                }
            }
            Self::SetStyle { style, .. } => style.validate(),
            Self::SetText { text, .. } => {
                if text.len() <= MAX_TEXT_BYTES {
                    Ok(())
                } else {
                    Err(CrdtError::TextTooLong)
                }
            }
            Self::SetImage { image, .. } => image.validate(),
            Self::SetPoints { points, .. } => {
                if points.len() > MAX_POINTS {
                    return Err(CrdtError::TooManyPoints);
                }
                for point in points {
                    point.validate()?;
                }
                Ok(())
            }
        }
    }
}
