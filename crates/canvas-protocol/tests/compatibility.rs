#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use canvas_core::{ClientId, ElementId, OperationId, Point, VersionVector};
use canvas_protocol::{
    ClientMessage, MAX_FRAME_BYTES, MAX_OPERATIONS_PER_MESSAGE, PROTOCOL_VERSION, PresenceState,
    ProtocolError, RoomId, ToolKind, decode_client,
};

#[test]
fn unsupported_versions_and_unknown_messages_are_rejected() {
    let wrong_version = br#"{"protocol_version":999,"message":{"type":"ping","nonce":1}}"#;
    assert!(matches!(
        decode_client(wrong_version),
        Err(ProtocolError::UnsupportedVersion(999))
    ));

    let unknown = br#"{"protocol_version":1,"message":{"type":"future_feature"}}"#;
    assert!(matches!(
        decode_client(unknown),
        Err(ProtocolError::Json(_))
    ));
}

#[test]
fn bounded_payloads_are_rejected_before_transport() {
    let operations = (0..=MAX_OPERATIONS_PER_MESSAGE)
        .map(|sequence| {
            canvas_core::Operation::new(
                OperationId::new(ClientId::from_u128(1), sequence as u64 + 1),
                canvas_core::LamportTimestamp::new(sequence as u64 + 1),
                VersionVector::default(),
                canvas_core::OperationKind::Delete {
                    element_id: ElementId::from_u128(sequence as u128 + 1),
                },
            )
        })
        .collect();
    let message = ClientMessage::SubmitOperations {
        room_id: RoomId::from_u128(1),
        request_id: 1,
        operations,
    };
    assert!(matches!(
        message.validate(),
        Err(ProtocolError::TooManyOperations)
    ));

    let oversized = vec![b'x'; MAX_FRAME_BYTES + 1];
    assert!(matches!(
        decode_client(&oversized),
        Err(ProtocolError::FrameTooLarge)
    ));
}

#[test]
fn presence_and_ephemeral_strokes_are_validated_without_becoming_operations() {
    let presence = PresenceState {
        client_id: ClientId::from_u128(1),
        cursor: Some(Point::new(2.0, 3.0)),
        selected_elements: vec![ElementId::from_u128(2)],
        active_tool: ToolKind::Freehand,
    };
    assert!(presence.validate().is_ok());
    assert_eq!(PROTOCOL_VERSION, 1);
}
