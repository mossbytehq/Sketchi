//! Operation-first local editor state and compensating undo/redo.

use thiserror::Error;

use canvas_core::{
    CrdtDocument, CrdtError, Document, EditorCommand, EmbeddedImage, Operation, OperationId,
    StylePatch,
};

use crate::connection::{ConnectionError, SyncController};

/// Errors raised by local command execution.
#[derive(Debug, Error)]
pub enum EditorError {
    /// Core CRDT validation rejected the operation.
    #[error("core operation failed: {0}")]
    Core(#[from] CrdtError),
    /// No undo entry is available.
    #[error("nothing to undo")]
    NothingToUndo,
    /// No redo entry is available.
    #[error("nothing to redo")]
    NothingToRedo,
}

#[derive(Clone, Debug)]
struct HistoryEntry {
    command: EditorCommand,
    inverse: EditorCommand,
}

/// Local-first editor that turns every mutation into a core operation.
#[derive(Clone, Debug)]
pub struct Editor {
    client_id: canvas_core::ClientId,
    next_sequence: u64,
    crdt: CrdtDocument,
    pending: Vec<Operation>,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

impl Editor {
    /// Creates an empty editor for one stable client identity.
    #[must_use]
    pub fn new(client_id: canvas_core::ClientId) -> Self {
        Self {
            client_id,
            next_sequence: 1,
            crdt: CrdtDocument::new(),
            pending: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Executes a command locally and queues its operation for transport.
    ///
    /// # Errors
    ///
    /// Returns [`EditorError::Core`] when the shared CRDT rejects the command.
    pub fn execute(&mut self, command: EditorCommand) -> Result<OperationId, EditorError> {
        let inverse = self.inverse_for(&command);
        let operation = self.apply_command(command.clone())?;
        if let Some(inverse) = inverse {
            self.undo.push(HistoryEntry { command, inverse });
            self.redo.clear();
        }
        Ok(operation.id)
    }

    /// Applies a remote operation through the same CRDT path without queueing it.
    ///
    /// # Errors
    ///
    /// Returns [`EditorError::Core`] when the shared CRDT rejects the operation.
    pub fn apply_remote(&mut self, operation: &Operation) -> Result<(), EditorError> {
        self.crdt.apply(operation)?;
        self.next_sequence = self
            .next_sequence
            .max(operation.id.sequence.saturating_add(1));
        Ok(())
    }

    /// Creates a compensating operation for the latest local command.
    ///
    /// # Errors
    ///
    /// Returns [`EditorError::NothingToUndo`] when history is empty, or
    /// [`EditorError::Core`] when the inverse operation is rejected.
    pub fn undo(&mut self) -> Result<OperationId, EditorError> {
        let entry = self.undo.pop().ok_or(EditorError::NothingToUndo)?;
        let operation = self.apply_command(entry.inverse.clone())?;
        self.redo.push(entry);
        Ok(operation.id)
    }

    /// Replays the latest undone command as a new operation.
    ///
    /// # Errors
    ///
    /// Returns [`EditorError::NothingToRedo`] when history is empty, or
    /// [`EditorError::Core`] when the replay is rejected.
    pub fn redo(&mut self) -> Result<OperationId, EditorError> {
        let entry = self.redo.pop().ok_or(EditorError::NothingToRedo)?;
        let operation = self.apply_command(entry.command.clone())?;
        self.undo.push(entry);
        Ok(operation.id)
    }

    /// Returns the current materialized document.
    #[must_use]
    pub fn document(&self) -> Document {
        self.crdt.document()
    }

    /// Returns queued local operations awaiting acknowledgement.
    #[must_use]
    pub fn pending_operations(&self) -> &[Operation] {
        &self.pending
    }

    /// Removes and returns all queued local operations for transport.
    pub fn take_pending(&mut self) -> Vec<Operation> {
        std::mem::take(&mut self.pending)
    }

    /// Persists local operations into the reconnect journal and clears the
    /// in-memory transport queue only after the journal transaction succeeds.
    ///
    /// # Errors
    ///
    /// Returns [`ConnectionError`] when the durable journal cannot be updated.
    pub fn persist_pending(
        &mut self,
        synchronization: &mut SyncController,
    ) -> Result<usize, ConnectionError> {
        if self.pending.is_empty() {
            return Ok(0);
        }
        synchronization.enqueue_all(&self.pending)?;
        let count = self.pending.len();
        self.pending.clear();
        Ok(count)
    }

    /// Removes acknowledged operation IDs from the local queue.
    pub fn acknowledge(&mut self, ids: &[OperationId]) {
        self.pending
            .retain(|operation| !ids.contains(&operation.id));
    }

    /// Returns the stable client identity.
    #[must_use]
    pub const fn client_id(&self) -> canvas_core::ClientId {
        self.client_id
    }

    fn apply_command(&mut self, command: EditorCommand) -> Result<Operation, EditorError> {
        let operation_id = OperationId::new(self.client_id, self.next_sequence);
        self.next_sequence = self.next_sequence.saturating_add(1);
        let timestamp = self.crdt.tick();
        let deps = self.crdt.version_vector().clone();
        let operation = command.into_operation(operation_id, timestamp, deps);
        self.crdt.apply(&operation)?;
        self.pending.push(operation.clone());
        Ok(operation)
    }

    fn inverse_for(&self, command: &EditorCommand) -> Option<EditorCommand> {
        match command {
            EditorCommand::Create(element) => Some(EditorCommand::Delete(element.id)),
            EditorCommand::Delete(element_id) => self
                .document()
                .element(*element_id)
                .cloned()
                .map(EditorCommand::Create),
            EditorCommand::SetPosition(element_id, _) => self
                .document()
                .element(*element_id)
                .map(|element| EditorCommand::SetPosition(*element_id, element.transform.position)),
            EditorCommand::SetSize(element_id, _) => self
                .document()
                .element(*element_id)
                .map(|element| EditorCommand::SetSize(*element_id, element.transform.size)),
            EditorCommand::SetRotation(element_id, _) => self
                .document()
                .element(*element_id)
                .map(|element| EditorCommand::SetRotation(*element_id, element.transform.rotation)),
            EditorCommand::SetStyle(element_id, _) => {
                self.document().element(*element_id).map(|element| {
                    EditorCommand::SetStyle(
                        *element_id,
                        StylePatch {
                            stroke: Some(element.style.stroke),
                            fill: Some(element.style.fill),
                            stroke_width: Some(element.style.stroke_width),
                            stroke_style: Some(element.style.stroke_style),
                            sloppiness: Some(element.style.sloppiness),
                            edges: Some(element.style.edges),
                            opacity: Some(element.style.opacity),
                            font_family: Some(element.style.font_family),
                            font_size: Some(element.style.font_size),
                            text_align: Some(element.style.text_align),
                        },
                    )
                })
            }
            EditorCommand::SetText(element_id, _) => self
                .document()
                .element(*element_id)
                .map(|element| EditorCommand::SetText(*element_id, element.text.clone())),
            EditorCommand::SetImage(element_id, _) => self
                .document()
                .element(*element_id)
                .and_then(|element| element.image.clone())
                .map(|image: EmbeddedImage| EditorCommand::SetImage(*element_id, image)),
            EditorCommand::SetPoints(element_id, _) => self
                .document()
                .element(*element_id)
                .map(|element| EditorCommand::SetPoints(*element_id, element.points.clone())),
            EditorCommand::Reorder(element_id, _) => self
                .document()
                .element(*element_id)
                .map(|element| EditorCommand::Reorder(*element_id, element.z_index)),
        }
    }
}
