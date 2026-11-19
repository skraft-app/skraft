//! The search shared by the « Composants » card and the navigator (docs/ui/components-library.md#search):
//! a name, natures and a material; what a definition shows.

use skraft_model::{ComponentKind, Document, FastMap, FastSet, Id};

/// What a list is filtered by. The criteria are and-ed; the default lets everything through.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Filter {
    /// Typed in the search field.
    pub text: String,
    pub parts: bool,
    pub assemblies: bool,
    /// The navigator's body rows.
    pub bodies: bool,
    /// Only what shows this material.
    pub material: Option<Id>,
}

impl Default for Filter {
    fn default() -> Self {
        Filter { text: String::new(), parts: true, assemblies: true, bodies: true, material: None }
    }
}

/// A nature of what a row shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nature {
    Part,
    Assembly,
    Body,
}

impl Filter {
    /// Does it filter anything out?
    pub fn is_active(&self) -> bool {
        !self.text.trim().is_empty() || !self.parts || !self.assemblies || !self.bodies || self.material.is_some()
    }

    /// The filter as it matches, its text folded once.
    pub fn matcher(&self) -> Matcher {
        Matcher { needle: fold(self.text.trim()), filter: self.clone() }
    }
}

/// A [`Filter`] ready to match.
#[derive(Clone, Debug)]
pub struct Matcher {
    needle: String,
    filter: Filter,
}

impl Matcher {
    /// Is a nature let through?
    pub fn nature(&self, n: Nature) -> bool {
        match n {
            Nature::Part => self.filter.parts,
            Nature::Assembly => self.filter.assemblies,
            Nature::Body => self.filter.bodies,
        }
    }

    /// Does one of `names` hold the text typed (case and diacritics ignored)? Any, when none is.
    pub fn name<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> bool {
        self.needle.is_empty() || names.into_iter().any(|n| fold(n).contains(&self.needle))
    }

    /// The material asked for, if any.
    pub fn material(&self) -> Option<Id> {
        self.filter.material
    }

    /// Does something showing `shown` (its materials) pass the material criterion?
    pub fn shows(&self, shown: impl IntoIterator<Item = Id>) -> bool {
        match self.filter.material {
            None => true,
            Some(m) => shown.into_iter().any(|s| s == m),
        }
    }

    /// Everything of nature `n` named `names`, showing `shown`.
    pub fn matches<'a>(&self, n: Nature, names: impl IntoIterator<Item = &'a str>, shown: impl IntoIterator<Item = Id>) -> bool {
        self.nature(n) && self.shows(shown) && self.name(names)
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

/// The materials each component shows (docs/ui/components-library.md#search): those its
/// occurrences show in the model — their own, else one inherited from an assembly's occurrence
/// above —, and for a part those of its faces. A component placed nowhere shows its faces' only.
pub fn materials_shown(doc: &Document) -> FastMap<Id, FastSet<Id>> {
    // The components reached from the root, each after every assembly holding it.
    fn walk(doc: &Document, c: Id, seen: &mut FastSet<Id>, order: &mut Vec<Id>) {
        if !seen.insert(c) {
            return;
        }
        if let Ok(a) = doc.assembly(c) {
            for o in a.occurrences() {
                walk(doc, o.component, seen, order);
            }
        }
        order.push(c);
    }
    let mut seen = FastSet::default();
    let mut order = Vec::new();
    walk(doc, doc.root(), &mut seen, &mut order);
    // What reaches each component from above: a material, or none (`None`).
    let mut inherited: FastMap<Id, FastSet<Option<Id>>> = FastMap::default();
    inherited.entry(doc.root()).or_default().insert(None);
    for &c in order.iter().rev() {
        let Ok(a) = doc.assembly(c) else { continue };
        let above: Vec<Option<Id>> = inherited.get(&c).map(|s| s.iter().copied().collect()).unwrap_or_default();
        for o in a.occurrences() {
            let set = inherited.entry(o.component).or_default();
            match o.material {
                Some(m) => {
                    set.insert(Some(m));
                }
                None => set.extend(above.iter().copied()),
            }
        }
    }
    let mut shown: FastMap<Id, FastSet<Id>> = FastMap::default();
    for c in doc.components() {
        let mut s: FastSet<Id> = inherited.get(&c.id).map(|s| s.iter().flatten().copied().collect()).unwrap_or_default();
        if let ComponentKind::Part(p) = &c.kind {
            s.extend(p.faces.values().copied());
        }
        if !s.is_empty() {
            shown.insert(c.id, s);
        }
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_without_case_nor_accents() {
        assert_eq!(fold("Étagère Œuvre"), "etagere oeuvre");
        let m = Filter { text: " etag ".into(), ..Filter::default() }.matcher();
        assert!(m.name(["Étagère#2"]));
        assert!(m.name(["Gauche", "ÉTAGÈRE"]), "any of the names");
        assert!(!m.name(["Pied"]));
        assert!(Filter::default().matcher().name(["Pied"]), "nothing typed: everything");
        assert!(!Filter::default().is_active());
        let parts = Filter { assemblies: false, ..Filter::default() };
        assert!(parts.is_active() && !parts.matcher().matches(Nature::Assembly, ["x"], []));
        let oak = Filter { material: Some(Id(7)), ..Filter::default() }.matcher();
        assert!(oak.shows([Id(3), Id(7)]) && !oak.shows([Id(3)]));
    }
}
