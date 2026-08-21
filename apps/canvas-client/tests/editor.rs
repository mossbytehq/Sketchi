#![allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::unwrap_used,
    missing_docs
)]

use canvas_client::{
    connection::{SyncController, SyncUpdate},
    editor::Editor,
    storage::Journal,
};
use canvas_core::{
    ClientId, CrdtDocument, EditorCommand, Element, ElementId, LamportTimestamp, Operation,
    OperationId, OperationKind, Point, Size, Transform,
};
use canvas_protocol::{RoomId, ServerMessage};

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
fn undo_history_is_bounded() {
    let element_id = ElementId::from_u128(12);
    let mut editor = Editor::new(ClientId::from_u128(1));
    editor
        .execute(EditorCommand::Create(Element::rectangle(
            element_id,
            Transform::new(Point::default(), Size::new(20.0, 20.0)),
        )))
        .unwrap();

    for sequence in 1_u16..=80 {
        editor
            .execute(EditorCommand::SetPosition(
                element_id,
                Point::new(f32::from(sequence), 0.0),
            ))
            .unwrap();
    }

    let mut undo_count = 0;
    while editor.undo().is_ok() {
        undo_count += 1;
    }
    assert_eq!(undo_count, 64);
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

#[test]
fn snapshot_rebase_keeps_local_work_and_applies_server_state() {
    let local_id = ElementId::from_u128(47);
    let remote_id = ElementId::from_u128(48);
    let mut editor = Editor::new(ClientId::from_u128(1));
    editor
        .execute(EditorCommand::Create(Element::rectangle(
            local_id,
            Transform::new(Point::default(), Size::new(4.0, 4.0)),
        )))
        .unwrap();

    let remote_operation = Operation::new(
        OperationId::new(ClientId::from_u128(2), 1),
        LamportTimestamp::new(1),
        canvas_core::VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                remote_id,
                Transform::new(Point::new(10.0, 10.0), Size::new(8.0, 8.0)),
            ),
        },
    );
    let mut server = CrdtDocument::new();
    server.apply(&remote_operation).unwrap();

    editor.apply_snapshot(server.snapshot(), &[]).unwrap();

    assert!(editor.document().element(local_id).is_some());
    assert!(editor.document().element(remote_id).is_some());
    assert_eq!(editor.pending_operations().len(), 1);
}

#[test]
fn remote_sequence_numbers_do_not_advance_a_different_client() {
    let mut editor = Editor::new(ClientId::from_u128(3));
    let remote_operation = Operation::new(
        OperationId::new(ClientId::from_u128(4), 10_000),
        LamportTimestamp::new(1),
        canvas_core::VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                ElementId::from_u128(49),
                Transform::new(Point::default(), Size::new(4.0, 4.0)),
            ),
        },
    );
    editor.apply_remote(&remote_operation).unwrap();

    let operation_id = editor
        .execute(EditorCommand::Create(Element::rectangle(
            ElementId::from_u128(50),
            Transform::new(Point::default(), Size::new(4.0, 4.0)),
        )))
        .unwrap();

    assert_eq!(operation_id, OperationId::new(ClientId::from_u128(3), 1));
}

#[test]
fn server_updates_reconcile_editor_and_durable_queue_together() {
    let room_id = RoomId::from_u128(1);
    let local_id = ElementId::from_u128(51);
    let remote_id = ElementId::from_u128(52);
    let mut editor = Editor::new(ClientId::from_u128(5));
    editor
        .execute(EditorCommand::Create(Element::rectangle(
            local_id,
            Transform::new(Point::default(), Size::new(4.0, 4.0)),
        )))
        .unwrap();
    let local_operation = editor
        .pending_operations()
        .first()
        .cloned()
        .expect("the local create must be pending");
    let mut synchronization = SyncController::new(Journal::open_in_memory().unwrap());
    editor.persist_pending(&mut synchronization).unwrap();

    let remote_operation = Operation::new(
        OperationId::new(ClientId::from_u128(6), 1),
        LamportTimestamp::new(1),
        canvas_core::VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                remote_id,
                Transform::new(Point::new(10.0, 10.0), Size::new(8.0, 8.0)),
            ),
        },
    );
    let mut server = CrdtDocument::new();
    server.apply(&remote_operation).unwrap();

    assert_eq!(
        editor
            .apply_server_message(
                &mut synchronization,
                &ServerMessage::Snapshot {
                    room_id,
                    snapshot: server.snapshot(),
                },
            )
            .unwrap(),
        SyncUpdate::Snapshot
    );
    assert!(editor.document().element(local_id).is_some());
    assert!(editor.document().element(remote_id).is_some());

    let delta_id = ElementId::from_u128(53);
    let delta = Operation::new(
        OperationId::new(ClientId::from_u128(7), 1),
        LamportTimestamp::new(2),
        canvas_core::VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                delta_id,
                Transform::new(Point::new(20.0, 20.0), Size::new(6.0, 6.0)),
            ),
        },
    );
    assert_eq!(
        editor
            .apply_server_message(
                &mut synchronization,
                &ServerMessage::Operations {
                    room_id,
                    operations: vec![delta],
                },
            )
            .unwrap(),
        SyncUpdate::Operations
    );
    assert!(editor.document().element(delta_id).is_some());

    assert_eq!(
        editor
            .apply_server_message(
                &mut synchronization,
                &ServerMessage::Ack {
                    room_id,
                    request_id: 1,
                    accepted: vec![local_operation.id],
                },
            )
            .unwrap(),
        SyncUpdate::Acknowledged
    );
    assert_eq!(synchronization.pending_count().unwrap(), 0);
}
