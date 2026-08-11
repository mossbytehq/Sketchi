//! `SQLite` room, operation-log, and snapshot persistence.

use std::path::Path;

use canvas_core::{CrdtSnapshot, Operation};
use canvas_protocol::RoomId;
use rusqlite::{Connection, params};
use thiserror::Error;

/// Persistence failures.
#[derive(Debug, Error)]
pub enum StoreError {
    /// `SQLite` operation failed.
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// JSON operation or snapshot encoding failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// The store did not contain the requested room.
    #[error("room not found")]
    RoomNotFound,
}

/// SQLite-backed room store.
pub struct RoomStore {
    connection: Connection,
}

impl RoomStore {
    /// Opens a database and applies the checked-in migrations.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the database cannot be opened or migrated.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::from_connection(Connection::open(path)?)
    }

    /// Opens an isolated in-memory store for tests.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the schema cannot be initialized.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self, StoreError> {
        connection.execute_batch(include_str!("../migrations/001_initial.sql"))?;
        Ok(Self { connection })
    }

    /// Creates a room with only the hashed capability token persisted.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `SQLite` cannot insert the room.
    pub fn create_room(&mut self, room_id: RoomId, token_hash: &str) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO rooms (room_id, token_hash, created_at) VALUES (?1, ?2, unixepoch())",
            params![room_id.to_string(), token_hash],
        )?;
        Ok(())
    }

    /// Returns the stored capability hash, if the room exists.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `SQLite` cannot query the room.
    pub fn token_hash(&self, room_id: RoomId) -> Result<Option<String>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT token_hash FROM rooms WHERE room_id = ?1")?;
        let mut rows = statement.query(params![room_id.to_string()])?;
        rows.next()?
            .map(|row| row.get(0))
            .transpose()
            .map_err(StoreError::from)
    }

    /// Returns whether a room exists.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `SQLite` cannot query the room.
    pub fn room_exists(&self, room_id: RoomId) -> Result<bool, StoreError> {
        Ok(self.token_hash(room_id)?.is_some())
    }

    /// Atomically appends a batch of durable operations.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] and rolls back the batch when `SQLite` cannot
    /// commit it.
    pub fn append_operations(
        &mut self,
        room_id: RoomId,
        operations: &[Operation],
    ) -> Result<(), StoreError> {
        if !self.room_exists(room_id)? {
            return Err(StoreError::RoomNotFound);
        }
        let transaction = self.connection.transaction()?;
        for operation in operations {
            let payload = serde_json::to_string(operation)?;
            transaction.execute(
                "INSERT OR IGNORE INTO operations (room_id, operation_id, payload, created_at) VALUES (?1, ?2, ?3, unixepoch())",
                params![room_id.to_string(), operation.id.to_string(), payload],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Appends one operation atomically.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the room is missing or the transaction
    /// cannot commit.
    pub fn append_operation(
        &mut self,
        room_id: RoomId,
        operation: &Operation,
    ) -> Result<(), StoreError> {
        self.append_operations(room_id, std::slice::from_ref(operation))
    }

    /// Loads the complete operation log in insertion order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `SQLite` reads or JSON decoding fail.
    pub fn load_operations(&self, room_id: RoomId) -> Result<Vec<Operation>, StoreError> {
        if !self.room_exists(room_id)? {
            return Err(StoreError::RoomNotFound);
        }
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM operations WHERE room_id = ?1 ORDER BY rowid ASC")?;
        let rows =
            statement.query_map(params![room_id.to_string()], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    /// Stores the latest complete CRDT snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the room is missing or SQLite/JSON fails.
    pub fn save_snapshot(
        &mut self,
        room_id: RoomId,
        snapshot: &CrdtSnapshot,
    ) -> Result<(), StoreError> {
        if !self.room_exists(room_id)? {
            return Err(StoreError::RoomNotFound);
        }
        let payload = serde_json::to_string(snapshot)?;
        self.connection.execute(
            "INSERT INTO snapshots (room_id, payload, created_at) VALUES (?1, ?2, unixepoch())
             ON CONFLICT(room_id) DO UPDATE SET payload = excluded.payload, created_at = excluded.created_at",
            params![room_id.to_string(), payload],
        )?;
        Ok(())
    }

    /// Loads the latest snapshot, if one has been created.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `SQLite` reads or JSON decoding fail.
    pub fn load_snapshot(&self, room_id: RoomId) -> Result<Option<CrdtSnapshot>, StoreError> {
        if !self.room_exists(room_id)? {
            return Err(StoreError::RoomNotFound);
        }
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM snapshots WHERE room_id = ?1")?;
        let mut rows = statement.query(params![room_id.to_string()])?;
        rows.next()?
            .map(|row| {
                let payload = row.get::<_, String>(0)?;
                serde_json::from_str(&payload).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
            })
            .transpose()
            .map_err(StoreError::from)
    }
}
