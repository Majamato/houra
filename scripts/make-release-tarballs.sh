#!/usr/bin/env bash
# Create the source and vendored-crates tarballs that the RPM spec expects.
# Usage: make-release-tarballs.sh OUTPUT_DIR [GIT_REF]
# GIT_REF defaults to the tag v<version>, with the version read from meson.build.
set -euo pipefail

if (($# < 1 || $# > 2)); then
    printf 'Usage: %s OUTPUT_DIR [GIT_REF]\n' "${0##*/}" >&2
    exit 2
fi

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
project_root=$(cd -- "$script_dir/.." && pwd)
mkdir -p -- "$1"
output_dir=$(cd -- "$1" && pwd)

cd "$project_root"
version=$(sed -n "s/^  version: '\(.*\)',$/\1/p" meson.build)
if [[ -z $version ]]; then
    printf 'Cannot read the version from meson.build\n' >&2
    exit 1
fi
ref=${2:-v$version}
if ! git rev-parse --quiet --verify "$ref^{commit}" >/dev/null; then
    printf 'Git ref %s does not exist. Tag the release first:\n  git tag v%s\n' "$ref" "$version" >&2
    exit 1
fi

# Every file that names the version must agree at the ref being archived.
check_version() {
    local file=$1 found=$2
    if [[ $found != "$version" ]]; then
        printf '%s at %s has version %s, but meson.build has %s\n' \
            "$file" "$ref" "${found:-<none>}" "$version" >&2
        exit 1
    fi
}
check_version meson.build "$(git show "$ref:meson.build" | sed -n "s/^  version: '\(.*\)',$/\1/p")"
check_version Cargo.toml "$(git show "$ref:Cargo.toml" | sed -n 's/^version = "\(.*\)"$/\1/p' | head -n 1)"
check_version packaging/fedora/houra.spec \
    "$(git show "$ref:packaging/fedora/houra.spec" | sed -n 's/^Version: *//p')"
check_version data/io.github.majamato.Houra.metainfo.xml.in \
    "$(git show "$ref:data/io.github.majamato.Houra.metainfo.xml.in" |
        sed -n 's/.*<release version="\([^"]*\)".*/\1/p' | head -n 1)"
check_version packaging/debian/changelog \
    "$(git show "$ref:packaging/debian/changelog" | sed -n '1s/^houra (\([^-)]*\)-[^)]*) unstable;.*/\1/p')"
check_version packaging/arch/PKGBUILD \
    "$(git show "$ref:packaging/arch/PKGBUILD" | sed -n 's/^pkgver=//p')"

work_dir=$(mktemp -d)
trap 'rm -rf -- "$work_dir"' EXIT

name=houra-$version
# A fixed timestamp and owner make repeated runs produce the same archives.
mtime=$(git log -1 --format=%ct "$ref")
tar_options=(--sort=name --owner=0 --group=0 --numeric-owner --mtime="@$mtime")

printf 'Archiving %s as %s.tar.xz...\n' "$ref" "$name"
git archive --format=tar --prefix="$name/" "$ref" | xz -9 >"$output_dir/$name.tar.xz"

printf 'Vendoring crates as %s-vendor.tar.xz...\n' "$name"
mkdir -- "$work_dir/source"
git archive --format=tar "$ref" | tar -x -C "$work_dir/source"
cargo vendor --quiet --locked --manifest-path "$work_dir/source/Cargo.toml" "$work_dir/vendor" >/dev/null
tar -c -C "$work_dir" "${tar_options[@]}" vendor | xz -9 >"$output_dir/$name-vendor.tar.xz"

printf '\nRelease tarballs:\n  %s\n  %s\n' \
    "$output_dir/$name.tar.xz" "$output_dir/$name-vendor.tar.xz"
