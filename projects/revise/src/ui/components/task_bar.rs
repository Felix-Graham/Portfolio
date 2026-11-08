use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Sense, Shape, Stroke};

use crate::tasks::time_bar::{self, TaskProgress};
use crate::theme::{Border, Theme};

/// Feature request: task "bars". The bar's own size, inside its border.
pub const BAR_WIDTH: f32 = 56.0;
pub const BAR_HEIGHT: f32 = 10.0;
/// An overfill is at least this wide, so a task only a minute over still
/// visibly breaks out.
const MIN_OVERFLOW_PX: f32 = 3.0;
const DASH: f32 = 2.0;
const GAP: f32 = 2.0;

/// Where the parts of a bar go, for a given progress. Pure geometry so it
/// can be tested without drawing anything.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarGeometry {
    /// The bordered rectangle.
    pub track: Rect,
    /// The coloured fill: inside the border, plus any overfill past it.
    pub fill: Rect,
    /// Whether the fill has broken out of the border.
    pub overfilled: bool,
    /// Everything the bar occupies (track plus overfill).
    pub bounds: Rect,
}

pub fn geometry(top_left: Pos2, progress: TaskProgress) -> BarGeometry {
    let f = time_bar::fill(progress.ratio());
    let overfilled = progress.is_over();
    let overflow = if overfilled { (f.overflow * BAR_WIDTH).max(MIN_OVERFLOW_PX) } else { 0.0 };
    let track = Rect::from_min_size(top_left, egui::vec2(BAR_WIDTH, BAR_HEIGHT));
    let fill = Rect::from_min_size(top_left, egui::vec2(f.inside * BAR_WIDTH + overflow, BAR_HEIGHT));
    BarGeometry { track, fill, overfilled, bounds: Rect::from_min_size(top_left, egui::vec2(BAR_WIDTH + overflow, BAR_HEIGHT)) }
}

/// Draws a task's bar and returns its response (hover for the numbers).
///
/// An empty bar is a red-tinted rectangle; as time is spent it fills left
/// to right, the colour moving red → orange → green. Once the budget is
/// exceeded the fill breaks out past the border — which turns dashed on
/// that side — and the colour carries on green → blue → light blue.
pub fn task_bar(ui: &mut egui::Ui, theme: &Theme, progress: TaskProgress) -> egui::Response {
    let (reserved, response) = ui.allocate_exact_size(geometry(Pos2::ZERO, progress).bounds.size(), Sense::hover());
    let g = geometry(reserved.min, progress);
    let [r, gr, b] = time_bar::color_at(progress.ratio());
    let color = Color32::from_rgb(r, gr, b);
    let outline = Stroke::new(Border::HAIRLINE, theme.palette.border_strong);
    let painter = ui.painter();
    let corner = CornerRadius::same(2);

    painter.rect_filled(g.track, corner, color.gamma_multiply(0.2));
    if progress.spent > 0 {
        painter.rect_filled(g.fill, corner, color);
    }

    if g.overfilled {
        // Open on the right: the fill pokes through where the border would be,
        // and that edge is dashed to show it.
        let t = g.track;
        painter.line_segment([t.right_top(), t.left_top()], outline);
        painter.line_segment([t.left_top(), t.left_bottom()], outline);
        painter.line_segment([t.left_bottom(), t.right_bottom()], outline);
        painter.extend(Shape::dashed_line(&[t.right_top(), t.right_bottom()], outline, DASH, GAP));
    } else {
        painter.rect_stroke(g.track, corner, outline, egui::StrokeKind::Inside);
    }

    response.on_hover_text(progress.describe())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_bar_has_no_fill_width_and_no_overfill() {
        let g = geometry(Pos2::ZERO, TaskProgress::new(0, 60));
        assert_eq!(g.fill.width(), 0.0);
        assert!(!g.overfilled);
        assert_eq!(g.bounds, g.track);
    }

    #[test]
    fn the_fill_grows_with_the_share_of_the_budget_spent() {
        let g = geometry(Pos2::ZERO, TaskProgress::new(30, 60));
        assert_eq!(g.fill.width(), BAR_WIDTH / 2.0);
        assert_eq!(g.fill.height(), BAR_HEIGHT);
    }

    #[test]
    fn exactly_on_budget_fills_the_bar_without_breaking_out() {
        let g = geometry(Pos2::ZERO, TaskProgress::new(60, 60));
        assert_eq!(g.fill, g.track);
        assert!(!g.overfilled);
    }

    #[test]
    fn overfill_extends_past_the_border_and_makes_room_for_itself() {
        let g = geometry(Pos2::ZERO, TaskProgress::new(90, 60));
        assert!(g.overfilled);
        assert_eq!(g.fill.right(), BAR_WIDTH + BAR_WIDTH * 0.5);
        assert!(g.fill.right() > g.track.right());
        assert_eq!(g.bounds.right(), g.fill.right(), "the row reserves the space the fill takes");
    }

    #[test]
    fn a_task_one_minute_over_still_visibly_breaks_out() {
        let g = geometry(Pos2::ZERO, TaskProgress::new(61, 60));
        assert!(g.fill.right() - g.track.right() >= MIN_OVERFLOW_PX);
    }

    #[test]
    fn the_overfill_stops_growing_at_one_extra_bar_width() {
        let g = geometry(Pos2::ZERO, TaskProgress::new(6000, 60));
        assert_eq!(g.fill.right(), BAR_WIDTH * 2.0);
    }
}
