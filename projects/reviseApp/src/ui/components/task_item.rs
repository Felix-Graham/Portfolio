use eframe::egui::{self, RichText};

use crate::theme::{Spacing, Theme};
use crate::ui::components::checkbox;

/// A single task row: checkbox + title (+ optional subject/due tag).
/// Shared by the Dashboard's "outstanding tasks" list and the To Do view.
pub struct TaskItem<'a> {
    pub title: &'a str,
    pub completed: &'a mut bool,
    pub tag: Option<&'a str>,
}

impl<'a> TaskItem<'a> {
    pub fn new(title: &'a str, completed: &'a mut bool) -> Self {
        Self {
            title,
            completed,
            tag: None,
        }
    }

    /// Shows a subject tag next to the task (Sprint 9).
    pub fn tag(mut self, tag: &'a str) -> Self {
        self.tag = Some(tag);
        self
    }

    pub fn show(self, ui: &mut egui::Ui, theme: &Theme) -> egui::Response {
        ui.horizontal(|ui| {
            let response = checkbox(ui, theme, self.completed, self.title);
            if let Some(tag) = self.tag {
                ui.add_space(Spacing::SM);
                ui.label(
                    RichText::new(tag.to_uppercase())
                        .font(theme.small_font())
                        .color(theme.palette.text_faint),
                );
            }
            response
        })
        .inner
    }
}
