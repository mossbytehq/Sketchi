//! Selection geometry and gesture math used by the workspace UI.

use canvas_core::{Element, ElementKind, Point, Rect, Size, Transform};

const MIN_ELEMENT_SIZE: f32 = 4.0;

/// One of the eight resize handles around a selected element.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SelectionHandle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

impl SelectionHandle {
    /// Returns the horizontal movement axis represented by this handle.
    pub(crate) const fn horizontal(self) -> Option<bool> {
        match self {
            Self::TopLeft | Self::Left | Self::BottomLeft => Some(false),
            Self::TopRight | Self::Right | Self::BottomRight => Some(true),
            Self::Top | Self::Bottom => None,
        }
    }

    /// Returns the vertical movement axis represented by this handle.
    pub(crate) const fn vertical(self) -> Option<bool> {
        match self {
            Self::TopLeft | Self::Top | Self::TopRight => Some(false),
            Self::BottomLeft | Self::Bottom | Self::BottomRight => Some(true),
            Self::Left | Self::Right => None,
        }
    }
}

/// Returns the axis-aligned world bounds of a document element.
#[must_use]
pub(crate) fn element_bounds(element: &Element) -> Rect {
    if matches!(
        element.kind,
        ElementKind::Line | ElementKind::Arrow | ElementKind::Freehand
    ) && !element.points.is_empty()
    {
        let Some(first) = element.points.first() else {
            return Rect::new(element.transform.position, element.transform.size);
        };
        let (min_x, max_x, min_y, max_y) = element.points.iter().skip(1).fold(
            (first.x, first.x, first.y, first.y),
            |(min_x, max_x, min_y, max_y), point| {
                (
                    min_x.min(point.x),
                    max_x.max(point.x),
                    min_y.min(point.y),
                    max_y.max(point.y),
                )
            },
        );
        return Rect::new(
            Point::new(min_x, min_y),
            Size::new(max_x - min_x, max_y - min_y),
        );
    }

    let corners = rotated_corners(element.transform);
    bounds_from_points(&corners)
        .unwrap_or_else(|| Rect::new(element.transform.position, element.transform.size))
}

/// Returns the unrotated editing rectangle for one element.
#[must_use]
pub(crate) fn selection_rect(element: &Element) -> Rect {
    if matches!(
        element.kind,
        ElementKind::Line | ElementKind::Arrow | ElementKind::Freehand
    ) && !element.points.is_empty()
    {
        element_bounds(element)
    } else {
        Rect::new(element.transform.position, element.transform.size)
    }
}

/// Returns the four corners of an element's editing box with an outward
/// screen-space padding converted to world units by the caller.
#[must_use]
pub(crate) fn padded_selection_corners(element: &Element, padding: f32) -> [Point; 4] {
    let rect = padded_selection_rect(element, padding);
    rotated_rect_corners(rect, element.transform.rotation)
}

/// Returns the union of the supplied element bounds.
#[must_use]
pub(crate) fn selection_bounds<'a>(
    elements: impl IntoIterator<Item = &'a Element>,
) -> Option<Rect> {
    let mut points = Vec::new();
    for element in elements {
        let bounds = element_bounds(element);
        points.push(bounds.min);
        points.push(bounds.max());
    }
    bounds_from_points(&points)
}

/// Returns whether an element intersects a marquee rectangle.
#[must_use]
pub(crate) fn marquee_intersects(element: &Element, marquee: Rect) -> bool {
    let element = normalize_rect(element_bounds(element));
    let marquee = normalize_rect(marquee);
    element.min.x <= marquee.max().x
        && element.max().x >= marquee.min.x
        && element.min.y <= marquee.max().y
        && element.max().y >= marquee.min.y
}

/// Returns a transform resized from one of the eight handles.
#[must_use]
pub(crate) fn resize_transform(
    element: &Element,
    handle: SelectionHandle,
    pointer: Point,
) -> Transform {
    let start = element.transform;
    let pointer = rotate_around(
        pointer,
        Point::new(
            start.position.x + start.size.width / 2.0,
            start.position.y + start.size.height / 2.0,
        ),
        -start.rotation,
    );
    let left = start.position.x;
    let right = left + start.size.width;
    let top = start.position.y;
    let bottom = top + start.size.height;

    let (new_left, new_right) = match handle.horizontal() {
        Some(false) => (pointer.x.min(right - MIN_ELEMENT_SIZE), right),
        Some(true) => (left, pointer.x.max(left + MIN_ELEMENT_SIZE)),
        None => (left, right),
    };
    let (new_top, new_bottom) = match handle.vertical() {
        Some(false) => (pointer.y.min(bottom - MIN_ELEMENT_SIZE), bottom),
        Some(true) => (top, pointer.y.max(top + MIN_ELEMENT_SIZE)),
        None => (top, bottom),
    };

    Transform {
        position: Point::new(new_left, new_top),
        size: Size::new(new_right - new_left, new_bottom - new_top),
        rotation: start.rotation,
    }
}

/// Resizes one element and scales any world-space points it owns.
#[must_use]
pub(crate) fn resized_element(
    element: &Element,
    handle: SelectionHandle,
    pointer: Point,
) -> Element {
    let mut resized = element.clone();
    resized.transform = resize_transform(element, handle, pointer);
    if element.points.is_empty() {
        return resized;
    }

    let source = Rect::new(element.transform.position, element.transform.size);
    let target = Rect::new(resized.transform.position, resized.transform.size);
    let scale_x = axis_scale(source.size.width, target.size.width);
    let scale_y = axis_scale(source.size.height, target.size.height);
    resized.points = element
        .points
        .iter()
        .copied()
        .map(|point| scale_point(point, source, target, scale_x, scale_y))
        .collect();
    resized
}

/// Translates an element and any world-space points it owns.
#[must_use]
pub(crate) fn translated_element(element: &Element, delta: Point) -> Element {
    let mut translated = element.clone();
    translated.transform.position.x += delta.x;
    translated.transform.position.y += delta.y;
    for point in &mut translated.points {
        point.x += delta.x;
        point.y += delta.y;
    }
    translated
}

/// Returns the eight handle positions for a selection bounds rectangle.
#[must_use]
pub(crate) fn handle_position(bounds: Rect, handle: SelectionHandle) -> Point {
    let center = Point::new(
        bounds.min.x + bounds.size.width / 2.0,
        bounds.min.y + bounds.size.height / 2.0,
    );
    let max = bounds.max();
    match handle {
        SelectionHandle::TopLeft => bounds.min,
        SelectionHandle::Top => Point::new(center.x, bounds.min.y),
        SelectionHandle::TopRight => Point::new(max.x, bounds.min.y),
        SelectionHandle::Right => Point::new(max.x, center.y),
        SelectionHandle::BottomRight => max,
        SelectionHandle::Bottom => Point::new(center.x, max.y),
        SelectionHandle::BottomLeft => Point::new(bounds.min.x, max.y),
        SelectionHandle::Left => Point::new(bounds.min.x, center.y),
    }
}

/// Returns one handle's world position, including element rotation.
#[must_use]
pub(crate) fn selection_handle_position(element: &Element, handle: SelectionHandle) -> Point {
    let rect = selection_rect(element);
    let center = Point::new(
        rect.min.x + rect.size.width / 2.0,
        rect.min.y + rect.size.height / 2.0,
    );
    rotate_around(
        handle_position(rect, handle),
        center,
        element.transform.rotation,
    )
}

/// Returns one outward-padded handle position, including element rotation.
#[must_use]
pub(crate) fn padded_selection_handle_position(
    element: &Element,
    handle: SelectionHandle,
    padding: f32,
) -> Point {
    let rect = padded_selection_rect(element, padding);
    let center = Point::new(
        rect.min.x + rect.size.width / 2.0,
        rect.min.y + rect.size.height / 2.0,
    );
    rotate_around(
        handle_position(rect, handle),
        center,
        element.transform.rotation,
    )
}

/// Returns the world position of the rotation handle.
#[must_use]
pub(crate) fn rotation_handle_position(bounds: Rect, distance: f32) -> Point {
    let center_x = bounds.min.x + bounds.size.width / 2.0;
    Point::new(center_x, bounds.min.y - distance.max(0.0))
}

/// Returns the rotation handle position from an outward-padded editing box.
#[must_use]
pub(crate) fn padded_selection_rotation_handle_position(
    element: &Element,
    distance: f32,
    padding: f32,
) -> Point {
    let rect = padded_selection_rect(element, padding);
    let center = Point::new(
        rect.min.x + rect.size.width / 2.0,
        rect.min.y + rect.size.height / 2.0,
    );
    rotate_around(
        rotation_handle_position(rect, distance),
        center,
        element.transform.rotation,
    )
}

/// Returns an outward-padded resize handle under a pointer, if any.
#[must_use]
pub(crate) fn padded_selection_handle_at(
    element: &Element,
    pointer: Point,
    tolerance: f32,
    padding: f32,
) -> Option<SelectionHandle> {
    const HANDLES: [SelectionHandle; 8] = [
        SelectionHandle::TopLeft,
        SelectionHandle::Top,
        SelectionHandle::TopRight,
        SelectionHandle::Right,
        SelectionHandle::BottomRight,
        SelectionHandle::Bottom,
        SelectionHandle::BottomLeft,
        SelectionHandle::Left,
    ];
    HANDLES.into_iter().find(|handle| {
        let position = padded_selection_handle_position(element, *handle, padding);
        (pointer.x - position.x).abs() <= tolerance && (pointer.y - position.y).abs() <= tolerance
    })
}

/// Returns the group resize handle under a pointer, if any.
#[must_use]
pub(crate) fn selection_handle_at_bounds(
    bounds: Rect,
    pointer: Point,
    tolerance: f32,
) -> Option<SelectionHandle> {
    const HANDLES: [SelectionHandle; 8] = [
        SelectionHandle::TopLeft,
        SelectionHandle::Top,
        SelectionHandle::TopRight,
        SelectionHandle::Right,
        SelectionHandle::BottomRight,
        SelectionHandle::Bottom,
        SelectionHandle::BottomLeft,
        SelectionHandle::Left,
    ];
    HANDLES.into_iter().find(|handle| {
        let position = handle_position(bounds, *handle);
        (pointer.x - position.x).abs() <= tolerance && (pointer.y - position.y).abs() <= tolerance
    })
}

/// Resizes one element as part of a shared group bounds resize.
#[must_use]
pub(crate) fn group_resized_element(
    element: &Element,
    bounds: Rect,
    handle: SelectionHandle,
    pointer: Point,
) -> Element {
    let next_bounds = group_resize_bounds(bounds, handle, pointer);
    let scale_x = if bounds.size.width > f32::EPSILON {
        next_bounds.size.width / bounds.size.width
    } else {
        1.0
    };
    let scale_y = if bounds.size.height > f32::EPSILON {
        next_bounds.size.height / bounds.size.height
    } else {
        1.0
    };
    let mut resized = element.clone();
    resized.transform.position = scale_point(
        element.transform.position,
        bounds,
        next_bounds,
        scale_x,
        scale_y,
    );
    resized.transform.size = Size::new(
        element.transform.size.width * scale_x,
        element.transform.size.height * scale_y,
    );
    resized.points = element
        .points
        .iter()
        .copied()
        .map(|point| scale_point(point, bounds, next_bounds, scale_x, scale_y))
        .collect();
    resized
}

fn group_resize_bounds(bounds: Rect, handle: SelectionHandle, pointer: Point) -> Rect {
    let max = bounds.max();
    let (new_left, new_right) = match handle.horizontal() {
        Some(false) => (pointer.x.min(max.x - MIN_ELEMENT_SIZE), max.x),
        Some(true) => (bounds.min.x, pointer.x.max(bounds.min.x + MIN_ELEMENT_SIZE)),
        None => (bounds.min.x, max.x),
    };
    let (new_top, new_bottom) = match handle.vertical() {
        Some(false) => (pointer.y.min(max.y - MIN_ELEMENT_SIZE), max.y),
        Some(true) => (bounds.min.y, pointer.y.max(bounds.min.y + MIN_ELEMENT_SIZE)),
        None => (bounds.min.y, max.y),
    };
    Rect::new(
        Point::new(new_left, new_top),
        Size::new(new_right - new_left, new_bottom - new_top),
    )
}

fn scale_point(point: Point, source: Rect, target: Rect, scale_x: f32, scale_y: f32) -> Point {
    Point::new(
        if source.size.width > f32::EPSILON {
            target.min.x + (point.x - source.min.x) * scale_x
        } else {
            target.min.x + target.size.width / 2.0
        },
        if source.size.height > f32::EPSILON {
            target.min.y + (point.y - source.min.y) * scale_y
        } else {
            target.min.y + target.size.height / 2.0
        },
    )
}

fn axis_scale(source: f32, target: f32) -> f32 {
    if source > f32::EPSILON {
        target / source
    } else {
        1.0
    }
}

/// Returns whether a pointer is over the outward-padded rotation handle.
#[must_use]
pub(crate) fn padded_selection_over_rotation_handle(
    element: &Element,
    pointer: Point,
    tolerance: f32,
    padding: f32,
) -> bool {
    let position = padded_selection_rotation_handle_position(element, 28.0, padding);
    (pointer.x - position.x).abs() <= tolerance && (pointer.y - position.y).abs() <= tolerance
}

/// Returns the angle from a center point to a pointer.
#[must_use]
pub(crate) fn pointer_angle(center: Point, pointer: Point) -> f32 {
    (pointer.y - center.y).atan2(pointer.x - center.x)
}

/// Returns the shortest signed angular delta between two angles.
#[must_use]
pub(crate) fn angle_delta(start: f32, current: f32) -> f32 {
    let mut delta = current - start;
    while delta > std::f32::consts::PI {
        delta -= std::f32::consts::TAU;
    }
    while delta < -std::f32::consts::PI {
        delta += std::f32::consts::TAU;
    }
    delta
}

/// Rotates a point around a center.
#[must_use]
pub(crate) fn rotate_around(point: Point, center: Point, angle: f32) -> Point {
    let sin = angle.sin();
    let cos = angle.cos();
    let x = point.x - center.x;
    let y = point.y - center.y;
    Point::new(center.x + x * cos - y * sin, center.y + x * sin + y * cos)
}

fn rotated_corners(transform: Transform) -> [Point; 4] {
    rotated_rect_corners(
        Rect::new(transform.position, transform.size),
        transform.rotation,
    )
}

fn padded_selection_rect(element: &Element, padding: f32) -> Rect {
    let rect = selection_rect(element);
    let padding = if padding.is_finite() {
        padding.max(0.0)
    } else {
        0.0
    };
    Rect::new(
        Point::new(rect.min.x - padding, rect.min.y - padding),
        Size::new(
            rect.size.width + padding * 2.0,
            rect.size.height + padding * 2.0,
        ),
    )
}

fn rotated_rect_corners(rect: Rect, rotation: f32) -> [Point; 4] {
    let min = rect.min;
    let max = rect.max();
    let center = Point::new(
        min.x + rect.size.width / 2.0,
        min.y + rect.size.height / 2.0,
    );
    [
        rotate_around(min, center, rotation),
        rotate_around(Point::new(max.x, min.y), center, rotation),
        rotate_around(max, center, rotation),
        rotate_around(Point::new(min.x, max.y), center, rotation),
    ]
}

fn bounds_from_points(points: &[Point]) -> Option<Rect> {
    let first = points.first().copied()?;
    let (min_x, max_x, min_y, max_y) = points.iter().skip(1).fold(
        (first.x, first.x, first.y, first.y),
        |(min_x, max_x, min_y, max_y), point| {
            (
                min_x.min(point.x),
                max_x.max(point.x),
                min_y.min(point.y),
                max_y.max(point.y),
            )
        },
    );
    Some(Rect::new(
        Point::new(min_x, min_y),
        Size::new(max_x - min_x, max_y - min_y),
    ))
}

fn normalize_rect(rect: Rect) -> Rect {
    let max = rect.max();
    Rect::new(
        Point::new(rect.min.x.min(max.x), rect.min.y.min(max.y)),
        Size::new((max.x - rect.min.x).abs(), (max.y - rect.min.y).abs()),
    )
}

#[cfg(test)]
mod tests {
    use canvas_core::{Element, ElementId, Point, Rect, Size, Transform};

    use super::{
        SelectionHandle, angle_delta, group_resized_element, marquee_intersects,
        padded_selection_corners, padded_selection_handle_position, resize_transform,
        resized_element, translated_element,
    };

    #[test]
    fn marquee_intersection_uses_element_bounds() {
        let element = Element::rectangle(
            ElementId::from_u128(1),
            Transform::new(Point::new(40.0, 30.0), Size::new(60.0, 40.0)),
        );

        assert!(marquee_intersects(
            &element,
            Rect::new(Point::new(0.0, 0.0), Size::new(50.0, 50.0))
        ));
        assert!(!marquee_intersects(
            &element,
            Rect::new(Point::new(0.0, 0.0), Size::new(20.0, 20.0))
        ));
    }

    #[test]
    fn resize_from_bottom_right_preserves_the_opposite_anchor() {
        let element = Element::rectangle(
            ElementId::from_u128(2),
            Transform::new(Point::new(10.0, 20.0), Size::new(40.0, 30.0)),
        );

        let resized = resize_transform(
            &element,
            SelectionHandle::BottomRight,
            Point::new(80.0, 90.0),
        );

        assert_eq!(resized.position, Point::new(10.0, 20.0));
        assert_eq!(resized.size, Size::new(70.0, 70.0));
    }

    #[test]
    fn resizing_lines_and_arrows_scales_their_points_with_the_transform() {
        for (index, kind) in [
            canvas_core::ElementKind::Line,
            canvas_core::ElementKind::Arrow,
        ]
        .into_iter()
        .enumerate()
        {
            let element = Element::with_points(
                ElementId::from_u128(7 + u128::try_from(index).unwrap_or(0)),
                kind,
                Transform::new(Point::new(10.0, 20.0), Size::new(40.0, 30.0)),
                vec![Point::new(10.0, 20.0), Point::new(50.0, 50.0)],
            );

            let resized = resized_element(
                &element,
                SelectionHandle::BottomRight,
                Point::new(90.0, 80.0),
            );

            assert_eq!(resized.transform.size, Size::new(80.0, 60.0));
            assert_eq!(
                resized.points,
                vec![Point::new(10.0, 20.0), Point::new(90.0, 80.0)]
            );
        }
    }

    #[test]
    fn padded_selection_geometry_sits_outside_the_element() {
        let element = Element::rectangle(
            ElementId::from_u128(5),
            Transform::new(Point::new(10.0, 20.0), Size::new(40.0, 30.0)),
        );

        assert_eq!(
            padded_selection_corners(&element, 5.0),
            [
                Point::new(5.0, 15.0),
                Point::new(55.0, 15.0),
                Point::new(55.0, 55.0),
                Point::new(5.0, 55.0),
            ]
        );
        assert_eq!(
            padded_selection_handle_position(&element, SelectionHandle::BottomRight, 5.0),
            Point::new(55.0, 55.0)
        );
    }

    #[test]
    fn padded_selection_geometry_follows_element_rotation() {
        let mut transform = Transform::new(Point::new(10.0, 20.0), Size::new(40.0, 30.0));
        transform.rotation = std::f32::consts::FRAC_PI_2;
        let element = Element::rectangle(ElementId::from_u128(6), transform);

        let actual = padded_selection_corners(&element, 0.0);
        let expected = [
            Point::new(45.0, 15.0),
            Point::new(45.0, 55.0),
            Point::new(15.0, 55.0),
            Point::new(15.0, 15.0),
        ];
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!((actual.x - expected.x).abs() < 1e-5);
            assert!((actual.y - expected.y).abs() < 1e-5);
        }
    }

    #[test]
    fn translating_a_path_moves_its_transform_and_points() {
        let element = Element::freehand(
            ElementId::from_u128(3),
            Transform::new(Point::new(10.0, 20.0), Size::new(20.0, 30.0)),
            vec![Point::new(10.0, 20.0), Point::new(30.0, 50.0)],
        );
        let translated = translated_element(&element, Point::new(5.0, -2.0));
        assert_eq!(translated.transform.position, Point::new(15.0, 18.0));
        assert_eq!(
            translated.points.get(1).copied(),
            Some(Point::new(35.0, 48.0))
        );
    }

    #[test]
    fn group_resize_scales_elements_from_the_opposite_anchor() {
        let element = Element::rectangle(
            ElementId::from_u128(4),
            Transform::new(Point::new(20.0, 30.0), Size::new(20.0, 10.0)),
        );
        let bounds = Rect::new(Point::new(10.0, 20.0), Size::new(40.0, 30.0));
        let resized = group_resized_element(
            &element,
            bounds,
            SelectionHandle::BottomRight,
            Point::new(90.0, 80.0),
        );

        assert_eq!(resized.transform.position, Point::new(30.0, 40.0));
        assert_eq!(resized.transform.size, Size::new(40.0, 20.0));
    }

    #[test]
    fn angular_delta_wraps_at_pi() {
        let delta = angle_delta(3.0, -3.0);
        assert!(delta > 0.0);
        assert!(delta < 1.0);
    }
}
