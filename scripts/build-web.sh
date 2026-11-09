#!/usr/bin/env bash
# Builds the browser version into dist/web (serve that folder, e.g. `python3 -m http.server -d dist/web`).
set -euo pipefail
cd "$(dirname "$0")/.."
profile="${1:-release}"
flag=$([ "$profile" = "dev" ] && echo "--dev" || echo "--release")
wasm-pack build apps/solidsketch --target web $flag --no-typescript --no-pack --out-dir ../../dist/web/pkg
cp web/index.html dist/web/index.html
echo "dist/web prêt : python3 -m http.server 8080 -d dist/web  →  http://localhost:8080"
