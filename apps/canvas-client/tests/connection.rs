#![allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::panic,
    clippy::unwrap_used,
    missing_docs
)]

use std::time::{Duration, Instant};

use canvas_client::{
    connection::{ConnectionConfig, ConnectionError, PresenceThrottle, SyncController, SyncUpdate},
    storage::Journal,
};
use canvas_core::{
    ClientId, CrdtDocument, Element, ElementId, LamportTimestamp, Operation, OperationId,
    OperationKind, Point, Size, Transform, VersionVector,
};
use canvas_protocol::{ClientMessage, RoomId, ServerMessage, ToolKind};

fn operation(sequence: u64) -> Operation {
    Operation::new(
        OperationId::new(ClientId::from_u128(1), sequence),
        LamportTimestamp::new(sequence),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                ElementId::from_u128(u128::from(sequence) + 20),
                Transform::new(Point::default(), Size::new(10.0, 10.0)),
            ),
        },
    )
}

#[test]
fn pending_operations_replay_in_stable_batches_and_survive_retry() {
    let room_id = RoomId::from_u128(7);
    let first = operation(1);
    let second = operation(2);
    let third = operation(3);
    let mut controller = SyncController::new(Journal::open_in_memory().unwrap());
    controller
        .enqueue_all(&[third.clone(), first.clone(), second.clone()])
        .unwrap();

    let replay = controller.replay_pending(room_id, 9, 2).unwrap();
    assert_eq!(replay.len(), 2);
    assert_eq!(
        replay,
        vec![
            ClientMessage::SubmitOperations {
                room_id,
                request_id: 9,
                operations: vec![first.clone(), second.clone()],
            },
            ClientMessage::SubmitOperations {
                room_id,
                request_id: 10,
                operations: vec![third.clone()],
            },
        ]
    );
    assert_eq!(controller.replay_pending(room_id, 9, 2).unwrap(), replay);
}

#[test]
fn acknowledgements_are_idempotent_and_only_remove_accepted_operations() {
    let room_id = RoomId::from_u128(8);
    let first = operation(1);
    let second = operation(2);
    let mut controller = SyncController::new(Journal::open_in_memory().unwrap());
    controller
        .enqueue_all(&[first.clone(), second.clone()])
        .unwrap();

    let ack = ServerMessage::Ack {
        room_id,
        request_id: 9,
        accepted: vec![first.id, first.id],
    };
    assert_eq!(
        controller.apply_server_message(&ack).unwrap(),
        SyncUpdate::Acknowledged
    );
    assert_eq!(
        controller.pending_operations().unwrap(),
        vec![second.clone()]
    );
    assert_eq!(
        controller.apply_server_message(&ack).unwrap(),
        SyncUpdate::Acknowledged
    );
    assert_eq!(controller.pending_operations().unwrap(), vec![second]);
}

#[test]
fn sync_metadata_tracks_snapshot_delta_and_builds_join_request() {
    let room_id = RoomId::from_u128(9);
    let first = operation(1);
    let second = operation(2);
    let mut snapshot_document = CrdtDocument::new();
    snapshot_document.apply(&first).unwrap();
    let snapshot = snapshot_document.snapshot();
    let mut controller = SyncController::new(Journal::open_in_memory().unwrap());

    assert_eq!(
        controller
            .apply_server_message(&ServerMessage::Snapshot { room_id, snapshot })
            .unwrap(),
        SyncUpdate::Snapshot
    );
    assert_eq!(controller.known_version().get(ClientId::from_u128(1)), 1);

    assert_eq!(
        controller
            .apply_server_message(&ServerMessage::Operations {
                room_id,
                operations: vec![second.clone()],
            })
            .unwrap(),
        SyncUpdate::Operations
    );
    assert_eq!(controller.known_version().get(ClientId::from_u128(1)), 2);
    assert_eq!(
        controller.join_message(room_id, "capability"),
        ClientMessage::JoinRoom {
            room_id,
            capability_token: "capability".to_owned(),
            known_version: controller.known_version().clone(),
        }
    );
}

#[test]
fn presence_throttle_coalesces_updates_and_emits_at_most_one_per_interval() {
    let start = Instant::now();
    let room_id = RoomId::from_u128(10);
    let mut throttle = PresenceThrottle::new(Duration::from_millis(100)).unwrap();
    let first = presence(Point::new(1.0, 1.0));
    let latest = presence(Point::new(3.0, 3.0));

    assert_eq!(
        throttle.offer(room_id, first.clone(), start),
        Some(ClientMessage::Presence {
            room_id,
            state: first,
        })
    );
    assert!(
        throttle
            .offer(
                room_id,
                presence(Point::new(2.0, 2.0)),
                start + Duration::from_millis(10)
            )
            .is_none()
    );
    assert!(
        throttle
            .offer(room_id, latest.clone(), start + Duration::from_millis(20))
            .is_none()
    );
    assert!(throttle.has_pending());
    assert!(throttle.flush(start + Duration::from_millis(99)).is_none());
    assert_eq!(
        throttle.flush(start + Duration::from_millis(100)),
        Some(ClientMessage::Presence {
            room_id,
            state: latest,
        })
    );
    assert!(!throttle.has_pending());
}

#[test]
fn websocket_connection_config_requires_a_valid_pin_for_tls() {
    let pin = "ab".repeat(32);
    let config = ConnectionConfig::new("wss://127.0.0.1:3210/ws", Some(&pin)).unwrap();
    assert_eq!(config.endpoint(), "wss://127.0.0.1:3210/ws");
    assert!(ConnectionConfig::new("wss://127.0.0.1:3210/ws", None).is_err());
    assert!(matches!(
        ConnectionConfig::new("http://127.0.0.1:3210/ws", None),
        Err(ConnectionError::InvalidEndpoint(_))
    ));
}

#[test]
fn presence_offer_at_deadline_discards_stale_coalesced_state() {
    let start = Instant::now();
    let room_id = RoomId::from_u128(11);
    let mut throttle = PresenceThrottle::new(Duration::from_millis(100)).unwrap();

    let _ = throttle.offer(room_id, presence(Point::new(1.0, 1.0)), start);
    assert!(
        throttle
            .offer(
                room_id,
                presence(Point::new(2.0, 2.0)),
                start + Duration::from_millis(10)
            )
            .is_none()
    );
    assert!(
        throttle
            .offer(
                room_id,
                presence(Point::new(3.0, 3.0)),
                start + Duration::from_millis(100)
            )
            .is_some()
    );
    assert!(!throttle.has_pending());
    assert!(throttle.flush(start + Duration::from_millis(200)).is_none());
}

fn presence(cursor: Point) -> canvas_protocol::PresenceState {
    canvas_protocol::PresenceState {
        client_id: ClientId::from_u128(1),
        cursor: Some(cursor),
        selected_elements: Vec::new(),
        active_tool: ToolKind::Select,
    }
}
