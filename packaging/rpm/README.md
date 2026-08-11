# RPM package

The RPM spec packages a verified Sketchi client staging directory, including
the matching `sketchi-server` sidecar, desktop entry, icon, version, and
license. It intentionally does not compile Rust code itself.

Build it after staging a release build:

```sh
rpm_top="$PWD/dist/rpmbuild"
mkdir -p "$rpm_top"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
rpmbuild -bb packaging/rpm/sketchi.spec \
  --define "_topdir $rpm_top" \
  --define "_sketchi_version 0.1.0" \
  --define "_sketchi_stage $PWD/dist/staging/Sketchi-0.1.0-x86_64-unknown-linux-gnu"
```

The release workflow verifies the resulting `.rpm` file list before publishing
it.
