#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

echo "=== Building Rust crates ==="
cargo build

echo ""
echo "=== Running tests ==="
cargo test

echo ""
echo "=== Clippy ==="
cargo clippy --all-targets --all-features -- -D warnings

echo ""
echo "=== Format check ==="
cargo fmt --check

echo ""
echo "=== Rust build complete ==="
