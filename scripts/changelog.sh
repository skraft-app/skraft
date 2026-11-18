#!/usr/bin/env bash
# Prints the raw material of a CHANGELOG.md entry: what main received between two refs, one
# block per merged branch with the subjects of its commits, then the commits made on main
# directly. A draft to write the user-facing entry from, not the entry itself:
#
#   scripts/changelog.sh [<from> [<to>]]
#
#   from   the previous release (default: the latest v* tag reachable from <to>, else the
#          whole history)
#   to     the release being written (default: HEAD)
#
# Branches are grouped by their prefix, a hint for the entry's sections: feature/ (Added or
# Changed), fix/ (Fixed), others. A commit's full message: git show -s <sha>.
set -euo pipefail
cd "$(dirname "$0")/.."

case "${1:-}" in -h | --help) sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; exit 0 ;; esac

to="${2:-HEAD}"
if [ -n "${1:-}" ]; then
  from="$1"
else
  from="$(git describe --tags --abbrev=0 --match 'v*' "$to" 2>/dev/null || true)"
fi
range="${from:+$from..}$to"

echo "# Changes ${from:-from the start} → $to"

# First-parent history of main: merges are the branches, the rest was committed on main.
merges="$(git log --first-parent --merges --reverse --format='%H' "$range")"
direct="$(git log --first-parent --no-merges --reverse --format='%h %as %s' "$range")"

for group in feature fix other; do
  printed=false
  for merge in $merges; do
    subject="$(git log -1 --format='%s' "$merge")"
    branch="$(sed -n "s/^Merge branch '\([^']*\)'.*/\1/p" <<<"$subject")"
    branch="${branch:-$subject}"
    case "$branch" in
      feature/*) kind=feature ;;
      fix/*) kind=fix ;;
      *) kind=other ;;
    esac
    [ "$kind" = "$group" ] || continue
    if ! $printed; then
      case "$group" in
        feature) echo; echo "## Feature branches (Added / Changed)" ;;
        fix) echo; echo "## Fix branches (Fixed)" ;;
        other) echo; echo "## Other branches" ;;
      esac
      printed=true
    fi
    echo
    echo "### $branch ($(git log -1 --format='%h %as' "$merge"))"
    git log --reverse --no-merges --format='- %h %s' "$merge^1..$merge^2"
  done
done

if [ -n "$direct" ]; then
  echo
  echo "## Commits on main"
  echo
  sed 's/^/- /' <<<"$direct"
fi
