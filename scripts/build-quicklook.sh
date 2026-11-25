#!/usr/bin/env bash
# Builds Skraft.app's Quick Look extensions into its Contents/PlugIns, each signed ad hoc and
# sandboxed (packaging/macos/quicklook/): the thumbnail Finder shows for a .skraft file and its
# preview on Space, both the picture the file carries (docs/io/file-format.md). Called by
# scripts/package-macos.sh before the app itself is signed, which must not be --deep: that
# would sign the extensions again without their entitlements.
#   scripts/build-quicklook.sh <arm64|x86_64> <path/to/Skraft.app>
set -euo pipefail
cd "$(dirname "$0")/.."
arch="${1:?usage: build-quicklook.sh <arm64|x86_64> <Skraft.app>}"
app="${2:?usage: build-quicklook.sh <arm64|x86_64> <Skraft.app>}"
version="$(scripts/version.sh)"
src=packaging/macos/quicklook

for kind in Thumbnail Preview; do
  case $kind in
    Thumbnail) min=11.0; frameworks=(-framework QuickLookThumbnailing -framework AppKit) ;;
    Preview) min=12.0; frameworks=(-framework QuickLookUI -framework AppKit) ;;
  esac
  appex="$app/Contents/PlugIns/Skraft$kind.appex"
  mkdir -p "$appex/Contents/MacOS"
  sed "s/@VERSION@/$version/g" "$src/$kind-Info.plist" > "$appex/Contents/Info.plist"
  # An extension's entry point is the system's: no main of its own.
  xcrun swiftc -O -target "$arch-apple-macos$min" -module-name "Skraft$kind" -parse-as-library \
    -application-extension "${frameworks[@]}" -Xlinker -e -Xlinker _NSExtensionMain \
    "$src/FilePicture.swift" "$src/${kind}Provider.swift" -o "$appex/Contents/MacOS/Skraft$kind"
  codesign --force --sign - --entitlements "$src/extension.entitlements" "$appex"
done
