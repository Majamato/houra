#!/usr/bin/env bash
# Check install.sh with the binary tarball on this system: a per-user install,
# a repeated install, an uninstall that keeps the data, and the same for --system.
# Usage: smoke-test-install.sh DIST_DIR
# Run it as root in a throwaway container that has Houra's runtime libraries,
# gsettings and desktop-file-validate installed. It changes /usr/local.
set -euo pipefail

if (($# != 1)); then
    printf 'Usage: %s DIST_DIR\n' "${0##*/}" >&2
    exit 2
fi
shopt -s nullglob
tarballs=("$1"/houra-*-x86_64-linux.tar.xz)
shopt -u nullglob
if ((${#tarballs[@]} != 1)); then
    printf 'Expected exactly one houra-X.Y.Z-x86_64-linux.tar.xz in %s\n' "$1" >&2
    exit 1
fi
tarball=$(realpath -- "${tarballs[0]}")
version=${tarball##*/houra-}
version=${version%-x86_64-linux.tar.xz}

work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
tar -xJf "$tarball" -C "$work" --strip-components=1 "houra-$version-x86_64-linux/install.sh"
installer=$work/install.sh
uuid=houra@majamato.github.io

fail() {
    printf 'FAIL: %s\n' "$*" >&2
    exit 1
}

# Checks an installed prefix. $1 is the prefix, $2 the HOME for gsettings.
check_installed() {
    local prefix=$1
    if ldd "$prefix/bin/houra" | grep 'not found'; then
        fail "libraries are missing for $prefix/bin/houra"
    fi
    [[ $("$prefix/bin/houra" --version) == "houra $version" ]] || fail "wrong --version"
    [[ $(readlink -f "$prefix/share/gnome-shell/extensions/$uuid/locale") == "$prefix/share/locale" ]] ||
        fail "the extension's locale link does not resolve to $prefix/share/locale"
    [[ -f $prefix/share/glib-2.0/schemas/gschemas.compiled ]] || fail "schemas were not compiled"
    grep -qxF "Exec=\"$prefix/bin/houra\"" "$prefix/share/applications/io.github.majamato.Houra.desktop" ||
        fail "the launcher does not run $prefix/bin/houra"
    desktop-file-validate "$prefix/share/applications/io.github.majamato.Houra.desktop"
    gsettings list-keys io.github.majamato.Houra >/dev/null ||
        fail "GSettings does not find Houra's schema in $prefix"
}

check_removed() {
    local prefix=$1 left
    left=$(find "$prefix" \( -name 'houra*' -o -name 'io.github.majamato.Houra*' -o -name "$uuid" \) \
        ! -path "$prefix/share/houra" ! -path "$prefix/share/houra/*" -print)
    [[ -z $left ]] || fail "files were left after uninstalling: $left"
}

printf '== Per-user install\n'
export HOME=$work/home\ with\ space
mkdir -p "$HOME"
unset XDG_DATA_HOME XDG_CONFIG_HOME
export HOURA_ALLOW_ROOT=1
bash "$installer" --from-file "$tarball"
check_installed "$HOME/.local"
bash "$installer" --from-file "$tarball" | grep -q 'already installed' ||
    fail "a repeated install did not report the installed version"
bash "$installer" --from-file "$tarball" --reinstall >/dev/null

printf '== Per-user uninstall\n'
mkdir -p "$HOME/.local/share/houra" "$HOME/.config/autostart"
printf 'data' >"$HOME/.local/share/houra/houra.sqlite3"
printf '[Desktop Entry]\nExec=%s\n' "${HOME// /\\ }/.local/bin/houra" \
    >"$HOME/.config/autostart/io.github.majamato.Houra.desktop"
"$HOME/.local/share/houra-installer/install.sh" --uninstall
[[ $(<"$HOME/.local/share/houra/houra.sqlite3") == data ]] || fail "uninstall touched the data"
[[ ! -e $HOME/.config/autostart/io.github.majamato.Houra.desktop ]] ||
    fail "uninstall left the autostart entry"
check_removed "$HOME/.local"

printf '== System-wide install\n'
unset HOURA_ALLOW_ROOT
bash "$installer" --system --from-file "$tarball"
check_installed /usr/local
bash "$installer" --system --uninstall
check_removed /usr/local

printf 'All install checks passed for Houra %s.\n' "$version"
