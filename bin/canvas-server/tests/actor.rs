#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use std::sync::{Arc, Mutex};

use canvas_core::{
    ClientId, Element, ElementId, LamportTimestamp, Operation, OperationId, OperationKind, Point,
    Size, Transform, VersionVector,
};
use canvas_protocol::RoomId;
use canvas_server::{actor::spawn, room::Room, store::RoomStore};

#[tokio::test]
async fn room_actor_serializes_join_submit_and_sync() {
    let mut store = RoomStore::open_in_memory().unwrap();
    let room_id = RoomId::from_u128(100);
    store.create_room(room_id, "hash").unwrap();
    let store = Arc::new(Mutex::new(store));
    let room = Room::load(room_id, Arc::clone(&store)).unwrap();
    let (handle, task) = spawn(room);
    let client_id = ClientId::from_u128(1);
    handle.join(client_id).await.unwrap();
    let operation = Operation::new(
        OperationId::new(client_id, 1),
        LamportTimestamp::new(1),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                ElementId::from_u128(101),
                Transform::new(Point::default(), Size::new(10.0, 10.0)),
            ),
        },
    );
    let outcome = handle.submit(client_id, vec![operation]).await.unwrap();
    assert_eq!(outcome.applied.len(), 1);
    assert_eq!(
        handle
            .sync(VersionVector::default())
            .await
            .unwrap()
            .operations
            .len(),
        1
    );
    task.abort();
}
