//! Editor commands that are translated into durable operations.

use crate::{
    LamportTimestamp, VersionVector,
    element::{Element, EmbeddedImage, StylePatch},
    geometry::{Point, Size},
    ids::{ElementId, OperationId},
    operation::{Operation, OperationKind},
};

/// User-facing mutation intent before it becomes an operation.
#[derive(Clone, Debug, PartialEq)]
pub enum EditorCommand {
    /// Create an element.
    Create(Element),
    /// Delete an element.
    Delete(ElementId),
    /// Move an element.
    SetPosition(ElementId, Point),
    /// Resize an element.
    SetSize(ElementId, Size),
    /// Rotate an element.
    SetRotation(ElementId, f32),
    /// Change selected style properties.
    SetStyle(ElementId, StylePatch),
    /// Replace text content.
    SetText(ElementId, String),
    /// Replace an embedded image payload.
    SetImage(ElementId, EmbeddedImage),
    /// Replace point content.
    SetPoints(ElementId, Vec<Point>),
    /// Change stacking order.
    Reorder(ElementId, i64),
}

impl EditorCommand {
    /// Converts the command into the one shared operation representation.
    #[must_use]
    pub fn into_operation(
        self,
        operation_id: OperationId,
        timestamp: LamportTimestamp,
        deps: VersionVector,
    ) -> Operation {
        let kind = match self {
            Self::Create(element) => OperationKind::Create { element },
            Self::Delete(element_id) => OperationKind::Delete { element_id },
            Self::SetPosition(element_id, position) => OperationKind::SetPosition {
                element_id,
                position,
            },
            Self::SetSize(element_id, size) => OperationKind::SetSize { element_id, size },
            Self::SetRotation(element_id, rotation) => OperationKind::SetRotation {
                element_id,
                rotation,
            },
            Self::SetStyle(element_id, style) => OperationKind::SetStyle { element_id, style },
            Self::SetText(element_id, text) => OperationKind::SetText { element_id, text },
            Self::SetImage(element_id, image) => OperationKind::SetImage { element_id, image },
            Self::SetPoints(element_id, points) => OperationKind::SetPoints { element_id, points },
            Self::Reorder(element_id, z_index) => OperationKind::Reorder {
                element_id,
                z_index,
            },
        };
        Operation::new(operation_id, timestamp, deps, kind)
    }
}
