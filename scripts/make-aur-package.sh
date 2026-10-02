#!/usr/bin/env bash
# Prepare the AUR houra-bin package for a release: PKGBUILD with the binary
# tarball's checksum, houra.install and .SRCINFO. Needs makepkg (Arch Linux).
# Usage: make-aur-package.sh RELEASE_DIR OUTPUT_DIR
# RELEASE_DIR holds houra-X.Y.Z-x86_64-linux.tar.xz. OUTPUT_DIR becomes the
# contents of the AUR git repository; the tarball is copied there too so
# makepkg can test the package without downloading it.
set -euo pipefail

if (($# != 2)); then
    printf 'Usage: %s RELEASE_DIR OUTPUT_DIR\n' "${0##*/}" >&2
    exit 2
fi
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
project_root=$(cd -- "$script_dir/.." && pwd)
release_dir=$(cd -- "$1" && pwd)
mkdir -p -- "$2"
output_dir=$(cd -- "$2" && pwd)

pkgver=$(sed -n 's/^pkgver=//p' "$project_root/packaging/arch/PKGBUILD")
tarball=$release_dir/houra-$pkgver-x86_64-linux.tar.xz
if [[ ! -f $tarball ]]; then
    printf 'Missing %s\n' "$tarball" >&2
    exit 1
fi
checksum=$(sha256sum -- "$tarball" | cut -d' ' -f1)
sed "s/^sha256sums=('SKIP')$/sha256sums=('$checksum')/" \
    "$project_root/packaging/arch/PKGBUILD" >"$output_dir/PKGBUILD"
if ! grep -qx "sha256sums=('$checksum')" "$output_dir/PKGBUILD"; then
    printf 'packaging/arch/PKGBUILD must contain the line: sha256sums=('"'"'SKIP'"'"')\n' >&2
    exit 1
fi
cp -- "$project_root/packaging/arch/houra.install" "$output_dir/"
cp -- "$tarball" "$output_dir/"
(cd "$output_dir" && makepkg --printsrcinfo >.SRCINFO)
printf 'AUR package files in %s\n' "$output_dir"
