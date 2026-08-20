#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use canvas_core::{
    ApplyResult, ClientId, CrdtDocument, Element, ElementId, LamportTimestamp, Operation,
    OperationId, OperationKind, Point, RegisterMetadata, Size, StylePatch, Transform,
    VersionVector,
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

#[test]
fn snapshot_rejects_registers_newer_than_the_snapshot_clock() {
    let operation = Operation::new(
        OperationId::new(ClientId::from_u128(1), 1),
        LamportTimestamp::new(9),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                ElementId::from_u128(102),
                Transform::new(Point::default(), Size::new(10.0, 10.0)),
            ),
        },
    );
    let mut document = CrdtDocument::new();
    document.apply(&operation).unwrap();

    let mut snapshot = document.snapshot();
    snapshot.clock = LamportTimestamp::new(1);

    assert!(matches!(
        CrdtDocument::from_snapshot(snapshot),
        Err(canvas_core::CrdtError::InvalidSnapshot(_))
    ));
}

#[test]
fn snapshot_rejects_retained_operations_newer_than_the_snapshot_clock() {
    let operation = Operation::new(
        OperationId::new(ClientId::from_u128(1), 1),
        LamportTimestamp::new(9),
        VersionVector::default(),
        OperationKind::SetStyle {
            element_id: ElementId::from_u128(104),
            style: StylePatch::default(),
        },
    );
    let mut document = CrdtDocument::new();
    document.apply(&operation).unwrap();

    let mut snapshot = document.snapshot();
    snapshot.elements.clear();
    snapshot.clock = LamportTimestamp::new(1);

    assert!(matches!(
        CrdtDocument::from_snapshot(snapshot),
        Err(canvas_core::CrdtError::InvalidSnapshot(_))
    ));
}

#[test]
fn snapshot_rejects_nonzero_register_metadata_with_invalid_operation_identity() {
    let operation = Operation::new(
        OperationId::new(ClientId::from_u128(1), 1),
        LamportTimestamp::new(1),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                ElementId::from_u128(103),
                Transform::new(Point::default(), Size::new(10.0, 10.0)),
            ),
        },
    );
    let mut document = CrdtDocument::new();
    document.apply(&operation).unwrap();

    let mut snapshot = document.snapshot();
    snapshot
        .elements
        .first_mut()
        .expect("snapshot contains the created element")
        .position
        .metadata = RegisterMetadata {
        timestamp: LamportTimestamp::new(0),
        operation_id: OperationId::new(ClientId::from_u128(1), 1),
    };

    assert!(matches!(
        CrdtDocument::from_snapshot(snapshot),
        Err(canvas_core::CrdtError::InvalidSnapshot(_))
    ));
}

#[test]
fn snapshot_rejects_version_vector_entries_without_retained_coverage() {
    let operation = Operation::new(
        OperationId::new(ClientId::from_u128(1), 1),
        LamportTimestamp::new(1),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                ElementId::from_u128(105),
                Transform::new(Point::default(), Size::new(10.0, 10.0)),
            ),
        },
    );
    let mut document = CrdtDocument::new();
    document.apply(&operation).unwrap();

    let mut snapshot = document.snapshot();
    snapshot
        .version_vector
        .observe(OperationId::new(ClientId::from_u128(1), 2));

    assert!(matches!(
        CrdtDocument::from_snapshot(snapshot),
        Err(canvas_core::CrdtError::InvalidSnapshot(_))
    ));
}

#[test]
fn snapshot_accepts_noncontiguous_max_seen_operation_sequences() {
    let operation = Operation::new(
        OperationId::new(ClientId::from_u128(1), 3),
        LamportTimestamp::new(3),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                ElementId::from_u128(107),
                Transform::new(Point::default(), Size::new(10.0, 10.0)),
            ),
        },
    );
    let mut document = CrdtDocument::new();
    document.apply(&operation).unwrap();

    assert!(CrdtDocument::from_snapshot(document.snapshot()).is_ok());
}

#[test]
fn snapshot_rejects_retained_register_metadata_with_the_wrong_timestamp() {
    let operation = Operation::new(
        OperationId::new(ClientId::from_u128(1), 1),
        LamportTimestamp::new(1),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                ElementId::from_u128(106),
                Transform::new(Point::default(), Size::new(10.0, 10.0)),
            ),
        },
    );
    let mut document = CrdtDocument::new();
    document.apply(&operation).unwrap();

    let mut snapshot = document.snapshot();
    snapshot.clock = LamportTimestamp::new(2);
    snapshot
        .elements
        .first_mut()
        .expect("snapshot contains the created element")
        .position
        .metadata
        .timestamp = LamportTimestamp::new(2);

    assert!(matches!(
        CrdtDocument::from_snapshot(snapshot),
        Err(canvas_core::CrdtError::InvalidSnapshot(_))
    ));
}
