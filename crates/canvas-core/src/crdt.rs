//! Operation-based CRDT implementation.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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

fn validate_snapshot_metadata(
    metadata: RegisterMetadata,
    clock: LamportTimestamp,
    version_vector: &VersionVector,
    field: &str,
) -> Result<(), CrdtError> {
    if metadata == RegisterMetadata::ZERO {
        return Ok(());
    }
    if metadata.timestamp.value() == 0
        || metadata.operation_id.client_id.is_nil()
        || metadata.operation_id.sequence == 0
    {
        return Err(CrdtError::InvalidSnapshot(format!(
            "{field} has invalid register metadata"
        )));
    }
    if metadata.timestamp > clock {
        return Err(CrdtError::InvalidSnapshot(format!(
            "{field} register metadata is newer than the snapshot clock"
        )));
    }
    if version_vector.get(metadata.operation_id.client_id) < metadata.operation_id.sequence {
        return Err(CrdtError::InvalidSnapshot(format!(
            "{field} register metadata is outside the snapshot version vector"
        )));
    }
    Ok(())
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

fn metadata_of<T>(register: &Register<T>) -> RegisterMetadata {
    register.metadata
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

    fn validate_metadata(
        &self,
        clock: LamportTimestamp,
        version_vector: &VersionVector,
    ) -> Result<(), CrdtError> {
        if let Some(metadata) = self.created {
            validate_snapshot_metadata(metadata, clock, version_vector, "created")?;
        }
        if let Some(metadata) = self.deleted {
            validate_snapshot_metadata(metadata, clock, version_vector, "deleted")?;
        }
        for (field, metadata) in [
            ("kind", metadata_of(&self.kind)),
            ("position", metadata_of(&self.position)),
            ("size", metadata_of(&self.size)),
            ("rotation", metadata_of(&self.rotation)),
            ("stroke", metadata_of(&self.stroke)),
            ("fill", metadata_of(&self.fill)),
            ("stroke_width", metadata_of(&self.stroke_width)),
            ("stroke_style", metadata_of(&self.stroke_style)),
            ("sloppiness", metadata_of(&self.sloppiness)),
            ("edges", metadata_of(&self.edges)),
            ("opacity", metadata_of(&self.opacity)),
            ("font_family", metadata_of(&self.font_family)),
            ("font_size", metadata_of(&self.font_size)),
            ("text_align", metadata_of(&self.text_align)),
            ("text", metadata_of(&self.text)),
            ("points", metadata_of(&self.points)),
            ("image", metadata_of(&self.image)),
            ("z_index", metadata_of(&self.z_index)),
        ] {
            validate_snapshot_metadata(metadata, clock, version_vector, field)?;
        }
        Ok(())
    }

    fn validate(
        &self,
        clock: LamportTimestamp,
        version_vector: &VersionVector,
    ) -> Result<(), CrdtError> {
        if self.id.is_nil() {
            return Err(CrdtError::InvalidSnapshot(
                "element id cannot be nil".to_owned(),
            ));
        }
        self.validate_metadata(clock, version_vector)?;
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
    /// Version-vector range covered by compacted operation history.
    #[serde(default)]
    pub compacted_version_vector: VersionVector,
    /// Stable fingerprints for operations whose payload is still retained.
    /// IDs covered by `compacted_version_vector` are permanently tombstoned
    /// and are not replayed after compaction.
    #[serde(default)]
    pub operation_fingerprints: Vec<OperationFingerprint>,
}

/// Compact exact-content identity for one operation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OperationFingerprint {
    /// Operation identity.
    pub id: OperationId,
    /// SHA-256 digest of the canonical operation encoding.
    pub digest: [u8; 32],
}

/// Result of accepting a valid operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyResult {
    /// The operation was newly accepted and incorporated.
    Applied,
    /// The operation was already accepted or is covered by compacted causal
    /// history.
    Duplicate,
}

/// Operation-based document replica.
#[derive(Clone, Debug, Default)]
pub struct CrdtDocument {
    elements: BTreeMap<ElementId, ElementSnapshot>,
    seen_operations: BTreeMap<OperationId, Operation>,
    version_vector: VersionVector,
    clock: LamportClock,
    materialized: Document,
    compacted_version_vector: VersionVector,
    operation_fingerprints: BTreeMap<OperationId, [u8; 32]>,
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
            materialized: Document::default(),
            compacted_version_vector: VersionVector::default(),
            operation_fingerprints: BTreeMap::new(),
        }
    }

    /// Applies one operation through the CRDT mutation path.
    ///
    /// # Errors
    ///
    /// Returns a [`CrdtError`] when the operation is invalid, reuses a retained
    /// ID with different content, or exceeds the element bound.
    pub fn apply(&mut self, operation: &Operation) -> Result<ApplyResult, CrdtError> {
        operation.validate()?;
        let fingerprint = operation_fingerprint(operation)?;
        if operation.id.sequence <= self.compacted_version_vector.get(operation.id.client_id) {
            return Ok(ApplyResult::Duplicate);
        }
        if let Some(previous) = self.seen_operations.get(&operation.id) {
            return if previous == operation {
                Ok(ApplyResult::Duplicate)
            } else {
                Err(CrdtError::OperationIdReuse(operation.id.to_string()))
            };
        }
        if let Some(previous) = self.operation_fingerprints.get(&operation.id) {
            return if previous == &fingerprint {
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

        let target_element_id = operation.target_element_id();
        let metadata = RegisterMetadata::from_operation(operation);
        self.apply_kind(&operation.kind, metadata);
        if let Some(element) = self
            .elements
            .get(&target_element_id)
            .and_then(ElementSnapshot::materialize)
        {
            self.materialized.upsert_element(element);
        } else {
            self.materialized.remove_element(target_element_id);
        }
        self.seen_operations.insert(operation.id, operation.clone());
        self.operation_fingerprints
            .insert(operation.id, fingerprint);
        self.version_vector.observe(operation.id);
        self.clock.observe(operation.timestamp);
        Ok(ApplyResult::Applied)
    }

    /// Validates a batch against the current replica without cloning the
    /// document. The returned results are the results `apply` will produce
    /// when the batch is subsequently committed in order.
    ///
    /// # Errors
    ///
    /// Returns the first validation, ID-reuse, or capacity error in the batch.
    pub fn validate_batch(&self, operations: &[Operation]) -> Result<Vec<ApplyResult>, CrdtError> {
        let mut fingerprints = BTreeMap::new();
        let mut new_elements = BTreeSet::new();
        let mut results = Vec::with_capacity(operations.len());

        for operation in operations {
            operation.validate()?;
            let fingerprint = operation_fingerprint(operation)?;
            if operation.id.sequence <= self.compacted_version_vector.get(operation.id.client_id) {
                results.push(ApplyResult::Duplicate);
                continue;
            }
            if let Some(previous) = self.seen_operations.get(&operation.id) {
                if previous == operation {
                    results.push(ApplyResult::Duplicate);
                    continue;
                }
                return Err(CrdtError::OperationIdReuse(operation.id.to_string()));
            }
            if let Some(previous) = self.operation_fingerprints.get(&operation.id) {
                if previous == &fingerprint {
                    results.push(ApplyResult::Duplicate);
                    continue;
                }
                return Err(CrdtError::OperationIdReuse(operation.id.to_string()));
            }
            if let Some(previous) = fingerprints.insert(operation.id, fingerprint) {
                if previous == fingerprint {
                    results.push(ApplyResult::Duplicate);
                    continue;
                }
                return Err(CrdtError::OperationIdReuse(operation.id.to_string()));
            }
            if let OperationKind::Create { element } = &operation.kind
                && !self.elements.contains_key(&element.id)
                && new_elements.insert(element.id)
                && self.elements.len() + new_elements.len() > MAX_ELEMENTS
            {
                return Err(CrdtError::TooManyElements);
            }
            results.push(ApplyResult::Applied);
        }

        Ok(results)
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
        self.materialized.clone()
    }

    /// Returns the visible materialized document without cloning it.
    #[must_use]
    pub const fn document_ref(&self) -> &Document {
        &self.materialized
    }

    fn build_document(&self) -> Document {
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

    /// Compacts remembered operation payloads while retaining contiguous
    /// received coverage.
    ///
    /// Only sequences that were actually received in order are covered. This
    /// avoids treating an unseen out-of-order operation as a duplicate while
    /// allowing compacted IDs to be represented by a bounded version vector.
    pub fn compact_seen_operations(&mut self, retention: usize) {
        self.advance_compacted_version_vector();
        let remove_count = self.seen_operations.len().saturating_sub(retention);
        let ids = self
            .seen_operations
            .keys()
            .filter(|id| id.sequence <= self.compacted_version_vector.get(id.client_id))
            .take(remove_count)
            .copied()
            .collect::<Vec<_>>();
        for id in ids {
            self.seen_operations.remove(&id);
            self.operation_fingerprints.remove(&id);
        }
    }

    /// Returns the number of operation payloads retained for exact duplicate
    /// and operation-ID reuse checks.
    #[must_use]
    pub fn seen_operation_count(&self) -> usize {
        self.seen_operations.len()
    }

    fn advance_compacted_version_vector(&mut self) {
        let client_ids = self
            .seen_operations
            .keys()
            .map(|id| id.client_id)
            .collect::<BTreeSet<_>>();
        for client_id in client_ids {
            let mut next = self
                .compacted_version_vector
                .get(client_id)
                .saturating_add(1);
            while next != 0
                && self
                    .seen_operations
                    .contains_key(&OperationId::new(client_id, next))
            {
                next = next.saturating_add(1);
            }
            let contiguous = next.saturating_sub(1);
            self.compacted_version_vector
                .advance_to(client_id, contiguous);
        }
    }

    /// Creates a canonical serializable snapshot.
    #[must_use]
    pub fn snapshot(&self) -> CrdtSnapshot {
        CrdtSnapshot {
            elements: self.elements.values().cloned().collect(),
            seen_operations: self.seen_operations.values().cloned().collect(),
            version_vector: self.version_vector.clone(),
            clock: self.clock.current(),
            compacted_version_vector: self.compacted_version_vector.clone(),
            operation_fingerprints: self
                .operation_fingerprints
                .iter()
                .map(|(id, digest)| OperationFingerprint {
                    id: *id,
                    digest: *digest,
                })
                .collect(),
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
            compacted_version_vector,
            operation_fingerprints: snapshot_fingerprints,
        } = snapshot;
        version_vector.validate()?;
        if !version_vector.dominates(&compacted_version_vector) {
            return Err(CrdtError::InvalidSnapshot(
                "compacted version vector exceeds snapshot knowledge".to_owned(),
            ));
        }
        if snapshot_elements.len() > MAX_ELEMENTS {
            return Err(CrdtError::TooManyElements);
        }
        let mut elements = BTreeMap::new();
        for state in snapshot_elements {
            state.validate(clock, &version_vector)?;
            if elements.insert(state.id, state).is_some() {
                return Err(CrdtError::InvalidSnapshot(
                    "snapshot contains duplicate element IDs".to_owned(),
                ));
            }
        }
        let mut seen_operations = BTreeMap::new();
        for operation in snapshot_operations {
            operation.validate()?;
            if operation.timestamp > clock {
                return Err(CrdtError::InvalidSnapshot(
                    "snapshot operation is newer than the snapshot clock".to_owned(),
                ));
            }
            if seen_operations.insert(operation.id, operation).is_some() {
                return Err(CrdtError::InvalidSnapshot(
                    "snapshot contains duplicate operation IDs".to_owned(),
                ));
            }
        }
        let mut operation_fingerprints = BTreeMap::new();
        for fingerprint in snapshot_fingerprints {
            if operation_fingerprints
                .insert(fingerprint.id, fingerprint.digest)
                .is_some()
            {
                return Err(CrdtError::InvalidSnapshot(
                    "snapshot contains duplicate operation fingerprints".to_owned(),
                ));
            }
        }
        for operation in seen_operations.values() {
            let fingerprint = operation_fingerprint(operation)?;
            if let Some(previous) = operation_fingerprints.insert(operation.id, fingerprint)
                && previous != fingerprint
            {
                return Err(CrdtError::InvalidSnapshot(
                    "snapshot operation fingerprint does not match payload".to_owned(),
                ));
            }
        }
        validate_snapshot_fingerprints(
            &version_vector,
            &compacted_version_vector,
            &operation_fingerprints,
        )?;
        validate_retained_register_metadata(&elements, &seen_operations)?;
        let mut document = Self {
            elements,
            seen_operations,
            version_vector,
            clock: LamportClock::from_timestamp(clock),
            materialized: Document::default(),
            compacted_version_vector,
            operation_fingerprints,
        };
        document.materialized = document.build_document();
        Ok(document)
    }
}

fn state_metadata(state: &ElementSnapshot) -> impl Iterator<Item = RegisterMetadata> + '_ {
    state
        .created
        .into_iter()
        .chain(state.deleted)
        .chain([
            state.kind.metadata,
            state.position.metadata,
            state.size.metadata,
            state.rotation.metadata,
            state.stroke.metadata,
            state.fill.metadata,
            state.stroke_width.metadata,
            state.stroke_style.metadata,
            state.sloppiness.metadata,
            state.edges.metadata,
            state.opacity.metadata,
            state.font_family.metadata,
            state.font_size.metadata,
            state.text_align.metadata,
            state.text.metadata,
            state.points.metadata,
            state.image.metadata,
            state.z_index.metadata,
        ])
        .filter(|metadata| *metadata != RegisterMetadata::ZERO)
}

fn validate_snapshot_fingerprints(
    version_vector: &VersionVector,
    compacted_version_vector: &VersionVector,
    operation_fingerprints: &BTreeMap<OperationId, [u8; 32]>,
) -> Result<(), CrdtError> {
    for operation_id in operation_fingerprints.keys() {
        if operation_id.client_id.is_nil()
            || operation_id.sequence == 0
            || version_vector.get(operation_id.client_id) < operation_id.sequence
        {
            return Err(CrdtError::InvalidSnapshot(
                "snapshot contains operation fingerprint outside version vector".to_owned(),
            ));
        }
    }
    let mut covered_version = compacted_version_vector.clone();
    for operation_id in operation_fingerprints.keys() {
        covered_version.observe(*operation_id);
    }
    if covered_version != *version_vector {
        return Err(CrdtError::InvalidSnapshot(
            "snapshot version vector contains an unretained operation".to_owned(),
        ));
    }
    Ok(())
}

fn validate_retained_register_metadata(
    elements: &BTreeMap<ElementId, ElementSnapshot>,
    seen_operations: &BTreeMap<OperationId, Operation>,
) -> Result<(), CrdtError> {
    for state in elements.values() {
        for metadata in state_metadata(state) {
            if let Some(operation) = seen_operations.get(&metadata.operation_id)
                && operation.timestamp != metadata.timestamp
            {
                return Err(CrdtError::InvalidSnapshot(
                    "register metadata does not match its retained operation".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn operation_fingerprint(operation: &Operation) -> Result<[u8; 32], CrdtError> {
    let bytes = serde_json::to_vec(operation)
        .map_err(|error| CrdtError::OperationFingerprint(error.to_string()))?;
    Ok(Sha256::digest(bytes).into())
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
