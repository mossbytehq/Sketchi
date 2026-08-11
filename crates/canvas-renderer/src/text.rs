//! Renderer-owned text presentation data.

use canvas_core::{Color, Point};

/// A text run ready for a glyph cache or GPU text backend.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    /// World-space anchor.
    pub origin: Point,
    /// Text content.
    pub text: String,
    /// Text color.
    pub color: Color,
}
