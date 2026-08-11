//! Read-only materialized document view.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{element::Element, ids::ElementId};

/// Materialized visible document state.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Document {
    elements: BTreeMap<ElementId, Element>,
}

impl Document {
    /// Returns the number of visible elements.
    #[must_use]
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Returns whether no elements are visible.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Looks up a visible element.
    #[must_use]
    pub fn element(&self, id: ElementId) -> Option<&Element> {
        self.elements.get(&id)
    }

    /// Iterates over visible elements in stable ID order.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.elements.values()
    }

    pub(crate) fn from_elements(elements: BTreeMap<ElementId, Element>) -> Self {
        Self { elements }
    }
}
