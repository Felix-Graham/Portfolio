use eframe::egui::{self, Align2, Frame, Margin, RichText, CornerRadius, Stroke};

use crate::theme::{Border, Spacing, Theme};

/// A centered, resizable modal window. `open` is toggled by the
/// caller; the modal closes itself on Escape or via its own close
/// button.
///
/// Sized to fit the viewport rather than a fixed pixel size (Issue 1
/// feedback: dialogs were exceeding the window width with no way to
/// shrink or scroll them). Content taller than the available height
/// scrolls instead of overflowing off-screen, and the window itself
/// can still be resized smaller by dragging its edges.
pub struct Modal<'a> {
    id: &'a str,
    title: &'a str,
}

impl<'a> Modal<'a> {
    pub fn new(id: &'a str, title: &'a str) -> Self {
        Self { id, title }
    }

    pub fn show(
        self,
        ctx: &egui::Context,
        theme: &Theme,
        open: &mut bool,
        add_contents: impl FnOnce(&mut egui::Ui),
    ) {
        if !*open {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            *open = false;
            return;
        }

        let p = &theme.palette;
        let screen = ctx.content_rect();
        // Leave a margin around the viewport edge so the window never
        // exceeds it, and never asks for more room than actually
        // exists on small/narrow screens.
        let max_width = (screen.width() - Spacing::XL).clamp(240.0, 520.0);
        let max_height = (screen.height() - Spacing::XL).max(200.0);
        let default_width = max_width.min(420.0);

        let frame = Frame::NONE
            .fill(p.surface)
            .stroke(Stroke::new(Border::THICK, p.border_strong))
            .corner_radius(CornerRadius::same(Border::RADIUS))
            .inner_margin(Margin::same(Spacing::MD as i8));

        egui::Window::new(RichText::new(self.title).font(theme.label_font()).color(p.text))
            .id(egui::Id::new(self.id))
            .open(open)
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(true)
            .default_width(default_width)
            .max_width(max_width)
            .max_height(max_height)
            .frame(frame)
            .show(ctx, |ui| {
                // A little headroom below max_height for the title bar
                // and frame margins, so the scroll area itself doesn't
                // force the window past the viewport.
                egui::ScrollArea::vertical().max_height(max_height - 80.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    add_contents(ui);
                });
            });
    }
}
