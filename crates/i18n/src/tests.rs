//! The code and the catalogs agree: every key the code asks for is in the reference catalog
//! with the placeholders the code fills, every key there is used (by the code or by a
//! `$t(…)` reference), every reference leads somewhere without a cycle, and every other locale
//! has the same keys and placeholders.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::*;

/// Keys read without [`t!`]: the locale's own name ([`locale_name`]).
const READ_DIRECTLY: &[&str] = &["locale.name"];

/// A `t!` or `tp!` call found in the sources.
#[derive(Debug)]
struct Call {
    at: String,
    key: String,
    plural: bool,
    args: BTreeSet<String>,
}

fn catalog(locale: &str) -> BTreeMap<String, String> {
    _rust_i18n_backend()
        .messages_for_locale(locale)
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .filter(|(k, _)| !k.starts_with('_')) // the file's own fields (`_version`)
        .collect()
}

/// A locale's texts with their references resolved, as the app shows them.
fn resolved(locale: &str) -> BTreeMap<String, String> {
    catalog(locale).into_iter().map(|(k, v)| (k, resolve(locale, &v))).collect()
}

fn placeholders(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(i) = rest.find("%{") {
        let Some(j) = rest[i..].find('}') else { break };
        found.insert(rest[i + 2..i + j].to_owned());
        rest = &rest[i + j..];
    }
    found
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let p = entry.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// The `t!(…)` and `tp!(…)` calls of a source, comments left out (whole comment lines: a
/// call is never written after code on the same line as a comment).
fn calls_in(path: &Path, text: &str) -> Vec<Call> {
    let code: String = text.lines().map(|l| if l.trim_start().starts_with("//") { "" } else { l }).collect::<Vec<_>>().join("\n");
    let b = code.as_bytes();
    let mut calls = Vec::new();
    let mut i = 0;
    while let Some(off) = code[i..].find("!(") {
        let bang = i + off;
        i = bang + 2;
        // The macro's name: `t` or `tp`, not `assert`, `format`…
        let head = code[..bang].trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_').len();
        let plural = match &code[head..bang] {
            "t" => false,
            "tp" => true,
            _ => continue,
        };
        if head > 0 && b[head - 1] == b':' && !code[..head].ends_with("solidsketch_i18n::") {
            continue; // another crate's `t!`
        }
        let line = code[..bang].lines().count();
        let at = format!("{}:{line}", path.display());
        // The arguments, up to the closing parenthesis (strings skipped).
        let (mut depth, mut j, mut in_str) = (1, i, false);
        while j < b.len() && depth > 0 {
            match b[j] {
                b'\\' if in_str => j += 1,
                b'"' => in_str = !in_str,
                b'(' | b'[' | b'{' if !in_str => depth += 1,
                b')' | b']' | b'}' if !in_str => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        let inner = code[i..j - 1].trim();
        let Some(rest) = inner.strip_prefix('"') else {
            panic!("{at}: a key must be a literal");
        };
        let end = rest.find('"').unwrap();
        let key = rest[..end].to_owned();
        let mut args = BTreeSet::new();
        // Named values: `name = value` at the top level of the call.
        let tail = &rest[end + 1..];
        let (mut depth, mut in_str) = (0, false);
        let t = tail.as_bytes();
        let mut k = 0;
        while k < t.len() {
            match t[k] {
                b'\\' if in_str => k += 1,
                b'"' => in_str = !in_str,
                b'(' | b'[' | b'{' if !in_str => depth += 1,
                b')' | b']' | b'}' if !in_str => depth -= 1,
                b',' if !in_str && depth == 0 => {
                    let item = tail[k + 1..].trim_start();
                    let name: String = item.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                    let after = item[name.len()..].trim_start();
                    if !name.is_empty() && after.starts_with('=') && !after.starts_with("==") {
                        args.insert(name);
                    }
                }
                _ => {}
            }
            k += 1;
        }
        calls.push(Call { at, key, plural, args });
    }
    calls
}

fn all_calls() -> Vec<Call> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    sources(&root.join("apps"), &mut files);
    sources(&root.join("crates"), &mut files);
    let mut calls = Vec::new();
    // This file's own examples are not calls.
    files.retain(|f| !f.ends_with("i18n/src/tests.rs"));
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        calls.extend(calls_in(&f, &text));
    }
    calls
}

#[test]
fn the_scanner_reads_calls() {
    let src = "let a = t!(\"a.b\");\n// t!(\"commented\")\nlet b = tp!(\"c.d\", n, name = f(x, y), other = \"=\");\nassert!(x);";
    let calls = calls_in(Path::new("x.rs"), src);
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!((calls[0].key.as_str(), calls[0].plural, calls[0].args.len()), ("a.b", false, 0));
    assert_eq!((calls[1].key.as_str(), calls[1].plural), ("c.d", true));
    assert_eq!(calls[1].args, ["name", "other"].map(String::from).into());
}

#[test]
fn the_code_and_the_reference_catalog_agree() {
    let fr = resolved(DEFAULT_LOCALE);
    assert!(!fr.is_empty());
    let calls = all_calls();
    assert!(calls.len() > 100, "the sources were not found ({} calls)", calls.len());
    let mut problems = Vec::new();
    let mut used: BTreeSet<String> = READ_DIRECTLY.iter().map(|k| k.to_string()).collect();
    for c in &calls {
        let forms = if c.plural { vec![c.key.clone(), format!("{}_plural", c.key)] } else { vec![c.key.clone()] };
        let mut wanted = BTreeSet::new();
        for form in &forms {
            used.insert(form.clone());
            match fr.get(form) {
                None => problems.push(format!("{}: `{form}` is not in {DEFAULT_LOCALE}.yml", c.at)),
                Some(text) => {
                    let given: BTreeSet<String> = c.args.iter().cloned().chain(c.plural.then(|| "count".to_owned())).collect();
                    for p in placeholders(text).difference(&given) {
                        problems.push(format!("{}: `{form}` wants %{{{p}}}, not given", c.at));
                    }
                    wanted.extend(placeholders(text));
                }
            }
        }
        if forms.iter().all(|f| fr.contains_key(f)) {
            for a in c.args.difference(&wanted) {
                problems.push(format!("{}: `{}` has no %{{{a}}}", c.at, c.key));
            }
        }
    }
    // A text taken in by another one is used.
    used.extend(catalog(DEFAULT_LOCALE).values().flat_map(|t| references(t).into_iter().map(str::to_owned).collect::<Vec<_>>()));
    for k in fr.keys().filter(|k| !used.contains(*k)) {
        problems.push(format!("`{k}` in {DEFAULT_LOCALE}.yml is not used"));
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

#[test]
fn every_locale_has_the_reference_keys_and_placeholders() {
    let fr = resolved(DEFAULT_LOCALE);
    let mut problems = Vec::new();
    for locale in available_locales().iter().filter(|l| *l != DEFAULT_LOCALE) {
        let other = resolved(locale);
        for (k, text) in &fr {
            match other.get(k) {
                None => problems.push(format!("{locale}: `{k}` missing")),
                Some(t) if placeholders(t) != placeholders(text) => problems.push(format!("{locale}: `{k}`: other placeholders")),
                _ => {}
            }
        }
        for k in other.keys().filter(|k| !fr.contains_key(*k)) {
            problems.push(format!("{locale}: `{k}` is not in {DEFAULT_LOCALE}.yml"));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

/// Every `$t(key)` leads to a text of the same locale, and no text takes itself in, even
/// through others.
#[test]
fn references_lead_somewhere_without_a_cycle() {
    let mut problems = Vec::new();
    for locale in available_locales() {
        let texts = catalog(&locale);
        for (key, text) in &texts {
            for r in references(text) {
                if !texts.contains_key(r) {
                    problems.push(format!("{locale}: `{key}` refers to `{r}`, which is not there"));
                }
            }
            // Depth first from `key`: a path that comes back to a key on it is a cycle.
            let mut stack = vec![(key.as_str(), vec![key.as_str()])];
            while let Some((at, path)) = stack.pop() {
                for r in texts.get(at).map(|t| references(t)).unwrap_or_default() {
                    if path.contains(&r) {
                        problems.push(format!("{locale}: cycle {} → {r}", path.join(" → ")));
                    } else if texts.contains_key(r) {
                        stack.push((r, [path.clone(), vec![r]].concat()));
                    }
                }
            }
        }
    }
    problems.dedup();
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

#[test]
fn references_are_resolved() {
    let fr = catalog(DEFAULT_LOCALE);
    let (key, text) = fr.iter().find(|(_, t)| !references(t).is_empty()).expect("a text with a reference");
    let shown = translate(key, &[]);
    assert!(!shown.contains("$t("), "{key}: {text} → {shown}");
    assert_eq!(references("$t(a.b) et $t(c)"), ["a.b", "c"]);
    assert_eq!(resolve(DEFAULT_LOCALE, "$t(no.such.key) !"), "$t(no.such.key) !");
}

#[test]
fn no_catalog_holds_an_ellipsis() {
    let found: Vec<String> = available_locales()
        .iter()
        .flat_map(|l| catalog(l).into_iter().filter(|(_, t)| t.contains(ELLIPSIS)).map(move |(k, _)| format!("{l}: `{k}`")))
        .collect();
    assert!(found.is_empty(), "the code adds the ellipsis (`ellipsis`), not the catalog:\n{}", found.join("\n"));
}

#[test]
fn plurals() {
    assert!(!is_plural("fr", 0) && !is_plural("fr", 1) && is_plural("fr", 2));
    assert!(is_plural("en", 0) && !is_plural("en-GB", 1) && is_plural("en", 2));
}

#[test]
fn texts_numbers_and_the_locale() {
    assert_eq!(locale(), DEFAULT_LOCALE);
    assert_eq!(fill("%{a} et %{b}", &[("a", "1".into()), ("b", "2".into())]), "1 et 2");
    assert_eq!(translate("no.such.key", &[]), "no.such.key");
    assert_eq!(locale_name("fr"), "Français");
    assert_eq!(number("-12.5"), "-12,5");
    assert_eq!(ellipsis("Ouvrir"), "Ouvrir…");
    assert!(available_locales().contains(&DEFAULT_LOCALE.to_owned()));
}
