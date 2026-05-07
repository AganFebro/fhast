#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "=== fhast full build ==="

"$SCRIPT_DIR/build-rust.sh"
echo ""
"$SCRIPT_DIR/build-extension.sh"

echo ""
echo "=== Full build complete ==="
