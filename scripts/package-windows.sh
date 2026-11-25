#!/usr/bin/env bash
# Packages a release build for Windows into dist/release/: Skraft-<version>-windows-<arch>.msi
# (WiX 5, packaging/windows/skraft.wxs) and the bare executable zipped (no install), each with
# LICENSE.txt and THIRD-PARTY-LICENSES.txt.
# Runs in Git Bash on Windows, the WiX tool and its UI extension installed (dotnet tool install
# --global wix --version 5.0.2; wix extension add --global WixToolset.UI.wixext/5.0.2) and
# cargo-about (scripts/third-party-licenses.sh).
#   scripts/package-windows.sh x86_64-pc-windows-msvc   (after cargo build --release --target …)
set -euo pipefail
cd "$(dirname "$0")/.."
target="${1:?usage: package-windows.sh <target>}"
version="$(scripts/version.sh)"
arch="${target%%-*}"
case "$arch" in x86_64) wix_arch=x64 ;; aarch64) wix_arch=arm64 ;; *) echo "unknown arch $arch" >&2; exit 1 ;; esac
exe="target/$target/release/skraft.exe"
[ -f "$exe" ] || { echo "no $exe: build it first" >&2; exit 1; }

# A path for wix.exe: Git Bash's /tmp/… means nothing to it.
native() { if command -v cygpath > /dev/null; then cygpath -w "$1"; else echo "$1"; fi; }

work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
cp LICENSE "$work/LICENSE.txt"
scripts/third-party-licenses.sh "$work/THIRD-PARTY-LICENSES.txt" > /dev/null

mkdir -p dist/release
name="Skraft-$version-windows-$arch"
wix build packaging/windows/skraft.wxs -arch "$wix_arch" -ext WixToolset.UI.wixext \
  -d Version="$version" -d ExeFile="$exe" -d IconFile=apps/skraft/assets/app-icon.ico \
  -d DialogBmp=packaging/windows/dialog.bmp -d BannerBmp=packaging/windows/banner.bmp \
  -d LicenseFile="$(native "$work/LICENSE.txt")" -d NoticesFile="$(native "$work/THIRD-PARTY-LICENSES.txt")" \
  -o "dist/release/$name.msi"
rm -f "dist/release/$name.msi.wixpdb" "dist/release/$name.wixpdb"

# The portable executable.
cp "$exe" "$work/Skraft.exe"
rm -f "dist/release/$name.zip"
(cd "$work" && 7z a -tzip -bso0 "$OLDPWD/dist/release/$name.zip" Skraft.exe LICENSE.txt THIRD-PARTY-LICENSES.txt)
ls dist/release
