#![allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::unwrap_used,
    missing_docs
)]

use canvas_client::{connection::SyncController, editor::Editor, storage::Journal};
use canvas_core::{ClientId, EditorCommand, Element, ElementId, Point, Size, Transform};

#[test]
fn editor_applies_local_commands_as_operations_and_tracks_pending_work() {
    let element_id = ElementId::from_u128(10);
    let mut editor = Editor::new(ClientId::from_u128(1));
    editor
        .execute(EditorCommand::Create(Element::rectangle(
            element_id,
            Transform::new(Point::new(0.0, 0.0), Size::new(20.0, 20.0)),
        )))
        .unwrap();
    assert_eq!(editor.pending_operations().len(), 1);
    assert!(editor.document().element(element_id).is_some());
}

#[test]
fn position_undo_and_redo_are_compensating_operations() {
    let element_id = ElementId::from_u128(11);
    let mut editor = Editor::new(ClientId::from_u128(1));
    editor
        .execute(EditorCommand::Create(Element::rectangle(
            element_id,
            Transform::new(Point::new(0.0, 0.0), Size::new(20.0, 20.0)),
        )))
        .unwrap();
    editor
        .execute(EditorCommand::SetPosition(
            element_id,
            Point::new(50.0, 60.0),
        ))
        .unwrap();
    editor.undo().unwrap();
    assert_eq!(
        editor
            .document()
            .element(element_id)
            .unwrap()
            .transform
            .position,
        Point::new(0.0, 0.0)
    );
    editor.redo().unwrap();
    assert_eq!(
        editor
            .document()
            .element(element_id)
            .unwrap()
            .transform
            .position,
        Point::new(50.0, 60.0)
    );
}

#[test]
fn local_operations_can_be_transferred_to_the_durable_sync_journal() {
    let client_id = ClientId::from_u128(44);
    let mut editor = Editor::new(client_id);
    editor
        .execute(EditorCommand::Create(Element::rectangle(
            ElementId::from_u128(45),
            Transform::new(Point::default(), Size::new(4.0, 4.0)),
        )))
        .unwrap();
    let mut sync = SyncController::new(Journal::open_in_memory().unwrap());

    assert_eq!(editor.persist_pending(&mut sync).unwrap(), 1);
    assert!(editor.pending_operations().is_empty());
    assert_eq!(sync.pending_count().unwrap(), 1);
}
