#!/usr/bin/env bash
set -euo pipefail

source_root=$1
build_root=$2
output=$(realpath -m -- "$3")
shift 3

export CARGO_TARGET_DIR="$build_root/cargo-target"
cd "$source_root"
cargo build --release "$@"
cp "$CARGO_TARGET_DIR/release/houra" "$output"
