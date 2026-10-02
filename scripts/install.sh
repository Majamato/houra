#!/usr/bin/env bash
# Install, update or remove Houra from its prebuilt release tarball.
#
#   curl -fsSL https://github.com/Majamato/houra/releases/latest/download/install.sh | bash
#
# By default Houra is installed for the current user under ~/.local, without
# root. Run with --help for the other options. The script never deletes your
# time tracking data in ~/.local/share/houra.
#
# All code runs from main at the end, so a partly downloaded script does nothing.
set -euo pipefail
shopt -s inherit_errexit

readonly APP_ID=io.github.majamato.Houra
readonly UUID=houra@majamato.github.io
# Test hooks. Leave them unset for a normal install. HOURA_ALLOW_ROOT=1 allows a
# per-user install as root, for containers that have no other user.
readonly RELEASES_URL=${HOURA_RELEASES_URL:-https://github.com/Majamato/houra/releases}
readonly OS_RELEASE=${HOURA_OS_RELEASE:-/etc/os-release}
readonly SYSTEM_PREFIX=${HOURA_SYSTEM_PREFIX:-/usr/local}
readonly DISTRO_PREFIX=${HOURA_DISTRO_PREFIX:-/usr}

# Paths, relative to the prefix, that this script may install or remove.
readonly OWNED_PATTERN='^(bin/houra|share/applications/io\.github\.majamato\.Houra\.desktop|share/metainfo/io\.github\.majamato\.Houra\.metainfo\.xml|share/glib-2\.0/schemas/io\.github\.majamato\.Houra\.gschema\.xml|share/icons/hicolor/[a-z0-9]+/apps/io\.github\.majamato\.Houra(-symbolic)?\.svg|share/gnome-shell/extensions/houra@majamato\.github\.io/[A-Za-z0-9._/-]+|share/locale/[A-Za-z0-9_@.]+/LC_MESSAGES/houra\.mo|share/licenses/houra/LICENSE|share/houra-installer/(manifest|version|install\.sh))$'

mode=install
scope=user
requested_version=
from_file=
reinstall=0
prefix=
work_dir=

say() { printf '%s\n' "$*"; }
warn() { printf 'Warning: %s\n' "$*" >&2; }
die() {
    printf 'Error: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Install, update or remove Houra, a time tracker for GNOME.

Usage: install.sh [OPTIONS]

With no options, installs the latest release for the current user in ~/.local,
or updates an existing installation. Run the same command again to update.

Options:
  --system          Install for all users in /usr/local (run as root)
  --uninstall       Remove Houra; your data in ~/.local/share/houra is kept
  --version X.Y.Z   Install that release instead of the latest one
  --from-file PATH  Install from a downloaded houra-X.Y.Z-x86_64-linux.tar.xz
  --reinstall       Install even if the same version is already installed
  -h, --help        Show this help
EOF
}

parse_args() {
    while (($#)); do
        case $1 in
            --system) scope=system ;;
            --uninstall) mode=uninstall ;;
            --reinstall) reinstall=1 ;;
            --version)
                (($# >= 2)) || die "--version needs a version number, such as 0.1.0"
                requested_version=${2#v}
                shift
                ;;
            --from-file)
                (($# >= 2)) || die "--from-file needs the path of a Houra tarball"
                from_file=$2
                shift
                ;;
            -h | --help)
                usage
                exit 0
                ;;
            *) die "unknown option '$1'. Run with --help to see the options." ;;
        esac
        shift
    done
    if [[ -n $requested_version && -n $from_file ]]; then
        die "use either --version or --from-file, not both"
    fi
    if [[ -n $requested_version && ! $requested_version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        die "'$requested_version' is not a version number such as 0.1.0"
    fi
}

# --- Distribution detection -------------------------------------------------

# Prints dnf, apt, pacman, zypper or nothing, from os-release ID then ID_LIKE.
package_manager() {
    local id='' id_like='' word
    if [[ -r $OS_RELEASE ]]; then
        id=$(sed -n 's/^ID=//p' "$OS_RELEASE" | tr -d '"')
        id_like=$(sed -n 's/^ID_LIKE=//p' "$OS_RELEASE" | tr -d '"')
    fi
    for word in $id $id_like; do
        case $word in
            fedora | rhel | centos) echo dnf && return ;;
            debian | ubuntu) echo apt && return ;;
            arch | archlinux) echo pacman && return ;;
            opensuse* | suse | sles) echo zypper && return ;;
        esac
    done
}

# Prints the distribution package that provides a library or tool.
package_for() {
    local manager=$1 item=$2
    case $manager:$item in
        dnf:libgtk-4.so.1 | pacman:libgtk-4.so.1) echo gtk4 ;;
        apt:libgtk-4.so.1 | zypper:libgtk-4.so.1) echo libgtk-4-1 ;;
        dnf:libadwaita-1.so.0 | pacman:libadwaita-1.so.0) echo libadwaita ;;
        apt:libadwaita-1.so.0 | zypper:libadwaita-1.so.0) echo libadwaita-1-0 ;;
        dnf:glib-compile-schemas | pacman:glib-compile-schemas) echo glib2 ;;
        apt:glib-compile-schemas) echo libglib2.0-bin ;;
        zypper:glib-compile-schemas) echo glib2-tools ;;
        apt:xz | zypper:xz) echo xz-utils ;;
        *:sha256sum | *:mktemp) echo coreutils ;;
        apt:ldd) echo libc-bin ;;
        dnf:ldd | pacman:ldd | zypper:ldd) echo glibc ;;
        *) echo "$item" ;;
    esac
}

# Prints the command that installs the packages for the given libraries or tools.
install_hint() {
    local manager packages=() item
    manager=$(package_manager)
    for item in "$@"; do
        packages+=("$(package_for "${manager:-unknown}" "$item")")
    done
    case $manager in
        dnf) printf '  sudo dnf install %s\n' "${packages[*]}" ;;
        apt) printf '  sudo apt install %s\n' "${packages[*]}" ;;
        pacman) printf '  sudo pacman -S --needed %s\n' "${packages[*]}" ;;
        zypper) printf '  sudo zypper install %s\n' "${packages[*]}" ;;
        *) printf '  Install the packages that provide: %s\n' "$*" ;;
    esac
}

# --- Preflight --------------------------------------------------------------

require_tools() {
    local missing=() tool
    local tools=(tar xz sha256sum mktemp ldd)
    [[ -n $from_file ]] || tools+=(curl)
    for tool in "${tools[@]}"; do
        command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
    done
    if ((${#missing[@]})); then
        printf 'Error: this script needs %s. Install it with:\n' "${missing[*]}" >&2
        install_hint "${missing[@]}" >&2
        exit 1
    fi
}

home_of_invoking_user() {
    if [[ -n ${SUDO_USER:-} ]] && command -v getent >/dev/null 2>&1; then
        getent passwd "$SUDO_USER" | cut -d: -f6
    else
        printf '%s\n' "$HOME"
    fi
}

choose_prefix() {
    if [[ $scope == system ]]; then
        prefix=$SYSTEM_PREFIX
        local probe=$prefix
        while [[ ! -e $probe ]]; do probe=$(dirname -- "$probe"); done
        if [[ ! -w $probe ]]; then
            local flags=--system
            [[ $mode == uninstall ]] && flags+=" --uninstall"
            die "installing for all users needs root. Run:
  curl -fsSL $RELEASES_URL/latest/download/install.sh | sudo bash -s -- $flags"
        fi
        return
    fi
    [[ -n ${HOME:-} ]] || die "HOME is not set"
    if ((EUID == 0)) && [[ -z ${HOURA_ALLOW_ROOT:-} ]]; then
        die "this would install Houra only for root. Run the command as your own user
without sudo, or add --system to install it for all users."
    fi
    prefix=$HOME/.local
    if [[ -n ${XDG_DATA_HOME:-} && ${XDG_DATA_HOME%/} != "$HOME/.local/share" ]]; then
        die "XDG_DATA_HOME is set to $XDG_DATA_HOME, so GNOME would not find a per-user
install in ~/.local. Use --system to install Houra for all users instead."
    fi
}

# --- Conflicts --------------------------------------------------------------

package_owner() {
    local file=$1
    if command -v rpm >/dev/null 2>&1 && rpm -qf "$file" >/dev/null 2>&1; then
        rpm -qf "$file"
    elif command -v dpkg >/dev/null 2>&1 && dpkg -S "$file" >/dev/null 2>&1; then
        dpkg -S "$file" | cut -d: -f1
    elif command -v pacman >/dev/null 2>&1 && pacman -Qoq "$file" >/dev/null 2>&1; then
        pacman -Qoq "$file"
    fi
}

check_conflicts() {
    local packaged=$DISTRO_PREFIX/bin/houra
    if [[ -e $packaged ]]; then
        local owner source=$packaged
        owner=$(package_owner "$packaged" || true)
        [[ -z $owner ]] || source="the package $owner, $packaged"
        die "Houra is already installed from $source.
Keep using that package, which your system updates for you, or remove it
before using this script."
    fi
    if [[ $scope == user && -f $SYSTEM_PREFIX/share/houra-installer/manifest ]]; then
        die "Houra is installed for all users in $SYSTEM_PREFIX. Update or remove it with
  curl -fsSL $RELEASES_URL/latest/download/install.sh | sudo bash -s -- --system"
    fi
    if [[ $scope == system ]]; then
        local user_home
        user_home=$(home_of_invoking_user)
        if [[ -f $user_home/.local/share/houra-installer/manifest ]]; then
            die "Houra is installed for your user in $user_home/.local, and that copy would
hide this one. Remove it first by running the script without sudo and with --uninstall."
        fi
    fi
}

# --- Download and verification ----------------------------------------------

latest_version() {
    local url
    url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$RELEASES_URL/latest") ||
        die "could not reach $RELEASES_URL. Check your internet connection."
    [[ $url == */tag/v* ]] || die "could not find the latest release at $RELEASES_URL"
    printf '%s\n' "${url##*/tag/v}"
}

# Checks FILE against the SHA256SUMS file that lists it.
verify_checksum() {
    local file=$1 sums=$2 name expected actual
    name=$(basename -- "$file")
    expected=$(awk -v name="$name" '$2 == name || $2 == "*" name {print $1}' "$sums")
    [[ -n $expected ]] || die "$name is not listed in SHA256SUMS"
    actual=$(sha256sum -- "$file" | cut -d' ' -f1)
    [[ $actual == "$expected" ]] ||
        die "the download of $name is corrupt (SHA-256 mismatch). Nothing was installed."
}

# Prints the path of the tarball to install, downloading it when needed.
fetch_tarball() {
    if [[ -n $from_file ]]; then
        [[ -f $from_file ]] || die "$from_file does not exist"
        local sums
        sums=$(dirname -- "$from_file")/SHA256SUMS
        if [[ -f $sums ]]; then
            verify_checksum "$from_file" "$sums"
        else
            warn "no SHA256SUMS next to $from_file; its SHA-256 is $(sha256sum -- "$from_file" | cut -d' ' -f1)"
        fi
        printf '%s\n' "$from_file"
        return
    fi
    local version=${requested_version:-}
    [[ -n $version ]] || version=$(latest_version)
    local name=houra-$version-x86_64-linux.tar.xz
    local base=$RELEASES_URL/download/v$version
    say "Downloading Houra $version..." >&2
    curl -fsSL -o "$work_dir/$name" "$base/$name" ||
        die "could not download $base/$name"
    curl -fsSL -o "$work_dir/SHA256SUMS" "$base/SHA256SUMS" ||
        die "could not download $base/SHA256SUMS"
    verify_checksum "$work_dir/$name" "$work_dir/SHA256SUMS"
    printf '%s\n' "$work_dir/$name"
}

# Unpacks the tarball into the work directory and prints the tree's path.
unpack() {
    local tarball=$1 listing top entry relative
    listing=$(tar -tJf "$tarball") || die "$tarball is not a valid tarball"
    top=$(head -n 1 <<<"$listing")
    top=${top%%/*}
    [[ $top =~ ^houra-[0-9]+\.[0-9]+\.[0-9]+-x86_64-linux$ ]] ||
        die "$tarball is not a Houra release tarball"
    while IFS= read -r entry; do
        relative=${entry#"$top"/}
        relative=${relative%/}
        if [[ $entry != "$top"/* && $entry != "$top" && $entry != "$top/" ]] ||
            [[ /$relative/ == */../* ]]; then
            die "$tarball contains an unexpected path: $entry"
        fi
    done <<<"$listing"
    tar -xJf "$tarball" -C "$work_dir" --no-same-owner
    local tree=$work_dir/$top
    while IFS= read -r entry; do
        relative=${entry#"$tree"/}
        if [[ -L $entry ]]; then
            [[ $relative == share/gnome-shell/extensions/$UUID/locale &&
                $(readlink -- "$entry") == ../../../locale ]] ||
                die "$tarball contains an unexpected link: $relative"
        elif [[ ! -f $entry ]]; then
            die "$tarball contains an unexpected file type: $relative"
        elif [[ $relative != install.sh && ! $relative =~ $OWNED_PATTERN ]]; then
            die "$tarball contains an unexpected file: $relative"
        fi
    done < <(find "$tree" -mindepth 1 ! -type d)
    printf '%s\n' "$tree"
}

# --- System checks ----------------------------------------------------------

check_libraries() {
    local tree=$1 output missing=()
    output=$(ldd "$tree/bin/houra" 2>&1) || true
    if grep -q "GLIBC_[0-9.]*' not found" <<<"$output"; then
        die "Houra needs glibc 2.41 or newer, from a 2025 or newer distribution.
Nothing was installed."
    fi
    mapfile -t missing < <(awk '/=> not found/ {print $1}' <<<"$output")
    if ((${#missing[@]})); then
        {
            say "Error: Houra needs libraries that are not installed: ${missing[*]}"
            say "Install them with:"
            install_hint "${missing[@]}"
            say "Then run this script again. Nothing was installed."
        } >&2
        exit 1
    fi
    if ! "$tree/bin/houra" --version >/dev/null 2>&1; then
        die "Houra does not start on this system. It needs GTK 4.12 and libadwaita 1.5
or newer. Nothing was installed."
    fi
}

find_compile_schemas() {
    local candidate
    for candidate in "$(command -v glib-compile-schemas 2>/dev/null || true)" \
        /usr/lib/x86_64-linux-gnu/glib-2.0/glib-compile-schemas \
        /usr/lib64/glib-2.0/glib-compile-schemas \
        /usr/lib/glib-2.0/glib-compile-schemas; do
        if [[ -n $candidate && -x $candidate ]]; then
            printf '%s\n' "$candidate"
            return
        fi
    done
    {
        say "Error: Houra needs glib-compile-schemas to install its settings. Install it with:"
        install_hint glib-compile-schemas
    } >&2
    exit 1
}

check_shell_version() {
    local tree=$1 versions label version major
    versions=$(tr -d ' \n' <"$tree/share/gnome-shell/extensions/$UUID/metadata.json" |
        sed -n 's/.*"shell-version":\[\([^]]*\)\].*/\1/p' | tr -d '"' | tr ',' ' ')
    label=$(awk '{s = $1; for (i = 2; i < NF; i++) s = s ", " $i; if (NF > 1) s = s " or " $NF; print s}' <<<"$versions")
    if ! command -v gnome-shell >/dev/null 2>&1; then
        warn "GNOME Shell is not installed. Houra's top-bar timer and idle detection
need GNOME Shell $label."
        return
    fi
    version=$(gnome-shell --version 2>/dev/null | awk '{print $NF}')
    major=${version%%.*}
    if [[ " $versions " != *" ${major:-none} "* ]]; then
        warn "this is GNOME Shell ${version:-of an unknown version}, but Houra's top-bar
extension supports GNOME Shell $label. The app works, but the top bar may not show Houra."
    fi
}

# --- Installation -----------------------------------------------------------

# Prints the files and links of a tree, relative to it, without install.sh.
tree_files() {
    (cd -- "$1" && find . -mindepth 1 ! -type d ! -path ./install.sh | sed 's|^\./||' | LC_ALL=C sort)
}

# Rejects manifest entries that are not Houra's own files.
check_manifest() {
    local manifest=$1 entry
    while IFS= read -r entry; do
        if [[ ! $entry =~ $OWNED_PATTERN || /$entry/ == */../* || $entry == share/houra/* ]]; then
            die "$manifest lists '$entry', which is not a Houra file. Nothing was changed."
        fi
    done <"$manifest"
}

# Removes the listed files and links. Never removes directories.
remove_entries() {
    local entry path
    while IFS= read -r entry; do
        [[ -n $entry ]] || continue
        path=$prefix/$entry
        if [[ -L $path || -f $path ]]; then
            rm -f -- "$path"
        fi
    done
}

remove_empty_owned_dirs() {
    local dir
    for dir in "share/gnome-shell/extensions/$UUID/icons" "share/gnome-shell/extensions/$UUID" \
        share/licenses/houra share/houra-installer; do
        if [[ -d $prefix/$dir ]]; then
            rmdir --ignore-fail-on-non-empty -- "$prefix/$dir"
        fi
    done
}

# Escapes a path for a desktop file's Exec key, as install-desktop.py does.
exec_argument() {
    # Quoted replacements are literal in every Bash version.
    local value=$1 bs=\\
    value=${value//"$bs"/"$bs$bs"}
    value=${value//'"'/"$bs\""}
    value=${value//'`'/"$bs\`"}
    value=${value//'$'/"$bs\$"}
    value="\"$value\""
    value=${value//"$bs"/"$bs$bs"}
    printf '%s\n' "${value//'%'/'%%'}"
}

install_file() {
    local source=$1 target=$2 temporary
    mkdir -p -- "$(dirname -- "$target")"
    temporary=$target.houra-new.$$
    if [[ -L $source ]]; then
        ln -sfn -- "$(readlink -- "$source")" "$temporary"
    else
        cp -- "$source" "$temporary"
        chmod "$( [[ -x $source ]] && echo 755 || echo 644)" "$temporary"
    fi
    mv -fT -- "$temporary" "$target"
}

refresh_caches() {
    local compile_schemas=$1 schemas=$prefix/share/glib-2.0/schemas
    if compgen -G "$schemas/*.gschema.xml" >/dev/null; then
        "$compile_schemas" "$schemas"
    else
        rm -f -- "$schemas/gschemas.compiled"
    fi
    local icons=$prefix/share/icons/hicolor tool
    if [[ $scope == system || -f $icons/icon-theme.cache ]]; then
        for tool in gtk4-update-icon-cache gtk-update-icon-cache; do
            if command -v "$tool" >/dev/null 2>&1; then
                "$tool" -qtf "$icons" || true
                break
            fi
        done
    fi
}

install_houra() {
    local tarball tree version installed='' state=$prefix/share/houra-installer
    tarball=$(fetch_tarball)
    tree=$(unpack "$tarball")
    version=${tree##*/houra-}
    version=${version%-x86_64-linux}
    [[ -f $state/version ]] && installed=$(<"$state/version")
    if [[ $installed == "$version" && $reinstall == 0 ]]; then
        say "Houra $version is already installed in $prefix."
        exit 0
    fi

    check_libraries "$tree"
    local compile_schemas
    compile_schemas=$(find_compile_schemas)
    check_shell_version "$tree"

    local old_manifest='' new_manifest=$work_dir/manifest
    if [[ -f $state/manifest ]]; then
        check_manifest "$state/manifest"
        old_manifest=$state/manifest
    elif [[ -e $prefix/share/gnome-shell/extensions/$UUID ]]; then
        say "Replacing the copy of Houra's top-bar extension in $prefix/share/gnome-shell/extensions."
        rm -rf -- "${prefix:?}/share/gnome-shell/extensions/$UUID"
    fi
    {
        tree_files "$tree"
        printf '%s\n' share/houra-installer/install.sh share/houra-installer/manifest \
            share/houra-installer/version
    } >"$new_manifest"
    check_manifest "$new_manifest"

    say "Installing Houra $version in $prefix..."
    local entry
    while IFS= read -r entry; do
        install_file "$tree/$entry" "$prefix/$entry"
    done < <(tree_files "$tree")
    local desktop=$prefix/share/applications/$APP_ID.desktop exec_line
    exec_line="Exec=$(exec_argument "$prefix/bin/houra")"
    # awk -v would expand the backslashes, so the line comes from the environment.
    EXEC_LINE=$exec_line awk '/^Exec=/ {print ENVIRON["EXEC_LINE"]; next} {print}' \
        "$desktop" >"$desktop.houra-new.$$"
    mv -fT -- "$desktop.houra-new.$$" "$desktop"
    if [[ -n $old_manifest ]]; then
        LC_ALL=C sort -u "$old_manifest" | LC_ALL=C comm -23 - <(LC_ALL=C sort -u "$new_manifest") |
            remove_entries
    fi
    mkdir -p -- "$state"
    install_file "$tree/install.sh" "$state/install.sh"
    printf '%s\n' "$version" >"$state/version.houra-new.$$"
    mv -fT -- "$state/version.houra-new.$$" "$state/version"
    install_file "$new_manifest" "$state/manifest"
    remove_empty_owned_dirs
    refresh_caches "$compile_schemas"

    say ""
    if [[ -n $installed ]]; then
        say "Updated Houra from $installed to $version."
    else
        say "Installed Houra $version."
    fi
    say "Open Houra from the Activities overview, then log out and back in once so"
    say "GNOME Shell loads its top-bar timer. Houra turns the extension on for you."
    if [[ $scope == user && ":$PATH:" != *":$prefix/bin:"* ]]; then
        say ""
        say "To start houra from a terminal, add $prefix/bin to your PATH."
    fi
    say ""
    say "To update, run the same command again. To uninstall, run:"
    if [[ $scope == system ]]; then
        say "  sudo $state/install.sh --system --uninstall"
    else
        say "  $state/install.sh --uninstall"
    fi
}

uninstall_houra() {
    local state=$prefix/share/houra-installer
    if [[ ! -f $state/manifest ]]; then
        if [[ -e $DISTRO_PREFIX/bin/houra ]]; then
            die "Houra in $DISTRO_PREFIX was installed by your distribution's package manager.
Remove it with that instead."
        fi
        die "Houra was not installed in $prefix with this script."
    fi
    check_manifest "$state/manifest"
    local version='' compile_schemas
    compile_schemas=$(find_compile_schemas)
    [[ -f $state/version ]] && version=$(<"$state/version")
    remove_entries <"$state/manifest"
    remove_empty_owned_dirs
    refresh_caches "$compile_schemas"

    if [[ $scope == user ]]; then
        local autostart=${XDG_CONFIG_HOME:-$HOME/.config}/autostart/$APP_ID.desktop
        local escaped=$prefix/bin/houra
        escaped=${escaped// /\\ }
        if [[ -f $autostart ]] && grep -qxF "Exec=$escaped" "$autostart"; then
            rm -f -- "$autostart"
        fi
    fi
    say "Removed Houra${version:+ $version} from $prefix."
    if [[ $scope == user ]]; then
        say "Your data in ${XDG_DATA_HOME:-$HOME/.local/share}/houra was kept. Delete that folder to remove it."
    else
        say "Each user's data in ~/.local/share/houra was kept."
    fi
}

main() {
    parse_args "$@"
    if [[ $(uname -m) != x86_64 ]]; then
        die "Houra's prebuilt release is for x86_64 computers only; this one is $(uname -m)."
    fi
    choose_prefix
    if [[ $mode == uninstall ]]; then
        uninstall_houra
        return
    fi
    require_tools
    check_conflicts
    work_dir=$(mktemp -d)
    trap 'rm -rf -- "$work_dir"' EXIT
    install_houra
}

main "$@"
