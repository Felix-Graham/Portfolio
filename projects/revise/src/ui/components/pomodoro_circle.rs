use eframe::egui::{self, Pos2, Shape, Stroke, Vec2};

use crate::theme::Theme;

/// Draws a circular countdown: a faint full background ring, a
/// progress arc drawn clockwise from the top, and the given text
/// centered inside. Used at a small size on the Dashboard's compact
/// Pomodoro card and much larger in the full-screen focus overlay —
/// same drawing code either way, just a different `radius`.
pub fn show(ui: &mut egui::Ui, theme: &Theme, progress: f32, center_text: &str, phase_label: &str, radius: f32) {
    let p = &theme.palette;
    let size = Vec2::splat(radius * 2.0);
    let (response, painter) = ui.allocate_painter(size, egui::Sense::hover());
    let center = response.rect.center();

    let ring_width = (radius / 12.0).max(3.0);

    // Background ring — the full circle, faint.
    painter.circle_stroke(center, radius - ring_width / 2.0, Stroke::new(ring_width, p.border));

    // Progress arc — sampled points clockwise from the top (12
    // o'clock), since egui has no direct arc primitive: a polyline
    // approximating one is the standard workaround.
    let progress = progress.clamp(0.0, 1.0);
    if progress > 0.0 {
        let steps = ((progress * 120.0) as usize).max(2);
        let arc_radius = radius - ring_width / 2.0;
        let points: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let t = progress * (i as f32 / steps as f32);
                // -90 degrees so 0 progress starts at the top, moving clockwise.
                let angle = -std::f32::consts::FRAC_PI_2 + t * std::f32::consts::TAU;
                center + Vec2::new(angle.cos(), angle.sin()) * arc_radius
            })
            .collect();
        painter.add(Shape::line(points, Stroke::new(ring_width, p.focus)));
    }

    let text_size = (radius / 2.8).max(12.0);
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        center_text,
        egui::FontId::monospace(text_size),
        p.text,
    );

    let label_pos = center + Vec2::new(0.0, text_size * 0.9);
    let small = theme.small_font();
    painter.text(label_pos, egui::Align2::CENTER_CENTER, phase_label.to_uppercase(), small, p.text_faint);
}
