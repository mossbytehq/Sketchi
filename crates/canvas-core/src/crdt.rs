//! Operation-based CRDT implementation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    clock::{LamportClock, LamportTimestamp},
    document::Document,
    element::{
        Color, EdgeStyle, Element, ElementKind, EmbeddedImage, Sloppiness, StrokeStyle, Style,
        TextAlign, TextFontFamily,
    },
    error::CrdtError,
    geometry::{Point, Size, Transform},
    ids::{ElementId, OperationId},
    operation::{Operation, OperationKind},
    version_vector::VersionVector,
};

/// Maximum number of element tombstones and live states retained by a replica.
pub const MAX_ELEMENTS: usize = 100_000;

/// Ordering metadata attached to every LWW register.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct RegisterMetadata {
    /// Lamport timestamp of the write.
    pub timestamp: LamportTimestamp,
    /// Tie-breaker for equal Lamport timestamps.
    pub operation_id: OperationId,
}

impl RegisterMetadata {
    const ZERO: Self = Self {
        timestamp: LamportTimestamp::new(0),
        operation_id: OperationId::zero(),
    };

    fn from_operation(operation: &Operation) -> Self {
        Self {
            timestamp: operation.timestamp,
            operation_id: operation.id,
        }
    }
}

/// An LWW value and the metadata that selected it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Register<T> {
    /// Current register value.
    pub value: T,
    /// Metadata used for conflict resolution.
    pub metadata: RegisterMetadata,
}

impl<T> Register<T> {
    fn new(value: T) -> Self {
        Self {
            value,
            metadata: RegisterMetadata::ZERO,
        }
    }

    fn assign(&mut self, value: T, metadata: RegisterMetadata) -> bool {
        if metadata > self.metadata {
            self.value = value;
            self.metadata = metadata;
            true
        } else {
            false
        }
    }
}

impl<T: Default> Default for Register<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

/// Full per-element CRDT state retained in a snapshot.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ElementSnapshot {
    /// Stable element ID.
    pub id: ElementId,
    /// Creation marker; absent when only out-of-order updates have arrived.
    pub created: Option<RegisterMetadata>,
    /// Element kind register.
    pub kind: Register<ElementKind>,
    /// Position register.
    pub position: Register<Point>,
    /// Size register.
    pub size: Register<Size>,
    /// Rotation register.
    pub rotation: Register<f32>,
    /// Stroke color register.
    pub stroke: Register<Color>,
    /// Fill color register.
    pub fill: Register<Option<Color>>,
    /// Stroke width register.
    pub stroke_width: Register<f32>,
    /// Stroke rendering pattern register.
    #[serde(default)]
    pub stroke_style: Register<StrokeStyle>,
    /// Hand-drawn variation register.
    #[serde(default)]
    pub sloppiness: Register<Sloppiness>,
    /// Corner treatment register.
    #[serde(default)]
    pub edges: Register<EdgeStyle>,
    /// Overall opacity register.
    #[serde(default)]
    pub opacity: Register<f32>,
    /// Text font family register.
    #[serde(default)]
    pub font_family: Register<TextFontFamily>,
    /// Text font size register.
    #[serde(default = "default_font_size_register")]
    pub font_size: Register<f32>,
    /// Text alignment register.
    #[serde(default)]
    pub text_align: Register<TextAlign>,
    /// Text register.
    pub text: Register<String>,
    /// Point sequence register.
    pub points: Register<Vec<Point>>,
    /// Embedded image register.
    #[serde(default)]
    pub image: Register<Option<EmbeddedImage>>,
    /// Stacking order register.
    pub z_index: Register<i64>,
    /// Permanent deletion marker.
    pub deleted: Option<RegisterMetadata>,
}

fn default_font_size_register() -> Register<f32> {
    Register::new(Style::default().font_size)
}

impl ElementSnapshot {
    fn new(id: ElementId) -> Self {
        Self {
            id,
            created: None,
            kind: Register::new(ElementKind::default()),
            position: Register::new(Point::default()),
            size: Register::new(Size::default()),
            rotation: Register::new(0.0),
            stroke: Register::new(Style::default().stroke),
            fill: Register::new(Style::default().fill),
            stroke_width: Register::new(Style::default().stroke_width),
            stroke_style: Register::new(Style::default().stroke_style),
            sloppiness: Register::new(Style::default().sloppiness),
            edges: Register::new(Style::default().edges),
            opacity: Register::new(Style::default().opacity),
            font_family: Register::new(Style::default().font_family),
            font_size: Register::new(Style::default().font_size),
            text_align: Register::new(Style::default().text_align),
            text: Register::new(String::new()),
            points: Register::new(Vec::new()),
            image: Register::new(None),
            z_index: Register::new(0),
            deleted: None,
        }
    }

    fn validate(&self) -> Result<(), CrdtError> {
        if self.id.is_nil() {
            return Err(CrdtError::InvalidSnapshot(
                "element id cannot be nil".to_owned(),
            ));
        }
        self.position.value.validate()?;
        self.size.value.validate()?;
        if !self.rotation.value.is_finite() {
            return Err(CrdtError::InvalidSnapshot(
                "rotation must be finite".to_owned(),
            ));
        }
        Style {
            stroke: self.stroke.value,
            fill: self.fill.value,
            stroke_width: self.stroke_width.value,
            stroke_style: self.stroke_style.value,
            sloppiness: self.sloppiness.value,
            edges: self.edges.value,
            opacity: self.opacity.value,
            font_family: self.font_family.value,
            font_size: self.font_size.value,
            text_align: self.text_align.value,
        }
        .validate()?;
        if self.text.value.len() > crate::MAX_TEXT_BYTES {
            return Err(CrdtError::TextTooLong);
        }
        if self.points.value.len() > crate::MAX_POINTS {
            return Err(CrdtError::TooManyPoints);
        }
        for point in &self.points.value {
            point.validate()?;
        }
        match (self.kind.value, self.image.value.as_ref()) {
            (ElementKind::Image, Some(image)) => image
                .validate()
                .map_err(|error| CrdtError::InvalidSnapshot(error.to_string()))?,
            (ElementKind::Image, None) => {
                return Err(CrdtError::InvalidSnapshot(
                    "image element is missing its embedded payload".to_owned(),
                ));
            }
            (_, Some(_)) => {
                return Err(CrdtError::InvalidSnapshot(
                    "only image elements may contain an embedded payload".to_owned(),
                ));
            }
            (_, None) => {}
        }
        Ok(())
    }

    fn apply_create(&mut self, element: &Element, metadata: RegisterMetadata) {
        if self.created.is_none_or(|current| metadata > current) {
            self.created = Some(metadata);
        }
        self.kind.assign(element.kind, metadata);
        self.position.assign(element.transform.position, metadata);
        self.size.assign(element.transform.size, metadata);
        self.rotation.assign(element.transform.rotation, metadata);
        self.stroke.assign(element.style.stroke, metadata);
        self.fill.assign(element.style.fill, metadata);
        self.stroke_width
            .assign(element.style.stroke_width, metadata);
        self.stroke_style
            .assign(element.style.stroke_style, metadata);
        self.sloppiness.assign(element.style.sloppiness, metadata);
        self.edges.assign(element.style.edges, metadata);
        self.opacity.assign(element.style.opacity, metadata);
        self.font_family.assign(element.style.font_family, metadata);
        self.font_size.assign(element.style.font_size, metadata);
        self.text_align.assign(element.style.text_align, metadata);
        self.text.assign(element.text.clone(), metadata);
        self.points.assign(element.points.clone(), metadata);
        self.image.assign(element.image.clone(), metadata);
        self.z_index.assign(element.z_index, metadata);
    }

    fn materialize(&self) -> Option<Element> {
        if self.created.is_some() && self.deleted.is_none() {
            Some(Element {
                id: self.id,
                kind: self.kind.value,
                transform: Transform {
                    position: self.position.value,
                    size: self.size.value,
                    rotation: self.rotation.value,
                },
                style: Style {
                    stroke: self.stroke.value,
                    fill: self.fill.value,
                    stroke_width: self.stroke_width.value,
                    stroke_style: self.stroke_style.value,
                    sloppiness: self.sloppiness.value,
                    edges: self.edges.value,
                    opacity: self.opacity.value,
                    font_family: self.font_family.value,
                    font_size: self.font_size.value,
                    text_align: self.text_align.value,
                },
                text: self.text.value.clone(),
                points: self.points.value.clone(),
                image: self.image.value.clone(),
                z_index: self.z_index.value,
            })
        } else {
            None
        }
    }
}

/// Serializable complete CRDT state.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CrdtSnapshot {
    /// Per-element registers, including invisible tombstones, sorted by ID.
    pub elements: Vec<ElementSnapshot>,
    /// Seen operation content used for idempotence and ID-reuse detection, sorted by ID.
    pub seen_operations: Vec<Operation>,
    /// Causal knowledge of this replica.
    pub version_vector: VersionVector,
    /// Current Lamport clock state.
    pub clock: LamportTimestamp,
}

/// Result of accepting a valid operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyResult {
    /// The operation was newly accepted and incorporated.
    Applied,
    /// The exact operation had already been accepted.
    Duplicate,
}

/// Operation-based document replica.
#[derive(Clone, Debug, Default)]
pub struct CrdtDocument {
    elements: BTreeMap<ElementId, ElementSnapshot>,
    seen_operations: BTreeMap<OperationId, Operation>,
    version_vector: VersionVector,
    clock: LamportClock,
}

impl CrdtDocument {
    /// Creates an empty replica.
    #[must_use]
    pub fn new() -> Self {
        Self {
            elements: BTreeMap::new(),
            seen_operations: BTreeMap::new(),
            version_vector: VersionVector::default(),
            clock: LamportClock::new(),
        }
    }

    /// Applies one operation through the CRDT mutation path.
    ///
    /// # Errors
    ///
    /// Returns a [`CrdtError`] when the operation is invalid, reuses an ID with
    /// different content, or exceeds the element bound.
    pub fn apply(&mut self, operation: &Operation) -> Result<ApplyResult, CrdtError> {
        operation.validate()?;
        if let Some(previous) = self.seen_operations.get(&operation.id) {
            return if previous == operation {
                Ok(ApplyResult::Duplicate)
            } else {
                Err(CrdtError::OperationIdReuse(operation.id.to_string()))
            };
        }
        if matches!(operation.kind, OperationKind::Create { .. })
            && !self.elements.contains_key(&operation.target_element_id())
            && self.elements.len() >= MAX_ELEMENTS
        {
            return Err(CrdtError::TooManyElements);
        }

        let metadata = RegisterMetadata::from_operation(operation);
        self.apply_kind(&operation.kind, metadata);
        self.seen_operations.insert(operation.id, operation.clone());
        self.version_vector.observe(operation.id);
        self.version_vector.merge(&operation.deps);
        self.clock.observe(operation.timestamp);
        Ok(ApplyResult::Applied)
    }

    #[allow(clippy::too_many_lines)]
    fn apply_kind(&mut self, kind: &OperationKind, metadata: RegisterMetadata) {
        match kind {
            OperationKind::Create { element } => {
                self.elements
                    .entry(element.id)
                    .or_insert_with(|| ElementSnapshot::new(element.id))
                    .apply_create(element, metadata);
            }
            OperationKind::Delete { element_id } => {
                let state = self
                    .elements
                    .entry(*element_id)
                    .or_insert_with(|| ElementSnapshot::new(*element_id));
                if state.deleted.is_none_or(|current| metadata > current) {
                    state.deleted = Some(metadata);
                }
            }
            OperationKind::SetPosition {
                element_id,
                position,
            } => {
                self.elements
                    .entry(*element_id)
                    .or_insert_with(|| ElementSnapshot::new(*element_id))
                    .position
                    .assign(*position, metadata);
            }
            OperationKind::SetSize { element_id, size } => {
                self.elements
                    .entry(*element_id)
                    .or_insert_with(|| ElementSnapshot::new(*element_id))
                    .size
                    .assign(*size, metadata);
            }
            OperationKind::SetRotation {
                element_id,
                rotation,
            } => {
                self.elements
                    .entry(*element_id)
                    .or_insert_with(|| ElementSnapshot::new(*element_id))
                    .rotation
                    .assign(*rotation, metadata);
            }
            OperationKind::SetStyle { element_id, style } => {
                let state = self
                    .elements
                    .entry(*element_id)
                    .or_insert_with(|| ElementSnapshot::new(*element_id));
                if let Some(stroke) = style.stroke {
                    state.stroke.assign(stroke, metadata);
                }
                if let Some(fill) = style.fill {
                    state.fill.assign(fill, metadata);
                }
                if let Some(stroke_width) = style.stroke_width {
                    state.stroke_width.assign(stroke_width, metadata);
                }
                if let Some(stroke_style) = style.stroke_style {
                    state.stroke_style.assign(stroke_style, metadata);
                }
                if let Some(sloppiness) = style.sloppiness {
                    state.sloppiness.assign(sloppiness, metadata);
                }
                if let Some(edges) = style.edges {
                    state.edges.assign(edges, metadata);
                }
                if let Some(opacity) = style.opacity {
                    state.opacity.assign(opacity, metadata);
                }
                if let Some(font_family) = style.font_family {
                    state.font_family.assign(font_family, metadata);
                }
                if let Some(font_size) = style.font_size {
                    state.font_size.assign(font_size, metadata);
                }
                if let Some(text_align) = style.text_align {
                    state.text_align.assign(text_align, metadata);
                }
            }
            OperationKind::SetText { element_id, text } => {
                self.elements
                    .entry(*element_id)
                    .or_insert_with(|| ElementSnapshot::new(*element_id))
                    .text
                    .assign(text.clone(), metadata);
            }
            OperationKind::SetImage { element_id, image } => {
                self.elements
                    .entry(*element_id)
                    .or_insert_with(|| ElementSnapshot::new(*element_id))
                    .image
                    .assign(Some(image.clone()), metadata);
            }
            OperationKind::SetPoints { element_id, points } => {
                self.elements
                    .entry(*element_id)
                    .or_insert_with(|| ElementSnapshot::new(*element_id))
                    .points
                    .assign(points.clone(), metadata);
            }
            OperationKind::Reorder {
                element_id,
                z_index,
            } => {
                self.elements
                    .entry(*element_id)
                    .or_insert_with(|| ElementSnapshot::new(*element_id))
                    .z_index
                    .assign(*z_index, metadata);
            }
        }
    }

    /// Returns the visible materialized document.
    #[must_use]
    pub fn document(&self) -> Document {
        let elements = self
            .elements
            .values()
            .filter_map(|state| state.materialize().map(|element| (element.id, element)))
            .collect();
        Document::from_elements(elements)
    }

    /// Returns whether an element has a permanent tombstone.
    #[must_use]
    pub fn is_tombstoned(&self, element_id: ElementId) -> bool {
        self.elements
            .get(&element_id)
            .is_some_and(|state| state.deleted.is_some())
    }

    /// Returns the current causal knowledge.
    #[must_use]
    pub const fn version_vector(&self) -> &VersionVector {
        &self.version_vector
    }

    /// Advances the replica clock for a new local operation.
    pub fn tick(&mut self) -> LamportTimestamp {
        self.clock.tick()
    }

    /// Returns the current Lamport timestamp.
    #[must_use]
    pub const fn clock(&self) -> LamportTimestamp {
        self.clock.current()
    }

    /// Creates a canonical serializable snapshot.
    #[must_use]
    pub fn snapshot(&self) -> CrdtSnapshot {
        CrdtSnapshot {
            elements: self.elements.values().cloned().collect(),
            seen_operations: self.seen_operations.values().cloned().collect(),
            version_vector: self.version_vector.clone(),
            clock: self.clock.current(),
        }
    }

    /// Restores a replica from a validated snapshot.
    ///
    /// # Errors
    ///
    /// Returns a [`CrdtError`] when snapshot registers, IDs, operations, or
    /// causal metadata are inconsistent.
    pub fn from_snapshot(snapshot: CrdtSnapshot) -> Result<Self, CrdtError> {
        let CrdtSnapshot {
            elements: snapshot_elements,
            seen_operations: snapshot_operations,
            version_vector,
            clock,
        } = snapshot;
        version_vector.validate()?;
        if snapshot_elements.len() > MAX_ELEMENTS {
            return Err(CrdtError::TooManyElements);
        }
        let mut elements = BTreeMap::new();
        for state in snapshot_elements {
            state.validate()?;
            if elements.insert(state.id, state).is_some() {
                return Err(CrdtError::InvalidSnapshot(
                    "snapshot contains duplicate element IDs".to_owned(),
                ));
            }
        }
        let mut seen_operations = BTreeMap::new();
        for operation in snapshot_operations {
            operation.validate()?;
            if seen_operations.insert(operation.id, operation).is_some() {
                return Err(CrdtError::InvalidSnapshot(
                    "snapshot contains duplicate operation IDs".to_owned(),
                ));
            }
        }
        Ok(Self {
            elements,
            seen_operations,
            version_vector,
            clock: LamportClock::from_timestamp(clock),
        })
    }
}

trait TargetElementId {
    fn target_element_id(&self) -> ElementId;
}

impl TargetElementId for Operation {
    fn target_element_id(&self) -> ElementId {
        match &self.kind {
            OperationKind::Create { element } => element.id,
            OperationKind::Delete { element_id }
            | OperationKind::SetPosition { element_id, .. }
            | OperationKind::SetSize { element_id, .. }
            | OperationKind::SetRotation { element_id, .. }
            | OperationKind::SetStyle { element_id, .. }
            | OperationKind::SetText { element_id, .. }
            | OperationKind::SetImage { element_id, .. }
            | OperationKind::SetPoints { element_id, .. }
            | OperationKind::Reorder { element_id, .. } => *element_id,
        }
    }
}
