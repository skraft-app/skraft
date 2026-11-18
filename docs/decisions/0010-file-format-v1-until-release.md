# File format version 1 until the first release

- Status: accepted (2026-11-12)
- Design: [File format](../io/file-format.md)

## Context

The file format had already gone through versions 2 and 3, and the first model of the
document had a converter to the next. The app is not published yet.

## Decision

The format stays at version 1 until the app is published: it changes without a new version and
older files are not read — no converter, no reference files. Versioning and backward
compatibility start with the first release.

## Alternatives considered

- Versioning every change with converters and reference files (the practice until then: files
  written by versions 2 and 3, the first model's converter).

## Consequences

- Files written before by versions 2 and 3, and the first model's converter, were dropped.
- The documents of the app's former name (`.ssk`, `"format": "solidsketch"`) are not read
  either ([README](../README.md)).
