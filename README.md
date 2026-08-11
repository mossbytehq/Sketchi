# Sketchi

Sketchi is a native Linux and Windows collaborative whiteboard written in
Rust. It uses a small operation-based CRDT so the desktop client and the
collaboration server share the same document semantics.

The repository is deliberately layered:

```text
canvas-client -> canvas-core + canvas-protocol + canvas-renderer
canvas-server -> canvas-core + canvas-protocol
```

## Quick start

The pinned toolchain is Rust 1.97.1. Run commands from the repository root.

Start the desktop client with the Cargo alias:

```sh
cargo sketchi
```

The package name is `canvas-client`, so `cargo run -p sketchi` is not a valid
command. The equivalent explicit command is:

```sh
cargo run --package canvas-client --bin sketchi
```

To start the collaboration server locally:

```sh
cargo run --package canvas-server --bin sketchi-server
```

Set `RUST_LOG=info` when you need startup and runtime diagnostics:

```sh
RUST_LOG=info cargo sketchi
```

On KDE Wayland, Sketchi uses the native Wayland backend. To verify the
backend and image drag-and-drop path while troubleshooting, run:

```sh
env -u WINIT_UNIX_BACKEND WAYLAND_DISPLAY=wayland-0 RUST_LOG='info,wgpu=warn' cargo sketchi
```

The repository carries a small `vendor/winit` patch because the resolved
`winit` release did not forward Wayland `wl_data_device` file drops as
`HoveredFile`/`DroppedFile` events. PNG and JPEG drops are decoded, previewed,
and embedded after the native Wayland event arrives.

## Development checks

Run the normal local gate before committing:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

For a faster client-only check while working on the UI:

```sh
cargo test -p canvas-client --lib --locked --offline
cargo clippy -p canvas-client --all-targets --all-features --locked --offline -- -D warnings
```

## Build cache and generated files

Build output lives in `target/` and is ignored by Git. Incremental compilation
is disabled in `.cargo/config.toml` to reduce persistent disk usage. Use the
provided aliases when you need to reclaim space:

```sh
cargo clean-cache   # remove all Cargo build output
cargo clean-dev     # remove the development profile
cargo clean-release # remove release-profile output
```

Release staging is written under `dist/` and `artifacts/`; Arch and RPM native
packages are built from the verified Linux staging directory; local databases,
logs, environment files, and editor metadata are also ignored. Source files
under `.cargo/`, `.github/`, and `packaging/` are project files and should
remain trackable. `.agents/` is local-only developer skill state and is
intentionally ignored.

## Workspace layout

```text
crates/canvas-core       document model and operation-based CRDT
crates/canvas-protocol   versioned collaboration messages
crates/canvas-renderer   camera and geometry/rendering support
apps/canvas-client       native Sketchi desktop client
bin/canvas-server        collaboration server
xtask                    packaging and release checks
```

The `cargo xtask` alias owns reproducible staging, package smoke tests, and
release checks. See [docs/development.md](docs/development.md) and
[docs/packaging.md](docs/packaging.md) for the delivery workflow.

Sketchi is initially licensed under MIT; that choice should be reviewed before
the first public release.
