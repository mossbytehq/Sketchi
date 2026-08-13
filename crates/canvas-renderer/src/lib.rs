//! Rendering-facing types for Sketchi.
//!
//! Rendering consumes document snapshots and presentation state. It does not
//! own synchronization, persistence, rooms, or transport connections.

#![forbid(unsafe_code)]

mod camera;
mod error;
mod geometry;
mod selection;
mod text;

pub use camera::Camera;
pub use error::RendererError;
pub use geometry::{RenderPrimitive, Scene, hit_test};
pub use selection::SelectionState;
pub use text::TextRun;

/// Stateless scene-extraction facade.
#[derive(Clone, Copy, Debug, Default)]
pub struct Renderer;

impl Renderer {
    /// Creates a scene extractor.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Extracts an ordered presentation scene from a document snapshot.
    #[must_use]
    pub fn draw(&self, document: &canvas_core::Document) -> Scene {
        geometry::Scene::from_document(document)
    }

    /// Finds the topmost element under a world-space point.
    #[must_use]
    pub fn hit_test(
        &self,
        document: &canvas_core::Document,
        point: canvas_core::Point,
        tolerance: f32,
    ) -> Option<canvas_core::ElementId> {
        geometry::hit_test(document, point, tolerance)
    }
}
