# Sketchi Collaborative Whiteboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Each task ends with its own verification gate.

**Goal:** Build a Linux/Windows Rust desktop collaborative whiteboard with a shared operation-based CRDT, wgpu rendering, a TLS WebSocket server, SQLite durability, local-first reconnect, and reproducible package/release automation.

**Architecture:** `canvas-core` is pure document/CRDT logic. `canvas-protocol` is a versioned JSON contract. `canvas-renderer` owns wgpu rendering. `canvas-client` owns winit/egui input and local persistence. `canvas-server` owns rooms, WebSockets, TLS, and SQLite. The client and server apply the same core operations.

**Tech Stack:** Rust 1.97.1, edition 2024, Cargo resolver 2, winit, wgpu, egui, tokio, axum, WebSockets, serde/serde_json, uuid, glam, thiserror, tracing, rustls, rusqlite bundled SQLite, cargo-deb, WiX/cargo-wix, AppImage tooling, GitHub Actions.

## Global Constraints

- The project lives at the Sketchi repository root; `yerd/` is read-only reference material and is not a workspace member.
- Technical package names remain `canvas-core`, `canvas-protocol`, `canvas-renderer`, `canvas-client`, and `canvas-server`; public binaries/packages use Sketchi branding.
- `canvas-core` has no graphics, UI, async, networking, filesystem, process, or persistence dependencies.
- Every document mutation is represented by an operation and applied through one CRDT path.
- Operations are idempotent; same-property conflicts use `(LamportTimestamp, OperationId)` ordering; deletion wins permanently; tombstones are not garbage-collected.
- Protocol payloads are versioned JSON with size and field validation; presence is ephemeral and never persisted.
- TLS is required for non-loopback standalone servers; local supervised servers use pinned loopback certificates.
- No web, WASM, mobile, macOS, accounts, Redis, PostgreSQL, Kubernetes, or generic CRDT library work is included.
- MIT is the initial license default and must be reviewed before the first public release.
- All new behavior follows test-first development: write a focused failing test, verify RED, implement the minimum GREEN behavior, then refactor while green.

---

### Task 1: Repository, project skills, and workspace foundation

**Files:**
- Create: `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/config.toml`, `.gitignore`, `LICENSE.md`, `README.md`
- Create: `.agents/skills/sketchi-core-rust/SKILL.md` and `agents/openai.yaml`
- Create: `.agents/skills/sketchi-release-engineering/SKILL.md` and `agents/openai.yaml`
- Create: `docs/architecture.md`, `docs/development.md`, `docs/crdt.md`, `docs/protocol.md`, `docs/packaging.md`
- Create: `xtask/Cargo.toml`, `xtask/src/main.rs`
- Create: `crates/canvas-core/Cargo.toml`, `crates/canvas-protocol/Cargo.toml`, `crates/canvas-renderer/Cargo.toml`
- Create: `apps/canvas-client/Cargo.toml`, `bin/canvas-server/Cargo.toml`

**Interfaces:** Establish the workspace package names, shared dependency table, lints, version `0.1.0`, and `cargo xtask` alias. The server package must expose both a library and `sketchi-server` binary; the client package must expose the `sketchi` binary.

- [ ] Initialize the root Git repository and establish the project-local skill directories using the provided skill initializer.
- [ ] Run baseline pressure/forward tests for each skill, write the minimal guidance, and run the validator.
- [ ] Add the virtual Cargo workspace and empty crate roots with documented purity boundaries.
- [ ] Add developer documentation describing the dependency graph, operation-first mutation rule, and four delivery slices.
- [ ] Run `cargo metadata --no-deps`, `cargo fmt --all --check`, and `cargo test --workspace`.
- [ ] Commit the foundation as `chore: bootstrap Sketchi workspace`.

### Task 2: Core document model, operations, and CRDT

**Files:**
- Create: `crates/canvas-core/src/{lib.rs,error.rs,ids.rs,geometry.rs,element.rs,document.rs,clock.rs,version_vector.rs,operation.rs,crdt.rs,command.rs}`
- Create: `crates/canvas-core/tests/{operations.rs,convergence.rs,snapshot.rs,no_runtime_deps.rs}`

**Interfaces:** Implement `ClientId`, `ElementId`, `OperationId`, `LamportTimestamp`, `VersionVector`, `Document`, `Element`, `ElementKind`, `Transform`, `Style`, `Operation`, `OperationKind`, `CrdtDocument::apply`, `snapshot`, and `from_snapshot`.

- [ ] Write failing tests for sequential operations, duplicate operations, out-of-order operations, register conflicts, delete-wins behavior, and snapshot round trips.
- [ ] Write a seeded multi-client convergence property test that delivers identical operation sets in different orders.
- [ ] Implement the minimum model and CRDT state required by the failing tests.
- [ ] Add validation for finite geometry, bounded text/points, operation-ID reuse, and canonical deterministic snapshots.
- [ ] Run the focused core tests, then the complete workspace test/lint gate.
- [ ] Commit as `feat: add canvas core CRDT`.

### Task 3: Versioned JSON protocol

**Files:**
- Create: `crates/canvas-protocol/src/{lib.rs,error.rs,message.rs,validation.rs}`
- Create: `crates/canvas-protocol/tests/{wire.rs,compatibility.rs,no_runtime_deps.rs}`
- Modify: `docs/protocol.md`

**Interfaces:** Add `ClientMessage` and `ServerMessage` variants for Hello, room creation/join, sync, operations, acknowledgements, presence, lifecycle, ping/pong, credentials, snapshots, and structured errors.

- [ ] Write failing JSON round-trip and golden-wire tests.
- [ ] Implement tagged versioned messages using shared core types.
- [ ] Add protocol version checks, bounded payload validation, and token/operation identity validation.
- [ ] Verify the protocol crate remains synchronous and runtime-free.
- [ ] Commit as `feat: define collaboration protocol`.

### Task 4: Renderer and local desktop editor

**Files:**
- Create: `crates/canvas-renderer/src/{lib.rs,error.rs,camera.rs,pipelines.rs,geometry.rs,text.rs,selection.rs}`
- Create: `apps/canvas-client/src/{main.rs,app.rs,input.rs,tools.rs,editor.rs,connection.rs,storage.rs,supervisor.rs}`
- Create: renderer camera/geometry tests and client tool/editor tests

**Interfaces:** `Camera::{world_to_screen,screen_to_world}`, renderer draw entrypoint over `Document`, tool-to-command conversion, local operation factory, and bounded event channels between winit and tokio tasks.

- [ ] Write failing camera, hit-test, tool, freehand, and local mutation tests before UI code.
- [ ] Implement the winit/wgpu/egui shell and keep the UI event loop non-blocking.
- [ ] Implement camera, pan/zoom, shape rendering, text, selection, tools, freehand pointer-up commit, and local undo/redo as inverse operations.
- [ ] Add client identity and local SQLite journal scaffolding without network replay yet.
- [ ] Run headless tests and compile/build checks on Linux; keep GPU smoke checks optional when no adapter is available.
- [ ] Commit as `feat: add local whiteboard editor`.

### Task 5: Server, rooms, TLS, and persistence

**Files:**
- Create: `bin/canvas-server/src/{lib.rs,main.rs,config.rs,auth.rs,room.rs,actor.rs,websocket.rs,tls.rs,store.rs,error.rs}`
- Create: `bin/canvas-server/migrations/001_initial.sql`
- Create: server unit/integration tests and temporary SQLite fixtures

**Interfaces:** `ServerConfig`, `RoomStore`, room actor commands, readiness/health endpoints, WebSocket session handling, and `sketchi-server --check-config`/`--version` commands.

- [ ] Write failing room actor, token, SQLite migration, TLS startup, and two-client WebSocket tests.
- [ ] Implement capability-token room creation/join, token hashing, operation validation, durable commit-before-ack, broadcast, presence, and graceful shutdown.
- [ ] Implement full CRDT snapshots, operation-log replay, and snapshot scheduling at 500 operations or 30 seconds.
- [ ] Implement local supervised-server readiness JSON and loopback certificate pinning.
- [ ] Run server integration tests and commit as `feat: add collaboration server`.

### Task 6: Synchronization, reconnect, presence, and live strokes

**Files:**
- Modify: `apps/canvas-client/src/{connection.rs,storage.rs,supervisor.rs,app.rs}`
- Modify: `crates/canvas-protocol/src/message.rs`
- Modify: `bin/canvas-server/src/{actor.rs,websocket.rs}`

- [ ] Write failing reconnect, journal-replay, acknowledgement-retry, presence-throttle, and live-stroke tests.
- [ ] Implement snapshot-plus-delta sync, durable pending-operation replay, retry-safe acknowledgements, and temporary server-loss behavior.
- [ ] Implement cursor/selection/tool presence at a bounded update rate without persistence.
- [ ] Add `StrokeStart`, `StrokeChunk`, and `StrokeEnd` as ephemeral messages; finalize the durable freehand operation on stroke end.
- [ ] Run two-client and restart/reconnect integration tests.
- [ ] Commit as `feat: add local-first synchronization`.

### Task 7: CI, packaging, and release automation

**Files:**
- Create: `.github/actions/install-linux-deps/action.yml`
- Create: `.github/workflows/{ci.yml,security.yml,build.yml,release.yml}`
- Create: `packaging/linux/{AppDir,desktop,README.md}` and `packaging/windows/main.wxs`
- Modify: `xtask/src/main.rs`, package manifests, `docs/packaging.md`

- [ ] Write package-staging and artifact-name tests before adding release commands.
- [ ] Implement Linux x86_64 and Windows x86_64 CI gates with Cargo caching and locked builds.
- [ ] Stage client plus server sidecar for AppImage, `.deb`, MSI, and portable archives.
- [ ] Add cargo-deb, WiX, AppImage, checksum, install-smoke, version-check, and draft-release jobs.
- [ ] Verify installed client/server binaries and local-room startup in package smoke tests.
- [ ] Commit as `ci: add Sketchi build and release automation`.

### Task 8: Final verification and handoff

- [ ] Run `cargo fmt --all --check`.
- [ ] Run `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- [ ] Run `cargo test --workspace --all-targets --locked`.
- [ ] Run dependency/advisory checks and package staging checks.
- [ ] Perform the two-client MVP acceptance scenario on Linux where available.
- [ ] Review the complete diff against this plan and record any deferred non-blocking items.
- [ ] Commit final documentation and verification evidence.
