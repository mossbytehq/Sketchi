#![allow(clippy::expect_used, clippy::unwrap_used, missing_docs)]

use canvas_core::{
    ApplyResult, ClientId, Color, CrdtDocument, CrdtError, EdgeStyle, Element, ElementId,
    ElementKind, EmbeddedImage, LamportTimestamp, MAX_IMAGE_BYTES, MAX_POINTS, MAX_TEXT_BYTES,
    Operation, OperationId, OperationKind, Point, Size, Sloppiness, StrokeStyle, StylePatch,
    Transform, VersionVector,
};

fn operation(client: u128, sequence: u64, timestamp: u64, kind: OperationKind) -> Operation {
    Operation::new(
        OperationId::new(ClientId::from_u128(client), sequence),
        LamportTimestamp::new(timestamp),
        VersionVector::default(),
        kind,
    )
}

fn rectangle(id: ElementId) -> Element {
    Element::rectangle(
        id,
        Transform::new(Point::new(0.0, 0.0), Size::new(100.0, 50.0)),
    )
}

fn embedded_image() -> EmbeddedImage {
    EmbeddedImage::new("image/png", 2, 3, vec![1, 2, 3])
}

#[test]
fn embedded_image_survives_crdt_delivery_and_snapshot_round_trip() {
    let element_id = ElementId::from_u128(14);
    let create = operation(
        1,
        1,
        1,
        OperationKind::Create {
            element: Element::image(
                element_id,
                Transform::new(Point::new(10.0, 20.0), Size::new(200.0, 300.0)),
                embedded_image(),
            ),
        },
    );

    let mut first = CrdtDocument::new();
    assert_eq!(first.apply(&create).unwrap(), ApplyResult::Applied);
    assert_eq!(
        first.document().element(element_id).unwrap().image,
        Some(embedded_image())
    );

    let snapshot = first.snapshot();
    let restored = CrdtDocument::from_snapshot(snapshot).unwrap();
    assert_eq!(restored.document(), first.document());

    let mut out_of_order = CrdtDocument::new();
    out_of_order.apply(&create).unwrap();
    assert_eq!(
        out_of_order.document().element(element_id).unwrap().image,
        Some(embedded_image())
    );
}

#[test]
fn image_payload_updates_are_durable_and_merge_after_creation() {
    let element_id = ElementId::from_u128(16);
    let create = operation(
        1,
        1,
        1,
        OperationKind::Create {
            element: Element::image(
                element_id,
                Transform::new(Point::default(), Size::new(20.0, 20.0)),
                embedded_image(),
            ),
        },
    );
    let replacement = EmbeddedImage::new("image/png", 1, 1, vec![9, 8, 7]);
    let update = operation(
        1,
        2,
        2,
        OperationKind::SetImage {
            element_id,
            image: replacement.clone(),
        },
    );

    let mut document = CrdtDocument::new();
    document.apply(&create).unwrap();
    document.apply(&update).unwrap();

    assert_eq!(
        document.document().element(element_id).unwrap().image,
        Some(replacement)
    );
    assert_eq!(
        CrdtDocument::from_snapshot(document.snapshot())
            .unwrap()
            .document(),
        document.document()
    );
}

#[test]
fn oversized_embedded_images_are_rejected_before_crdt_delivery() {
    let element = Element::image(
        ElementId::from_u128(15),
        Transform::new(Point::default(), Size::new(2.0, 2.0)),
        EmbeddedImage::new("image/png", 2, 2, vec![0; MAX_IMAGE_BYTES + 1]),
    );
    let create = operation(1, 1, 1, OperationKind::Create { element });

    assert!(matches!(
        create.validate(),
        Err(CrdtError::InvalidOperation(_))
    ));
}

#[test]
fn oversized_decoded_image_dimensions_are_rejected_before_crdt_delivery() {
    let element = Element::image(
        ElementId::from_u128(17),
        Transform::new(Point::default(), Size::new(2.0, 2.0)),
        EmbeddedImage::new("image/png", 4_097, 4_097, vec![0]),
    );
    let create = operation(1, 1, 1, OperationKind::Create { element });

    assert!(matches!(
        create.validate(),
        Err(CrdtError::InvalidOperation(_))
    ));
}

#[test]
fn diamond_create_survives_crdt_and_snapshot_round_trip() {
    let element_id = ElementId::from_u128(18);
    let create = operation(
        1,
        1,
        1,
        OperationKind::Create {
            element: Element::diamond(
                element_id,
                Transform::new(Point::new(10.0, 20.0), Size::new(80.0, 60.0)),
            ),
        },
    );

    let mut document = CrdtDocument::new();
    document.apply(&create).unwrap();
    assert_eq!(
        document.document().element(element_id).unwrap().kind,
        ElementKind::Diamond
    );

    let restored = CrdtDocument::from_snapshot(document.snapshot()).unwrap();
    assert_eq!(restored.document(), document.document());
}

#[test]
fn triangle_create_survives_crdt_and_snapshot_round_trip() {
    let element_id = ElementId::from_u128(21);
    let create = operation(
        1,
        1,
        1,
        OperationKind::Create {
            element: Element::triangle(
                element_id,
                Transform::new(Point::new(10.0, 20.0), Size::new(80.0, 60.0)),
            ),
        },
    );

    let mut document = CrdtDocument::new();
    document.apply(&create).unwrap();
    assert_eq!(
        document.document().element(element_id).unwrap().kind,
        ElementKind::Triangle
    );

    let restored = CrdtDocument::from_snapshot(document.snapshot()).unwrap();
    assert_eq!(restored.document(), document.document());
}

#[test]
fn sequential_operations_and_duplicates_are_safe() {
    let element_id = ElementId::from_u128(7);
    let create = operation(
        1,
        1,
        1,
        OperationKind::Create {
            element: rectangle(element_id),
        },
    );
    let move_operation = operation(
        1,
        2,
        2,
        OperationKind::SetPosition {
            element_id,
            position: Point::new(20.0, 30.0),
        },
    );

    let mut document = CrdtDocument::new();
    assert_eq!(document.apply(&create).unwrap(), ApplyResult::Applied);
    assert_eq!(
        document.apply(&move_operation).unwrap(),
        ApplyResult::Applied
    );
    assert_eq!(
        document.apply(&move_operation).unwrap(),
        ApplyResult::Duplicate
    );

    let materialized = document.document();
    let element = materialized.element(element_id).unwrap();
    assert_eq!(element.transform.position, Point::new(20.0, 30.0));
    assert_eq!(document.document().len(), 1);
}

#[test]
fn out_of_order_updates_merge_with_creation() {
    let element_id = ElementId::from_u128(8);
    let create = operation(
        1,
        1,
        1,
        OperationKind::Create {
            element: rectangle(element_id),
        },
    );
    let resize = operation(
        2,
        1,
        3,
        OperationKind::SetSize {
            element_id,
            size: Size::new(200.0, 80.0),
        },
    );

    let mut document = CrdtDocument::new();
    document.apply(&resize).unwrap();
    document.apply(&create).unwrap();

    let materialized = document.document();
    let element = materialized.element(element_id).unwrap();
    assert_eq!(element.transform.size, Size::new(200.0, 80.0));
}

#[test]
fn concurrent_properties_merge_and_same_property_order_is_deterministic() {
    let element_id = ElementId::from_u128(9);
    let create = operation(
        1,
        1,
        1,
        OperationKind::Create {
            element: rectangle(element_id),
        },
    );
    let move_operation = operation(
        1,
        2,
        5,
        OperationKind::SetPosition {
            element_id,
            position: Point::new(10.0, 11.0),
        },
    );
    let resize = operation(
        2,
        1,
        5,
        OperationKind::SetSize {
            element_id,
            size: Size::new(400.0, 300.0),
        },
    );
    let tie = operation(
        3,
        1,
        5,
        OperationKind::SetPosition {
            element_id,
            position: Point::new(90.0, 91.0),
        },
    );

    let mut first = CrdtDocument::new();
    for current in [&create, &move_operation, &resize, &tie] {
        first.apply(current).unwrap();
    }

    let mut second = CrdtDocument::new();
    for current in [&tie, &resize, &move_operation, &create] {
        second.apply(current).unwrap();
    }

    assert_eq!(first.document(), second.document());
    let materialized = first.document();
    let element = materialized.element(element_id).unwrap();
    assert_eq!(element.transform.position, Point::new(90.0, 91.0));
    assert_eq!(element.transform.size, Size::new(400.0, 300.0));
}

#[test]
fn delete_wins_even_when_it_arrives_before_create_or_updates() {
    let element_id = ElementId::from_u128(10);
    let delete = operation(2, 1, 2, OperationKind::Delete { element_id });
    let create = operation(
        1,
        1,
        1,
        OperationKind::Create {
            element: rectangle(element_id),
        },
    );
    let update = operation(
        3,
        1,
        20,
        OperationKind::SetPosition {
            element_id,
            position: Point::new(999.0, 999.0),
        },
    );

    let mut document = CrdtDocument::new();
    document.apply(&delete).unwrap();
    document.apply(&update).unwrap();
    document.apply(&create).unwrap();

    assert!(document.document().element(element_id).is_none());
    assert!(document.is_tombstoned(element_id));
}

#[test]
fn reusing_an_operation_id_with_different_content_is_rejected() {
    let element_id = ElementId::from_u128(11);
    let first = operation(
        1,
        1,
        1,
        OperationKind::Create {
            element: rectangle(element_id),
        },
    );
    let reused = operation(1, 1, 2, OperationKind::Delete { element_id });

    let mut document = CrdtDocument::new();
    document.apply(&first).unwrap();
    let error = document.apply(&reused).unwrap_err();
    assert!(error.to_string().contains("operation id"));
}

#[test]
fn style_fields_merge_independently() {
    let element_id = ElementId::from_u128(12);
    let create = operation(
        1,
        1,
        1,
        OperationKind::Create {
            element: rectangle(element_id),
        },
    );
    let stroke = operation(
        1,
        2,
        6,
        OperationKind::SetStyle {
            element_id,
            style: StylePatch {
                stroke: Some(Color::rgb(200, 0, 0)),
                ..StylePatch::default()
            },
        },
    );
    let fill = operation(
        2,
        1,
        5,
        OperationKind::SetStyle {
            element_id,
            style: StylePatch {
                fill: Some(Some(Color::rgb(0, 200, 0))),
                ..StylePatch::default()
            },
        },
    );

    let mut document = CrdtDocument::new();
    for current in [&create, &stroke, &fill] {
        document.apply(current).unwrap();
    }
    let materialized = document.document();
    let element = materialized.element(element_id).unwrap();
    assert_eq!(element.style.stroke, Color::rgb(200, 0, 0));
    assert_eq!(element.style.fill, Some(Color::rgb(0, 200, 0)));
}

#[test]
fn extended_style_fields_merge_and_materialize() {
    let element_id = ElementId::from_u128(120);
    let create = operation(
        1,
        1,
        1,
        OperationKind::Create {
            element: rectangle(element_id),
        },
    );
    let style = operation(
        1,
        2,
        2,
        OperationKind::SetStyle {
            element_id,
            style: StylePatch {
                stroke_style: Some(StrokeStyle::Dashed),
                sloppiness: Some(Sloppiness::Cartoonist),
                edges: Some(EdgeStyle::Rounded),
                opacity: Some(0.42),
                ..StylePatch::default()
            },
        },
    );

    let mut document = CrdtDocument::new();
    document.apply(&create).unwrap();
    document.apply(&style).unwrap();

    let materialized = document.document();
    let element = materialized.element(element_id).unwrap();
    assert_eq!(element.style.stroke_style, StrokeStyle::Dashed);
    assert_eq!(element.style.sloppiness, Sloppiness::Cartoonist);
    assert_eq!(element.style.edges, EdgeStyle::Rounded);
    assert!((element.style.opacity - 0.42).abs() < f32::EPSILON);
}

#[test]
fn opacity_must_be_a_finite_percentage() {
    let element_id = ElementId::from_u128(121);
    for opacity in [f32::NAN, -0.01, 1.01, f32::INFINITY] {
        let operation = operation(
            1,
            1,
            1,
            OperationKind::SetStyle {
                element_id,
                style: StylePatch {
                    opacity: Some(opacity),
                    ..StylePatch::default()
                },
            },
        );
        assert!(matches!(
            operation.validate(),
            Err(CrdtError::InvalidGeometry(_))
        ));
    }
}

#[test]
fn invalid_geometry_and_bounded_payloads_are_rejected() {
    let element_id = ElementId::from_u128(13);
    let invalid_position = operation(
        1,
        1,
        1,
        OperationKind::SetPosition {
            element_id,
            position: Point::new(f32::NAN, 0.0),
        },
    );
    let oversized_text = operation(
        1,
        2,
        2,
        OperationKind::SetText {
            element_id,
            text: "x".repeat(MAX_TEXT_BYTES + 1),
        },
    );
    let oversized_points = operation(
        1,
        3,
        3,
        OperationKind::SetPoints {
            element_id,
            points: vec![Point::default(); MAX_POINTS + 1],
        },
    );

    assert!(matches!(
        invalid_position.validate(),
        Err(CrdtError::InvalidGeometry(_))
    ));
    assert_eq!(oversized_text.validate(), Err(CrdtError::TextTooLong));
    assert_eq!(oversized_points.validate(), Err(CrdtError::TooManyPoints));
}
