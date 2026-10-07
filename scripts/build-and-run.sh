#!/usr/bin/env bash
# Build and run the development application by default, or the release
# application with `release`.
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
project_root=$(cd -- "$script_dir/.." && pwd)
release_build_dir=${HOURA_BUILD_DIR:-"$project_root/build-release"}

packaged_houra() {
    local dir dirs
    IFS=: read -ra dirs <<<"${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
    for dir in "${dirs[@]}"; do
        [[ -f $dir/applications/io.github.majamato.Houra.desktop ]] && return 0
    done
    return 1
}

usage() {
    printf 'Usage: %s [dev [--release] [--offline]|release [--replace-packaged]]\n' "${0##*/}" >&2
}

case "${1:-dev}" in
    dev)
        profile=debug
        offline_flag=()
        if (($# > 1)); then
            for arg in "${@:2}"; do
                case "$arg" in
                    --release)
                        profile=release
                        ;;
                    --offline)
                        offline_flag=(--offline)
                        ;;
                    *)
                        usage
                        exit 2
                        ;;
                esac
            done
        fi
        release_flag=()
        if [[ $profile == release ]]; then
            release_flag=(--release)
        fi
        "$script_dir/build-dev.sh" "${release_flag[@]}" "${offline_flag[@]}"
        "$script_dir/install-dev.sh" "${release_flag[@]}"
        printf '\nRunning the development application...\n'
        "$project_root/target/dev/$profile/houra"
        ;;
    release)
        case "${2:-}" in
            ""|--replace-packaged)
                ;;
            *)
                usage
                exit 2
                ;;
        esac
        if (($# > 2)); then
            usage
            exit 2
        fi
        if [[ ${2:-} != --replace-packaged ]] && packaged_houra; then
            printf '%s\n' \
                "Refusing to run the release build: a packaged Houra launcher exists." \
                "The release build shares the packaged app's database, settings, autostart entry and bus name." \
                "Running it would rewrite the packaged app's autostart entry to point at this local build," \
                "and a newer database schema would lock the packaged app out of its database." \
                "Pass 'release --replace-packaged' to run it anyway, or use 'dev' for isolated development." >&2
            exit 1
        fi
        "$script_dir/build-release.sh"
        export GSETTINGS_SCHEMA_DIR="$release_build_dir"
        printf '\nRunning the release application...\n'
        "$release_build_dir/houra"
        ;;
    *)
        usage
        exit 2
        ;;
esac
