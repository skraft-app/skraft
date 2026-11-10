//! The user-visible texts of SolidSketch: one YAML catalog per locale in `/locales` (`fr.yml`
//! is the reference), compiled in by rust-i18n. See docs/ARCHITECTURE.md §6 quater.
//!
//! A key is a dotted path that follows where the text shows in the app: `menu.file.open`,
//! `tool.move.hint.start`, `error.expr.zero_division`. A text takes named values (`%{name}`);
//! a count picks a key or its `_plural` form ([`tp!`]). Keys are always literals, so the tests
//! check both ways that the code and the catalogs agree, placeholders included.
//!
//! Nothing is cached: egui draws every frame, so [`set_locale`] changes the app at once.

use std::sync::atomic::{AtomicBool, Ordering};

rust_i18n::i18n!("../../locales", fallback = "fr");

/// The reference locale: every other one has the same keys.
pub const DEFAULT_LOCALE: &str = "fr";

/// The text of `key` in the current locale, its `%{name}` placeholders filled:
/// `t!("menu.edit.undo_op", op = label)`.
#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::translate($key, &[])
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::translate($key, &[$((stringify!($name), ::std::string::ToString::to_string(&$value))),+])
    };
}

/// The text of `key`, or of `key_plural`, as `count` asks in the current locale; `%{count}`
/// is filled with it, and the other placeholders as with [`t!`].
#[macro_export]
macro_rules! tp {
    ($key:literal, $count:expr) => {
        $crate::translate_count($key, concat!($key, "_plural"), $count, &[])
    };
    ($key:literal, $count:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::translate_count(
            $key,
            concat!($key, "_plural"),
            $count,
            &[$((stringify!($name), ::std::string::ToString::to_string(&$value))),+],
        )
    };
}

/// Behind [`t!`]. A missing key shows as itself (the tests make sure none is).
#[doc(hidden)]
pub fn translate(key: &str, args: &[(&str, String)]) -> String {
    let text = _rust_i18n_try_translate(&locale(), key).map(|t| t.into_owned()).unwrap_or_else(|| key.to_owned());
    fill(&text, args)
}

/// Behind [`tp!`].
#[doc(hidden)]
pub fn translate_count(one: &str, many: &str, count: usize, args: &[(&str, String)]) -> String {
    let mut all = vec![("count", count.to_string())];
    all.extend(args.iter().cloned());
    translate(if is_plural(&locale(), count) { many } else { one }, &all)
}

fn fill(text: &str, args: &[(&str, String)]) -> String {
    if args.is_empty() {
        return text.to_owned();
    }
    let names: Vec<&str> = args.iter().map(|(n, _)| *n).collect();
    let values: Vec<String> = args.iter().map(|(_, v)| v.clone()).collect();
    rust_i18n::replace_patterns(text, &names, &values)
}

/// Does `count` take the plural in `locale`? French: from 2 on (0 and 1 are singular);
/// otherwise, the English rule (all but 1). Languages with more forms need more than
/// `_plural`.
pub fn is_plural(locale: &str, count: usize) -> bool {
    match language(locale) {
        "fr" => count > 1,
        _ => count != 1,
    }
}

fn language(locale: &str) -> &str {
    locale.split(['-', '_']).next().unwrap_or(locale)
}

/// The locales there is a catalog for, sorted.
pub fn available_locales() -> Vec<String> {
    _rust_i18n_available_locales().into_iter().map(|l| l.into_owned()).collect()
}

/// The current locale, among the available ones ([`DEFAULT_LOCALE`] until one is set).
pub fn locale() -> String {
    let current = rust_i18n::locale().to_string();
    if _rust_i18n_backend().translate(&current, "locale.name").is_some() { current } else { DEFAULT_LOCALE.to_owned() }
}

/// Changes the language of every text from now on. An unknown locale falls back to
/// [`DEFAULT_LOCALE`].
pub fn set_locale(locale: &str) {
    rust_i18n::set_locale(locale);
}

/// A locale's name in itself (`Français`), for the language menu.
pub fn locale_name(locale: &str) -> String {
    _rust_i18n_try_translate(locale, "locale.name").map(|t| t.into_owned()).unwrap_or_else(|| locale.to_owned())
}

/// The ellipsis that ends a menu item opening a dialog or a text still going on. Never in a
/// catalog: added by the code, the same for every language.
pub const ELLIPSIS: &str = "…";

/// `text` followed by [`ELLIPSIS`]: `Ouvrir…`.
pub fn ellipsis(text: impl Into<String>) -> String {
    let mut text = text.into();
    text.push_str(ELLIPSIS);
    text
}

/// A number as the locale writes it: `12,5` in French. `text` is Rust's own formatting of
/// it (`12.5`).
pub fn number(text: &str) -> String {
    text.replace('.', &t!("locale.decimal_separator"))
}

static MAC: AtomicBool = AtomicBool::new(cfg!(target_os = "macos"));

/// Texts that name keys differ on macOS (Option, ⌘) and elsewhere (Ctrl). The app says which
/// it runs on — in the browser too, which `cfg!` cannot tell.
pub fn set_mac(mac: bool) {
    MAC.store(mac, Ordering::Relaxed);
}

pub fn mac() -> bool {
    MAC.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests;
