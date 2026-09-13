set -euo pipefail

source_root=$1
build_root=$2
output=$3
shift 3

export CARGO_TARGET_DIR="$build_root/cargo-target"
cargo build --manifest-path "$source_root/Cargo.toml" --release "$@"
cp "$CARGO_TARGET_DIR/release/houra" "$output"
