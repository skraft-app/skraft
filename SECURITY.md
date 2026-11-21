# Security policy

## Reporting a vulnerability

Please do not report a vulnerability in a public issue. Use GitHub's private reporting instead:
in the repository's **Security** tab, choose **Report a vulnerability**.

Include the Skraft version, your system, what an attacker can achieve and the steps to reproduce
it, with a sample file if one is needed. Skraft is maintained on a best-effort basis, so there is
no guaranteed response time, but every report will be read and you will be credited if you wish.

## Supported versions

Only the latest release receives security fixes.

## Scope

The most relevant areas are:

- **Opening a file.** `.skraft`, `.skraftmat`, `.skrafttpl`, STEP and SketchUp files can come from
  anyone: a file should never be able to crash the app, exhaust its memory or run code.
- **The control channel** that lets agents and scripts drive the app
  ([docs/control-protocol.md](docs/control-protocol.md)). It is off by default, listens on the
  loopback interface only and requires a token readable by the current user alone.

Not considered vulnerabilities:

- The installers are not yet signed by a known publisher, as the [README](README.md#download) says.
- A flaw in a dependency that Skraft cannot trigger. If it can, please report it here as well as
  upstream.
