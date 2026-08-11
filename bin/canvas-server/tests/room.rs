#![allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::unwrap_used,
    missing_docs
)]

use std::sync::{Arc, Mutex};

use canvas_core::{
    ClientId, Element, ElementId, LamportTimestamp, Operation, OperationId, OperationKind, Point,
    Size, Transform, VersionVector,
};
use canvas_protocol::RoomId;
use canvas_server::room::{RoomManager, SubmitOutcome};
use canvas_server::store::RoomStore;

fn create_operation(client_id: ClientId, sequence: u64, element_id: ElementId) -> Operation {
    Operation::new(
        OperationId::new(client_id, sequence),
        LamportTimestamp::new(sequence),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                element_id,
                Transform::new(Point::default(), Size::new(20.0, 20.0)),
            ),
        },
    )
}

#[test]
fn room_applies_before_acknowledging_and_syncs_snapshot_plus_delta() {
    let store = Arc::new(Mutex::new(RoomStore::open_in_memory().unwrap()));
    let mut manager = RoomManager::new(store);
    let created = manager.create_room().unwrap();
    let client = ClientId::from_u128(1);
    manager
        .join(created.room_id, &created.token, client)
        .unwrap();

    let operation = create_operation(client, 1, ElementId::from_u128(10));
    let outcome = manager
        .submit(created.room_id, client, std::slice::from_ref(&operation))
        .unwrap();
    assert_eq!(
        outcome,
        SubmitOutcome {
            applied: vec![operation.clone()],
            acknowledged: vec![operation.id],
        }
    );
    assert!(
        manager
            .document(created.room_id)
            .unwrap()
            .element(ElementId::from_u128(10))
            .is_some()
    );

    let duplicate = manager
        .submit(created.room_id, client, std::slice::from_ref(&operation))
        .unwrap();
    assert_eq!(
        duplicate,
        SubmitOutcome {
            applied: Vec::new(),
            acknowledged: vec![operation.id],
        }
    );
    let sync = manager
        .sync(created.room_id, &VersionVector::default())
        .unwrap();
    assert_eq!(sync.operations.len(), 1);
    assert_eq!(sync.snapshot.elements.len(), 1);
}

#[test]
fn room_rejects_operations_from_a_different_client_identity() {
    let store = Arc::new(Mutex::new(RoomStore::open_in_memory().unwrap()));
    let mut manager = RoomManager::new(store);
    let created = manager.create_room().unwrap();
    let client = ClientId::from_u128(1);
    let other = ClientId::from_u128(2);
    manager
        .join(created.room_id, &created.token, client)
        .unwrap();
    let error = manager
        .submit(
            created.room_id,
            client,
            &[create_operation(other, 1, ElementId::from_u128(11))],
        )
        .unwrap_err();
    assert!(error.to_string().contains("client"));
}

#[test]
fn explicit_room_ids_are_supported_for_restart_loading() {
    let _ = RoomId::from_u128(123);
}
