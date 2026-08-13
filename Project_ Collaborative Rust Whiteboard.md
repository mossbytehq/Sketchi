# Project: Collaborative Rust Whiteboard

Build a native desktop collaborative whiteboard application in Rust.

The initial platforms are **Linux and Windows**. Do not prioritize web, macOS, mobile, or WASM.

The application should eventually feel similar to Excalidraw, but its core differentiator is real-time multiplayer drawing: multiple users can draw and manipulate the same canvas simultaneously, with live cursors, selections, and eventually live freehand strokes.

## Core architecture

Use a Cargo workspace with these crates:

- `canvas-core`
- `canvas-protocol`
- `canvas-renderer`
- `canvas-client`
- `canvas-server`

Keep `canvas-core` independent of graphics, networking, UI frameworks, and persistence.

The architecture must be:

    Input
      ↓
    Editor Command
      ↓
    Operation
      ↓
    CRDT
      ↓
    Document
      ↓
    Renderer

Networking should operate alongside the CRDT:

    Local operation
      ├── apply immediately to local CRDT
      └── send to server

    Remote operation
      └── apply to local CRDT

The same `canvas-core` CRDT implementation must be usable by both client and server.

## Technology

Use:

- Rust stable
- Cargo workspace
- winit for desktop window/input
- wgpu for rendering
- tokio for async work
- axum for the collaboration server
- WebSockets for initial networking
- serde for serialization
- JSON for the initial protocol
- uuid for IDs
- glam for geometry/math
- thiserror for library errors
- tracing for logging
- SQLite for initial persistence

Do not prematurely optimize the network protocol into a custom binary format.

Do not introduce Redis, Kubernetes, PostgreSQL, or cloud infrastructure during the initial MVP.

## Document model

Create:

    Document
    Element
    ElementKind
    Transform
    Style

Element kinds initially:

    Rectangle
    Ellipse
    Line
    Arrow
    Text
    Freehand

Every element has a stable `ElementId`.

Every client has a stable `ClientId`.

Operations have:

    OperationId {
        client_id,
        sequence
    }

Use Lamport timestamps for deterministic ordering.

Use a version vector to track causal knowledge.

## CRDT model

Use a small domain-specific operation-based CRDT.

Do not use a generic CRDT library initially.

Mutable element properties should use LWW semantics.

For example:

    x
    y
    width
    height
    rotation
    stroke
    fill
    stroke_width
    text

must be independently mergeable where practical.

Define operations such as:

    Create
    Delete
    SetPosition
    SetSize
    SetRotation
    SetStyle
    SetText
    SetPoints
    Reorder

Operations must be idempotent.

Receiving the same operation twice must not modify the document twice.

All conflict resolution must be deterministic.

Given the same valid set of operations, every replica must produce exactly the same final document regardless of operation arrival order.

Do not implement tombstone garbage collection yet.

## CRDT testing

Before implementing networking or the full renderer, write extensive tests.

Test:

1. Sequential operations.
2. Duplicate operations.
3. Operations arriving out of order.
4. Concurrent modifications to different properties.
5. Concurrent modifications to the same property.
6. Concurrent creation and deletion.
7. Multiple simulated clients.
8. Randomized operation sequences.

Create a convergence test that generates operations for multiple clients, delivers them in different orders, and asserts that every client ends with exactly the same document.

This convergence property is one of the most important requirements of the project.

## Renderer

Use wgpu.

The renderer must only depend on the document state and rendering-specific data.

It must not know about WebSockets, CRDT internals, server rooms, or persistence.

Implement:

- camera
- zoom
- pan
- world-to-screen conversion
- screen-to-world conversion
- rectangle rendering
- ellipse rendering
- line rendering
- arrow rendering
- freehand rendering
- selection rendering

Keep rendering architecture modular so additional element types can be added later.

## Input/editor architecture

Do not mutate the document directly from mouse events.

Use tools and editor commands.

Example:

    MouseDown
      ↓
    RectangleTool
      ↓
    EditorCommand::CreateRectangle
      ↓
    Operation::Create
      ↓
    CRDT
      ↓
    Renderer

Initial tools:

- Select
- Rectangle
- Ellipse
- Line
- Arrow
- Freehand
- Pan

Implement local editing before networking.

## Networking

Use WebSockets through axum.

Define a shared protocol crate.

Client messages initially include:

    Hello
    JoinRoom
    SubmitOperations
    RequestSync
    Presence
    Ping

Server messages initially include:

    Welcome
    Snapshot
    Operations
    Ack
    Presence
    UserJoined
    UserLeft
    Pong

A new client should:

1. Connect.
2. Send Hello.
3. Join a room.
4. Receive a snapshot and version vector.
5. Begin receiving operations.
6. Submit local operations.
7. Receive acknowledgements.
8. Receive remote operations.
9. Apply remote operations through the same CRDT engine.

## Server

A room should contain:

    RoomId
    CrdtDocument
    connected clients
    operation log

The server should:

1. Validate operations.
2. Apply operations to the room CRDT.
3. Persist durable operations.
4. Broadcast operations to connected clients.
5. Send snapshots to newly joined clients.
6. Track ephemeral presence separately.

The server should not contain a second implementation of drawing semantics.

## Presence

Presence is separate from durable document state.

Presence should contain:

    ClientId
    cursor position
    selected elements
    active tool

Presence should not be persisted.

Throttle cursor updates rather than sending every raw mouse event.

## Freehand drawing

Initially:

    pointer down
      ↓
    collect points locally
      ↓
    pointer up
      ↓
    create one durable Freehand operation

After the basic collaboration system works, add live stroke streaming.

Live stroke streaming should use ephemeral messages such as:

    StrokeStart
    StrokeChunk
    StrokeEnd

The final stroke should become durable document state.

## Persistence

Initially use SQLite.

Persist:

- documents
- operation log
- snapshots

Do not reconstruct a large document from every historical operation on every connection.

Periodically create document snapshots.

A new client should receive:

    latest snapshot
      +
    operations after snapshot

## Local-first direction

Design the client so that local edits are applied immediately without waiting for the server.

Eventually support:

    local operation
      ↓
    local persistence
      ↓
    network queue
      ↓
    server synchronization

The application should be able to continue editing while temporarily disconnected.

Full offline synchronization can be implemented after the basic multiplayer MVP works.

## Platform requirements

The initial release must run on:

- Linux x86_64
- Windows x86_64

Keep platform-specific code isolated.

Do not make Linux or Windows assumptions inside `canvas-core`.

Set up CI for both platforms once the project builds.

## Development order

Implement in this exact general order:

### Milestone 1
Workspace and crate structure.

### Milestone 2
`canvas-core` IDs, geometry, elements, document model.

### Milestone 3
Lamport clocks, version vectors, operations, CRDT.

### Milestone 4
Extensive CRDT and convergence tests.

Do not proceed until convergence tests are reliable.

### Milestone 5
Basic winit + wgpu application.

### Milestone 6
Camera, pan, zoom, and coordinate conversion.

### Milestone 7
Rectangle, ellipse, line, arrow rendering.

### Milestone 8
Selection and editing tools.

### Milestone 9
Freehand drawing.

### Milestone 10
Local undo/redo.

### Milestone 11
WebSocket server and room model.

### Milestone 12
Two-client synchronization.

### Milestone 13
Presence and live cursors.

### Milestone 14
Concurrent editing testing.

### Milestone 15
Persistence and snapshots.

### Milestone 16
Reconnection.

### Milestone 17
Live freehand stroke streaming.

### Milestone 18
Linux and Windows packaging/CI.

## Engineering rules

Prefer simple explicit Rust code over clever abstractions.

Do not add dependencies unless they solve a concrete problem.

Do not prematurely optimize.

Do not mix rendering, input, CRDT, and networking logic.

Do not bypass `canvas-core` to modify document state.

Every new document mutation should be represented by an operation.

Every remote operation must pass through the same CRDT application path as local operations.

Write tests for important synchronization behavior.

Keep public APIs small.

Document architectural decisions in `docs/architecture.md`, `docs/crdt.md`, and `docs/protocol.md`.

At each milestone, make the project compile and test successfully before moving on.

When making architectural changes, update the relevant documentation.

## Definition of success for the first MVP

A user should be able to launch the application on Linux or Windows, create a room, have two desktop clients join it, draw shapes, move/resize/delete them, see the other user's cursor and selection, and observe changes appear on both clients in real time.

Two clients making concurrent edits must eventually converge to exactly the same document state.

The application should remain usable locally even when the server connection temporarily disappears.