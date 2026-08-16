Name:           sketchi
Version:        %{_sketchi_version}
Release:        1%{?dist}
Summary:        Collaborative Rust whiteboard
License:        MIT
URL:            https://github.com/mossbytehq/Sketchi
BuildArch:      x86_64

Requires:       wayland
Requires:       libxkbcommon
Requires:       vulkan-loader
Requires:       libX11
Requires:       libxcb
Requires:       libXrandr
Requires:       libXi
Requires:       libXcursor
Requires:       libXinerama

%description
Sketchi is a collaborative whiteboard desktop application with a local
collaboration server sidecar.

%prep

%build

%install
rm -rf %{buildroot}
test -d "%{_sketchi_stage}"
install -Dm755 "%{_sketchi_stage}/Sketchi" \
  "%{buildroot}%{_bindir}/sketchi"
install -Dm755 "%{_sketchi_stage}/Sketchi-server" \
  "%{buildroot}%{_bindir}/sketchi-server"
install -Dm644 "%{_sketchi_stage}/LICENSE.md" \
  "%{buildroot}%{_docdir}/%{name}/LICENSE.md"
install -Dm644 "%{_sketchi_stage}/VERSION" \
  "%{buildroot}%{_datadir}/%{name}/VERSION"
install -Dm644 "%{_sketchi_stage}/share/applications/sketchi.desktop" \
  "%{buildroot}%{_datadir}/applications/sketchi.desktop"
install -Dm644 "%{_sketchi_stage}/share/icons/hicolor/512x512/apps/sketchi.png" \
  "%{buildroot}%{_datadir}/icons/hicolor/512x512/apps/sketchi.png"

%files
%license %{_docdir}/%{name}/LICENSE.md
%{_bindir}/sketchi
%{_bindir}/sketchi-server
%{_datadir}/%{name}/VERSION
%{_datadir}/applications/sketchi.desktop
%{_datadir}/icons/hicolor/512x512/apps/sketchi.png
