#!/usr/bin/env bash
# Packages a release build for Linux into dist/release/SolidSketch-<version>-linux-<arch>.AppImage
# (linuxdeploy, downloaded into target/tools on first use).
#   scripts/package-linux.sh x86_64-unknown-linux-gnu   (after cargo build --release --target …)
set -euo pipefail
cd "$(dirname "$0")/.."
target="${1:?usage: package-linux.sh <target>}"
version="$(scripts/version.sh)"
arch="${target%%-*}"
exe="target/$target/release/solidsketch"
[ -f "$exe" ] || { echo "no $exe: build it first" >&2; exit 1; }

tool="target/tools/linuxdeploy-$arch.AppImage"
if [ ! -x "$tool" ]; then
  mkdir -p target/tools
  curl -fsSL -o "$tool" "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$arch.AppImage"
  chmod +x "$tool"
fi
tool="$(pwd)/$tool"

work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
cp apps/solidsketch/assets/app-icon.svg "$work/solidsketch.svg"
mkdir -p dist/release
out="$(pwd)/dist/release/SolidSketch-$version-linux-$arch.AppImage"
rm -f "$out"
# No FUSE on CI runners: the tool unpacks itself instead of mounting.
(cd "$work" && APPIMAGE_EXTRACT_AND_RUN=1 LDAI_OUTPUT="$out" OUTPUT="$out" VERSION="$version" "$tool" \
  --appdir AppDir --executable "$OLDPWD/$exe" --desktop-file "$OLDPWD/packaging/linux/solidsketch.desktop" \
  --icon-file solidsketch.svg --output appimage)
[ -f "$out" ] || mv "$work"/SolidSketch*.AppImage "$out"
echo "$out"
