#!/usr/bin/env bash
# Prints the app's version: the workspace's (the first `version =` of the root Cargo.toml).
set -euo pipefail
cd "$(dirname "$0")/.."
sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1
