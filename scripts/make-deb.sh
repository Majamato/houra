#!/usr/bin/env bash
# Build Houra's Debian source package, or a binary .deb, from the release tarballs.
# Usage: make-deb.sh RELEASE_DIR OUTPUT_DIR [--series CODENAME] [--binary]
#
# RELEASE_DIR holds houra-X.Y.Z.tar.xz and houra-X.Y.Z-vendor.tar.xz. For the
# PPA, use the files attached to the GitHub release: every upload of a version
# must carry byte-identical orig tarballs.
#
#   --series CODENAME  Version and target the package for an Ubuntu series,
#                      such as resolute (26.04). Without it the package
#                      targets Debian unstable.
#   --binary           Build the .deb instead of the source package. The build
#                      dependencies in packaging/debian/control must be installed.
set -euo pipefail

declare -A UBUNTU_SERIES=([resolute]=26.04 [stonking]=26.10)

usage() {
    printf 'Usage: %s RELEASE_DIR OUTPUT_DIR [--series CODENAME] [--binary]\n' "${0##*/}" >&2
    exit 2
}

(($# >= 2)) || usage
release_dir=$(cd -- "$1" && pwd)
mkdir -p -- "$2"
output_dir=$(cd -- "$2" && pwd)
shift 2
series=
binary=0
while (($#)); do
    case $1 in
        --series)
            (($# >= 2)) || usage
            series=$2
            shift
            ;;
        --binary) binary=1 ;;
        *) usage ;;
    esac
    shift
done
if [[ -n $series && -z ${UBUNTU_SERIES[$series]:-} ]]; then
    printf 'Unknown Ubuntu series %s. Known: %s\n' "$series" "${!UBUNTU_SERIES[*]}" >&2
    exit 1
fi

shopt -s nullglob
sources=("$release_dir"/houra-[0-9]*[0-9].tar.xz)
shopt -u nullglob
if ((${#sources[@]} != 1)); then
    printf 'Expected exactly one houra-X.Y.Z.tar.xz in %s\n' "$release_dir" >&2
    exit 1
fi
name=$(basename -- "${sources[0]}" .tar.xz)
version=${name#houra-}

work_dir=$(mktemp -d)
trap 'rm -rf -- "$work_dir"' EXIT

cp -- "${sources[0]}" "$work_dir/houra_$version.orig.tar.xz"
cp -- "$release_dir/$name-vendor.tar.xz" "$work_dir/houra_$version.orig-vendor.tar.xz"
tar -xf "$work_dir/houra_$version.orig.tar.xz" -C "$work_dir"
source_dir=$work_dir/$name
tar -xf "$work_dir/houra_$version.orig-vendor.tar.xz" -C "$source_dir"
cp -r -- "$source_dir/packaging/debian" "$source_dir/debian"

if [[ -n $series ]]; then
    package_version=$version-1~ubuntu${UBUNTU_SERIES[$series]}.1
    sed -i "1s/^houra ([^)]*) unstable;/houra ($package_version) $series;/" \
        "$source_dir/debian/changelog"
    if ! head -n 1 "$source_dir/debian/changelog" | grep -qF "($package_version) $series;"; then
        printf 'Could not retarget debian/changelog for %s\n' "$series" >&2
        exit 1
    fi
fi

cd "$source_dir"
if ((binary)); then
    dpkg-buildpackage -b -us -uc
else
    dpkg-buildpackage -S -sa -d -us -uc
fi
cd "$work_dir"
find . -maxdepth 1 -type f \( -name '*.deb' -o -name '*.dsc' -o -name '*.changes' \
    -o -name '*.buildinfo' -o -name 'houra_*.tar.*' \) -exec cp -t "$output_dir" -- {} +
printf '\nPackages in %s:\n' "$output_dir"
ls -1 -- "$output_dir"
