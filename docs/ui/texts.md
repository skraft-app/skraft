---
status: built
code: [crates/i18n/src/lib.rs, crates/i18n/src/tests.rs, crates/i18n/build.rs, locales/fr.yml, apps/skraft/src/separators.rs]
---

# Texts and languages

How every text the user sees reaches the screen: the catalogs, their keys and conventions, the
platform's key names, the system's number separators and the tests that keep the catalogs
whole.

## Catalogs

Every text the user sees is in a YAML catalog, one file per locale in `/locales` (`fr.yml`
is the reference). The `skraft-i18n` crate compiles them in with rust-i18n and is the only way
to them: `t!("menu.file.open")`, `t!("status.save_failed", error = e)`. The code holds no
user-visible string. Not yet: `fr.yml` is the only locale.

## Conventions

- **Keys** are paths that follow where the text shows: `menu.*`, `toolbar.*`, `ribbon.*`,
  `titlebar.*`, `tabs.*`, `start.*`, `option_bar.*`, `prefs.*`, `panel.entity_info.*`,
  `panel.outliner.*`, `toast.*`, `notice.*`, `status.*`, then per tool
  `tool.<tool>.{name, hint.*, vcb.*, option.*, say.*}`, `infer.*`, `operation.*` (Undo /
  Redo), `unit.*`, `default_name.*` and `error.{expr, model, file, kernel, worker,
  preferences}.*`. A text shown in two places has two keys — which may share their words
  through `common`.
- **Shared terms**: `common.*` holds generic terms (Undo, Erase, Copy, Grid…), named by meaning
  (`common.undo`, not after the French « Annuler », which is also a dialog's Cancel). A text
  takes one in with **`$t(key)`** — `menu.edit.undo_op: "$t(common.undo) %{op}"` — resolved at
  lookup (`skraft_i18n::resolve`), nested references too, before the placeholders are
  filled. Any key can be referred to; a term is taken as is (no change of case), so one used
  inside a sentence is written in the sentence. Each locale chooses its own references.
- **Tool names** (`tool.<tool>.name`, also the commands' and ribbon buttons' labels): a verb in
  the infinitive whenever the tool is an action — « Sélectionner », « Effacer », « Dessiner une
  boîte », « Tourner », « Copier en ligne », « Unir », « Mesurer » — a
  noun only where no verb fits without ambiguity (« Plan de coupe »: « Couper » is the clipboard's
  Cut; « Profil »).
- **Values** are named, `%{name}`, and few: a sentence is one key, but a label built from
  parts stays built by the code ("Edge × red axis" is `infer.edge`, "×", `infer.line.axis`).
- **No ellipsis** (…) in a catalog: the code adds it (`skraft_i18n::ellipsis`), the same
  for every language — "Open…" is `menu.file.open` + `…`. A sentence it would cut in the middle
  is two keys (`status.computing`, `status.computing_cancel`). A test checks it.
- **Plurals**: a key and its `_plural` form (OpenCutList's convention), picked by `tp!(key,
  count)` with the locale's rule (French: 0 and 1 singular). Languages with more plural forms
  are not handled.
- **Errors** are typed (`model::Error`, `Refusal`, `FileError`, `ExprError`, `kernel::Error`):
  their `Display` looks up one key per kind. SolidCore's own messages stay in English inside.
- **Platform**: hints and shortcuts name keys as the platform does (Option / Ctrl, `⇧⌘Z` /
  `Ctrl+Shift+Z`); the app tells `skraft-i18n` whether it runs on a Mac (`ctx.os()`, right
  in the browser too).

## Numbers and locale

- **Numbers** follow the system's regional settings, not the texts' language: at start the app
  reads its decimal separator, and its list separator where it has one (`src/separators.rs`: macOS `NSLocale`, Windows `GetLocaleInfoEx` `LOCALE_SDECIMAL`
  / `LOCALE_SLIST`, Linux the numeric locale by `localeconv` on its own thread's locale, the
  browser `Intl.NumberFormat`) and gives them to `skraft-i18n` (`set_separators`).
  Without a list separator (all but Windows) it follows from the decimal one: `;` after `,`,
  `,` after `.`; only `;` or `,`, never the decimal one. Until then, headless and in the tests,
  `,` and `;` (`with_separators` sets others on one thread, for the tests). Numbers are
  written with the decimal separator (`number`); typed values always separate a list with
  `;`, and with `,` too when it is the list separator (then not a decimal point).
- **Locale**: in the preferences (General › Language), applied at once — egui draws every frame
  and nothing caches a text. Texts already in the status bar stay as they were. egui ids never
  come from a text (fixed `id` / `id_salt`), so a window or a section keeps its state across a
  change of language.

## Checks

- **Checked by the tests** (`crates/i18n`): every key the sources use is in `fr.yml` with the
  placeholders the call fills (references resolved), every key there is used by the code or by
  a reference, every `$t(…)` leads to a key of its locale without a cycle, and every other
  locale has the same keys and placeholders. Keys are therefore always literals.
