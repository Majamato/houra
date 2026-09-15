#!/usr/bin/env bash
# Build the optimized application and all GNOME installation assets.
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
project_root=$(cd -- "$script_dir/.." && pwd)
build_dir=${HOURA_BUILD_DIR:-${WORK_TIME_BUILD_DIR:-"$project_root/build-release"}}

missing_packages=()

require_command() {
    local command_name=$1
    local fedora_package=$2
    if ! command -v "$command_name" >/dev/null 2>&1; then
        missing_packages+=("$fedora_package")
    fi
}

require_pkg_config() {
    local module=$1
    local minimum_version=$2
    local fedora_package=$3
    if ! pkg-config --atleast-version="$minimum_version" "$module" >/dev/null 2>&1; then
        missing_packages+=("$fedora_package")
    fi
}

require_command cargo cargo
require_command rustc rust
require_command cc gcc
require_command meson meson
require_command ninja ninja-build
require_command pkg-config pkgconf-pkg-config
require_command glib-compile-resources glib2-devel
require_command glib-compile-schemas glib2-devel
require_command msgfmt gettext
require_command xgettext gettext

if command -v pkg-config >/dev/null 2>&1; then
    require_pkg_config gtk4 4.12 gtk4-devel
    require_pkg_config libadwaita-1 1.5 libadwaita-devel
    require_pkg_config gio-2.0 2.84 glib2-devel
fi

if ((${#missing_packages[@]} > 0)); then
    mapfile -t missing_packages < <(printf '%s\n' "${missing_packages[@]}" | sort -u)
    printf 'Cannot build Houra. Missing Fedora packages:\n' >&2
    printf '  - %s\n' "${missing_packages[@]}" >&2
    printf '\nInstall them yourself with:\n  sudo dnf install' >&2
    printf ' %q' "${missing_packages[@]}" >&2
    printf '\n' >&2
    exit 1
fi

cd "$project_root"
if [[ -f "$build_dir/meson-private/coredata.dat" ]]; then
    meson setup --reconfigure "$build_dir" --buildtype=release -Doffline=false
else
    meson setup "$build_dir" --buildtype=release -Doffline=false
fi

printf 'Building the release application...\n'
meson compile -C "$build_dir" "$@"
printf '\nRelease binary:\n  %s/houra\n' "$build_dir"
printf 'Stage a complete install with:\n  DESTDIR=%q meson install -C %q\n' \
    "$project_root/stage" "$build_dir"
