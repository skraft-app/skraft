#!/usr/bin/env bash
# Writes the third-party notices the installers ship beside LICENSE: every crate the app
# links, with its license's text (cargo-about, about.toml + about.hbs), then the fonts the app
# embeds (apps/skraft/assets/fonts/<font>/, each with its license file: LICENSE or OFL.txt).
# Fails when a crate's license is not one about.toml accepts, or a font has no license file.
#
#   scripts/third-party-licenses.sh [<out>]     (default: dist/THIRD-PARTY-LICENSES.txt)
#
# Needs cargo-about (brew install cargo-about, or cargo install --locked cargo-about).
set -euo pipefail
cd "$(dirname "$0")/.."

out="${1:-dist/THIRD-PARTY-LICENSES.txt}"
command -v cargo-about > /dev/null || {
  echo "cargo-about is missing: brew install cargo-about (or cargo install --locked cargo-about)" >&2
  exit 1
}

# Each font folder's license file.
fonts=()
for dir in apps/skraft/assets/fonts/*/; do
  license=""
  for name in LICENSE OFL.txt; do [ -f "$dir$name" ] && license="$dir$name"; done
  [ -n "$license" ] || { echo "font $dir has no license file (LICENSE or OFL.txt)" >&2; exit 1; }
  fonts+=("$license")
done

mkdir -p "$(dirname "$out")"
cargo about generate --locked --fail -m apps/skraft/Cargo.toml about.hbs -o "$out"

# SolidCore's crates that are not published: cargo-about skips those as private, like ours.
{
  echo "================================================================================"
  echo "SolidCore crates not published on crates.io"
  echo "(https://github.com/tangency-labs/solidcore), Apache License 2.0, its text above:"
  echo
  awk '/^name = /{ name = $3 } /^version = /{ version = $3 }
       /^source = "git\+https:\/\/github.com\/tangency-labs\/solidcore/ { gsub(/"/, "", name); gsub(/"/, "", version); print name, version }' Cargo.lock |
    while read -r name version; do
      grep -q "^- $name $version " "$out" || echo "- $name $version"
    done
  echo
} >> "$out"

# The fonts, those with the same license text in one block.
{
  echo "================================================================================"
  echo "Fonts embedded in Skraft"
  done_list=" "
  for license in "${fonts[@]}"; do
    case "$done_list" in *" $license "*) continue ;; esac
    echo
    echo "--------------------------------------------------------------------------------"
    echo "Used by:"
    for other in "${fonts[@]}"; do
      if cmp -s "$license" "$other"; then
        echo "- $(basename "$(dirname "$other")")"
        done_list="$done_list$other "
      fi
    done
    echo
    cat "$license"
  done
} >> "$out"

# Code adapted from a project the app does not link, with its license.
{
  echo
  echo "================================================================================"
  echo "Code adapted from other projects"
  echo
  echo "--------------------------------------------------------------------------------"
  echo "SolidCraft (https://github.com/tangency-labs/solidcraft), MIT license:"
  echo "parts of Skraft's rendering, control protocol and Prism tool (its line-to-arc gesture"
  echo "and outline offset)."
  echo
  cat licenses/solidcraft-MIT.txt
} >> "$out"
echo "$out"
