//! Causal knowledge tracking for operation batches.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    error::CrdtError,
    ids::{ClientId, OperationId},
};

/// Per-client highest operation sequence observed by a replica.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VersionVector {
    entries: BTreeMap<ClientId, u64>,
}

#[derive(Deserialize)]
struct VersionVectorWire {
    entries: Vec<VersionVectorEntry>,
}

#[derive(Deserialize, Serialize)]
struct VersionVectorEntry {
    client_id: ClientId,
    sequence: u64,
}

impl Serialize for VersionVector {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let entries = self
            .entries
            .iter()
            .map(|(&client_id, &sequence)| VersionVectorEntry {
                client_id,
                sequence,
            })
            .collect::<Vec<_>>();
        VersionVectorWireForSerialize { entries }.serialize(serializer)
    }
}

#[derive(Serialize)]
struct VersionVectorWireForSerialize {
    entries: Vec<VersionVectorEntry>,
}

impl<'de> Deserialize<'de> for VersionVector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = VersionVectorWire::deserialize(deserializer)?;
        let mut vector = Self::default();
        for entry in wire.entries {
            if entry.sequence == 0 {
                return Err(serde::de::Error::custom(
                    "version vector sequences must be non-zero",
                ));
            }
            let current = vector.entries.entry(entry.client_id).or_default();
            *current = (*current).max(entry.sequence);
        }
        Ok(vector)
    }
}

impl VersionVector {
    /// Returns the highest sequence known for a client.
    #[must_use]
    pub fn get(&self, client_id: ClientId) -> u64 {
        self.entries.get(&client_id).copied().unwrap_or_default()
    }

    /// Records an operation ID, retaining the greatest sequence.
    pub fn observe(&mut self, operation_id: OperationId) {
        let entry = self.entries.entry(operation_id.client_id).or_default();
        *entry = (*entry).max(operation_id.sequence);
    }

    /// Merges another vector by taking per-client maxima.
    pub fn merge(&mut self, other: &Self) {
        for (&client_id, &sequence) in &other.entries {
            let entry = self.entries.entry(client_id).or_default();
            *entry = (*entry).max(sequence);
        }
    }

    /// Returns whether this vector contains at least the other vector.
    #[must_use]
    pub fn dominates(&self, other: &Self) -> bool {
        other
            .entries
            .iter()
            .all(|(&client_id, &sequence)| self.get(client_id) >= sequence)
    }

    /// Iterates over stable client/sequence entries.
    pub fn entries(&self) -> impl Iterator<Item = (&ClientId, &u64)> {
        self.entries.iter()
    }

    /// Validates serialized causal metadata.
    ///
    /// # Errors
    ///
    /// Returns [`CrdtError::InvalidOperation`](crate::CrdtError::InvalidOperation)
    /// when a stored sequence is zero.
    pub fn validate(&self) -> Result<(), CrdtError> {
        if self.entries.values().any(|sequence| *sequence == 0) {
            Err(CrdtError::InvalidOperation(
                "version vector sequences must be non-zero".to_owned(),
            ))
        } else {
            Ok(())
        }
    }
}
