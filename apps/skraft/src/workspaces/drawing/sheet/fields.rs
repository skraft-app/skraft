//! The fields a sheet's texts show (docs/ui/drawing.md).

use super::*;

/// The texts' fields (docs/ui/drawing.md#fields): what they say on this sheet.
pub struct Fields<'a> {
    pub document: &'a str,
    pub sheet: &'a str,
    pub number: usize,
    pub count: usize,
    pub scale: Option<f64>,
    /// Where the other fields' values are looked for: the document, this sheet.
    pub values: Option<(&'a skraft_model::Document, Id)>,
}

/// The fields among `names` that are given values: not those Skraft fills itself.
pub fn custom_fields(names: impl IntoIterator<Item = String>) -> Vec<String> {
    names.into_iter().filter(|n| !FIELDS.contains(&n.as_str()) && !n.trim().is_empty()).collect()
}

/// The fields a text may hold, in the « Insérer un champ » menu's order.
pub const FIELDS: &[&str] = &["document", "date", "sheet", "sheet.number", "sheet.count", "scale"];

impl Fields<'_> {
    /// `text` with its `{…}` fields filled, for an element under the occurrences `path` (one
    /// without a value left as it is).
    pub fn fill(&self, text: &str, path: &[Id]) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(i) = rest.find('{') {
            out.push_str(&rest[..i]);
            let tail = &rest[i..];
            match tail.find('}') {
                Some(j) => {
                    let key = &tail[1..j];
                    match self.value(key, path) {
                        Some(v) => out.push_str(&v),
                        None => out.push_str(&tail[..=j]),
                    }
                    rest = &tail[j + 1..];
                }
                None => {
                    out.push_str(tail);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    }

    pub(super) fn value(&self, key: &str, path: &[Id]) -> Option<String> {
        Some(match key {
            "document" => self.document.to_owned(),
            "date" => today(),
            "sheet" => self.sheet.to_owned(),
            "sheet.number" => self.number.to_string(),
            "sheet.count" => self.count.to_string(),
            "scale" => self.scale.map(scale_label).unwrap_or_default(),
            _ => {
                let (doc, sheet) = self.values?;
                doc.field(sheet, path, key)?.shown(&doc.settings().units)
            }
        })
    }
}

/// Today's date, day/month/year (UTC).
fn today() -> String {
    let secs = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (y, m, d) = crate::system::dates::civil_from_days((secs / 86_400) as i64);
    format!("{d:02}/{m:02}/{y}")
}

/// What the texts' fields say on sheet `id`, which shows `placed`.
pub(super) fn fields_of<'a>(
    e: &'a Editor,
    d: &'a Drawing,
    id: Id,
    title: &'a str,
    placed: &[skraft_model::Placed<'_>],
) -> Option<Fields<'a>> {
    d.sheet(id).map(|s| Fields {
        document: title,
        sheet: &s.name,
        number: d.sheet_number(id).map_or(0, |(n, _)| n),
        count: d.sheet_number(id).map_or(0, |(_, c)| c),
        // The first base view it shows, through its occurrences.
        scale: placed.iter().find_map(|p| match &p.element.kind {
            ElementKind::View(View { kind: ViewKind::Base { scale, look, .. }, .. }) if look.frame().1.is_none() => Some(*scale),
            _ => None,
        }),
        values: Some((e.document(), id)),
    })
}
