#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../extension"

echo "=== Installing extension deps ==="
npm install

echo ""
echo "=== TypeScript build ==="
npm run build

echo ""
echo "=== Lint ==="
npm run lint

echo ""
echo "=== Format ==="
npm run format

echo ""
echo "=== Extension build complete ==="
