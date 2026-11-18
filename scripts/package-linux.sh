#!/usr/bin/env bash
# Packages a release build for Linux into dist/release/Skraft-<version>-linux-<arch>.AppImage
# (linuxdeploy, downloaded into target/tools on first use), LICENSE and THIRD-PARTY-LICENSES.txt
# in its usr/share/doc/skraft (the latter from cargo-about: scripts/third-party-licenses.sh).
#   scripts/package-linux.sh x86_64-unknown-linux-gnu   (after cargo build --release --target …)
set -euo pipefail
cd "$(dirname "$0")/.."
target="${1:?usage: package-linux.sh <target>}"
version="$(scripts/version.sh)"
arch="${target%%-*}"
exe="target/$target/release/skraft"
[ -f "$exe" ] || { echo "no $exe: build it first" >&2; exit 1; }

tool="target/tools/linuxdeploy-$arch.AppImage"
if [ ! -x "$tool" ]; then
  mkdir -p target/tools
  curl -fsSL -o "$tool" "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$arch.AppImage"
  chmod +x "$tool"
fi
tool="$(pwd)/$tool"

work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
cp apps/skraft/assets/app-icon.svg "$work/skraft.svg"
# The .skraft documents' MIME type, for the desktop integration (appimaged, AppImageLauncher).
mkdir -p "$work/AppDir/usr/share/mime/packages"
cp packaging/linux/skraft.xml "$work/AppDir/usr/share/mime/packages/"
mkdir -p "$work/AppDir/usr/share/doc/skraft"
cp LICENSE "$work/AppDir/usr/share/doc/skraft/"
scripts/third-party-licenses.sh "$work/AppDir/usr/share/doc/skraft/THIRD-PARTY-LICENSES.txt" > /dev/null
mkdir -p dist/release
out="$(pwd)/dist/release/Skraft-$version-linux-$arch.AppImage"
rm -f "$out"
# No FUSE on CI runners: the tool unpacks itself instead of mounting.
(cd "$work" && APPIMAGE_EXTRACT_AND_RUN=1 LDAI_OUTPUT="$out" OUTPUT="$out" VERSION="$version" "$tool" \
  --appdir AppDir --executable "$OLDPWD/$exe" --desktop-file "$OLDPWD/packaging/linux/skraft.desktop" \
  --icon-file skraft.svg --output appimage)
[ -f "$out" ] || mv "$work"/Skraft*.AppImage "$out"
echo "$out"
