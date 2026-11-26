//! The « Matières » card (docs/ui/catalog.md, docs/render/look.md): the document's materials,
//! coverings and colorations in a tab each, as tiles or rows, how many parts show each, the
//! unused ones apart. A click chooses one (what the paint tool paints with, its properties in
//! the entity info) without changing the tool.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use skraft_i18n::{t, tp};
use skraft_model::{Document, Family, Id, Operation};
use skraft_tools::search::Matcher;
use skraft_tools::{Editor, ToolKind};

use crate::commands::Action;
use crate::kit::icons;
use crate::kit::style::Tip;
use crate::system::images::{Decoder, Size};
use crate::window::ribbon::Corners;
use crate::workspaces::common::cards::NEW_COLORS;
use crate::workspaces::common::cards::catalog::{self, Card, Ink, LIST_ROW, Renaming, Rows, Shelf};
use crate::workspaces::common::cards::library::natural_cmp;

/// What the card keeps of a document between frames (never on disk): its search, its tab, its
/// rows, made again when the document or the search changes.
#[derive(Default)]
pub struct MaterialsState {
    search: String,
    /// The family whose tab is shown.
    pub family: Family,
    rows: Option<(u64, String, [Rows; 3])>,
}

/// How the card is drawn: as a list or tiles, its height, where its tabs' band starts.
pub struct Look {
    pub list: bool,
    /// The height the card gives its content.
    pub height: f32,
    /// The bottom of the card's title bar, where the tabs' band goes on from it, and the
    /// card's inner margin, which the band spans (as the library's, `library::Look`).
    pub band_top: f32,
    pub margin: f32,
}

/// The items of `family` that `matcher` lets through: those a part or an unused component
/// carries first (with how many parts of the model show them), then those nothing refers to.
fn rows_of(doc: &Document, matcher: &Matcher, family: Family) -> Rows {
    let counts = doc.material_counts();
    let referenced = doc.materials_referenced();
    let mut rows = Rows::default();
    for (id, _) in doc.family(family).filter(|(_, m)| matcher.name([m.name.as_str()])) {
        match counts.get(&id) {
            Some(&n) => rows.used.push((id, n)),
            None if referenced.contains(&id) => rows.used.push((id, 0)),
            None => rows.unused.push(id),
        }
    }
    let name = |id: &Id| doc.material(*id).map_or("", |m| m.name.as_str());
    rows.used.sort_by(|a, b| natural_cmp(name(&a.0), name(&b.0)));
    rows.unused.sort_by(|a, b| natural_cmp(name(a), name(b)));
    rows
}

/// The card's content, in `area`: the tabs (materials, coverings, colorations), the search, the
/// buttons (new, export, import) and the grid / list toggle, then the tab's items in their
/// sections.
pub fn card(ui: &mut egui::Ui, editor: &Editor, state: &mut MaterialsState, look: Look, actions: &mut Vec<Action>, area: egui::ScrollArea) {
    let doc = editor.document();
    let top = ui.cursor().top();
    let matcher = Matcher::new(&state.search);
    let all = crate::kit::search::listed(&mut state.rows, editor.history().revision(), &state.search, || {
        Family::ALL.map(|f| rows_of(doc, &matcher, f))
    });
    // The tabs, as the library's, without counts: three would not fit the card's width.
    let labels = [t!("panel.materials.materials_tab"), t!("panel.materials.coverings_tab"), t!("panel.materials.colorations_tab")];
    let ids = [
        ui.make_persistent_id("materials_tab_materials"),
        ui.make_persistent_id("materials_tab_coverings"),
        ui.make_persistent_id("materials_tab_colorations"),
    ];
    let m = look.margin as i8;
    let inset = egui::Margin { left: m, right: m, top: (ui.cursor().top() - look.band_top).max(0.0).ceil() as i8, bottom: 0 };
    let handles = ["materials:materials_tab", "materials:coverings_tab", "materials:colorations_tab"];
    let shown = Family::ALL.iter().position(|f| *f == state.family).unwrap_or(0);
    if let Some(k) =
        crate::window::ribbon::tab_strip(ui, &labels, &ids, Some(shown), ui.available_width(), inset, Corners::Square, &handles)
    {
        state.family = Family::ALL[k];
    }
    ui.add_space(crate::kit::search::GAP - ui.spacing().item_spacing.y);
    let family = state.family;
    let rows = &all[shown];
    let (as_list, height) = (look.list, look.height);
    let salt = ui.make_persistent_id("materials_search");
    crate::kit::search::field(ui, salt, "materials", &mut state.search, |ui, side| {
        catalog::view_toggle(ui, side, as_list, Card::Materials, actions);
        buttons(ui, doc, side, family, actions);
    });
    ui.add_space(crate::kit::search::GAP - ui.spacing().item_spacing.y);
    if rows.is_empty() && matcher.is_active() {
        ui.weak(t!("search.no_match"));
        return;
    }
    let area = catalog::fit(ui, area, height, top);
    let mut shelf = Materials {
        editor,
        doc,
        painting: editor.tool_kind() == ToolKind::Paint,
        current: editor.paint_material(),
        // « Aucune » is this tab's when the tool takes this family off.
        none: editor.paint_material().is_none() && editor.paint_family() == family,
        family,
        lead: !matcher.is_active(),
    };
    catalog::show(ui, &mut shelf, rows, as_list, actions, area);
}

/// The search row's buttons, laid right to left: import, export all, new (an item of the tab's
/// family, its first type).
fn buttons(ui: &mut egui::Ui, doc: &Document, side: f32, family: Family, actions: &mut Vec<Action>) {
    let ink = ui.visuals().text_color();
    let button = |ui: &mut egui::Ui, enabled: bool, icon: egui::ImageSource<'static>, tip: String, handle: &'static str| {
        let image = egui::Image::new(icon).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(ink);
        let r = ui.add_enabled(enabled, egui::Button::image(image).min_size(egui::Vec2::splat(side)));
        crate::kit::handles::publish_enabled(|| handle.into(), r.rect, enabled);
        r.tip(tip).clicked()
    };
    if button(ui, true, icons::IMPORT, t!("panel.materials.import_tip"), "materials:import") {
        actions.push(Action::ImportMaterials);
    }
    let any = doc.materials().next().is_some();
    if button(ui, any, icons::EXPORT, t!("panel.materials.export_tip"), "materials:export") {
        actions.push(Action::ExportMaterials(None));
    }
    let tip = match family {
        Family::Material => t!("panel.materials.new"),
        Family::Covering => t!("panel.materials.new_covering"),
        Family::Coloration => t!("panel.materials.new_coloration"),
    };
    if button(ui, true, icons::ADD, tip, "materials:new") {
        let count = doc.materials().count();
        let taken = |n: &str| doc.materials().any(|(_, m)| m.name == n);
        let named = |k| match family {
            Family::Material => t!("default_name.material", number = k),
            Family::Covering => t!("default_name.covering", number = k),
            Family::Coloration => t!("default_name.coloration", number = k),
        };
        let name = (1..).map(named).find(|n| !taken(n)).unwrap_or_default();
        let kind = family.kinds().first().copied();
        actions.push(Action::Apply(Operation::AddMaterial { name, color: NEW_COLORS[count % NEW_COLORS.len()], kind }));
    }
}

/// The card's items, for the shared presentation.
struct Materials<'a> {
    editor: &'a Editor,
    doc: &'a Document,
    painting: bool,
    current: Option<Id>,
    /// The tool takes this tab's family off (« Aucune » chosen here).
    none: bool,
    /// The tab's family.
    family: Family,
    /// « Aucune » shown first: not under a search.
    lead: bool,
}

impl Shelf for Materials<'_> {
    fn rename_salt(&self) -> &'static str {
        "materials_rename"
    }

    fn has(&self, id: Id) -> bool {
        self.doc.material(id).is_ok()
    }

    fn name(&self, id: Id) -> (String, bool) {
        (self.doc.material(id).map_or_else(|_| String::new(), |m| m.name.clone()), false)
    }

    fn ink(&mut self, ui: &egui::Ui, id: Id) -> Ink {
        Ink::new(ui, self.current == Some(id), false)
    }

    fn picture(&mut self, ui: &mut egui::Ui, rect: egui::Rect, id: Id, _ink: &Ink) {
        let Ok(m) = self.doc.material(id) else { return };
        // A scheme: its coats, a band each, the bottom one below.
        if m.is_scheme() {
            let coats = self.doc.coats_of(id);
            ui.painter().rect_filled(rect, 4.0, ui.visuals().faint_bg_color);
            let n = coats.len().max(1) as f32;
            for (k, (_, c)) in coats.iter().enumerate() {
                let [r, g, b] = c.color;
                let top = rect.bottom() - rect.height() * (k + 1) as f32 / n;
                let band = egui::Rect::from_min_max(egui::pos2(rect.left(), top), egui::pos2(rect.right(), top + rect.height() / n));
                ui.painter().rect_filled(band, 0.0, egui::Color32::from_rgb(r, g, b));
            }
            return;
        }
        let [r, g, b] = m.color;
        ui.painter().rect_filled(rect, 4.0, egui::Color32::from_rgb(r, g, b));
        if let Some(t) = &m.texture
            && let Some(tex) = thumbnail(ui.ctx(), self.doc, &t.image)
        {
            paint_texture(ui, &tex, quarters(t.rotation), rect, true, 4.0);
        }
    }

    fn interact(
        &mut self,
        ui: &mut egui::Ui,
        id: Id,
        count: Option<u64>,
        response: egui::Response,
        rename: Renaming,
        actions: &mut Vec<Action>,
    ) {
        crate::kit::handles::publish(|| format!("materials:{}", id.0), response.rect);
        let name = self.name(id).0;
        let tip = match count {
            Some(0) => t!("panel.materials.only_unused_tip"),
            Some(n) => tp!("panel.materials.count_tip", n as usize),
            None => t!("panel.materials.unused_tip"),
        };
        let response = response.tip(format!("{name}\n{tip}"));
        let key = rename.key;
        let chosen = self.editor.chosen_material() == Some(id);
        if let Some(name) = rename.gesture(ui, &response, chosen)
            && let Ok(m) = self.doc.material(id)
        {
            actions.push(Action::Apply(Operation::EditMaterial { material: id, name, color: m.color }));
        }
        if response.clicked() {
            actions.push(Action::ChooseMaterial(Some(id)));
        }
        // The brush over the picture, on the item under the pointer or the current one.
        if self.current == Some(id) || ui.rect_contains_pointer(response.rect) {
            let on = self.painting && self.current == Some(id);
            if brush(ui, &response, on, &t!("panel.materials.paint_tip"), &format!("materials:{}:paint", id.0)) {
                actions.push(Action::PaintWith(Some(id)));
            }
        }
        egui::Popup::context_menu(&response).style(crate::kit::style::menu).show(|ui| {
            if ui.button(t!("panel.materials.paint")).clicked() {
                actions.push(Action::PaintWith(Some(id)));
            }
            ui.separator();
            if ui.button(t!("panel.components.rename")).clicked() {
                Renaming::start(ui, key);
            }
            if ui.button(t!("panel.materials.export_one")).clicked() {
                actions.push(Action::ExportMaterials(Some(id)));
            }
            if ui.button(t!("panel.components.delete")).tip(t!("panel.materials.delete_tip")).clicked() {
                actions.push(Action::DeleteMaterial(id));
            }
        });
    }

    fn header(&mut self, ui: &mut egui::Ui, height: f32, unused: bool, actions: &mut Vec<Action>) {
        let title = if unused { t!("panel.materials.unused") } else { t!("panel.materials.in_use") };
        let purge = unused.then(|| (true, t!("panel.materials.purge_tip"), "materials:purge"));
        if catalog::header(ui, height, title, purge) {
            actions.push(Action::Apply(Operation::PurgeMaterials));
        }
    }

    fn lead(&self) -> Option<String> {
        self.lead.then(|| t!("panel.materials.none"))
    }

    fn lead_ink(&mut self, ui: &egui::Ui) -> Ink {
        Ink::new(ui, self.painting && self.none, false)
    }

    /// No material: a frame crossed out.
    fn lead_picture(&mut self, ui: &mut egui::Ui, rect: egui::Rect, ink: &Ink) {
        ui.painter().rect_filled(rect, 4.0, ui.visuals().faint_bg_color);
        let inner = rect.shrink(rect.width() * 0.2);
        ui.painter().line_segment([inner.left_bottom(), inner.right_top()], egui::Stroke::new(1.0, ink.weak));
    }

    fn lead_interact(&mut self, ui: &mut egui::Ui, response: egui::Response, actions: &mut Vec<Action>) {
        crate::kit::handles::publish(|| "materials:none".into(), response.rect);
        let response = response.tip(t!("panel.materials.unpaint_tip"));
        if response.clicked() {
            actions.push(Action::PaintFamily(self.family));
            actions.push(Action::ChooseMaterial(None));
        }
        if ui.rect_contains_pointer(response.rect) || (self.painting && self.none) {
            let on = self.painting && self.none;
            if brush(ui, &response, on, &t!("panel.materials.unpaint_tip"), "materials:none:paint") {
                actions.push(Action::PaintFamily(self.family));
                actions.push(Action::PaintWith(None));
            }
        }
    }
}

/// The small brush over an item's picture: true when clicked (the paint tool starts).
fn brush(ui: &mut egui::Ui, item: &egui::Response, on: bool, tip: &str, handle: &str) -> bool {
    const SIDE: f32 = 18.0;
    let rect = item.rect;
    // In a tile, its top left corner; in a row, level with the picture.
    let dy = if rect.height() > LIST_ROW + 1.0 { 3.0 } else { (rect.height() - SIDE) / 2.0 };
    let at = egui::Rect::from_min_size(rect.min + egui::vec2(3.0, dy), egui::Vec2::splat(SIDE));
    let r = ui.interact(at, item.id.with("paint"), egui::Sense::click());
    crate::kit::handles::publish(|| handle.to_owned(), at);
    let v = ui.visuals();
    let fill = if on {
        v.selection.bg_fill
    } else if r.hovered() {
        v.widgets.hovered.weak_bg_fill
    } else {
        v.window_fill.gamma_multiply(0.85)
    };
    ui.painter().rect_filled(at, 4.0, fill);
    let ink = if on { v.selection.stroke.color } else { v.text_color() };
    egui::Image::new(icons::PAINT).tint(ink).paint_at(ui, egui::Rect::from_center_size(at.center(), egui::vec2(12.0, 12.0)));
    r.tip(tip).clicked()
}

/// A texture's rotation (radians) as quarter turns, the nearest: the turns the interface offers
/// (docs/render/look.md#textures).
pub(crate) fn quarters(rotation: f64) -> u8 {
    (rotation / std::f64::consts::FRAC_PI_2).round().rem_euclid(4.0) as u8
}

/// A texture's image size (texels) once turned by `quarter` quarter turns.
pub(crate) fn turned_size(tex: &egui::TextureHandle, quarter: u8) -> [f32; 2] {
    let [w, h] = tex.size().map(|v| v.max(1) as f32);
    if quarter % 2 == 1 { [h, w] } else { [w, h] }
}

/// A texture's thumbnail `tex` in `rect`, turned anticlockwise by `quarter` quarter turns as the
/// texture's rotation turns it on the parts; `cover`: its middle filling `rect`, its proportions
/// kept (else all of it, stretched to `rect`). Its corners rounded by `radius` when not turned
/// (egui rounds only a rectangle of the image).
pub(crate) fn paint_texture(ui: &egui::Ui, tex: &egui::TextureHandle, quarter: u8, rect: egui::Rect, cover: bool, radius: f32) {
    let [w, h] = turned_size(tex, quarter);
    let (du, dv) = if cover {
        let (image, room) = (w / h, rect.width() / rect.height().max(1.0));
        if image > room { ((1.0 - room / image) / 2.0, 0.0) } else { (0.0, (1.0 - image / room) / 2.0) }
    } else {
        (0.0, 0.0)
    };
    if quarter % 4 == 0 {
        let uv = egui::Rect::from_min_max(egui::pos2(du, dv), egui::pos2(1.0 - du, 1.0 - dv));
        ui.painter().add(egui::epaint::RectShape::filled(rect, radius, egui::Color32::WHITE).with_texture(tex.id(), uv));
        return;
    }
    // A point of the turned image (0 to 1, from its top left) taken from the image itself.
    let source = |x: f32, y: f32| match quarter % 4 {
        1 => egui::pos2(1.0 - y, x),
        2 => egui::pos2(1.0 - x, 1.0 - y),
        _ => egui::pos2(y, 1.0 - x),
    };
    let mut mesh = egui::Mesh::with_texture(tex.id());
    let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
    for (corner, (x, y)) in corners.into_iter().zip([(du, dv), (1.0 - du, dv), (1.0 - du, 1.0 - dv), (du, 1.0 - dv)]) {
        mesh.vertices.push(egui::epaint::Vertex { pos: corner, uv: source(x, y), color: egui::Color32::WHITE });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(mesh);
}

// ---- the textures' thumbnails ------------------------------------------------------------------

/// A thumbnail's side (texels).
const THUMB_SIZE: u32 = 256;

/// The thumbnails made, by image id (the same image, the same thumbnail in every document), and
/// those on their way. Only the interface's thread asks for them.
#[derive(Default)]
struct Thumbs {
    made: HashMap<String, Option<egui::TextureHandle>>,
    asked: HashSet<String>,
    decoder: Decoder,
}

thread_local! {
    static THUMBS: RefCell<Thumbs> = RefCell::new(Thumbs::default());
}

/// The thumbnail of the image `id` of `doc`, none while it is decoded (off the interface's
/// thread: a large JPEG takes ~100 ms) or when it does not read.
pub(crate) fn thumbnail(ctx: &egui::Context, doc: &Document, id: &str) -> Option<egui::TextureHandle> {
    THUMBS.with_borrow_mut(|thumbs| {
        for (key, image) in thumbs.decoder.take(ctx) {
            let made = image.map(|i| {
                let size = [i.size[0] as usize, i.size[1] as usize];
                let image = egui::ColorImage::from_rgba_unmultiplied(size, &i.rgba);
                ctx.load_texture(format!("thumbnail {key}"), image, egui::TextureOptions::LINEAR)
            });
            thumbs.asked.remove(&key);
            thumbs.made.insert(key, made);
        }
        if let Some(made) = thumbs.made.get(id) {
            return made.clone();
        }
        if !thumbs.asked.contains(id)
            && let Some(asset) = doc.asset(id)
        {
            thumbs.asked.insert(id.to_owned());
            thumbs.decoder.ask(ctx, id.to_owned(), asset.bytes.clone(), Size::Fit(THUMB_SIZE));
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use skraft_model::{Asset, History, Texture};

    use super::*;

    #[test]
    fn a_texture_thumbnail_comes_from_the_decoders_without_blocking() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(300, 200, image::Rgba([200, 150, 100, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let asset = Asset::new(png.into_inner()).unwrap();
        let image = asset.id();
        let mut h = History::new(Document::new());
        h.apply(Operation::AddMaterial { name: "Chêne".into(), color: [1, 2, 3], kind: Some(skraft_model::MaterialKind::Solid) }).unwrap();
        let material = h.document().materials().next().unwrap().0;
        let texture = Texture { image: image.clone(), size: [600.0, 400.0], rotation: 0.0 };
        h.apply(Operation::SetTexture { material, texture: Some(texture), image: Some(asset), color: None }).unwrap();
        let ctx = egui::Context::default();
        assert!(thumbnail(&ctx, h.document(), &image).is_none(), "asked: not there yet");
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let made = loop {
            if let Some(made) = thumbnail(&ctx, h.document(), &image) {
                break made;
            }
            assert!(std::time::Instant::now() < end, "never decoded");
            std::thread::yield_now();
        };
        assert_eq!(made.size()[0], 256);
    }
}
