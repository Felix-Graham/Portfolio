use eframe::egui::{self, Button as EguiButton, CornerRadius, RichText, Stroke};

use crate::theme::{Border, Theme};

/// Primary vs. quiet (secondary) button treatments. Both stay
/// monochrome; primary just inverts fill/text on hover-active.
pub enum ButtonKind {
    Primary,
    Quiet,
}

pub struct RButton<'a> {
    label: &'a str,
    kind: ButtonKind,
}

impl<'a> RButton<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            kind: ButtonKind::Quiet,
        }
    }

    pub fn primary(mut self) -> Self {
        self.kind = ButtonKind::Primary;
        self
    }

    pub fn show(self, ui: &mut egui::Ui, theme: &Theme) -> egui::Response {
        let p = &theme.palette;
        let (fill, text_color, stroke) = match self.kind {
            ButtonKind::Primary => (p.text, p.background, Stroke::new(Border::HAIRLINE, p.text)),
            ButtonKind::Quiet => (p.surface, p.text, Stroke::new(Border::HAIRLINE, p.border)),
        };

        ui.add(
            EguiButton::new(RichText::new(self.label).font(theme.label_font()).color(text_color))
                .fill(fill)
                .stroke(stroke)
                .corner_radius(CornerRadius::same(Border::RADIUS)),
        )
    }
}
