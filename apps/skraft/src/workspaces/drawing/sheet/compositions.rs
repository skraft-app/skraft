//! Compositions on a sheet: made, entered, carried, exploded (docs/ui/drawing.md#compositions).

use super::*;

/// Where a composition carried lands on sheet `under`: the level open on the active sheet
/// (`level`: the sheet, what is open, its view), right on the sheet on another.
pub(super) fn carry_target(
    board: &Board,
    view: Paperview,
    level: Option<(Id, Container, Paperview)>,
    under: Id,
) -> Option<(Id, Container, Paperview)> {
    match level {
        Some(l) if l.0 == under => Some(l),
        _ => board.sheet(under).map(|s| (under, Container::Sheet(under), view.at(s.origin))),
    }
}

/// The composition dragged from the card: its outline at the pointer over a sheet, an occurrence
/// there, free, when the button is let go, that sheet becoming active; given up off the papers,
/// over a card, inside itself, or by Esc.
#[allow(clippy::too_many_arguments)]
pub(super) fn carry_composition(
    ui: &egui::Ui,
    e: &mut Editor,
    st: &mut SheetState,
    room: egui::Rect,
    board: &Board,
    view: Paperview,
    level: Option<(Id, Container, Paperview)>,
) {
    let Some(composition) = st.carried else { return };
    let ctx = ui.ctx();
    let (pos, down, esc) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down(), i.key_pressed(egui::Key::Escape)));
    let d = e.document().drawing();
    if esc || d.composition(composition).is_none() {
        st.carried = None;
        return;
    }
    let over = pos.filter(|p| on_stage(ctx, room, *p));
    let target = over
        .and_then(|p| board.sheet_at(view.to_sheet(p)))
        .and_then(|under| carry_target(board, view, level, under))
        .filter(|(_, to, _)| !matches!(to, Container::Composition(x) if d.reaches(composition, *x)));
    // Outlined as the presentation it lands in projects its views.
    let projection = target.and_then(|(s, ..)| d.presentation_of(s)).map_or_else(|| st.projection(d), |p| p.projection);
    let Some(sketch) = st.library.sketch(e, composition, projection).cloned() else {
        if !down {
            st.carried = None;
        }
        return;
    };
    let at = over.zip(target).map(|(p, (_, _, lv))| lv.to_sheet(p) - (sketch.lo + sketch.hi) / 2.0);
    if let (Some(at), Some((_, _, lv))) = (at, target) {
        let stroke = egui::Stroke::new(1.0, ui.visuals().selection.stroke.color);
        let painter = ui.painter().with_clip_rect(room);
        painter
            .extend(sketch.segments.iter().map(|(a, b)| egui::Shape::line_segment([lv.to_screen(*a + at), lv.to_screen(*b + at)], stroke)));
    }
    if down {
        return;
    }
    st.carried = None;
    if let (Some(at), Some((sheet, to, _))) = (at, target) {
        let place = Place::free(at);
        let element = ElementKind::Occurrence(skraft_model::drawing::Occurrence { composition, place, props: Default::default() });
        match e.apply(Operation::AddElement { to, element }) {
            Ok(Some(o)) => {
                if level.map(|l| l.0) != Some(sheet) {
                    make_active(st, board, sheet);
                }
                st.selected = o.element.into_iter().collect();
            }
            Ok(None) => {}
            Err(m) => st.message = Some(m),
        }
    }
}

/// The occurrences among them.
pub fn selected_occurrences<'a>(e: &'a Editor, st: &SheetState) -> Vec<(Id, &'a skraft_model::drawing::Occurrence)> {
    selection(e, st).into_iter().filter_map(|el| el.kind.occurrence().map(|i| (el.id, i))).collect()
}

/// The elements picked gathered into a composition placed there, picked.
pub fn compose_selected(e: &mut Editor, st: &mut SheetState) -> Result<(), String> {
    let elements: Vec<Id> = selection(e, st).iter().map(|el| el.id).collect();
    if elements.is_empty() {
        return Err(t!("drawing.no_view_selected"));
    }
    if let Some(o) = e.apply(Operation::MakeComposition { elements, name: None })? {
        st.selected = o.element.into_iter().collect();
    }
    Ok(())
}

/// The occurrences picked replaced by what their compositions hold, picked.
pub fn explode_selected(e: &mut Editor, st: &mut SheetState) -> Result<(), String> {
    let ops: Vec<Operation> = selected_occurrences(e, st).iter().map(|(id, _)| Operation::ExplodeOccurrence { occurrence: *id }).collect();
    let op = match ops.len() {
        0 => return Err(t!("drawing.no_view_selected")),
        1 => ops.into_iter().next().expect("one"),
        _ => Operation::Batch(ops),
    };
    if let Some(o) = e.apply(op)? {
        st.selected = o.elements;
    }
    Ok(())
}

/// The occurrences picked each given a composition of its own.
pub fn unique_selected(e: &mut Editor, st: &SheetState) -> Result<(), String> {
    let ops: Vec<Operation> =
        selected_occurrences(e, st).iter().map(|(id, _)| Operation::MakeOccurrenceUnique { occurrence: *id }).collect();
    match ops.len() {
        0 => Err(t!("drawing.no_view_selected")),
        1 => e.apply(ops.into_iter().next().expect("one")).map(|_| ()),
        _ => e.apply(Operation::Batch(ops)).map(|_| ()),
    }
}

/// The occurrence picked entered, its composition edited in place.
pub fn edit_selected(e: &Editor, st: &mut SheetState) -> Result<(), String> {
    let Some((id, _)) = selected_occurrences(e, st).first().copied() else { return Err(t!("drawing.no_view_selected")) };
    st.enter(id);
    Ok(())
}

/// Definition `composition` placed on the level open: where its first occurrence on a sheet is (a title
/// block at its corner), a little aside on the sheet already showing it.
pub(super) fn place_composition(e: &mut Editor, st: &mut SheetState, composition: Id) {
    let d = e.document().drawing();
    let Some(s) = st.shown(d) else { return };
    let Some(to) = st.container(d) else { return };
    let on = |list: &[skraft_model::Element]| {
        list.iter().find_map(|el| el.kind.occurrence().filter(|i| i.composition == composition).map(|i| i.place))
    };
    let mut place = match on(d.elements_in(to).unwrap_or(&[])) {
        Some(p) => Place { at: p.at + DVec2::new(10.0, -10.0), ..p },
        None => d.sheets().find_map(|x| on(&x.elements)).unwrap_or_else(|| Place::free(s.paper.size() / 2.0)),
    };
    // A composition has no paper.
    if matches!(to, Container::Composition(_)) {
        place.corner = None;
    }
    let element = ElementKind::Occurrence(skraft_model::drawing::Occurrence { composition, place, props: Default::default() });
    if let Ok(Some(o)) = e.apply(Operation::AddElement { to, element }) {
        st.selected = o.element.into_iter().collect();
    }
}
