//! Feature request: a timer picker that "should look like the default
//! iOS Clock app, with a scroll select for hours, minutes, seconds ...
//! a square look with each value in a column of cells for numbers."
//!
//! `show` draws one such wheel: a column of `ROWS` square, hairline-
//! bordered cells holding consecutive numbers, with the selected value
//! in the middle cell (heavier border, brighter, larger) and its
//! neighbours fading away from it. Like iOS, the wheel wraps — scroll
//! past 59 and you're back at 0. Three of them side by side (hours,
//! minutes, seconds) make the picker.
//!
//! It's moved by: the mouse wheel / trackpad while hovering (the
//! scroll is consumed so the Dashboard's own scroll area doesn't also
//! move), dragging the column up or down, or clicking any visible cell
//! to jump to it. The little arithmetic the interaction needs — wrap
//! around, which numbers are visible, turning scrolled pixels into
//! whole steps, which cell was clicked — is pure and unit-tested below
//! rather than only visible on screen.

use eframe::egui::{self, CornerRadius, Sense, Stroke, StrokeKind, Vec2};

use crate::theme::{Border, Theme};

/// Cells visible at once. Odd, so there is a single middle cell for
/// the selected value.
pub const ROWS: usize = 5;
pub const CELL_W: f32 = 64.0;
pub const CELL_H: f32 = 40.0;

/// `value + delta` wrapped into `0..modulus`, in either direction.
/// A `modulus` of 0 is treated as 1 (so it can't divide by zero).
pub fn wrap_add(value: u32, delta: i64, modulus: u32) -> u32 {
    let m = i64::from(modulus.max(1));
    (i64::from(value) + delta).rem_euclid(m) as u32
}

/// The numbers shown top to bottom for a wheel resting on `selected`:
/// `rows` values centred on it, wrapping at both ends. (`rows` is
/// expected to be odd; with an even count the extra cell is at the
/// bottom.)
pub fn visible_values(selected: u32, modulus: u32, rows: usize) -> Vec<u32> {
    let half = (rows / 2) as i64;
    (0..rows as i64).map(|i| wrap_add(selected, i - half, modulus)).collect()
}

/// Removes whole steps of `step_px` from `accum` and returns how many
/// were removed (signed, truncating toward zero), leaving the
/// remainder to carry into the next frame — so slow trackpad scrolling
/// still adds up to a step instead of being rounded away every frame.
pub fn take_steps(accum: &mut f32, step_px: f32) -> i32 {
    if step_px <= 0.0 {
        return 0;
    }
    let steps = (*accum / step_px).trunc();
    *accum -= steps * step_px;
    steps as i32
}

/// Which row of a `rows`-high column of `cell_h`-tall cells starting
/// at `top` a pointer at `pointer_y` is over, as an offset from the
/// middle row (`-2..=2` for 5 rows; `0` = the selected cell). `None`
/// if the pointer is outside the column.
pub fn row_offset_at(pointer_y: f32, top: f32, cell_h: f32, rows: usize) -> Option<i64> {
    let row = ((pointer_y - top) / cell_h).floor();
    if row < 0.0 || row >= rows as f32 {
        return None;
    }
    Some(row as i64 - (rows / 2) as i64)
}

/// Draws one wheel, with a small caption above it, and returns whether
/// `value` changed this frame. `modulus` is the number of distinct
/// values (24 for hours, 60 for minutes and seconds). `id_salt` must be
/// unique among the wheels on screen.
pub fn show(ui: &mut egui::Ui, theme: &Theme, id_salt: &str, value: &mut u32, modulus: u32, caption: &str) -> bool {
    let p = &theme.palette;
    let mut changed = false;
    *value = (*value).min(modulus.saturating_sub(1));

    ui.vertical(|ui| {
        ui.set_width(CELL_W);
        ui.label(egui::RichText::new(caption.to_uppercase()).font(theme.small_font()).color(p.text_faint));
        ui.add_space(2.0);

        let size = Vec2::new(CELL_W, CELL_H * ROWS as f32);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let response = response.on_hover_cursor(egui::CursorIcon::ResizeVertical);
        let acc_id = egui::Id::new(("scroll_select_accum", id_salt));

        // Content moved down by N cells means the number N above is
        // now in the middle, so the value goes *down* by N.
        let mut content_move_px = 0.0;
        if response.dragged() {
            content_move_px += response.drag_delta().y;
        }
        if response.hovered() {
            let wheel = ui.input(|i| i.smooth_scroll_delta.y);
            if wheel != 0.0 {
                content_move_px += wheel;
                // Consume it so the surrounding ScrollArea doesn't
                // scroll the page as well.
                ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
            }
        }
        if content_move_px != 0.0 {
            let steps = ui.ctx().data_mut(|d| {
                let acc = d.get_temp_mut_or_default::<f32>(acc_id);
                *acc += content_move_px;
                take_steps(acc, CELL_H)
            });
            if steps != 0 {
                *value = wrap_add(*value, -i64::from(steps), modulus);
                changed = true;
            }
        } else if !response.dragged() {
            // At rest: drop any sub-step remainder so the next gesture
            // starts clean.
            ui.ctx().data_mut(|d| d.insert_temp(acc_id, 0.0_f32));
        }

        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                if let Some(offset) = row_offset_at(pos.y, rect.top(), CELL_H, ROWS) {
                    if offset != 0 {
                        *value = wrap_add(*value, offset, modulus);
                        changed = true;
                    }
                }
            }
        }

        let painter = ui.painter_at(rect);
        let numbers = visible_values(*value, modulus, ROWS);
        let middle = ROWS / 2;
        let corner = CornerRadius::same(Border::RADIUS);
        for (row, number) in numbers.iter().enumerate() {
            let cell = egui::Rect::from_min_size(rect.min + Vec2::new(0.0, row as f32 * CELL_H), Vec2::new(CELL_W, CELL_H));
            let distance = row.abs_diff(middle);
            let (text_color, size) = match distance {
                0 => (p.text, 24.0),
                1 => (p.text_muted, 17.0),
                _ => (p.text_faint, 14.0),
            };
            if distance == 0 {
                painter.rect_filled(cell, corner, p.surface);
            }
            painter.rect_stroke(cell, corner, Stroke::new(Border::HAIRLINE, p.border), StrokeKind::Inside);
            painter.text(cell.center(), egui::Align2::CENTER_CENTER, format!("{number:02}"), theme.mono(size), text_color);
        }
        // The selected cell's heavier outline, drawn last so its edges
        // aren't overpainted by its neighbours' hairlines.
        let selected = egui::Rect::from_min_size(rect.min + Vec2::new(0.0, middle as f32 * CELL_H), Vec2::new(CELL_W, CELL_H));
        painter.rect_stroke(selected, corner, Stroke::new(Border::THICK, p.text), StrokeKind::Inside);
    });

    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_add_wraps_forwards_and_backwards() {
        assert_eq!(wrap_add(58, 3, 60), 1);
        assert_eq!(wrap_add(1, -3, 60), 58);
        assert_eq!(wrap_add(0, -1, 24), 23);
        assert_eq!(wrap_add(23, 1, 24), 0);
    }

    #[test]
    fn wrap_add_handles_large_and_zero_deltas() {
        assert_eq!(wrap_add(10, 0, 60), 10);
        assert_eq!(wrap_add(10, 600, 60), 10);
        assert_eq!(wrap_add(10, -601, 60), 9);
    }

    #[test]
    fn wrap_add_survives_a_zero_modulus() {
        assert_eq!(wrap_add(5, 3, 0), 0);
    }

    #[test]
    fn visible_values_are_centred_on_the_selection() {
        assert_eq!(visible_values(30, 60, 5), vec![28, 29, 30, 31, 32]);
    }

    #[test]
    fn visible_values_wrap_at_both_ends() {
        assert_eq!(visible_values(0, 60, 5), vec![58, 59, 0, 1, 2]);
        assert_eq!(visible_values(59, 60, 5), vec![57, 58, 59, 0, 1]);
        assert_eq!(visible_values(0, 24, 5), vec![22, 23, 0, 1, 2]);
    }

    #[test]
    fn visible_values_repeat_when_the_modulus_is_smaller_than_the_window() {
        assert_eq!(visible_values(0, 2, 5), vec![0, 1, 0, 1, 0]);
    }

    #[test]
    fn take_steps_returns_whole_steps_and_keeps_the_remainder() {
        let mut acc = 95.0;
        assert_eq!(take_steps(&mut acc, 40.0), 2);
        assert!((acc - 15.0).abs() < 1e-5);
    }

    #[test]
    fn take_steps_truncates_toward_zero_for_negative_movement() {
        let mut acc = -95.0;
        assert_eq!(take_steps(&mut acc, 40.0), -2);
        assert!((acc + 15.0).abs() < 1e-5);
    }

    #[test]
    fn take_steps_accumulates_small_movements_across_calls() {
        let mut acc = 0.0;
        let mut total = 0;
        for _ in 0..10 {
            acc += 5.0;
            total += take_steps(&mut acc, 40.0);
        }
        assert_eq!(total, 1);
        assert!((acc - 10.0).abs() < 1e-5);
    }

    #[test]
    fn take_steps_ignores_a_nonpositive_step() {
        let mut acc = 100.0;
        assert_eq!(take_steps(&mut acc, 0.0), 0);
        assert_eq!(acc, 100.0);
    }

    #[test]
    fn row_offset_at_maps_pointer_rows_to_offsets_from_the_middle() {
        // Column top at 100, 40-tall cells, 5 rows: 100..300.
        assert_eq!(row_offset_at(105.0, 100.0, 40.0, 5), Some(-2));
        assert_eq!(row_offset_at(145.0, 100.0, 40.0, 5), Some(-1));
        assert_eq!(row_offset_at(190.0, 100.0, 40.0, 5), Some(0));
        assert_eq!(row_offset_at(225.0, 100.0, 40.0, 5), Some(1));
        assert_eq!(row_offset_at(299.0, 100.0, 40.0, 5), Some(2));
    }

    #[test]
    fn row_offset_at_is_none_outside_the_column() {
        assert_eq!(row_offset_at(99.0, 100.0, 40.0, 5), None);
        assert_eq!(row_offset_at(300.0, 100.0, 40.0, 5), None);
    }
}
