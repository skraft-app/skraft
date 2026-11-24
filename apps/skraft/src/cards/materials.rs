//! The « Matières » card (docs/ui/catalog.md, docs/render/look.md): the document's materials as
//! tiles or rows, how many parts show each, the unused ones apart. A click chooses one (what the
//! paint tool paints with, its properties in the entity info) without changing the tool.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use skraft_i18n::{t, tp};
use skraft_model::{Document, Id, Operation};
use skraft_tools::search::Matcher;
use skraft_tools::{Editor, ToolKind};

use crate::cards::catalog::{self, Card, Ink, LIST_ROW, Renaming, Rows, Shelf};
use crate::cards::library::natural_cmp;
use crate::cards::panels::NEW_COLORS;
use crate::commands::Action;
use crate::kit::icons;
use crate::kit::style::Tip;
use crate::system::images::{Decoder, Size};

/// What the card keeps of a document between frames (never on disk): its search, its rows, made
/// again when the document or the search changes.
#[derive(Default)]
pub struct MaterialsState {
    search: String,
    rows: Option<(u64, String, Rows)>,
}

/// The materials `matcher` lets through: those a part or an unused component carries first
/// (with how many parts of the model show them), then those nothing refers to.
fn rows_of(doc: &Document, matcher: &Matcher) -> Rows {
    let counts = doc.material_counts();
    let referenced = doc.materials_referenced();
    let mut rows = Rows::default();
    for (id, _) in doc.materials().filter(|(_, m)| matcher.name([m.name.as_str()])) {
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

/// The card's content, `height` high, in `area`: the search, the buttons (new, export, import)
/// and the grid / list toggle, then the materials in their sections.
pub fn card(
    ui: &mut egui::Ui,
    editor: &Editor,
    state: &mut MaterialsState,
    as_list: bool,
    height: f32,
    actions: &mut Vec<Action>,
    area: egui::ScrollArea,
) {
    let doc = editor.document();
    let top = ui.cursor().top();
    let matcher = Matcher::new(&state.search);
    let rows = crate::kit::search::listed(&mut state.rows, editor.history().revision(), &state.search, || rows_of(doc, &matcher));
    let salt = ui.make_persistent_id("materials_search");
    crate::kit::search::field(ui, salt, "materials", &mut state.search, |ui, side| {
        catalog::view_toggle(ui, side, as_list, Card::Materials, actions);
        buttons(ui, doc, side, actions);
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
        lead: !matcher.is_active(),
    };
    catalog::show(ui, &mut shelf, rows, as_list, actions, area);
}

/// The search row's buttons, laid right to left: import, export all, new.
fn buttons(ui: &mut egui::Ui, doc: &Document, side: f32, actions: &mut Vec<Action>) {
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
    if button(ui, true, icons::ADD, t!("panel.materials.new"), "materials:new") {
        let count = doc.materials().count();
        let taken = |n: &str| doc.materials().any(|(_, m)| m.name == n);
        let name = (count + 1..).map(|k| t!("default_name.material", number = k)).find(|n| !taken(n)).unwrap_or_default();
        actions.push(Action::Apply(Operation::AddMaterial { name, color: NEW_COLORS[count % NEW_COLORS.len()] }));
    }
}

/// The card's items, for the shared presentation.
struct Materials<'a> {
    editor: &'a Editor,
    doc: &'a Document,
    painting: bool,
    current: Option<Id>,
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
        let [r, g, b] = m.color;
        ui.painter().rect_filled(rect, 4.0, egui::Color32::from_rgb(r, g, b));
        if let Some(tex) = m.texture.as_ref().and_then(|t| thumbnail(ui.ctx(), self.doc, &t.image)) {
            egui::Image::new(&tex).uv(centre_square(&tex)).corner_radius(4.0).paint_at(ui, rect);
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
                actions.push(Action::Apply(Operation::DeleteMaterial { material: id }));
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
        Ink::new(ui, self.painting && self.current.is_none(), false)
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
            actions.push(Action::ChooseMaterial(None));
        }
        if ui.rect_contains_pointer(response.rect) || (self.painting && self.current.is_none()) {
            let on = self.painting && self.current.is_none();
            if brush(ui, &response, on, &t!("panel.materials.unpaint_tip"), "materials:none:paint") {
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

/// The middle square of a texture, in its UVs.
pub(crate) fn centre_square(tex: &egui::TextureHandle) -> egui::Rect {
    let [w, h] = tex.size().map(|v| v.max(1) as f32);
    let (du, dv) = ((1.0 - (h / w).min(1.0)) / 2.0, (1.0 - (w / h).min(1.0)) / 2.0);
    egui::Rect::from_min_max(egui::pos2(du, dv), egui::pos2(1.0 - du, 1.0 - dv))
}

// ---- the textures' thumbnails ------------------------------------------------------------------

/// A thumbnail's side (texels).
const THUMB_SIZE: u32 = 128;

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
        h.apply(Operation::AddMaterial { name: "Chêne".into(), color: [1, 2, 3] }).unwrap();
        let material = h.document().materials().next().unwrap().0;
        let texture = Texture { image: image.clone(), size: [600.0, 400.0], rotation: 0.0 };
        h.apply(Operation::SetTexture { material, texture: Some(texture), image: Some(asset) }).unwrap();
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
        assert_eq!(made.size(), [128, 85]);
    }
}
