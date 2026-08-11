# Development guide

## Toolchain

The repository pins Rust 1.97.1 in `rust-toolchain.toml`. Run commands from
the Sketchi root; `yerd/` is a separate repository and is not a workspace
member.

The normal local gate is:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

When dependencies or package metadata change, regenerate and inspect
`Cargo.lock`, then keep it committed. CI always uses `--locked`.

## Change order

Document behavior starts with a test in the lowest affected layer. Verify the
test fails for the missing behavior, implement the smallest change, then run
the focused test and the workspace gate. A renderer, client, or server must
not bypass `canvas-core` to mutate a document.

The `cargo xtask` alias is the home for repository-owned checks and release
staging. It should remain deterministic and should not hide platform-specific
tool requirements.

## Native Wayland image drops

The desktop client keeps native Wayland enabled on KDE. The resolved `winit`
release does not currently forward `wl_data_device` file drops, so
`vendor/winit` contains the small backend bridge that turns Wayland URI-list
offers into the standard `HoveredFile` and `DroppedFile` events consumed by
egui. Keep this patch when updating the lockfile, and verify it with:

```sh
env -u WINIT_UNIX_BACKEND WAYLAND_DISPLAY=wayland-0 RUST_LOG='info,wgpu=warn' cargo sketchi
```

The log should show a native file-hover event, an image preview decode, and a
native file-drop event when a PNG or JPEG is dragged onto the canvas.

## Boundaries

- Keep `canvas-core` synchronous and runtime-free.
- Keep protocol DTOs versioned and bounded; reuse core operations.
- Keep presence out of snapshots and SQLite operation logs.
- Keep rendering independent from transport and persistence.
- Use capability tokens rather than adding an account system.
- Require TLS for non-loopback standalone server endpoints; loopback development
  uses a pinned local certificate.

See [architecture.md](architecture.md), [crdt.md](crdt.md), and
[protocol.md](protocol.md) before changing a cross-layer interface.
