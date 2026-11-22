#!/usr/bin/env bash
# Removes a worktree made by scripts/new-worktree.sh, ../skraft-<name>, its build cache
# with it, then its branch:
#
#   scripts/drop-worktree.sh <name> [--unmerged]
#
#   name         the worktree's suffix: ../skraft-<name>
#   --unmerged   its branch goes even if main does not hold it (work given up); without it, a
#                branch not merged into main stops everything
#
# Refused, nothing removed: changes not committed in the worktree (commit them, or remove it
# by hand with `git worktree remove --force`), or a process running from it (an app, a build,
# an MCP server: stop it first).
set -euo pipefail

usage() {
  sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

name="" unmerged=false
for arg in "$@"; do
  case "$arg" in
    --unmerged) unmerged=true ;;
    -h | --help) usage 0 ;;
    -*) echo "unknown option: $arg" >&2; usage 2 >&2 ;;
    *)
      if [ -z "$name" ]; then name="$arg"
      else echo "too many arguments" >&2; usage 2 >&2
      fi
      ;;
  esac
done
[ -n "$name" ] || usage 2 >&2

# The main worktree (the repository's own folder), wherever this is run from.
common="$(cd "$(dirname "$0")" && git rev-parse --path-format=absolute --git-common-dir)"
main="$(dirname "$common")"
dest="$(dirname "$main")/$(basename "$main")-$name"

# 1. A worktree of this repository, not the main one.
git -C "$main" worktree list --porcelain | grep -qxF "worktree $dest" || { echo "$dest is not a worktree of $main" >&2; exit 1; }
[ "$dest" != "$main" ] || { echo "that is the main worktree" >&2; exit 1; }
branch="$(git -C "$dest" branch --show-current)"

# 2. Nothing left behind: no change not committed (the ignored build cache aside)…
if [ -n "$(git -C "$dest" status --porcelain)" ]; then
  echo "$dest has changes not committed:" >&2
  git -C "$dest" status --short >&2
  exit 1
fi
# …its branch in main…
if [ -n "$branch" ] && ! $unmerged && ! git -C "$main" merge-base --is-ancestor "$branch" main; then
  echo "$branch is not merged into main:" >&2
  git -C "$main" log --oneline "main..$branch" >&2
  echo "(--unmerged to drop it all the same)" >&2
  exit 1
fi
# …and nothing running from it: named on its command line, or its executable or current folder
# in it (a relative launch, `./target/debug/skraft`, names neither). Neither this script and
# what launched it, nor an idle shell left in the folder.
running() {
  local real p pid skip=" "
  real="$(cd -P "$dest" && pwd -P)"
  p=$$
  while [ -n "$p" ] && [ "$p" -gt 1 ]; do
    skip="$skip$p "
    p="$(ps -o ppid= -p "$p" | tr -d ' ' || true)"
  done
  for pid in $({
    pgrep -f "$dest/" || true
    if command -v lsof >/dev/null; then
      lsof -w -n -P -d cwd,txt -Fpcn 2>/dev/null | awk -v d="$real" '
        /^p/ { pid = substr($0, 2) }
        /^c/ { cmd = substr($0, 2); sub(/^-/, "", cmd) }
        /^n/ {
          path = substr($0, 2)
          if ((path == d || index(path, d "/") == 1) && cmd !~ /^(sh|bash|zsh|fish|dash)$/) print pid
        }' || true
    fi
  } | sort -un); do
    case "$skip" in *" $pid "*) continue ;; esac
    # Still there: not one of the subshells of this very search.
    if kill -0 "$pid" 2>/dev/null; then echo "$pid"; fi
  done
}
pids="$(running)"
if [ -n "$pids" ]; then
  echo "processes run from $dest (stop them first):" >&2
  for pid in $pids; do
    ps -o pid=,command= -p "$pid" | cut -c1-160 >&2 || true
  done
  exit 1
fi

# 3. The worktree (its target/ with it), then its branch.
git -C "$main" worktree remove "$dest"
# Merged into main or given up, as checked above (`-d` would ask the branch checked out in the
# main worktree, maybe not main).
[ -z "$branch" ] || git -C "$main" branch -D "$branch"
echo "$dest removed."
