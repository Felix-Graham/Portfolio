use eframe::egui::{self, RichText};

use crate::theme::Theme;

/// A minimal square checkbox, no rounded tick graphics, just a hairline
/// box that fills solid when checked. Used directly by [`super::task_item`].
pub fn checkbox(ui: &mut egui::Ui, theme: &Theme, checked: &mut bool, label: &str) -> egui::Response {
    let p = &theme.palette;
    let box_glyph = if *checked { "[x]" } else { "[ ]" };
    let text = RichText::new(format!("{box_glyph} {label}"))
        .font(theme.label_font())
        .color(if *checked { p.text_faint } else { p.text })
        .strikethrough_if(*checked);

    let response = ui.selectable_label(false, text);
    if response.clicked() {
        *checked = !*checked;
    }
    response
}

// Small local extension trait so call sites read naturally above.
trait StrikethroughIf {
    fn strikethrough_if(self, cond: bool) -> Self;
}
impl StrikethroughIf for RichText {
    fn strikethrough_if(self, cond: bool) -> Self {
        if cond {
            self.strikethrough()
        } else {
            self
        }
    }
}
