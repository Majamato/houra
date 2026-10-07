#!/usr/bin/env bash
# Build the development application and prepare its isolated artifacts,
# without installing or starting anything.
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
project_root=$(cd -- "$script_dir/.." && pwd)
target_dir="$project_root/target/dev"
# The production release directory never affects development builds.
unset HOURA_BUILD_DIR

profile=debug
offline=false
for arg in "$@"; do
    case "$arg" in
        --release)
            profile=release
            ;;
        --offline)
            offline=true
            ;;
        *)
            printf 'Usage: %s [--release] [--offline]\n' "${0##*/}" >&2
            exit 2
            ;;
    esac
done

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
require_command pkg-config pkgconf-pkg-config
require_command glib-compile-resources glib2-devel
require_command glib-compile-schemas glib2-devel
require_command msgfmt gettext
require_command python3 python3

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

cargo_args=(--workspace --locked --features "native-ui,dev-app" --target-dir "$target_dir")
if [[ $profile == release ]]; then
    cargo_args+=(--release)
fi
if [[ $offline == true ]]; then
    cargo_args+=(--offline)
fi

cd "$project_root"
printf 'Building the development application...\n'
cargo build "${cargo_args[@]}"
bin_dir="$target_dir/$profile"
python3 -B "$script_dir/prepare-dev.py" --output-dir "$bin_dir"
printf '\nDevelopment binary:\n  %s/houra\n' "$bin_dir"
