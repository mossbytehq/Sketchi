#![allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    missing_docs
)]

use canvas_client::tools::{LiveStroke, PointerEvent, Tool, ToolController, ToolOutput};
use canvas_core::{EditorCommand, ElementId, ElementKind, Point};
use canvas_protocol::{ClientMessage, RoomId, StrokeId};

#[test]
fn rectangle_tool_emits_a_create_command_on_pointer_up() {
    let element_id = ElementId::from_u128(4);
    let mut tools = ToolController::new(Tool::Rectangle);
    tools.pointer_down(element_id, Point::new(100.0, 100.0));
    tools.pointer_move(Point::new(40.0, 20.0));
    let output = tools.pointer_up(Point::new(40.0, 20.0)).unwrap();
    let ToolOutput::Command(EditorCommand::Create(element)) = output else {
        panic!("rectangle tool did not create an element");
    };
    assert_eq!(element.id, element_id);
    assert_eq!(element.transform.position, Point::new(40.0, 20.0));
    assert_eq!(element.transform.size.width, 60.0);
    assert_eq!(element.transform.size.height, 80.0);
}

#[test]
fn shape_preview_is_available_before_pointer_up() {
    let element_id = ElementId::from_u128(10);
    let mut tools = ToolController::new(Tool::Rectangle);
    tools.pointer_down(element_id, Point::new(100.0, 100.0));
    tools.pointer_move(Point::new(40.0, 20.0));

    let preview = tools.preview().expect("active shape should be previewable");
    assert_eq!(preview.id, element_id);
    assert_eq!(preview.kind, ElementKind::Rectangle);
    assert_eq!(preview.transform.position, Point::new(40.0, 20.0));
    assert_eq!(preview.transform.size.width, 60.0);
    assert_eq!(preview.transform.size.height, 80.0);
}

#[test]
fn diamond_tool_commits_a_diamond_element() {
    let element_id = ElementId::from_u128(11);
    let mut tools = ToolController::new(Tool::Diamond);
    tools.pointer_down(element_id, Point::new(100.0, 100.0));
    tools.pointer_move(Point::new(40.0, 20.0));
    let output = tools.pointer_up(Point::new(40.0, 20.0)).unwrap();
    let ToolOutput::Command(EditorCommand::Create(element)) = output else {
        panic!("diamond tool did not create an element");
    };
    assert_eq!(element.kind, ElementKind::Diamond);
    assert_eq!(element.transform.position, Point::new(40.0, 20.0));
    assert_eq!(element.transform.size, canvas_core::Size::new(60.0, 80.0));
}

#[test]
fn triangle_tool_commits_a_triangle_element() {
    let element_id = ElementId::from_u128(12);
    let mut tools = ToolController::new(Tool::Triangle);
    tools.pointer_down(element_id, Point::new(100.0, 100.0));
    tools.pointer_move(Point::new(40.0, 20.0));
    let output = tools.pointer_up(Point::new(40.0, 20.0)).unwrap();
    let ToolOutput::Command(EditorCommand::Create(element)) = output else {
        panic!("triangle tool did not create an element");
    };
    assert_eq!(element.kind, ElementKind::Triangle);
    assert_eq!(element.transform.position, Point::new(40.0, 20.0));
    assert_eq!(element.transform.size, canvas_core::Size::new(60.0, 80.0));
}

#[test]
fn freehand_tool_commits_one_bounded_command_on_release() {
    let element_id = ElementId::from_u128(5);
    let mut tools = ToolController::new(Tool::Freehand);
    tools.pointer_down(element_id, Point::new(0.0, 0.0));
    tools.pointer_move(Point::new(1.0, 2.0));
    tools.pointer_move(Point::new(3.0, 4.0));
    let output = tools.pointer_up(Point::new(5.0, 6.0)).unwrap();
    let ToolOutput::Command(EditorCommand::Create(element)) = output else {
        panic!("freehand tool did not create an element");
    };
    assert_eq!(element.points.len(), 4);
    assert_eq!(element.points[3], Point::new(5.0, 6.0));
}

#[test]
fn pan_is_reported_as_a_camera_delta_without_mutating_the_document() {
    let mut tools = ToolController::new(Tool::Pan);
    tools.pointer_down(ElementId::from_u128(6), Point::new(10.0, 20.0));
    let output = tools.pointer_move(Point::new(20.0, 50.0));
    assert!(matches!(output, Some(ToolOutput::Pan { delta }) if delta == Point::new(10.0, 30.0)));
    assert!(tools.pointer_up(Point::new(20.0, 50.0)).is_none());
    let _ = PointerEvent::Moved(Point::new(0.0, 0.0));
}

#[test]
fn live_stroke_emits_ephemeral_messages_and_one_durable_command() {
    let room_id = RoomId::from_u128(7);
    let stroke_id = StrokeId::from_u128(8);
    let start = Point::new(1.0, 2.0);
    let mut stroke = LiveStroke::new(room_id, stroke_id, start);

    assert_eq!(
        stroke.start_message(),
        ClientMessage::StrokeStart {
            room_id,
            stroke_id,
            start,
        }
    );
    assert_eq!(
        stroke
            .push_chunk(vec![Point::new(2.0, 3.0), Point::new(4.0, 5.0)])
            .unwrap(),
        ClientMessage::StrokeChunk {
            room_id,
            stroke_id,
            points: vec![Point::new(2.0, 3.0), Point::new(4.0, 5.0)],
        }
    );
    assert_eq!(
        stroke.end_message(),
        ClientMessage::StrokeEnd { room_id, stroke_id }
    );
    let EditorCommand::Create(element) = stroke.finalize(ElementId::from_u128(9)) else {
        panic!("live stroke did not finalize as a durable create");
    };
    assert_eq!(element.points.len(), 3);
    assert_eq!(element.transform.size.width, 3.0);
    assert_eq!(element.transform.size.height, 3.0);
}
