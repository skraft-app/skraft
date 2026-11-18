//! The user-visible texts of Skraft: one YAML catalog per locale in `/locales` (`fr.yml`
//! is the reference), compiled in by rust-i18n. See docs/ARCHITECTURE.md §6 quater.
//!
//! A key is a dotted path that follows where the text shows in the app: `menu.file.open`,
//! `tool.move.hint.start`, `error.expr.zero_division`. A text takes named values (`%{name}`);
//! a count picks a key or its `_plural` form ([`tp!`]). A text can take another one in,
//! `$t(common.undo)`: generic terms are written once, in `common` ([`resolve`]). Keys are
//! always literals, so the tests check both ways that the code and the catalogs agree,
//! placeholders and references included.
//!
//! Nothing is cached: egui draws every frame, so [`set_locale`] changes the app at once.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

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
    let locale = locale();
    let text = _rust_i18n_try_translate(&locale, key).map(|t| t.into_owned()).unwrap_or_else(|| key.to_owned());
    fill(&resolve(&locale, &text), args)
}

/// Opens a reference to another text.
const REFERENCE: &str = "$t(";
/// References inside references, at most (beyond, a cycle: the tests forbid them).
const MAX_DEPTH: usize = 8;

/// `text` with each `$t(key)` replaced by the text of `key` in `locale`, itself resolved:
/// `"$t(common.undo) %{op}"` → `"Annuler %{op}"`. The placeholders are left for the caller to
/// fill, those of the texts taken in included. An unknown key is left as written.
pub fn resolve(locale: &str, text: &str) -> String {
    resolve_at(locale, text, 0)
}

fn resolve_at(locale: &str, text: &str, depth: usize) -> String {
    if depth >= MAX_DEPTH || !text.contains(REFERENCE) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(REFERENCE) {
        let after = &rest[i + REFERENCE.len()..];
        let Some(end) = after.find(')') else { break };
        out.push_str(&rest[..i]);
        match _rust_i18n_try_translate(locale, &after[..end]) {
            Some(t) => out.push_str(&resolve_at(locale, &t, depth + 1)),
            None => out.push_str(&rest[i..i + REFERENCE.len() + end + 1]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// The keys `text` refers to (`$t(key)`), in order.
pub fn references(text: &str) -> Vec<&str> {
    let mut keys = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(REFERENCE) {
        let after = &rest[i + REFERENCE.len()..];
        let Some(end) = after.find(')') else { break };
        keys.push(&after[..end]);
        rest = &after[end + 1..];
    }
    keys
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

/// A number as the system writes it: `12,5` with a decimal comma. `text` is Rust's own
/// formatting of it (`12.5`).
pub fn number(text: &str) -> String {
    text.replace('.', decimal_separator().encode_utf8(&mut [0; 4]))
}

/// The separators of numbers are the system's regional settings, not the texts' language
/// (French texts on a system writing `12.5`): the app reads them at start
/// ([`set_separators`]); until then, and headless, `,` and `;`.
static DECIMAL: AtomicU32 = AtomicU32::new(',' as u32);
static LIST: AtomicU32 = AtomicU32::new(';' as u32);

/// Sets the system's decimal separator, and its list separator when it has one (Windows);
/// otherwise the list separator follows from the decimal one ([`list_separator_for`]).
pub fn set_separators(decimal: char, list: Option<char>) {
    let decimal = if decimal.is_alphanumeric() || decimal.is_whitespace() { ',' } else { decimal };
    DECIMAL.store(decimal as u32, Ordering::Relaxed);
    LIST.store(list_separator_for(decimal, list) as u32, Ordering::Relaxed);
}

thread_local! {
    /// Separators for this thread only ([`with_separators`]).
    static SEPARATORS: std::cell::Cell<Option<(char, char)>> = const { std::cell::Cell::new(None) };
}

/// Runs `f` with these separators on this thread only, as [`set_separators`] would set them:
/// for tests, which run side by side.
pub fn with_separators<R>(decimal: char, list: Option<char>, f: impl FnOnce() -> R) -> R {
    let before = SEPARATORS.replace(Some((decimal, list_separator_for(decimal, list))));
    let r = f();
    SEPARATORS.set(before);
    r
}

/// The decimal separator numbers are written with (`,` or `.`, as the system says).
pub fn decimal_separator() -> char {
    match SEPARATORS.get() {
        Some((d, _)) => d,
        None => char::from_u32(DECIMAL.load(Ordering::Relaxed)).unwrap_or(','),
    }
}

/// The separator of typed lists besides `;` (always one), and the one lists are written
/// with: `;`, or `,` on a system writing `12.5` — `,` is then no decimal point in what is
/// typed.
pub fn list_separator() -> char {
    match SEPARATORS.get() {
        Some((_, l)) => l,
        None => char::from_u32(LIST.load(Ordering::Relaxed)).unwrap_or(';'),
    }
}

/// The list separator of a system with `decimal` and, if it has one, `list`: only `;` or
/// `,` (a typed list takes nothing else), never the decimal separator; without one, `,`
/// when the decimal separator is not (as spreadsheets do).
pub fn list_separator_for(decimal: char, list: Option<char>) -> char {
    match list {
        Some(l @ (';' | ',')) if l != decimal => l,
        _ if decimal == ',' => ';',
        _ => ',',
    }
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
