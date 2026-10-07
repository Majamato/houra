#!/usr/bin/env bash
# Remove Houra Dev's launcher, icons, extension, autostart entry, and settings.
# `--purge-data` also removes the development database directory. Production
# files are never touched.
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
project_root=$(cd -- "$script_dir/.." && pwd)

purge=false
for arg in "$@"; do
    case "$arg" in
        --purge-data)
            purge=true
            ;;
        *)
            printf 'Usage: %s [--purge-data]\n' "${0##*/}" >&2
            exit 2
            ;;
    esac
done

manifest_value() {
    python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]][sys.argv[3]])' \
        "$project_root/data/app-variants.json" "$1" "$2"
}

app_id=$(manifest_value devel app_id)
app_name=$(manifest_value devel app_name)
data_subdir=$(manifest_value devel data_subdir)
settings_path=$(manifest_value devel settings_path)
extension_uuid=$(manifest_value devel extension_uuid)

data_home=${XDG_DATA_HOME:-$HOME/.local/share}
config_home=${XDG_CONFIG_HOME:-$HOME/.config}

for value in "$app_id" "$data_subdir" "$settings_path" "$extension_uuid"; do
    if [[ -z $value ]]; then
        printf 'Cannot uninstall: the variant manifest has an empty devel value.\n' >&2
        exit 1
    fi
done

rm -f -- "$data_home/applications/$app_id.desktop"
rm -f -- "$data_home/icons/hicolor/scalable/apps/$app_id.svg"
for icon in "$data_home"/icons/hicolor/scalable/apps/"$app_id"-*.svg; do
    [[ -f $icon ]] || continue
    rm -f -- "$icon"
done
rm -f -- "$data_home/icons/hicolor/symbolic/apps/$app_id-symbolic.svg"
rm -rf -- "${data_home:?}/gnome-shell/extensions/$extension_uuid"
rm -f -- "$config_home/autostart/$app_id.desktop"

if command -v gsettings >/dev/null 2>&1; then
    enabled=$(gsettings get org.gnome.shell enabled-extensions 2>/dev/null || true)
    if [[ -n $enabled ]] \
        && updated=$(python3 -c 'import ast,sys
value = sys.argv[1].strip()
entries = [] if value in ("@as []", "[]") else list(ast.literal_eval(value))
print([entry for entry in entries if entry != sys.argv[2]])' \
            "$enabled" "$extension_uuid" 2>/dev/null); then
        gsettings set org.gnome.shell enabled-extensions "$updated" \
            || printf 'Warning: could not update enabled-extensions.\n' >&2
    else
        printf 'Warning: could not read enabled-extensions; the dev extension may stay enabled.\n' >&2
    fi
else
    printf 'Warning: gsettings is unavailable; the dev extension may stay enabled.\n' >&2
fi

if command -v dconf >/dev/null 2>&1; then
    dconf reset -f "$settings_path" \
        || printf 'Warning: could not reset dev settings under %s.\n' "$settings_path" >&2
else
    printf 'Warning: dconf is unavailable; dev settings under %s were left behind.\n' "$settings_path" >&2
fi

if [[ $purge == true ]]; then
    rm -rf -- "${data_home:?}/$data_subdir"
fi

if command -v gtk4-update-icon-cache >/dev/null 2>&1; then
    gtk4-update-icon-cache -qtf "$data_home/icons/hicolor" \
        || printf 'Warning: gtk4-update-icon-cache could not refresh %s.\n' "$data_home/icons/hicolor" >&2
fi
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$data_home/applications" \
        || printf 'Warning: update-desktop-database could not refresh %s.\n' "$data_home/applications" >&2
fi

printf 'Removed the %s desktop assets.\n' "$app_name"
if [[ $purge == true ]]; then
    printf 'Removed the development data directory:\n  %s\n' "$data_home/$data_subdir"
else
    printf 'Kept the development data directory (pass --purge-data to remove it):\n  %s\n' "$data_home/$data_subdir"
fi
