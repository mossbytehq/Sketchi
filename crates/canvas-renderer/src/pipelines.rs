//! GPU pipeline organization boundary.

/// Names of the initial renderer pipeline families.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelineKind {
    /// Filled and stroked shapes.
    Shapes,
    /// Polyline and freehand geometry.
    Lines,
    /// Text atlas rendering.
    Text,
    /// Selection and remote presence overlays.
    Overlay,
}

/// Registry used by the eventual wgpu backend to keep pipeline setup modular.
#[derive(Clone, Copy, Debug, Default)]
pub struct PipelineCatalog;

impl PipelineCatalog {
    /// Returns the pipeline families required by the initial scene renderer.
    #[must_use]
    pub const fn kinds(self) -> [PipelineKind; 4] {
        [
            PipelineKind::Shapes,
            PipelineKind::Lines,
            PipelineKind::Text,
            PipelineKind::Overlay,
        ]
    }
}
