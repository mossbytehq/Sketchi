//! Durable whiteboard element types.

use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize, de::Deserializer, ser::Serializer};

use crate::{error::CrdtError, geometry::Point, geometry::Transform, ids::ElementId};

/// Initial set of drawable element kinds.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    /// Axis-aligned rectangle.
    #[default]
    Rectangle,
    /// Diamond bounded by the element transform.
    Diamond,
    /// Triangle bounded by the element transform.
    Triangle,
    /// Ellipse bounded by the element transform.
    Ellipse,
    /// Straight line between the transform corners or points.
    Line,
    /// Straight line with an arrow head.
    Arrow,
    /// Text anchored at the transform position.
    Text,
    /// A durable freehand point sequence.
    Freehand,
    /// An image whose source bytes are embedded in the document.
    Image,
}

/// Maximum number of source bytes retained for one embedded image.
pub const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
/// Maximum width or height retained for one embedded image.
pub const MAX_IMAGE_DIMENSION: u32 = 8_192;
/// Maximum decoded pixel count retained for one embedded image.
pub const MAX_IMAGE_PIXELS: u64 = 16_777_216;

mod base64_bytes {
    use super::{Deserialize, Deserializer, STANDARD, Serializer};
    use base64::Engine as _;

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        STANDARD.decode(encoded).map_err(serde::de::Error::custom)
    }
}

/// An image stored as bounded source bytes in document state.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EmbeddedImage {
    /// MIME type of the source bytes.
    pub mime_type: String,
    /// Decoded pixel width.
    pub width: u32,
    /// Decoded pixel height.
    pub height: u32,
    /// Original encoded image bytes.
    #[serde(with = "base64_bytes")]
    pub bytes: Vec<u8>,
}

impl EmbeddedImage {
    /// Creates an embedded image payload.
    #[must_use]
    pub fn new(mime_type: impl Into<String>, width: u32, height: u32, bytes: Vec<u8>) -> Self {
        Self {
            mime_type: mime_type.into(),
            width,
            height,
            bytes,
        }
    }

    /// Validates the bounded source payload and its decoded metadata.
    ///
    /// # Errors
    ///
    /// Returns [`CrdtError::InvalidOperation`](crate::CrdtError::InvalidOperation)
    /// when the payload is empty, unsupported, oversized, or has invalid
    /// dimensions.
    pub fn validate(&self) -> Result<(), CrdtError> {
        if self.bytes.is_empty() {
            return Err(CrdtError::InvalidOperation(
                "embedded image cannot be empty".to_owned(),
            ));
        }
        if self.bytes.len() > MAX_IMAGE_BYTES {
            return Err(CrdtError::InvalidOperation(
                "embedded image exceeds the maximum byte size".to_owned(),
            ));
        }
        if !matches!(self.mime_type.as_str(), "image/png" | "image/jpeg") {
            return Err(CrdtError::InvalidOperation(
                "embedded image format is unsupported".to_owned(),
            ));
        }
        let pixel_count = u64::from(self.width) * u64::from(self.height);
        if self.width == 0
            || self.height == 0
            || self.width > MAX_IMAGE_DIMENSION
            || self.height > MAX_IMAGE_DIMENSION
            || pixel_count > MAX_IMAGE_PIXELS
        {
            return Err(CrdtError::InvalidOperation(
                "embedded image dimensions are invalid or oversized".to_owned(),
            ));
        }
        Ok(())
    }
}

/// An RGBA color stored without floating-point serialization ambiguity.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Color {
    /// Red channel.
    pub red: u8,
    /// Green channel.
    pub green: u8,
    /// Blue channel.
    pub blue: u8,
    /// Alpha channel.
    pub alpha: u8,
}

impl Color {
    /// Creates an opaque RGB color.
    #[must_use]
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: u8::MAX,
        }
    }

    /// Creates a color with an explicit alpha channel.
    #[must_use]
    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }
}

/// Stroke rendering pattern.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StrokeStyle {
    /// A continuous stroke.
    #[default]
    Solid,
    /// A repeated long/short stroke pattern.
    Dashed,
    /// A repeated dot pattern.
    Dotted,
}

/// Interior rendering pattern used when an element has a fill color.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FillStyle {
    /// A continuous fill.
    #[default]
    Solid,
    /// Diagonal hatch lines.
    Hachure,
    /// Crossing diagonal hatch lines.
    CrossHatch,
}

/// Degree of hand-drawn variation requested by the author.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sloppiness {
    /// Clean geometry with no intentional variation.
    Architect,
    /// Lightly hand-drawn geometry.
    #[default]
    Artist,
    /// Stronger hand-drawn variation.
    Cartoonist,
}

/// Corner treatment for bounded shapes.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeStyle {
    /// Square corners.
    #[default]
    Sharp,
    /// Rounded corners.
    Rounded,
}

/// Font family choices supported by the text editor.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextFontFamily {
    /// Friendly handwritten-style UI text.
    #[default]
    Handwritten,
    /// Proportional sans-serif text.
    Sans,
    /// Monospaced text.
    Monospace,
    /// Proportional serif text.
    Serif,
}

/// Horizontal alignment for a text element.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    /// Align text to the left edge.
    #[default]
    Left,
    /// Center text within its element bounds.
    Center,
    /// Align text to the right edge.
    Right,
}

/// Visual properties shared by drawable elements.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct Style {
    /// Stroke color.
    pub stroke: Color,
    /// Optional interior color.
    pub fill: Option<Color>,
    /// Interior rendering pattern used for the fill color.
    #[serde(default)]
    pub fill_style: FillStyle,
    /// Stroke width in world units.
    pub stroke_width: f32,
    /// Stroke rendering pattern.
    #[serde(default)]
    pub stroke_style: StrokeStyle,
    /// Hand-drawn variation level.
    #[serde(default)]
    pub sloppiness: Sloppiness,
    /// Corner treatment for bounded shapes.
    #[serde(default)]
    pub edges: EdgeStyle,
    /// Overall element opacity in the inclusive range `0..=1`.
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// Font family used by text elements.
    #[serde(default)]
    pub font_family: TextFontFamily,
    /// Font size used by text elements in world units.
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    /// Horizontal alignment used by text elements.
    #[serde(default)]
    pub text_align: TextAlign,
}

const fn default_opacity() -> f32 {
    1.0
}

const fn default_font_size() -> f32 {
    16.0
}

impl Default for Style {
    fn default() -> Self {
        Self {
            stroke: Color::rgb(31, 41, 55),
            fill: None,
            fill_style: FillStyle::Solid,
            stroke_width: 2.0,
            stroke_style: StrokeStyle::Solid,
            sloppiness: Sloppiness::Artist,
            edges: EdgeStyle::Sharp,
            opacity: default_opacity(),
            font_family: TextFontFamily::default(),
            font_size: default_font_size(),
            text_align: TextAlign::default(),
        }
    }
}

impl Style {
    /// Validates floating-point style fields.
    ///
    /// # Errors
    ///
    /// Returns [`CrdtError::InvalidGeometry`](crate::CrdtError::InvalidGeometry)
    /// when the stroke width is non-finite or negative.
    pub fn validate(self) -> Result<(), CrdtError> {
        if !self.stroke_width.is_finite() || self.stroke_width < 0.0 {
            Err(CrdtError::InvalidGeometry(
                "stroke width must be finite and non-negative".to_owned(),
            ))
        } else if !self.opacity.is_finite() || !(0.0..=1.0).contains(&self.opacity) {
            Err(CrdtError::InvalidGeometry(
                "opacity must be finite and between zero and one".to_owned(),
            ))
        } else if !self.font_size.is_finite() || !(1.0..=512.0).contains(&self.font_size) {
            Err(CrdtError::InvalidGeometry(
                "font size must be finite and between one and 512".to_owned(),
            ))
        } else {
            Ok(())
        }
    }
}

/// Partial style update used by an operation.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct StylePatch {
    /// Replacement stroke color, when present.
    pub stroke: Option<Color>,
    /// Replacement fill; `Some(None)` clears the fill.
    pub fill: Option<Option<Color>>,
    /// Replacement interior rendering pattern, when present.
    #[serde(default)]
    pub fill_style: Option<FillStyle>,
    /// Replacement stroke width, when present.
    pub stroke_width: Option<f32>,
    /// Replacement stroke rendering pattern, when present.
    #[serde(default)]
    pub stroke_style: Option<StrokeStyle>,
    /// Replacement hand-drawn variation, when present.
    #[serde(default)]
    pub sloppiness: Option<Sloppiness>,
    /// Replacement corner treatment, when present.
    #[serde(default)]
    pub edges: Option<EdgeStyle>,
    /// Replacement opacity, when present.
    #[serde(default)]
    pub opacity: Option<f32>,
    /// Replacement text font family, when present.
    #[serde(default)]
    pub font_family: Option<TextFontFamily>,
    /// Replacement text font size, when present.
    #[serde(default)]
    pub font_size: Option<f32>,
    /// Replacement text alignment, when present.
    #[serde(default)]
    pub text_align: Option<TextAlign>,
}

impl StylePatch {
    /// Validates fields present in the patch.
    ///
    /// # Errors
    ///
    /// Returns [`CrdtError::InvalidGeometry`](crate::CrdtError::InvalidGeometry)
    /// when a supplied stroke width is non-finite or negative.
    pub fn validate(self) -> Result<(), CrdtError> {
        if self
            .stroke_width
            .is_some_and(|stroke_width| !stroke_width.is_finite() || stroke_width < 0.0)
        {
            return Err(CrdtError::InvalidGeometry(
                "stroke width must be finite and non-negative".to_owned(),
            ));
        }
        if self
            .opacity
            .is_some_and(|opacity| !opacity.is_finite() || !(0.0..=1.0).contains(&opacity))
        {
            return Err(CrdtError::InvalidGeometry(
                "opacity must be finite and between zero and one".to_owned(),
            ));
        }
        if self
            .font_size
            .is_some_and(|font_size| !font_size.is_finite() || !(1.0..=512.0).contains(&font_size))
        {
            return Err(CrdtError::InvalidGeometry(
                "font size must be finite and between one and 512".to_owned(),
            ));
        }
        Ok(())
    }

    /// Applies this patch to a style.
    #[must_use]
    pub fn apply_to(self, mut style: Style) -> Style {
        if let Some(stroke) = self.stroke {
            style.stroke = stroke;
        }
        if let Some(fill) = self.fill {
            style.fill = fill;
        }
        if let Some(fill_style) = self.fill_style {
            style.fill_style = fill_style;
        }
        if let Some(stroke_width) = self.stroke_width {
            style.stroke_width = stroke_width;
        }
        if let Some(stroke_style) = self.stroke_style {
            style.stroke_style = stroke_style;
        }
        if let Some(sloppiness) = self.sloppiness {
            style.sloppiness = sloppiness;
        }
        if let Some(edges) = self.edges {
            style.edges = edges;
        }
        if let Some(opacity) = self.opacity {
            style.opacity = opacity;
        }
        if let Some(font_family) = self.font_family {
            style.font_family = font_family;
        }
        if let Some(font_size) = self.font_size {
            style.font_size = font_size;
        }
        if let Some(text_align) = self.text_align {
            style.text_align = text_align;
        }
        style
    }
}

/// A durable drawable element in a document.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Element {
    /// Stable element identity.
    pub id: ElementId,
    /// Rendering and editing kind.
    pub kind: ElementKind,
    /// Position, dimensions, and rotation.
    pub transform: Transform,
    /// Visual style.
    pub style: Style,
    /// Text content; empty for non-text elements.
    pub text: String,
    /// Local points for line, arrow, and freehand elements.
    pub points: Vec<Point>,
    /// Embedded source image for image elements.
    #[serde(default)]
    pub image: Option<EmbeddedImage>,
    /// Deterministic stacking position.
    pub z_index: i64,
}

impl Element {
    /// Creates an element with default style and empty optional content.
    #[must_use]
    pub fn new(id: ElementId, kind: ElementKind, transform: Transform) -> Self {
        Self {
            id,
            kind,
            transform,
            style: Style::default(),
            text: String::new(),
            points: Vec::new(),
            image: None,
            z_index: 0,
        }
    }

    /// Creates a rectangle element.
    #[must_use]
    pub fn rectangle(id: ElementId, transform: Transform) -> Self {
        Self::new(id, ElementKind::Rectangle, transform)
    }

    /// Creates a diamond element.
    #[must_use]
    pub fn diamond(id: ElementId, transform: Transform) -> Self {
        Self::new(id, ElementKind::Diamond, transform)
    }

    /// Creates a triangle element.
    #[must_use]
    pub fn triangle(id: ElementId, transform: Transform) -> Self {
        Self::new(id, ElementKind::Triangle, transform)
    }

    /// Creates a text element.
    #[must_use]
    pub fn text(id: ElementId, transform: Transform, text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::new(id, ElementKind::Text, transform)
        }
    }

    /// Creates an ellipse element.
    #[must_use]
    pub fn ellipse(id: ElementId, transform: Transform) -> Self {
        Self::new(id, ElementKind::Ellipse, transform)
    }

    /// Creates a line element with local endpoints.
    #[must_use]
    pub fn line(id: ElementId, transform: Transform, points: Vec<Point>) -> Self {
        Self::with_points(id, ElementKind::Line, transform, points)
    }

    /// Creates an arrow element with local endpoints.
    #[must_use]
    pub fn arrow(id: ElementId, transform: Transform, points: Vec<Point>) -> Self {
        Self::with_points(id, ElementKind::Arrow, transform, points)
    }

    /// Creates a freehand element with local points.
    #[must_use]
    pub fn freehand(id: ElementId, transform: Transform, points: Vec<Point>) -> Self {
        Self::with_points(id, ElementKind::Freehand, transform, points)
    }

    /// Creates an image element with an embedded source payload.
    #[must_use]
    pub fn image(id: ElementId, transform: Transform, image: EmbeddedImage) -> Self {
        let mut element = Self::new(id, ElementKind::Image, transform);
        element.style.stroke = Color::rgba(0, 0, 0, 0);
        element.image = Some(image);
        element
    }

    /// Creates an element with a point sequence.
    #[must_use]
    pub fn with_points(
        id: ElementId,
        kind: ElementKind,
        transform: Transform,
        points: Vec<Point>,
    ) -> Self {
        Self {
            points,
            ..Self::new(id, kind, transform)
        }
    }

    /// Validates the element's bounded durable fields.
    ///
    /// # Errors
    ///
    /// Returns a [`CrdtError`] when geometry is invalid, text is oversized, or
    /// the point sequence exceeds its bound.
    pub fn validate(&self) -> Result<(), CrdtError> {
        if self.id.is_nil() {
            return Err(CrdtError::InvalidOperation(
                "element id cannot be nil".to_owned(),
            ));
        }
        self.transform.validate()?;
        self.style.validate()?;
        if self.text.len() > crate::MAX_TEXT_BYTES {
            return Err(CrdtError::TextTooLong);
        }
        if self.points.len() > crate::MAX_POINTS {
            return Err(CrdtError::TooManyPoints);
        }
        for point in &self.points {
            point.validate()?;
        }
        match (self.kind, self.image.as_ref()) {
            (ElementKind::Image, Some(image)) => image.validate()?,
            (ElementKind::Image, None) => {
                return Err(CrdtError::InvalidOperation(
                    "image element is missing its embedded payload".to_owned(),
                ));
            }
            (_, Some(_)) => {
                return Err(CrdtError::InvalidOperation(
                    "only image elements may contain an embedded payload".to_owned(),
                ));
            }
            (_, None) => {}
        }
        Ok(())
    }
}
