#!/usr/bin/env bash
# Install the prepared development launcher, icons, and extension.
# Builds nothing and starts nothing; run ./scripts/build-dev.sh first.
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
project_root=$(cd -- "$script_dir/.." && pwd)

profile=debug
for arg in "$@"; do
    case "$arg" in
        --release)
            profile=release
            ;;
        *)
            printf 'Usage: %s [--release]\n' "${0##*/}" >&2
            exit 2
            ;;
    esac
done

build_dir="$project_root/target/dev/$profile"
"$script_dir/install-desktop.py" --variant devel --build-dir "$build_dir"
"$script_dir/install-shell-extension.sh" dev --build-dir "$build_dir"
