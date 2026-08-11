//! Durable local operation journal.

use std::path::Path;

use canvas_core::{Operation, OperationId};
use rusqlite::{Connection, params};
use thiserror::Error;

/// Errors raised by the local journal.
#[derive(Debug, Error)]
pub enum StorageError {
    /// `SQLite` or transaction failure.
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Operation JSON could not be encoded or decoded.
    #[error("operation JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// SQLite-backed queue for operations awaiting server acknowledgement.
pub struct Journal {
    connection: Connection,
}

impl Journal {
    /// Opens or creates a journal database and its schema.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when `SQLite` cannot open or initialize the file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    /// Opens an isolated in-memory journal for tests and ephemeral sessions.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when the in-memory schema cannot be created.
    pub fn open_in_memory() -> Result<Self, StorageError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self, StorageError> {
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS pending_operations (
                operation_id TEXT PRIMARY KEY NOT NULL,
                payload TEXT NOT NULL
            );",
        )?;
        Ok(Self { connection })
    }

    /// Appends one operation idempotently.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when encoding or inserting the operation fails.
    pub fn append(&self, operation: &Operation) -> Result<(), StorageError> {
        self.append_all(std::slice::from_ref(operation))
    }

    /// Appends a batch of operations in one transaction, retaining existing
    /// rows when a retry contains an already-journaled operation ID.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when encoding or inserting an operation fails.
    pub fn append_all(&self, operations: &[Operation]) -> Result<(), StorageError> {
        let encoded = operations
            .iter()
            .map(|operation| {
                Ok::<_, StorageError>((operation.id.to_string(), serde_json::to_string(operation)?))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let transaction = self.connection.unchecked_transaction()?;
        for (operation_id, payload) in encoded {
            transaction.execute(
                "INSERT OR IGNORE INTO pending_operations (operation_id, payload) VALUES (?1, ?2)",
                params![operation_id, payload],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Loads pending operations in stable operation-ID order.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when `SQLite` reads or JSON decoding fail.
    pub fn load(&self) -> Result<Vec<Operation>, StorageError> {
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM pending_operations ORDER BY operation_id ASC")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut operations = rows
            .map(|row| Ok(serde_json::from_str(&row?)?))
            .collect::<Result<Vec<Operation>, StorageError>>()?;
        operations.sort_unstable_by_key(|operation| operation.id);
        Ok(operations)
    }

    /// Loads at most `limit` pending operations in stable operation-ID order.
    ///
    /// A zero limit returns an empty batch without querying the database.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when `SQLite` reads or JSON decoding fail.
    pub fn load_batch(&self, limit: usize) -> Result<Vec<Operation>, StorageError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut operations = self.load()?;
        operations.truncate(limit);
        Ok(operations)
    }

    /// Returns the number of operations still awaiting acknowledgement.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when `SQLite` cannot read the count.
    pub fn pending_count(&self) -> Result<usize, StorageError> {
        let count: i64 =
            self.connection
                .query_row("SELECT COUNT(*) FROM pending_operations", [], |row| {
                    row.get(0)
                })?;
        Ok(usize::try_from(count).unwrap_or(usize::MAX))
    }

    /// Removes acknowledged operation IDs.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when `SQLite` cannot delete the rows.
    pub fn remove(&self, ids: &[OperationId]) -> Result<(), StorageError> {
        let transaction = self.connection.unchecked_transaction()?;
        for id in ids {
            transaction.execute(
                "DELETE FROM pending_operations WHERE operation_id = ?1",
                params![id.to_string()],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}
