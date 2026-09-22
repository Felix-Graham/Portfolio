use eframe::egui::{self, Frame, Margin, RichText, CornerRadius, Stroke};

use crate::theme::{Border, Spacing, Theme};

/// Represents a revision session or calendar event: time, subject, and
/// an optional task-completion progress. Used on the Dashboard.
pub struct EventCard<'a> {
    pub time: &'a str,
    pub subject: &'a str,
    pub progress: Option<(usize, usize)>,
}

impl<'a> EventCard<'a> {
    pub fn new(time: &'a str, subject: &'a str) -> Self {
        Self {
            time,
            subject,
            progress: None,
        }
    }

    /// Attaches a "X / Y complete" line (Sprint 4: event task
    /// progress), matching the plan's Section 11 example format.
    pub fn progress(mut self, completed: usize, total: usize) -> Self {
        self.progress = Some((completed, total));
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
