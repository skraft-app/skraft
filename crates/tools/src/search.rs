//! The search shared by the « Composants » card and the navigator (docs/ui/components-library.md#search):
//! a name typed, matched without case nor diacritics.

/// What is typed in a search field, ready to match: trimmed, folded once.
#[derive(Clone, Debug, Default)]
pub struct Matcher {
    needle: String,
}

impl Matcher {
    pub fn new(text: &str) -> Matcher {
        Matcher { needle: fold(text.trim()) }
    }

    /// Does it filter anything out (something typed)?
    pub fn is_active(&self) -> bool {
        !self.needle.is_empty()
    }

    /// Does one of `names` hold the text typed (case and diacritics ignored)? Any, when none is.
    pub fn name<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> bool {
        self.needle.is_empty() || names.into_iter().any(|n| fold(n).contains(&self.needle))
    }
}

/// `s` folded for matching: lower case, the diacritics of Latin letters dropped (« Étagère » →
/// « etagere »), the ligatures spelt out.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        for l in c.to_lowercase() {
            match base(l) {
                Some(b) => out.push_str(b),
                None => out.push(l),
            }
        }
    }
    out
}

/// The base letters of a lower case Latin letter with diacritics, if it is one.
fn base(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ď' | 'đ' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ĥ' | 'ħ' => "h",
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĵ' => "j",
        'ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => "s",
        'ţ' | 'ť' | 'ŧ' | 'ț' => "t",
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        'æ' => "ae",
        'œ' => "oe",
        'ß' => "ss",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_without_case_nor_accents() {
        assert_eq!(fold("Étagère Œuvre"), "etagere oeuvre");
        let m = Matcher::new(" etag ");
        assert!(m.is_active() && m.name(["Étagère#2"]));
        assert!(m.name(["Gauche", "ÉTAGÈRE"]), "any of the names");
        assert!(!m.name(["Pied"]));
        let none = Matcher::new("  ");
        assert!(!none.is_active() && none.name(["Pied"]), "nothing typed: everything");
    }
}
