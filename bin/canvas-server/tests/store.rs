#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use canvas_core::{
    ClientId, Element, ElementId, LamportTimestamp, Operation, OperationId, OperationKind, Point,
    Size, Transform, VersionVector,
};
use canvas_protocol::RoomId;
use canvas_server::auth::CapabilityToken;
use canvas_server::store::RoomStore;

fn operation() -> Operation {
    Operation::new(
        OperationId::new(ClientId::from_u128(1), 1),
        LamportTimestamp::new(1),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                ElementId::from_u128(2),
                Transform::new(Point::default(), Size::new(10.0, 10.0)),
            ),
        },
    )
}

#[test]
fn sqlite_store_persists_room_tokens_operations_and_snapshots() {
    let mut store = RoomStore::open_in_memory().unwrap();
    let room_id = RoomId::from_u128(3);
    let token = CapabilityToken::generate();
    store.create_room(room_id, &token.hash()).unwrap();
    let operation = operation();
    store.append_operation(room_id, &operation).unwrap();
    assert_eq!(store.load_operations(room_id).unwrap(), vec![operation]);
    assert!(store.token_hash(room_id).unwrap().is_some());
}
