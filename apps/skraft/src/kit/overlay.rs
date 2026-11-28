//! What the 2D stages draw on screen over their content, the 3D view's and the drawing sheet's
//! alike (docs/render/drawer.md#screen-overlays): the dashes by what they mean, the selection
//! rectangle, the square grip, the cross, a turned text, the markers of the points inferred;
//! and the leader of the interface's figures (the mouse of the Preferences, « Empreindre »'s
//! tenon).

use skraft_theme::Theme;

use crate::workspaces::common::viewport::color;

/// A dash pattern, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dash {
    pub on: f32,
    pub off: f32,
}

/// What is being selected or marked: the selection rectangle, the zoom window, a marked column.
pub const SELECTION: Dash = Dash { on: 5.0, off: 4.0 };
/// A view's frame, a column without a sheet.
pub const FRAME: Dash = Dash { on: 6.0, off: 4.0 };
/// Where a click would place something.
pub const GHOST: Dash = Dash { on: 4.0, off: 4.0 };
/// An alignment followed, a circle's radius being drawn.
pub const ALIGNMENT: Dash = Dash { on: 3.0, off: 3.0 };
/// The line between two tracked points.
pub const TRACKED: Dash = Dash { on: 4.0, off: 3.0 };
/// The box around the composition being edited.
pub const LEVEL: Dash = Dash { on: 5.0, off: 3.0 };

/// A drawing's leader from a part of a figure to its text: a dot at `anchor`, a slant, then a
/// shelf `shelf` long to `end` (its text's start), level with it.
pub fn leader(painter: &egui::Painter, anchor: egui::Pos2, end: egui::Pos2, shelf: f32, stroke: egui::Stroke) {
    let back = if end.x >= anchor.x { -shelf } else { shelf };
    painter.add(egui::Shape::line(vec![anchor, egui::pos2(end.x + back, end.y), end], stroke));
    painter.circle_filled(anchor, 2.0, stroke.color);
}

/// The polyline `path`, dashed.
pub fn dashed(painter: &egui::Painter, path: &[egui::Pos2], stroke: egui::Stroke, dash: Dash) {
    painter.extend(egui::Shape::dashed_line(path, stroke, dash.on, dash.off));
}

/// The outline of `r`, dashed.
pub fn dashed_rect(painter: &egui::Painter, r: egui::Rect, stroke: egui::Stroke, dash: Dash) {
    dashed(painter, &[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()], stroke, dash);
}

/// The outline of the four corners `q`, dashed.
pub fn dashed_quad(painter: &egui::Painter, q: [egui::Pos2; 4], stroke: egui::Stroke, dash: Dash) {
    dashed(painter, &[q[0], q[1], q[2], q[3], q[0]], stroke, dash);
}

/// The selection rectangle: solid when it takes what it encloses (drawn left to right), dashed
/// when it takes what it touches (`crossing`, right to left), as in SketchUp.
pub fn marquee(painter: &egui::Painter, r: egui::Rect, crossing: bool, theme: &Theme) {
    painter.rect_filled(r, 0.0, color(theme.marquee_fill));
    let stroke = egui::Stroke::new(1.0, color(theme.marquee_stroke));
    if crossing {
        dashed_rect(painter, r, stroke, SELECTION);
    } else {
        painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
    }
}

/// A square grip `side` points wide, centred on `at`.
pub fn square_grip(painter: &egui::Painter, at: egui::Pos2, side: f32, fill: egui::Color32, stroke: egui::Stroke) {
    painter.rect(egui::Rect::from_center_size(at, egui::Vec2::splat(side)), 0.0, fill, stroke, egui::StrokeKind::Middle);
}

/// The marker of a point inferred (docs/tools/inferences.md#visual-feedback): its shape tells
/// what the point is, its colour the kind or the line; violet is kept for the temporary points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Marker {
    /// A point of the geometry (an endpoint, a midpoint): a disc.
    Point(egui::Color32),
    /// A third of an edge: a triangle.
    Third(egui::Color32),
    /// A centre: a disc with a small cross.
    Center(egui::Color32),
    /// The origin: a smaller disc on a wider cross.
    Origin(egui::Color32),
    /// A corner of the box of what the tool takes: a square.
    Corner(egui::Color32),
    /// A line across an edge or a guide: the edge's square, in the line's colour.
    Crossing(egui::Color32),
    /// Two lines crossing: a disc in the first's colour (`.0`) ringed with the second's (`.1`).
    TwoLines(egui::Color32, egui::Color32),
    /// Where edges or faces of the solids cross, a line through a face: an ×.
    Intersection(egui::Color32),
    /// On an edge: a smaller square.
    OnEdge(egui::Color32),
    /// On a face: a diamond.
    OnFace(egui::Color32),
    /// On a line or a guide circle: a small disc, in its colour.
    OnLine(egui::Color32),
    /// A tracked point: a violet ring, wide enough to surround the marker of the point there.
    Tracked,
    /// The middle of the tracked points: a small violet dot, ringed when aimed at.
    TrackedMiddle { aimed: bool },
}

/// The marker `m` on `at`, outlined in the theme's ink for markers.
pub fn marker(painter: &egui::Painter, at: egui::Pos2, m: Marker, theme: &Theme) {
    use egui::{Stroke, vec2};
    let outline = Stroke::new(1.0, color(theme.marker_outline));
    // Violet on a light edge: readable over edges and dark faces.
    let ring = |radius: f32| {
        painter.circle_stroke(at, radius, Stroke::new(3.0, color(theme.tracked_halo)));
        painter.circle_stroke(at, radius, Stroke::new(1.5, color(theme.tracked)));
    };
    match m {
        Marker::Point(c) => {
            painter.circle(at, 5.0, c, outline);
        }
        Marker::Third(c) => {
            let pts = vec![at + vec2(0.0, -6.0), at + vec2(5.5, 4.0), at + vec2(-5.5, 4.0)];
            painter.add(egui::Shape::convex_polygon(pts, c, outline));
        }
        Marker::Center(c) => {
            painter.circle(at, 5.0, c, outline);
            cross(painter, at, 3.0, outline);
        }
        Marker::Origin(c) => {
            painter.circle(at, 4.0, c, outline);
            cross(painter, at, 8.0, outline);
        }
        Marker::Corner(c) | Marker::Crossing(c) => square_grip(painter, at, 9.0, c, outline),
        Marker::TwoLines(first, second) => {
            painter.circle(at, 6.0, second, outline);
            painter.circle(at, 3.0, first, Stroke::NONE);
        }
        Marker::Intersection(c) => x_mark(painter, at, 5.0, 2.0, c, outline),
        Marker::OnEdge(c) => square_grip(painter, at, 8.0, c, outline),
        Marker::OnFace(c) => {
            let d = 5.0;
            let pts = vec![at + vec2(0.0, -d), at + vec2(d, 0.0), at + vec2(0.0, d), at + vec2(-d, 0.0)];
            painter.add(egui::Shape::convex_polygon(pts, c, outline));
        }
        Marker::OnLine(c) => {
            painter.circle(at, 4.0, c, outline);
        }
        Marker::Tracked => ring(7.0),
        Marker::TrackedMiddle { aimed } => {
            if aimed {
                ring(7.0);
            }
            painter.circle(at, 2.5, color(theme.tracked), Stroke::new(1.0, color(theme.tracked_halo)));
        }
    }
}

/// An `×` on `at`, each arm `arm` points long: `fill` lines `width` wide over a wider
/// `outline`, readable on any background.
fn x_mark(painter: &egui::Painter, at: egui::Pos2, arm: f32, width: f32, fill: egui::Color32, outline: egui::Stroke) {
    let d = arm / std::f32::consts::SQRT_2;
    let arms = [[at + egui::vec2(-d, -d), at + egui::vec2(d, d)], [at + egui::vec2(-d, d), at + egui::vec2(d, -d)]];
    for a in arms {
        painter.line_segment(a, egui::Stroke::new(width + 2.0 * outline.width, outline.color));
    }
    for a in arms {
        painter.line_segment(a, egui::Stroke::new(width, fill));
    }
}

/// A `+` on `at`, each arm `arm` points long.
pub fn cross(painter: &egui::Painter, at: egui::Pos2, arm: f32, stroke: egui::Stroke) {
    painter.line_segment([at - egui::vec2(arm, 0.0), at + egui::vec2(arm, 0.0)], stroke);
    painter.line_segment([at - egui::vec2(0.0, arm), at + egui::vec2(0.0, arm)], stroke);
}

/// `text` from its bottom left corner `at`, turned about it by `angle` (on screen); what it
/// covers on screen, and its size before it is turned.
pub fn turned_text(
    painter: &egui::Painter,
    at: egui::Pos2,
    angle: f32,
    text: String,
    font: egui::FontId,
    color: egui::Color32,
) -> (egui::Rect, egui::Vec2) {
    if angle == 0.0 {
        let r = painter.text(at, egui::Align2::LEFT_BOTTOM, text, font, color);
        return (r, r.size());
    }
    let galley = painter.layout_no_wrap(text, font, color);
    let size = galley.size();
    let rot = egui::emath::Rot2::from_angle(angle);
    let top_left = at + rot * egui::vec2(0.0, -size.y);
    let corners = [top_left, top_left + rot * egui::vec2(size.x, 0.0), top_left + rot * size, top_left + rot * egui::vec2(0.0, size.y)];
    painter.add(egui::epaint::TextShape::new(top_left, galley, color).with_angle(angle));
    (egui::Rect::from_points(&corners), size)
}
