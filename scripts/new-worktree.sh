#!/usr/bin/env bash
# Creates a worktree beside the repository, ../skraft-<name>, on a new branch from main,
# with main's build cache cloned so its first build compiles our crates only, not the
# dependencies:
#
#   scripts/new-worktree.sh <name> [branch] [--build]
#
#   name      the worktree's suffix (letters, digits, -): ../skraft-<name>
#   branch    the branch to make from main (default: feature/<name>); an existing one is
#             checked out as it is
#   --build   then `cargo build --workspace` in it, so it is ready
#
# The cache is cloned (copy-on-write on APFS: instant, no space until files differ), not
# shared: each worktree builds on its own, in parallel with the others. Two traps:
#   - main's target/ is cloned only while nothing compiles there (else the cache is half
#     written): the script waits for that build to end.
#   - Cargo judges freshness by modification times, and the clone keeps main's: it must be made
#     before any file of the worktree is edited (here, right after the checkout, whose files
#     are all newer), else a file edited earlier than main's last build looks built already.
#     Build scripts are touched all the same.
# Then build and test with `--workspace`: `-p <crate>` unifies the dependencies' features
# differently, and they (SolidCore included) are compiled again from scratch.
set -euo pipefail

usage() {
  sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

name="" branch="" build=false
for arg in "$@"; do
  case "$arg" in
    --build) build=true ;;
    -h | --help) usage 0 ;;
    -*) echo "unknown option: $arg" >&2; usage 2 >&2 ;;
    *)
      if [ -z "$name" ]; then name="$arg"
      elif [ -z "$branch" ]; then branch="$arg"
      else echo "too many arguments" >&2; usage 2 >&2
      fi
      ;;
  esac
done
[ -n "$name" ] || usage 2 >&2
[[ "$name" =~ ^[A-Za-z0-9][A-Za-z0-9-]*$ ]] || { echo "bad name: $name (letters, digits, -)" >&2; exit 2; }
branch="${branch:-feature/$name}"

# The main worktree (the repository's own folder), wherever this is run from.
common="$(cd "$(dirname "$0")" && git rev-parse --path-format=absolute --git-common-dir)"
main="$(dirname "$common")"
dest="$(dirname "$main")/$(basename "$main")-$name"
[ ! -e "$dest" ] || { echo "$dest exists already" >&2; exit 1; }

# 1. Nothing compiling in main: rustc writing into its target/, or a build script of it (not
#    the app or the MCP server running from target/debug).
compiling() {
  pgrep -f "rustc .*$main/target/" >/dev/null || pgrep -f "$main/target/[^ ]*/build/[^ ]*/build-script-build" >/dev/null
}
waited=false
while compiling; do
  $waited || echo "main is compiling: waiting for it to end before cloning its cache (Ctrl-C to give up)…"
  waited=true
  sleep 5
done

# 2. The worktree, on its branch.
if git -C "$main" show-ref --verify --quiet "refs/heads/$branch"; then
  git -C "$main" worktree add "$dest" "$branch"
else
  git -C "$main" worktree add -b "$branch" "$dest" main
fi

# 3. Main's cache cloned (but its scratch folder), copy-on-write where the file system can.
if [ -d "$main/target" ]; then
  mkdir "$dest/target"
  for entry in "$main/target"/* "$main/target"/.[!.]*; do
    [ -e "$entry" ] || continue
    [ "$(basename "$entry")" = "tmp" ] && continue
    case "$(uname -s)" in
      Darwin) cp -cRp "$entry" "$dest/target/" ;;
      *) cp -a --reflink=auto "$entry" "$dest/target/" ;;
    esac
  done
  # Build scripts run again (the catalog of texts, the build's identity), whatever the times.
  (cd "$dest" && find . -name build.rs -not -path "./target/*" -exec touch {} +)
  echo "main's build cache cloned into $dest/target"
else
  echo "main has no target/ yet: nothing to clone, the first build compiles everything"
fi

# 4. Ready to build?
if $build; then
  (cd "$dest" && cargo build --workspace)
fi

echo
echo "Worktree $dest on $branch. Build and test there with --workspace (not -p <crate>)."
