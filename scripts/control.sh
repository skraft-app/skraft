#!/usr/bin/env bash
# Calls a method of the running app's control channel (docs/ARCHITECTURE.md §7 septies):
#
#   scripts/control.sh model.geometry '{"selection": true}'
#   scripts/control.sh app.wait '{"idle": true}'
#   scripts/control.sh app.screenshot > view.png
#
# Finds the app by its discovery file (`current.port`) and its token file, in
# SolidSketch/control under the user's data folder; SOLIDSKETCH_CONTROL_DIR, _PORT and _TOKEN
# override them. The channel must be on (title bar, or `SOLIDSKETCH_CONTROL=1` at launch).
set -euo pipefail
method="${1:?usage: control.sh <method> [params as JSON]}"
params="${2:-null}"

case "$(uname -s)" in
  Darwin) data="$HOME/Library/Application Support" ;;
  MINGW* | MSYS* | CYGWIN*) data="${APPDATA:-$HOME/AppData/Roaming}" ;;
  *) data="${XDG_DATA_HOME:-$HOME/.local/share}" ;;
esac
dir="${SOLIDSKETCH_CONTROL_DIR:-$data/SolidSketch/control}"
port="${SOLIDSKETCH_CONTROL_PORT:-$(cat "$dir/current.port" 2>/dev/null || true)}"
if [ -z "$port" ]; then
  echo "no running SolidSketch with its control channel on (no $dir/current.port)" >&2
  exit 1
fi
token="${SOLIDSKETCH_CONTROL_TOKEN:-$(cat "$dir/$port.token" 2>/dev/null || true)}"
if [ -z "$token" ]; then
  echo "no token for port $port (no $dir/$port.token)" >&2
  exit 1
fi

curl -sS -X POST "http://127.0.0.1:$port/call" \
  -H 'X-SolidSketch: 1' \
  -H "Authorization: Bearer $token" \
  -H 'Content-Type: application/json' \
  --data-binary "{\"method\": \"$method\", \"params\": $params}"
