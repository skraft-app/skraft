#!/usr/bin/env bash
# Packages a release build for Windows into dist/release/: SolidSketch-<version>-windows-<arch>.msi
# (WiX 5, packaging/windows/solidsketch.wxs) and the bare executable zipped (no install).
# Runs in Git Bash on Windows, the WiX tool installed (dotnet tool install --global wix --version 5.0.2).
#   scripts/package-windows.sh x86_64-pc-windows-msvc   (after cargo build --release --target …)
set -euo pipefail
cd "$(dirname "$0")/.."
target="${1:?usage: package-windows.sh <target>}"
version="$(scripts/version.sh)"
arch="${target%%-*}"
case "$arch" in x86_64) wix_arch=x64 ;; aarch64) wix_arch=arm64 ;; *) echo "unknown arch $arch" >&2; exit 1 ;; esac
exe="target/$target/release/solidsketch.exe"
[ -f "$exe" ] || { echo "no $exe: build it first" >&2; exit 1; }

mkdir -p dist/release
name="SolidSketch-$version-windows-$arch"
wix build packaging/windows/solidsketch.wxs -arch "$wix_arch" \
  -d Version="$version" -d ExeFile="$exe" -d IconFile=apps/solidsketch/assets/app-icon.ico \
  -o "dist/release/$name.msi"
rm -f "dist/release/$name.msi.wixpdb" "dist/release/$name.wixpdb"

# The portable executable.
work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
cp "$exe" "$work/SolidSketch.exe"
rm -f "dist/release/$name.zip"
(cd "$work" && 7z a -tzip -bso0 "$OLDPWD/dist/release/$name.zip" SolidSketch.exe)
ls dist/release
