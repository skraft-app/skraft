#!/usr/bin/env bash
# Prints the CHANGELOG.md entry of a version (its `## [<version>]` section, heading left
# out): the GitHub Release's text. Fails when the entry is missing or empty, so a release is
# never published without its notes.
#
#   scripts/release-notes.sh [<version>]     (default: scripts/version.sh)
set -euo pipefail
cd "$(dirname "$0")/.."

version="${1:-$(scripts/version.sh)}"
version="${version#v}"

# The section's lines, the blank ones before and after it left out.
notes="$(awk -v v="$version" '
  /^## \[/ { if (inside) exit; inside = index($0, "## [" v "]") == 1; next }
  inside { line[++n] = $0; if ($0 ~ /[^[:space:]]/) { if (!first) first = n; last = n } }
  END { for (i = first; first && i <= last; i++) print line[i] }
' CHANGELOG.md)"

if [ -z "$notes" ]; then
  echo "CHANGELOG.md has no entry for $version (a \`## [$version] - <date>\` section)" >&2
  exit 1
fi
printf '%s\n' "$notes"
