//! The side panels' docking (docs/ui/interface.md): two columns, one per edge of the view,
//! each a stack of cards. A card keeps the height the user gave it, except the last unfolded one
//! of its column that may fill ([`PanelId::fills`]: not the entity info), which fills the height
//! left. Pure layout: the drawing is in `app`.

use serde::{Deserialize, Serialize};

/// Default width of a column, in points.
pub const COLUMN_WIDTH: f32 = 270.0;
/// Narrowest a column can be dragged to, in points.
pub const COLUMN_MIN_WIDTH: f32 = 200.0;

/// A side panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelId {
    EntityInfo,
    Materials,
    /// The navigator: the workspace's tree, the model's or, on a sheet, the drawing's
    /// (docs/ui/drawing.md#navigator). `outliner`: its former name, `layers`: the drawing's former
    /// panel, in layouts already saved.
    #[serde(alias = "outliner", alias = "layers")]
    Navigator,
    /// The library: the workspace's definitions, the document's parts and assemblies
    /// (docs/ui/components-library.md) or, on a sheet, the drawing's compositions
    /// (docs/ui/drawing.md#the-compositions-card). `components`, `compositions`: its two former
    /// cards, in layouts already saved.
    #[serde(alias = "components", alias = "compositions")]
    Library,
}

impl PanelId {
    pub const ALL: [PanelId; 4] = [PanelId::EntityInfo, PanelId::Materials, PanelId::Navigator, PanelId::Library];

    /// Its command (`view.*`), which shows or closes it.
    pub fn command(self) -> &'static str {
        match self {
            PanelId::EntityInfo => "view.entity_info",
            PanelId::Materials => "view.materials",
            PanelId::Navigator => "view.navigator",
            PanelId::Library => "view.library",
        }
    }

    pub fn from_command(id: &str) -> Option<PanelId> {
        PanelId::ALL.into_iter().find(|p| p.command() == id)
    }

    /// It may fill the height left in its column: a list, which shows more rows. The entity
    /// info keeps the height it is given.
    pub fn fills(self) -> bool {
        self != PanelId::EntityInfo
    }

    fn default_height(self) -> f32 {
        match self {
            PanelId::EntityInfo => 280.0,
            PanelId::Navigator => 300.0,
            PanelId::Materials | PanelId::Library => 240.0,
        }
    }

    /// Shown in a first layout, or once added to one written by an older build: the library
    /// waits to be asked for (the drawing shows it at first, [`crate::workspaces::Workspace::panels`]).
    fn shown_by_default(self) -> bool {
        self != PanelId::Library
    }

    /// Its card as a first layout has it.
    fn first_card(self) -> Card {
        Card { id: self, height: self.default_height(), shown: self.shown_by_default(), folded: false }
    }
}

/// An edge of the view a column docks to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dock {
    #[default]
    Left,
    Right,
}

/// A panel in its column.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub id: PanelId,
    /// Its whole height (frame included) when it does not fill its column.
    pub height: f32,
    /// Closed from the View menu: it keeps its place to come back to.
    pub shown: bool,
    /// Only its header shows.
    pub folded: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Column {
    pub width: f32,
    /// From top to bottom.
    pub cards: Vec<Card>,
}

impl Default for Column {
    fn default() -> Self {
        Column { width: COLUMN_WIDTH, cards: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub left: Column,
    pub right: Column,
}

impl Default for Layout {
    fn default() -> Self {
        let cards = PanelId::ALL.map(PanelId::first_card).to_vec();
        Layout { left: Column { width: COLUMN_WIDTH, cards }, right: Column::default() }
    }
}

/// Where a card goes in its column.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub id: PanelId,
    pub top: f32,
    pub height: f32,
    /// It fills the height left (the last unfolded card of its column that may): no bottom edge
    /// to drag.
    pub fills: bool,
}

/// The sizes the placement works with.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// A folded card's height.
    pub header: f32,
    /// The least height an unfolded card keeps.
    pub min_height: f32,
    /// Between two cards.
    pub gap: f32,
}

impl Layout {
    pub fn column(&self, dock: Dock) -> &Column {
        match dock {
            Dock::Left => &self.left,
            Dock::Right => &self.right,
        }
    }

    pub fn column_mut(&mut self, dock: Dock) -> &mut Column {
        match dock {
            Dock::Left => &mut self.left,
            Dock::Right => &mut self.right,
        }
    }

    pub fn card(&self, id: PanelId) -> Option<&Card> {
        self.left.cards.iter().chain(&self.right.cards).find(|c| c.id == id)
    }

    pub fn card_mut(&mut self, id: PanelId) -> Option<&mut Card> {
        self.left.cards.iter_mut().chain(&mut self.right.cards).find(|c| c.id == id)
    }

    pub fn is_shown(&self, id: PanelId) -> bool {
        self.card(id).is_some_and(|c| c.shown)
    }

    pub fn toggle(&mut self, id: PanelId) {
        if let Some(c) = self.card_mut(id) {
            c.shown = !c.shown;
        }
    }

    /// Shows `id`, unfolded. Says whether it was closed or folded.
    pub fn reveal(&mut self, id: PanelId) -> bool {
        let Some(c) = self.card_mut(id) else { return false };
        let hidden = !c.shown || c.folded;
        (c.shown, c.folded) = (true, false);
        hidden
    }

    /// Moves `id` into the column at `dock`, above `before` (at the bottom if `None`).
    pub fn move_card(&mut self, id: PanelId, dock: Dock, before: Option<PanelId>) {
        if before == Some(id) {
            return;
        }
        let Some(card) = [Dock::Left, Dock::Right].into_iter().find_map(|d| {
            let cards = &mut self.column_mut(d).cards;
            cards.iter().position(|c| c.id == id).map(|k| cards.remove(k))
        }) else {
            return;
        };
        let cards = &mut self.column_mut(dock).cards;
        let at = before.and_then(|b| cards.iter().position(|c| c.id == b)).unwrap_or(cards.len());
        cards.insert(at, card);
    }

    /// Every panel exactly once (preferences written by another build may lack or repeat one:
    /// a panel repeated is shown if one of its cards was).
    pub fn normalize(&mut self) {
        self.normalize_with(None);
    }

    /// The same for a workspace showing `shown` at first: a card added shown if it is one.
    pub fn normalize_with(&mut self, shown: Option<&[PanelId]>) {
        let mut seen = Vec::new();
        let mut shown_again = Vec::new();
        for d in [Dock::Left, Dock::Right] {
            self.column_mut(d).cards.retain(|c| {
                let new = !seen.contains(&c.id);
                seen.push(c.id);
                if !new && c.shown {
                    shown_again.push(c.id);
                }
                new
            });
        }
        for id in shown_again {
            if let Some(c) = self.card_mut(id) {
                c.shown = true;
            }
        }
        for id in PanelId::ALL {
            if !seen.contains(&id) {
                let mut card = id.first_card();
                if let Some(shown) = shown {
                    card.shown = shown.contains(&id);
                }
                self.left.cards.push(card);
            }
        }
    }

    /// The column has a card to show (`skip`: the one being dragged).
    pub fn occupied(&self, dock: Dock, skip: Option<PanelId>) -> bool {
        self.column(dock).cards.iter().any(|c| c.shown && Some(c.id) != skip)
    }
}

/// The cards of `column` shown (but `skip`), stacked from `top` within `room` in height.
#[cfg(test)]
pub fn place(column: &Column, top: f32, room: f32, m: Metrics, skip: Option<PanelId>) -> Vec<Placed> {
    place_with(column, top, room, m, skip, PanelId::fills)
}

/// [`place`], `fills` saying which cards may fill.
pub fn place_with(column: &Column, top: f32, room: f32, m: Metrics, skip: Option<PanelId>, fills: fn(PanelId) -> bool) -> Vec<Placed> {
    let cards: Vec<&Card> = column.cards.iter().filter(|c| c.shown && Some(c.id) != skip).collect();
    if cards.is_empty() {
        return Vec::new();
    }
    let filler = cards.iter().rposition(|c| !c.folded && fills(c.id));
    let mut heights: Vec<f32> = cards
        .iter()
        .enumerate()
        .map(|(k, c)| {
            if c.folded {
                m.header
            } else if Some(k) == filler {
                m.min_height
            } else {
                c.height.max(m.min_height)
            }
        })
        .collect();
    let gaps = m.gap * (cards.len() - 1) as f32;
    let used: f32 = heights.iter().sum::<f32>() + gaps;
    if used <= room {
        if let Some(f) = filler {
            heights[f] += room - used;
        }
    } else {
        // Too tall: the other unfolded cards give their height back, the nearest to the filler
        // first (without one, from the bottom up).
        let from = filler.unwrap_or(cards.len());
        let mut givers: Vec<usize> = (0..cards.len()).filter(|&k| !cards[k].folded && Some(k) != filler).collect();
        givers.sort_by_key(|&k| (k as isize - from as isize).abs());
        let mut excess = used - room;
        for k in givers {
            let give = (heights[k] - m.min_height).max(0.0).min(excess);
            heights[k] -= give;
            excess -= give;
        }
    }
    let mut y = top;
    cards
        .iter()
        .zip(heights)
        .enumerate()
        .map(|(k, (c, height))| {
            let placed = Placed { id: c.id, top: y, height, fills: Some(k) == filler };
            y += height + m.gap;
            placed
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: Metrics = Metrics { header: 30.0, min_height: 80.0, gap: 8.0 };

    fn card(id: PanelId, height: f32) -> Card {
        Card { id, height, shown: true, folded: false }
    }

    #[test]
    fn reveal_shows_a_card_unfolded_and_says_whether_it_did() {
        let mut l = Layout::default();
        assert!(!l.reveal(PanelId::Materials), "shown already");
        l.toggle(PanelId::Materials);
        assert!(l.reveal(PanelId::Materials) && l.is_shown(PanelId::Materials));
        l.card_mut(PanelId::Materials).unwrap().folded = true;
        assert!(l.reveal(PanelId::Materials));
        assert!(!l.card(PanelId::Materials).unwrap().folded);
    }

    #[test]
    fn the_last_unfolded_card_fills_the_column() {
        let col = Column { width: 270.0, cards: vec![card(PanelId::EntityInfo, 200.0), card(PanelId::Navigator, 100.0)] };
        let p = place(&col, 10.0, 600.0, M, None);
        assert_eq!(p[0], Placed { id: PanelId::EntityInfo, top: 10.0, height: 200.0, fills: false });
        assert_eq!(p[1], Placed { id: PanelId::Navigator, top: 218.0, height: 392.0, fills: true });
    }

    #[test]
    fn folded_cards_below_the_filler_stay_at_the_bottom() {
        let mut last = card(PanelId::Materials, 150.0);
        last.folded = true;
        let col = Column { width: 270.0, cards: vec![card(PanelId::EntityInfo, 200.0), card(PanelId::Navigator, 100.0), last] };
        let p = place(&col, 0.0, 600.0, M, None);
        assert!(p[1].fills);
        assert_eq!(p[2].height, 30.0);
        assert_eq!(p[2].top + p[2].height, 600.0);
    }

    #[test]
    fn a_short_column_shrinks_the_cards_above_the_filler() {
        let col = Column {
            width: 270.0,
            cards: vec![card(PanelId::EntityInfo, 300.0), card(PanelId::Materials, 300.0), card(PanelId::Navigator, 100.0)],
        };
        let p = place(&col, 0.0, 500.0, M, None);
        // The nearest card above the filler gives first.
        assert_eq!(p[0].height, 300.0);
        assert_eq!(p[1].height, 104.0);
        assert_eq!(p[2].height, 80.0);
    }

    #[test]
    fn hidden_and_dragged_cards_take_no_room() {
        let mut hidden = card(PanelId::EntityInfo, 150.0);
        hidden.shown = false;
        let col = Column { width: 270.0, cards: vec![hidden, card(PanelId::Materials, 200.0), card(PanelId::Navigator, 100.0)] };
        let p = place(&col, 0.0, 600.0, M, Some(PanelId::Navigator));
        assert_eq!(p.len(), 1);
        assert!(p[0].fills);
        assert_eq!(p[0].height, 600.0);
    }

    #[test]
    fn the_entity_info_never_fills() {
        let col = Column { width: 270.0, cards: vec![card(PanelId::Navigator, 100.0), card(PanelId::EntityInfo, 200.0)] };
        let p = place(&col, 0.0, 600.0, M, None);
        assert!(p[0].fills);
        assert_eq!((p[0].height, p[1].height), (392.0, 200.0));
        // Alone: its own height, room left below.
        let alone = Column { width: 270.0, cards: vec![card(PanelId::EntityInfo, 200.0)] };
        assert_eq!(place(&alone, 0.0, 600.0, M, None)[0], Placed { id: PanelId::EntityInfo, top: 0.0, height: 200.0, fills: false });
    }

    #[test]
    fn without_a_filler_a_short_column_shrinks_from_the_bottom() {
        let col = Column { width: 270.0, cards: vec![card(PanelId::EntityInfo, 300.0)] };
        assert_eq!(place(&col, 0.0, 250.0, M, None)[0].height, 250.0);
    }

    #[test]
    fn cards_move_between_columns() {
        let mut l = Layout::default();
        l.move_card(PanelId::Materials, Dock::Right, None);
        assert_eq!(l.right.cards.iter().map(|c| c.id).collect::<Vec<_>>(), [PanelId::Materials]);
        l.move_card(PanelId::Navigator, Dock::Right, Some(PanelId::Materials));
        assert_eq!(l.right.cards.iter().map(|c| c.id).collect::<Vec<_>>(), [PanelId::Navigator, PanelId::Materials]);
        assert_eq!(l.left.cards.iter().map(|c| c.id).collect::<Vec<_>>(), [PanelId::EntityInfo, PanelId::Library]);
        // Within its column, above another one.
        l.move_card(PanelId::Materials, Dock::Right, Some(PanelId::Navigator));
        assert_eq!(l.right.cards.iter().map(|c| c.id).collect::<Vec<_>>(), [PanelId::Materials, PanelId::Navigator]);
    }

    #[test]
    fn normalize_keeps_every_panel_once() {
        let mut l = Layout {
            left: Column { width: 270.0, cards: vec![card(PanelId::Navigator, 1.0)] },
            right: Column { width: 270.0, cards: vec![card(PanelId::Navigator, 2.0)] },
        };
        l.normalize();
        assert_eq!(l.right.cards.len(), 0);
        let ids: Vec<_> = l.left.cards.iter().map(|c| c.id).collect();
        assert_eq!(ids, [PanelId::Navigator, PanelId::EntityInfo, PanelId::Materials, PanelId::Library]);
        // The library added closed, as in a first layout.
        assert!(!l.is_shown(PanelId::Library) && !Layout::default().is_shown(PanelId::Library));
        assert!(l.is_shown(PanelId::EntityInfo));
        // A workspace's own first panels: the navigator added shown in « Mise en plan ».
        let mut l = Layout { left: Column { width: 270.0, cards: vec![card(PanelId::EntityInfo, 1.0)] }, right: Column::default() };
        l.normalize_with(Some(&[PanelId::EntityInfo, PanelId::Navigator]));
        assert!(l.is_shown(PanelId::Navigator) && !l.is_shown(PanelId::Materials));
    }

    #[test]
    fn a_layout_saved_with_the_drawings_former_panel_shows_the_navigator() {
        let saved = r#"{"left":{"width":270.0,"cards":[
            {"id":"entity_info","height":200.0,"shown":true,"folded":false},
            {"id":"outliner","height":300.0,"shown":false,"folded":false},
            {"id":"layers","height":150.0,"shown":true,"folded":false}]},"right":{"width":270.0,"cards":[]}}"#;
        let mut l: Layout = serde_json::from_str(saved).unwrap();
        l.normalize();
        assert_eq!(l.left.cards.iter().filter(|c| c.id == PanelId::Navigator).count(), 1);
        assert!(l.is_shown(PanelId::Navigator));
    }

    #[test]
    fn a_layout_saved_with_the_two_former_libraries_shows_one() {
        let saved = r#"{"left":{"width":270.0,"cards":[
            {"id":"components","height":240.0,"shown":false,"folded":false},
            {"id":"compositions","height":240.0,"shown":true,"folded":false}]},"right":{"width":270.0,"cards":[]}}"#;
        let mut l: Layout = serde_json::from_str(saved).unwrap();
        l.normalize();
        assert_eq!(l.left.cards.iter().filter(|c| c.id == PanelId::Library).count(), 1);
        assert!(l.is_shown(PanelId::Library), "shown, as one of them was");
    }
}
