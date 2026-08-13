//! Read-only materialized document view.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

use crate::{element::Element, ids::ElementId};

/// Materialized visible document state.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Document {
    elements: BTreeMap<ElementId, Element>,
    #[serde(skip)]
    ordered_element_ids: Vec<ElementId>,
}

#[derive(Deserialize)]
struct DocumentWire {
    elements: BTreeMap<ElementId, Element>,
}

impl<'de> Deserialize<'de> for Document {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = DocumentWire::deserialize(deserializer)?;
        Ok(Self::from_elements(wire.elements))
    }
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

    /// Iterates visible elements in deterministic stacking order.
    #[must_use]
    pub fn elements_in_z_order(&self) -> impl DoubleEndedIterator<Item = &Element> {
        self.ordered_element_ids
            .iter()
            .filter_map(|id| self.elements.get(id))
    }

    pub(crate) fn from_elements(elements: BTreeMap<ElementId, Element>) -> Self {
        let mut ordered_element_ids = elements
            .values()
            .map(|element| element.id)
            .collect::<Vec<_>>();
        ordered_element_ids.sort_by_key(|id| {
            elements
                .get(id)
                .map_or((0, *id), |element| (element.z_index, element.id))
        });
        Self {
            elements,
            ordered_element_ids,
        }
    }

    pub(crate) fn upsert_element(&mut self, element: Element) {
        let id = element.id;
        self.elements.insert(id, element);
        self.ordered_element_ids
            .retain(|candidate| *candidate != id);
        let order_key = self
            .elements
            .get(&id)
            .map_or((0, id), |element| (element.z_index, element.id));
        let position = self
            .ordered_element_ids
            .binary_search_by_key(&order_key, |candidate| {
                self.elements
                    .get(candidate)
                    .map_or((0, *candidate), |element| (element.z_index, element.id))
            })
            .unwrap_or_else(|position| position);
        self.ordered_element_ids.insert(position, id);
    }

    pub(crate) fn remove_element(&mut self, id: ElementId) {
        self.elements.remove(&id);
        self.ordered_element_ids
            .retain(|candidate| *candidate != id);
    }
}
