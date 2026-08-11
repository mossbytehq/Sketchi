//! Renderer-side selection state.

use std::collections::BTreeSet;

use canvas_core::ElementId;

/// Ephemeral local selection state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SelectionState {
    selected: BTreeSet<ElementId>,
}

impl SelectionState {
    /// Returns whether an element is selected.
    #[must_use]
    pub fn contains(&self, element_id: ElementId) -> bool {
        self.selected.contains(&element_id)
    }

    /// Selects one element and clears previous selection.
    pub fn select_one(&mut self, element_id: ElementId) {
        self.selected.clear();
        self.selected.insert(element_id);
    }

    /// Toggles one element while preserving the rest of the selection.
    pub fn toggle(&mut self, element_id: ElementId) {
        if !self.selected.remove(&element_id) {
            self.selected.insert(element_id);
        }
    }

    /// Clears the selection.
    pub fn clear(&mut self) {
        self.selected.clear();
    }

    /// Iterates selected IDs in stable order.
    pub fn elements(&self) -> impl Iterator<Item = &ElementId> {
        self.selected.iter()
    }
}
