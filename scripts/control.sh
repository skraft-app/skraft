#!/usr/bin/env bash
# Calls a method of a running app's control channel (docs/ARCHITECTURE.md §7 septies):
#
#   scripts/control.sh model.geometry '{"selection": true}'
#   scripts/control.sh app.wait '{"idle": true}'
#   scripts/control.sh app.screenshot > view.png
#   scripts/control.sh instances        the apps running: pid, port, worktree, commit, launcher
#
# Several apps may run (one per worktree). The one called: SOLIDSKETCH_CONTROL_PORT's if set,
# else the one this Claude Code session launched (CLAUDE_CODE_SESSION_ID), else one built from
# the worktree this is run in, else the last started (the discovery file `current.port`) —
# from the registry, SolidSketch/control/instances under the user's data folder.
# SOLIDSKETCH_CONTROL_DIR and _TOKEN override the folder and the token. The channel must be on
# (title bar, or `SOLIDSKETCH_CONTROL=1` at launch). The app's journal shows the calls as the
# pilot's, named `control.sh · session <id> · <worktree>`.
set -euo pipefail
method="${1:?usage: control.sh <method> [params as JSON] | control.sh instances}"
params="${2:-null}"

case "$(uname -s)" in
  Darwin) data="$HOME/Library/Application Support" ;;
  MINGW* | MSYS* | CYGWIN*) data="${APPDATA:-$HOME/AppData/Roaming}" ;;
  *) data="${XDG_DATA_HOME:-$HOME/.local/share}" ;;
esac
dir="${SOLIDSKETCH_CONTROL_DIR:-$data/SolidSketch/control}"
registry="$dir/instances"

# A field of a registry file (pretty JSON, one field per line); the build's are one level
# deeper.
field() { sed -n "s/^  \"$2\": \"\{0,1\}\([^\",]*\)\"\{0,1\},\{0,1\}$/\1/p" "$1" | head -n 1; }
build_field() { sed -n "s/^    \"$2\": \"\{0,1\}\([^\",]*\)\"\{0,1\},\{0,1\}$/\1/p" "$1" | head -n 1; }
# The live apps' registry files, the last started first.
live() {
  [ -d "$registry" ] || return 0
  for f in "$registry"/*.json; do
    [ -e "$f" ] || continue
    kill -0 "$(basename "$f" .json)" 2>/dev/null && echo "$(field "$f" started) $f"
  done | sort -rn | cut -d' ' -f2-
}

if [ "$method" = instances ]; then
  printf '%-7s %-6s %-32s %-9s %-9s %s\n' PID PORT WORKTREE COMMIT SESSION LAUNCHER
  live | while read -r f; do
    worktree="$(build_field "$f" worktree)"
    commit="$(build_field "$f" commit)"
    [ "$(build_field "$f" dirty)" = true ] && commit="$commit*"
    session="$(field "$f" claude_session)"
    session="${session/null/-}"
    port="$(field "$f" port)"
    printf '%-7s %-6s %-32s %-9s %-9s %s\n' "$(field "$f" pid)" "${port/null/-}" "$(basename "${worktree:-?}")" \
      "${commit:--}" "${session:0:8}" "$(field "$f" launcher | sed 's/^null$//')"
  done
  exit 0
fi

toplevel="$(git rev-parse --show-toplevel 2>/dev/null || true)"
port="${SOLIDSKETCH_CONTROL_PORT:-}"
if [ -z "$port" ]; then
  files="$(live)"
  if [ -n "${CLAUDE_CODE_SESSION_ID:-}" ]; then
    while read -r f; do
      [ -n "$f" ] || continue
      p="$(field "$f" port)"
      if [ "$(field "$f" claude_session)" = "$CLAUDE_CODE_SESSION_ID" ] && [ "$p" != null ]; then port="$p"; break; fi
    done <<< "$files"
  fi
  if [ -z "$port" ] && [ -n "$toplevel" ]; then
    while read -r f; do
      [ -n "$f" ] || continue
      p="$(field "$f" port)"
      if [ "$(build_field "$f" worktree)" = "$toplevel" ] && [ "$p" != null ]; then port="$p"; break; fi
    done <<< "$files"
  fi
fi
port="${port:-$(cat "$dir/current.port" 2>/dev/null || true)}"
if [ -z "$port" ]; then
  echo "no running SolidSketch with its control channel on (no $dir/current.port)" >&2
  exit 1
fi
token="${SOLIDSKETCH_CONTROL_TOKEN:-$(cat "$dir/$port.token" 2>/dev/null || true)}"
if [ -z "$token" ]; then
  echo "no token for port $port (no $dir/$port.token)" >&2
  exit 1
fi

client="control.sh"
[ -n "${CLAUDE_CODE_SESSION_ID:-}" ] && client="$client · session ${CLAUDE_CODE_SESSION_ID:0:8}"
label="$(basename "${toplevel:-x}")"
[ "${label#solidsketch-}" != "$label" ] && client="$client · ${label#solidsketch-}"

curl -sS -X POST "http://127.0.0.1:$port/call" \
  -H 'X-SolidSketch: 1' \
  -H "Authorization: Bearer $token" \
  -H "X-SolidSketch-Client: $client" \
  -H 'Content-Type: application/json' \
  --data-binary "{\"method\": \"$method\", \"params\": $params}"
