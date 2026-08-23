//! Pointer tools that translate input into editor commands.

use canvas_core::{EditorCommand, Element, ElementId, ElementKind, Point, Rect, Size, Transform};
use canvas_protocol::{ClientMessage, MAX_STROKE_CHUNK_POINTS, RoomId, StrokeId};
use thiserror::Error;

/// Initial local editor tools.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tool {
    /// Select existing elements.
    Select,
    /// Create text elements.
    Text,
    /// Create rectangles.
    Rectangle,
    /// Create diamonds.
    Diamond,
    /// Create triangles.
    Triangle,
    /// Create pentagons.
    Pentagon,
    /// Create hexagons.
    Hexagon,
    /// Create ellipses.
    Ellipse,
    /// Create lines.
    Line,
    /// Create lines that bend clockwise from start to end.
    CurvedLineClockwise,
    /// Create lines that bend counter-clockwise from start to end.
    CurvedLineCounterClockwise,
    /// Create arrows.
    Arrow,
    /// Create arrows that bend clockwise from start to end.
    CurvedArrowClockwise,
    /// Create arrows that bend counter-clockwise from start to end.
    CurvedArrowCounterClockwise,
    /// Create freehand paths.
    Freehand,
    /// Pan the camera.
    Pan,
}

/// Minimal normalized pointer event used by the input adapter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerEvent {
    /// Pointer button pressed for an element/tool gesture.
    Pressed {
        /// Stable element ID reserved by the editor.
        element_id: ElementId,
        /// World-space pointer position.
        position: Point,
    },
    /// Pointer moved while a gesture is active.
    Moved(Point),
    /// Pointer button released.
    Released(Point),
}

/// Output from a tool gesture.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolOutput {
    /// Durable command to be handed to the editor.
    Command(EditorCommand),
    /// Camera movement in screen/world space as selected by the input adapter.
    Pan {
        /// Pointer delta for the camera.
        delta: Point,
    },
}

/// Errors raised while building an ephemeral live-stroke chunk.
#[derive(Debug, Error, PartialEq)]
pub enum LiveStrokeError {
    /// A chunk exceeded the bounded protocol payload.
    #[error("stroke chunk exceeds the maximum size")]
    TooManyPoints,
    /// A preview point is not finite.
    #[error("stroke point must be finite")]
    InvalidPoint,
}

/// Client-side live freehand preview that becomes one durable create command
/// only when the stroke ends.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveStroke {
    room_id: RoomId,
    stroke_id: StrokeId,
    points: Vec<Point>,
}

impl LiveStroke {
    /// Starts a live preview with its first world-space point.
    #[must_use]
    pub fn new(room_id: RoomId, stroke_id: StrokeId, start: Point) -> Self {
        Self {
            room_id,
            stroke_id,
            points: vec![start],
        }
    }

    /// Returns the ephemeral start message.
    #[must_use]
    pub fn start_message(&self) -> ClientMessage {
        ClientMessage::StrokeStart {
            room_id: self.room_id,
            stroke_id: self.stroke_id,
            start: self.points.first().copied().unwrap_or_default(),
        }
    }

    /// Adds a bounded chunk and returns the corresponding ephemeral message.
    ///
    /// # Errors
    ///
    /// Returns [`LiveStrokeError`] when the chunk is oversized or contains a
    /// non-finite point.
    pub fn push_chunk(&mut self, points: Vec<Point>) -> Result<ClientMessage, LiveStrokeError> {
        if points.len() > MAX_STROKE_CHUNK_POINTS {
            return Err(LiveStrokeError::TooManyPoints);
        }
        if points.iter().any(|point| !point.is_finite()) {
            return Err(LiveStrokeError::InvalidPoint);
        }
        self.points.extend(points.clone());
        Ok(ClientMessage::StrokeChunk {
            room_id: self.room_id,
            stroke_id: self.stroke_id,
            points,
        })
    }

    /// Returns the ephemeral end message.
    #[must_use]
    pub const fn end_message(&self) -> ClientMessage {
        ClientMessage::StrokeEnd {
            room_id: self.room_id,
            stroke_id: self.stroke_id,
        }
    }

    /// Finalizes the preview into one operation-first durable create command.
    #[must_use]
    pub fn finalize(&self, element_id: ElementId) -> EditorCommand {
        let start = self.points.first().copied().unwrap_or_default();
        let mut element = Element::freehand(
            element_id,
            Transform::new(start, Size::default()),
            self.points.clone(),
        );
        element.transform.size = bounds_size(&self.points);
        EditorCommand::Create(element)
    }
}

/// Stateful pointer gesture translator.
#[derive(Clone, Debug)]
pub struct ToolController {
    tool: Tool,
    stabilization: f32,
    active_element: Option<ElementId>,
    start: Option<Point>,
    last: Option<Point>,
    points: Vec<Point>,
    canvas_bounds: Option<Rect>,
}

impl ToolController {
    /// Creates a controller for one active tool.
    #[must_use]
    pub fn new(tool: Tool) -> Self {
        Self {
            tool,
            stabilization: 0.0,
            active_element: None,
            start: None,
            last: None,
            points: Vec::new(),
            canvas_bounds: None,
        }
    }

    /// Changes the active tool and clears any in-progress gesture.
    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.cancel();
    }

    /// Applies the current input preferences to future freehand samples.
    pub fn set_input_settings(&mut self, stabilization: f32, _pressure_sensitivity: f32) {
        self.stabilization = if stabilization.is_finite() {
            stabilization.clamp(0.0, 1.0)
        } else {
            0.5
        };
    }

    /// Sets the currently visible world-space canvas bounds for curved tools.
    pub fn set_canvas_bounds(&mut self, bounds: Rect) {
        self.canvas_bounds = Some(bounds);
    }

    /// Returns the active tool.
    #[must_use]
    pub const fn tool(&self) -> Tool {
        self.tool
    }

    /// Handles a normalized pointer event.
    pub fn handle(&mut self, event: PointerEvent) -> Option<ToolOutput> {
        match event {
            PointerEvent::Pressed {
                element_id,
                position,
            } => {
                self.pointer_down(element_id, position);
                None
            }
            PointerEvent::Moved(position) => self.pointer_move(position),
            PointerEvent::Released(position) => self.pointer_up(position),
        }
    }

    /// Starts a gesture with a stable element ID supplied by the editor.
    pub fn pointer_down(&mut self, element_id: ElementId, position: Point) {
        self.active_element = Some(element_id);
        self.start = Some(position);
        self.last = Some(position);
        self.points.clear();
        if self.tool == Tool::Freehand {
            self.points.push(position);
        }
    }

    /// Updates an active gesture, returning camera movement for pan.
    pub fn pointer_move(&mut self, position: Point) -> Option<ToolOutput> {
        let last = self.last.replace(position)?;
        if self.tool == Tool::Pan {
            return Some(ToolOutput::Pan {
                delta: Point::new(position.x - last.x, position.y - last.y),
            });
        }
        if self.tool == Tool::Freehand {
            self.points.push(self.stabilized_sample(position));
        }
        None
    }

    /// Returns the element currently being drawn without committing it.
    #[must_use]
    pub fn preview(&self) -> Option<Element> {
        let start = self.start?;
        let element_id = self.active_element?;
        let end = self.last?;

        match self.tool {
            Tool::Rectangle => Some(shape_from_drag(
                element_id,
                ElementKind::Rectangle,
                start,
                end,
            )),
            Tool::Diamond => Some(shape_from_drag(
                element_id,
                ElementKind::Diamond,
                start,
                end,
            )),
            Tool::Triangle => Some(shape_from_drag(
                element_id,
                ElementKind::Triangle,
                start,
                end,
            )),
            Tool::Pentagon => Some(shape_from_drag(
                element_id,
                ElementKind::Pentagon,
                start,
                end,
            )),
            Tool::Hexagon => Some(shape_from_drag(
                element_id,
                ElementKind::Hexagon,
                start,
                end,
            )),
            Tool::Ellipse => Some(shape_from_drag(
                element_id,
                ElementKind::Ellipse,
                start,
                end,
            )),
            Tool::Line => Some(line_from_drag(element_id, ElementKind::Line, start, end)),
            Tool::CurvedLineClockwise => Some(curved_path_from_drag(
                element_id,
                ElementKind::Line,
                start,
                end,
                true,
                self.canvas_bounds,
            )),
            Tool::CurvedLineCounterClockwise => Some(curved_path_from_drag(
                element_id,
                ElementKind::Line,
                start,
                end,
                false,
                self.canvas_bounds,
            )),
            Tool::Arrow => Some(line_from_drag(element_id, ElementKind::Arrow, start, end)),
            Tool::CurvedArrowClockwise => Some(curved_path_from_drag(
                element_id,
                ElementKind::Arrow,
                start,
                end,
                true,
                self.canvas_bounds,
            )),
            Tool::CurvedArrowCounterClockwise => Some(curved_path_from_drag(
                element_id,
                ElementKind::Arrow,
                start,
                end,
                false,
                self.canvas_bounds,
            )),
            Tool::Freehand => {
                let mut points = self.points.clone();
                if points.last().copied() != Some(end) {
                    points.push(self.stabilized_sample(end));
                }
                let mut element = Element::freehand(
                    element_id,
                    Transform::new(start, Size::default()),
                    points.clone(),
                );
                element.transform.size = bounds_size(&points);
                Some(element)
            }
            Tool::Select | Tool::Text | Tool::Pan => None,
        }
    }

    /// Completes a gesture and emits one durable command where applicable.
    pub fn pointer_up(&mut self, position: Point) -> Option<ToolOutput> {
        self.start?;
        self.active_element?;
        self.last = Some(position);
        if self.tool == Tool::Freehand && self.points.last().copied() != Some(position) {
            self.points.push(self.stabilized_sample(position));
        }
        let output = self
            .preview()
            .map(|element| ToolOutput::Command(EditorCommand::Create(element)));
        self.cancel();
        output
    }

    /// Cancels an in-progress gesture.
    pub fn cancel(&mut self) {
        self.active_element = None;
        self.start = None;
        self.last = None;
        self.points.clear();
    }

    fn stabilized_sample(&self, position: Point) -> Point {
        let previous = self.points.last().copied().unwrap_or(position);
        let smoothing = self.stabilization;
        let responsiveness = (1.0 - smoothing).max(0.05);
        Point::new(
            previous.x + (position.x - previous.x) * responsiveness,
            previous.y + (position.y - previous.y) * responsiveness,
        )
    }
}

fn shape_from_drag(id: ElementId, kind: ElementKind, start: Point, end: Point) -> Element {
    let position = Point::new(start.x.min(end.x), start.y.min(end.y));
    let width = (start.x - end.x).abs();
    let height = (start.y - end.y).abs();
    let size = if matches!(
        kind,
        ElementKind::Rectangle
            | ElementKind::Diamond
            | ElementKind::Triangle
            | ElementKind::Pentagon
            | ElementKind::Hexagon
            | ElementKind::Ellipse
    ) {
        Size::new(width.max(MIN_SHAPE_SIZE), height.max(MIN_SHAPE_SIZE))
    } else {
        Size::new(width, height)
    };
    let transform = Transform::new(position, size);
    match kind {
        ElementKind::Diamond => Element::diamond(id, transform),
        ElementKind::Triangle => Element::triangle(id, transform),
        ElementKind::Pentagon => Element::pentagon(id, transform),
        ElementKind::Hexagon => Element::hexagon(id, transform),
        _ => Element::new(id, kind, transform),
    }
}

const MIN_SHAPE_SIZE: f32 = 4.0;

fn line_from_drag(id: ElementId, kind: ElementKind, start: Point, end: Point) -> Element {
    Element::with_points(
        id,
        kind,
        Transform::new(
            Point::new(start.x.min(end.x), start.y.min(end.y)),
            Size::new((start.x - end.x).abs(), (start.y - end.y).abs()),
        ),
        vec![start, end],
    )
}

const CURVED_PATH_SEGMENTS: u16 = 24;
const CURVED_PATH_BEND: f32 = 0.35;

fn curved_path_from_drag(
    id: ElementId,
    kind: ElementKind,
    start: Point,
    end: Point,
    clockwise: bool,
    canvas_bounds: Option<Rect>,
) -> Element {
    let direction = Point::new(end.x - start.x, end.y - start.y);
    let length = (direction.x * direction.x + direction.y * direction.y).sqrt();
    if length <= f32::EPSILON {
        return line_from_drag(id, kind, start, end);
    }

    let midpoint = Point::new(f32::midpoint(start.x, end.x), f32::midpoint(start.y, end.y));
    let perpendicular = Point::new(-direction.y / length, direction.x / length);
    let bend_direction = if clockwise { -1.0 } else { 1.0 };
    let control = Point::new(
        midpoint.x + perpendicular.x * length * CURVED_PATH_BEND * bend_direction,
        midpoint.y + perpendicular.y * length * CURVED_PATH_BEND * bend_direction,
    );
    let control = canvas_bounds.map_or(control, |bounds| {
        let max = bounds.max();
        Point::new(
            control.x.clamp(bounds.min.x, max.x),
            control.y.clamp(bounds.min.y, max.y),
        )
    });
    let points = (0..=CURVED_PATH_SEGMENTS)
        .map(|index| {
            let t = f32::from(index) / f32::from(CURVED_PATH_SEGMENTS);
            let inverse = 1.0 - t;
            Point::new(
                inverse * inverse * start.x + 2.0 * inverse * t * control.x + t * t * end.x,
                inverse * inverse * start.y + 2.0 * inverse * t * control.y + t * t * end.y,
            )
        })
        .collect::<Vec<_>>();
    let position = Point::new(
        points
            .iter()
            .map(|point| point.x)
            .fold(f32::INFINITY, f32::min),
        points
            .iter()
            .map(|point| point.y)
            .fold(f32::INFINITY, f32::min),
    );
    Element::with_points(
        id,
        kind,
        Transform::new(position, bounds_size(&points)),
        points,
    )
}

fn bounds_size(points: &[Point]) -> Size {
    let Some(first) = points.first() else {
        return Size::default();
    };
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
    Size::new(max_x - min_x, max_y - min_y)
}

#[cfg(test)]
mod tests {
    use canvas_core::{ElementId, ElementKind, Point, Rect, Size};

    use super::{Tool, ToolController};

    #[test]
    fn curved_arrows_bend_on_opposite_sides() {
        let start = Point::new(0.0, 0.0);
        let end = Point::new(100.0, 0.0);
        let mut clockwise = ToolController::new(Tool::CurvedArrowClockwise);
        clockwise.pointer_down(ElementId::from_u128(1), start);
        clockwise.pointer_move(end);
        let clockwise_preview = clockwise.preview();

        let mut counter_clockwise = ToolController::new(Tool::CurvedArrowCounterClockwise);
        counter_clockwise.pointer_down(ElementId::from_u128(2), start);
        counter_clockwise.pointer_move(end);
        let counter_clockwise_preview = counter_clockwise.preview();

        assert!(clockwise_preview.as_ref().is_some_and(|preview| {
            preview.points.len() == 25
                && preview.points.get(12).is_some_and(|point| point.y < 0.0)
                && preview.points.first() == Some(&start)
                && preview.points.last() == Some(&end)
        }));
        assert!(counter_clockwise_preview.as_ref().is_some_and(|preview| {
            preview.points.len() == 25
                && preview.points.get(12).is_some_and(|point| point.y > 0.0)
                && preview.points.first() == Some(&start)
                && preview.points.last() == Some(&end)
        }));
    }

    #[test]
    fn curved_arrow_bend_is_clamped_to_visible_canvas_bounds() {
        let mut tool = ToolController::new(Tool::CurvedArrowCounterClockwise);
        tool.set_canvas_bounds(Rect::new(Point::new(0.0, 0.0), Size::new(100.0, 20.0)));
        tool.pointer_down(ElementId::from_u128(3), Point::new(0.0, 0.0));
        tool.pointer_move(Point::new(100.0, 0.0));

        assert!(
            tool.preview()
                .is_some_and(|preview| preview.points.iter().all(|point| point.y <= 20.0))
        );
    }

    #[test]
    fn curved_line_tools_keep_line_geometry_without_arrowheads() {
        let mut tool = ToolController::new(Tool::CurvedLineClockwise);
        tool.pointer_down(ElementId::from_u128(4), Point::new(0.0, 0.0));
        tool.pointer_move(Point::new(100.0, 0.0));

        assert!(tool.preview().is_some_and(|preview| {
            preview.kind == ElementKind::Line && preview.points.len() == 25
        }));
    }
}
