#!/usr/bin/env bash
# Build the prebuilt x86_64 tarball that install.sh and the AUR package use.
# Usage: make-binary-tarball.sh RELEASE_DIR [OUTPUT_DIR]
# RELEASE_DIR holds houra-X.Y.Z.tar.xz and houra-X.Y.Z-vendor.tar.xz from
# make-release-tarballs.sh. Building from them proves they are complete.
#
# The binary links against the system's glibc, GTK and libadwaita, so build
# on the oldest distribution Houra supports; the release workflow uses Debian 13.
set -euo pipefail

if (($# < 1 || $# > 2)); then
    printf 'Usage: %s RELEASE_DIR [OUTPUT_DIR]\n' "${0##*/}" >&2
    exit 2
fi

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
release_dir=$(cd -- "$1" && pwd)
mkdir -p -- "${2:-$release_dir}"
output_dir=$(cd -- "${2:-$release_dir}" && pwd)

shopt -s nullglob
sources=("$release_dir"/houra-[0-9]*[0-9].tar.xz)
shopt -u nullglob
if ((${#sources[@]} != 1)); then
    printf 'Expected exactly one houra-X.Y.Z.tar.xz in %s\n' "$release_dir" >&2
    exit 1
fi
name=$(basename -- "${sources[0]}" .tar.xz)
version=${name#houra-}
vendor=$release_dir/$name-vendor.tar.xz
if [[ ! -f $vendor ]]; then
    printf 'Missing %s\n' "$vendor" >&2
    exit 1
fi
if [[ $(uname -m) != x86_64 ]]; then
    printf 'The binary tarball is x86_64 only; this machine is %s\n' "$(uname -m)" >&2
    exit 1
fi

work_dir=$(mktemp -d)
trap 'rm -rf -- "$work_dir"' EXIT

tar -xf "${sources[0]}" -C "$work_dir"
source_dir=$work_dir/$name
tar -xf "$vendor" -C "$source_dir"
bash "$source_dir/build-aux/use-vendored-crates.sh" "$source_dir"

export CARGO_HOME=$work_dir/cargo-home
meson setup "$work_dir/build" "$source_dir" \
    --buildtype=release --prefix=/usr/local -Doffline=true
meson compile -C "$work_dir/build"
DESTDIR=$work_dir/stage meson install -C "$work_dir/build" --no-rebuild

tree_name=houra-$version-x86_64-linux
tree=$work_dir/$tree_name
mv -- "$work_dir/stage/usr/local" "$tree"
install -m 755 "$script_dir/install.sh" "$tree/install.sh"
# Each install compiles the schemas in place, covering every installed app.
rm -f -- "$tree/share/glib-2.0/schemas/gschemas.compiled" \
    "$tree/share/icons/hicolor/icon-theme.cache"

reported=$("$tree/bin/houra" --version)
if [[ $reported != "houra $version" ]]; then
    printf 'bin/houra --version printed "%s", expected "houra %s"\n' "$reported" "$version" >&2
    exit 1
fi
link=$tree/share/gnome-shell/extensions/houra@majamato.github.io/locale
if [[ $(readlink -- "$link") != ../../../locale ]]; then
    printf 'The extension locale link must be relative, found: %s\n' "$(readlink -- "$link")" >&2
    exit 1
fi

# The source tarball's newest file time keeps repeated builds identical.
mtime=$(tar -tvJf "${sources[0]}" --full-time | awk '{print $4 " " $5}' | sort | tail -n 1)
tar -c -C "$work_dir" --sort=name --owner=0 --group=0 --numeric-owner \
    --mtime="$mtime" "$tree_name" | xz -9 >"$output_dir/$tree_name.tar.xz"
printf '\nBinary tarball:\n  %s\n' "$output_dir/$tree_name.tar.xz"
