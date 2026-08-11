//! Rendering-facing types for Sketchi.
//!
//! Rendering consumes document snapshots and presentation state. It does not
//! own synchronization, persistence, rooms, or transport connections.

#![forbid(unsafe_code)]

mod camera;
mod error;
mod geometry;
mod pipelines;
mod selection;
mod text;

pub use camera::Camera;
pub use error::RendererError;
pub use geometry::{RenderPrimitive, Scene, hit_test};
pub use pipelines::{PipelineCatalog, PipelineKind};
pub use selection::SelectionState;
pub use text::TextRun;

/// Presentation-only renderer facade.
#[derive(Clone, Copy, Debug)]
pub struct Renderer {
    camera: Camera,
    pipelines: PipelineCatalog,
}

impl Renderer {
    /// Creates a renderer facade for a camera.
    #[must_use]
    pub const fn new(camera: Camera) -> Self {
        Self {
            camera,
            pipelines: PipelineCatalog,
        }
    }

    /// Returns the current camera.
    #[must_use]
    pub const fn camera(self) -> Camera {
        self.camera
    }

    /// Returns the pipeline catalog used by this renderer.
    #[must_use]
    pub const fn pipelines(self) -> PipelineCatalog {
        self.pipelines
    }

    /// Updates the camera used for scene preparation.
    pub fn set_camera(&mut self, camera: Camera) {
        self.camera = camera;
    }

    /// Extracts an ordered presentation scene from a document snapshot.
    #[must_use]
    pub fn draw(&self, document: &canvas_core::Document) -> Scene {
        geometry::Scene::from_document(document)
    }
}
