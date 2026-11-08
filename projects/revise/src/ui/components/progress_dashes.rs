//! The Dashboard's page indicator: one short dash per page in a column
//! down the right-hand edge, the current page's dash bright and the
//! rest faint ("top / middle / bottom" for three pages). It started
//! life as a continuous scroll-progress bar (feature request: "a
//! progress bar instead of a scroll bar, looking like the attached
//! image but in the theme's style"); with the Dashboard now paged
//! (see `ui::pager`) the position is a page, not a pixel offset.
//!
//! It is *momentary*: the caller passes an opacity that is `1.0`
//! while the person is scrolling or has just changed page and fades
//! to `0.0` afterwards. While it is visible it is also clickable —
//! pressing a dash jumps to that page — and hovering it keeps it up.
//!
//! The arithmetic (how bright each dash is mid-slide, which dash a
//! pointer is on, where the column sits) is pure and unit-tested;
//! only the painting and pointer handling in `show` are not.

use eframe::egui::{self, Color32, CornerRadius, Rect, Sense, Vec2};

use crate::theme::Theme;

/// Width of each dash.
pub const BAR_WIDTH: f32 = 6.0;
/// Height of each dash, and the gap between neighbours.
pub const DASH_HEIGHT: f32 = 26.0;
pub const GAP: f32 = 8.0;
/// Extra reach on each side of the column for pressing it.
const HIT_PADDING: f32 = 6.0;
/// Below this opacity the indicator is invisible and takes no input.
const MIN_INTERACTIVE_OPACITY: f32 = 0.05;

/// How bright (`0.0..=1.0`) each of `count` dashes is when the page
/// position (which animates between whole numbers) is `position`:
/// full on the page itself, fading linearly to nothing one page away,
/// so the highlight slides from dash to dash with the page.
pub fn dash_fills(position: f32, count: usize) -> Vec<f32> {
    (0..count).map(|i| (1.0 - (i as f32 - position).abs()).clamp(0.0, 1.0)).collect()
}

/// The column's total height for `count` dashes.
pub fn column_height(count: usize) -> f32 {
    if count == 0 {
        return 0.0;
    }
    count as f32 * DASH_HEIGHT + (count as f32 - 1.0) * GAP
}

/// The column's rect: `BAR_WIDTH` wide, flush with `right`, vertically
/// centred in `area`.
pub fn column_rect(area: Rect, right: f32, count: usize) -> Rect {
    let height = column_height(count);
    let top = area.center().y - height / 2.0;
    Rect::from_min_size(egui::pos2(right - BAR_WIDTH, top), Vec2::new(BAR_WIDTH, height))
}

/// Which dash a pointer at `pointer_y` is on, for a column starting at
/// `top`. The gaps belong to the dash above them, so every point in
/// the column maps to some page.
pub fn page_at(pointer_y: f32, top: f32, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let stride = DASH_HEIGHT + GAP;
    (((pointer_y - top) / stride).floor().max(0.0) as usize).min(count - 1)
}

/// Draws the indicator for `count` pages at animated `position`, in
/// `column` (see `column_rect`), at `opacity`. Returns the page the
/// person is pressing, if any. (Staying up while hovered is the
/// caller's job — see `is_hovered`.)
pub fn show(
    ui: &mut egui::Ui,
    theme: &Theme,
    column: Rect,
    position: f32,
    count: usize,
    opacity: f32,
) -> Option<usize> {
    if count == 0 || opacity <= MIN_INTERACTIVE_OPACITY {
        return None;
    }
    let p = &theme.palette;

    let hit = column.expand2(Vec2::new(HIT_PADDING, 0.0));
    let response = ui.interact(hit, egui::Id::new("dashboard_page_dashes"), Sense::click_and_drag());
    let mut pressed = None;
    if response.is_pointer_button_down_on() || response.dragged() || response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            pressed = Some(page_at(pos.y, column.top(), count));
        }
    }

    let mut painter = ui.painter_at(hit.expand(1.0));
    painter.set_opacity(opacity);
    let corner = CornerRadius::same(1);
    for (i, fill) in dash_fills(position, count).into_iter().enumerate() {
        let top = column.top() + i as f32 * (DASH_HEIGHT + GAP);
        let dash = Rect::from_min_size(egui::pos2(column.left(), top), Vec2::new(BAR_WIDTH, DASH_HEIGHT));
        // Faint hairline grey for pages you aren't on, the bright text
        // colour for the one you are, blending as the page slides.
        let color: Color32 = p.border.lerp_to_gamma(p.text, fill);
        painter.rect_filled(dash, corner, color);
    }

    pressed
}

/// Whether the pointer is over the indicator's column — lets the
/// caller `touch` the pager so the indicator stays up while hovered.
pub fn is_hovered(ui: &egui::Ui, column: Rect) -> bool {
    ui.input(|i| i.pointer.hover_pos()).is_some_and(|pos| column.expand2(Vec2::new(HIT_PADDING, 0.0)).contains(pos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    #[test]
    fn at_rest_only_the_current_dash_is_lit() {
        assert_eq!(dash_fills(0.0, 3), vec![1.0, 0.0, 0.0]);
        assert_eq!(dash_fills(1.0, 3), vec![0.0, 1.0, 0.0]);
        assert_eq!(dash_fills(2.0, 3), vec![0.0, 0.0, 1.0]);
    }

    #[test]
    fn mid_slide_the_highlight_is_shared_between_neighbours() {
        let fills = dash_fills(0.25, 3);
        assert!((fills[0] - 0.75).abs() < 1e-6);
        assert!((fills[1] - 0.25).abs() < 1e-6);
        assert_eq!(fills[2], 0.0);
        let fills = dash_fills(1.5, 3);
        assert!((fills[1] - 0.5).abs() < 1e-6 && (fills[2] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn dash_fills_handles_no_pages() {
        assert!(dash_fills(0.0, 0).is_empty());
    }

    #[test]
    fn column_height_counts_dashes_and_gaps() {
        assert_eq!(column_height(0), 0.0);
        assert_eq!(column_height(1), DASH_HEIGHT);
        assert_eq!(column_height(3), 3.0 * DASH_HEIGHT + 2.0 * GAP);
    }

    #[test]
    fn the_column_is_centred_and_flush_right() {
        let area = Rect::from_min_max(pos2(0.0, 100.0), pos2(500.0, 500.0));
        let column = column_rect(area, 490.0, 3);
        assert_eq!(column.right(), 490.0);
        assert_eq!(column.width(), BAR_WIDTH);
        assert!((column.center().y - area.center().y).abs() < 1e-4);
        assert!((column.height() - column_height(3)).abs() < 1e-4);
    }

    #[test]
    fn pointers_map_to_dashes_including_the_gaps() {
        let top = 100.0;
        assert_eq!(page_at(top - 50.0, top, 3), 0);
        assert_eq!(page_at(top + 1.0, top, 3), 0);
        // The gap under the first dash still belongs to it.
        assert_eq!(page_at(top + DASH_HEIGHT + 1.0, top, 3), 0);
        assert_eq!(page_at(top + DASH_HEIGHT + GAP + 1.0, top, 3), 1);
        assert_eq!(page_at(top + 2.0 * (DASH_HEIGHT + GAP) + 1.0, top, 3), 2);
        assert_eq!(page_at(top + 9999.0, top, 3), 2);
        assert_eq!(page_at(top, top, 0), 0);
    }
}
