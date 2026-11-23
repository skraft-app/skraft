# The desktop app detects new versions but does not install them

- Status: accepted (2026-11-23; built)
- Design: [Updates](../ui/updates.md)

## Context

Testers had no way to learn that a new build exists. The installers are not notarized on macOS
nor signed on Windows, and the app had no HTTP client.

## Decision

- **The app detects, the browser downloads, the user installs.** A button opens this platform's
  installer (the MSI on Windows); nothing is replaced by the app.
- **A static `latest.json` among the release's files**, read at
  `releases/latest/download/latest.json`. The release workflow uploads it last, once the
  installers are there.
- **On by default**, at most once a day, a preference turns it off. The request carries no
  identifier beyond the app's version in its `User-Agent`; GitHub sees the address.
- **Never in the way**: on a thread after the launch, silent on failure, a mark in the title bar
  and one toast per version.
- **Only this repository's files of that version are opened**; any other URL in the manifest
  falls back to the release's page.

## Alternatives considered

- **Installing by itself**: without signatures it would hand the testers' machines to whoever
  compromises a release, and needs an installer path per format. To reconsider with real
  certificates, around `0.1.0`.
- **The GitHub API**: limited to 60 requests an hour per address, which a shared network reaches.
- **A file on skraft.app**: needs a token for the release workflow to write to another repository.
- **Asking at first launch**: one more dialog for a request that carries nothing personal.
- **`rustls-platform-verifier`**: pulls a certificate bundle under a license not on the allowed
  list. `rustls-native-certs` reads the same system stores.
