//! Feature request: make each Dashboard section (today / focus /
//! stats) a full-screen page — neighbours never visible, a scroll
//! gesture snaps to the next page, items arranged to fill the page,
//! and a momentary side indicator showing which page you are on.
//!
//! This module is the pager's brain, in the project's usual shape:
//! plain functions and a small state struct over plain numbers (time
//! is passed in, never read), so every decision is unit-tested; the
//! drawing and pointer wiring live in `ui::dashboard` and
//! `ui::components::progress_dashes`.
//!
//! ## The gesture problem
//!
//! A mouse wheel sends discrete notches; a trackpad sends a long,
//! decaying tail of tiny deltas (inertia). Naively changing page
//! whenever the accumulated delta passes a threshold would skip
//! several pages per flick. `Pager::feed` therefore treats one
//! continuous gesture as one page change: after a change, input is
//! ignored for `COOLDOWN` seconds *and* for as long as the gesture
//! keeps arriving (each active frame pushes the ignore window out by
//! `GAP`), so the inertia tail is swallowed and only a fresh gesture
//! (a pause of at least `GAP`) can move again.
//!
//! ## Cells that scroll
//!
//! A page's cells (see `Panel::fill`) scroll internally when their
//! content is taller than the cell. While the pointer is over a cell
//! that can still scroll in the gesture's direction, the cell gets the
//! wheel; only at its edge does the gesture reach the pager — and a
//! gesture that *began* inside a scrolling cell is held off until it
//! pauses (`Pager::hold_off`), so flicking a cell to its end doesn't
//! also flip the page.

use eframe::egui::{vec2, Context, Id, Pos2, Rect};

/// Accumulated scroll (points) that counts as "go to the next page".
/// One mouse-wheel notch is comfortably over this; a gentle trackpad
/// swipe needs a deliberate flick.
pub const SCROLL_THRESHOLD: f32 = 30.0;
/// Deltas smaller than this don't count as scrolling at all.
pub const QUIET: f32 = 0.5;
/// A pause in scrolling at least this long (seconds) ends a gesture.
pub const GAP: f64 = 0.18;
/// Minimum time (seconds) after a page change before another can start.
pub const COOLDOWN: f64 = 0.40;
/// How long the slide between pages takes (seconds).
pub const SLIDE_SECONDS: f32 = 0.28;
/// The indicator stays fully visible this long after the last
/// activity, then fades out over `INDICATOR_FADE`.
pub const INDICATOR_HOLD: f64 = 0.9;
pub const INDICATOR_FADE: f64 = 0.5;

/// Which page is showing, plus the little bit of gesture bookkeeping
/// `feed` needs. `page` is the *target*; the on-screen position
/// animates toward it separately (see `ui::dashboard`).
#[derive(Debug, Clone)]
pub struct Pager {
    pub page: usize,
    acc: f32,
    last_active: f64,
    ignore_until: f64,
    last_activity: f64,
}

impl Default for Pager {
    fn default() -> Self {
        Self {
            page: 0,
            acc: 0.0,
            last_active: f64::NEG_INFINITY,
            ignore_until: f64::NEG_INFINITY,
            // 0.0 (app start) rather than "never": the indicator is
            // visible briefly on the first frames, which tells a new
            // user there is more than one page.
            last_activity: 0.0,
        }
    }
}

impl Pager {
    /// Feeds one frame's vertical scroll delta (egui convention:
    /// negative = scrolling down = toward the next page). Returns
    /// `true` if the page changed.
    pub fn feed(&mut self, delta_y: f32, now: f64, page_count: usize) -> bool {
        self.page = self.page.min(page_count.saturating_sub(1));
        let active = delta_y.abs() > QUIET;
        if active {
            self.last_activity = now;
        }

        if now < self.ignore_until {
            if active {
                self.ignore_until = self.ignore_until.max(now + GAP);
            }
            self.acc = 0.0;
            return false;
        }

        if now - self.last_active > GAP {
            self.acc = 0.0;
        }
        if !active {
            return false;
        }
        self.last_active = now;
        self.acc += delta_y;

        let Some(direction) = direction_for(self.acc) else {
            return false;
        };
        self.acc = 0.0;
        let target = clamped_step(self.page, direction, page_count);
        if target == self.page {
            // Already at the first/last page: nothing to do, and no
            // cooldown either, so scrolling back works immediately.
            return false;
        }
        self.page = target;
        self.ignore_until = now + COOLDOWN;
        true
    }

    /// Ignores the current gesture (and its inertia tail) — called
    /// while an inner cell is the one being scrolled.
    pub fn hold_off(&mut self, now: f64) {
        self.acc = 0.0;
        self.last_activity = now;
        self.ignore_until = self.ignore_until.max(now + GAP);
    }

    /// Moves one page in `direction` (`1` next, `-1` previous), for
    /// keyboard shortcuts. Returns `true` if the page changed.
    pub fn step(&mut self, direction: i32, now: f64, page_count: usize) -> bool {
        let target = clamped_step(self.page, direction, page_count);
        self.touch(now);
        let changed = target != self.page;
        self.page = target;
        self.acc = 0.0;
        if changed {
            self.ignore_until = now + COOLDOWN;
        }
        changed
    }

    /// Jumps straight to `page` (clicking the indicator). Returns
    /// `true` if the page changed.
    pub fn go_to(&mut self, page: usize, now: f64, page_count: usize) -> bool {
        let target = page.min(page_count.saturating_sub(1));
        self.touch(now);
        let changed = target != self.page;
        self.page = target;
        self.acc = 0.0;
        changed
    }

    /// Records activity that should keep the indicator visible.
    pub fn touch(&mut self, now: f64) {
        self.last_activity = now;
    }

    /// The indicator's opacity right now, `0.0..=1.0`.
    pub fn indicator_opacity(&self, now: f64) -> f32 {
        fade_opacity(now - self.last_activity, INDICATOR_HOLD, INDICATOR_FADE)
    }

    /// When the UI next needs to repaint purely for the indicator's
    /// sake: `Some(seconds)` from now, `Some(0.0)` while it is
    /// fading (every frame), `None` once it is gone.
    pub fn repaint_after(&self, now: f64) -> Option<f64> {
        let elapsed = now - self.last_activity;
        if elapsed < INDICATOR_HOLD {
            Some((INDICATOR_HOLD - elapsed).max(0.0))
        } else if elapsed < INDICATOR_HOLD + INDICATOR_FADE {
            Some(0.0)
        } else {
            None
        }
    }
}

/// `Some(1)` (next) / `Some(-1)` (previous) once `accumulated` scroll
/// passes the threshold in either direction.
pub fn direction_for(accumulated: f32) -> Option<i32> {
    if accumulated <= -SCROLL_THRESHOLD {
        Some(1)
    } else if accumulated >= SCROLL_THRESHOLD {
        Some(-1)
    } else {
        None
    }
}

/// `page` moved by `direction`, kept inside `0..page_count`.
pub fn clamped_step(page: usize, direction: i32, page_count: usize) -> usize {
    if page_count == 0 {
        return 0;
    }
    (page as i64 + direction as i64).clamp(0, page_count as i64 - 1) as usize
}

/// Which page a position `fraction` (`0.0` top .. `1.0` bottom) of an
/// indicator split into `page_count` equal parts belongs to.
pub fn page_at_fraction(fraction: f32, page_count: usize) -> usize {
    if page_count == 0 {
        return 0;
    }
    ((fraction.clamp(0.0, 1.0) * page_count as f32).floor() as usize).min(page_count - 1)
}

/// Vertical offset of page `index` when the (animated) position is
/// `position` pages down, for pages `page_height` tall.
pub fn slide_offset(index: usize, position: f32, page_height: f32) -> f32 {
    (index as f32 - position) * page_height
}

/// Whether any part of page `index` is on screen at `position`.
pub fn is_visible(index: usize, position: f32) -> bool {
    (index as f32 - position).abs() < 1.0
}

/// Opacity `elapsed` seconds after the last activity: fully visible
/// for `hold`, then a linear fade over `fade`, then gone.
pub fn fade_opacity(elapsed: f64, hold: f64, fade: f64) -> f32 {
    if elapsed <= hold {
        1.0
    } else if elapsed >= hold + fade {
        0.0
    } else {
        (1.0 - (elapsed - hold) / fade) as f32
    }
}

/// Whether a scroll area at `offset` with `content_height` of content
/// in a `viewport_height` window can still scroll `(up, down)`.
pub fn can_scroll(offset: f32, content_height: f32, viewport_height: f32) -> (bool, bool) {
    let range = content_height - viewport_height;
    if range <= 0.5 {
        return (false, false);
    }
    (offset > 0.5, offset < range - 0.5)
}

/// Splits `total` into consecutive `(start, length)` ranges in
/// proportion to `weights`, with `gap` between neighbours. Used to
/// carve a page into rows and columns that fill it exactly.
pub fn split(total: f32, weights: &[f32], gap: f32) -> Vec<(f32, f32)> {
    if weights.is_empty() {
        return Vec::new();
    }
    let gaps = gap * (weights.len() as f32 - 1.0);
    let usable = (total - gaps).max(0.0);
    let weight_sum: f32 = weights.iter().map(|w| w.max(0.0)).sum();
    let mut start = 0.0;
    weights
        .iter()
        .map(|w| {
            let length = if weight_sum > 0.0 { usable * w.max(0.0) / weight_sum } else { usable / weights.len() as f32 };
            let range = (start, length);
            start += length + gap;
            range
        })
        .collect()
}

/// Carves `area` into a grid of rects that fills it exactly: rows and
/// columns in proportion to the weights, `gap` between neighbours.
/// `result[row][column]`.
pub fn grid(area: Rect, row_weights: &[f32], column_weights: &[f32], gap: f32) -> Vec<Vec<Rect>> {
    let rows = split(area.height(), row_weights, gap);
    let columns = split(area.width(), column_weights, gap);
    rows.iter()
        .map(|&(row_start, row_len)| {
            columns
                .iter()
                .map(|&(column_start, column_len)| {
                    Rect::from_min_size(area.min + vec2(column_start, row_start), vec2(column_len, row_len))
                })
                .collect()
        })
        .collect()
}

/// A scroll cell's report of whether it can still scroll, and where.
#[derive(Debug, Clone, Copy)]
pub struct ScrollReport {
    pub rect: Rect,
    pub can_up: bool,
    pub can_down: bool,
}

/// Whether a cell under `pointer` can use a scroll of `delta_y`
/// itself (so the pager should leave the gesture alone).
pub fn inner_handles(reports: &[ScrollReport], pointer: Option<Pos2>, delta_y: f32) -> bool {
    let Some(pointer) = pointer else { return false };
    reports.iter().any(|r| {
        r.rect.contains(pointer) && ((delta_y < 0.0 && r.can_down) || (delta_y > 0.0 && r.can_up))
    })
}

fn reports_id() -> Id {
    Id::new("pager_scroll_reports")
}

/// Called by a filled `Panel` each frame it draws.
pub fn push_report(ctx: &Context, report: ScrollReport) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<ScrollReport>>(reports_id()).push(report));
}

/// Takes (and clears) the reports pushed since the last call — i.e.
/// the previous frame's, when called at the start of a frame.
pub fn take_reports(ctx: &Context) -> Vec<ScrollReport> {
    ctx.data_mut(|d| std::mem::take(d.get_temp_mut_or_default::<Vec<ScrollReport>>(reports_id())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    #[test]
    fn direction_needs_the_threshold_and_follows_the_sign() {
        assert_eq!(direction_for(0.0), None);
        assert_eq!(direction_for(-SCROLL_THRESHOLD + 1.0), None);
        assert_eq!(direction_for(-SCROLL_THRESHOLD), Some(1));
        assert_eq!(direction_for(SCROLL_THRESHOLD), Some(-1));
        assert_eq!(direction_for(500.0), Some(-1));
    }

    #[test]
    fn stepping_is_clamped_to_the_pages() {
        assert_eq!(clamped_step(0, 1, 3), 1);
        assert_eq!(clamped_step(2, 1, 3), 2);
        assert_eq!(clamped_step(0, -1, 3), 0);
        assert_eq!(clamped_step(1, -1, 3), 0);
        assert_eq!(clamped_step(0, 1, 0), 0);
    }

    #[test]
    fn indicator_positions_map_to_pages() {
        assert_eq!(page_at_fraction(0.0, 3), 0);
        assert_eq!(page_at_fraction(0.33, 3), 0);
        assert_eq!(page_at_fraction(0.34, 3), 1);
        assert_eq!(page_at_fraction(0.99, 3), 2);
        assert_eq!(page_at_fraction(1.0, 3), 2);
        assert_eq!(page_at_fraction(-1.0, 3), 0);
        assert_eq!(page_at_fraction(0.5, 0), 0);
    }

    #[test]
    fn pages_slide_by_whole_heights_and_neighbours_are_hidden_at_rest() {
        assert_eq!(slide_offset(0, 0.0, 600.0), 0.0);
        assert_eq!(slide_offset(1, 0.0, 600.0), 600.0);
        assert_eq!(slide_offset(0, 1.0, 600.0), -600.0);
        assert_eq!(slide_offset(1, 0.5, 600.0), 300.0);

        // At rest only the current page is "visible"; mid-slide, two.
        let visible_at = |pos: f32| (0..3).filter(|i| is_visible(*i, pos)).count();
        assert_eq!(visible_at(0.0), 1);
        assert_eq!(visible_at(1.0), 1);
        assert_eq!(visible_at(0.5), 2);
        assert_eq!(visible_at(1.25), 2);
    }

    #[test]
    fn indicator_holds_then_fades_then_disappears() {
        assert_eq!(fade_opacity(-1.0, 1.0, 0.5), 1.0);
        assert_eq!(fade_opacity(0.0, 1.0, 0.5), 1.0);
        assert_eq!(fade_opacity(1.0, 1.0, 0.5), 1.0);
        assert!((fade_opacity(1.25, 1.0, 0.5) - 0.5).abs() < 1e-6);
        assert_eq!(fade_opacity(1.5, 1.0, 0.5), 0.0);
        assert_eq!(fade_opacity(10.0, 1.0, 0.5), 0.0);
    }

    #[test]
    fn scroll_ability_follows_offset_and_overflow() {
        assert_eq!(can_scroll(0.0, 100.0, 200.0), (false, false));
        assert_eq!(can_scroll(0.0, 400.0, 200.0), (false, true));
        assert_eq!(can_scroll(100.0, 400.0, 200.0), (true, true));
        assert_eq!(can_scroll(200.0, 400.0, 200.0), (true, false));
    }

    #[test]
    fn split_fills_the_total_exactly_with_gaps() {
        let parts = split(100.0, &[1.0, 1.0], 10.0);
        assert_eq!(parts, vec![(0.0, 45.0), (55.0, 45.0)]);

        let parts = split(130.0, &[1.0, 3.0], 10.0);
        assert_eq!(parts, vec![(0.0, 30.0), (40.0, 90.0)]);
        let (last_start, last_len) = parts[1];
        assert_eq!(last_start + last_len, 130.0);

        assert!(split(50.0, &[], 4.0).is_empty());
        // Too small for the gaps: lengths clamp to zero, never negative.
        assert!(split(5.0, &[1.0, 1.0], 10.0).iter().all(|(_, len)| *len == 0.0));
        // All-zero weights fall back to an even split.
        assert_eq!(split(20.0, &[0.0, 0.0], 0.0), vec![(0.0, 10.0), (10.0, 10.0)]);
    }

    #[test]
    fn grid_cells_tile_the_area_without_overlap() {
        let area = Rect::from_min_max(pos2(10.0, 20.0), pos2(210.0, 120.0));
        let cells = grid(area, &[1.0, 1.0], &[1.0, 3.0], 10.0);
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].len(), 2);
        // Corners meet the area's corners exactly.
        assert_eq!(cells[0][0].min, area.min);
        assert_eq!(cells[1][1].max, area.max);
        // Columns 1:3 of (200 - 10) wide.
        assert!((cells[0][0].width() - 47.5).abs() < 1e-4);
        assert!((cells[0][1].width() - 142.5).abs() < 1e-4);
        // Gaps are exactly the requested size, both ways.
        assert!((cells[0][1].left() - cells[0][0].right() - 10.0).abs() < 1e-4);
        assert!((cells[1][0].top() - cells[0][0].bottom() - 10.0).abs() < 1e-4);
        // A single full-size cell is the area itself.
        assert_eq!(grid(area, &[1.0], &[1.0], 10.0), vec![vec![area]]);
    }

    #[test]
    fn repaints_are_scheduled_only_while_the_indicator_is_alive() {
        let mut pager = Pager::default();
        pager.touch(10.0);
        let wait = pager.repaint_after(10.2).unwrap();
        assert!((wait - (INDICATOR_HOLD - 0.2)).abs() < 1e-9);
        assert_eq!(pager.repaint_after(10.0 + INDICATOR_HOLD + 0.1), Some(0.0));
        assert_eq!(pager.repaint_after(10.0 + INDICATOR_HOLD + INDICATOR_FADE + 0.1), None);
    }

    #[test]
    fn inner_cells_take_the_gesture_only_when_they_can_use_it() {
        let cell = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0));
        let down_only = [ScrollReport { rect: cell, can_up: false, can_down: true }];
        let inside = Some(pos2(50.0, 50.0));
        let outside = Some(pos2(150.0, 50.0));

        // Scrolling down (negative delta) with room below: the cell's.
        assert!(inner_handles(&down_only, inside, -10.0));
        // Scrolling up with nowhere to go: the pager's.
        assert!(!inner_handles(&down_only, inside, 10.0));
        // Pointer elsewhere, or unknown: the pager's.
        assert!(!inner_handles(&down_only, outside, -10.0));
        assert!(!inner_handles(&down_only, None, -10.0));
        assert!(!inner_handles(&[], inside, -10.0));
    }

    #[test]
    fn a_flick_below_the_threshold_does_nothing_and_a_notch_moves_one_page() {
        let mut pager = Pager::default();
        assert!(!pager.feed(-10.0, 1.0, 3));
        assert_eq!(pager.page, 0);
        assert!(pager.feed(-50.0, 1.01, 3));
        assert_eq!(pager.page, 1);
    }

    #[test]
    fn small_deltas_in_one_gesture_add_up() {
        let mut pager = Pager::default();
        let mut t = 1.0;
        let mut changed = false;
        for _ in 0..10 {
            changed |= pager.feed(-5.0, t, 3);
            t += 0.016;
        }
        assert!(changed);
        assert_eq!(pager.page, 1);
    }

    #[test]
    fn a_trackpad_inertia_tail_moves_only_one_page() {
        let mut pager = Pager::default();
        let mut t = 1.0;
        let mut changes = 0;
        // 2 seconds of steadily decaying deltas, 60 fps.
        let mut delta = -60.0_f32;
        for _ in 0..120 {
            if pager.feed(delta, t, 3) {
                changes += 1;
            }
            delta *= 0.97;
            t += 1.0 / 60.0;
        }
        assert_eq!(changes, 1);
        assert_eq!(pager.page, 1);
    }

    #[test]
    fn a_new_gesture_after_a_pause_moves_again() {
        let mut pager = Pager::default();
        assert!(pager.feed(-60.0, 1.0, 3));
        // Silence well past the cooldown and the gap...
        assert!(!pager.feed(0.0, 2.0, 3));
        // ...then a fresh flick.
        assert!(pager.feed(-60.0, 2.1, 3));
        assert_eq!(pager.page, 2);
    }

    #[test]
    fn scrolling_past_the_ends_stays_put_and_does_not_lock_the_pager() {
        let mut pager = Pager::default();
        assert!(!pager.feed(60.0, 1.0, 3)); // up from the first page
        assert_eq!(pager.page, 0);
        // No cooldown was started, so heading the other way works at once.
        assert!(pager.feed(-60.0, 1.01, 3));
        assert_eq!(pager.page, 1);
    }

    #[test]
    fn a_gesture_held_off_by_an_inner_cell_does_not_flip_the_page() {
        let mut pager = Pager::default();
        let mut t = 1.0;
        // The gesture starts inside a scrolling cell...
        pager.hold_off(t);
        // ...the cell reaches its end, and the same gesture keeps arriving.
        for _ in 0..30 {
            t += 0.016;
            assert!(!pager.feed(-40.0, t, 3));
        }
        assert_eq!(pager.page, 0);
        // After a real pause, a new gesture works.
        t += 0.5;
        assert!(pager.feed(-40.0, t, 3));
    }

    #[test]
    fn keyboard_and_indicator_navigation() {
        let mut pager = Pager::default();
        assert!(pager.step(1, 1.0, 3));
        assert_eq!(pager.page, 1);
        assert!(pager.step(1, 1.1, 3));
        assert!(!pager.step(1, 1.2, 3)); // already last
        assert_eq!(pager.page, 2);
        assert!(pager.step(-1, 1.3, 3));
        assert!(pager.go_to(0, 1.4, 3));
        assert!(!pager.go_to(0, 1.5, 3));
        assert!(pager.go_to(99, 1.6, 3)); // clamped to the last page
        assert_eq!(pager.page, 2);
    }

    #[test]
    fn activity_keeps_the_indicator_visible() {
        let mut pager = Pager::default();
        // At app start it is shown briefly, then fades away.
        assert_eq!(pager.indicator_opacity(0.5), 1.0);
        assert_eq!(pager.indicator_opacity(30.0), 0.0);
        // Scrolling (even when held off) brings it back.
        pager.hold_off(30.0);
        assert_eq!(pager.indicator_opacity(30.5), 1.0);
        assert_eq!(pager.indicator_opacity(40.0), 0.0);
        pager.touch(40.0);
        assert_eq!(pager.indicator_opacity(40.1), 1.0);
    }

    #[test]
    fn a_shrinking_page_count_clamps_the_current_page() {
        let mut pager = Pager::default();
        pager.go_to(2, 1.0, 3);
        assert!(!pager.feed(0.0, 2.0, 2));
        assert_eq!(pager.page, 1);
    }
}
