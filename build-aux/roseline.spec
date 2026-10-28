# rpm's debugsource pass has no C sources to name for a pure cargo build
%global debug_package %{nil}

Name:           roseline
Version:        0.1.1.hotfix1
Release:        1%{?dist}
Summary:        Run Roblox with Rosemary on Linux

License:        GPL-3.0-or-later
URL:            https://github.com/buwryme/Roseline
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  gtk4-devel
BuildRequires:  libadwaita-devel
BuildRequires:  libsecret-devel
BuildRequires:  glib2-devel
Requires:       gtk4%{?_isa}
Requires:       libadwaita%{?_isa}
Requires:       libsecret%{?_isa}

%description
A small GTK4/Libadwaita app written in Rust that runs the macOS Roblox clients via Rosemary.

%prep
%autosetup -n %{name}-%{version}

%build
cargo build --release --locked

%install
install -Dm755 "target/release/roseline" "%{buildroot}%{_bindir}/roseline"
strip --strip-unneeded "%{buildroot}%{_bindir}/roseline" || true
install -Dm644 "data/net.buwryy.Roseline.desktop" "%{buildroot}%{_datadir}/applications/net.buwryy.Roseline.desktop"
install -Dm644 "data/net.buwryy.Roseline.Settings.desktop" "%{buildroot}%{_datadir}/applications/net.buwryy.Roseline.Settings.desktop"
install -Dm644 "data/net.buwryy.Roseline.metainfo.xml" "%{buildroot}%{_datadir}/metainfo/net.buwryy.Roseline.metainfo.xml"
install -Dm644 "data/icons/hicolor/scalable/apps/net.buwryy.Roseline.svg" "%{buildroot}%{_datadir}/icons/hicolor/scalable/apps/net.buwryy.Roseline.svg"
install -Dm644 "data/icons/hicolor/128x128/apps/net.buwryy.Roseline.png" "%{buildroot}%{_datadir}/icons/hicolor/128x128/apps/net.buwryy.Roseline.png"
install -Dm644 "LICENSE" "%{buildroot}%{_licensedir}/roseline/LICENSE"
install -Dm644 "NOTICE" "%{buildroot}%{_licensedir}/roseline/NOTICE"

%files
%{_bindir}/roseline
%{_datadir}/applications/net.buwryy.Roseline.desktop
%{_datadir}/applications/net.buwryy.Roseline.Settings.desktop
%{_datadir}/metainfo/net.buwryy.Roseline.metainfo.xml
%{_datadir}/icons/hicolor/scalable/apps/net.buwryy.Roseline.svg
%{_datadir}/icons/hicolor/128x128/apps/net.buwryy.Roseline.png
%license %{_licensedir}/roseline/LICENSE
%license %{_licensedir}/roseline/NOTICE

%changelog
* Tue Oct 27 2026 buwryme <hello@buwryy.net> - 0.1.1.hotfix1-1
- Keep Roblox updates under Roseline and pin the checked Rosemary release
* Fri Oct 23 2026 buwryme <hello@buwryy.net> - 0.1.1-1
- Font overrides, Discord RPC updates, and the remote installer
* Sun Oct 18 2026 buwryme <hello@buwryy.net> - 0.1.0-1
- Initial packaging
