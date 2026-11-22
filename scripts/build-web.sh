#!/usr/bin/env bash
# Builds the browser version into dist/web (serve that folder, e.g. `python3 -m http.server -d dist/web`):
# the UI's wasm in pkg/, the kernel's in worker/ with the script that runs it in a web worker
# (docs/model/kernel.md).
#
# wasm-pack runs wasm-bindgen and wasm-opt every time, even on an unchanged build, and wasm-opt
# takes a while. So cargo builds first (a no-op when nothing changed), and wasm-pack runs only
# when the wasm cargo produced differs from the one the last packaging came from.
set -euo pipefail
cd "$(dirname "$0")/.."
profile="${1:-release}"
if [ "$profile" = "dev" ]; then flag="--dev"; cargo_flag=""; dir="debug"; else flag="--release"; cargo_flag="--release"; dir="release"; fi

# package, its folder, the artefact cargo writes, where wasm-pack puts it
package() {
  local name="$1" crate="$2" artefact="$3" out="$4"
  cargo build --lib --target wasm32-unknown-unknown $cargo_flag -p "$name"
  local wasm="target/wasm32-unknown-unknown/$dir/$artefact.wasm"
  local stamp="dist/web/$out/.built-from"
  local source
  source="$(shasum -a 256 "$wasm" | cut -d' ' -f1) $flag"
  if [ -f "$stamp" ] && [ "$(cat "$stamp")" = "$source" ] && [ -f "dist/web/$out/${artefact}_bg.wasm" ]; then
    echo "$out/ à jour : wasm-pack sauté"
    return
  fi
  wasm-pack build "$crate" --target web $flag --no-typescript --no-pack --out-dir "../../dist/web/$out"
  echo "$source" > "$stamp"
}

package skraft apps/skraft skraft pkg
package skraft-kernel-worker apps/kernel-worker skraft_kernel_worker worker
cp web/kernel-worker.js web/wheel.js web/favicon.svg dist/web/
# The loading screen's progress needs the decompressed sizes.
sed -e "s/__UI_WASM_BYTES__/$(wc -c < dist/web/pkg/skraft_bg.wasm | tr -d ' ')/" \
    -e "s/__KERNEL_WASM_BYTES__/$(wc -c < dist/web/worker/skraft_kernel_worker_bg.wasm | tr -d ' ')/" \
    web/index.html > dist/web/index.html
echo "dist/web prêt : python3 -m http.server 8080 -d dist/web  →  http://localhost:8080"
