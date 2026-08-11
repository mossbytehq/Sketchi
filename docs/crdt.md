# Document and CRDT contract

Every durable mutation is an operation with an `OperationId` made from a stable
`ClientId` and a client-local sequence. Operations carry a Lamport timestamp
and the sender's version vector. A replica applies all valid operations through
one `CrdtDocument::apply` path.

Mutable properties use deterministic last-writer-wins registers. The comparison
key is `(LamportTimestamp, OperationId)`, so equal timestamps are still
ordered identically on every replica. Independent properties can merge without
overwriting each other. A delete creates a permanent tombstone; tombstones are
retained for the initial product so a late create or update cannot resurrect an
element.

The application contract is:

```rust
pub fn apply(&mut self, operation: &Operation) -> Result<ApplyResult, CrdtError>;
```

Applying the same operation again is a no-op. Invalid geometry, non-finite
coordinates, oversized text or point lists, operation-ID reuse, and malformed
causal metadata are rejected. A canonical snapshot sorts stable identifiers and
contains the version vector and Lamport state so a restored replica has the
same future ordering behavior.

The primary correctness property is permutation convergence: for one valid set
of operations, every replica must produce the same snapshot regardless of
arrival order. Tests cover sequential, duplicate, out-of-order, concurrent,
delete-wins, snapshot, and seeded randomized multi-client delivery.

