# Sketchi architecture

Sketchi is split into layers with one direction of ownership:

```text
                 +-------------------+
                 |   canvas-client   |
                 | input, local-first|
                 +---------+---------+
                           |
                 +---------v---------+
                 |  canvas-renderer  |
                 | wgpu presentation |
                 +---------+---------+
                           |
                 +---------v---------+
                 |    canvas-core    |
                 | document + CRDT   |
                 +---------+---------+
                           ^
                 +---------+---------+
                 |  canvas-protocol  |
                 | versioned JSON    |
                 +---------+---------+
                           ^
                 +---------+---------+
                 |   canvas-server   |
                 | rooms + transport |
                 +-------------------+
```

`canvas-core` is the semantic boundary. It contains stable identifiers,
geometry, elements, operations, Lamport ordering, version vectors, validation,
and the operation-based CRDT. It does not depend on a runtime, UI, renderer,
network, filesystem, process supervisor, or database.

`canvas-protocol` serializes the core operations and session messages. It is a
wire contract, not a second document model. Presence and live strokes are
ephemeral protocol messages and never become durable document state.

`canvas-renderer` turns a document snapshot plus presentation state into GPU
draw calls. It has no knowledge of rooms or synchronization. The client maps
input to editor commands, creates operations, applies them locally, journals
them, and sends them when a connection is available.

The server authenticates capability tokens, validates messages, applies the
same core CRDT, commits durable operations before acknowledging them, and
broadcasts accepted operations. SQLite stores room metadata, operation logs,
and snapshots. A supervised local server uses the same `sketchi-server`
executable as a standalone deployment.

## Delivery slices

1. Establish the workspace and pure core model.
2. Prove deterministic convergence and define the JSON protocol.
3. Add the local desktop editor and renderer.
4. Add server synchronization, persistence, reconnect, and release packaging.

The initial platform scope is Linux x86_64 and Windows x86_64. Web, WASM,
mobile, macOS, accounts, and external infrastructure are intentionally outside
the MVP.

