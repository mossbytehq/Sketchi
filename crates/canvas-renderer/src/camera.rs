//! Two-dimensional camera and coordinate conversion.

use canvas_core::{Point, Size};

/// Camera state for a world-space whiteboard viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    viewport: Size,
    center: Point,
    zoom: f32,
}

impl Camera {
    /// Creates a camera centered on the world origin.
    #[must_use]
    pub fn new(viewport: Size) -> Self {
        Self {
            viewport,
            center: Point::default(),
            zoom: 1.0,
        }
    }

    /// Returns the viewport size in screen pixels or logical points.
    #[must_use]
    pub const fn viewport(self) -> Size {
        self.viewport
    }

    /// Updates the viewport size.
    pub fn set_viewport(&mut self, viewport: Size) {
        self.viewport = viewport;
    }

    /// Returns the world point at the camera center.
    #[must_use]
    pub const fn center(self) -> Point {
        self.center
    }

    /// Returns the current zoom multiplier.
    #[must_use]
    pub const fn zoom(self) -> f32 {
        self.zoom
    }

    /// Converts a world coordinate to a screen coordinate.
    #[must_use]
    pub fn world_to_screen(self, world: Point) -> Point {
        Point::new(
            (world.x - self.center.x) * self.zoom + self.viewport.width / 2.0,
            (world.y - self.center.y) * self.zoom + self.viewport.height / 2.0,
        )
    }

    /// Converts a screen coordinate to a world coordinate.
    #[must_use]
    pub fn screen_to_world(self, screen: Point) -> Point {
        Point::new(
            (screen.x - self.viewport.width / 2.0) / self.zoom + self.center.x,
            (screen.y - self.viewport.height / 2.0) / self.zoom + self.center.y,
        )
    }

    /// Pans the camera by a screen-space delta.
    pub fn pan_by_screen_delta(&mut self, delta: Point) {
        self.center.x -= delta.x / self.zoom;
        self.center.y -= delta.y / self.zoom;
    }

    /// Zooms around a screen-space cursor while keeping its world anchor fixed.
    pub fn zoom_at(&mut self, cursor: Point, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        self.zoom_to(cursor, self.zoom * factor);
    }

    /// Adjusts zoom by an additive amount while keeping its world anchor fixed.
    ///
    /// Unlike [`Self::zoom_at`], this changes the displayed zoom by a fixed
    /// number of percentage points. For example, `0.15` changes 100% to 115%
    /// and 115% to 130%, while `-0.15` changes 130% back to 115%.
    pub fn zoom_by(&mut self, cursor: Point, delta: f32) {
        if !delta.is_finite() {
            return;
        }
        self.zoom_to(cursor, self.zoom + delta);
    }

    /// Sets an absolute zoom value around a screen-space cursor.
    pub fn zoom_to(&mut self, cursor: Point, zoom: f32) {
        if !zoom.is_finite() || zoom <= 0.0 {
            return;
        }
        let anchor = self.screen_to_world(cursor);
        self.zoom = zoom.clamp(0.05, 64.0);
        self.center = Point::new(
            anchor.x - (cursor.x - self.viewport.width / 2.0) / self.zoom,
            anchor.y - (cursor.y - self.viewport.height / 2.0) / self.zoom,
        );
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self::new(Size::new(800.0, 600.0))
    }
}

#[cfg(test)]
mod tests {
    use super::Camera;
    use canvas_core::{Point, Size};

    #[test]
    fn additive_zoom_steps_are_reversible() {
        let mut camera = Camera::new(Size::new(800.0, 600.0));
        let center = Point::new(400.0, 300.0);

        camera.zoom_by(center, 0.15);
        assert!((camera.zoom() - 1.15).abs() < f32::EPSILON);
        camera.zoom_by(center, 0.15);
        assert!((camera.zoom() - 1.30).abs() < f32::EPSILON);
        camera.zoom_by(center, -0.15);
        assert!((camera.zoom() - 1.15).abs() < f32::EPSILON);
    }

    #[test]
    fn additive_zoom_keeps_the_cursor_world_anchor_fixed() {
        let mut camera = Camera::new(Size::new(800.0, 600.0));
        let cursor = Point::new(123.0, 234.0);
        let anchor = camera.screen_to_world(cursor);

        camera.zoom_by(cursor, 0.08);

        let next_anchor = camera.screen_to_world(cursor);
        assert!((next_anchor.x - anchor.x).abs() < f32::EPSILON);
        assert!((next_anchor.y - anchor.y).abs() < f32::EPSILON);
    }
}
