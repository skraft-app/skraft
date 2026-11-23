#!/usr/bin/env bash
# Prints a release's latest.json, which the desktop app reads to find a new version
# (docs/ui/updates.md): the version, its CHANGELOG.md entry and each platform's installer among
# the files of <dir>. A platform not built is left out: the app then opens the release's page.
#
#   scripts/update-manifest.sh <dir> [<version>]     (default: scripts/version.sh)
set -euo pipefail

dir="${1:?usage: update-manifest.sh <dir> [<version>]}"
root="$(cd "$(dirname "$0")/.." && pwd)"
version="${2:-$("$root/scripts/version.sh")}"
version="${version#v}"
notes="$("$root/scripts/release-notes.sh" "$version")"
base="https://github.com/skraft-app/skraft/releases/download/v$version"

assets='{}'
for entry in macos-arm64:dmg macos-x86_64:dmg windows-x86_64:msi linux-x86_64:AppImage; do
  platform="${entry%%:*}"
  file="Skraft-$version-$platform.${entry##*:}"
  if [ -f "$dir/$file" ]; then
    assets="$(jq -c --arg k "$platform" --arg v "$base/$file" '. + {($k): $v}' <<<"$assets")"
  fi
done
if [ "$assets" = '{}' ]; then
  echo "no installer of $version in $dir" >&2
  exit 1
fi

jq -n --arg version "$version" --arg notes "$notes" --argjson assets "$assets" \
  '{version: $version, notes: $notes, assets: $assets}'
