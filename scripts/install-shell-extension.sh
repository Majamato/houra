#!/usr/bin/env bash
# Install Houra's GNOME Shell extension for the current user, for development.
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
project_root=$(cd -- "$script_dir/.." && pwd)
uuid=houra@majamato.github.io
target=${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/$uuid

rm -rf -- "$target"
mkdir -p -- "$target/icons"
cp -- "$project_root"/shell-extension/*.js "$project_root"/shell-extension/*.css \
    "$project_root/shell-extension/metadata.json" "$target/"
cp -- "$project_root/data/icons/hicolor/symbolic/apps/io.github.majamato.Houra-symbolic.svg" \
    "$target/icons/houra-symbolic.svg"

printf 'Installed the top-bar extension:\n  %s\n' "$target"
printf '\nGNOME Shell on Wayland loads new or changed extension code only at login.\n'
printf 'Log out and back in; Houra enables the extension the next time it starts.\n'
