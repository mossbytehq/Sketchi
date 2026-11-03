# Development

Sketchi is a Rust workspace for Windows and Linux. Install
[rustup](https://rustup.rs/); `rust-toolchain.toml` pins the toolchain and
components, and `Cargo.toml` declares the workspace's minimum Rust version.
On Windows, install the MSVC C++ build tools and Windows SDK as well.

## Build and run

```sh
git clone https://github.com/mossbytehq/Sketchi.git
cd Sketchi
cargo sketchi
```

On Linux, install the native development packages used by CI before building.
For Debian or Ubuntu:

```sh
sudo apt-get update
sudo apt-get install --no-install-recommends -y \
  libasound2-dev libfontconfig1-dev libudev-dev libwayland-dev \
  libx11-dev libxkbcommon-dev pkg-config
```

The package list is maintained in
[the Linux dependency action](.github/actions/install-linux-deps/action.yml).
Other distributions need the equivalent development packages.

### Useful commands

| Command | Purpose |
| --- | --- |
| `cargo sketchi` | Build and start the desktop app. |
| `cargo sketchi-release` | Build and start the optimized desktop app. |
| `cargo run --locked --package canvas-server --bin sketchi-server` | Run the collaboration server. |
| `cargo fmt --all --check` | Check Rust formatting. |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Run the workspace lint gate. |
| `cargo test --workspace --all-targets --locked` | Run workspace tests. |
| `cargo xtask --help` | List packaging and release commands. |

Set `RUST_LOG=info` to see startup and runtime diagnostics. In PowerShell,
set it with `$env:RUST_LOG = "info"` before running the app.

## Validation and contributions

Before opening a pull request, run:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

CI checks Windows and Linux and also builds the workspace in release mode.
The Security workflow audits `Cargo.lock` on changes and on a weekly schedule;
findings appear in Actions logs and job summaries.

Keep changes focused, add regression coverage when behavior changes, and
include the operating system, version or commit, reproduction steps, and
relevant diagnostics when reporting a bug. Remove private data from reports.

Git normalizes text files to LF through `.gitattributes` so Windows and Linux
use the same line endings. New pull requests request a Mossbyte review through
[CODEOWNERS](.github/CODEOWNERS).

## Project layout

```text
crates/canvas-core       document model and operation-based CRDT
crates/canvas-protocol   versioned collaboration messages
crates/canvas-renderer   camera and geometry/rendering support
apps/canvas-client      Sketchi desktop app
bin/canvas-server       collaboration server
xtask                   packaging and release checks
packaging/              installer definitions and package assets
.github/workflows/      CI, artifact builds, security audits, and releases
```

Keep document mutations in `CrdtDocument::apply`, wire messages in
`canvas-protocol`, and rendering separate from collaboration and persistence.
`canvas-core` must stay independent of runtime and platform APIs. Preserve
deterministic CRDT behavior and snapshot compatibility.

## Hosting a standalone server

Run the server on a machine that every collaborator can reach, such as a VPS.
It needs a TLS certificate and private key in PEM format:

```sh
sketchi-server --bind 0.0.0.0:3210 --certificate cert.pem --private-key key.pem
```

On Windows, the installed server is `Sketchi-server.exe`, beside the app.
When creating or joining a room, open **Advanced** and enter the server's
`wss://<host>:3210/ws` address and the SHA-256 fingerprint of its certificate.
Allow the chosen port through the host's firewall and keep the private key
private.

## Versions and releases

From the repository root, replace `<version>` with a stable version such as
`0.4.9`:

```sh
cargo xtask set-version --workspace <version>
cargo xtask version-check
```

Commit both `Cargo.toml` and `Cargo.lock`. The version command updates the
manifest and synchronizes workspace package entries in the lockfile offline,
preserving locked dependency versions. If synchronization fails, it restores
both files and reports the error.

The optional `set-version` flags also edit an **existing** GitHub release
and require an authenticated GitHub CLI:

| Flag | Effect on the existing release |
| --- | --- |
| `--d` | Make it a draft. |
| `--rc` | Publish it as a pre-release. |
| `--r` | Publish it as the latest stable release. |

These flags do not build or upload artifacts. For a new release, push the
version change, then run **Actions → Release → Run workflow**, selecting the
branch or commit to build. The workflow builds Windows and Linux artifacts,
checks the packages, generates `SHA256SUMS`, and publishes the completed
release. A version containing `-` is marked as a pre-release by the workflow.

Published tags must point to the selected commit. If a version has already
been tagged at another commit, bump the version rather than reusing the tag.
CI and artifact builds share per-platform dependency caches; workspace crates
still rebuild, including the client that embeds its matching server.
