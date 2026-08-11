# Arch Linux package

The `PKGBUILD` packages a verified Sketchi client staging directory, including
the matching `sketchi-server` sidecar, desktop entry, icon, version, and
license. It intentionally does not compile Rust code itself.

Build it after staging a release build:

```sh
SKETCHI_VERSION=0.1.0 \
SKETCHI_STAGE_DIR="$PWD/dist/staging/Sketchi-0.1.0-x86_64-unknown-linux-gnu" \
PKGDEST="$PWD/dist/artifacts" \
BUILDDIR="$PWD/dist/arch-build" \
makepkg --cleanbuild --force --nodeps --skipinteg \
  -D packaging/arch -p PKGBUILD
```

The release workflow runs this in a pinned Arch Linux build container and
verifies the resulting `.pkg.tar.zst` contents before publishing it.
