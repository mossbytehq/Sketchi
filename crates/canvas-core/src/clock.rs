//! Lamport logical clock.

use serde::{Deserialize, Serialize};

/// Logical timestamp used to order concurrent operations deterministically.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct LamportTimestamp(u64);

impl LamportTimestamp {
    /// Creates a timestamp.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric timestamp.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Monotonic logical clock owned by a CRDT replica.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LamportClock {
    current: LamportTimestamp,
}

impl LamportClock {
    /// Creates a clock at zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current: LamportTimestamp::new(0),
        }
    }

    /// Returns the current timestamp without advancing it.
    #[must_use]
    pub const fn current(self) -> LamportTimestamp {
        self.current
    }

    /// Advances and returns the next local timestamp.
    #[must_use]
    pub fn tick(&mut self) -> LamportTimestamp {
        self.current = LamportTimestamp::new(self.current.value().saturating_add(1));
        self.current
    }

    /// Observes a remote timestamp without consuming a local tick.
    pub fn observe(&mut self, remote: LamportTimestamp) {
        self.current = LamportTimestamp::new(self.current.value().max(remote.value()));
    }

    pub(crate) const fn from_timestamp(current: LamportTimestamp) -> Self {
        Self { current }
    }
}
