//! Durable local operation journal.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use canvas_core::Document;
use canvas_core::{Operation, OperationId};
use directories::BaseDirs;
use rusqlite::{Connection, OptionalExtension, params};
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
    /// A saved document could not be decoded as JSON.
    #[error("document JSON error: {0}")]
    DocumentJson(#[source] serde_json::Error),
    /// An operation ID was reused with a different operation payload.
    #[error("operation ID {operation_id} was reused with different content")]
    OperationIdReuse {
        /// Reused operation identity.
        operation_id: OperationId,
    },
    /// Document file could not be read or written.
    #[error("document file error: {0}")]
    DocumentIo(#[source] io::Error),
    /// Sync-journal directory could not be created.
    #[error("sync journal directory error: {0}")]
    JournalIo(#[source] io::Error),
}

const DOCUMENT_FILE_NAME: &str = "sketchi.autosave.json";
const JOURNAL_FILE_NAME: &str = "sketchi.sync.sqlite3";

fn configured_directory(directory: &str) -> PathBuf {
    let directory = directory.trim();
    if let Some(relative) = directory.strip_prefix("~/")
        && let Some(base_dirs) = BaseDirs::new()
    {
        return base_dirs.home_dir().join(relative);
    }
    PathBuf::from(if directory.is_empty() {
        "autosave"
    } else {
        directory
    })
}

fn document_path(directory: &str) -> PathBuf {
    configured_directory(directory).join(DOCUMENT_FILE_NAME)
}

/// Returns the durable sync-journal path for the configured data directory.
#[must_use]
pub fn journal_path(directory: &str) -> PathBuf {
    configured_directory(directory).join(JOURNAL_FILE_NAME)
}

#[cfg(windows)]
fn document_backup_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(".{DOCUMENT_FILE_NAME}.bak"))
}

/// Loads the last locally saved materialized document, when present.
///
/// # Errors
///
/// Returns [`StorageError`] when the file cannot be read or decoded.
pub fn load_document(directory: &str) -> Result<Option<Document>, StorageError> {
    let path = document_path(directory);
    #[cfg(windows)]
    let path = if path.exists() {
        path
    } else {
        document_backup_path(&path)
    };
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(StorageError::DocumentIo(error)),
    };
    Ok(Some(
        serde_json::from_slice(&bytes).map_err(StorageError::DocumentJson)?,
    ))
}

/// Loads a materialized document from an explicit JSON file path.
///
/// # Errors
///
/// Returns [`StorageError`] when the file cannot be read or decoded.
pub fn load_document_from_path(path: impl AsRef<Path>) -> Result<Document, StorageError> {
    let bytes = fs::read(path).map_err(StorageError::DocumentIo)?;
    serde_json::from_slice(&bytes).map_err(StorageError::DocumentJson)
}

/// Saves a materialized document in the configured local directory.
///
/// # Errors
///
/// Returns [`StorageError`] when the directory cannot be created or the file
/// cannot be encoded or written.
pub fn save_document(directory: &str, document: &Document) -> Result<PathBuf, StorageError> {
    let path = document_path(directory);
    let Some(parent) = path.parent() else {
        return Err(StorageError::DocumentIo(io::Error::new(
            io::ErrorKind::InvalidInput,
            "document path has no parent directory",
        )));
    };
    fs::create_dir_all(parent).map_err(StorageError::DocumentIo)?;
    let bytes = serde_json::to_vec_pretty(document)?;
    let temporary_path = parent.join(format!(".{DOCUMENT_FILE_NAME}.{}.tmp", std::process::id()));
    fs::write(&temporary_path, bytes).map_err(StorageError::DocumentIo)?;
    #[cfg(windows)]
    if path.exists() {
        let backup_path = document_backup_path(&path);
        if backup_path.exists() {
            fs::remove_file(&backup_path).map_err(StorageError::DocumentIo)?;
        }
        if let Err(error) = fs::rename(&path, &backup_path) {
            let _ = fs::remove_file(&temporary_path);
            return Err(StorageError::DocumentIo(error));
        }
        if let Err(error) = fs::rename(&temporary_path, &path) {
            let _ = fs::rename(&backup_path, &path);
            let _ = fs::remove_file(&temporary_path);
            return Err(StorageError::DocumentIo(error));
        }
        let _ = fs::remove_file(&backup_path);
        return Ok(path);
    }
    if let Err(error) = fs::rename(&temporary_path, &path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(StorageError::DocumentIo(error));
    }
    Ok(path)
}

/// Opens the durable sync journal below the configured data directory.
///
/// The parent directory is created when necessary, so collaboration startup
/// does not depend on a prior autosave.
///
/// # Errors
///
/// Returns [`StorageError::JournalIo`] when the directory cannot be created or
/// [`StorageError::Sqlite`] when the journal cannot be opened.
pub fn open_journal(directory: &str) -> Result<Journal, StorageError> {
    let path = journal_path(directory);
    let Some(parent) = path.parent() else {
        return Err(StorageError::JournalIo(io::Error::new(
            io::ErrorKind::InvalidInput,
            "journal path has no parent directory",
        )));
    };
    fs::create_dir_all(parent).map_err(StorageError::JournalIo)?;
    Journal::open(path)
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
                Ok::<_, StorageError>((
                    operation.id,
                    operation.id.to_string(),
                    serde_json::to_string(operation)?,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let transaction = self.connection.unchecked_transaction()?;
        for (operation_id, operation_id_text, payload) in encoded {
            let existing = transaction
                .query_row(
                    "SELECT payload FROM pending_operations WHERE operation_id = ?1",
                    params![operation_id_text],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            match existing {
                Some(existing) if existing != payload => {
                    return Err(StorageError::OperationIdReuse { operation_id });
                }
                Some(_) => {}
                None => {
                    transaction.execute(
                        "INSERT INTO pending_operations (operation_id, payload) VALUES (?1, ?2)",
                        params![operation_id_text, payload],
                    )?;
                }
            }
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
