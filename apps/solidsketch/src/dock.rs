//! The side panels' docking (docs/ARCHITECTURE.md §6 bis): two columns, one per edge of the view,
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
    Outliner,
}

impl PanelId {
    pub const ALL: [PanelId; 3] = [PanelId::EntityInfo, PanelId::Materials, PanelId::Outliner];

    /// Its command in the View menu, which shows or closes it.
    pub fn command(self) -> &'static str {
        match self {
            PanelId::EntityInfo => "view.entity_info",
            PanelId::Materials => "view.materials",
            PanelId::Outliner => "view.outliner",
        }
    }

    pub fn from_command(id: &str) -> Option<PanelId> {
        PanelId::ALL.into_iter().find(|p| p.command() == id)
    }

    /// It may fill the height left in its column: a list, which shows more rows. The entity
    /// info keeps the height it is given.
    pub fn fills(self) -> bool {
        !matches!(self, PanelId::EntityInfo)
    }

    fn default_height(self) -> f32 {
        match self {
            PanelId::EntityInfo => 280.0,
            PanelId::Materials => 180.0,
            PanelId::Outliner => 300.0,
        }
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
        let cards = PanelId::ALL.map(|id| Card { id, height: id.default_height(), shown: true, folded: false }).to_vec();
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

    /// Every panel exactly once (preferences written by another build may lack or repeat one).
    pub fn normalize(&mut self) {
        let mut seen = Vec::new();
        for d in [Dock::Left, Dock::Right] {
            self.column_mut(d).cards.retain(|c| {
                let new = !seen.contains(&c.id);
                seen.push(c.id);
                new
            });
        }
        for id in PanelId::ALL {
            if !seen.contains(&id) {
                self.left.cards.push(Card { id, height: id.default_height(), shown: true, folded: false });
            }
        }
    }

    /// The column has a card to show (`skip`: the one being dragged).
    pub fn occupied(&self, dock: Dock, skip: Option<PanelId>) -> bool {
        self.column(dock).cards.iter().any(|c| c.shown && Some(c.id) != skip)
    }
}

/// The cards of `column` shown (but `skip`), stacked from `top` within `room` in height.
pub fn place(column: &Column, top: f32, room: f32, m: Metrics, skip: Option<PanelId>) -> Vec<Placed> {
    let cards: Vec<&Card> = column.cards.iter().filter(|c| c.shown && Some(c.id) != skip).collect();
    if cards.is_empty() {
        return Vec::new();
    }
    let filler = cards.iter().rposition(|c| !c.folded && c.id.fills());
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
    fn the_last_unfolded_card_fills_the_column() {
        let col = Column { width: 270.0, cards: vec![card(PanelId::EntityInfo, 200.0), card(PanelId::Outliner, 100.0)] };
        let p = place(&col, 10.0, 600.0, M, None);
        assert_eq!(p[0], Placed { id: PanelId::EntityInfo, top: 10.0, height: 200.0, fills: false });
        assert_eq!(p[1], Placed { id: PanelId::Outliner, top: 218.0, height: 392.0, fills: true });
    }

    #[test]
    fn folded_cards_below_the_filler_stay_at_the_bottom() {
        let mut last = card(PanelId::Materials, 150.0);
        last.folded = true;
        let col = Column { width: 270.0, cards: vec![card(PanelId::EntityInfo, 200.0), card(PanelId::Outliner, 100.0), last] };
        let p = place(&col, 0.0, 600.0, M, None);
        assert!(p[1].fills);
        assert_eq!(p[2].height, 30.0);
        assert_eq!(p[2].top + p[2].height, 600.0);
    }

    #[test]
    fn a_short_column_shrinks_the_cards_above_the_filler() {
        let col = Column {
            width: 270.0,
            cards: vec![card(PanelId::EntityInfo, 300.0), card(PanelId::Materials, 300.0), card(PanelId::Outliner, 100.0)],
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
        let col = Column { width: 270.0, cards: vec![hidden, card(PanelId::Materials, 200.0), card(PanelId::Outliner, 100.0)] };
        let p = place(&col, 0.0, 600.0, M, Some(PanelId::Outliner));
        assert_eq!(p.len(), 1);
        assert!(p[0].fills);
        assert_eq!(p[0].height, 600.0);
    }

    #[test]
    fn the_entity_info_never_fills() {
        let col = Column { width: 270.0, cards: vec![card(PanelId::Outliner, 100.0), card(PanelId::EntityInfo, 200.0)] };
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
        l.move_card(PanelId::Outliner, Dock::Right, Some(PanelId::Materials));
        assert_eq!(l.right.cards.iter().map(|c| c.id).collect::<Vec<_>>(), [PanelId::Outliner, PanelId::Materials]);
        assert_eq!(l.left.cards.iter().map(|c| c.id).collect::<Vec<_>>(), [PanelId::EntityInfo]);
        // Within its column, above another one.
        l.move_card(PanelId::Materials, Dock::Right, Some(PanelId::Outliner));
        assert_eq!(l.right.cards.iter().map(|c| c.id).collect::<Vec<_>>(), [PanelId::Materials, PanelId::Outliner]);
    }

    #[test]
    fn normalize_keeps_every_panel_once() {
        let mut l = Layout {
            left: Column { width: 270.0, cards: vec![card(PanelId::Outliner, 1.0)] },
            right: Column { width: 270.0, cards: vec![card(PanelId::Outliner, 2.0)] },
        };
        l.normalize();
        assert_eq!(l.right.cards.len(), 0);
        let ids: Vec<_> = l.left.cards.iter().map(|c| c.id).collect();
        assert_eq!(ids, [PanelId::Outliner, PanelId::EntityInfo, PanelId::Materials]);
    }
}
