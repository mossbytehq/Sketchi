#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use canvas_client::storage::{
    Journal, StorageError, journal_path, load_document, load_document_from_path, open_journal,
    save_document,
};
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
fn journal_rejects_operation_id_reuse_with_different_content() {
    let journal = Journal::open_in_memory().unwrap();
    let first = operation_with_sequence(1);
    let mut conflicting = first.clone();
    conflicting.timestamp = LamportTimestamp::new(99);

    journal.append(&first).unwrap();

    assert!(matches!(
        journal.append(&conflicting),
        Err(StorageError::OperationIdReuse { operation_id }) if operation_id == first.id
    ));
    assert_eq!(journal.load().unwrap(), vec![first]);
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

#[test]
fn local_documents_round_trip_through_the_configured_directory() {
    let directory = std::env::temp_dir().join(format!(
        "sketchi-storage-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let directory_string = directory.to_string_lossy().into_owned();
    let element = Element::rectangle(
        ElementId::from_u128(30),
        Transform::new(Point::new(2.0, 3.0), Size::new(40.0, 50.0)),
    );
    let operation = Operation::new(
        OperationId::new(ClientId::from_u128(31), 1),
        LamportTimestamp::new(1),
        VersionVector::default(),
        OperationKind::Create {
            element: element.clone(),
        },
    );
    let mut replica = canvas_core::CrdtDocument::new();
    replica.apply(&operation).unwrap();
    let document = replica.document();

    save_document(&directory_string, &document).unwrap();
    assert_eq!(load_document(&directory_string).unwrap(), Some(document));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn document_files_load_from_an_explicit_path() {
    let directory = std::env::temp_dir().join(format!(
        "sketchi-storage-open-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("drawing.json");
    let element = Element::rectangle(
        ElementId::from_u128(32),
        Transform::new(Point::new(4.0, 5.0), Size::new(60.0, 70.0)),
    );
    let operation = Operation::new(
        OperationId::new(ClientId::from_u128(33), 1),
        LamportTimestamp::new(1),
        VersionVector::default(),
        OperationKind::Create { element },
    );
    let mut replica = canvas_core::CrdtDocument::new();
    replica.apply(&operation).unwrap();
    let document = replica.document();
    std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();

    assert_eq!(load_document_from_path(&path).unwrap(), document);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn invalid_document_json_returns_a_document_error() {
    let directory = std::env::temp_dir().join(format!(
        "sketchi-storage-invalid-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("invalid.json");
    std::fs::write(&path, b"not a Sketchi document").unwrap();

    assert!(matches!(
        load_document_from_path(&path),
        Err(StorageError::DocumentJson(_))
    ));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn durable_journal_creates_and_reopens_below_configured_directory() {
    let directory = std::env::temp_dir().join(format!(
        "sketchi-journal-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let directory_string = directory.to_string_lossy().into_owned();
    let operation = operation();
    let journal = open_journal(&directory_string).unwrap();
    journal.append(&operation).unwrap();
    drop(journal);

    assert!(journal_path(&directory_string).is_file());
    let reopened = Journal::open(journal_path(&directory_string)).unwrap();
    assert_eq!(reopened.load().unwrap(), vec![operation]);
    drop(reopened);
    std::fs::remove_dir_all(directory).unwrap();
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
