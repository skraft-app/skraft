# Updates

The desktop app says when a newer version exists and opens its installer in the browser. It does
not install anything ([decision 0053](../decisions/0053-update-check-without-self-install.md)).
The web version is always the latest and has none of this.

## The check

Five seconds after the launch, on a thread, at most once in 24 hours, the app reads
`releases/latest/download/latest.json`. A failure is only logged, and the next launch tries again.
What it found is kept in the preferences, so a launch that does not check still shows it. Debug
builds do not check by themselves, unless `SKRAFT_UPDATE_URL` names a manifest to try.

« Vérifier les mises à jour » in the general preferences turns the automatic check off. The command
`app.updates` (palette, a button in « À propos »; in no menu) always works and tells the outcome in
the same box: the search, then « à jour » or the failure, closed by **OK**.

## What the user sees

A newer version shows a **magenta badge in the title bar**, « 0.0.3 disponible », after the app's
version (and a dev build's badge), and **one toast** per version. Either opens a **modal box**,
laid out as the save prompt's: the version, the release's notes, then **Ignorer cette version**
alone on the left, **Plus tard** and **Télécharger** (filled, Enter; this platform's installer,
in the browser) on the right; Esc is « Plus tard ». An ignored version loses its badge and toast;
the next one has them again.

**Why a modal box:** it asks a question and waits for its answer, as the save prompt; its veil
tells it from the window it was asked from (« À propos », left open under it).

The notes are the `CHANGELOG.md` entry, in English whatever the app's language.

## The manifest

`scripts/update-manifest.sh` writes it and the release workflow uploads it after the installers:

```json
{
  "version": "0.0.3",
  "notes": "### Added\n- …",
  "assets": { "macos-arm64": "https://github.com/skraft-app/skraft/releases/download/v0.0.3/Skraft-0.0.3-macos-arm64.dmg" }
}
```

Keys are `macos-arm64`, `macos-x86_64`, `windows-x86_64`, `linux-x86_64`. The app opens an asset
only if it is a file of this repository's release of that version; otherwise, or when the platform
is missing, it opens the release's page.
