//! Winit-independent input normalization boundary.

use canvas_core::Point;

use crate::tools::{PointerEvent, ToolController, ToolOutput};

/// Input events normalized from winit before tool dispatch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputEvent {
    /// Pointer button press.
    Pressed {
        /// Stable element ID reserved by the editor.
        element_id: canvas_core::ElementId,
        /// World-space position.
        position: Point,
    },
    /// Pointer movement.
    Moved(Point),
    /// Pointer release.
    Released(Point),
}

/// Dispatches an input event into the active tool.
pub fn dispatch(controller: &mut ToolController, event: InputEvent) -> Option<ToolOutput> {
    let event = match event {
        InputEvent::Pressed {
            element_id,
            position,
        } => PointerEvent::Pressed {
            element_id,
            position,
        },
        InputEvent::Moved(position) => PointerEvent::Moved(position),
        InputEvent::Released(position) => PointerEvent::Released(position),
    };
    controller.handle(event)
}
