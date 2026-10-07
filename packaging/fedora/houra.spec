%global app_id io.github.majamato.Houra

Name:           houra
Version:        0.2.0
Release:        1%{?dist}
Summary:        Time tracker for GNOME with a top-bar timer
License:        GPL-3.0-or-later
URL:            https://github.com/Majamato/houra
Source0:        %{url}/releases/download/v%{version}/%{name}-%{version}.tar.xz
Source1:        %{url}/releases/download/v%{version}/%{name}-%{version}-vendor.tar.xz

BuildRequires:  cargo >= 1.88
BuildRequires:  rust >= 1.88
BuildRequires:  meson >= 1.3
BuildRequires:  gcc
BuildRequires:  gtk4-devel >= 4.12
BuildRequires:  libadwaita-devel >= 1.5
BuildRequires:  glib2-devel
BuildRequires:  sqlite-devel
BuildRequires:  gettext
BuildRequires:  desktop-file-utils
BuildRequires:  appstream
Requires:       gtk4 >= 4.12
Requires:       libadwaita >= 1.5

%description
Houra tracks time by project and activity. It keeps count through breaks,
idle time, suspends and crashes, lets you review time you were away, and
exports weekly reports as CSV. Data is stored locally in SQLite under the
user's XDG data directory.

The package includes a GNOME Shell extension that shows the active timer in
the top bar. Houra enables it automatically the first time it starts; log out
and back in once so GNOME Shell loads it.

%prep
%autosetup
tar -xf %{SOURCE1}
bash build-aux/use-vendored-crates.sh .

%build
%meson -Doffline=true
%meson_build

%install
%meson_install
%find_lang %{name}

%check
cargo test --workspace --all-targets --offline
desktop-file-validate %{buildroot}%{_datadir}/applications/%{app_id}.desktop
appstreamcli validate --no-net %{buildroot}%{_metainfodir}/%{app_id}.metainfo.xml

%files -f %{name}.lang
%license %{_datadir}/licenses/houra/LICENSE
%{_bindir}/houra
%{_datadir}/applications/%{app_id}.desktop
%{_metainfodir}/%{app_id}.metainfo.xml
%{_datadir}/glib-2.0/schemas/%{app_id}.gschema.xml
%{_datadir}/icons/hicolor/scalable/apps/%{app_id}.svg
%{_datadir}/icons/hicolor/symbolic/apps/%{app_id}-symbolic.svg
%dir %{_datadir}/gnome-shell
%dir %{_datadir}/gnome-shell/extensions
%{_datadir}/gnome-shell/extensions/houra@majamato.github.io/

%changelog
* Wed Oct 07 2026 majamato - 0.2.0-1
- Show each entry's total time on its row when it spans several days
- Refuse edits that end in the future, and let a stuck timer stop and Houra quit

* Thu Oct 01 2026 majamato - 0.1.0-1
- First public release
