#!/usr/bin/env bash
# Packages a release build for macOS into dist/release/Skraft-<version>-macos-<arch>.dmg:
# Skraft.app (ad-hoc signed, not notarized) and a link to /Applications to drop it on.
#   scripts/package-macos.sh aarch64-apple-darwin   (after cargo build --release --target …)
set -euo pipefail
cd "$(dirname "$0")/.."
target="${1:?usage: package-macos.sh <target>}"
version="$(scripts/version.sh)"
arch="${target%%-*}"; [ "$arch" = "aarch64" ] && arch="arm64"
exe="target/$target/release/skraft"
[ -f "$exe" ] || { echo "no $exe: build it first" >&2; exit 1; }

work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
app="$work/dmg/Skraft.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$exe" "$app/Contents/MacOS/skraft"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"

# The icon set from the 1024 px icon.
iconset="$work/Skraft.iconset"; mkdir "$iconset"
for size in 16 32 128 256 512; do
  sips -z $size $size apps/skraft/assets/app-icon.png --out "$iconset/icon_${size}x${size}.png" > /dev/null
  sips -z $((size * 2)) $((size * 2)) apps/skraft/assets/app-icon.png --out "$iconset/icon_${size}x${size}@2x.png" > /dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Skraft.icns"

codesign --force --deep --sign - "$app"
ln -s /Applications "$work/dmg/Applications"

mkdir -p dist/release
out="dist/release/Skraft-$version-macos-$arch.dmg"
rm -f "$out"
hdiutil create -volname "Skraft $version" -srcfolder "$work/dmg" -fs HFS+ -format UDZO -ov "$out" > /dev/null
echo "$out"
