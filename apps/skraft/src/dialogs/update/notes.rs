//! A release's notes: its `CHANGELOG.md` entry, the Markdown it uses drawn as such — headings,
//! paragraphs, lists nested by their indentation (each item's lines hanging after its bullet),
//! and in the text **bold**, `code` and links (their text).

/// A block of the notes.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Block {
    /// `#` to `######`: its level and text.
    Heading(usize, String),
    /// `- `, `* ` or `+ `: its depth (0 at the margin) and text, its following lines joined.
    Item(usize, String),
    Paragraph(String),
}

/// The spaces a list's level is indented by.
const LEVEL: usize = 2;

fn blocks(text: &str) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    // A line after an item or a paragraph, not blank, not a block of its own, goes on with it.
    let mut open = false;
    for line in text.lines() {
        let indent = line.len() - line.trim_start().len();
        let line = line.trim();
        if line.is_empty() {
            open = false;
            continue;
        }
        let hashes = line.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
            out.push(Block::Heading(hashes, line[hashes..].trim().to_owned()));
            open = false;
        } else if let Some(item) = ["- ", "* ", "+ "].iter().find_map(|m| line.strip_prefix(m)) {
            out.push(Block::Item(indent / LEVEL, item.trim().to_owned()));
            open = true;
        } else if open && let Some(Block::Item(_, t) | Block::Paragraph(t)) = out.last_mut() {
            t.push(' ');
            t.push_str(line);
        } else {
            out.push(Block::Paragraph(line.to_owned()));
            open = true;
        }
    }
    out
}

/// A run of text and how it is set.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Span {
    text: String,
    bold: bool,
    code: bool,
}

/// A block's text in runs: `**bold**`, `` `code` `` (nothing read within), `[text](url)` as its
/// text. A mark left open sets the rest.
fn spans(text: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let (mut bold, mut code) = (false, false);
    let push = |s: &str, bold: bool, code: bool, out: &mut Vec<Span>| {
        if s.is_empty() {
            return;
        }
        match out.last_mut() {
            Some(last) if last.bold == bold && last.code == code => last.text.push_str(s),
            _ => out.push(Span { text: s.to_owned(), bold, code }),
        }
    };
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '`' {
            code = !code;
            rest = &rest[1..];
        } else if code {
            let end = rest.find('`').unwrap_or(rest.len());
            push(&rest[..end], bold, true, &mut out);
            rest = &rest[end..];
        } else if let Some(after) = rest.strip_prefix("**") {
            bold = !bold;
            rest = after;
        } else if let Some((label, after)) = link(rest) {
            push(label, bold, false, &mut out);
            rest = after;
        } else {
            push(&rest[..c.len_utf8()], bold, false, &mut out);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// `[text](url)` at the start of `s`: its text, and what follows it.
fn link(s: &str) -> Option<(&str, &str)> {
    let inner = s.strip_prefix('[')?;
    let close = inner.find(']')?;
    let after = inner[close + 1..].strip_prefix('(')?;
    let end = after.find(')')?;
    Some((&inner[..close], &after[end + 1..]))
}

/// A bullet's radius (points).
const BULLET: f32 = 2.0;

/// The room a list's level takes, and its bullet's column (points).
const INDENT: f32 = 16.0;

/// The notes drawn in `ui`, as wide as it is.
pub(super) fn show(ui: &mut egui::Ui, text: &str) {
    let body = egui::TextStyle::Body.resolve(ui.style());
    let mut first = true;
    let mut last_item = false;
    for block in blocks(text) {
        match block {
            Block::Heading(level, t) => {
                if !first {
                    ui.add_space(10.0);
                }
                // `#` and `##` a little larger than the text, the rest its size.
                let size = body.size + if level <= 2 { 2.0 } else { 0.0 };
                ui.label(job(ui, &t, crate::kit::style::semibold(size)));
                ui.add_space(2.0);
                last_item = false;
            }
            Block::Item(depth, t) => {
                let left = depth as f32 * INDENT;
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.add_space(left);
                    // As high as the text's first line, the bullet centred on it.
                    let line = ui.fonts_mut(|f| f.row_height(&body));
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(INDENT, line), egui::Sense::hover());
                    bullet(ui, rect.left_center() + egui::vec2(BULLET + 2.0, 0.0), depth);
                    ui.vertical(|ui| ui.add(egui::Label::new(job(ui, &t, body.clone())).wrap()));
                });
                last_item = true;
            }
            Block::Paragraph(t) => {
                if !first && last_item {
                    ui.add_space(4.0);
                }
                ui.add(egui::Label::new(job(ui, &t, body.clone())).wrap());
                last_item = false;
            }
        }
        first = false;
    }
}

/// A list item's bullet at `at`, by its depth as a browser draws them: a disc, a circle, then
/// squares. Drawn, not set in a font: those lack the last two.
fn bullet(ui: &egui::Ui, at: egui::Pos2, depth: usize) {
    let color = ui.visuals().text_color();
    let painter = ui.painter();
    match depth {
        0 => painter.circle_filled(at, BULLET, color),
        1 => painter.circle_stroke(at, BULLET - 0.4, egui::Stroke::new(0.9, color)),
        _ => painter.rect_filled(egui::Rect::from_center_size(at, egui::Vec2::splat(2.0 * BULLET - 0.4)), 0.0, color),
    };
}

/// A block's text laid out: its runs in `font`, bold in semibold, code in the monospace font.
fn job(ui: &egui::Ui, text: &str, font: egui::FontId) -> egui::text::LayoutJob {
    let color = ui.visuals().text_color();
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    for s in spans(text) {
        let font_id = if s.code {
            egui::FontId::new(mono.size.min(font.size), mono.family.clone())
        } else if s.bold {
            crate::kit::style::semibold(font.size)
        } else {
            font.clone()
        };
        let background = if s.code { ui.visuals().code_bg_color } else { egui::Color32::TRANSPARENT };
        job.append(&s.text, 0.0, egui::TextFormat { font_id, color, background, ..Default::default() });
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_follow_the_markdown() {
        let text = "## [0.0.7] - 2026-11-27\n\n### Added\n- one\n  goes on\n  - nested\n- two\n\nA paragraph\non two lines.\n";
        assert_eq!(
            blocks(text),
            vec![
                Block::Heading(2, "[0.0.7] - 2026-11-27".to_owned()),
                Block::Heading(3, "Added".to_owned()),
                Block::Item(0, "one goes on".to_owned()),
                Block::Item(1, "nested".to_owned()),
                Block::Item(0, "two".to_owned()),
                Block::Paragraph("A paragraph on two lines.".to_owned()),
            ]
        );
        assert_eq!(blocks("#tag"), vec![Block::Paragraph("#tag".to_owned())], "no space: not a heading");
    }

    #[test]
    fn spans_read_bold_code_and_links() {
        let s = |text: &str, bold, code| Span { text: text.to_owned(), bold, code };
        assert_eq!(
            spans("**Stretch** opens `a **b**` [here](https://x.y)."),
            vec![s("Stretch", true, false), s(" opens ", false, false), s("a **b**", false, true), s(" here.", false, false)]
        );
        assert_eq!(spans("**open"), vec![s("open", true, false)]);
        assert_eq!(spans("[not a link"), vec![s("[not a link", false, false)]);
    }
}
