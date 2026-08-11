//! Stable identifiers used by the document and operation log.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identity for one collaborating client.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct ClientId(Uuid);

impl ClientId {
    /// Creates a random client ID.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates an ID from a UUID.
    #[must_use]
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Creates a deterministic ID useful for fixtures and tests.
    #[must_use]
    pub const fn from_u128(value: u128) -> Self {
        Self(Uuid::from_u128(value))
    }

    /// Returns the underlying UUID.
    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }

    /// Returns whether this is the nil UUID.
    #[must_use]
    pub const fn is_nil(self) -> bool {
        self.0.is_nil()
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Stable identity for a document element.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct ElementId(Uuid);

impl ElementId {
    /// Creates a random element ID.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates an ID from a UUID.
    #[must_use]
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Creates a deterministic ID useful for fixtures and tests.
    #[must_use]
    pub const fn from_u128(value: u128) -> Self {
        Self(Uuid::from_u128(value))
    }

    /// Returns the underlying UUID.
    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }

    /// Returns whether this is the nil UUID.
    #[must_use]
    pub const fn is_nil(self) -> bool {
        self.0.is_nil()
    }
}

impl fmt::Display for ElementId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Client-local sequence identity for one durable operation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OperationId {
    /// Client that created the operation.
    pub client_id: ClientId,
    /// Monotonically increasing sequence owned by the client.
    pub sequence: u64,
}

impl OperationId {
    /// Creates an operation ID.
    #[must_use]
    pub const fn new(client_id: ClientId, sequence: u64) -> Self {
        Self {
            client_id,
            sequence,
        }
    }

    /// Returns the sentinel used only for empty register metadata.
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            client_id: ClientId::from_u128(0),
            sequence: 0,
        }
    }
}

impl fmt::Display for OperationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.client_id, self.sequence)
    }
}
