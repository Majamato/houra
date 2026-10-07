#!/usr/bin/env bash
# Install Houra's GNOME Shell extension for the current user.
# Defaults to the prepared development extension; `release` installs the
# stable sources. Never enables extensions or changes Shell settings.
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
project_root=$(cd -- "$script_dir/.." && pwd)

manifest_value() {
    python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]][sys.argv[3]])' \
        "$project_root/data/app-variants.json" "$1" "$2"
}

mode=dev
build_dir="$project_root/target/dev/debug"
while (($# > 0)); do
    case "$1" in
        dev|release)
            mode=$1
            shift
            ;;
        --build-dir)
            if (($# < 2)); then
                printf 'Usage: %s [dev|release] [--build-dir PATH]\n' "${0##*/}" >&2
                exit 2
            fi
            build_dir=$2
            shift 2
            ;;
        --build-dir=*)
            build_dir=${1#--build-dir=}
            shift
            ;;
        *)
            printf 'Usage: %s [dev|release] [--build-dir PATH]\n' "${0##*/}" >&2
            exit 2
            ;;
    esac
done

if [[ $mode == release ]]; then
    uuid=$(manifest_value stable extension_uuid)
    source_dir=$project_root/shell-extension
    icon_source=$project_root/data/icons/hicolor/symbolic/apps/$(manifest_value stable app_id)-symbolic.svg
    hint=
else
    uuid=$(manifest_value devel extension_uuid)
    source_dir=$build_dir/shell-extension
    icon_source=$source_dir/icons/houra-symbolic.svg
    hint=' Run ./scripts/build-dev.sh first.'
fi

for required in metadata.json identity.js activeTimer.js extension.js indicator.js \
    format.js stylesheet-dark.css stylesheet-light.css; do
    if [[ ! -f $source_dir/$required ]]; then
        printf 'Missing %s.%s\n' "$source_dir/$required" "$hint" >&2
        exit 1
    fi
done
if [[ ! -f $icon_source ]]; then
    printf 'Missing %s.%s\n' "$icon_source" "$hint" >&2
    exit 1
fi
if ! python3 -c 'import json,sys; sys.exit(json.load(open(sys.argv[1])).get("uuid") != sys.argv[2])' \
    "$source_dir/metadata.json" "$uuid"; then
    printf 'Expected extension %s in %s.\n' "$uuid" "$source_dir/metadata.json" >&2
    exit 1
fi

target=${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/$uuid

mkdir -p -- "${target%/*}"
staging=$(mktemp -d "${target%/*}/.${uuid}.XXXXXX")
trap 'rm -rf -- "$staging"' EXIT
mkdir -p -- "$staging/icons"
cp -- "$source_dir"/*.js "$source_dir"/*.css "$source_dir/metadata.json" "$staging/"
cp -- "$icon_source" "$staging/icons/houra-symbolic.svg"
if [[ $mode == dev && -d $source_dir/locale ]]; then
    cp -r -- "$source_dir/locale" "$staging/"
fi

if [[ -d $target ]] && diff -rq -- "$staging" "$target" >/dev/null; then
    rm -rf -- "$staging"
    trap - EXIT
    printf 'Top-bar extension unchanged:\n  %s\n' "$target"
    exit 0
fi

rm -rf -- "$target" && mv -- "$staging" "$target"
trap - EXIT

printf 'Installed the top-bar extension:\n  %s\n' "$target"
printf '\nGNOME Shell on Wayland loads new or changed extension code only at login.\n'
printf 'Log out and back in; Houra enables the extension the next time it starts.\n'
