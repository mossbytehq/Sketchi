#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use canvas_client::storage::Journal;
use canvas_core::{
    ClientId, Element, ElementId, LamportTimestamp, Operation, OperationId, OperationKind, Point,
    Size, Transform, VersionVector,
};

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
fn journal_is_idempotent_and_removes_acknowledged_operations() {
    let journal = Journal::open_in_memory().unwrap();
    let operation = operation();
    journal.append(&operation).unwrap();
    journal.append(&operation).unwrap();
    assert_eq!(journal.load().unwrap(), vec![operation.clone()]);
    journal.remove(&[operation.id]).unwrap();
    assert!(journal.load().unwrap().is_empty());
}

#[test]
fn journal_loads_deterministic_bounded_batches() {
    let journal = Journal::open_in_memory().unwrap();
    let first = operation_with_sequence(1);
    let second = operation_with_sequence(2);
    let third = operation_with_sequence(3);
    let tenth = operation_with_sequence(10);

    journal
        .append_all(&[tenth.clone(), third.clone(), first.clone(), second.clone()])
        .unwrap();

    assert_eq!(journal.pending_count().unwrap(), 4);
    assert_eq!(journal.load_batch(2).unwrap(), vec![first, second]);
    assert_eq!(
        journal.load_batch(4).unwrap(),
        vec![
            operation_with_sequence(1),
            operation_with_sequence(2),
            operation_with_sequence(3),
            tenth,
        ]
    );
}

fn operation_with_sequence(sequence: u64) -> Operation {
    Operation::new(
        OperationId::new(ClientId::from_u128(1), sequence),
        LamportTimestamp::new(sequence),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                ElementId::from_u128(u128::from(sequence) + 10),
                Transform::new(Point::default(), Size::new(10.0, 10.0)),
            ),
        },
    )
}
