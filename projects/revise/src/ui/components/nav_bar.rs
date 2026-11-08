use eframe::egui::{self, CornerRadius, RichText, Stroke};

use crate::app::View;
use crate::theme::{Border, Spacing, Theme};

/// The persistent top navigation bar. Renders the four primary
/// sections, highlights the active one, and shows each section's
/// keyboard shortcut digit (Sprint 1: "basic keyboard navigation").
pub struct NavigationBar;

impl NavigationBar {
    /// Returns `Some(view)` if the user clicked a different section.
    pub fn show(ui: &mut egui::Ui, theme: &Theme, active: View) -> Option<View> {
        let p = &theme.palette;
        let mut selected = None;

        ui.horizontal(|ui| {
            for view in View::ALL {
                let is_active = view == active;

                let text = RichText::new(format!("{}  {}", view.shortcut_digit(), view.label()))
                    .font(theme.label_font())
                    .color(if is_active { p.text } else { p.text_muted });

                let stroke = if is_active {
                    Stroke::new(Border::THICK, p.focus)
                } else {
                    Stroke::new(Border::HAIRLINE, p.border)
                };

                let button = egui::Button::new(text)
                    .fill(if is_active { p.surface } else { p.background })
                    .stroke(stroke)
                    .corner_radius(CornerRadius::same(Border::RADIUS));

                let response = ui.add(button);
                if response.clicked() && !is_active {
                    selected = Some(view);
                }

                ui.add_space(Spacing::SM);
            }
        });

        selected
    }
}
