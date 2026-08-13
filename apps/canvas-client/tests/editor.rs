#![allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::unwrap_used,
    missing_docs
)]

use canvas_client::{connection::SyncController, editor::Editor, storage::Journal};
use canvas_core::{
    ClientId, CrdtDocument, EditorCommand, Element, ElementId, Point, Size, Transform,
};

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

#[test]
fn restoring_a_document_keeps_create_operations_pending_for_sync() {
    let element = Element::rectangle(
        ElementId::from_u128(46),
        Transform::new(Point::default(), Size::new(4.0, 4.0)),
    );
    let mut replica = CrdtDocument::new();
    replica
        .apply(&canvas_core::Operation::new(
            canvas_core::OperationId::new(ClientId::from_u128(90), 1),
            canvas_core::LamportTimestamp::new(1),
            canvas_core::VersionVector::default(),
            canvas_core::OperationKind::Create {
                element: element.clone(),
            },
        ))
        .unwrap();

    let restored = Editor::from_document(ClientId::from_u128(91), &replica.document()).unwrap();

    assert_eq!(restored.pending_operations().len(), 1);
    assert_eq!(restored.document().element(element.id), Some(&element));
}
