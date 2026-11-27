//! The navigator's tree of the model (docs/ui/interface.md#navigator).

use super::*;

fn occurrence_icon(doc: &Document, o: &Occurrence) -> egui::ImageSource<'static> {
    if doc.part(o.component).is_ok() { icons::BOX } else { icons::COMPONENT }
}

/// The name of an occurrence — of a component, of a body — in its navigator row, its colour `color`,
/// on `back`, as [`crate::workspaces::common::occurrence_name`] writes it (own name, then the definition's — `def`:
/// its component's, its body definition's — between brackets; automatic, `auto`, attenuated
/// unless the row is selected). Its response, and where its own name ends and the
/// definition's starts (halfway across the space between them) where it has one.
fn occurrence_label(
    ui: &mut egui::Ui,
    own: &str,
    def: &str,
    auto: bool,
    color: egui::Color32,
    selected: bool,
    back: egui::Color32,
) -> (egui::Response, Option<f32>) {
    let weak = if selected { color } else { color.lerp_to_gamma(back, weak_share(ui.visuals())) };
    let size = egui::TextStyle::Body.resolve(ui.style()).size;
    let job = crate::workspaces::common::occurrence_name::job(
        own,
        def,
        auto,
        crate::workspaces::common::occurrence_name::NameInk { size, ink: color, weak },
    );
    let r = ui.add(egui::Label::new(job).selectable(false).truncate());
    let split = (!own.is_empty()).then(|| r.rect.min.x + crate::workspaces::common::occurrence_name::split(ui, own, size));
    (r, split)
}

/// What the navigator keeps of a document between frames, in the document itself (closed with
/// it, never written to disk): the rows unfolded, the ids reborn, the active path last seen.
#[derive(Default)]
pub struct NavigatorState {
    /// The keys of the rows unfolded (a row missing: folded).
    pub(super) open: FastSet<egui::Id>,
    births: Births,
    pub(super) active: Option<ActivePath>,
    /// Its search (docs/ui/components-library.md#search), and whether a definition holds a
    /// row it lets through, by definition — kept for the document's revision and the search
    /// it was found for.
    pub search: String,
    pub(super) holds: Option<(u64, String, Holds)>,
}

/// Whether a definition holds a row a search lets through, by definition.
pub(super) type Holds = FastMap<Id, bool>;

/// One line of the navigator: an occurrence at some depth of the tree, or a body of the part it
/// places.
pub(super) struct NavigatorRow<'d> {
    /// The occurrences above it.
    pub(super) path: Vec<Id>,
    pub(super) occurrence: &'d Occurrence,
    /// Its folding state, for an assembly, a part, a group.
    pub(super) fold: Option<Fold>,
    /// An assembly above it is hidden: it is not drawn either.
    hidden_above: bool,
    /// The body this line shows, of the part `occurrence` places: none for the occurrence's own.
    pub(super) body: Option<BodyRow<'d>>,
    /// Shown under a search only for what it holds (it does not match): attenuated.
    pub(super) dim: bool,
}

/// A body in the navigator, under its part (docs/model/bodies.md#body-groups).
pub(super) struct BodyRow<'d> {
    pub(super) part: &'d skraft_model::Part,
    pub(super) body: &'d skraft_model::Body,
    /// The groups it lies in, the part's own body first: a click opens them.
    pub(super) group: Vec<Id>,
    /// Its siblings, itself included: the bodies of the part or group it lies in.
    siblings: &'d [skraft_model::Body],
    /// The line is the body's profile's, under it, not the body's own.
    pub(super) profile: bool,
}

/// The icon of a body's nature (docs/model/bodies.md#body-names).
fn nature_icon(nature: Nature) -> egui::ImageSource<'static> {
    match nature {
        Nature::Extrusion => icons::EXTRUSION,
        Nature::Revolution => icons::REVOLUTION,
        Nature::Solid => icons::SOLID,
        Nature::Group => icons::GROUP,
        Nature::Round => icons::ROUND,
        Nature::Chamfer => icons::CHAMFER,
    }
}

/// How far the weak text lies from the text towards the background, 0 to 1.
pub(crate) fn weak_share(v: &egui::Visuals) -> f32 {
    let (text, weak, back) = (v.text_color(), v.weak_text_color(), v.window_fill);
    let d = |a: egui::Color32, b: egui::Color32| {
        (a.r() as f32 - b.r() as f32).abs() + (a.g() as f32 - b.g() as f32).abs() + (a.b() as f32 - b.b() as f32).abs()
    };
    let span = d(text, back);
    if span > 0.0 { (d(text, weak) / span).clamp(0.0, 1.0) } else { 0.0 }
}

/// What the interface calls the body `b` of `p`: its definition's name (docs/model/bodies.md#body-names),
/// automatic or not.
pub(super) fn body_label(p: &skraft_model::Part, b: &skraft_model::Body) -> String {
    p.def_of(b).name.clone()
}

impl BodyRow<'_> {
    pub(super) fn label(&self) -> String {
        body_label(self.part, self.body)
    }

    /// Its nature's; a linked body's says where it comes from: « Empreindre ».
    pub(super) fn icon(&self) -> egui::ImageSource<'static> {
        let def = self.part.def_of(self.body);
        if def.source == skraft_model::BodySource::Linked { icons::IMPRINT } else { nature_icon(def.nature()) }
    }
}

/// The navigator, in `area`: the search field `search::GAP` under `line` (the bottom of the
/// card's title bar line), the model (named `name`, its file's) on the first row, then the
/// tree; only the rows in view are laid out (a model can hold tens of thousands of
/// occurrences).
pub fn navigator(
    ui: &mut egui::Ui,
    editor: &Editor,
    state: &mut NavigatorState,
    name: &str,
    actions: &mut Vec<Action>,
    area: egui::ScrollArea,
    (body, line): (f32, f32),
) {
    let doc = editor.document();
    // The search field over the whole width, by name only (docs/ui/components-library.md#search),
    // as far from the title bar as the library's from its tabs; the tree in the
    // height left under it, its first row as far again (the item spacing and the scroll
    // area's top margin below).
    let top = ui.cursor().top();
    ui.add_space((line + crate::kit::search::GAP - ui.cursor().top()).max(0.0));
    let salt = ui.make_persistent_id("navigator_search");
    crate::kit::search::field(ui, salt, "navigator", &mut state.search, |_, _| {});
    let left = (body - (ui.cursor().top() - top)).max(0.0);
    let area = area.max_height(left).min_scrolled_height(left);
    let height = ui.spacing().interact_size.y;
    // Room above the first row and below the last for their backgrounds, which spill half the
    // rows' spacing beyond them (`tree_row`): the scroll area clips to its content and margin,
    // so the first row selected was shorter than the others.
    let mut margin = ui.spacing().scroll.content_margin;
    let gap = (ui.spacing().item_spacing.y / 2.0).ceil() as i8;
    margin.top = margin.top.max(gap);
    margin.bottom = margin.bottom.max(gap);
    let area = area.content_margin(margin);
    // The ids handed out again since the last frame reborn (an empty model too: undone
    // to it, then made again).
    if state.births.next_id != doc.next_id() {
        state.births.update(doc.next_id());
    }
    if doc.root_assembly().occurrences().is_empty() {
        area.show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            model_row(ui, editor, name, height, actions);
        });
        actions.push(Action::HoverMachining(None));
        return;
    }
    // The active path gone up (or elsewhere): what was open below folded.
    let active = ActivePath::of(editor);
    if state.active.as_ref() != Some(&active)
        && let Some(before) = state.active.replace(active.clone())
    {
        before.fold_left(&state.births, &mut state.open, &active);
    }
    let mut rows = Vec::new();
    let matcher = skraft_tools::search::Matcher::new(&state.search);
    if matcher.is_active() {
        // Under a search: every row it lets through and the rows above them, whatever is
        // folded (the folds left as they are).
        let revision = editor.history().revision();
        if state.holds.as_ref().is_none_or(|(r, f, _)| *r != revision || *f != state.search) {
            state.holds = Some((revision, state.search.clone(), FastMap::default()));
        }
        let Some((_, _, holds)) = state.holds.as_mut() else { return };
        let mut f = Filtered { matcher: &matcher, holds, doc };
        f.rows(doc.root(), &mut Vec::new(), &mut rows, false, 0);
    } else {
        flatten(&state.births, &mut state.open, editor, doc, doc.root(), &active.occurrences, &mut Vec::new(), &mut rows, false, 0);
    }
    if rows.is_empty() && matcher.is_active() {
        area.show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.weak(t!("search.no_match"));
        });
        actions.push(Action::HoverMachining(None));
        return;
    }
    let folds = &mut state.open;
    let salt = ui.make_persistent_id("navigator_folds");
    let selection: std::collections::HashSet<Id> = editor.selection().iter().copied().collect();
    let text = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();
    let on_selection = ui.visuals().selection.stroke.color;
    let mut over = None;
    let mut over_body = None;
    // The body whose profile is edited: its « Profil » row the one selected.
    let editing = editor.profile_body();
    // The components in error at any depth, each found once (definitions are shared).
    let mut errors = FastMap::default();
    let cx = TreeCx { editor, doc, selection: &selection, height, salt, text, weak, on_selection, editing };
    // Row 0 the model's, the tree's after it.
    area.show_rows(ui, height, rows.len() + 1, |ui, range| {
        ui.set_min_width(ui.available_width());
        if range.start == 0 {
            model_row(ui, editor, name, height, actions);
        }
        for row in rows.drain(range.start.saturating_sub(1)..range.end.saturating_sub(1)) {
            match &row.body {
                Some(b) if b.profile => {
                    profile_row(ui, editor, &row, height, editing, actions);
                    continue;
                }
                Some(_) => {
                    body_row(ui, editor, row, height, (salt, folds), actions, &mut over_body);
                    continue;
                }
                None => {}
            }
            occurrence_row(ui, cx, row, folds, &mut errors, &mut over, actions);
        }
    });
    paint_drag_ghost(ui.ctx());
    actions.push(Action::HoverNavigator(over));
    actions.push(Action::HoverMachining(over_body));
}

/// What the rows of the navigator's tree read.
#[derive(Clone, Copy)]
struct TreeCx<'a> {
    editor: &'a Editor,
    doc: &'a Document,
    selection: &'a std::collections::HashSet<Id>,
    height: f32,
    /// Keeps a row's fold apart from another document's.
    salt: egui::Id,
    text: egui::Color32,
    weak: egui::Color32,
    on_selection: egui::Color32,
    /// The body whose profile is edited.
    editing: Option<Id>,
}

/// What a row's line gives back: its toggle clicked (an occurrence's eye, a body's sign), the
/// name typed, where its own name ends.
type LineOutcome = (bool, Option<Option<String>>, Option<f32>);

/// How an occurrence's row shows, and its name's edit.
#[derive(Clone, Copy)]
struct OccurrenceLook {
    selected: bool,
    tint: egui::Color32,
    greyed: bool,
    pieces: usize,
    broken: bool,
    inner: bool,
    /// A link to look at (docs/model/bodies.md#links): this source off its seat (`here`, the
    /// assembly it lies in), or a linked body broken or a source off or overlapping below it.
    here: Id,
    link_off: bool,
    link_below: bool,
    rename: egui::Id,
    rename_occurrence: egui::Id,
    /// Its name edited: the occurrence's own (true) or its component's.
    renaming: Option<bool>,
}

/// What the links tell on an occurrence's row: what it overlaps inside a part they both enter,
/// and the parts it shapes.
struct OccurrenceLinks {
    overlap_tip: Option<String>,
    shapes: Option<String>,
}

fn occurrence_links(editor: &Editor, doc: &Document, here: Id, o: &Occurrence) -> OccurrenceLinks {
    let source = skraft_model::LinkSource { assembly: here, occurrence: o.id };
    let list: Vec<_> = editor.link_overlaps().of(source).collect();
    let overlap_tip = (!list.is_empty()).then(|| {
        let mut parts: Vec<String> = list.iter().map(|(s, _)| doc.link_source_name(*s)).collect();
        let mut receivers: Vec<String> = list.iter().filter_map(|(_, r)| doc.component(*r).ok().map(|c| c.name.clone())).collect();
        parts.sort();
        parts.dedup();
        receivers.sort();
        receivers.dedup();
        t!("panel.navigator.link_overlap", parts = parts.join(", "), receivers = receivers.join(", "))
    });
    let shapes = editor
        .link_map()
        .receivers_of(source)
        .map(|parts| parts.iter().filter_map(|p| doc.component(*p).ok().map(|c| c.name.as_str())).collect::<Vec<_>>().join(", "));
    OccurrenceLinks { overlap_tip, shapes }
}

fn occurrence_look(ui: &egui::Ui, cx: TreeCx<'_>, row: &NavigatorRow<'_>, errors: &mut FastMap<Id, bool>) -> OccurrenceLook {
    let TreeCx { editor, doc, selection, text, weak, on_selection, editing, .. } = cx;
    let o = row.occurrence;
    // The part open: selected while none of its bodies is (a body selected is marked on
    // its own row, not its part's).
    // The part open: marked open when it is the level open, else (a group open in it) only
    // as on the way to it, its name in the accent's colour (docs/model/opening-a-part.md).
    let open_here = editor.open_part() == Some(o.id) && editor.context() == row.path.as_slice();
    let open = open_here && editor.open_group().is_empty() && selection.is_empty() && editing.is_none();
    let on_path = open_here && !editor.open_group().is_empty();
    let selected = (editor.context() == row.path.as_slice() && (selection.contains(&o.id) || open))
        || editor.deep_selection().iter().any(|p| p.occurrences == row.path && p.entity == o.id);
    // Hidden, itself or by an assembly above: greyed.
    let greyed = o.hidden || row.hidden_above;
    let tint = if selected || on_path {
        on_selection
    } else if greyed {
        hidden_color(ui.visuals(), false)
    } else if row.dim {
        weak
    } else {
        text
    };
    // A part in several pieces, or empty: an error to correct (docs/model/bodies.md). A body failed
    // in it, at any depth, or a part in error in an assembly: ⚠ too, its name as it is
    // (the error is below, seen once unfolded).
    let part = doc.part(o.component).ok();
    let pieces = part.map_or(1, |p| p.pieces());
    let broken = pieces != 1;
    let inner = !broken && in_error(doc, o.component, errors, 0);
    let here = doc.context_of(&row.path).unwrap_or(doc.root());
    let link_off = editor.link_status().is_off(here, o.id);
    let link_below = editor.link_status().holds(o.component) || editor.link_overlaps().holds(o.component);
    // A name edited in place, from a double-click, or F2 while it is the one occurrence
    // selected: the occurrence's own, shown first, where it has one and the double-click
    // is on it (F2: where it has one), else its component's (« Occurrence <Composant> »).
    let rename = ui.make_persistent_id(("navigator_rename", &row.path, o.id));
    let rename_occurrence = rename.with("occurrence");
    let renaming = match ui.data(|d| d.get_temp::<egui::Id>(egui::Id::unique(RENAMING))) {
        Some(id) if id == rename => Some(false),
        Some(id) if id == rename_occurrence => Some(true),
        _ => None,
    };
    OccurrenceLook { selected, tint, greyed, pieces, broken, inner, here, link_off, link_below, rename, rename_occurrence, renaming }
}

/// The line of an occurrence: its fold, icon, material, name (or the field editing it) and eye.
/// Its response; the eye clicked, the name typed, where its own name ends.
fn occurrence_line(
    ui: &mut egui::Ui,
    cx: TreeCx<'_>,
    row: &NavigatorRow<'_>,
    look: &OccurrenceLook,
    name: &str,
    links: &OccurrenceLinks,
    folds: &mut FastSet<egui::Id>,
) -> (egui::Response, LineOutcome) {
    let TreeCx { doc, height, salt, weak, .. } = cx;
    let OccurrenceLook { selected, tint, greyed, broken, inner, link_off, link_below, rename, rename_occurrence, renaming, .. } = *look;
    let o = row.occurrence;
    let icon = egui::Image::new(occurrence_icon(doc, o)).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint);
    let line = TreeLine { height, selected, lit: false, band: false, depth: row.path.len(), fold: row.fold, icon };
    // The eye first, at the right end (open while shown, struck through once hidden), then the
    // name in the room left, truncated with "…" so a long name never pushes the eye out.
    let mark = if selected { tint } else { weak };
    let eye = |ui: &mut egui::Ui, _| {
        let (icon, tip) = if o.hidden { (icons::EYE_OFF, t!("panel.navigator.show")) } else { (icons::EYE, t!("panel.navigator.hide")) };
        let img = egui::Image::new(icon).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(mark);
        let eye = ui.add(egui::Button::image(img).frame(false));
        crate::kit::handles::publish(|| format!("navigator:eye:{}", o.id.0), eye.rect);
        let eye = eye.tip(tip).clicked();
        // A part shaping others: the chain left of the eye, in the column of the bodies' marks,
        // its tip naming them.
        if let Some(parts) = &links.shapes {
            ui.add(linked_mark(ui, mark)).tip(t!("panel.navigator.imprints_in", parts = parts));
        }
        // A component bought or existing: its mark, left of the rest (docs/ui/parts-list.md#rows).
        let source = doc.component(o.component).map_or(Source::Made, |c| c.source);
        let source_mark = match source {
            Source::Made => None,
            Source::Bought => Some((icons::BOUGHT, t!("panel.entity_info.source_bought"))),
            Source::Existing => Some((icons::EXISTING, t!("panel.entity_info.source_existing"))),
        };
        if let Some((icon, tip)) = source_mark {
            let img = egui::Image::new(icon).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(mark);
            let r = ui.add(img.sense(egui::Sense::hover()));
            crate::kit::handles::publish(|| format!("navigator:source:{}", o.id.0), r.rect);
            r.tip(tip);
        }
        eye
    };
    let label = |ui: &mut egui::Ui| {
        if broken || inner || link_off || link_below || links.overlap_tip.is_some() {
            ui.add(warning(ui));
        }
        let color = if broken { ui.visuals().error_fg_color } else { tint };
        // The name edited alone in its field, the whole row's width.
        match renaming {
            Some(true) => return (rename_field(ui, rename_occurrence, &o.name), None),
            Some(false) => return (rename_field(ui, rename, name), None),
            None => {}
        }
        // What a part shows, if anything, before its name: its material, its coloration, and a
        // mark when faces carry their own.
        if let Ok(p) = doc.part(o.component) {
            material_marks(ui, doc, p, greyed);
        }
        let auto = doc.component(o.component).is_ok_and(|c| !c.named);
        let back = ui.visuals().window_fill;
        let (_, split) = occurrence_label(ui, &o.name, name, auto, color, selected, back);
        (None, split)
    };
    let (response, (eye, (renamed, split))) = tree_line(ui, line, (salt, folds), eye, label);
    (response, (eye, renamed, split))
}

/// What is dragged over an occurrence's row (decision 0044): a part takes bodies or parts in, at
/// its root; an assembly makes a part of bodies.
fn drop_on_occurrence(
    ui: &egui::Ui,
    editor: &Editor,
    o: &Occurrence,
    path: &EntityPath,
    response: &egui::Response,
    actions: &mut Vec<Action>,
) {
    let doc = editor.document();
    let (bodies_over, parts_over) = dragged_over(response);
    if bodies_over.is_some() || parts_over.is_some() {
        let is_part = doc.part(o.component).is_ok();
        let target: DropTarget<'_> = match (bodies_over, parts_over) {
            (Some(drag), _) if is_part => {
                let same = same_part(doc, &drag.from.part_path(), path);
                drag.to(same, path, Vec::new()).map(|to| (Mark::Into, Box::new(move |copy| drag.op(editor, to, copy)) as Box<_>))
            }
            (Some(drag), _) => {
                let inside: Vec<Id> = path.occurrences.iter().copied().chain([o.id]).collect();
                Some((Mark::Into, Box::new(move |copy| drag.op(editor, BodiesTo::NewPart(inside), copy)) as Box<_>))
            }
            (None, Some(drag)) if is_part => {
                let at = path.clone();
                drag.onto_part(doc, &at, Vec::new(), false)
                    .map(|_| (Mark::Into, Box::new(move |copy| drag.onto_part(doc, &at, Vec::new(), copy).expect("checked")) as Box<_>))
            }
            (None, Some(drag)) => {
                let inside: Vec<Id> = path.occurrences.iter().copied().chain([o.id]).collect();
                drag.onto_assembly(doc, inside.clone(), false)
                    .map(|_| (Mark::Into, Box::new(move |copy| drag.onto_assembly(doc, inside, copy).expect("checked")) as Box<_>))
            }
            _ => None,
        };
        drop_on_row(ui, response, 0.0, target, actions);
    }
}

/// An occurrence's right-click menu, « Aplatir » on a part with a tree.
fn occurrence_menu(
    editor: &Editor,
    selection: &std::collections::HashSet<Id>,
    o: &Occurrence,
    path: &EntityPath,
    response: &egui::Response,
    (rename, rename_occurrence): (egui::Id, egui::Id),
    actions: &mut Vec<Action>,
) {
    let doc = editor.document();
    // A source off its seat (docs/model/bodies.md#links): put back from here.
    let here = doc.context_of(&path.occurrences).unwrap_or(doc.root());
    let off = editor.link_status().is_off(here, o.id).then_some(skraft_model::LinkSource { assembly: here, occurrence: o.id });
    let part = doc.part(o.component).ok();
    let in_selection = (editor.open_part().is_none() && editor.context() == path.occurrences.as_slice() && selection.contains(&o.id))
        || editor.deep_selection().iter().any(|p| p.occurrences == path.occurrences && p.entity == o.id);
    let select = (!in_selection).then(|| Action::SelectPath(path.clone(), false));
    let own = (!o.name.is_empty()).then_some(rename_occurrence);
    // « Aplatir » on a part with a tree: the parts selected with it, when it is.
    let composed = part.is_some_and(|p| p.has_tree());
    row_menu(
        response,
        doc.assembly(o.component).is_ok(),
        (rename, own),
        select,
        actions,
        |ui, actions| {
            if let Some(source) = off {
                ui.separator();
                menu_op(ui, "reseat", t!("operation.reseat_sources"), Operation::ReseatSources { sources: vec![source] }, actions);
                menu_op(ui, "bring", t!("operation.bring_linked_body"), Operation::BringLinkedBody { source }, actions);
                let separate = Operation::SeparateReceivers { sources: vec![source] };
                menu_op(ui, "separate", t!("operation.separate_receivers"), separate, actions);
            }
        },
        |ui, actions| {
            if composed && flatten_button(ui) {
                let targets: Vec<EntityPath> = match in_selection {
                    true => selection
                        .iter()
                        .map(|id| EntityPath::new(path.occurrences.clone(), *id))
                        .chain(editor.deep_selection().iter().cloned())
                        .filter(|q| doc.resolve_part(q).is_ok_and(|(_, p, _)| p.has_tree()))
                        .collect(),
                    false => vec![path.clone()],
                };
                let scope = scope_in(editor, path);
                let ops = targets.into_iter().map(|target| Operation::FlattenPart { target, scope }).collect();
                actions.push(Action::Apply(Operation::Batch(ops)));
            }
        },
    );
}

/// An occurrence's row of the tree.
fn occurrence_row(
    ui: &mut egui::Ui,
    cx: TreeCx<'_>,
    row: NavigatorRow<'_>,
    folds: &mut FastSet<egui::Id>,
    errors: &mut FastMap<Id, bool>,
    over: &mut Option<EntityPath>,
    actions: &mut Vec<Action>,
) {
    let TreeCx { editor, doc, selection, .. } = cx;
    let o = row.occurrence;
    let look = occurrence_look(ui, cx, &row, errors);
    let OccurrenceLook { selected, pieces, inner, here, link_off, link_below, rename, rename_occurrence, renaming, .. } = look;
    let part = doc.part(o.component).ok();
    let name = doc.component(o.component).map(|c| c.name.clone()).unwrap_or_default();
    let links = occurrence_links(editor, doc, here, o);
    let (response, (eye, renamed, split)) = occurrence_line(ui, cx, &row, &look, &name, &links, folds);
    let tip = match pieces {
        0 => Some(t!("panel.navigator.empty_part")),
        1 if inner && part.is_some() => Some(t!("panel.navigator.failed_inside_part")),
        1 if inner => Some(t!("panel.navigator.error_inside_assembly")),
        1 => None,
        _ => Some(t!("panel.navigator.pieces", count = pieces)),
    };
    let OccurrenceLinks { overlap_tip, .. } = links;
    let tip = tip.or(if link_off { None } else { overlap_tip });
    let tip = tip.or_else(|| match (link_off, link_below, part.is_some()) {
        (true, ..) => Some(t!("panel.navigator.link_off")),
        (_, true, true) => Some(t!("panel.navigator.link_lost_in_part")),
        (_, true, false) => Some(t!("panel.navigator.link_inside_assembly")),
        _ => None,
    });
    let response = match tip {
        Some(tip) => response.tip(tip),
        None => response,
    };
    let path = EntityPath::new(row.path, o.id);
    // The whole row, its indent and toggle included.
    if ui.rect_contains_pointer(response.rect) {
        *over = Some(path.clone());
    }
    crate::kit::handles::publish(|| format!("navigator:occurrence:{}", o.id.0), response.rect);
    // Bodies dragged onto their part's row: to its root (decision 0044).
    // Dragged (decision 0044): the selected occurrences of its assembly with it when it
    // is selected. Dropped on: a part takes bodies or parts in, at its root; an assembly
    // makes a part of bodies.
    let response = response.interact(egui::Sense::click_and_drag());
    let with: Vec<Id> = match selected && editor.context() == path.occurrences.as_slice() {
        true => doc
            .context_of(&path.occurrences)
            .and_then(|c| doc.assembly(c))
            .map(|a| a.occurrences().iter().map(|x| x.id).filter(|id| selection.contains(id)).collect())
            .unwrap_or_default(),
        false => vec![o.id],
    };
    if response.drag_started() {
        let shown = if o.name.is_empty() { name.clone() } else { o.name.to_string() };
        set_drag_ghost(ui.ctx(), occurrence_icon(doc, o), shown, with.len());
    }
    response.dnd_set_drag_payload(PartsDrag { from: path.occurrences.clone(), occurrences: with });
    drop_on_occurrence(ui, editor, o, &path, &response, actions);
    // A component's name is never empty, and unique: an emptied one is kept, one taken
    // refused (the failure told); an occurrence's emptied is cleared.
    let alone = selected && selection.len() == 1;
    let on_occurrence = match (split, response.interact_pointer_pos()) {
        (Some(split), Some(at)) if response.double_clicked() => at.x < split,
        (split, _) => split.is_some(),
    };
    let key = if on_occurrence { rename_occurrence } else { rename };
    match (renaming, start_rename(ui, &response, renaming.is_some(), alone, key, renamed)) {
        (Some(true), Some(Some(name))) => {
            let context = doc.context_of(&path.occurrences).unwrap_or(doc.root());
            actions.push(Action::Apply(Operation::RenameOccurrence { context, occurrence: o.id, name }));
        }
        (_, Some(Some(name))) if !name.is_empty() => {
            actions.push(Action::Apply(Operation::RenameComponent { component: o.component, name }));
        }
        _ => {}
    }
    occurrence_menu(editor, selection, o, &path, &response, (rename, rename_occurrence), actions);
    if eye {
        actions.push(Action::SetHidden(path, !o.hidden));
    } else if response.clicked() {
        actions.push(Action::SelectPath(path, ui.input(|i| i.modifiers.shift)));
    }
}

/// The gap between a navigator row's material marks: closer than the row's items, one group.
const MARKS_GAP: f32 = 3.0;

/// A navigator row's materials: the part's material, then its coloration, each its swatch as
/// the « Matières » card and the material fields draw it (a scheme its coats); then, when faces
/// carry a covering or a coloration of their own, a swatch cut across, the most used of them in
/// its lower half. Faded on a hidden row (`greyed`); each names what it shows in its tip.
fn material_marks(ui: &mut egui::Ui, doc: &Document, p: &skraft_model::Part, greyed: bool) {
    let own = |m: &Option<Id>| m.filter(|m| doc.material(*m).is_ok());
    let swatches: Vec<Id> = [own(&p.material), own(&p.coloration)].into_iter().flatten().collect();
    // The faces' own items, the most used first, those the part shows already left out.
    let mut faces: Vec<(Id, usize)> = Vec::new();
    for m in p.coverings.values().chain(p.faces.values()) {
        if Some(*m) == p.material || Some(*m) == p.coloration || doc.material(*m).is_err() {
            continue;
        }
        match faces.iter_mut().find(|(f, _)| f == m) {
            Some((_, n)) => *n += 1,
            None => faces.push((*m, 1)),
        }
    }
    faces.sort_by_key(|f| std::cmp::Reverse(f.1));
    let n = swatches.len() + usize::from(!faces.is_empty());
    if n == 0 {
        return;
    }
    let side = crate::kit::style::MENU_MARK;
    let width = n as f32 * side + (n - 1) as f32 * MARKS_GAP;
    let (all, _) = ui.allocate_exact_size(egui::vec2(width, side), egui::Sense::hover());
    // Painted in a child that takes no room (a scope would add a gap before the name).
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(all));
    if greyed {
        child.multiply_opacity(0.4);
    }
    let at = |k: usize| egui::Rect::from_min_size(all.min + egui::vec2(k as f32 * (side + MARKS_GAP), 0.0), egui::Vec2::splat(side));
    let name = |m: Id| doc.material(m).map_or(String::new(), |m| m.name.clone());
    for (k, m) in swatches.iter().enumerate() {
        super::materials::paint_material(&child, doc, at(k), *m, 2.0);
        ui.interact(at(k), ui.scope_id().with(("material_mark", k)), egui::Sense::hover()).tip(name(*m));
    }
    if let Some(&(first, _)) = faces.first() {
        let r = at(swatches.len());
        paint_faces_mark(&child, doc, r, first);
        let names: Vec<String> = faces.iter().map(|(m, _)| name(*m)).collect();
        let tip = t!("panel.navigator.faces_tip", names = names.join(", "));
        ui.interact(r, ui.scope_id().with("faces_mark"), egui::Sense::hover()).tip(tip);
    }
}

/// The mark of faces carrying their own items: a faint swatch cut across, its lower right half
/// the colour of `m` (a scheme: its top coat).
fn paint_faces_mark(ui: &egui::Ui, doc: &Document, r: egui::Rect, m: Id) {
    let color = match doc.material(m) {
        Ok(x) if x.is_scheme() => doc.coats_of(m).last().map_or(x.color, |(_, c)| c.color),
        Ok(x) => x.color,
        Err(_) => return,
    };
    let [red, g, b] = color;
    // The rounded square's outline, its corners as the swatches' (radius 2), cut by the diagonal
    // from top right to bottom left: it crosses those two corners' arcs at their middle (their
    // centres lie on it), so each half is the outline's points on its side.
    const RADIUS: f32 = 2.0;
    const STEPS: usize = 4;
    let mut outline = Vec::new();
    let corners = [
        (r.right_bottom() + egui::vec2(-RADIUS, -RADIUS), 0.0),
        (r.left_bottom() + egui::vec2(RADIUS, -RADIUS), 0.25),
        (r.left_top() + egui::vec2(RADIUS, RADIUS), 0.5),
        (r.right_top() + egui::vec2(-RADIUS, RADIUS), 0.75),
    ];
    for (centre, start) in corners {
        for k in 0..=STEPS {
            let a = std::f32::consts::TAU * (start + 0.25 * k as f32 / STEPS as f32);
            outline.push(centre + RADIUS * egui::vec2(a.cos(), a.sin()));
        }
    }
    let side = |q: egui::Pos2| (q.x - r.left()) / r.width() + (q.y - r.top()) / r.height() - 1.0;
    let half = |lower: bool| -> Vec<egui::Pos2> {
        outline.iter().copied().filter(|q| if lower { side(*q) >= -1e-3 } else { side(*q) <= 1e-3 }).collect()
    };
    let p = ui.painter();
    p.add(egui::Shape::convex_polygon(half(false), ui.visuals().faint_bg_color, egui::Stroke::NONE));
    p.add(egui::Shape::convex_polygon(half(true), egui::Color32::from_rgb(red, g, b), egui::Stroke::NONE));
}

/// The model, the navigator's first row, at the list's left edge (no indent, no fold nor eye):
/// a click chooses it (`Editor::model_chosen`), highlighted then, the entity info showing it.
fn model_row(ui: &mut egui::Ui, editor: &Editor, name: &str, height: f32, actions: &mut Vec<Action>) {
    let selected = editor.model_chosen();
    let tint = if selected { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() };
    let (response, ()) = tree_row(ui, height, selected, false, false, |ui| {
        ui.add(egui::Image::new(icons::MODEL).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint));
        let font = crate::kit::style::semibold(egui::TextStyle::Body.resolve(ui.style()).size);
        ui.add(egui::Label::new(egui::RichText::new(name).font(font).color(tint)).selectable(false).truncate());
    });
    // Bodies dropped on the model: a part of them at its root (decision 0044).
    let (bodies_over, parts_over) = dragged_over(&response);
    if let Some(drag) = bodies_over {
        let make = Box::new(move |copy| drag.op(editor, BodiesTo::NewPart(Vec::new()), copy));
        drop_on_row(ui, &response, 0.0, Some((Mark::Into, make)), actions);
    } else if let Some(drag) = parts_over {
        // Occurrences dropped on the model: to its root.
        let doc = editor.document();
        let target = drag
            .onto_assembly(doc, Vec::new(), false)
            .map(|_| (Mark::Into, Box::new(move |copy| drag.onto_assembly(doc, Vec::new(), copy).expect("checked")) as DropOperation<'_>));
        drop_on_row(ui, &response, 0.0, target, actions);
    }
    if response.tip(t!("panel.navigator.model_tip")).clicked() {
        actions.push(Action::SelectModel);
    }
}

/// Where the editing is (the navigator's active path): the assemblies edited and the part open,
/// the groups open in it, the body whose profile is edited.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct ActivePath {
    pub(super) occurrences: Vec<Id>,
    pub(super) groups: Vec<Id>,
    pub(super) profile: Option<Id>,
}

impl ActivePath {
    pub(super) fn of(editor: &Editor) -> Self {
        let mut occurrences = editor.context().to_vec();
        let part = editor.open_part();
        occurrences.extend(part);
        let groups = if part.is_some() { editor.open_group().to_vec() } else { Vec::new() };
        Self { occurrences, groups, profile: editor.profile_body().filter(|_| part.is_some()) }
    }

    /// `self` left for `now`: the rows unfolded along `self` beyond what both share, folded
    /// (out of `folds`).
    fn fold_left(&self, births: &Births, folds: &mut FastSet<egui::Id>, now: &Self) {
        let mut fold = |id: egui::Id| {
            folds.remove(&id);
        };
        let shared = self.occurrences.iter().zip(&now.occurrences).take_while(|(a, b)| a == b).count();
        for k in shared..self.occurrences.len() {
            fold(births.occurrence_fold(&self.occurrences[..k], self.occurrences[k]));
        }
        // In the part open: its groups and the body whose profile was edited.
        let Some((&part, above)) = self.occurrences.split_last() else { return };
        let same_part = self.occurrences == now.occurrences;
        let shared = if same_part { self.groups.iter().zip(&now.groups).take_while(|(a, b)| a == b).count() } else { 0 };
        for k in shared..self.groups.len() {
            fold(births.body_fold((above, part), &self.groups[..k], self.groups[k]));
        }
        if let Some(body) = self.profile
            && !(same_part && self.groups == now.groups && now.profile == Some(body))
        {
            fold(births.body_fold((above, part), &self.groups, body));
        }
    }
}

/// The births of the ids handed out again: an undo takes the document's next id back, the
/// next things made take the ids of those undone. A row's fold is kept under its ids salted
/// with their births, so that a new thing does not open as the one undone was (a copy
/// unfolded, undone, made again).
#[derive(Clone, Debug, Default)]
pub(super) struct Births {
    /// The document's next id when last seen.
    next_id: u64,
    /// The ids given back by undos, each with how many times.
    pub(super) of: FastMap<Id, u32>,
}

impl Births {
    /// The document's next id now `next_id`: the ids above it, given back, reborn.
    fn update(&mut self, next_id: u64) {
        for id in next_id..self.next_id {
            *self.of.entry(Id(id)).or_default() += 1;
        }
        self.next_id = next_id;
    }

    /// The births of `ids`, folded into one salt (0 while no id was given back).
    pub(super) fn salt<'a>(&self, ids: impl IntoIterator<Item = &'a Id>) -> u64 {
        if self.of.is_empty() {
            return 0;
        }
        ids.into_iter().fold(0, |h: u64, id| h.rotate_left(7) ^ u64::from(self.of.get(id).copied().unwrap_or(0)))
    }

    /// The fold of the occurrence `id` under the occurrences `path`.
    fn occurrence_fold(&self, path: &[Id], id: Id) -> egui::Id {
        egui::Id::unique(("navigator", path, id, self.salt(path.iter().chain([&id]))))
    }

    /// The fold of the body `id` in the groups `group` of the part of the occurrence `at`.
    fn body_fold(&self, at: (&[Id], Id), group: &[Id], id: Id) -> egui::Id {
        let salt = self.salt(at.0.iter().chain([&at.1]).chain(group).chain([&id]));
        egui::Id::unique(("navigator_body", at.0, at.1, group, id, salt))
    }
}

/// The rows of the tree under `assembly`, those of folded assemblies left out (`hidden_above`:
/// an occurrence above `assembly` is hidden).
#[allow(clippy::too_many_arguments)]
fn flatten<'d>(
    births: &Births,
    folds: &mut FastSet<egui::Id>,
    editor: &Editor,
    doc: &'d Document,
    assembly: Id,
    active: &[Id],
    path: &mut Vec<Id>,
    out: &mut Vec<NavigatorRow<'d>>,
    hidden_above: bool,
    depth: usize,
) {
    let Ok(a) = doc.assembly(assembly) else { return };
    if depth > skraft_model::MAX_DEPTH {
        return;
    }
    // Above the end of the active path: only its step at this level.
    let step = active.get(path.len()).copied().filter(|_| active.starts_with(path));
    for o in a.occurrences() {
        if step.is_some_and(|id| id != o.id) {
            continue;
        }
        if let Ok(p) = doc.part(o.component) {
            // Its bodies under it, a single one too.
            let key = births.occurrence_fold(path, o.id);
            // Open while the part is.
            let open = unfold(folds, key, editor.context() == path.as_slice() && editor.open_part() == Some(o.id));
            out.push(NavigatorRow {
                path: path.clone(),
                occurrence: o,
                fold: Some(Fold { key, open }),
                hidden_above,
                body: None,
                dim: false,
            });
            if open {
                let open_group = (editor.context() == path.as_slice() && editor.open_part() == Some(o.id)).then(|| editor.open_group());
                let row = |fold, group, body, siblings, profile| NavigatorRow {
                    path: path.clone(),
                    occurrence: o,
                    fold,
                    hidden_above,
                    body: Some(BodyRow { part: p, body, group, siblings, profile }),
                    dim: false,
                };
                let at = (path.as_slice(), o.id);
                // The profile edited, in the group open: its body unfolded.
                let open = Open { group: open_group.unwrap_or(&[]), profile: open_group.and(editor.profile_body()) };
                body_rows(births, folds, at, p, &p.bodies, &mut Vec::new(), open, &mut |r: BodyLine<'d>| {
                    out.push(row(r.0, r.1, r.2, r.3, r.4))
                });
            }
            continue;
        }
        if doc.assembly(o.component).is_err() {
            out.push(NavigatorRow { path: path.clone(), occurrence: o, fold: None, hidden_above, body: None, dim: false });
            continue;
        }
        let key = births.occurrence_fold(path, o.id);
        // Open along the context being edited.
        let along = editor.context().len() > path.len() && editor.context().starts_with(path) && editor.context()[path.len()] == o.id;
        let open = unfold(folds, key, along);
        out.push(NavigatorRow { path: path.clone(), occurrence: o, fold: Some(Fold { key, open }), hidden_above, body: None, dim: false });
        if open {
            path.push(o.id);
            flatten(births, folds, editor, doc, o.component, active, path, out, hidden_above || o.hidden, depth + 1);
            path.pop();
        }
    }
}

/// The navigator's rows under a search (docs/ui/components-library.md#search): the rows it lets
/// through — an occurrence by its name or its component's, a body by its name or its
/// definition's —, and the rows above them, attenuated when they do not pass themselves. No
/// folds: what holds a match is shown open, the rest closed.
pub(super) struct Filtered<'a, 'd> {
    pub(super) matcher: &'a skraft_tools::search::Matcher,
    /// Does a definition hold a row that passes?
    pub(super) holds: &'a mut Holds,
    pub(super) doc: &'d Document,
}

impl<'d> Filtered<'_, 'd> {
    /// Does the occurrence `o` pass itself?
    pub(super) fn occurrence(&self, o: &Occurrence) -> bool {
        let Ok(c) = self.doc.component(o.component) else { return false };
        self.matcher.name([o.name.as_str(), c.name.as_str()])
    }

    /// Does the body `b` of `p` pass?
    pub(super) fn body(&self, p: &skraft_model::Part, b: &skraft_model::Body) -> bool {
        self.matcher.name([b.name.as_str(), p.def_of(b).name.as_str()])
    }

    /// Does one of `bodies`, or a body in their groups, pass?
    fn holds_body(&self, p: &skraft_model::Part, bodies: &[skraft_model::Body], depth: usize) -> bool {
        depth <= skraft_model::MAX_DEPTH && bodies.iter().any(|b| self.body(p, b) || self.holds_body(p, p.def_of(b).children(), depth + 1))
    }

    /// Does the definition `c` hold a row that passes?
    pub(super) fn holds(&mut self, c: Id, depth: usize) -> bool {
        if let Some(h) = self.holds.get(&c) {
            return *h;
        }
        let h = match self.doc.component(c).map(|c| &c.kind) {
            Ok(ComponentKind::Part(p)) => self.holds_body(p, &p.bodies, 0),
            Ok(ComponentKind::Assembly(a)) if depth <= skraft_model::MAX_DEPTH => {
                a.occurrences().iter().any(|o| self.occurrence(o) || self.holds(o.component, depth + 1))
            }
            _ => false,
        };
        self.holds.insert(c, h);
        h
    }

    /// The rows of the assembly `assembly` under `path`.
    pub(super) fn rows(&mut self, assembly: Id, path: &mut Vec<Id>, out: &mut Vec<NavigatorRow<'d>>, hidden_above: bool, depth: usize) {
        let doc = self.doc;
        let Ok(a) = doc.assembly(assembly) else { return };
        if depth > skraft_model::MAX_DEPTH {
            return;
        }
        for o in a.occurrences() {
            let me = self.occurrence(o);
            let below = self.holds(o.component, depth + 1);
            if !me && !below {
                continue;
            }
            out.push(NavigatorRow { path: path.clone(), occurrence: o, fold: None, hidden_above, body: None, dim: !me });
            if !below {
                continue;
            }
            if let Ok(p) = doc.part(o.component) {
                self.body_rows(p, &p.bodies, &mut Vec::new(), path, o, hidden_above, out);
            } else {
                path.push(o.id);
                self.rows(o.component, path, out, hidden_above || o.hidden, depth + 1);
                path.pop();
            }
        }
    }

    /// The rows of `bodies` (of the part `o` places, in the groups `group`) that pass, and the
    /// groups holding one.
    #[allow(clippy::too_many_arguments)]
    fn body_rows(
        &self,
        p: &'d skraft_model::Part,
        bodies: &'d [skraft_model::Body],
        group: &mut Vec<Id>,
        path: &[Id],
        o: &'d Occurrence,
        hidden_above: bool,
        out: &mut Vec<NavigatorRow<'d>>,
    ) {
        if group.len() > skraft_model::MAX_DEPTH {
            return;
        }
        for b in bodies {
            let me = self.body(p, b);
            let children = p.def_of(b).children();
            let below = self.holds_body(p, children, group.len() + 1);
            if !me && !below {
                continue;
            }
            let body = Some(BodyRow { part: p, body: b, group: group.clone(), siblings: bodies, profile: false });
            out.push(NavigatorRow { path: path.to_vec(), occurrence: o, fold: None, hidden_above, body, dim: !me });
            if below {
                group.push(b.id);
                self.body_rows(p, children, group, path, o, hidden_above, out);
                group.pop();
            }
        }
    }
}

/// Whether the row of fold `key` is open: unfolded in `folds`, or `forced` (along the active
/// path), then kept unfolded there (folded again as the active path leaves it, `fold_left`).
fn unfold(folds: &mut FastSet<egui::Id>, key: egui::Id, forced: bool) -> bool {
    if forced {
        folds.insert(key);
        true
    } else {
        folds.contains(&key)
    }
}

/// Is the component `id` in error, or does it hold one, at any depth: a part in several pieces
/// or empty, or holding a body failed; an assembly holding such a part. Remembered in `memo`,
/// `depth` assemblies down.
fn in_error(doc: &Document, id: Id, memo: &mut FastMap<Id, bool>, depth: usize) -> bool {
    if let Some(e) = memo.get(&id) {
        return *e;
    }
    let e = match doc.part(id) {
        Ok(p) => p.pieces() != 1 || holds_failure(p, &p.bodies, 0),
        Err(_) => {
            depth <= skraft_model::MAX_DEPTH
                && doc.assembly(id).is_ok_and(|a| a.occurrences().iter().any(|o| in_error(doc, o.component, memo, depth + 1)))
        }
    };
    memo.insert(id, e);
    e
}

/// Does one of `bodies` (of the part `p`, `depth` groups down) fail, or a body of a group among
/// them, at any depth?
fn holds_failure(p: &skraft_model::Part, bodies: &[skraft_model::Body], depth: usize) -> bool {
    depth <= skraft_model::MAX_DEPTH && bodies.iter().any(|b| p.is_failed(b.id) || holds_failure(p, p.def_of(b).children(), depth + 1))
}

/// A body row's parts: its folding state (a group's, a profiled body's), the groups it lies in,
/// the body, its siblings, whether the row is the body's profile's.
pub(super) type BodyLine<'d> = (Option<Fold>, Vec<Id>, &'d skraft_model::Body, &'d [skraft_model::Body], bool);

/// What is open in a part, its rows unfolded along it: the group open (the bodies leading to
/// it), the body of that group whose profile is edited.
#[derive(Clone, Copy)]
pub(super) struct Open<'a> {
    pub(super) group: &'a [Id],
    pub(super) profile: Option<Id>,
}

/// The rows of `bodies` (of the part `p` the occurrence `at` places, in the groups `group`), in
/// their order of creation (a row toggling its sign stays under the pointer);
/// under an open group, its children. The groups along `open` (the group open in the part) are
/// unfolded.
#[allow(clippy::too_many_arguments)]
fn body_rows<'d>(
    births: &Births,
    folds: &mut FastSet<egui::Id>,
    at: (&[Id], Id),
    p: &'d skraft_model::Part,
    bodies: &'d [skraft_model::Body],
    group: &mut Vec<Id>,
    open: Open,
    out: &mut dyn FnMut(BodyLine<'d>),
) {
    if group.len() > skraft_model::MAX_DEPTH {
        return;
    }
    // Above the end of the active path (the group open, the profile edited): only its step.
    let step = match open.group.get(group.len()) {
        Some(&id) if open.group.starts_with(group) => Some(id),
        _ => open.profile.filter(|_| open.group == group.as_slice()),
    };
    for b in bodies {
        if step.is_some_and(|id| id != b.id) {
            continue;
        }
        let children = p.def_of(b).children();
        let profiled = p.profile_of(b).is_some();
        if children.is_empty() && !profiled {
            out((None, group.clone(), b, bodies, false));
            continue;
        }
        let key = births.body_fold(at, group, b.id);
        // Along the group open, or its profile edited: unfolded.
        let edited = open.profile == Some(b.id) && open.group == group.as_slice();
        group.push(b.id);
        let opened = unfold(folds, key, edited || open.group.starts_with(group));
        group.pop();
        out((Some(Fold { key, open: opened }), group.clone(), b, bodies, false));
        if !opened {
            continue;
        }
        if profiled {
            out((None, group.clone(), b, bodies, true));
        } else {
            group.push(b.id);
            body_rows(births, folds, at, p, children, group, open, out);
            group.pop();
        }
    }
}

/// A body's « Profil » line, under it on the band of its part's bodies: selected while it is
/// edited (`editing`, in the part or group open), a click editing it (its part and group
/// opened).
fn profile_row(ui: &mut egui::Ui, editor: &Editor, row: &NavigatorRow, height: f32, editing: Option<Id>, actions: &mut Vec<Action>) {
    let Some(b) = &row.body else { return };
    let path = EntityPath::new(row.path.clone(), row.occurrence.id).in_group(b.group.clone());
    let selected = editor.open_part_path().as_ref() == Some(&path) && editing == Some(b.body.id);
    let v = ui.visuals();
    let tint = if selected {
        v.selection.stroke.color
    } else if row.occurrence.hidden || row.hidden_above {
        hidden_color(v, true)
    } else {
        v.text_color()
    };
    let (response, ()) = tree_row(ui, height, selected, false, true, |ui| {
        // Under its body's icon: one level further, its fold's room left.
        let spacing = ui.spacing().item_spacing.x;
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(ui.spacing().indent * (row.path.len() + 3 + b.group.len()) as f32);
        ui.spacing_mut().item_spacing.x = spacing;
        ui.add(egui::Image::new(icons::PROFILE).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint));
        let label = egui::RichText::new(t!("panel.navigator.profile")).color(tint);
        ui.add(egui::Label::new(label).selectable(false).truncate());
    });
    if response.tip(t!("panel.navigator.profile_tip")).clicked() && !selected {
        actions.push(Action::EditProfile(path, b.body.id));
    }
}

/// One body's line of the navigator, on the band of its part's bodies: its fold for a group, its
/// nature's icon and name, its sign toggle at the right end; a click opens its part and selects
/// the part's own body it lies in (Shift: added). Its part open on its siblings, it is hovered
/// and selected with the view (`over`: the body hovered here). The toggle
/// flips its sign — the selected bodies' too when it is one of them, in its part or group
/// opened —, in every occurrence of the part unless it is open for this one alone
/// (« Celui-ci »).
#[allow(clippy::too_many_arguments)]
/// Bodies dragged in the navigator (decision 0044, docs/model/bodies.md#moving-bodies): the
/// level they lie in (its part's path and groups), which of its bodies.
#[derive(Clone, Debug)]
pub(super) struct BodiesDrag {
    pub(super) from: EntityPath,
    pub(super) bodies: Vec<Id>,
}

impl BodiesDrag {
    /// Where bodies dropped at the level `level` of the part `part` places go, if they may: in
    /// their own part (`same`), another level of it, not where they lie nor inside one of them;
    /// in another part, into it.
    pub(super) fn to(&self, same: bool, part: &EntityPath, level: Vec<Id>) -> Option<BodiesTo> {
        if !same {
            return Some(BodiesTo::Part(part.clone().in_group(level)));
        }
        (level != self.from.group && !level.iter().any(|g| self.bodies.contains(g))).then_some(BodiesTo::Level(level))
    }

    /// The move to `to`, with the scope of their part when it is open.
    pub(super) fn op(&self, editor: &Editor, to: BodiesTo, copy: bool) -> Operation {
        let scope = scope_in(editor, &self.from);
        Operation::MoveBodies { from: self.from.clone(), bodies: self.bodies.clone(), to, copy, scope }
    }
}

/// The scope a change made from a row acts with: the one its part is open with (here or in one
/// of its groups), else all.
fn scope_in(editor: &Editor, path: &EntityPath) -> Scope {
    let part_open = editor.open_part_path().is_some_and(|o| o.part_path() == path.part_path());
    if part_open { editor.part_scope() } else { Scope::All }
}

/// The operation a drop makes, told whether the copy modifier is held.
pub(super) type DropOperation<'a> = Box<dyn FnOnce(bool) -> Operation + 'a>;

/// Where a drag may land on a row: how it is marked, and what dropping it there does.
pub(super) type DropTarget<'a> = Option<(Mark, DropOperation<'a>)>;

/// Occurrences dragged in the navigator (decision 0044): the assembly they lie in (the
/// occurrences leading to it), which of its occurrences.
#[derive(Clone, Debug)]
pub(super) struct PartsDrag {
    pub(super) from: Vec<Id>,
    pub(super) occurrences: Vec<Id>,
}

impl PartsDrag {
    /// Into the part `part` places, at its level `level`, as its bodies: not a part they place,
    /// nor one inside them (an assembly dragged), nor themselves.
    fn onto_part(&self, doc: &Document, part: &EntityPath, level: Vec<Id>, copy: bool) -> Option<Operation> {
        let n = self.from.len();
        let inside =
            part.occurrences.len() > n && part.occurrences[..n] == self.from[..] && self.occurrences.contains(&part.occurrences[n]);
        let itself = part.occurrences == self.from && self.occurrences.contains(&part.entity);
        let component = doc.resolve(part).ok()?.component;
        let context = doc.context_of(&self.from).ok()?;
        let placed =
            doc.assembly(context).ok()?.occurrences().iter().filter(|o| self.occurrences.contains(&o.id)).any(|o| o.component == component);
        (!inside && !itself && !placed).then(|| Operation::MoveParts {
            from: self.from.clone(),
            occurrences: self.occurrences.clone(),
            to: part.clone().in_group(level),
            copy,
        })
    }
}

impl PartsDrag {
    /// Into the assembly `to` leads to (empty: the model): not the one they lie in, nor one of
    /// them or one they hold (decision 0044).
    fn onto_assembly(&self, doc: &Document, to: Vec<Id>, copy: bool) -> Option<Operation> {
        let (from, into) = (doc.context_of(&self.from).ok()?, doc.context_of(&to).ok()?);
        let holds = doc
            .assembly(from)
            .ok()?
            .occurrences()
            .iter()
            .filter(|o| self.occurrences.contains(&o.id))
            .any(|o| doc.contains_component(o.component, into));
        (from != into && !holds).then(|| Operation::MoveOccurrences {
            from: self.from.clone(),
            occurrences: self.occurrences.clone(),
            to,
            copy,
        })
    }
}

/// Does the occurrence `a` place the same part as `b`?
fn same_part(doc: &Document, a: &EntityPath, b: &EntityPath) -> bool {
    match (doc.resolve(a), doc.resolve(b)) {
        (Ok(x), Ok(y)) => x.component == y.component,
        _ => false,
    }
}

/// The path of the group `id` lying in the groups `group`: the level its children are in.
fn inner_path(group: &[Id], id: Id) -> Vec<Id> {
    group.iter().copied().chain([id]).collect()
}

/// How a row shows where a drop on it goes.
#[derive(Clone, Copy)]
pub(super) enum Mark {
    /// Into the row's group, part or assembly: the row framed.
    Into,
    /// Beside the row, in its level: a line above it (`true`) or under it.
    Beside(bool),
}

/// While something is dragged over a row (`response`, from `indent` on): `target` — how the
/// row shows where it would go and the operation (given whether the platform's copy key is
/// held), none when it may not go there — marked, the cursor; dropped, the operation.
fn drop_on_row(ui: &egui::Ui, response: &egui::Response, indent: f32, target: DropTarget<'_>, actions: &mut Vec<Action>) {
    let copy = crate::workspaces::common::viewport::modifiers(ui.ctx(), ui.input(|i| i.modifiers)).copy;
    let Some((mark, op)) = target else {
        ui.ctx().set_cursor_icon(egui::CursorIcon::NoDrop);
        return;
    };
    let stroke = egui::Stroke::new(1.5, ui.visuals().selection.stroke.color);
    let r = response.rect;
    match mark {
        Mark::Into => {
            ui.painter().rect_stroke(r.shrink(1.0), 3.0, stroke, egui::StrokeKind::Inside);
        }
        Mark::Beside(above) => {
            let y = if above { r.top() } else { r.bottom() };
            ui.painter().hline(r.left() + indent..=r.right(), y, stroke);
        }
    }
    ui.ctx().set_cursor_icon(if copy { egui::CursorIcon::Copy } else { egui::CursorIcon::Grabbing });
    // Taken as the type it is: egui's takes the payload whatever its type, and keeps it only
    // when it is the one asked.
    let released = match response.dnd_hover_payload::<BodiesDrag>() {
        Some(_) => response.dnd_release_payload::<BodiesDrag>().is_some(),
        None => response.dnd_release_payload::<PartsDrag>().is_some(),
    };
    if released {
        actions.push(Action::Apply(op(copy)));
    }
}

/// Is something of the navigator dragged over `response`?
fn dragged_over(response: &egui::Response) -> (Option<std::sync::Arc<BodiesDrag>>, Option<std::sync::Arc<PartsDrag>>) {
    (response.dnd_hover_payload::<BodiesDrag>(), response.dnd_hover_payload::<PartsDrag>())
}

/// How a body's row shows, and its name's edit.
struct BodyLook<'d> {
    path: EntityPath,
    def: &'d skraft_model::BodyDef,
    /// The level open is the one it lies in.
    open: bool,
    /// Its bodies shown, there: it can be selected, lit, hovered.
    shown: bool,
    selected: bool,
    lit: bool,
    tint: egui::Color32,
    weak: egui::Color32,
    failed: bool,
    /// A group holding a body failed, at any depth.
    failed_inside: bool,
    /// ⚠ before its name.
    warned: bool,
    /// A linked body that lost its link, staying as it is (docs/model/bodies.md#links).
    link_lost: bool,
    /// A linked body: it follows its source, neither its sign nor its name are its own.
    linked: bool,
    /// The name of the part it takes its shape from, while its link holds.
    source: Option<String>,
    rename: egui::Id,
    rename_own: egui::Id,
    /// Its name edited: the body's own (true) or its definition's.
    renaming: Option<bool>,
}

fn body_look<'d>(ui: &egui::Ui, editor: &Editor, row: &NavigatorRow<'d>, b: &BodyRow<'d>) -> BodyLook<'d> {
    let path = EntityPath::new(row.path.clone(), row.occurrence.id).in_group(b.group.clone());
    let open = editor.open_part_path().as_ref() == Some(&path);
    let shown = open && editor.showing_machinings();
    // A group (a node) open: its row marked open, as a part's is, while nothing in it is
    // selected; those on the way to it, their name in the accent's colour.
    let inner: Vec<Id> = b.group.iter().copied().chain([b.body.id]).collect();
    let open_path = editor.open_part_path().filter(|o| o.part_path() == path.part_path());
    let is_open = open_path.as_ref().is_some_and(|o| o.group == inner);
    let on_path = open_path.as_ref().is_some_and(|o| o.group.len() > inner.len() && o.group.starts_with(&inner));
    let open_mark = is_open && editor.selection().is_empty() && editor.profile_body().is_none();
    let selected = (shown && editor.selection().contains(&b.body.id)) || open_mark;
    let lit = shown && editor.hovered_machining() == Some(b.body.id);
    let greyed = row.occurrence.hidden || row.hidden_above;
    let v = ui.visuals();
    let weak = v.weak_text_color();
    let tint = if selected || on_path {
        v.selection.stroke.color
    } else if greyed {
        hidden_color(v, true)
    } else if row.dim {
        weak
    } else {
        v.text_color()
    };
    let failed = b.part.is_failed(b.body.id);
    // A group failed (empty, or its boolean failing): ⚠ as a part in error, an error to correct
    // (docs/model/bodies.md#empty-parts); one holding a body failed, at any depth: ⚠ too, its name as it is.
    let def = b.part.def_of(b.body);
    let inner = def.is_group() && !failed && holds_failure(b.part, def.children(), 0);
    let link_lost = editor.link_status().broken.contains(&(row.occurrence.component, b.body.id));
    let warned = (failed && def.is_group()) || inner || link_lost;
    let linked = def.source == skraft_model::BodySource::Linked;
    let source = editor.link_map().source_of(row.occurrence.component, b.body.id);
    let source = source.and_then(|s| editor.document().component(s).ok()).map(|c| c.name.clone());
    // A name edited in place (docs/model/bodies.md#body-names), from a double-click, or F2 while it is the
    // one body selected: the body's own, shown first, where it has one and the double-click is
    // on it (F2: where it has one), else its definition's (« Haut <Trou#1> »), as an
    // occurrence's row.
    let rename = ui.make_persistent_id(("navigator_rename", &row.path, row.occurrence.id, &b.group, b.body.id));
    let rename_own = rename.with("own");
    let renaming = match ui.data(|d| d.get_temp::<egui::Id>(egui::Id::unique(RENAMING))) {
        Some(id) if id == rename => Some(false),
        Some(id) if id == rename_own => Some(true),
        _ => None,
    };
    BodyLook {
        path,
        def,
        open,
        shown,
        selected,
        lit,
        tint,
        weak,
        failed,
        failed_inside: inner,
        warned,
        link_lost,
        linked,
        source,
        rename,
        rename_own,
        renaming,
    }
}

/// Draws a body's line: its fold, icon, sign toggle, name (or its field) and node size.
fn body_line(
    ui: &mut egui::Ui,
    editor: &Editor,
    row: &NavigatorRow<'_>,
    b: &BodyRow<'_>,
    look: &BodyLook<'_>,
    height: f32,
    folds: (egui::Id, &mut FastSet<egui::Id>),
) -> (egui::Response, LineOutcome) {
    let BodyLook { def, selected, lit, tint, weak, failed, warned, linked, rename, rename_own, renaming, .. } = *look;
    let icon = egui::Image::new(b.icon()).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(tint);
    // Under the part's icon: its level, plus one, plus the groups above.
    let depth = row.path.len() + 1 + b.group.len();
    let line = TreeLine { height, selected, lit, band: true, depth, fold: row.fold, icon };
    // The sign toggle first, at the right end where a part's row has its eye, then the name in
    // the room left.
    let sign = |ui: &mut egui::Ui, over: bool| {
        let (icon, tip) = match b.body.sign {
            Sign::Positive => (icons::UNION, t!("panel.navigator.sign_positive")),
            Sign::Negative => (icons::SUBTRACT, t!("panel.navigator.sign_negative")),
            Sign::Intersect => (icons::INTERSECT, t!("panel.navigator.sign_intersect")),
        };
        let mark = if selected { tint } else { weak };
        let img = egui::Image::new(icon).fit_to_exact_size(egui::vec2(14.0, 14.0)).tint(mark);
        let sense = if linked { egui::Sense::hover() } else { egui::Sense::click() };
        let sign = ui.add(egui::Button::image(img).frame(false).sense(sense));
        crate::kit::handles::publish(|| format!("navigator:sign:{}", b.body.id.0), sign.rect);
        // A linked body is a machining by its link (docs/model/bodies.md#links): its sign is not
        // clicked, a chain on its corner, its tip naming the source. Tied to it as the source's
        // row is to it.
        if let Some(source) = &look.source {
            let v = ui.visuals();
            let ground = match (selected, over || lit) {
                (true, _) => v.selection.bg_fill,
                (false, true) => v.widgets.hovered.weak_bg_fill,
                (false, false) => band_color(v),
            };
            linked_badge(ui, egui::Rect::from_center_size(sign.rect.center(), egui::vec2(14.0, 14.0)), mark, ground);
            return sign.tip(t!("panel.navigator.linked", source = source)).clicked();
        }
        sign.tip(tip).clicked()
    };
    let label = |ui: &mut egui::Ui| {
        if warned {
            ui.add(warning(ui));
        }
        match renaming {
            Some(true) => return (rename_field(ui, rename_own, &b.body.name), None),
            Some(false) => return (rename_field(ui, rename, &b.label()), None),
            None => {}
        }
        let color = if failed { ui.visuals().error_fg_color } else { tint };
        let back = band_color(ui.visuals());
        let (_, split) = occurrence_label(ui, &b.body.name, &b.label(), !def.named, color, selected, back);
        // A body node's size after its name (docs/tools/rounds.md).
        if let Some(op) = def.op().and_then(|o| o.blend()) {
            let size = editor.document().settings().units.format(op.size);
            let text = match op.kind {
                skraft_model::kernel::names::Blend::Round => t!("panel.navigator.round_size", size = size),
                skraft_model::kernel::names::Blend::Chamfer => t!("panel.navigator.chamfer_size", size = size),
            };
            ui.add_space(6.0);
            ui.label(egui::RichText::new(text).color(weak));
        }
        (None, split)
    };
    let (response, (toggled, (renamed, split))) = tree_line(ui, line, folds, sign, label);
    (response, (toggled, renamed, split))
}

/// A body's row's tip: why its node is refused, or what its ⚠ warns of.
fn body_tip(response: egui::Response, b: &BodyRow<'_>, look: &BodyLook<'_>) -> egui::Response {
    let (inner, warned, def) = (look.failed_inside, look.warned, look.def);
    if let Some(r) = b.part.refusal_of(b.body.id) {
        response.tip(skraft_model::blend_refusal(r))
    } else if look.link_lost {
        response.tip(t!("panel.navigator.link_lost"))
    } else if warned {
        let tip = match (inner, def.solid.is_empty()) {
            (true, _) => t!("panel.navigator.failed_inside_group"),
            (false, true) => t!("panel.navigator.empty_group"),
            (false, false) => t!("panel.navigator.failed_group"),
        };
        response.tip(tip)
    } else {
        response
    }
}

/// Something of the navigator dragged over a body's row (decision 0044): marked where it
/// would go, moved there when dropped. A group's middle takes it in, its edges and a leaf put
/// it beside them, in their level.
fn drop_on_body(
    ui: &egui::Ui,
    editor: &Editor,
    row: &NavigatorRow<'_>,
    b: &BodyRow<'_>,
    look: &BodyLook<'_>,
    response: &egui::Response,
    actions: &mut Vec<Action>,
) {
    let (path, def) = (&look.path, look.def);
    let (bodies_over, parts_over) = dragged_over(response);
    if bodies_over.is_some() || parts_over.is_some() {
        let doc = editor.document();
        let part = path.part_path();
        let r = response.rect;
        let y = ui.input(|i| i.pointer.hover_pos()).map_or(r.center().y, |p| p.y);
        let into = def.is_group() && (y - r.top()) > r.height() * 0.25 && (r.bottom() - y) > r.height() * 0.25;
        let (level, mark) = match into {
            true => (inner_path(&b.group, b.body.id), Mark::Into),
            false => (b.group.clone(), Mark::Beside(y < r.center().y)),
        };
        let target: DropTarget<'_> = match (bodies_over, parts_over) {
            (Some(drag), _) => {
                let same = same_part(doc, &drag.from.part_path(), &part);
                drag.to(same, &part, level).map(|to| (mark, Box::new(move |copy| drag.op(editor, to, copy)) as Box<_>))
            }
            (None, Some(drag)) => drag
                .onto_part(doc, &part, level.clone(), false)
                .map(|_| (mark, Box::new(move |copy| drag.onto_part(doc, &part, level, copy).expect("checked")) as Box<_>)),
            _ => None,
        };
        let indent = ui.spacing().indent * (row.path.len() + 1 + b.group.len()) as f32;
        drop_on_row(ui, response, indent, target, actions);
    }
}

/// A body's row's menu, with « Sortir du groupe » (« Sortir de la pièce », at its root), acting
/// on the bodies `with`.
fn body_menu(
    editor: &Editor,
    b: &BodyRow<'_>,
    look: &BodyLook<'_>,
    response: &egui::Response,
    with: &[Id],
    part: Id,
    actions: &mut Vec<Action>,
) {
    let path = &look.path;
    let (def, open, rename, rename_own) = (look.def, look.open, look.rename, look.rename_own);
    let select = (!(open && editor.selection().contains(&b.body.id))).then(|| Action::SelectBody(path.clone(), b.body.id, false));
    // A linked body's name is its source's: only its own can be edited.
    let linked = look.linked;
    let own = (!linked && !b.body.name.is_empty()).then_some(rename_own);
    let is_group = def.is_group();
    let move_out = |ui: &mut egui::Ui, actions: &mut Vec<Action>| {
        ui.separator();
        if !linked {
            let (label, to) = match b.group.split_last() {
                Some((_, up)) => (t!("menu.edit.move_out"), skraft_model::BodiesTo::Level(up.to_vec())),
                None => (t!("menu.edit.move_out_of_part"), skraft_model::BodiesTo::NewPart(path.occurrences.clone())),
            };
            let button = ui.button(label);
            crate::kit::handles::publish(|| "navigator:menu:move_out".into(), button.rect);
            if button.clicked() {
                let scope = scope_in(editor, path);
                actions.push(Action::Apply(Operation::MoveBodies { from: path.clone(), bodies: with.to_vec(), to, copy: false, scope }));
            }
        }
        if linked && b.group.is_empty() {
            let unlink = Operation::Unlink { part, bodies: vec![b.body.id] };
            menu_op(ui, "unlink", t!("operation.unlink"), unlink, actions);
        }
    };
    let flatten = |ui: &mut egui::Ui, actions: &mut Vec<Action>| {
        if is_group && flatten_button(ui) {
            let scope = scope_in(editor, path);
            actions.push(Action::Apply(Operation::FlattenBodies { target: path.clone(), bodies: with.to_vec(), scope }));
        }
    };
    let named = if linked { rename_own } else { rename };
    row_menu(response, is_group, (named, own), select, actions, move_out, flatten);
}

fn body_row(
    ui: &mut egui::Ui,
    editor: &Editor,
    row: NavigatorRow,
    height: f32,
    folds: (egui::Id, &mut FastSet<egui::Id>),
    actions: &mut Vec<Action>,
    over: &mut Option<Id>,
) {
    let Some(b) = &row.body else { return };
    let look = body_look(ui, editor, &row, b);
    let (response, (toggled, renamed, split)) = body_line(ui, editor, &row, b, &look, height, folds);
    let response = body_tip(response, b, &look);
    let BodyLook { shown, selected, rename, rename_own, renaming, .. } = look;
    if shown && ui.rect_contains_pointer(response.rect) {
        *over = Some(b.body.id);
    }
    // Dragged (decision 0044): the selected siblings with it when it is selected.
    crate::kit::handles::publish(|| format!("navigator:body:{}", b.body.id.0), response.rect);
    let response = response.interact(egui::Sense::click_and_drag());
    let with: Vec<Id> = match selected {
        true => editor.selection().iter().copied().filter(|id| b.siblings.iter().any(|s| s.id == *id)).collect(),
        false => vec![b.body.id],
    };
    if response.drag_started() {
        let shown = if b.body.name.is_empty() { b.label() } else { b.body.name.to_string() };
        set_drag_ghost(ui.ctx(), b.icon(), shown, with.len());
    }
    response.dnd_set_drag_payload(BodiesDrag { from: look.path.clone(), bodies: with.clone() });
    drop_on_body(ui, editor, &row, b, &look, &response, actions);
    let alone = selected && editor.selection().len() == 1;
    let on_own = match (split, response.interact_pointer_pos()) {
        (Some(split), Some(at)) if response.double_clicked() => at.x < split,
        (split, _) => split.is_some(),
    };
    let key = if on_own || look.linked { rename_own } else { rename };
    body_menu(editor, b, &look, &response, &with, row.occurrence.component, actions);
    let part = row.occurrence.component;
    match (renaming, start_rename(ui, &response, renaming.is_some(), alone, key, renamed)) {
        (Some(true), Some(Some(name))) => actions.push(Action::Apply(Operation::RenameBody { part, body: b.body.id, name })),
        (_, Some(Some(name))) => actions.push(Action::Apply(Operation::RenameBodyDef { part, def: b.body.def, name })),
        _ => {}
    }
    if toggled {
        let sign = match b.body.sign {
            // + → − → ∩ → + (docs/decisions/0045-intersection-a-third-sign.md).
            Sign::Positive => Sign::Negative,
            Sign::Negative => Sign::Intersect,
            Sign::Intersect => Sign::Positive,
        };
        // Selected (in its part or group, opened): the selected siblings with it.
        let scope = scope_in(editor, &look.path);
        actions.push(Action::Apply(Operation::SetSign { target: look.path, bodies: with, sign, scope }));
    } else if response.clicked() {
        actions.push(Action::SelectBody(look.path, b.body.id, ui.input(|i| i.modifiers.shift)));
    }
}

/// A navigator menu's entry applying `op`, under the handle `navigator_menu:<handle>`.
fn menu_op(ui: &mut egui::Ui, handle: &str, label: String, op: Operation, actions: &mut Vec<Action>) {
    let button = ui.button(label);
    crate::kit::handles::publish(|| format!("navigator:menu:{handle}"), button.rect);
    if button.clicked() {
        actions.push(Action::Apply(op));
    }
}

/// The menu's « Aplatir » (docs/model/bodies.md#flattening): clicked?
fn flatten_button(ui: &mut egui::Ui) -> bool {
    let button = ui.button(t!("common.flatten"));
    crate::kit::handles::publish(|| "navigator:menu:flatten".into(), button.rect);
    button.clicked()
}

/// A navigator row's right-click menu: « Renommer » (the field a double-click opens, under the
/// key `rename`; with an occurrence name, under `own`, one entry for each name), what `move_out`
/// adds (with its own separator), then « Aplatir » (`flatten`), « Éclater » where the row can be
/// (`explodable`), « Effacer ». Opening it changes nothing; the commands act on the selection
/// when the row is in it, else on the row alone, `select` choosing it first (docs/ui/interface.md).
fn row_menu(
    response: &egui::Response,
    explodable: bool,
    (rename, own): (egui::Id, Option<egui::Id>),
    mut select: Option<Action>,
    actions: &mut Vec<Action>,
    move_out: impl FnOnce(&mut egui::Ui, &mut Vec<Action>),
    flatten: impl FnOnce(&mut egui::Ui, &mut Vec<Action>),
) {
    egui::Popup::context_menu(response).style(crate::kit::style::menu).show(|ui| {
        let names = match own {
            Some(own) => vec![
                (t!("panel.navigator.rename_occurrence"), own, "rename_occurrence"),
                (t!("panel.navigator.rename_definition"), rename, "rename"),
            ],
            None => vec![(t!("panel.navigator.rename"), rename, "rename")],
        };
        for (label, key, handle) in names {
            let button = ui.button(label);
            crate::kit::handles::publish(|| format!("navigator:menu:{handle}"), button.rect);
            if button.clicked() {
                ui.data_mut(|d| d.insert_temp(egui::Id::unique(RENAMING), key));
            }
        }
        move_out(ui, actions);
        ui.separator();
        flatten(ui, actions);
        if explodable {
            let button = ui.button(t!("menu.edit.explode"));
            crate::kit::handles::publish(|| "navigator:menu:explode".into(), button.rect);
            if button.clicked() {
                actions.extend(select.take());
                actions.push(Action::Command("edit.explode"));
            }
        }
        let button = ui.button(t!("menu.edit.erase"));
        crate::kit::handles::publish(|| "navigator:menu:delete".into(), button.rect);
        if button.clicked() {
            actions.extend(select.take());
            actions.push(Action::Command("edit.erase"));
        }
    });
}
