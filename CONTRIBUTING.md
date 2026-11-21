# Contributing to Skraft

Thank you for your interest in Skraft. The project is at an early stage: bug reports, ideas and
focused pull requests are all welcome.

## Before you start

- **A bug:** open an issue with your Skraft version, your system, the steps to reproduce it and,
  if you can share it, the file that shows it.
- **A new tool, a change to the file format or to how the app is organised:** open an issue
  first, so that we can agree on the design before you spend time on it.
- **A security problem:** do not open a public issue, follow [SECURITY.md](SECURITY.md).

## Build and test

The Rust toolchain is pinned by `rust-toolchain.toml`: rustup installs it on the first build. On
Linux, install `libwayland-dev` and `libxkbcommon-dev` first.

```sh
cargo run -p skraft              # the desktop app, from the debug build
cargo build --workspace
cargo test --workspace
scripts/build-web.sh             # the browser version, into dist/web
```

Always build and test with `--workspace` rather than `-p <crate>`: a single crate unifies the
dependencies' features differently and rebuilds the kernel from scratch. The debug build is
optimised enough to use, so there is no need for `--release` to try something.

A pull request must pass these checks, which CI runs on Linux:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The off-screen rendering tests skip themselves on a machine without a GPU.

## Conventions

- **Language.** Code comments and documentation are in English.
- **User-facing texts** live only in the catalogs, `locales/fr.yml` (the reference) and
  `locales/en.yml`, with the same keys. Never write them as literals in the code, and never end
  them with `…`: the code appends it.
- **Tool names.** Action tools are named by an infinitive verb (« Copier en ligne », « Unir »),
  drawing tools by what they make (« Boîte », « Prisme »).
- **Tools ported from [OpenCutList](https://github.com/lairdubois/lairdubois-opencutlist-sketchup-extension)**
  copy its interaction (states, picking, previews, keys), not only its algorithm.
- **Performance is part of the design.** Measure what you change where it matters, with a
  benchmark or a timing test.
- **Design documentation.** [docs/](docs/README.md) describes the present state, one page per
  topic. A change of design updates its page in the same pull request; a structuring decision
  and its alternatives go to `docs/decisions/`.
- **Changelog.** `CHANGELOG.md` is written when a version is tagged: leave it out of your pull
  request.

## Licensing of contributions

Skraft is under the [GNU General Public License v3.0 or later](LICENSE). By submitting a
contribution, you confirm that you have the right to do so and that it is licensed under the same
terms.

If your contribution includes code from another project, say so in the pull request, check that
its license is compatible with the GPLv3 and keep its copyright notice. Do not copy from
SolidCraft's `ui-kit`, `look` and `theme` crates: their brand license does not allow reuse in
another program.
