#![allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::unwrap_used,
    missing_docs
)]

use std::sync::{Arc, Mutex};

use canvas_core::{
    ClientId, CrdtDocument, Element, ElementId, LamportTimestamp, Operation, OperationId,
    OperationKind, Point, Size, Transform, VersionVector,
};
use canvas_protocol::{PresenceState, RoomId, ToolKind};
use canvas_server::room::{RoomError, RoomManager, SubmitOutcome};
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
    assert!(sync.snapshot.elements.is_empty());
    assert_eq!(sync.version.get(client), 1);

    let mut replica = CrdtDocument::from_snapshot(sync.snapshot).unwrap();
    for operation in sync.operations {
        replica.apply(&operation).unwrap();
    }
    assert!(
        replica
            .document()
            .element(ElementId::from_u128(10))
            .is_some()
    );
}

#[test]
fn room_sync_replays_checkpoint_tail_even_when_client_knows_the_tail() {
    let store = Arc::new(Mutex::new(RoomStore::open_in_memory().unwrap()));
    let mut manager = RoomManager::new(store);
    let created = manager.create_room().unwrap();
    let client = ClientId::from_u128(3);
    manager
        .join(created.room_id, &created.token, client)
        .unwrap();

    let operation = create_operation(client, 1, ElementId::from_u128(30));
    manager
        .submit(created.room_id, client, std::slice::from_ref(&operation))
        .unwrap();

    let mut known_version = VersionVector::default();
    known_version.observe(operation.id);
    let sync = manager.sync(created.room_id, &known_version).unwrap();

    assert_eq!(sync.operations, vec![operation]);
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

#[test]
fn room_caps_participants_but_allows_idempotent_rejoin() {
    let store = Arc::new(Mutex::new(RoomStore::open_in_memory().unwrap()));
    let mut manager = RoomManager::new(store);
    let created = manager.create_room().unwrap();

    for (index, name) in ["Ada", "Lin", "Mina", "Noor"].into_iter().enumerate() {
        manager
            .join_named(
                created.room_id,
                &created.token,
                ClientId::from_u128(index as u128 + 1),
                name,
            )
            .unwrap();
    }
    assert!(matches!(
        manager.join_named(
            created.room_id,
            &created.token,
            ClientId::from_u128(5),
            "Fifth",
        ),
        Err(RoomError::RoomFull)
    ));

    manager
        .join_named(
            created.room_id,
            &created.token,
            ClientId::from_u128(2),
            "Lin (reconnected)",
        )
        .unwrap();
    let participants = manager
        .sync(created.room_id, &VersionVector::default())
        .unwrap()
        .participants;
    assert_eq!(participants.len(), 4);
    assert_eq!(
        participants
            .iter()
            .find(|participant| participant.client_id == ClientId::from_u128(2))
            .unwrap()
            .name,
        "Lin (reconnected)"
    );
}

#[test]
fn leaving_requires_membership_and_clears_ephemeral_presence() {
    let store = Arc::new(Mutex::new(RoomStore::open_in_memory().unwrap()));
    let mut manager = RoomManager::new(store);
    let created = manager.create_room().unwrap();
    let client = ClientId::from_u128(7);
    manager
        .join(created.room_id, &created.token, client)
        .unwrap();

    manager
        .update_presence(
            created.room_id,
            PresenceState {
                client_id: client,
                cursor: Some(Point::new(1.0, 2.0)),
                selected_elements: Vec::new(),
                active_tool: ToolKind::Select,
            },
        )
        .unwrap();
    assert_eq!(
        manager
            .sync(created.room_id, &VersionVector::default())
            .unwrap()
            .presence
            .len(),
        1
    );

    manager.leave(created.room_id, client).unwrap();
    assert!(manager.leave(created.room_id, client).is_err());
    manager
        .join(created.room_id, &created.token, client)
        .unwrap();
    assert!(
        manager
            .sync(created.room_id, &VersionVector::default())
            .unwrap()
            .presence
            .is_empty()
    );
}

#[test]
fn room_accepts_out_of_order_operations_and_acknowledges_duplicates() {
    let store = Arc::new(Mutex::new(RoomStore::open_in_memory().unwrap()));
    let mut manager = RoomManager::new(store);
    let created = manager.create_room().unwrap();
    let client = ClientId::from_u128(8);
    let element_id = ElementId::from_u128(80);
    manager
        .join(created.room_id, &created.token, client)
        .unwrap();

    let position = Operation::new(
        OperationId::new(client, 2),
        LamportTimestamp::new(2),
        VersionVector::default(),
        OperationKind::SetPosition {
            element_id,
            position: Point::new(40.0, 50.0),
        },
    );
    let create = create_operation(client, 1, element_id);
    assert_eq!(
        manager
            .submit(created.room_id, client, std::slice::from_ref(&position))
            .unwrap()
            .applied,
        vec![position.clone()]
    );
    assert_eq!(
        manager
            .submit(created.room_id, client, std::slice::from_ref(&create))
            .unwrap()
            .applied,
        vec![create.clone()]
    );

    let duplicate = manager
        .submit(created.room_id, client, std::slice::from_ref(&position))
        .unwrap();
    assert!(duplicate.applied.is_empty());
    assert_eq!(duplicate.acknowledged, vec![position.id]);

    let sync = manager
        .sync(created.room_id, &VersionVector::default())
        .unwrap();
    let mut replica = CrdtDocument::from_snapshot(sync.snapshot).unwrap();
    for operation in sync.operations {
        replica.apply(&operation).unwrap();
    }
    assert_eq!(
        replica
            .document()
            .element(element_id)
            .unwrap()
            .transform
            .position,
        Point::new(40.0, 50.0)
    );
}
