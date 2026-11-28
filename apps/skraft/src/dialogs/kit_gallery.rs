//! The kit's gallery (docs/ui/interface.md#the-kits-gallery): every widget of the kit, drawn by
//! the kit itself in its states, each under the call that draws it — a tab a family. Opened by
//! « Galerie du kit » (the Débogage menu: a dev build, `SKRAFT_DEBUG`, `?debug` on the web). A widget the kit
//! gains comes here too: a test checks it (`kit/guard.rs`).
//!
//! The captions under the specimens are code (`Look::Framed`, `.lit(true)`), not texts of the
//! interface: they are not translated.

use skraft_i18n::t;
use skraft_model::{Document, FastSet};
use skraft_theme::Theme;

use crate::commands::CommandView;
use crate::kit::style::{self, BAR_BUTTON, BAR_ICON, Tip};
use crate::kit::widgets::color::{Rgb, color_box, paint_swatch};
use crate::kit::widgets::{
    ChoiceList, Field, FieldLook, Fold, FormField, IconButton, Keycaps, LabelButton, Look, MenuEntry, MenuFace, PinEvent, PinItem, PinMenu,
    PinTexts, RENAMING, Segment, TextButton, Tick, TreeLine, badge, badge_button, bar_menu, choice_field, choice_item, command_item,
    confirm_buttons, cut_label, form_choice, form_message, form_row, framed_field, key_toggle, keycaps, keypad, linked_badge, linked_mark,
    live_field, option_button, picture_badge, pin_list, pin_menu, rename_field, segmented, segmented_sized, start_rename, tick, tree_line,
    tree_row, unpin_menu, warning, warning_box,
};
use crate::kit::{icons, overlay, search};
use crate::window::ribbon::{Corners, tab_strip};

/// The gallery's families, a tab each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Buttons,
    Choices,
    Fields,
    Menus,
    Rows,
    Marks,
    Colors,
    Frames,
    Overlays,
}

impl Tab {
    const ALL: [Tab; 9] =
        [Tab::Buttons, Tab::Choices, Tab::Fields, Tab::Menus, Tab::Rows, Tab::Marks, Tab::Colors, Tab::Frames, Tab::Overlays];

    fn label(self) -> String {
        match self {
            Tab::Buttons => t!("gallery.tab.buttons"),
            Tab::Choices => t!("gallery.tab.choices"),
            Tab::Fields => t!("gallery.tab.fields"),
            Tab::Menus => t!("gallery.tab.menus"),
            Tab::Rows => t!("gallery.tab.rows"),
            Tab::Marks => t!("gallery.tab.marks"),
            Tab::Colors => t!("gallery.tab.colors"),
            Tab::Frames => t!("gallery.tab.frames"),
            Tab::Overlays => t!("gallery.tab.overlays"),
        }
    }

    /// Its tab's handle (docs/control-protocol.md).
    fn handle(self) -> &'static str {
        match self {
            Tab::Buttons => "gallery:tab:buttons",
            Tab::Choices => "gallery:tab:choices",
            Tab::Fields => "gallery:tab:fields",
            Tab::Menus => "gallery:tab:menus",
            Tab::Rows => "gallery:tab:rows",
            Tab::Marks => "gallery:tab:marks",
            Tab::Colors => "gallery:tab:colors",
            Tab::Frames => "gallery:tab:frames",
            Tab::Overlays => "gallery:tab:overlays",
        }
    }
}

/// The gallery's state while it is open: what its specimens were set to.
#[derive(Clone, Debug)]
pub struct KitGallery {
    tab: Tab,
    toggle: bool,
    segment: usize,
    segment_icon: usize,
    choice: usize,
    checks: [bool; 2],
    picked: Option<String>,
    framed: String,
    live: String,
    form: [String; 2],
    form_choice: usize,
    search: String,
    keypad: String,
    color: Rgb,
    folds: FastSet<egui::Id>,
    selected: usize,
    names: [String; 4],
    pinned: [bool; 3],
    strip: usize,
    /// The modifier keys of `key_toggle` (⇧ on).
    keys: [bool; 4],
}

impl Default for KitGallery {
    fn default() -> Self {
        let mut folds = FastSet::default();
        folds.insert(fold_key(0));
        folds.insert(fold_key(1));
        KitGallery {
            tab: Tab::Buttons,
            toggle: true,
            segment: 0,
            segment_icon: 1,
            choice: 0,
            checks: [true, false],
            picked: None,
            framed: "120".to_owned(),
            live: t!("gallery.sample.name"),
            form: ["600".to_owned(), "18".to_owned()],
            form_choice: 0,
            search: String::new(),
            keypad: String::new(),
            color: [196, 152, 98],
            folds,
            selected: 1,
            names: [t!("gallery.sample.assembly"), t!("gallery.sample.part"), t!("gallery.sample.body"), t!("gallery.sample.hidden")],
            pinned: [true, false, true],
            strip: 0,
            keys: [true, false, false, false],
        }
    }
}

pub fn window_id() -> egui::Id {
    egui::Id::unique("kit_gallery")
}

/// The window, while `state` is: `views` the commands as they show now (the menus' rows),
/// `theme` the 3D view's (the selection rectangle), `doc` the document shown (its materials).
pub fn show(ctx: &egui::Context, state: &mut Option<KitGallery>, views: &[CommandView], theme: &Theme, doc: Option<&Document>) {
    let Some(g) = state else { return };
    let mut open = true;
    // One width, its content's: a window egui may resize grows with a content taking the width
    // it is given, a little more at each frame.
    let width = style::window_width(ctx, WINDOW);
    style::app_window(ctx, t!("gallery.title"), window_id(), &mut open, WINDOW, None)
        .resizable([false, true])
        .min_width(width)
        .max_width(width)
        .default_size([width, 720.0])
        .min_height(300.0)
        .show(ctx, |ui| {
            let labels: Vec<String> = Tab::ALL.iter().map(|t| t.label()).collect();
            let ids: Vec<egui::Id> = Tab::ALL.iter().map(|t| window_id().with(t.handle())).collect();
            let handles: Vec<&'static str> = Tab::ALL.iter().map(|t| t.handle()).collect();
            let shown = Tab::ALL.iter().position(|t| *t == g.tab);
            // Up to the title bar's line, as the journal's.
            let m = ui.style().spacing.window_margin;
            let inset = egui::Margin { top: m.top + ui.spacing().item_spacing.y as i8, ..m };
            if let Some(k) = tab_strip(ui, &labels, &ids, shown, ui.available_width(), inset, Corners::Square, &handles) {
                g.tab = Tab::ALL[k];
            }
            egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                ui.set_width(NAME + BODY);
                match g.tab {
                    Tab::Buttons => buttons(ui, g),
                    Tab::Choices => choices(ui, g),
                    Tab::Fields => fields(ui, g),
                    Tab::Menus => menus(ui, g, views),
                    Tab::Rows => rows(ui, g),
                    Tab::Marks => marks(ui, doc),
                    Tab::Colors => colors(ui, g),
                    Tab::Frames => frames(ui),
                    Tab::Overlays => overlays(ui, theme),
                }
            });
        });
    if !open {
        *state = None;
    }
}

/// The column of the widgets' names (points).
const NAME: f32 = 170.0;
/// The specimens' grid: a cell's width, the gap ending it, how many in a row (points).
const CELL: f32 = 100.0;
const GAP: f32 = 12.0;
const COLUMNS: f32 = 6.0;
/// The specimens' column.
const BODY: f32 = COLUMNS * CELL;
/// The window's width: its two columns and the room of a scroll bar.
const WINDOW: f32 = NAME + BODY + 18.0;

/// The width of what fills `span` cells of the grid.
fn width(span: usize) -> f32 {
    span as f32 * CELL - GAP
}

/// One widget, a row of the gallery: its names (split at « · », one a line) in the left column,
/// what `add` lays in the right one, a line under them.
fn entry(ui: &mut egui::Ui, name: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(8.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let size = egui::TextStyle::Body.resolve(ui.style()).size;
        let ink = ui.visuals().strong_text_color();
        ui.allocate_ui_with_layout(egui::vec2(NAME, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(NAME - GAP);
            for part in name.split(" · ") {
                ui.add(egui::Label::new(egui::RichText::new(part).font(egui::FontId::monospace(size)).color(ink)).wrap());
            }
        });
        ui.allocate_ui_with_layout(egui::vec2(BODY, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(BODY);
            ui.spacing_mut().item_spacing.x = GAP;
            add(ui);
        });
    });
    ui.add_space(8.0);
    ui.separator();
}

/// Specimens side by side in the grid's cells, their tops lined up, wrapped to the column.
fn row(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(GAP, 12.0);
        add(ui);
    });
}

/// A specimen in `span` cells, centred in them: what `add` draws, the call drawing it under it.
fn specimen<R>(ui: &mut egui::Ui, span: usize, call: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    cells(ui, span, egui::Align::Center, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        let r = add(ui);
        ui.add(egui::Label::new(egui::RichText::new(call).monospace().small().weak()).wrap());
        r
    })
}

/// An empty cell: the cells after it stay in their columns.
fn blank(ui: &mut egui::Ui) {
    ui.allocate_exact_size(egui::vec2(width(1), 0.0), egui::Sense::hover());
}

/// What `add` lays top-down in `span` cells, as wide, from their left.
fn block<R>(ui: &mut egui::Ui, span: usize, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    cells(ui, span, egui::Align::Min, add)
}

/// What `add` lays top-down in `span` cells, as wide, aligned by `align`.
fn cells<R>(ui: &mut egui::Ui, span: usize, align: egui::Align, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.allocate_ui_with_layout(egui::vec2(width(span), 0.0), egui::Layout::top_down(align), |ui| {
        ui.set_width(width(span));
        add(ui)
    })
    .inner
}

/// A rect of `size` laid out for a painter's specimen.
fn space(ui: &mut egui::Ui, size: egui::Vec2) -> egui::Rect {
    ui.allocate_exact_size(size, egui::Sense::hover()).0
}

fn buttons(ui: &mut egui::Ui, g: &mut KitGallery) {
    entry(ui, "IconButton", |ui| {
        for (look, name, lights) in
            [(Look::Plain, "Plain", false), (Look::Framed, "Framed", true), (Look::Veil, "Veil", true), (Look::Bar, "Bar", true)]
        {
            row(ui, |ui| {
                let button = || IconButton::new(icons::BOX, BAR_BUTTON, BAR_ICON).look(look);
                specimen(ui, 1, &format!("Look::{name}"), |ui| button().show(ui));
                if lights {
                    specimen(ui, 1, ".lit(true)", |ui| button().lit(true).show(ui));
                } else {
                    blank(ui);
                }
                specimen(ui, 1, ".enabled(false)", |ui| button().enabled(false).show(ui));
                match name {
                    "Plain" => drop(specimen(ui, 1, ".inert(true)", |ui| button().inert(true).show(ui).tip(t!("gallery.sample.tip")))),
                    "Veil" => drop(specimen(ui, 1, ".natural()", |ui| {
                        IconButton::new(icons::APP, BAR_BUTTON, BAR_ICON).look(look).natural().show(ui)
                    })),
                    _ => {}
                }
            });
        }
        row(ui, |ui| {
            specimen(ui, 1, ".show_at(rect, id)", |ui| {
                let rect = space(ui, egui::Vec2::splat(BAR_BUTTON));
                IconButton::new(icons::CLOSE, BAR_BUTTON, 12.0).show_at(ui, rect, window_id().with("show_at"))
            });
        });
    });
    entry(ui, "TextButton", |ui| {
        row(ui, |ui| {
            let label = t!("gallery.sample.label");
            specimen(ui, 1, "new(label)", |ui| TextButton::new(&label).show(ui));
            specimen(ui, 1, ".icon(icons::ADD)", |ui| TextButton::new(&label).icon(icons::ADD).show(ui));
            specimen(ui, 1, ".lit(true)", |ui| TextButton::new(&label).lit(true).show(ui));
            specimen(ui, 1, ".enabled(false)", |ui| TextButton::new(&label).enabled(false).show(ui));
            specimen(ui, 1, ".small()", |ui| TextButton::new(&label).small().show(ui));
        });
        row(ui, |ui| {
            let label = t!("gallery.sample.label");
            specimen(ui, 2, ".min_size(…)", |ui| TextButton::new(&label).min_size(egui::vec2(120.0, 0.0)).show(ui));
            specimen(ui, 2, ".size(…)", |ui| TextButton::new(t!("gallery.sample.long")).size(egui::vec2(120.0, 22.0)).show(ui));
        });
    });
    entry(ui, "LabelButton", |ui| {
        row(ui, |ui| {
            for (main, call) in [(true, "new(…, main: true)"), (false, "new(…, main: false)")] {
                specimen(ui, 2, call, |ui| {
                    let b = LabelButton::new(ui, icons::BOX, t!("gallery.sample.label"), main);
                    let rect = space(ui, egui::vec2(b.width(), 40.0));
                    b.show_at(ui, rect, if main { "gallery:label_button:main" } else { "gallery:label_button" })
                });
            }
        });
    });
    entry(ui, "confirm_buttons", |ui| {
        row(ui, |ui| {
            for (enabled, call) in [(true, "enabled: true"), (false, "enabled: false")] {
                specimen(ui, 3, call, |ui| {
                    ui.allocate_ui(egui::vec2(width(3), 26.0), |ui| {
                        confirm_buttons(ui, "gallery:confirm", t!("gallery.sample.ok"), enabled, t!("gallery.sample.cancel"))
                    })
                });
            }
        });
    });
    entry(ui, "bar_menu", |ui| {
        row(ui, |ui| {
            let items = |ui: &mut egui::Ui| {
                MenuEntry::new(t!("gallery.sample.item")).show(ui);
                MenuEntry::new(t!("gallery.sample.item")).shortcut("⌘E").show(ui);
            };
            specimen(ui, 1, "MenuFace::Text", |ui| {
                bar_menu(ui, MenuFace::Text { label: t!("gallery.sample.menu"), padding: 8.0 }, BAR_BUTTON, items)
            });
            specimen(ui, 1, "MenuFace::Image", |ui| {
                let image = egui::Image::new(icons::REMOTE).fit_to_exact_size(egui::Vec2::splat(16.0)).tint(ui.visuals().text_color());
                bar_menu(ui, MenuFace::Image(image), BAR_BUTTON, items)
            });
        });
    });
    entry(ui, "option_button", |ui| {
        row(ui, |ui| {
            let (label, tip) = (t!("gallery.sample.label"), t!("gallery.sample.tip"));
            specimen(ui, 1, "icon: None", |ui| {
                if option_button(ui, "gallery:option", g.toggle, &label, &tip, None, None) {
                    g.toggle = !g.toggle;
                }
            });
            specimen(ui, 1, "icon: Some(\"mirror\")", |ui| {
                if option_button(ui, "gallery:option_icon", g.toggle, &label, &tip, Some("mirror"), None) {
                    g.toggle = !g.toggle;
                }
            });
            specimen(ui, 1, "selected: false", |ui| option_button(ui, "gallery:option_off", false, &label, &tip, Some("mirror"), None));
            specimen(ui, 1, "swatch: Some(rgb)", |ui| {
                option_button(ui, "gallery:option_swatch", true, &label, &tip, Some("pick_material"), Some(g.color))
            });
        });
    });
}

fn choices(ui: &mut egui::Ui, g: &mut KitGallery) {
    let values = [t!("gallery.sample.one"), t!("gallery.sample.two"), t!("gallery.sample.three")];
    let tip = t!("gallery.sample.tip");
    entry(ui, "segmented", |ui| {
        row(ui, |ui| {
            specimen(ui, 2, "Segment::new(label, …)", |ui| {
                let segments: Vec<Segment> = values.iter().enumerate().map(|(k, v)| Segment::new(v, &tip, None, k == g.segment)).collect();
                if let Some(k) = segmented(ui, window_id().with("text"), &segments, |k| format!("gallery:segment:{k}")) {
                    g.segment = k;
                }
            });
            specimen(ui, 2, "Segment::new(…, Some(icon), …)", |ui| {
                let names = ["copy_measure_outside", "copy_measure_centered", "copy_measure_inside"];
                let segments: Vec<Segment> =
                    names.iter().zip(&values).enumerate().map(|(k, (n, v))| Segment::new(v, &tip, Some(n), k == g.segment_icon)).collect();
                if let Some(k) = segmented(ui, window_id().with("icons"), &segments, |k| format!("gallery:segment_icon:{k}")) {
                    g.segment_icon = k;
                }
            });
            specimen(ui, 2, ".enabled(false)", |ui| {
                let segments: Vec<Segment> =
                    values.iter().enumerate().map(|(k, v)| Segment::new(v, &tip, None, k == 0).enabled(k != 2)).collect();
                segmented(ui, window_id().with("disabled"), &segments, |k| format!("gallery:segment_off:{k}"))
            });
            specimen(ui, 2, "segmented_sized(…, (22, 14))", |ui| {
                let segments = [Segment::image(&values[0], &tip, icons::GRID, true), Segment::image(&values[1], &tip, icons::VIEWS, false)];
                segmented_sized(ui, window_id().with("sized"), &segments, (22.0, 14.0), |k| format!("gallery:segment_sized:{k}"))
            });
        });
    });
    entry(ui, "ChoiceList", |ui| {
        row(ui, |ui| {
            specimen(ui, 2, "new(salt, shown)", |ui| {
                ChoiceList::new("gallery_choice", values[g.choice].clone()).width(width(2)).show(ui, |list| {
                    for (k, v) in values.iter().enumerate() {
                        list.value(&mut g.choice, k, v);
                    }
                })
            });
            specimen(ui, 2, "new(salt, \"\")", |ui| ChoiceList::new("gallery_choice_none", "").width(width(2)).show(ui, |_| {}));
            let color = g.color;
            specimen(ui, 2, ".lead(paint)", |ui| {
                ChoiceList::new("gallery_choice_lead", values[0].clone())
                    .width(width(2))
                    .lead(move |ui, r| paint_swatch(ui, r, Some(color), egui::Stroke::new(1.0, ui.visuals().text_color())))
                    .show(ui, |list| {
                        for (k, v) in values.iter().enumerate() {
                            let mark = style::menu_mark(move |ui, r, ink| paint_swatch(ui, r, Some(color), egui::Stroke::new(1.0, ink)));
                            list.item_lead(k == 0, mark, v.clone());
                        }
                        list.separator();
                        list.item(false, t!("gallery.sample.item"));
                    })
            });
            specimen(ui, 2, ".keep_open() + check", |ui| {
                ChoiceList::new("gallery_choice_checks", values[0].clone()).width(width(2)).keep_open().show(ui, |list| {
                    let [a, b] = &mut g.checks;
                    list.check(a, &values[0], None);
                    list.check(b, &values[1], Some(*a));
                })
            });
            specimen(ui, 2, "choice_field", |ui| {
                if let Some(p) = choice_field(ui, window_id().with("field"), g.picked.as_deref(), &values, width(2)) {
                    g.picked = Some(p);
                }
            });
        });
    });
    entry(ui, "choice_item", |ui| {
        row(ui, |ui| {
            specimen(ui, 2, "choice_item(ui, on, label)", |ui| {
                ui.vertical(|ui| {
                    for (k, v) in values.iter().enumerate() {
                        if choice_item(ui, k == g.choice, v).clicked() {
                            g.choice = k;
                        }
                    }
                })
            });
        });
    });
    entry(ui, "tick", |ui| {
        row(ui, |ui| {
            specimen(ui, 1, "Tick::No", |ui| tick(ui, Tick::No));
            specimen(ui, 1, "Tick::Yes", |ui| tick(ui, Tick::Yes));
            specimen(ui, 1, "Tick::Covered", |ui| tick(ui, Tick::Covered));
        });
    });
}

fn fields(ui: &mut egui::Ui, g: &mut KitGallery) {
    let field = |align, lit, addon| Field {
        hint: None,
        tip: None,
        width: width(2),
        align,
        lit,
        addon,
        blur_applies: true,
        empty_applies: false,
        handle: None,
    };
    entry(ui, "framed_field", |ui| {
        row(ui, |ui| {
            let right = egui::Align2::RIGHT_CENTER;
            let x = ("X", crate::workspaces::common::viewport::color(skraft_theme::AXIS_X));
            let value = g.framed.clone();
            let a = specimen(ui, 2, "align: RIGHT_CENTER", |ui| {
                framed_field(ui, window_id().with(1), &value, &value, field(right, false, None))
            });
            let b = specimen(ui, 2, "align: LEFT_CENTER", |ui| {
                let name = g.live.clone();
                framed_field(ui, window_id().with(2), &name, &name, field(egui::Align2::LEFT_CENTER, false, None))
            });
            let c = specimen(ui, 2, "lit: true", |ui| framed_field(ui, window_id().with(3), &value, &value, field(right, true, None)));
            let d = specimen(ui, 2, "addon: Some((\"X\", red))", |ui| {
                framed_field(ui, window_id().with(4), &value, &value, field(right, false, Some(x)))
            });
            specimen(ui, 2, "hint: Some(…)", |ui| {
                framed_field(ui, window_id().with(5), "", "", Field { hint: Some("18"), ..field(right, false, None) })
            });
            if let Some(v) = b {
                g.live = v;
            }
            if let Some(v) = a.or(c).or(d) {
                g.framed = v;
            }
        });
    });
    entry(ui, "live_field", |ui| {
        row(ui, |ui| {
            let look = |bad| FieldLook {
                hint: None,
                tip: None,
                width: width(2),
                align: egui::Align2::LEFT_CENTER,
                lit: false,
                bad,
                addon: None,
                handle: None,
            };
            specimen(ui, 2, "bad: false", |ui| live_field(ui, window_id().with("live"), &mut g.live, look(false)));
            let mut bad = "12,,5".to_owned();
            specimen(ui, 2, "bad: true (FieldLook::bad)", |ui| live_field(ui, window_id().with("bad"), &mut bad, look(true)));
        });
    });
    entry(ui, "cut_label", |ui| {
        row(ui, |ui| {
            specimen(ui, 2, "cut_label(ui, text, 140.0)", |ui| cut_label(ui, t!("gallery.sample.long"), width(2)));
        });
    });
    entry(ui, "form_choice", |ui| {
        block(ui, 4, |ui| {
            let labels = [t!("gallery.sample.one"), t!("gallery.sample.two"), t!("gallery.sample.three")];
            let segments: Vec<Segment> =
                labels.iter().enumerate().map(|(i, l)| Segment::new(l, l, None, i == g.form_choice).enabled(i != 2)).collect();
            if let Some(i) = form_choice(ui, window_id().with("form_choice"), &t!("gallery.sample.length"), &segments, |i| {
                format!("gallery:form_choice:{i}")
            }) {
                g.form_choice = i;
            }
            ui.label(egui::RichText::new("third segment .enabled(false)").monospace().small().weak());
        });
    });
    entry(ui, "form_row · form_message · warning_box", |ui| {
        block(ui, 4, |ui| {
            let [length, width] = &mut g.form;
            let unreadable = length.trim().parse::<f64>().is_err();
            let f = |label, lit, bad, handle| FormField { label, unit: "mm", lit, bad, handle };
            let (l, w) = (t!("gallery.sample.length"), t!("gallery.sample.width"));
            form_row(ui, window_id().with("length"), length, f(&l, false, unreadable, "gallery:form:length"));
            form_row(ui, window_id().with("width"), width, f(&w, true, false, "gallery:form:width"));
            form_message(ui, unreadable.then(|| t!("gallery.sample.unreadable")).as_deref());
            ui.label(egui::RichText::new("lit: true · bad: <length unreadable> · form_message(Some/None)").monospace().small().weak());
            ui.add_space(6.0);
            warning_box(ui, &t!("gallery.sample.warning"));
        });
    });
    entry(ui, "search::field", |ui| {
        block(ui, 4, |ui| {
            search::field(ui, window_id().with("search"), "gallery", &mut g.search, |ui, side| {
                IconButton::new(icons::SETTINGS, side, 14.0).show(ui);
            });
        });
    });
    entry(ui, "keypad", |ui| {
        row(ui, |ui| {
            specimen(ui, 3, "keypad(ui, &mut text)", |ui| {
                ui.label(if g.keypad.is_empty() { "—".to_owned() } else { g.keypad.clone() });
                if keypad(ui, &mut g.keypad).is_some_and(|a| a == crate::kit::widgets::KeypadAct::Enter) {
                    g.keypad.clear();
                }
            });
        });
    });
}

fn menus(ui: &mut egui::Ui, g: &mut KitGallery, views: &[CommandView]) {
    // A menu's frame and style, as the popups draw theirs: the rows below as in a menu.
    let menu_frame = |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut egui::Ui)| {
        ui.scope(|ui| {
            style::menu(ui.style_mut());
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| add(ui));
            });
        });
    };
    let item = t!("gallery.sample.item");
    entry(ui, "MenuEntry", |ui| {
        row(ui, |ui| {
            specimen(ui, 3, "new · .icon · .shortcut · .enabled(false) · .tip", |ui| {
                menu_frame(ui, &mut |ui| {
                    MenuEntry::new(&item).show(ui);
                    MenuEntry::new(&item).icon(icons::VIEW_TOP).show(ui);
                    MenuEntry::new(&item).shortcut("⌘E").show(ui);
                    MenuEntry::new(&item).enabled(false).show(ui);
                    MenuEntry::new(&item).tip(t!("gallery.sample.tip")).handle("gallery:menu_entry").show(ui);
                })
            });
            specimen(ui, 3, "style::menu_item(…, check, column) · menu_item_lead", |ui| {
                let color = g.color;
                menu_frame(ui, &mut |ui| {
                    ui.add(style::menu_item(&item, Some(true), true));
                    ui.add(style::menu_item(&item, Some(false), true));
                    ui.add(style::menu_item(&item, None, true));
                    let mark = style::menu_mark(move |ui, r, ink| paint_swatch(ui, r, Some(color), egui::Stroke::new(1.0, ink)));
                    ui.add(style::menu_item_lead(mark, &item));
                })
            });
        });
    });
    entry(ui, "command_item", |ui| {
        row(ui, |ui| {
            specimen(ui, 3, "command_item(ui, view, column: true)", |ui| {
                menu_frame(ui, &mut |ui| {
                    for id in ["view.grid", "view.axes", "edit.undo", "app.preferences"] {
                        if let Some(v) = views.iter().find(|v| v.id == id) {
                            command_item(ui, v, true);
                        }
                    }
                })
            });
        });
    });
    let texts =
        PinTexts { tip: |on| if on { t!("gallery.sample.unpin") } else { t!("gallery.sample.pin") }, note: || t!("gallery.sample.note") };
    let ids = ["view.grid", "view.axes", "view.guides"];
    let pin_items = |pinned: [bool; 3]| -> Vec<PinItem<'_>> {
        ids.iter().zip(pinned).filter_map(|(id, p)| views.iter().find(|v| v.id == *id).map(|v| PinItem::command(v, p))).collect()
    };
    entry(ui, "pin_list · pin_menu · unpin_menu", |ui| {
        row(ui, |ui| {
            for (checks, call) in [(false, "pin_list(…, checks: false)"), (true, "pin_list(…, checks: true)")] {
                specimen(ui, 4, call, |ui| {
                    menu_frame(ui, &mut |ui| {
                        if let Some(PinEvent::Pin(id)) = pin_list(ui, &pin_items(g.pinned), checks, &texts)
                            && let Some(k) = ids.iter().position(|i| *i == id)
                        {
                            g.pinned[k] = !g.pinned[k];
                        }
                    })
                });
            }
            specimen(ui, 2, "pin_menu (click) · unpin_menu (right click)", |ui| {
                let r = IconButton::new(icons::GRID, BAR_BUTTON, BAR_ICON).look(Look::Bar).show(ui);
                let menu = PinMenu { id: window_id().with("pin_menu"), align: None, checks: false, texts: PinTexts { ..texts } };
                if let Some(PinEvent::Pin(id)) = pin_menu(&r, &menu, || pin_items(g.pinned))
                    && let Some(k) = ids.iter().position(|i| *i == id)
                {
                    g.pinned[k] = !g.pinned[k];
                }
                unpin_menu(&r, &texts, None);
            });
        });
    });
    entry(ui, "style::caption", |ui| {
        ui.label(style::caption(t!("gallery.sample.note")));
    });
}

/// The fold of the tree's row `k`.
fn fold_key(k: usize) -> egui::Id {
    egui::Id::unique("kit_gallery_fold").with(k)
}

fn rows(ui: &mut egui::Ui, g: &mut KitGallery) {
    entry(ui, "tree_line · start_rename · rename_field · linked_mark", |ui| {
        block(ui, 4, |ui| {
            // An assembly, its part, the part's two bodies (the band), the second hidden.
            let lines = [(0, true, icons::GROUP), (1, true, icons::PART), (2, false, icons::SOLID), (2, false, icons::SOLID)];
            let salt = window_id().with("tree");
            let KitGallery { folds, selected, names, .. } = g;
            let mut shown = true;
            for (k, (depth, folds_open, icon)) in lines.into_iter().enumerate() {
                if !shown {
                    break;
                }
                let band = depth == 2;
                let hidden = k == 3;
                let is_selected = *selected == k;
                let v = ui.visuals();
                let ink = if is_selected {
                    v.selection.stroke.color
                } else if hidden {
                    crate::kit::widgets::hidden_color(v, band)
                } else {
                    v.text_color()
                };
                let fold = folds_open.then(|| Fold { key: fold_key(k), open: folds.contains(&fold_key(k)) });
                let image = egui::Image::new(icon).fit_to_exact_size(egui::Vec2::splat(14.0)).tint(ink);
                let line = TreeLine { height: 22.0, selected: is_selected, lit: false, band, depth, fold, icon: image };
                let rename = salt.with(("rename", k));
                let renaming = ui.data(|d| d.get_temp::<egui::Id>(egui::Id::unique(RENAMING))) == Some(rename);
                let name = names[k].clone();
                let (r, (_, renamed)) = tree_line(
                    ui,
                    line,
                    (salt, folds),
                    |ui, over| {
                        if over || hidden {
                            IconButton::new(if hidden { icons::EYE_OFF } else { icons::EYE }, 18.0, 14.0).ink(ink).show(ui);
                        }
                        if k == 1 {
                            ui.add(linked_mark(ui, ink));
                        }
                    },
                    |ui| {
                        if renaming {
                            rename_field(ui, rename, &name)
                        } else {
                            ui.add(egui::Label::new(egui::RichText::new(&name).color(ink)).selectable(false).truncate());
                            None
                        }
                    },
                );
                if r.clicked() {
                    *selected = k;
                }
                if let Some(Some(new)) = start_rename(ui, &r, renaming, is_selected, rename, renamed) {
                    names[k] = new;
                }
                if let Some(f) = fold {
                    shown = f.open;
                }
            }
            ui.label(
                egui::RichText::new("band: true (bodies) · selected · hidden_color · double-click / F2: rename").monospace().small().weak(),
            );
        });
    });
    entry(ui, "tree_row", |ui| {
        block(ui, 4, |ui| {
            for (selected, lit, band, call) in [
                (false, false, false, "tree_row(…)"),
                (true, false, false, "selected: true"),
                (false, true, false, "lit: true"),
                (false, false, true, "band: true"),
            ] {
                tree_row(ui, 22.0, selected, lit, band, |ui| {
                    ui.label(egui::RichText::new(call).monospace());
                });
            }
        });
    });
    entry(ui, "ribbon::tab_strip", |ui| {
        block(ui, 4, |ui| {
            let labels = [t!("gallery.sample.one"), t!("gallery.sample.two"), t!("gallery.sample.three")];
            let ids: Vec<egui::Id> = (0..3).map(|k| window_id().with(("strip", k))).collect();
            if let Some(k) = tab_strip(ui, &labels, &ids, Some(g.strip), width(4), egui::Margin::ZERO, Corners::Round, &[]) {
                g.strip = k;
            }
            ui.label(egui::RichText::new("Corners::Round").monospace().small().weak());
        });
    });
    entry(ui, "keycaps", |ui| {
        row(ui, |ui| {
            let keys = ["⇧".to_owned(), "⌘".to_owned(), "S".to_owned()];
            for (look, call) in [(Keycaps::Plain, "Keycaps::Plain"), (Keycaps::Lit, "Keycaps::Lit"), (Keycaps::Fixed, "Keycaps::Fixed")] {
                specimen(ui, 2, call, |ui| {
                    let rect = space(ui, egui::vec2(width(2), 24.0));
                    keycaps(ui, &keys, rect.right_center(), look);
                });
            }
        });
    });
    entry(ui, "key_toggle", |ui| {
        row(ui, |ui| {
            for (k, key) in ["⇧", "⌃", "⌥", "⌘"].iter().enumerate() {
                specimen(ui, 1, if g.keys[k] { "on" } else { "off" }, |ui| {
                    key_toggle(ui, key, &mut g.keys[k], 24.0);
                });
            }
        });
    });
}

fn marks(ui: &mut egui::Ui, doc: Option<&Document>) {
    entry(ui, "warning · linked_mark · linked_badge · badge", |ui| {
        row(ui, |ui| {
            specimen(ui, 1, "warning", |ui| ui.add(warning(ui)));
            specimen(ui, 1, "linked_mark", |ui| ui.add(linked_mark(ui, ui.visuals().text_color())));
            specimen(ui, 1, "linked_badge", |ui| {
                let rect = space(ui, egui::Vec2::splat(18.0));
                let ink = ui.visuals().text_color();
                egui::Image::new(icons::SOLID).tint(ink).paint_at(ui, rect);
                linked_badge(ui, rect, ink, ui.visuals().window_fill);
            });
            specimen(ui, 1, "badge", |ui| {
                let rect = space(ui, egui::Vec2::splat(22.0));
                let v = ui.visuals();
                badge(ui, rect, icons::LOCK, 12.0, v.text_color(), v.widgets.inactive.weak_bg_fill);
            });
        });
    });
    entry(ui, "picture_badge · badge_button", |ui| {
        row(ui, |ui| {
            let picture = |ui: &mut egui::Ui| {
                let rect = space(ui, egui::vec2(72.0, 48.0));
                paint_swatch(ui, rect, Some([92, 128, 160]), egui::Stroke::new(1.0, ui.visuals().text_color()));
                rect
            };
            specimen(ui, 1, "picture_badge", |ui| {
                let rect = picture(ui);
                picture_badge(ui, rect, egui::Align2::RIGHT_BOTTOM, icons::LOCK, 10.0);
            });
            specimen(ui, 1, "badge_button", |ui| {
                let rect = picture(ui);
                badge_button(ui, rect, egui::Align2::RIGHT_TOP, icons::CLOSE, 8.0, window_id().with("close"));
                badge_button(ui, rect, egui::Align2::RIGHT_BOTTOM, icons::ROTATE, 10.0, window_id().with("turn"));
            });
        });
    });
    entry(ui, "materials::paint_material · paint_none", |ui| {
        row(ui, |ui| {
            use crate::workspaces::common::cards::materials::{paint_material, paint_none};
            specimen(ui, 1, "paint_none", |ui| {
                let rect = space(ui, egui::Vec2::splat(32.0));
                paint_none(ui, rect, ui.visuals().weak_text_color(), 3.0);
            });
            match doc.map(|d| (d, d.materials().take(12).collect::<Vec<_>>())) {
                Some((d, materials)) if !materials.is_empty() => {
                    for (id, m) in materials {
                        specimen(ui, 1, &m.name, |ui| {
                            let rect = space(ui, egui::Vec2::splat(32.0));
                            paint_material(ui, d, rect, id, 3.0);
                        });
                    }
                }
                _ => drop(ui.label(style::caption(t!("gallery.sample.no_materials")))),
            }
        });
    });
    entry(ui, "icons::chevron", |ui| {
        row(ui, |ui| {
            for (angle, call) in [(0.0, "0"), (-std::f32::consts::FRAC_PI_2, "-π/2"), (std::f32::consts::PI, "π")] {
                specimen(ui, 1, call, |ui| {
                    let rect = space(ui, egui::Vec2::splat(18.0));
                    icons::chevron(ui, rect.center(), 14.0, ui.visuals().text_color(), angle);
                });
            }
        });
    });
}

fn colors(ui: &mut egui::Ui, g: &mut KitGallery) {
    entry(ui, "color::color_box", |ui| {
        row(ui, |ui| {
            specimen(ui, 1, "enabled: true", |ui| {
                if let (_, Some(c)) = color_box(ui, "gallery_color", g.color, egui::vec2(48.0, 22.0), true) {
                    g.color = c;
                }
            });
            specimen(ui, 1, "enabled: false", |ui| color_box(ui, "gallery_color_off", g.color, egui::vec2(48.0, 22.0), false));
        });
    });
    entry(ui, "color::paint_swatch", |ui| {
        row(ui, |ui| {
            let edge = egui::Stroke::new(1.0, ui.visuals().text_color());
            specimen(ui, 1, "Some(rgb)", |ui| {
                let rect = space(ui, egui::Vec2::splat(20.0));
                paint_swatch(ui, rect, Some(g.color), edge);
            });
            specimen(ui, 1, "None", |ui| {
                let rect = space(ui, egui::Vec2::splat(20.0));
                paint_swatch(ui, rect, None, edge);
            });
        });
    });
}

fn frames(ui: &mut egui::Ui) {
    let card = t!("gallery.sample.card");
    entry(ui, "style::card_frame · option_card · hover_card", |ui| {
        row(ui, |ui| {
            specimen(ui, 1, "card_frame", |ui| style::card_frame(ui.style()).show(ui, |ui| ui.label(&card)));
            specimen(ui, 1, "option_card", |ui| style::option_card(ui.style()).show(ui, |ui| ui.label(&card)));
            specimen(ui, 1, "hover_card", |ui| {
                let frame = style::option_card(ui.style());
                style::hover_card(ui, frame, window_id().with("hover_card"), |ui| ui.label(&card))
            });
        });
    });
    entry(ui, "style::settings_section · settings_grid", |ui| {
        style::settings_section(ui, &t!("gallery.sample.section"));
        style::settings_grid(ui, "gallery_grid", |ui| {
            ui.label(t!("gallery.sample.length"));
            ui.label("600 mm");
            ui.end_row();
            ui.label(t!("gallery.sample.width"));
            ui.label("18 mm");
            ui.end_row();
        });
    });
    entry(ui, "style::section_title · semibold · caption", |ui| {
        row(ui, |ui| {
            specimen(ui, 1, "section_title", |ui| style::section_title(ui, t!("gallery.sample.section")));
            specimen(ui, 1, "semibold(16.0)", |ui| ui.label(egui::RichText::new(t!("gallery.sample.label")).font(style::semibold(16.0))));
            specimen(ui, 1, "caption", |ui| ui.label(style::caption(t!("gallery.sample.label"))));
        });
    });
    entry(ui, "Tip", |ui| {
        row(ui, |ui| {
            let label = t!("gallery.sample.label");
            specimen(ui, 1, ".tip(text)", |ui| TextButton::new(&label).show(ui).tip(t!("gallery.sample.tip")));
            specimen(ui, 1, ".tip_more(text, more)", |ui| {
                TextButton::new(&label).show(ui).tip_more(&t!("gallery.sample.tip"), &t!("gallery.sample.tip_more"))
            });
        });
    });
}

fn overlays(ui: &mut egui::Ui, theme: &Theme) {
    let ink = ui.visuals().text_color();
    let stroke = egui::Stroke::new(1.0, ink);
    let caption = |ui: &egui::Ui, at: egui::Pos2, text: &str| {
        let font = egui::TextStyle::Small.resolve(ui.style());
        ui.painter().text(at, egui::Align2::LEFT_CENTER, text, egui::FontId::monospace(font.size), ui.visuals().weak_text_color());
    };
    entry(ui, "overlay::Dash", |ui| {
        let dashes = [
            (overlay::SELECTION, "SELECTION"),
            (overlay::FRAME, "FRAME"),
            (overlay::GHOST, "GHOST"),
            (overlay::ALIGNMENT, "ALIGNMENT"),
            (overlay::TRACKED, "TRACKED"),
            (overlay::LEVEL, "LEVEL"),
        ];
        let rect = space(ui, egui::vec2(320.0, 18.0 * dashes.len() as f32));
        for (k, (dash, name)) in dashes.iter().enumerate() {
            let y = rect.top() + 9.0 + 18.0 * k as f32;
            overlay::dashed(ui.painter(), &[egui::pos2(rect.left(), y), egui::pos2(rect.left() + 180.0, y)], stroke, *dash);
            caption(ui, egui::pos2(rect.left() + 196.0, y), name);
        }
    });
    entry(ui, "overlay::dashed_rect · dashed_quad · marquee", |ui| {
        let rect = space(ui, egui::vec2(480.0, 90.0));
        let cell = |k: f32| egui::Rect::from_min_size(rect.min + egui::vec2(k * 120.0, 0.0), egui::vec2(100.0, 70.0));
        overlay::dashed_rect(ui.painter(), cell(0.0), stroke, overlay::FRAME);
        let c = cell(1.0);
        overlay::dashed_quad(
            ui.painter(),
            [c.left_bottom(), c.center_bottom() + egui::vec2(30.0, 0.0), c.right_top(), c.center_top() - egui::vec2(30.0, 0.0)],
            stroke,
            overlay::LEVEL,
        );
        overlay::marquee(ui.painter(), cell(2.0), false, theme);
        overlay::marquee(ui.painter(), cell(3.0), true, theme);
        for (k, name) in ["dashed_rect", "dashed_quad", "marquee", "marquee crossing"].iter().enumerate() {
            caption(ui, egui::pos2(cell(k as f32).left(), rect.bottom() - 8.0), name);
        }
    });
    entry(ui, "overlay::leader · square_grip · cross · turned_text", |ui| {
        let rect = space(ui, egui::vec2(480.0, 90.0));
        let accent = egui::Stroke::new(1.0, ui.visuals().selection.stroke.color);
        let anchor = rect.min + egui::vec2(10.0, 50.0);
        let end = rect.min + egui::vec2(90.0, 20.0);
        overlay::leader(ui.painter(), anchor, end, 16.0, stroke);
        overlay::leader(ui.painter(), anchor + egui::vec2(0.0, 20.0), end + egui::vec2(0.0, 50.0), 16.0, accent);
        overlay::square_grip(ui.painter(), rect.min + egui::vec2(170.0, 40.0), 8.0, ui.visuals().window_fill, accent);
        overlay::cross(ui.painter(), rect.min + egui::vec2(250.0, 40.0), 6.0, stroke);
        let font = egui::TextStyle::Body.resolve(ui.style());
        overlay::turned_text(ui.painter(), rect.min + egui::vec2(310.0, 70.0), -0.5, t!("gallery.sample.turned"), font, ink);
        for (x, name) in [(10.0, "leader"), (150.0, "square_grip"), (235.0, "cross"), (330.0, "turned_text")] {
            caption(ui, egui::pos2(rect.left() + x, rect.bottom() - 6.0), name);
        }
    });
}
