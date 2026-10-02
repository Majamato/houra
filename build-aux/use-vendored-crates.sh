#!/usr/bin/env bash
# Point Cargo at the crates unpacked from the release's -vendor tarball.
# Usage: use-vendored-crates.sh SOURCE_DIR   (SOURCE_DIR/vendor must exist)
set -euo pipefail

source_dir=${1:?Usage: use-vendored-crates.sh SOURCE_DIR}
if [[ ! -d $source_dir/vendor ]]; then
    printf 'No vendor/ directory in %s. Unpack the -vendor tarball there first.\n' "$source_dir" >&2
    exit 1
fi
mkdir -p -- "$source_dir/.cargo"
# Cargo resolves the relative directory from the parent of .cargo/.
cat >"$source_dir/.cargo/config.toml" <<'TOML'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
TOML
