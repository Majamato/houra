#!/usr/bin/env bash
# Build and run the release application, or the development application with `dev`.
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
project_root=$(cd -- "$script_dir/.." && pwd)
release_build_dir=${HOURA_BUILD_DIR:-${WORK_TIME_BUILD_DIR:-"$project_root/build-release"}}

build=${1:-release}

case "$build" in
    dev)
        "$script_dir/build-dev.sh"
        printf '\nRunning the development application...\n'
        "$project_root/target/debug/houra"
        ;;
    release)
        "$script_dir/build-release.sh"
        printf '\nRunning the release application...\n'
        "$release_build_dir/houra"
        ;;
    *)
        printf 'Usage: %s [dev]\n' "${0##*/}" >&2
        exit 2
        ;;
esac
