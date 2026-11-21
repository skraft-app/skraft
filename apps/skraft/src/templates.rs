//! The sheet templates (docs/ui/drawing.md#templates): the one Skraft comes with, and the
//! user's, `.skrafttpl` files in the library's folder (`<config>/Skraft/templates`, native).

use std::sync::Arc;

use skraft_model::Document;

/// The templates' file extension.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub const EXTENSION: &str = "skrafttpl";

/// A template offered for a new sheet: its name and its document.
#[derive(Clone)]
pub struct Template {
    pub name: String,
    pub document: Arc<Document>,
}

/// The library's folder of templates (none on the web).
#[cfg(not(target_arch = "wasm32"))]
pub fn folder() -> Option<std::path::PathBuf> {
    Some(dirs::config_dir()?.join("Skraft").join("templates"))
}

/// The templates offered: the one Skraft comes with, then the library's, by name.
pub fn all() -> Vec<Template> {
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut out = vec![Template { name: skraft_i18n::t!("drawing.template.default"), document: Arc::new(Document::default_template()) }];
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(dir) = folder()
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        let mut found: Vec<Template> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == EXTENSION))
            .filter_map(|p| {
                let bytes = std::fs::read(&p).ok()?;
                match Document::read_file(&bytes).ok()? {
                    (doc, skraft_model::Content::Template) => {
                        Some(Template { name: p.file_stem()?.to_string_lossy().into_owned(), document: Arc::new(doc) })
                    }
                    _ => None,
                }
            })
            .collect();
        found.sort_by_key(|t| t.name.to_lowercase());
        out.extend(found);
    }
    out
}

/// A template file put in the library as `name` (its characters a file name refuses
/// replaced); where it went.
#[cfg(not(target_arch = "wasm32"))]
pub fn install(name: &str, bytes: &[u8]) -> Result<std::path::PathBuf, String> {
    let dir = folder().ok_or_else(|| "no configuration folder".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let clean: String = name.chars().map(|c| if c.is_alphanumeric() || " -_.()".contains(c) { c } else { '_' }).collect();
    let clean = if clean.trim().is_empty() { "Modèle".to_string() } else { clean.trim().to_string() };
    let path = dir.join(format!("{clean}.{EXTENSION}"));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}
