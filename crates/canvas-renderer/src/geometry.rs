//! CPU-side scene extraction and geometric hit testing.

use canvas_core::{Document, Element, ElementId, ElementKind, EmbeddedImage, Point, Rect, Style};

/// Renderer-ready primitive derived from one document element.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderPrimitive {
    /// Rectangle outline and fill.
    Rectangle {
        /// Source element ID.
        id: ElementId,
        /// World-space bounds.
        rect: Rect,
        /// Element rotation in radians.
        rotation: f32,
        /// Visual style.
        style: Style,
    },
    /// Diamond outline and fill.
    Diamond {
        /// Source element ID.
        id: ElementId,
        /// World-space bounds.
        rect: Rect,
        /// Element rotation in radians.
        rotation: f32,
        /// Visual style.
        style: Style,
    },
    /// Triangle outline and fill.
    Triangle {
        /// Source element ID.
        id: ElementId,
        /// World-space bounds.
        rect: Rect,
        /// Element rotation in radians.
        rotation: f32,
        /// Visual style.
        style: Style,
    },
    /// Ellipse outline and fill.
    Ellipse {
        /// Source element ID.
        id: ElementId,
        /// World-space bounds.
        rect: Rect,
        /// Element rotation in radians.
        rotation: f32,
        /// Visual style.
        style: Style,
    },
    /// Straight line.
    Line {
        /// Source element ID.
        id: ElementId,
        /// World-space points.
        points: Vec<Point>,
        /// Visual style.
        style: Style,
    },
    /// Arrow line.
    Arrow {
        /// Source element ID.
        id: ElementId,
        /// World-space points.
        points: Vec<Point>,
        /// Visual style.
        style: Style,
    },
    /// Text run.
    Text {
        /// Source element ID.
        id: ElementId,
        /// World-space anchor.
        origin: Point,
        /// Rotation in radians around the text anchor.
        rotation: f32,
        /// Text content.
        text: String,
        /// Visual style.
        style: Style,
    },
    /// Freehand path.
    Freehand {
        /// Source element ID.
        id: ElementId,
        /// World-space points.
        points: Vec<Point>,
        /// Visual style.
        style: Style,
    },
    /// Embedded image with world-space bounds.
    Image {
        /// Source element ID.
        id: ElementId,
        /// World-space bounds.
        rect: Rect,
        /// Element rotation in radians.
        rotation: f32,
        /// Embedded source image.
        image: EmbeddedImage,
        /// Visual style, including opacity.
        style: Style,
    },
}

impl RenderPrimitive {
    /// Returns the source element ID.
    #[must_use]
    pub const fn id(&self) -> ElementId {
        match self {
            Self::Rectangle { id, .. }
            | Self::Diamond { id, .. }
            | Self::Triangle { id, .. }
            | Self::Ellipse { id, .. }
            | Self::Line { id, .. }
            | Self::Arrow { id, .. }
            | Self::Text { id, .. }
            | Self::Freehand { id, .. }
            | Self::Image { id, .. } => *id,
        }
    }

    /// Converts one document element into the renderer boundary type.
    #[must_use]
    pub fn from_element(element: &Element) -> Self {
        to_primitive(element)
    }
}

/// A renderer-ready ordered scene.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    primitives: Vec<RenderPrimitive>,
}

impl Scene {
    /// Returns the number of primitives.
    #[must_use]
    pub fn len(&self) -> usize {
        self.primitives.len()
    }

    /// Returns whether the scene has no primitives.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.primitives.is_empty()
    }

    /// Iterates primitives in deterministic stacking order.
    pub fn primitives(&self) -> impl Iterator<Item = &RenderPrimitive> {
        self.primitives.iter()
    }

    pub(crate) fn from_document(document: &Document) -> Self {
        let primitives = document.elements_in_z_order().map(to_primitive).collect();
        Self { primitives }
    }
}

fn to_primitive(element: &Element) -> RenderPrimitive {
    let rect = Rect::new(element.transform.position, element.transform.size);
    match element.kind {
        ElementKind::Rectangle => RenderPrimitive::Rectangle {
            id: element.id,
            rect,
            rotation: element.transform.rotation,
            style: element.style,
        },
        ElementKind::Diamond => RenderPrimitive::Diamond {
            id: element.id,
            rect,
            rotation: element.transform.rotation,
            style: element.style,
        },
        ElementKind::Triangle => RenderPrimitive::Triangle {
            id: element.id,
            rect,
            rotation: element.transform.rotation,
            style: element.style,
        },
        ElementKind::Ellipse => RenderPrimitive::Ellipse {
            id: element.id,
            rect,
            rotation: element.transform.rotation,
            style: element.style,
        },
        ElementKind::Line => RenderPrimitive::Line {
            id: element.id,
            points: points_or_bounds(element),
            style: element.style,
        },
        ElementKind::Arrow => RenderPrimitive::Arrow {
            id: element.id,
            points: points_or_bounds(element),
            style: element.style,
        },
        ElementKind::Text => RenderPrimitive::Text {
            id: element.id,
            origin: element.transform.position,
            rotation: element.transform.rotation,
            text: element.text.clone(),
            style: element.style,
        },
        ElementKind::Freehand => RenderPrimitive::Freehand {
            id: element.id,
            points: points_or_bounds(element),
            style: element.style,
        },
        ElementKind::Image => match element.image.clone() {
            Some(image) => RenderPrimitive::Image {
                id: element.id,
                rect,
                rotation: element.transform.rotation,
                image,
                style: element.style,
            },
            None => RenderPrimitive::Rectangle {
                id: element.id,
                rect,
                rotation: element.transform.rotation,
                style: element.style,
            },
        },
    }
}

fn points_or_bounds(element: &Element) -> Vec<Point> {
    let points = if element.points.is_empty() {
        let rect = Rect::new(element.transform.position, element.transform.size);
        vec![rect.min, rect.max()]
    } else {
        element.points.clone()
    };
    if element.transform.rotation.abs() <= f32::EPSILON {
        return points;
    }
    let center = Point::new(
        element.transform.position.x + element.transform.size.width / 2.0,
        element.transform.position.y + element.transform.size.height / 2.0,
    );
    points
        .into_iter()
        .map(|point| rotate_around(point, center, element.transform.rotation))
        .collect()
}

/// Finds the topmost element under a world-space point.
#[must_use]
pub fn hit_test(document: &Document, point: Point, tolerance: f32) -> Option<ElementId> {
    let tolerance = tolerance.max(0.0);
    let topmost = document
        .elements_in_z_order()
        .rev()
        .find(|element| element_contains(element, point, tolerance))?;
    let topmost_id = topmost.id;
    let topmost_is_unfilled_shape = is_unfilled_bounded_shape(topmost);

    // An unfilled bounded shape is still selectable from its interior when it
    // stands alone, but it should not hide a smaller object drawn inside it.
    // This mirrors the visible geometry more closely and keeps nested outline
    // shapes reachable with the select tool.
    if topmost_is_unfilled_shape {
        return document
            .elements_in_z_order()
            .rev()
            .filter(|element| element_contains(element, point, tolerance))
            .min_by(|left, right| {
                element_area(left)
                    .total_cmp(&element_area(right))
                    .then_with(|| right.z_index.cmp(&left.z_index))
                    .then_with(|| right.id.cmp(&left.id))
            })
            .map(|element| element.id);
    }

    Some(topmost_id)
}

fn is_unfilled_bounded_shape(element: &Element) -> bool {
    element.style.fill.is_none()
        && matches!(
            element.kind,
            ElementKind::Rectangle
                | ElementKind::Diamond
                | ElementKind::Triangle
                | ElementKind::Ellipse
        )
}

fn element_area(element: &Element) -> f32 {
    (element.transform.size.width.abs() * element.transform.size.height.abs()).max(0.0)
}

fn element_contains(element: &Element, point: Point, tolerance: f32) -> bool {
    let rect = Rect::new(element.transform.position, element.transform.size);
    match element.kind {
        ElementKind::Rectangle | ElementKind::Image | ElementKind::Text => {
            let center = Point::new(
                rect.min.x + rect.size.width / 2.0,
                rect.min.y + rect.size.height / 2.0,
            );
            let local = rotate_around(point, center, -element.transform.rotation);
            local.x >= rect.min.x - tolerance
                && local.x <= rect.max().x + tolerance
                && local.y >= rect.min.y - tolerance
                && local.y <= rect.max().y + tolerance
        }
        ElementKind::Diamond => {
            let center = Point::new(
                rect.min.x + rect.size.width / 2.0,
                rect.min.y + rect.size.height / 2.0,
            );
            let local = rotate_around(point, center, -element.transform.rotation);
            let half_width = rect.size.width / 2.0 + tolerance;
            let half_height = rect.size.height / 2.0 + tolerance;
            if half_width <= 0.0 || half_height <= 0.0 {
                return false;
            }
            (local.x - center.x).abs() / half_width + (local.y - center.y).abs() / half_height
                <= 1.0
        }
        ElementKind::Triangle => {
            let center = Point::new(
                rect.min.x + rect.size.width / 2.0,
                rect.min.y + rect.size.height / 2.0,
            );
            let local = rotate_around(point, center, -element.transform.rotation);
            point_in_triangle_or_near(local, triangle_points(rect), tolerance.max(0.0))
        }
        ElementKind::Ellipse => {
            let radius_x = rect.size.width / 2.0 + tolerance;
            let radius_y = rect.size.height / 2.0 + tolerance;
            if radius_x <= 0.0 || radius_y <= 0.0 {
                return false;
            }
            let center = Point::new(
                rect.min.x + rect.size.width / 2.0,
                rect.min.y + rect.size.height / 2.0,
            );
            let local = rotate_around(point, center, -element.transform.rotation);
            let x = (local.x - center.x) / radius_x;
            let y = (local.y - center.y) / radius_y;
            x * x + y * y <= 1.0
        }
        ElementKind::Line | ElementKind::Arrow | ElementKind::Freehand => {
            let points = points_or_bounds(element);
            points
                .windows(2)
                .any(|segment| match (segment.first(), segment.get(1)) {
                    (Some(start), Some(end)) => {
                        distance_to_segment(point, *start, *end) <= tolerance.max(4.0)
                    }
                    _ => false,
                })
        }
    }
}

fn triangle_points(rect: Rect) -> [Point; 3] {
    [
        Point::new(rect.min.x + rect.size.width / 2.0, rect.min.y),
        Point::new(rect.max().x, rect.max().y),
        Point::new(rect.min.x, rect.max().y),
    ]
}

fn point_in_triangle_or_near(point: Point, points: [Point; 3], tolerance: f32) -> bool {
    let [a, b, c] = points;
    let first = cross(b, a, point);
    let second = cross(c, b, point);
    let third = cross(a, c, point);
    let has_negative = first < 0.0 || second < 0.0 || third < 0.0;
    let has_positive = first > 0.0 || second > 0.0 || third > 0.0;
    if !(has_negative && has_positive) {
        return true;
    }
    distance_to_segment(point, a, b) <= tolerance
        || distance_to_segment(point, b, c) <= tolerance
        || distance_to_segment(point, c, a) <= tolerance
}

fn cross(a: Point, b: Point, point: Point) -> f32 {
    (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x)
}

fn rotate_around(point: Point, center: Point, angle: f32) -> Point {
    let sin = angle.sin();
    let cos = angle.cos();
    let x = point.x - center.x;
    let y = point.y - center.y;
    Point::new(center.x + x * cos - y * sin, center.y + x * sin + y * cos)
}

fn distance_to_segment(point: Point, start: Point, end: Point) -> f32 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length_squared = dx * dx + dy * dy;
    if length_squared <= f32::EPSILON {
        return distance(point, start);
    }
    let projection = ((point.x - start.x) * dx + (point.y - start.y) * dy) / length_squared;
    let t = projection.clamp(0.0, 1.0);
    distance(point, Point::new(start.x + t * dx, start.y + t * dy))
}

fn distance(first: Point, second: Point) -> f32 {
    let dx = first.x - second.x;
    let dy = first.y - second.y;
    (dx * dx + dy * dy).sqrt()
}
