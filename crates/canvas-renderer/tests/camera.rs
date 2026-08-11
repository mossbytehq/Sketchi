#![allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::unwrap_used,
    missing_docs
)]

use canvas_core::{Point, Size};
use canvas_renderer::Camera;

#[test]
fn world_and_screen_coordinates_round_trip() {
    let camera = Camera::new(Size::new(800.0, 600.0));
    let world = Point::new(120.0, -40.0);
    let screen = camera.world_to_screen(world);
    assert_eq!(camera.screen_to_world(screen), world);
}

#[test]
fn zoom_at_cursor_keeps_the_anchor_world_point_fixed() {
    let mut camera = Camera::new(Size::new(800.0, 600.0));
    let cursor = Point::new(650.0, 200.0);
    let before = camera.screen_to_world(cursor);
    camera.zoom_at(cursor, 2.0);
    assert_eq!(camera.screen_to_world(cursor), before);
    assert_eq!(camera.zoom(), 2.0);
}
