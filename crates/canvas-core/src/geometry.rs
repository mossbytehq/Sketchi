//! Small serializable geometry types shared by all document layers.

use serde::{Deserialize, Serialize};

use crate::error::CrdtError;

/// A point in canvas world coordinates.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f32,
    /// Vertical coordinate.
    pub y: f32,
}

impl Point {
    /// Creates a point.
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Returns whether both coordinates are finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    /// Validates the point for durable document storage.
    ///
    /// # Errors
    ///
    /// Returns [`CrdtError::InvalidGeometry`](crate::CrdtError::InvalidGeometry)
    /// when either coordinate is non-finite.
    pub fn validate(self) -> Result<(), CrdtError> {
        if self.is_finite() {
            Ok(())
        } else {
            Err(CrdtError::InvalidGeometry(
                "point must be finite".to_owned(),
            ))
        }
    }
}

/// Width and height in canvas world units.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Size {
    /// Width, which must be finite and non-negative.
    pub width: f32,
    /// Height, which must be finite and non-negative.
    pub height: f32,
}

impl Size {
    /// Creates a size.
    #[must_use]
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    /// Validates the size for durable document storage.
    ///
    /// # Errors
    ///
    /// Returns [`CrdtError::InvalidGeometry`](crate::CrdtError::InvalidGeometry)
    /// when a component is non-finite or negative.
    pub fn validate(self) -> Result<(), CrdtError> {
        if self.width.is_finite()
            && self.height.is_finite()
            && self.width >= 0.0
            && self.height >= 0.0
        {
            Ok(())
        } else {
            Err(CrdtError::InvalidGeometry(
                "size must be finite and non-negative".to_owned(),
            ))
        }
    }
}

/// Position, dimensions, and orientation of an element.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Transform {
    /// Top-left position in world coordinates.
    pub position: Point,
    /// Element dimensions in world units.
    pub size: Size,
    /// Clockwise rotation in radians.
    pub rotation: f32,
}

impl Transform {
    /// Creates an unrotated transform.
    #[must_use]
    pub const fn new(position: Point, size: Size) -> Self {
        Self {
            position,
            size,
            rotation: 0.0,
        }
    }

    /// Validates every transform component.
    ///
    /// # Errors
    ///
    /// Returns [`CrdtError::InvalidGeometry`](crate::CrdtError::InvalidGeometry)
    /// when a component is non-finite or a size is negative.
    pub fn validate(self) -> Result<(), CrdtError> {
        self.position.validate()?;
        self.size.validate()?;
        if self.rotation.is_finite() {
            Ok(())
        } else {
            Err(CrdtError::InvalidGeometry(
                "rotation must be finite".to_owned(),
            ))
        }
    }
}

/// An axis-aligned rectangle in world coordinates.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Rect {
    /// Left coordinate.
    pub min: Point,
    /// Width and height.
    pub size: Size,
}

impl Rect {
    /// Creates a rectangle.
    #[must_use]
    pub const fn new(min: Point, size: Size) -> Self {
        Self { min, size }
    }

    /// Returns the maximum corner.
    #[must_use]
    pub const fn max(self) -> Point {
        Point::new(self.min.x + self.size.width, self.min.y + self.size.height)
    }
}
