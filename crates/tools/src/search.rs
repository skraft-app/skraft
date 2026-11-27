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

/// A name's key for sorting by name: folded as a search matches it ([`fold`]: case and Latin
/// diacritics ignored), its runs of digits compared as numbers (« Pièce#2 » before
/// « Pièce#10 »).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameKey(String);

impl NameKey {
    pub fn new(name: &str) -> NameKey {
        NameKey(fold(name))
    }
}

impl Ord for NameKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        let (mut a, mut b) = (self.0.as_str(), other.0.as_str());
        loop {
            let (Some(x), Some(y)) = (a.chars().next(), b.chars().next()) else {
                return a.len().cmp(&b.len()).then_with(|| self.0.cmp(&other.0));
            };
            if x.is_ascii_digit() && y.is_ascii_digit() {
                let run = |s: &str| s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
                let (n, m) = (run(a), run(b));
                // Leading zeros dropped: by length, then digit by digit; any size of number.
                let (u, v) = (a[..n].trim_start_matches('0'), b[..m].trim_start_matches('0'));
                let o = u.len().cmp(&v.len()).then_with(|| u.cmp(v));
                if o != Ordering::Equal {
                    return o;
                }
                (a, b) = (&a[n..], &b[m..]);
                continue;
            }
            if x != y {
                return x.cmp(&y);
            }
            (a, b) = (&a[x.len_utf8()..], &b[y.len_utf8()..]);
        }
    }
}

impl PartialOrd for NameKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod name_key_tests {
    use super::NameKey;

    #[test]
    fn names_sort_ignoring_case_and_accents_numbers_as_numbers() {
        let mut names = vec!["Pièce#10", "pièce#2", "Étagère", "Pied", "etagere 3", "Pièce#1", "Pièce#002"];
        names.sort_by_key(|n| NameKey::new(n));
        assert_eq!(names, vec!["Étagère", "etagere 3", "Pièce#1", "Pièce#002", "pièce#2", "Pièce#10", "Pied"]);
    }
}
