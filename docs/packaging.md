# Packaging and releases

The public product is branded Sketchi while the technical Cargo packages stay
`canvas-core`, `canvas-protocol`, `canvas-renderer`, `canvas-client`, and
`canvas-server`. The desktop binary is `sketchi`; the server binary is
`sketchi-server`.

Every client artifact includes the matching server sidecar so a user can open
a local room without downloading another package. Server-only archives are
also published.

## Release artifacts

| Platform | Client artifacts | Server artifact |
| --- | --- | --- |
| Linux x86_64 | AppImage, `.deb`, Arch `.pkg.tar.zst`, RPM `.rpm`, portable `.tar.gz` | `.tar.gz` |
| Windows x86_64 | MSI, portable `.zip` | `.zip` |

CI builds with the committed lockfile and stages files from the current build.
`cargo xtask` owns version checks, deterministic staging, package-content
checks, install smoke tests, and SHA-256 manifest generation:

```sh
cargo xtask version-check --tag v0.1.0
cargo xtask package --target x86_64-unknown-linux-gnu --format portable
cargo xtask package-server --target x86_64-unknown-linux-gnu
cargo xtask artifact-check --path dist/staging/Sketchi-0.1.0-x86_64-unknown-linux-gnu \
  --target x86_64-unknown-linux-gnu --kind client
cargo xtask checksums --input-dir dist
```

The staging command never invents a package: it verifies that the current
release binaries exist and includes the client, matching `Sketchi-server`
sidecar, `LICENSE.md`, `VERSION`, and Linux desktop metadata. The runner then
uses the matching native tool to create the final artifact:

| Format | Native tool | Packaging source |
| --- | --- | --- |
| AppImage | pinned `appimagetool` | `packaging/linux/AppDir` |
| Debian | `cargo-deb` | `apps/canvas-client/Cargo.toml` |
| Arch Linux | `makepkg` | `packaging/arch/PKGBUILD` |
| RPM | `rpmbuild` | `packaging/rpm/sketchi.spec` |
| Windows MSI | WiX/cargo-wix | `packaging/windows/main.wxs` |

A format is not advertised until its tool produces an artifact and the
matching runner verifies its package contents. Arch and RPM builds package the
same verified Linux staging directory, so they include the client and matching
server sidecar rather than compiling a second, different binary.

The release workflow validates the tag against workspace metadata, builds both
binaries, checks versions and package contents, generates checksums, and
creates a draft release. Local-room readiness is exercised by the server
smoke path where a native runner is available. Signing is conditional on
configured credentials and never bypasses the test gates.

The Windows WiX source is [packaging/windows/main.wxs](../packaging/windows/main.wxs);
its `ClientBinary`, `ServerBinary`, `LicenseFile`, `IconFile`, and `Version`
variables are supplied by the Windows runner. `packaging/linux/AppDir` is a
source template; CI copies the current binaries into its ignored `usr/bin`
directory before running AppImage tooling. The Linux desktop entry and icon
use the `sketchi` application ID so KDE can associate the running window with
the installed taskbar icon.
