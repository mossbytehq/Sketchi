#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use canvas_core::{
    ClientId, CrdtDocument, Element, ElementId, LamportTimestamp, Operation, OperationId,
    OperationKind, Point, Size, Transform, VersionVector,
};

fn operation(client: u128, sequence: u64, timestamp: u64, kind: OperationKind) -> Operation {
    Operation::new(
        OperationId::new(ClientId::from_u128(client), sequence),
        LamportTimestamp::new(timestamp),
        VersionVector::default(),
        kind,
    )
}

fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut result = Vec::new();
    for index in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(index);
        for mut tail in permutations(&rest) {
            let mut permutation = vec![head.clone()];
            permutation.append(&mut tail);
            result.push(permutation);
        }
    }
    result
}

#[test]
fn every_delivery_order_produces_the_same_snapshot() {
    let element_id = ElementId::from_u128(42);
    let operations = vec![
        operation(
            1,
            1,
            1,
            OperationKind::Create {
                element: Element::rectangle(
                    element_id,
                    Transform::new(Point::new(0.0, 0.0), Size::new(20.0, 20.0)),
                ),
            },
        ),
        operation(
            2,
            1,
            4,
            OperationKind::SetPosition {
                element_id,
                position: Point::new(100.0, 80.0),
            },
        ),
        operation(
            3,
            1,
            4,
            OperationKind::SetSize {
                element_id,
                size: Size::new(300.0, 200.0),
            },
        ),
        operation(
            2,
            2,
            5,
            OperationKind::SetRotation {
                element_id,
                rotation: 0.5,
            },
        ),
        operation(4, 1, 6, OperationKind::Delete { element_id }),
        operation(
            5,
            1,
            100,
            OperationKind::SetPosition {
                element_id,
                position: Point::new(-50.0, -50.0),
            },
        ),
    ];

    let expected = {
        let mut document = CrdtDocument::new();
        for operation in &operations {
            document.apply(operation).unwrap();
        }
        document.snapshot()
    };

    for delivery in permutations(&operations) {
        let mut document = CrdtDocument::new();
        for operation in &delivery {
            document.apply(operation).unwrap();
        }
        assert_eq!(document.snapshot(), expected);
    }
}
