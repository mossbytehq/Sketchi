//! Operation-first local editor state and compensating undo/redo.

use thiserror::Error;

use canvas_core::{
    CrdtDocument, CrdtError, CrdtSnapshot, Document, EditorCommand, EmbeddedImage, Operation,
    OperationId, StylePatch,
};
use canvas_protocol::ServerMessage;

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

/// Errors raised while applying a server update to the editor and sync queue.
#[derive(Debug, Error)]
pub enum EditorSyncError {
    /// The shared CRDT rejected a snapshot or operation.
    #[error(transparent)]
    Editor(#[from] EditorError),
    /// The durable sync journal could not be updated or read.
    #[error(transparent)]
    Connection(#[from] ConnectionError),
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

    /// Creates an editor from a previously saved materialized document.
    ///
    /// The restored document is treated as local state: its create operations
    /// remain pending so a future connection can publish the autosaved data.
    /// Restored state does not create undo history.
    ///
    /// # Errors
    ///
    /// Returns [`EditorError::Core`] if a restored element fails core validation.
    pub fn from_document(
        client_id: canvas_core::ClientId,
        document: &Document,
    ) -> Result<Self, EditorError> {
        let mut editor = Self::new(client_id);
        for element in document.elements() {
            editor.apply_command(EditorCommand::Create(element.clone()))?;
        }
        editor.undo.clear();
        editor.redo.clear();
        Ok(editor)
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
        self.advance_local_sequence(operation);
        Ok(())
    }

    /// Applies a remote batch atomically through the shared CRDT path.
    ///
    /// The editor is unchanged when any operation in the batch is rejected.
    /// This is the boundary used for server operation frames so a malformed
    /// frame cannot leave the rendered document partially updated.
    ///
    /// # Errors
    ///
    /// Returns [`EditorError::Core`] when the shared CRDT rejects an operation.
    pub fn apply_remote_batch(&mut self, operations: &[Operation]) -> Result<(), EditorError> {
        let mut crdt = self.crdt.clone();
        for operation in operations {
            crdt.apply(operation)?;
        }
        self.crdt = crdt;
        for operation in operations {
            self.advance_local_sequence(operation);
        }
        Ok(())
    }

    /// Replaces the editor's CRDT state with a server snapshot and reapplies
    /// local operations that have not necessarily reached the server yet.
    ///
    /// `pending_operations` normally comes from the durable [`SyncController`]
    /// journal. The editor's in-memory pending operations are included as well,
    /// so a snapshot received during a local edit cannot discard that edit.
    /// Duplicate operations are harmless because the core CRDT is idempotent.
    /// The replacement is atomic: a rejected snapshot or replay leaves the
    /// current editor state untouched.
    ///
    /// # Errors
    ///
    /// Returns [`EditorError::Core`] when the snapshot or a replay operation is
    /// rejected by the shared CRDT.
    pub fn apply_snapshot(
        &mut self,
        snapshot: CrdtSnapshot,
        pending_operations: &[Operation],
    ) -> Result<(), EditorError> {
        let mut crdt = CrdtDocument::from_snapshot(snapshot)?;
        for operation in &self.pending {
            crdt.apply(operation)?;
        }
        for operation in pending_operations {
            crdt.apply(operation)?;
        }
        self.crdt = crdt;
        self.next_sequence = self
            .crdt
            .version_vector()
            .get(self.client_id)
            .saturating_add(1);
        Ok(())
    }

    /// Applies one server update to both the document and local sync state.
    ///
    /// Snapshot frames replace the CRDT state and replay durable local work;
    /// operation frames are applied atomically; acknowledgements clear both
    /// the durable journal and any still in-memory local queue. Presence and
    /// other ephemeral messages are intentionally ignored here because they
    /// belong to the UI/presence layer, not the durable document.
    ///
    /// # Errors
    ///
    /// Returns [`EditorSyncError`] when either the CRDT or durable journal
    /// rejects the update.
    pub fn apply_server_message(
        &mut self,
        synchronization: &mut SyncController,
        message: &ServerMessage,
    ) -> Result<crate::connection::SyncUpdate, EditorSyncError> {
        let update = synchronization.apply_server_message(message)?;
        match message {
            ServerMessage::Snapshot { snapshot, .. } => {
                let pending = synchronization.pending_operations()?;
                self.apply_snapshot(snapshot.clone(), &pending)?;
            }
            ServerMessage::Operations { operations, .. } => {
                self.apply_remote_batch(operations)?;
            }
            ServerMessage::Ack { accepted, .. } => self.acknowledge(accepted),
            _ => {}
        }
        Ok(update)
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
    pub fn document(&self) -> &Document {
        self.crdt.document_ref()
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

    fn advance_local_sequence(&mut self, operation: &Operation) {
        if operation.id.client_id == self.client_id {
            self.next_sequence = self
                .next_sequence
                .max(operation.id.sequence.saturating_add(1));
        }
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
