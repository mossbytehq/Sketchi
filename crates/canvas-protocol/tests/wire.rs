#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use canvas_core::{
    ClientId, CrdtDocument, Element, ElementId, EmbeddedImage, LamportTimestamp, MAX_IMAGE_BYTES,
    Operation, OperationId, OperationKind, Point, Size, Transform, VersionVector,
};
use canvas_protocol::{
    ClientMessage, PROTOCOL_VERSION, PresenceState, RoomId, ServerMessage, ToolKind, decode_client,
    decode_server, encode_client, encode_server,
};

fn create_operation() -> Operation {
    let mut deps = VersionVector::default();
    deps.observe(OperationId::new(ClientId::from_u128(9), 4));
    Operation::new(
        OperationId::new(ClientId::from_u128(1), 1),
        LamportTimestamp::new(1),
        deps,
        OperationKind::Create {
            element: Element::rectangle(
                ElementId::from_u128(2),
                Transform::new(Point::new(1.0, 2.0), Size::new(30.0, 40.0)),
            ),
        },
    )
}

#[test]
fn hello_has_a_stable_versioned_json_shape() {
    let message = ClientMessage::Hello {
        client_id: ClientId::from_u128(1),
        client_name: Some("Ada".to_owned()),
    };
    let encoded = encode_client(&message).unwrap();
    let json = String::from_utf8(encoded).unwrap();
    assert_eq!(
        json,
        r#"{"protocol_version":2,"message":{"type":"hello","client_id":"00000000-0000-0000-0000-000000000001","client_name":"Ada"}}"#
    );
    assert_eq!(decode_client(json.as_bytes()).unwrap(), message);
}

#[test]
fn operation_batches_and_presence_round_trip() {
    let room_id = RoomId::from_u128(99);
    let operation = create_operation();
    let submit = ClientMessage::SubmitOperations {
        room_id,
        request_id: 7,
        operations: vec![operation.clone()],
    };
    assert_eq!(
        decode_client(&encode_client(&submit).unwrap()).unwrap(),
        submit
    );

    let presence = ServerMessage::Presence {
        room_id,
        state: PresenceState {
            client_id: ClientId::from_u128(1),
            cursor: Some(Point::new(12.0, 15.0)),
            selected_elements: vec![ElementId::from_u128(2)],
            active_tool: ToolKind::Rectangle,
        },
    };
    assert_eq!(
        decode_server(&encode_server(&presence).unwrap()).unwrap(),
        presence
    );
}

#[test]
fn embedded_image_operation_round_trips_as_base64_json() {
    let room_id = RoomId::from_u128(99);
    let operation = Operation::new(
        OperationId::new(ClientId::from_u128(1), 2),
        LamportTimestamp::new(2),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::image(
                ElementId::from_u128(3),
                Transform::new(Point::default(), Size::new(20.0, 20.0)),
                EmbeddedImage::new("image/png", 1, 1, vec![1, 2, 3]),
            ),
        },
    );
    let submit = ClientMessage::SubmitOperations {
        room_id,
        request_id: 8,
        operations: vec![operation],
    };

    let encoded = encode_client(&submit).unwrap();
    let json = String::from_utf8(encoded.clone()).unwrap();
    assert!(json.contains("AQID"));
    assert!(!json.contains("[1,2,3]"));
    assert_eq!(decode_client(&encoded).unwrap(), submit);
}

#[test]
fn maximum_embedded_image_fits_operation_and_snapshot_frame_bounds() {
    let image = EmbeddedImage::new("image/png", 4_096, 4_096, vec![0; MAX_IMAGE_BYTES]);
    let operation = Operation::new(
        OperationId::new(ClientId::from_u128(1), 3),
        LamportTimestamp::new(3),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::image(
                ElementId::from_u128(4),
                Transform::new(Point::default(), Size::new(20.0, 20.0)),
                image,
            ),
        },
    );
    let submit = ClientMessage::SubmitOperations {
        room_id: RoomId::from_u128(99),
        request_id: 9,
        operations: vec![operation.clone()],
    };
    assert!(encode_client(&submit).unwrap().len() <= canvas_protocol::MAX_FRAME_BYTES);

    let mut document = CrdtDocument::new();
    document.apply(&operation).unwrap();
    let snapshot = ServerMessage::Snapshot {
        room_id: RoomId::from_u128(99),
        snapshot: document.snapshot(),
    };
    assert!(encode_server(&snapshot).unwrap().len() <= canvas_protocol::MAX_FRAME_BYTES);
}

#[test]
fn server_snapshot_and_ack_round_trip() {
    let room_id = RoomId::from_u128(99);
    let mut document = canvas_core::CrdtDocument::new();
    document.apply(&create_operation()).unwrap();
    let snapshot = document.snapshot();
    let server = ServerMessage::Snapshot { room_id, snapshot };
    assert_eq!(
        decode_server(&encode_server(&server).unwrap()).unwrap(),
        server
    );

    let ack = ServerMessage::Ack {
        room_id,
        request_id: 7,
        accepted: vec![OperationId::new(ClientId::from_u128(1), 1)],
    };
    assert_eq!(decode_server(&encode_server(&ack).unwrap()).unwrap(), ack);
}

#[test]
fn protocol_version_is_present_in_the_envelope() {
    let encoded = encode_client(&ClientMessage::Ping { nonce: 42 }).unwrap();
    assert!(
        String::from_utf8(encoded)
            .unwrap()
            .contains(&format!("protocol_version\":{PROTOCOL_VERSION}"))
    );
}
