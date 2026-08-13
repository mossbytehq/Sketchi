//! Capability-authenticated room state and operation application.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use canvas_core::{ClientId, CrdtDocument, CrdtSnapshot, Document, Operation, VersionVector};
use canvas_protocol::{PresenceState, RoomId};
use thiserror::Error;

use crate::{auth::CapabilityToken, store::RoomStore};

/// Snapshot interval by operation count.
pub const SNAPSHOT_OPERATION_INTERVAL: usize = 500;
/// Snapshot interval by elapsed wall time.
pub const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(30);
/// Recent operation payloads retained for exact duplicate/reuse checks.
const SEEN_OPERATION_RETENTION: usize = 256;
/// Maximum number of idle rooms retained in the manager cache.
const ROOM_CACHE_CAPACITY: usize = 256;

/// A newly created room and its one-time returned capability secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatedRoom {
    /// Room identity.
    pub room_id: RoomId,
    /// Secret required to join.
    pub token: CapabilityToken,
}

/// Result of submitting operations to a room.
#[derive(Clone, Debug, PartialEq)]
pub struct SubmitOutcome {
    /// Newly applied operations that should be broadcast.
    pub applied: Vec<Operation>,
    /// All operation IDs durably known after the request, including duplicates.
    pub acknowledged: Vec<canvas_core::OperationId>,
}

/// Synchronization payload for a client version vector.
#[derive(Clone, Debug, PartialEq)]
pub struct SyncPayload {
    /// Complete current snapshot.
    pub snapshot: CrdtSnapshot,
    /// Durable operations not covered by the caller's version vector.
    pub operations: Vec<Operation>,
}

/// Room-specific errors.
#[derive(Debug, Error)]
pub enum RoomError {
    /// The room is not known by the manager or store.
    #[error("room not found")]
    RoomNotFound,
    /// The operation was authored by another client.
    #[error("operation client does not match the joined client")]
    ClientMismatch,
    /// The caller is not currently joined.
    #[error("client is not joined to the room")]
    NotInRoom,
    /// A shared core operation failed.
    #[error(transparent)]
    Core(#[from] canvas_core::CrdtError),
    /// `SQLite` persistence failed.
    #[error(transparent)]
    Store(#[from] crate::store::StoreError),
    /// The capability token was invalid.
    #[error("invalid room capability")]
    Unauthorized,
    /// The room store mutex was poisoned.
    #[error("room store lock is poisoned")]
    StoreLock,
    /// The room actor task or channel has stopped.
    #[error("room actor is stopped")]
    ActorStopped,
}

/// One in-memory room actor state.
#[allow(clippy::struct_field_names)]
pub struct Room {
    room_id: RoomId,
    document: CrdtDocument,
    operations: Vec<Operation>,
    members: BTreeSet<ClientId>,
    presence: BTreeMap<ClientId, PresenceState>,
    store: Arc<Mutex<RoomStore>>,
    operations_since_snapshot: usize,
    last_snapshot: Instant,
    last_activity: Instant,
}

impl Room {
    /// Returns the room identity.
    #[must_use]
    pub const fn id(&self) -> RoomId {
        self.room_id
    }

    /// Loads a room's snapshot and operation log from the store.
    ///
    /// # Errors
    ///
    /// Returns [`RoomError`] when persistence or snapshot validation fails.
    pub fn load(room_id: RoomId, store: Arc<Mutex<RoomStore>>) -> Result<Self, RoomError> {
        let (snapshot, operations) = {
            let store = store.lock().map_err(|_| RoomError::StoreLock)?;
            (
                store.load_snapshot(room_id)?,
                store.load_operations(room_id)?,
            )
        };
        let mut document = snapshot
            .map(CrdtDocument::from_snapshot)
            .transpose()?
            .unwrap_or_default();
        for operation in &operations {
            document.apply(operation)?;
        }
        Ok(Self {
            room_id,
            document,
            operations,
            members: BTreeSet::new(),
            presence: BTreeMap::new(),
            store,
            operations_since_snapshot: 0,
            last_snapshot: Instant::now(),
            last_activity: Instant::now(),
        })
    }

    pub(crate) fn join(&mut self, client_id: ClientId) {
        self.members.insert(client_id);
        self.last_activity = Instant::now();
    }

    pub(crate) fn leave(&mut self, client_id: ClientId) {
        self.members.remove(&client_id);
        self.presence.remove(&client_id);
        self.last_activity = Instant::now();
    }

    fn has_members(&self) -> bool {
        !self.members.is_empty()
    }

    fn is_member(&self, client_id: ClientId) -> bool {
        self.members.contains(&client_id)
    }

    pub(crate) fn submit(
        &mut self,
        client_id: ClientId,
        operations: &[Operation],
    ) -> Result<SubmitOutcome, RoomError> {
        self.last_activity = Instant::now();
        if !self.is_member(client_id) {
            return Err(RoomError::NotInRoom);
        }
        for operation in operations {
            if operation.id.client_id != client_id {
                return Err(RoomError::ClientMismatch);
            }
        }
        let mut applied = Vec::new();
        let mut acknowledged = Vec::new();
        let results = self.document.validate_batch(operations)?;
        for (operation, result) in operations.iter().zip(results) {
            match result {
                canvas_core::ApplyResult::Applied => {
                    applied.push(operation.clone());
                    acknowledged.push(operation.id);
                }
                canvas_core::ApplyResult::Duplicate => {
                    acknowledged.push(operation.id);
                }
            }
        }
        if !applied.is_empty() {
            let mut store = self.store.lock().map_err(|_| RoomError::StoreLock)?;
            store.append_operations(self.room_id, &applied)?;
            for operation in &applied {
                debug_assert_eq!(
                    self.document.apply(operation)?,
                    canvas_core::ApplyResult::Applied
                );
            }
            self.operations.extend(applied.iter().cloned());
            self.operations_since_snapshot += applied.len();
            if self.operations_since_snapshot >= SNAPSHOT_OPERATION_INTERVAL
                || self.last_snapshot.elapsed() >= SNAPSHOT_INTERVAL
            {
                self.document
                    .compact_seen_operations(SEEN_OPERATION_RETENTION);
                let snapshot = self.document.snapshot();
                store.save_snapshot(self.room_id, &snapshot)?;
                store.compact_operations(self.room_id)?;
                self.operations.clear();
                self.operations_since_snapshot = 0;
                self.last_snapshot = Instant::now();
            }
        }
        Ok(SubmitOutcome {
            applied,
            acknowledged,
        })
    }

    pub(crate) fn sync(&mut self, known_version: &VersionVector) -> SyncPayload {
        self.last_activity = Instant::now();
        let operations = self
            .operations
            .iter()
            .filter(|operation| operation.id.sequence > known_version.get(operation.id.client_id))
            .cloned()
            .collect();
        SyncPayload {
            snapshot: self.document.snapshot(),
            operations,
        }
    }

    fn document(&self) -> Document {
        self.document.document()
    }

    pub(crate) fn update_presence(&mut self, state: PresenceState) -> Result<(), RoomError> {
        if !self.is_member(state.client_id) {
            return Err(RoomError::NotInRoom);
        }
        self.presence.insert(state.client_id, state);
        self.last_activity = Instant::now();
        Ok(())
    }
}

/// Thread-safe room registry and capability gate.
pub struct RoomManager {
    store: Arc<Mutex<RoomStore>>,
    rooms: BTreeMap<RoomId, Room>,
}

impl RoomManager {
    /// Creates a manager backed by a shared `SQLite` store.
    #[must_use]
    pub fn new(store: Arc<Mutex<RoomStore>>) -> Self {
        Self {
            store,
            rooms: BTreeMap::new(),
        }
    }

    /// Creates and persists a new capability-token room.
    ///
    /// # Errors
    ///
    /// Returns [`RoomError`] when `SQLite` initialization or room loading fails.
    pub fn create_room(&mut self) -> Result<CreatedRoom, RoomError> {
        self.evict_idle_rooms();
        let room_id = RoomId::new();
        let token = CapabilityToken::generate();
        self.store
            .lock()
            .map_err(|_| RoomError::StoreLock)?
            .create_room(room_id, &token.hash())?;
        let room = Room::load(room_id, Arc::clone(&self.store))?;
        self.rooms.insert(room_id, room);
        Ok(CreatedRoom { room_id, token })
    }

    /// Joins a room after verifying the capability token.
    ///
    /// # Errors
    ///
    /// Returns [`RoomError`] when the room/token is invalid.
    pub fn join(
        &mut self,
        room_id: RoomId,
        token: &CapabilityToken,
        client_id: ClientId,
    ) -> Result<(), RoomError> {
        let expected = self
            .store
            .lock()
            .map_err(|_| RoomError::StoreLock)?
            .token_hash(room_id)?
            .ok_or(RoomError::RoomNotFound)?;
        if !token.verify(&expected) {
            return Err(RoomError::Unauthorized);
        }
        let room = self.room_mut(room_id)?;
        room.join(client_id);
        Ok(())
    }

    /// Leaves a room.
    ///
    /// # Errors
    ///
    /// Returns [`RoomError::NotInRoom`] when the room is missing.
    pub fn leave(&mut self, room_id: RoomId, client_id: ClientId) -> Result<(), RoomError> {
        let remove = {
            let room = self.room_mut(room_id)?;
            room.leave(client_id);
            !room.has_members()
        };
        if remove {
            self.rooms.remove(&room_id);
        }
        Ok(())
    }

    /// Validates, durably commits, and applies a batch of operations.
    ///
    /// # Errors
    ///
    /// Returns [`RoomError`] when the client is not joined, the operation
    /// identity is invalid, or SQLite/core application fails.
    pub fn submit(
        &mut self,
        room_id: RoomId,
        client_id: ClientId,
        operations: &[Operation],
    ) -> Result<SubmitOutcome, RoomError> {
        self.room_mut(room_id)?.submit(client_id, operations)
    }

    /// Returns a snapshot plus operations after known causal state.
    ///
    /// # Errors
    ///
    /// Returns [`RoomError::RoomNotFound`] when the room is unknown.
    pub fn sync(
        &mut self,
        room_id: RoomId,
        known_version: &VersionVector,
    ) -> Result<SyncPayload, RoomError> {
        Ok(self.room_mut(room_id)?.sync(known_version))
    }

    /// Returns the visible materialized document.
    ///
    /// # Errors
    ///
    /// Returns [`RoomError::RoomNotFound`] when the room is unknown.
    pub fn document(&mut self, room_id: RoomId) -> Result<Document, RoomError> {
        Ok(self.room_mut(room_id)?.document())
    }

    /// Updates ephemeral presence without persistence.
    ///
    /// # Errors
    ///
    /// Returns [`RoomError`] when the room/member is unknown.
    pub fn update_presence(
        &mut self,
        room_id: RoomId,
        state: PresenceState,
    ) -> Result<(), RoomError> {
        self.room_mut(room_id)?.update_presence(state)
    }

    fn room_mut(&mut self, room_id: RoomId) -> Result<&mut Room, RoomError> {
        if !self.rooms.contains_key(&room_id) {
            self.evict_idle_rooms();
            let room =
                Room::load(room_id, Arc::clone(&self.store)).map_err(|error| match error {
                    RoomError::Store(crate::store::StoreError::RoomNotFound) => {
                        RoomError::RoomNotFound
                    }
                    other => other,
                })?;
            self.rooms.insert(room_id, room);
        }
        self.rooms.get_mut(&room_id).ok_or(RoomError::NotInRoom)
    }

    fn evict_idle_rooms(&mut self) {
        while self.rooms.len() >= ROOM_CACHE_CAPACITY {
            let Some(room_id) = self
                .rooms
                .iter()
                .filter(|(_, room)| !room.has_members())
                .min_by_key(|(_, room)| room.last_activity)
                .map(|(room_id, _)| *room_id)
            else {
                break;
            };
            self.rooms.remove(&room_id);
        }
    }
}
