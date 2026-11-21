//! The drawing's board (docs/ui/drawing.md#the-board, decision 0050): where every sheet lies on
//! the stage, in board millimetres (y up), computed from the tree and the papers, never saved.

use glam::DVec2;
use skraft_model::{Drawing, Id, Paper};

/// The room a column without a sheet stands for.
const EMPTY: Paper = Paper::A4;
/// The gap between two sheets of a column, as a share of the drawing's largest paper side.
const GAP_SHARE: f64 = 0.1;
/// The gap between two columns, wider: the presentations read apart without a rule.
const COLUMN_GAP_SHARE: f64 = 0.3;

/// A sheet on the board: its paper's bottom left corner (the sheet's origin) and size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoardSheet {
    pub id: Id,
    pub origin: DVec2,
    pub size: DVec2,
}

impl BoardSheet {
    /// Its paper's top right corner.
    pub fn max(&self) -> DVec2 {
        self.origin + self.size
    }

    fn contains(&self, p: DVec2) -> bool {
        p.cmpge(self.origin).all() && p.cmple(self.max()).all()
    }
}

/// A presentation's column: its sheets stacked from the board's top (y = 0), centred on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub presentation: Id,
    pub centre: f64,
    /// Its widest sheet's width.
    pub width: f64,
    pub sheets: Vec<BoardSheet>,
}

impl Column {
    /// What it covers: its sheets, or the room it keeps without one.
    pub fn bounds(&self) -> (DVec2, DVec2) {
        let bottom = self.sheets.last().map_or(-EMPTY.height, |s| s.origin.y);
        (DVec2::new(self.centre - self.width / 2.0, bottom), DVec2::new(self.centre + self.width / 2.0, 0.0))
    }
}

/// Every presentation's column, from the left.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Board {
    pub columns: Vec<Column>,
    /// Between two sheets of a column (millimetres).
    pub gap: f64,
    /// Between two columns (millimetres).
    pub column_gap: f64,
}

impl Board {
    /// The board of drawing `d`.
    pub fn of(d: &Drawing) -> Board {
        let largest = d.sheets().map(|s| s.paper.width.max(s.paper.height)).fold(0.0, f64::max);
        let side = if largest > 0.0 { largest } else { EMPTY.width };
        let (gap, column_gap) = (side * GAP_SHARE, side * COLUMN_GAP_SHARE);
        let mut x = 0.0;
        let columns = d
            .presentations
            .iter()
            .map(|p| {
                let width = if p.sheets.is_empty() { EMPTY.width } else { p.sheets.iter().map(|s| s.paper.width).fold(0.0, f64::max) };
                let centre = x + width / 2.0;
                let mut top = 0.0;
                let sheets = p
                    .sheets
                    .iter()
                    .map(|s| {
                        let size = s.paper.size();
                        let origin = DVec2::new(centre - size.x / 2.0, top - size.y);
                        top = origin.y - gap;
                        BoardSheet { id: s.id, origin, size }
                    })
                    .collect();
                x += width + column_gap;
                Column { presentation: p.id, centre, width, sheets }
            })
            .collect();
        Board { columns, gap, column_gap }
    }

    pub fn sheets(&self) -> impl Iterator<Item = &BoardSheet> {
        self.columns.iter().flat_map(|c| c.sheets.iter())
    }

    pub fn sheet(&self, id: Id) -> Option<&BoardSheet> {
        self.sheets().find(|s| s.id == id)
    }

    /// The sheet whose paper holds board point `p`.
    pub fn sheet_at(&self, p: DVec2) -> Option<Id> {
        self.sheets().find(|s| s.contains(p)).map(|s| s.id)
    }

    pub fn column(&self, presentation: Id) -> Option<&Column> {
        self.columns.iter().find(|c| c.presentation == presentation)
    }

    /// What all the columns cover.
    pub fn bounds(&self) -> Option<(DVec2, DVec2)> {
        self.columns.iter().map(Column::bounds).reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
    }
}

#[cfg(test)]
mod tests {
    use skraft_model::{Document, History, Operation};

    use super::*;

    fn sheet(h: &mut History, presentation: Id, paper: Paper) -> Id {
        h.apply(Operation::AddSheet { presentation: Some(presentation), name: None, paper }).unwrap().sheet.unwrap()
    }

    fn presentation(h: &mut History, paper: Option<Paper>) -> (Id, Option<Id>) {
        let o = h.apply(Operation::AddPresentation { name: None, paper }).unwrap();
        (o.presentation.unwrap(), o.sheet)
    }

    #[test]
    fn sheets_stack_centred_in_a_column_per_presentation() {
        let mut h = History::new(Document::new());
        let (p, a3) = presentation(&mut h, Some(Paper::A3));
        let portrait = sheet(&mut h, p, Paper { width: 210.0, height: 297.0 });
        let (q, a4) = presentation(&mut h, Some(Paper::A4));
        let b = Board::of(h.document().drawing());
        assert_eq!(b.gap, 42.0, "a tenth of the largest side");
        assert!((b.column_gap - 126.0).abs() < 1e-9, "three tenths between columns");
        assert_eq!(b.columns.len(), 2);
        let (first, second) = (b.sheet(a3.unwrap()).unwrap(), b.sheet(portrait).unwrap());
        assert_eq!((first.origin, first.max()), (DVec2::new(0.0, -297.0), DVec2::new(420.0, 0.0)));
        // Below it, past the gap, centred on the widest.
        assert_eq!((second.origin, second.max()), (DVec2::new(105.0, -636.0), DVec2::new(315.0, -339.0)));
        // The next column past the widest sheet and the gap, its top on the board's.
        let other = b.sheet(a4.unwrap()).unwrap();
        assert!((b.column(q).unwrap().centre - (546.0 + 148.5)).abs() < 1e-9 && other.origin.distance(DVec2::new(546.0, -210.0)) < 1e-9);
        let (lo, hi) = b.bounds().unwrap();
        assert!(lo.distance(DVec2::new(0.0, -636.0)) < 1e-9 && hi.distance(DVec2::new(843.0, 0.0)) < 1e-9);
    }

    #[test]
    fn a_presentation_without_a_sheet_keeps_its_column() {
        let mut h = History::new(Document::new());
        let (p, _) = presentation(&mut h, None);
        let (_, a3) = presentation(&mut h, Some(Paper::A3));
        let b = Board::of(h.document().drawing());
        assert_eq!(b.column(p).unwrap().bounds(), (DVec2::new(0.0, -210.0), DVec2::new(297.0, 0.0)));
        assert!((b.sheet(a3.unwrap()).unwrap().origin.x - (297.0 + 126.0)).abs() < 1e-9);
    }

    #[test]
    fn a_point_finds_the_sheet_under_it() {
        let mut h = History::new(Document::new());
        let (p, a3) = presentation(&mut h, Some(Paper::A3));
        let a4 = sheet(&mut h, p, Paper::A4);
        let b = Board::of(h.document().drawing());
        assert_eq!(b.sheet_at(DVec2::new(10.0, -10.0)), a3);
        assert_eq!(b.sheet_at(b.sheet(a4).unwrap().origin + DVec2::splat(1.0)), Some(a4));
        assert_eq!(b.sheet_at(DVec2::new(10.0, -310.0)), None, "the gap");
    }
}
