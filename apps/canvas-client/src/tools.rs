//! Pointer tools that translate input into editor commands.

use canvas_core::{EditorCommand, Element, ElementId, ElementKind, Point, Size, Transform};
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
    /// Create ellipses.
    Ellipse,
    /// Create lines.
    Line,
    /// Create arrows.
    Arrow,
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
    active_element: Option<ElementId>,
    start: Option<Point>,
    last: Option<Point>,
    points: Vec<Point>,
}

impl ToolController {
    /// Creates a controller for one active tool.
    #[must_use]
    pub fn new(tool: Tool) -> Self {
        Self {
            tool,
            active_element: None,
            start: None,
            last: None,
            points: Vec::new(),
        }
    }

    /// Changes the active tool and clears any in-progress gesture.
    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.cancel();
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
            self.points.push(position);
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
            Tool::Ellipse => Some(shape_from_drag(
                element_id,
                ElementKind::Ellipse,
                start,
                end,
            )),
            Tool::Line => Some(line_from_drag(element_id, ElementKind::Line, start, end)),
            Tool::Arrow => Some(line_from_drag(element_id, ElementKind::Arrow, start, end)),
            Tool::Freehand => {
                let mut points = self.points.clone();
                if points.last().copied() != Some(end) {
                    points.push(end);
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
            self.points.push(position);
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
}

fn shape_from_drag(id: ElementId, kind: ElementKind, start: Point, end: Point) -> Element {
    let position = Point::new(start.x.min(end.x), start.y.min(end.y));
    let size = Size::new((start.x - end.x).abs(), (start.y - end.y).abs());
    let transform = Transform::new(position, size);
    match kind {
        ElementKind::Diamond => Element::diamond(id, transform),
        ElementKind::Triangle => Element::triangle(id, transform),
        _ => Element::new(id, kind, transform),
    }
}

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
