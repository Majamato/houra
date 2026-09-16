Name:           houra
Version:        0.1.0
Release:        1%{?dist}
Summary:        Local-first GNOME work time tracker
License:        GPL-3.0-or-later
URL:            https://github.com/Majamato/houra
Source0:        %{url}/releases/download/v%{version}/%{name}-%{version}.tar.xz
Source1:        %{url}/releases/download/v%{version}/%{name}-%{version}-vendor.tar.xz

BuildRequires:  cargo >= 1.85
BuildRequires:  rust >= 1.85
BuildRequires:  meson >= 1.3
BuildRequires:  gcc
BuildRequires:  gtk4-devel >= 4.12
BuildRequires:  libadwaita-devel >= 1.5
BuildRequires:  glib2-devel
BuildRequires:  sqlite-devel
BuildRequires:  gettext
BuildRequires:  desktop-file-utils
BuildRequires:  libappstream-glib
Requires:       gtk4 >= 4.12
Requires:       libadwaita >= 1.5

%description
Houra records one project or activity at a time, reconciles GNOME idle
periods, and exports reports and backups. All user data stays on the local
machine under the user's XDG data directory.

%prep
%autosetup
tar -xf %{SOURCE1}
mkdir -p .cargo
printf '[source.crates-io]\nreplace-with = "vendored-sources"\n[source.vendored-sources]\ndirectory = "vendor"\n' > .cargo/config.toml

%build
%meson -Doffline=true
%meson_build

%install
%meson_install

%check
cargo test --workspace --all-targets --offline
desktop-file-validate %{buildroot}%{_datadir}/applications/io.github.majamato.Houra.desktop
appstream-util validate-relax --nonet %{buildroot}%{_metainfodir}/io.github.majamato.Houra.metainfo.xml

%files
%license %{_datadir}/licenses/houra/LICENSE
%{_bindir}/houra
%{_datadir}/applications/io.github.majamato.Houra.desktop
%{_metainfodir}/io.github.majamato.Houra.metainfo.xml
%{_datadir}/glib-2.0/schemas/io.github.majamato.Houra.gschema.xml
%{_datadir}/icons/hicolor/scalable/apps/io.github.majamato.Houra.svg
%{_datadir}/icons/hicolor/symbolic/apps/io.github.majamato.Houra-symbolic.svg
%{_datadir}/locale/*/LC_MESSAGES/houra.mo

%changelog
* Thu Aug 27 2026 majamato - 0.1.0-1
- Initial Fedora package
