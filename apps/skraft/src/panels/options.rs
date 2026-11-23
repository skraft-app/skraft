//! The tool option bar and the context card (docs/ui/interface.md#tool-option-bar).

use super::*;

/// Whether the context card has anything to show (docs/ui/interface.md#context-card): the
/// selection's scope of a tool that picks parts, or the open part's scope.
pub fn has_context_options(editor: &Editor) -> bool {
    shows_part_scope(editor) || editor.tool_options().iter().any(is_context_option) || shows_open_node(editor)
}

/// A round or a chamfer open, another tool on: the card says it, and how its edges are changed
/// (docs/tools/rounds.md).
fn shows_open_node(editor: &Editor) -> bool {
    !editor.blending() && editor.open_node().is_some()
}

/// Whether the tool's card has anything to show: the active tool's own options, or an open
/// part's « Solide ».
pub fn has_tool_options(editor: &Editor) -> bool {
    shows_body_sign(editor) || editor.tool_options().iter().any(|o| !is_context_option(o))
}

/// An option of the context's, not the tool's: the selection's scope, which the editor adds to
/// every tool that picks parts (docs/tools/selection.md#selection-scope).
fn is_context_option(o: &ToolOption) -> bool {
    o.key == skraft_tools::tools::PICK_SCOPE
}

/// The open part's scope, only for a part placed more than once (docs/model/data-model.md): for one alone,
/// « Celui-ci » and « Tous » are the same.
fn shows_part_scope(editor: &Editor) -> bool {
    (editor.open_part().is_some() && editor.open_part_copies() > 1) || deep_scope(editor)
}

/// Parts selected at any depth through an assembly placed more than once, by a tool that
/// changes them (« Parts », docs/tools/selection.md): « only this one » makes the assemblies along unique.
fn deep_scope(editor: &Editor) -> bool {
    editor.open_part().is_none()
        && matches!(
            editor.tool_kind(),
            ToolKind::Select
                | ToolKind::Move
                | ToolKind::Rotate
                | ToolKind::Erase
                | ToolKind::Paint
                | ToolKind::CopyLine
                | ToolKind::CopyGrid
                | ToolKind::Distribute
        )
        && editor.deep_shared()
}

/// The open part's « Solide », while a shape is drawn in it (docs/model/bodies.md).
fn shows_body_sign(editor: &Editor) -> bool {
    editor.open_part().is_some() && matches!(editor.tool_kind(), ToolKind::Box | ToolKind::Cylinder | ToolKind::Prism | ToolKind::Turn)
}

/// A group's name before its buttons, after a separator but for the first of its card: grey, a
/// caption — its buttons are what the eye looks for.
fn option_group(ui: &mut egui::Ui, first: &mut bool, label: &str, tip: &str) {
    if !std::mem::take(first) {
        ui.separator();
    }
    ui.label(egui::RichText::new(label).color(ui.visuals().weak_text_color())).tip(tip);
}

/// The context card (docs/ui/interface.md#context-card), left of the tool's: what a pick takes
/// (the selection's scope, docs/tools/selection.md), then which copies a change reaches (the
/// open part's scope, docs/model/opening-a-part.md) — the same for every tool that shows them.
pub fn context_options(ui: &mut egui::Ui, editor: &Editor, actions: &mut Vec<Action>) {
    let options = editor.tool_options();
    ui.horizontal(|ui| {
        // The row as high as its buttons from the start: the first label is centered on them.
        ui.set_min_height(OPTION_HEIGHT);
        let mut first = true;
        for o in options.iter().filter(|o| is_context_option(o)) {
            choice_option(ui, &mut first, o, actions);
        }
        // The node open: its name and size, its edges picked again from here.
        if shows_open_node(editor)
            && let Some((node, name, op)) = editor.open_node()
        {
            option_group(ui, &mut first, &name, &t!("option_bar.node_open_tip"));
            let size = editor.document().settings().units.format(op.size);
            let size = match op.kind {
                skraft_model::kernel::names::Blend::Round => t!("panel.outliner.round_size", size = size),
                skraft_model::kernel::names::Blend::Chamfer => t!("panel.outliner.chamfer_size", size = size),
            };
            ui.label(size);
            if ui.button(t!("option_bar.node_edit")).tip(t!("option_bar.node_edit_tip")).clicked() {
                actions.push(Action::EditBlend(node));
            }
        }
        if shows_part_scope(editor) {
            option_group(ui, &mut first, &t!("tool.boolean.option.scope"), &t!("option_bar.part_scope_tip"));
            let all = if deep_scope(editor) {
                t!("tool.boolean.option.all")
            } else {
                t!("tool.boolean.option.all_count", count = editor.open_part_copies())
            };
            let scope = editor.part_scope();
            let scopes = [
                (Scope::All, Segment::new(&all, &t!("tool.boolean.option.all_tip"), Some("scope_all"), scope == Scope::All)),
                (
                    Scope::This,
                    Segment::new(
                        &t!("tool.boolean.option.this"),
                        &t!("tool.boolean.option.this_tip"),
                        Some("scope_this"),
                        scope == Scope::This,
                    ),
                ),
            ];
            let (values, segments): (Vec<_>, Vec<_>) = scopes.into_iter().unzip();
            if let Some(i) = segmented(ui, egui::Id::unique("option_bar_part_scope"), &segments, |i| {
                ["option:part_scope:all", "option:part_scope:this"][i].into()
            }) {
                actions.push(Action::PartScope(values[i]));
            }
        }
    });
}

/// The active tool's card (docs/ui/interface.md): its choices, each a segmented control after
/// its name, then its lone toggles together in an "Options" group, all drawn like the ribbon's
/// buttons. An open part puts its « Solide » first (docs/model/bodies.md).
pub fn tool_options(ui: &mut egui::Ui, editor: &Editor, actions: &mut Vec<Action>) {
    let options: Vec<_> = editor.tool_options().into_iter().filter(|o| !is_context_option(o)).collect();
    ui.horizontal(|ui| {
        ui.set_min_height(OPTION_HEIGHT);
        let mut first = true;
        if shows_body_sign(editor) {
            option_group(ui, &mut first, &t!("option_bar.body_sign"), &t!("option_bar.body_sign_tip"));
            let sign = editor.body_sign();
            let signs = [
                (
                    Sign::Negative,
                    Segment::new(&t!("option_bar.remove"), &t!("option_bar.remove_tip"), Some("subtract"), sign == Sign::Negative),
                ),
                (Sign::Positive, Segment::new(&t!("option_bar.add"), &t!("option_bar.add_tip"), Some("union"), sign == Sign::Positive)),
                // An intersection (docs/decisions/0045-intersection-a-third-sign.md).
                (
                    Sign::Intersect,
                    Segment::new(&t!("option_bar.intersect"), &t!("option_bar.intersect_tip"), Some("intersect"), sign == Sign::Intersect),
                ),
            ];
            let (values, segments): (Vec<_>, Vec<_>) = signs.into_iter().unzip();
            if let Some(i) = segmented(ui, egui::Id::unique("option_bar_body_sign"), &segments, |i| {
                ["option:body_sign:negative", "option:body_sign:positive", "option:body_sign:intersect"][i].into()
            }) {
                actions.push(Action::BodySign(values[i]));
            }
        }
        for o in &options {
            let OptionValue::Length(v) = o.value else { continue };
            option_group(ui, &mut first, &o.label, &o.tip);
            if let Some(text) = length_option(ui, o.key, &o.tip, v, &editor.document().settings().units) {
                actions.push(Action::ToolLength(o.key, text));
            }
        }
        for o in &options {
            choice_option(ui, &mut first, o, actions);
        }
        let toggles: Vec<_> =
            options.iter().filter_map(|o| if let OptionValue::Toggle(on) = o.value { Some((o, on)) } else { None }).collect();
        if toggles.is_empty() {
            return;
        }
        option_group(ui, &mut first, &t!("option_bar.options"), &t!("option_bar.options_tip"));
        // Side by side like the ribbon's buttons.
        ui.spacing_mut().item_spacing.x = crate::ribbon::BUTTON_GAP;
        for (o, on) in toggles {
            if option_button(ui, &format!("option:{}", o.key), on, &o.label, &o.tip, o.icon, o.swatch) {
                actions.push(Action::ToolOption(o.key, OptionValue::Toggle(!on)));
            }
        }
    });
}

/// A choice option: its name, then its values as a segmented control (nothing for another kind).
fn choice_option(ui: &mut egui::Ui, first: &mut bool, o: &ToolOption, actions: &mut Vec<Action>) {
    let OptionValue::Choice(current) = &o.value else { return };
    option_group(ui, first, &o.label, &o.tip);
    let segments: Vec<_> = o.choices.iter().map(|c| Segment::new(&c.label, &c.tip, c.icon, current == c.value)).collect();
    if let Some(c) =
        segmented(ui, egui::Id::unique(("option_bar", o.key)), &segments, |i| format!("option:{}:{}", o.key, o.choices[i].value))
    {
        actions.push(Action::ToolOption(o.key, OptionValue::Choice(o.choices[c].value.to_owned())));
    }
}

/// A length option's value, after its group's name: a [`framed_field`], lit when the value is
/// not zero. Esc or a click away puts the value back. The text typed, when applied.
fn length_option(ui: &mut egui::Ui, key: &str, tip: &str, value: f64, units: &Units) -> Option<String> {
    let shown = units.format(value);
    let exact = format!("{}{}", units.exact_number(value), units.suffix());
    let field = Field {
        hint: None,
        tip: Some(tip),
        width: 80.0,
        align: egui::Align2::RIGHT_CENTER,
        lit: value != 0.0,
        addon: None,
        blur_applies: false,
        empty_applies: false,
        handle: Some(format!("option:{key}")),
    };
    framed_field(ui, egui::Id::unique(("length_option", key)), &shown, &exact, field)
}
