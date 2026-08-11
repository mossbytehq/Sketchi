# Linux packaging

`cargo xtask package` creates the deterministic client staging directory with
the `sketchi-server` sidecar, `LICENSE.md`, `VERSION`, and desktop metadata.
The native runner then turns that directory into the final format:

```sh
cargo xtask package --target x86_64-unknown-linux-gnu --format portable
# AppImage: run the pinned appimagetool against packaging/linux/AppDir
# Debian: cargo deb --package canvas-client --locked
# Arch Linux: see packaging/arch/README.md
# RPM: see packaging/rpm/README.md
```

`AppDir/usr/bin/` is populated by CI from the current release build and is
ignored in source control. No prebuilt binary is checked into the repository.
