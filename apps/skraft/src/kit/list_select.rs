//! Selecting in a list or a tree as the system does (docs/ui/interface.md#selecting-in-a-list): a
//! click takes its row alone, Cmd-click (Ctrl elsewhere) adds or takes it off, Shift-click takes
//! the rows from the anchor to it, Cmd+Shift-click adds them; ↑ ↓ move the selection, Shift
//! extending it, ← → fold and unfold. A range keeps only the rows that can be selected with the
//! anchor (`same`): one level of a tree.
//!
//! The list gives its selectable rows in order, keyed by what they stand for (`K`), and applies
//! the [`Outcome`] itself; this module keeps the anchor and the cursor ([`Marks`]) and which list
//! the arrow keys go to (the last one clicked in, until a press elsewhere).

/// What a click asks, from its modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    /// The row alone.
    Only,
    /// The row added, or taken off when it is selected.
    Toggle,
    /// The rows from the anchor to the row, in place of the selection or `add`ed to it.
    Range { add: bool },
}

/// The gesture of a click with the modifiers `m`: Cmd on macOS, Ctrl elsewhere (egui's
/// `command`), toggles; Shift takes a range.
pub fn gesture(m: egui::Modifiers) -> Gesture {
    match (m.shift, m.command) {
        (true, add) => Gesture::Range { add },
        (false, true) => Gesture::Toggle,
        (false, false) => Gesture::Only,
    }
}

/// What the list selects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome<K> {
    Only(K),
    Toggle(K),
    /// These rows (all of one level, never empty), in place of the selection or added to it.
    Rows(Vec<K>, bool),
}

/// An arrow key for the list that has the keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Up {
        extend: bool,
    },
    Down {
        extend: bool,
    },
    /// Folds the cursor's row.
    Left,
    /// Unfolds the cursor's row.
    Right,
}

/// The anchor of a range and the row the arrow keys move from, kept by the list between frames.
#[derive(Clone, Debug)]
pub struct Marks<K> {
    pub anchor: Option<K>,
    pub cursor: Option<K>,
}

impl<K> Default for Marks<K> {
    fn default() -> Self {
        Marks { anchor: None, cursor: None }
    }
}

impl<K: Clone + PartialEq> Marks<K> {
    /// A mark still standing: shown in `rows` and selected; else the first row selected (the
    /// selection made elsewhere, the view's), if any.
    fn valid(mark: &Option<K>, rows: &[K], selected: &impl Fn(&K) -> bool) -> Option<usize> {
        mark.as_ref()
            .and_then(|m| rows.iter().position(|r| r == m))
            .filter(|i| selected(&rows[*i]))
            .or_else(|| rows.iter().position(selected))
    }

    /// The rows from `a` to `b` (either way round) of the level of `a`.
    fn range(rows: &[K], a: usize, b: usize, same: &impl Fn(&K, &K) -> bool) -> Vec<K> {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        rows[lo..=hi].iter().filter(|r| same(&rows[a], r)).cloned().collect()
    }

    /// A click on `key` with `gesture`, `rows` the list's selectable rows in order, `selected`
    /// whether one is, `same` whether two can be selected together.
    pub fn click(
        &mut self,
        rows: &[K],
        key: K,
        gesture: Gesture,
        selected: impl Fn(&K) -> bool,
        same: impl Fn(&K, &K) -> bool,
    ) -> Outcome<K> {
        let to = rows.iter().position(|r| *r == key);
        let anchor = Self::valid(&self.anchor, rows, &selected);
        self.cursor = Some(key.clone());
        match (gesture, anchor, to) {
            (Gesture::Range { add }, Some(a), Some(b)) => {
                self.anchor = Some(rows[a].clone());
                Outcome::Rows(Self::range(rows, a, b, &same), add)
            }
            (Gesture::Toggle, ..) => {
                self.anchor = Some(key.clone());
                Outcome::Toggle(key)
            }
            // A range from nothing: the row alone, as a plain click.
            _ => {
                self.anchor = Some(key.clone());
                Outcome::Only(key)
            }
        }
    }

    /// An arrow `step` up or down: the row next to the cursor alone, or with `extend` the rows
    /// from the anchor to the next one of its level. None at an end, or for ← →: see
    /// [`Marks::fold_target`].
    pub fn step(&mut self, rows: &[K], step: Step, selected: impl Fn(&K) -> bool, same: impl Fn(&K, &K) -> bool) -> Option<Outcome<K>> {
        let (down, extend) = match step {
            Step::Up { extend } => (false, extend),
            Step::Down { extend } => (true, extend),
            Step::Left | Step::Right => return None,
        };
        let Some(cursor) = Self::valid(&self.cursor, rows, &selected) else {
            // Nothing selected in the list: its first row, or its last going up.
            let first = if down { rows.first() } else { rows.last() }?.clone();
            self.anchor = Some(first.clone());
            self.cursor = Some(first.clone());
            return Some(Outcome::Only(first));
        };
        if !extend {
            let next = if down { cursor + 1 } else { cursor.checked_sub(1)? };
            let key = rows.get(next)?.clone();
            self.anchor = Some(key.clone());
            self.cursor = Some(key.clone());
            return Some(Outcome::Only(key));
        }
        let anchor = Self::valid(&self.anchor, rows, &selected).unwrap_or(cursor);
        let mut next = cursor;
        loop {
            next = if down { next + 1 } else { next.checked_sub(1)? };
            if same(&rows[anchor], rows.get(next)?) {
                break;
            }
        }
        self.anchor = Some(rows[anchor].clone());
        self.cursor = Some(rows[next].clone());
        Some(Outcome::Rows(Self::range(rows, anchor, next, &same), false))
    }

    /// The row ← and → fold and unfold: the cursor's.
    pub fn fold_target(&self, rows: &[K], selected: impl Fn(&K) -> bool) -> Option<K> {
        Self::valid(&self.cursor, rows, &selected).map(|i| rows[i].clone())
    }
}

/// Which list the arrow keys go to, and the pass it was last shown.
#[derive(Clone, Copy)]
struct Focus {
    list: egui::Id,
    pass: u64,
}

fn focus_id() -> egui::Id {
    egui::Id::unique("list_select_focus")
}

fn step_id(list: egui::Id) -> egui::Id {
    list.with("list_select_step")
}

/// The list `list` clicked in: the arrow keys go to it.
pub fn take_keys(ctx: &egui::Context, list: egui::Id) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(focus_id(), Focus { list, pass }));
}

/// The list `list`, shown in `rect` this frame: it keeps the keys while it has them, until a
/// press elsewhere. Its arrow key, if one came for it.
pub fn keys(ui: &egui::Ui, list: egui::Id, rect: egui::Rect) -> Option<Step> {
    let ctx = ui.ctx();
    let focus: Option<Focus> = ctx.data(|d| d.get_temp(focus_id()));
    if focus.is_none_or(|f| f.list != list) {
        return None;
    }
    let elsewhere = ui.input(|i| i.pointer.any_pressed() && i.pointer.press_origin().is_some_and(|p| !rect.contains(p)));
    if elsewhere {
        ctx.data_mut(|d| d.remove::<Focus>(focus_id()));
        return None;
    }
    take_keys(ctx, list);
    ctx.data_mut(|d| {
        let step = d.get_temp::<Step>(step_id(list));
        d.remove::<Step>(step_id(list));
        step
    })
}

/// Does a list take the arrow keys (shown at the last frame, clicked in last)? The step `key`
/// with `modifiers` given to it then, if it is one: true when taken.
pub fn give_key(ctx: &egui::Context, key: egui::Key, modifiers: egui::Modifiers) -> bool {
    let Some(focus) = ctx.data(|d| d.get_temp::<Focus>(focus_id())) else { return false };
    if focus.pass + 1 < ctx.cumulative_pass_nr() {
        return false;
    }
    let plain = !modifiers.any();
    let extend = modifiers.shift_only();
    let step = match key {
        egui::Key::ArrowUp if plain || extend => Step::Up { extend },
        egui::Key::ArrowDown if plain || extend => Step::Down { extend },
        egui::Key::ArrowLeft if plain => Step::Left,
        egui::Key::ArrowRight if plain => Step::Right,
        _ => return false,
    };
    ctx.data_mut(|d| d.insert_temp(step_id(focus.list), step));
    true
}

/// In a `show_rows` callback whose rows start at `first`, each `height` high: scrolls the row
/// `index` into view (the one an arrow key moved to, perhaps not laid out).
pub fn reveal(ui: &egui::Ui, first: usize, index: usize, height: f32) {
    let step = height + ui.spacing().item_spacing.y;
    let top = ui.max_rect().top() + (index as f32 - first as f32) * step;
    ui.scroll_to_rect(egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=top + height), None);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rows of two levels: (level, n).
    type K = (u8, u8);
    const ROWS: [K; 6] = [(0, 1), (0, 2), (1, 1), (1, 2), (0, 3), (0, 4)];

    fn same(a: &K, b: &K) -> bool {
        a.0 == b.0
    }

    #[test]
    fn the_modifiers_follow_the_system() {
        let m = |shift, command| egui::Modifiers { shift, command, ..Default::default() };
        assert_eq!(gesture(m(false, false)), Gesture::Only);
        assert_eq!(gesture(m(false, true)), Gesture::Toggle);
        assert_eq!(gesture(m(true, false)), Gesture::Range { add: false });
        assert_eq!(gesture(m(true, true)), Gesture::Range { add: true });
    }

    #[test]
    fn a_range_runs_from_the_anchor_through_its_level_only() {
        let mut marks = Marks::default();
        let sel = |k: &K| *k == (0, 1);
        assert_eq!(marks.click(&ROWS, (0, 1), Gesture::Only, |_| false, same), Outcome::Only((0, 1)));
        let out = marks.click(&ROWS, (0, 4), Gesture::Range { add: false }, sel, same);
        assert_eq!(out, Outcome::Rows(vec![(0, 1), (0, 2), (0, 3), (0, 4)], false));
        // The anchor stays: a range back up.
        let out = marks.click(&ROWS, (0, 2), Gesture::Range { add: true }, sel, same);
        assert_eq!(out, Outcome::Rows(vec![(0, 1), (0, 2)], true));
        assert_eq!(marks.anchor, Some((0, 1)));
    }

    #[test]
    fn a_range_starts_from_the_selection_made_elsewhere() {
        let mut marks = Marks::default();
        let sel = |k: &K| *k == (1, 2);
        let out = marks.click(&ROWS, (1, 1), Gesture::Range { add: false }, sel, same);
        assert_eq!(out, Outcome::Rows(vec![(1, 1), (1, 2)], false));
        // Nothing selected: a plain click.
        let mut marks = Marks::default();
        assert_eq!(marks.click(&ROWS, (0, 3), Gesture::Range { add: false }, |_| false, same), Outcome::Only((0, 3)));
    }

    #[test]
    fn the_arrows_move_then_extend_over_the_anchors_level() {
        let mut marks = Marks::default();
        // Nothing selected: the first row.
        assert_eq!(marks.step(&ROWS, Step::Down { extend: false }, |_| false, same), Some(Outcome::Only((0, 1))));
        let sel = |k: &K| *k == (0, 2);
        marks.cursor = Some((0, 2));
        marks.anchor = Some((0, 2));
        assert_eq!(marks.step(&ROWS, Step::Down { extend: false }, sel, same), Some(Outcome::Only((1, 1))));
        // Extending skips the rows of another level.
        marks.cursor = Some((0, 2));
        marks.anchor = Some((0, 2));
        assert_eq!(marks.step(&ROWS, Step::Down { extend: true }, sel, same), Some(Outcome::Rows(vec![(0, 2), (0, 3)], false)));
        assert_eq!(marks.cursor, Some((0, 3)));
        let sel = |k: &K| matches!(k, (0, 2) | (0, 3));
        assert_eq!(marks.step(&ROWS, Step::Up { extend: true }, sel, same), Some(Outcome::Rows(vec![(0, 2)], false)));
        // At the top: nothing.
        let sel = |k: &K| *k == (0, 1);
        marks.cursor = Some((0, 1));
        assert_eq!(marks.step(&ROWS, Step::Up { extend: false }, sel, same), None);
    }
}
