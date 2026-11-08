use eframe::egui::{self, RichText};

use crate::theme::{Spacing, Theme};
use crate::tasks::time_bar::TaskProgress;
use crate::ui::components::checkbox;
use crate::ui::components::task_bar::task_bar;

/// A single task row: checkbox + title (+ optional subject/due tag).
/// Shared by the Dashboard's "outstanding tasks" list and the To Do view.
pub struct TaskItem<'a> {
    pub title: &'a str,
    pub completed: &'a mut bool,
    pub tag: Option<&'a str>,
    /// Feature request: task "bars" — time spent against the budget.
    pub bar: Option<TaskProgress>,
}

impl<'a> TaskItem<'a> {
    pub fn new(title: &'a str, completed: &'a mut bool) -> Self {
        Self {
            title,
            completed,
            tag: None,
            bar: None,
        }
    }

    /// Shows a subject tag next to the task (Sprint 9).
    pub fn tag(mut self, tag: &'a str) -> Self {
        self.tag = Some(tag);
        self
    }

    /// Shows the task's time bar right after its name.
    pub fn bar(mut self, progress: Option<TaskProgress>) -> Self {
        self.bar = progress;
        self
    }

    pub fn show(self, ui: &mut egui::Ui, theme: &Theme) -> egui::Response {
        ui.horizontal(|ui| {
            let response = checkbox(ui, theme, self.completed, self.title);
            if let Some(progress) = self.bar {
                ui.add_space(Spacing::SM);
                task_bar(ui, theme, progress);
            }
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
