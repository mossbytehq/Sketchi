#![allow(
    clippy::expect_used,
    clippy::float_cmp,
    clippy::unwrap_used,
    missing_docs
)]

use canvas_core::{
    CrdtDocument, Element, ElementId, EmbeddedImage, LamportTimestamp, Operation, OperationId,
    OperationKind, Point, Size, Transform, VersionVector,
};
use canvas_renderer::{RenderPrimitive, Renderer, hit_test};

fn document_with_rectangle() -> (CrdtDocument, ElementId) {
    let element_id = ElementId::from_u128(1);
    let operation = Operation::new(
        OperationId::new(canvas_core::ClientId::from_u128(1), 1),
        LamportTimestamp::new(1),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::rectangle(
                element_id,
                Transform::new(Point::new(10.0, 20.0), Size::new(100.0, 50.0)),
            ),
        },
    );
    let mut document = CrdtDocument::new();
    document.apply(&operation).unwrap();
    (document, element_id)
}

#[test]
fn hit_testing_returns_the_topmost_element() {
    let (document, element_id) = document_with_rectangle();
    let materialized = document.document();
    assert_eq!(
        hit_test(&materialized, Point::new(50.0, 40.0), 0.0),
        Some(element_id)
    );
    assert_eq!(hit_test(&materialized, Point::new(500.0, 500.0), 0.0), None);
}

#[test]
fn transparent_outer_shape_does_not_hide_an_inner_shape() {
    let inner_id = ElementId::from_u128(1);
    let outer_id = ElementId::from_u128(2);
    let mut document = CrdtDocument::new();
    for (counter, element) in [
        (
            1,
            Element::rectangle(
                inner_id,
                Transform::new(Point::new(40.0, 40.0), Size::new(30.0, 30.0)),
            ),
        ),
        (
            2,
            Element::rectangle(
                outer_id,
                Transform::new(Point::new(10.0, 10.0), Size::new(100.0, 100.0)),
            ),
        ),
    ] {
        document
            .apply(&Operation::new(
                OperationId::new(canvas_core::ClientId::from_u128(1), counter),
                LamportTimestamp::new(counter),
                VersionVector::default(),
                OperationKind::Create { element },
            ))
            .unwrap();
    }

    assert_eq!(
        hit_test(&document.document(), Point::new(55.0, 55.0), 0.0),
        Some(inner_id)
    );
    assert_eq!(
        hit_test(&document.document(), Point::new(20.0, 20.0), 0.0),
        Some(outer_id)
    );
}

#[test]
fn renderer_extracts_document_primitives_without_crdt_or_transport_state() {
    let (document, element_id) = document_with_rectangle();
    let renderer = Renderer::new();
    let scene = renderer.draw(&document.document());
    assert_eq!(scene.len(), 1);
    assert!(
        matches!(scene.primitives().next(), Some(RenderPrimitive::Rectangle { id, .. }) if *id == element_id)
    );
}

#[test]
fn renderer_preserves_text_rotation_in_the_scene() {
    let element_id = ElementId::from_u128(18);
    let mut element = Element::text(
        element_id,
        Transform::new(Point::new(10.0, 20.0), Size::new(100.0, 30.0)),
        "rotated",
    );
    element.transform.rotation = std::f32::consts::FRAC_PI_2;
    let mut document = CrdtDocument::new();
    document
        .apply(&Operation::new(
            OperationId::new(canvas_core::ClientId::from_u128(1), 1),
            LamportTimestamp::new(1),
            VersionVector::default(),
            OperationKind::Create { element },
        ))
        .unwrap();

    let renderer = Renderer::new();
    let scene = renderer.draw(&document.document());
    assert!(matches!(
        scene.primitives().next(),
        Some(RenderPrimitive::Text { rotation, .. })
            if (*rotation - std::f32::consts::FRAC_PI_2).abs() < f32::EPSILON
    ));
}

#[test]
fn renderer_extracts_and_hits_embedded_images() {
    let element_id = ElementId::from_u128(16);
    let operation = Operation::new(
        OperationId::new(canvas_core::ClientId::from_u128(1), 1),
        LamportTimestamp::new(1),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::image(
                element_id,
                Transform::new(Point::new(10.0, 20.0), Size::new(120.0, 80.0)),
                EmbeddedImage::new("image/png", 2, 2, vec![1, 2, 3, 4]),
            ),
        },
    );
    let mut document = CrdtDocument::new();
    document.apply(&operation).unwrap();

    let renderer = Renderer::new();
    let scene = renderer.draw(&document.document());
    assert!(matches!(
        scene.primitives().next(),
        Some(RenderPrimitive::Image { id, .. }) if *id == element_id
    ));
    assert_eq!(
        hit_test(&document.document(), Point::new(60.0, 50.0), 0.0),
        Some(element_id)
    );
}

#[test]
fn renderer_extracts_and_hits_a_diamond() {
    let element_id = ElementId::from_u128(19);
    let operation = Operation::new(
        OperationId::new(canvas_core::ClientId::from_u128(1), 1),
        LamportTimestamp::new(1),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::diamond(
                element_id,
                Transform::new(Point::new(10.0, 20.0), Size::new(100.0, 80.0)),
            ),
        },
    );
    let mut document = CrdtDocument::new();
    document.apply(&operation).unwrap();

    let renderer = Renderer::new();
    let scene = renderer.draw(&document.document());
    assert!(matches!(
        scene.primitives().next(),
        Some(RenderPrimitive::Diamond { id, .. }) if *id == element_id
    ));
    assert_eq!(
        hit_test(&document.document(), Point::new(60.0, 60.0), 0.0),
        Some(element_id)
    );
}

#[test]
fn renderer_extracts_and_hits_a_triangle() {
    let element_id = ElementId::from_u128(22);
    let operation = Operation::new(
        OperationId::new(canvas_core::ClientId::from_u128(1), 1),
        LamportTimestamp::new(1),
        VersionVector::default(),
        OperationKind::Create {
            element: Element::triangle(
                element_id,
                Transform::new(Point::new(10.0, 20.0), Size::new(100.0, 80.0)),
            ),
        },
    );
    let mut document = CrdtDocument::new();
    document.apply(&operation).unwrap();

    let renderer = Renderer::new();
    let scene = renderer.draw(&document.document());
    assert!(matches!(
        scene.primitives().next(),
        Some(RenderPrimitive::Triangle { id, .. }) if *id == element_id
    ));
    assert_eq!(
        hit_test(&document.document(), Point::new(60.0, 60.0), 0.0),
        Some(element_id)
    );
    assert_eq!(
        hit_test(&document.document(), Point::new(20.0, 60.0), 0.0),
        None
    );
}

#[test]
fn rotated_diamond_hit_testing_uses_the_element_rotation() {
    let element_id = ElementId::from_u128(20);
    let mut element = Element::diamond(
        element_id,
        Transform::new(Point::new(10.0, 20.0), Size::new(100.0, 40.0)),
    );
    element.transform.rotation = std::f32::consts::FRAC_PI_2;
    let operation = Operation::new(
        OperationId::new(canvas_core::ClientId::from_u128(1), 1),
        LamportTimestamp::new(1),
        VersionVector::default(),
        OperationKind::Create { element },
    );
    let mut document = CrdtDocument::new();
    document.apply(&operation).unwrap();

    assert_eq!(
        hit_test(&document.document(), Point::new(60.0, 80.0), 0.0),
        Some(element_id)
    );
}
