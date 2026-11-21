//! A document's drawing as a PDF (docs/ui/drawing.md#pdf): a page per sheet, at its paper's
//! size; the views' lines, the lines, rectangles and texts as vectors (the texts in the app's
//! font, embedded); the views' pictures (*Rendered*, *Hybrid*) and the images as images.

use glam::DVec2;
use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Size, Transform};
use krilla::image::Image;
use krilla::page::PageSettings;
use krilla::paint::{LineCap, LineJoin, Stroke, StrokeDash};
use krilla::text::{Font, TextDirection};
use krilla::{Data, Document};
use skraft_model::{ElementKind, View, ViewKind, ViewStyle};
use skraft_tools::Editor;

use crate::sheet::{Fields, Wanted};

/// The pictures' resolution, dots per inch.
pub const DPI: f64 = 200.0;
/// Points (PDF units) per millimetre.
const PT: f64 = 72.0 / 25.4;

/// Line widths on paper (millimetres): a view's visible and hidden lines, a free line's; the
/// hidden lines' dash and gap.
const LINE_MM: f64 = 0.35;
const HIDDEN_MM: f64 = 0.18;
const THIN_MM: f64 = 0.25;
const DASH_MM: [f64; 2] = [2.0, 1.0];

/// The drawing of `e`'s document as a PDF, every sheet a page; `title` its `{document}` field;
/// `picture` draws a view's picture (RGBA, rows top first).
pub fn drawing_pdf(e: &Editor, title: &str, mut picture: impl FnMut(&Wanted) -> Option<(u32, u32, Vec<u8>)>) -> Result<Vec<u8>, String> {
    let d = e.document().drawing();
    if d.sheets.is_empty() {
        return Err(skraft_i18n::t!("drawing.no_sheet"));
    }
    let font = Font::new(Data::from(crate::style::PLEX_SANS.to_vec()), 0).ok_or("font")?;
    let mut document = Document::new();
    for (k, sheet) in d.sheets.iter().enumerate() {
        let (w, h) = (sheet.paper.width, sheet.paper.height);
        let settings = PageSettings::from_wh((w * PT) as f32, (h * PT) as f32).ok_or("paper")?;
        let mut page = document.start_page_with(settings);
        let mut surface = page.surface();
        // Sheet millimetres (y up) to page points (y down).
        let at = |p: DVec2| Point::from_xy((p.x * PT) as f32, ((h - p.y) * PT) as f32);
        let stroke = |mm: f64, grey: u8, dash: Option<[f64; 2]>| Stroke {
            paint: rgb::Color::new(grey, grey, grey).into(),
            width: (mm * PT) as f32,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            dash: dash.map(|[a, b]| StrokeDash { array: vec![(a * PT) as f32, (b * PT) as f32], offset: 0.0 }),
            ..Stroke::default()
        };
        let fields = Fields {
            document: title,
            sheet: &sheet.name,
            number: k + 1,
            count: d.sheets.len(),
            scale: d.elements_of(sheet).find_map(|el| match &el.kind {
                ElementKind::View(View { kind: ViewKind::Base { scale, .. }, .. }) => Some(*scale),
                _ => None,
            }),
        };
        for el in d.elements_of(sheet) {
            if d.layer(el.layer).is_some_and(|l| l.hidden) {
                continue;
            }
            match &el.kind {
                ElementKind::View(v) => {
                    let Some(r) = d.resolve(el.id) else { continue };
                    let Some(p) = crate::sheet::printed(e, el.id, v.style, &r, DPI) else { continue };
                    let (lo, hi) = p.frame;
                    // Its picture under its lines.
                    if let Some(w) = &p.picture
                        && let Some((pw, ph, rgba)) = picture(w)
                    {
                        let top_left = at(DVec2::new(lo.x, hi.y));
                        surface.push_transform(&Transform::from_translate(top_left.x, top_left.y));
                        if let Some(size) = Size::from_wh(((hi.x - lo.x) * PT) as f32, ((hi.y - lo.y) * PT) as f32) {
                            surface.draw_image(Image::from_rgba8(rgba, pw, ph), size);
                        }
                        surface.pop();
                    }
                    if let Some(lines) = &p.lines {
                        let on_sheet = |q: &DVec2| at(r.at + (*q - p.centre) * r.scale);
                        if v.style == ViewStyle::Lines {
                            polylines(&mut surface, &lines.hidden, on_sheet, stroke(HIDDEN_MM, 90, Some(DASH_MM)));
                        }
                        polylines(&mut surface, &lines.visible, on_sheet, stroke(LINE_MM, 10, None));
                    }
                }
                ElementKind::Text { at: base, text, size } => {
                    let shown = fields.fill(text);
                    let lines: Vec<&str> = shown.lines().collect();
                    let n = lines.len().max(1);
                    // Bottom anchored, as on the sheet: the last line's baseline a descent up.
                    for (i, line) in lines.iter().enumerate() {
                        let baseline = base.y + 0.25 * size + (n - 1 - i) as f64 * 1.25 * size;
                        surface.draw_text(
                            at(DVec2::new(base.x, baseline)),
                            font.clone(),
                            (size * PT) as f32,
                            line,
                            false,
                            TextDirection::Auto,
                        );
                    }
                }
                ElementKind::Line { from, to } => {
                    polylines(&mut surface, &[vec![*from, *to]], |q| at(*q), stroke(THIN_MM, 30, None));
                }
                ElementKind::Rect { min, max } => {
                    let ring = vec![*min, DVec2::new(max.x, min.y), *max, DVec2::new(min.x, max.y), *min];
                    polylines(&mut surface, &[ring], |q| at(*q), stroke(THIN_MM, 30, None));
                }
                ElementKind::Dimension(dim) => {
                    let Some(s) = crate::sheet::dim_shape(e, dim) else { continue };
                    let grey = if s.orphan { 0 } else { 25 };
                    let lines: Vec<Vec<DVec2>> = s.lines.iter().map(|[a, b]| vec![*a, *b]).collect();
                    polylines(&mut surface, &lines, |q| at(*q), stroke(HIDDEN_MM, grey, None));
                    // The arrows filled.
                    let mut pb = PathBuilder::new();
                    for [t, a, b] in &s.arrows {
                        let (t, a, b) = (at(*t), at(*a), at(*b));
                        pb.move_to(t.x, t.y);
                        pb.line_to(a.x, a.y);
                        pb.line_to(b.x, b.y);
                        pb.close();
                    }
                    if let Some(path) = pb.finish() {
                        surface.set_stroke(None);
                        surface.set_fill(Some(krilla::paint::Fill::default()));
                        surface.draw_path(&path);
                    }
                    // The text about its middle, turned (the page's y down).
                    let size = (crate::dims::TEXT_MM * PT) as f32;
                    let width = s.text.chars().count() as f32 * size * 0.55;
                    let c = at(s.text_at);
                    surface.push_transform(&Transform::from_row(1.0, 0.0, 0.0, 1.0, c.x, c.y));
                    surface.push_transform(&Transform::from_rotate(-(s.text_angle.to_degrees() as f32)));
                    surface.draw_text(Point::from_xy(-width / 2.0, size * 0.35), font.clone(), size, &s.text, false, TextDirection::Auto);
                    surface.pop();
                    surface.pop();
                }
                ElementKind::Image { asset, min, max } => {
                    let Some(a) = e.document().asset(asset) else { continue };
                    let data = Data::from(a.bytes.to_vec());
                    let Ok(image) = Image::from_png(data.clone(), true).or_else(|_| Image::from_jpeg(data, true)) else { continue };
                    let top_left = at(DVec2::new(min.x, max.y));
                    surface.push_transform(&Transform::from_translate(top_left.x, top_left.y));
                    if let Some(size) = Size::from_wh(((max.x - min.x) * PT) as f32, ((max.y - min.y) * PT) as f32) {
                        surface.draw_image(image, size);
                    }
                    surface.pop();
                }
            }
        }
        surface.finish();
        page.finish();
    }
    document.finish().map_err(|e| format!("{e:?}"))
}

/// `lines` (each a polyline) stroked as one path.
fn polylines(surface: &mut krilla::surface::Surface<'_>, lines: &[Vec<DVec2>], at: impl Fn(&DVec2) -> Point, stroke: Stroke) {
    let mut pb = PathBuilder::new();
    for poly in lines.iter().filter(|p| p.len() > 1) {
        let p0 = at(&poly[0]);
        pb.move_to(p0.x, p0.y);
        for q in &poly[1..] {
            let q = at(q);
            pb.line_to(q.x, q.y);
        }
    }
    let Some(path) = pb.finish() else { return };
    surface.set_fill(None);
    surface.set_stroke(Some(stroke));
    surface.draw_path(&path);
    surface.set_stroke(None);
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use glam::DVec2;
    use skraft_model::kernel::{Job, JobId, JobResult, KernelService};
    use skraft_model::{Direction, Document, ElementKind, Look, Operation, Placing, Side, Subject, View, ViewKind, ViewStyle};
    use skraft_tools::Editor;

    /// Holds the kernel's jobs; `settle` runs them.
    #[derive(Clone, Default)]
    struct Hold {
        jobs: Rc<RefCell<Vec<(JobId, Job)>>>,
        ready: Rc<RefCell<Vec<(JobId, JobResult)>>>,
    }
    impl KernelService for Hold {
        fn submit(&mut self, id: JobId, job: Job) {
            self.jobs.borrow_mut().push((id, job));
        }
        fn poll(&mut self) -> Vec<(JobId, JobResult)> {
            std::mem::take(&mut *self.ready.borrow_mut())
        }
    }
    fn settle(e: &mut Editor, hold: &Hold) {
        loop {
            let jobs: Vec<(JobId, Job)> = std::mem::take(&mut *hold.jobs.borrow_mut());
            if jobs.is_empty() {
                break;
            }
            let results = skraft_kernel::parallel_map(&jobs, |(id, job)| (*id, skraft_kernel::run(job)));
            hold.ready.borrow_mut().extend(results);
            e.poll();
        }
    }

    /// A sheet from the default template, a view of the model from the front and its right and
    /// above projections, all in lines; written as a PDF.
    fn sheet_of(doc: Document) -> (Editor, Vec<u8>) {
        let hold = Hold::default();
        let mut e = Editor::new(doc, Box::new(hold.clone()));
        settle(&mut e, &hold);
        let template = Arc::new(Document::default_template());
        let sheet = e.apply(Operation::AddSheetFrom { template, name: None }).unwrap().unwrap().sheet.unwrap();
        settle(&mut e, &hold);
        let at = Placing { sheet, layer: None };
        let view = |kind| ElementKind::View(View { name: "V".into(), kind, style: ViewStyle::Lines });
        let base =
            ViewKind::Base { subject: Subject::Model, look: Look::Standard(Direction::Front), scale: 0.1, at: DVec2::new(110.0, 110.0) };
        let front = e.apply(Operation::AddElement { at: at.clone(), element: view(base) }).unwrap().unwrap().element.unwrap();
        for (side, distance) in [(Side::Right, 150.0), (Side::Above, 110.0)] {
            e.apply(Operation::AddElement { at: at.clone(), element: view(ViewKind::Projected { base: front, side, distance }) }).unwrap();
        }
        settle(&mut e, &hold);
        let bytes = super::drawing_pdf(&e, "Test.skraft", |_| None).unwrap();
        (e, bytes)
    }

    #[test]
    fn a_drawing_is_a_pdf_a_page_a_sheet() {
        let mut doc = Document::new();
        let root = doc.root();
        let mut h = skraft_model::History::new(doc);
        h.apply(Operation::AddPrimitive {
            context: root,
            primitive: skraft_model::Primitive::Box { origin: glam::DVec3::ZERO, size: glam::DVec3::new(600.0, 400.0, 300.0) },
            placement: glam::DAffine3::IDENTITY,
            material: None,
        })
        .unwrap();
        doc = h.document().clone();
        let (_, bytes) = sheet_of(doc);
        assert!(bytes.starts_with(b"%PDF-"), "a PDF");
        assert!(bytes.trim_ascii_end().ends_with(b"%%EOF"), "written to its end");
        assert!(bytes.len() > 2_000, "the title block's font and the lines: {} bytes", bytes.len());
    }

    /// The drawing of a model (`DRAW_MODEL`) written to `DRAW_OUT/drawing.pdf`, to look at.
    #[test]
    #[ignore]
    fn write_a_model_drawing() {
        let (Ok(path), Ok(out)) = (std::env::var("DRAW_MODEL"), std::env::var("DRAW_OUT")) else {
            eprintln!("DRAW_MODEL or DRAW_OUT unset: nothing to do");
            return;
        };
        let doc = Document::read_file(&std::fs::read(path).unwrap()).unwrap().0;
        let (_, bytes) = sheet_of(doc);
        std::fs::write(std::path::Path::new(&out).join("drawing.pdf"), bytes).unwrap();
    }
}
