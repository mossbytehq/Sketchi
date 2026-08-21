# Packaging and releases

Release builds are driven by the reusable workflows under `.github/workflows`
and the commands in `xtask`. Run release commands with `--locked` so the
committed `Cargo.lock` remains the dependency source of truth.

The release workflow builds and smoke-tests the native artifacts, creates a
complete draft release, generates `SHA256SUMS`, and publishes the release only
after those steps succeed. Stable releases include Linux packages and Windows
installers; portable client archives are also produced for supported release
channels.

Before publishing a release, verify the workspace version, artifact contents,
checksums, and the packaged client's local server sidecar readiness handshake.

To change the workspace version, use the repository's Cargo alias:

```sh
cargo set-version --workspace 0.2.0
```

The optional release flags update an existing GitHub release through the
authenticated `gh` CLI while keeping `Cargo.toml` at the exact version given:

```sh
cargo set-version --workspace 0.2.0 --d   # draft
cargo set-version --workspace 0.2.0 --rc  # pre-release
cargo set-version --workspace 0.2.0 --r   # latest stable release
```

These flags edit `v0.2.0`; they do not append `-draft` or `-rc` to the Cargo
version. The GitHub release must already exist, and `gh auth status` must be
valid before using them.
