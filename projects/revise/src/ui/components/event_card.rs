use eframe::egui::{self, Frame, Margin, RichText, CornerRadius, Stroke};

use crate::theme::{Border, Spacing, Theme};

/// Represents a revision session or calendar event: time, subject, and
/// an optional task-completion progress. Used on the Dashboard.
pub struct EventCard<'a> {
    pub time: &'a str,
    pub subject: &'a str,
    pub progress: Option<(usize, usize)>,
    pub accent_color: Option<egui::Color32>,
}

impl<'a> EventCard<'a> {
    pub fn new(time: &'a str, subject: &'a str) -> Self {
        Self {
            time,
            subject,
            progress: None,
            accent_color: None,
        }
    }

    /// Attaches a "X / Y complete" line (Sprint 4: event task
    /// progress), matching the plan's Section 11 example format.
    pub fn progress(mut self, completed: usize, total: usize) -> Self {
        self.progress = Some((completed, total));
        self
    }

    /// Feature request: "Colour" — "extend this theming to... calendar
    /// events by subject." These Dashboard cards (Current/Next
    /// session, Today's Schedule) are the same underlying revision
    /// sessions Calendar shows with its own per-subject accent bar
    /// (`ui::calendar::day_view`); this mirrors that bar here rather
    /// than introducing a different visual language for the same
    /// data. Left unset — the default from `new` — for an event with
    /// no subject, matching Calendar's own fallback for that case.
    pub fn accent_color(mut self, color: egui::Color32) -> Self {
        self.accent_color = Some(color);
        self
    }

    pub fn show(self, ui: &mut egui::Ui, theme: &Theme) {
        let p = &theme.palette;
        Frame::NONE
            .fill(p.surface)
            .stroke(Stroke::new(Border::HAIRLINE, p.border))
            .corner_radius(CornerRadius::same(Border::RADIUS))
            .inner_margin(Margin::same(Spacing::SM as i8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(color) = self.accent_color {
                        let (bar_rect, _) = ui.allocate_exact_size(egui::vec2(3.0, 18.0), egui::Sense::hover());
                        ui.painter().rect_filled(bar_rect, CornerRadius::same(1), color);
                        ui.add_space(Spacing::XS);
                    }
                    ui.label(RichText::new(self.time).font(theme.label_font()).color(p.text_muted));
                    ui.add_space(Spacing::SM);
                    ui.label(RichText::new(self.subject).font(theme.label_font()).color(p.text));
                });
                if let Some((completed, total)) = self.progress {
                    ui.label(
                        RichText::new(format!("{completed} / {total} complete"))
                            .font(theme.small_font())
                            .color(p.text_faint),
                    );
                }
            });
    }
}
