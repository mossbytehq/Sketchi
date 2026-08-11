#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use canvas_core::{
    ApplyResult, ClientId, CrdtDocument, Element, ElementId, LamportTimestamp, Operation,
    OperationId, OperationKind, Point, Size, Transform, VersionVector,
};

#[test]
fn snapshot_round_trip_preserves_state_and_idempotence() {
    let element_id = ElementId::from_u128(101);
    let operation = Operation::new(
        OperationId::new(ClientId::from_u128(1), 1),
        LamportTimestamp::new(9),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                element_id,
                Transform::new(Point::new(4.0, 5.0), Size::new(6.0, 7.0)),
            ),
        },
    );

    let mut original = CrdtDocument::new();
    original.apply(&operation).unwrap();
    let snapshot = original.snapshot();
    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let decoded = serde_json::from_slice(&encoded).unwrap();
    let mut restored = CrdtDocument::from_snapshot(decoded).unwrap();

    assert_eq!(restored.document(), original.document());
    assert_eq!(restored.snapshot(), snapshot);
    assert_eq!(restored.apply(&operation).unwrap(), ApplyResult::Duplicate);
}
