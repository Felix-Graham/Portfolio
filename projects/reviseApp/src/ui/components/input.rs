use eframe::egui::{self, Frame, Id, Margin, CornerRadius, Stroke, TextEdit};

use crate::theme::{Border, Spacing, Theme};

/// Single-line text input with a consistent thin-border treatment and
/// an optional placeholder, matching the rest of the design system.
///
/// Give an input a stable `id` when something outside its own view
/// needs to focus it programmatically — e.g. the `i` key focusing the
/// "primary" field of the active view (see `app::View::primary_input_id`
/// and `ReviseApp::handle_keyboard_navigation`).
pub struct Input<'a> {
    value: &'a mut String,
    placeholder: &'a str,
    id: Option<&'a str>,
    width: Option<f32>,
    rows: Option<usize>,
}

impl<'a> Input<'a> {
    pub fn new(value: &'a mut String, placeholder: &'a str) -> Self {
        Self { value, placeholder, id: None, width: None, rows: None }
    }

    /// Assigns a stable, globally-addressable id (via `egui::Id::new`)
    /// so `egui::Memory::request_focus` can target this exact field
    /// from outside its own render call.
    pub fn id(mut self, id: &'a str) -> Self {
        self.id = Some(id);
        self
    }

    /// Fixes the field's width instead of filling the row — for
    /// compact numeric fields like "every N days" (Sprint 6).
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Makes this a multi-line field, `rows` tall — for note content
    /// (Sprint 8), where Enter should insert a newline rather than
    /// submit the surrounding form. Callers check
    /// `response.has_focus()` rather than `response.lost_focus()` to
    /// tell whether Enter was meant for this field.
    pub fn multiline(mut self, rows: usize) -> Self {
        self.rows = Some(rows);
        self
    }

    pub fn show(self, ui: &mut egui::Ui, theme: &Theme) -> egui::Response {
        Frame::NONE
            .fill(theme.palette.surface)
            .stroke(Stroke::new(Border::HAIRLINE, theme.palette.border))
            .corner_radius(CornerRadius::same(Border::RADIUS))
            .inner_margin(Margin::symmetric(Spacing::SM as i8, Spacing::XS as i8))
            .show(ui, |ui| {
                let desired_width = self.width.unwrap_or_else(|| ui.available_width());
                let mut text_edit = match self.rows {
                    Some(rows) => TextEdit::multiline(self.value).desired_rows(rows),
                    None => TextEdit::singleline(self.value),
                }
                .hint_text(self.placeholder)
                .frame(Frame::NONE)
                .font(theme.label_font())
                .desired_width(desired_width);

                if let Some(id) = self.id {
                    text_edit = text_edit.id(Id::new(id));
                }

                ui.add(text_edit)
            })
            .inner
    }
}
