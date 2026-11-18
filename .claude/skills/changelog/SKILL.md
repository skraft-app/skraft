---
name: changelog
description: Write the CHANGELOG.md entry of the next Skraft release from the commits since the last v* tag, for users, in English. Use when the user prepares a release / a version tag, or asks for the changelog or release notes.
argument-hint: "[version]"
---

# Writing a release's CHANGELOG.md entry

The entry is for **Skraft's users**, in **English**: what they can now do, what behaves
differently, what was broken and works now. Not how it was done.

## Steps

1. **Version.** The argument if given, else ask (patch / minor / major from the current
   `scripts/version.sh`). The tag will be `v<version>`; the workflow refuses a tag that
   differs from the workspace version of `Cargo.toml`.
2. **Raw material.** `scripts/changelog.sh` (from the latest `v*` tag to HEAD): one block per
   merged branch with its commits' subjects. Read a commit's full message with
   `git show -s <sha>` when the subject is not enough; read the diff only when the message
   still leaves the user-visible effect unclear.
3. **Write the entry** at the top of the entries in `CHANGELOG.md`:

   ```markdown
   ## [0.2.0] - 2026-12-01

   ### Added
   - Copy tools: copy along a line or a grid, or distribute copies between two points.

   ### Changed
   - …

   ### Fixed
   - …
   ```

   - Sections, in this order, only those with items: Added, Changed, Deprecated, Removed,
     Fixed. A `feature/` branch is Added (a new tool, option, command) or Changed (an existing
     behaviour); a `fix/` branch is Fixed.
   - **One item per thing the user notices**, not per branch or commit: several branches on
     one subject make one item; internal work (refactors, tests, scripts, CI, docs, perf with
     no felt effect, dependency bumps) is left out unless the user feels it.
   - Plain words any user of the app understands: Skraft is a simple and intuitive general 3D
     modeller on real solids (SolidCore), not a woodworking-only tool. Commands in bold, by
     their name in the English UI (`locales/en.yml`) when there is one; while the UI is French
     only (`locales/fr.yml`), by their English meaning followed by the French label the user
     sees: **Copy along a line** (« Copier en ligne »). No branch names, crate names, file
     names or commit hashes.
   - The most important items first in each section. One line each, a second sentence only
     when needed.
   - 0.1.0, the first release, is an overview paragraph only (already in `CHANGELOG.md`;
     re-date it at tagging): its history is not listed.
4. **Date** = the day of the tag (today unless told otherwise).
5. **Version bump**, if the workspace version is not already `<version>`: `version` in the
   root `Cargo.toml`, then `cargo update --workspace` for `Cargo.lock`.
6. **Check** `scripts/release-notes.sh <version>` prints the entry (it is the GitHub
   Release's text).
7. **Hand over.** Show the entry to the user for review. Never commit or tag: the user does
   it, e.g. `git tag v<version> && git push origin v<version>`.
