//! The separators of numbers, read from the system's regional settings (not from the texts'
//! language): its decimal separator, and its list separator where it has one (Windows); the
//! numbers shown and the lists typed in the measurements box follow them
//! (`skraft_i18n::set_separators`, `skraft_model::measure::list`).

/// Reads them and gives them to `skraft_i18n`; when the system does not say, the
/// defaults stay (`,` and `;`).
pub fn read() {
    if let Some((decimal, list)) = system() {
        log::info!("system separators: decimal {decimal:?}, list {list:?}");
        skraft_i18n::set_separators(decimal, list);
    }
}

/// The first character of a separator the system gives.
fn first(text: &str) -> Option<char> {
    text.trim().chars().next()
}

/// macOS: the current locale, the user's own settings included (System Settings › Region); it
/// has no list separator.
#[cfg(target_os = "macos")]
fn system() -> Option<(char, Option<char>)> {
    let locale = objc2_foundation::NSLocale::currentLocale();
    Some((first(&locale.decimalSeparator().to_string())?, None))
}

/// Windows: the user's locale (Region settings), its decimal and list separators.
#[cfg(windows)]
fn system() -> Option<(char, Option<char>)> {
    use windows_sys::Win32::Globalization::{GetLocaleInfoEx, LOCALE_SDECIMAL, LOCALE_SLIST};
    let info = |what: u32| -> Option<char> {
        let mut buffer = [0u16; 8];
        // A null locale name: the user's default locale.
        let n = unsafe { GetLocaleInfoEx(std::ptr::null(), what, buffer.as_mut_ptr(), buffer.len() as i32) };
        (n > 1).then(|| String::from_utf16_lossy(&buffer[..n as usize - 1])).as_deref().and_then(first)
    };
    Some((info(LOCALE_SDECIMAL)?, info(LOCALE_SLIST)))
}

/// Linux: the environment's numeric locale (`LC_ALL`, `LC_NUMERIC`, `LANG`), read on this
/// thread only (`uselocale`): the process's C locale stays "C", which other libraries' number
/// formatting may count on. No list separator.
#[cfg(target_os = "linux")]
fn system() -> Option<(char, Option<char>)> {
    unsafe {
        let locale = libc::newlocale(libc::LC_NUMERIC_MASK, c"".as_ptr(), std::ptr::null_mut());
        if locale.is_null() {
            return None;
        }
        let before = libc::uselocale(locale);
        let conv = libc::localeconv();
        let decimal = (!conv.is_null() && !(*conv).decimal_point.is_null())
            .then(|| std::ffi::CStr::from_ptr((*conv).decimal_point).to_string_lossy().into_owned());
        libc::uselocale(before);
        libc::freelocale(locale);
        Some((first(&decimal?)?, None))
    }
}

/// The browser: its language's number format (`Intl.NumberFormat`), no list separator.
#[cfg(target_arch = "wasm32")]
fn system() -> Option<(char, Option<char>)> {
    let format = js_sys::Intl::NumberFormat::new(&js_sys::Array::new(), &js_sys::Object::new());
    let parts = format.format_to_parts(1.5);
    let decimal = parts.iter().find_map(|part| {
        let kind = js_sys::Reflect::get(&part, &"type".into()).ok()?.as_string()?;
        (kind == "decimal").then(|| js_sys::Reflect::get(&part, &"value".into()).ok()?.as_string()).flatten()
    });
    Some((first(&decimal?)?, None))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows, target_arch = "wasm32")))]
fn system() -> Option<(char, Option<char>)> {
    None
}
