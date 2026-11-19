//! The language of the texts (docs/ui/texts.md#numbers-and-locale): the one chosen in the
//! preferences, or else the system's — the first of its preferred languages with a catalog,
//! English when none has one.

use std::sync::OnceLock;

/// The locale the system's preferred languages pick, read once.
pub fn system() -> &'static str {
    static SYSTEM: OnceLock<String> = OnceLock::new();
    SYSTEM.get_or_init(|| {
        let languages: Vec<String> = sys_locale::get_locales().collect();
        let locale = skraft_i18n::system_locale(&languages);
        log::info!("system languages {languages:?}: texts in {locale:?}");
        locale
    })
}

/// Sets the texts' locale to `chosen` (the preferences'), or to the system's for none.
pub fn apply(chosen: Option<&str>) {
    skraft_i18n::set_locale(chosen.unwrap_or_else(|| system()));
}
