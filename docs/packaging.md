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
