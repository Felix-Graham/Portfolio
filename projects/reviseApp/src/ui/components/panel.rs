use eframe::egui::{self, CornerRadius, Frame, Margin, Stroke};

use crate::theme::{Border, Spacing, Theme};

/// A bordered content container. The base building block for shell
/// sections (Dashboard cards, Calendar columns, list rows, etc).
pub struct Panel<'a> {
    title: Option<&'a str>,
    padding: f32,
}

impl<'a> Panel<'a> {
    pub fn new() -> Self {
        Self {
            title: None,
            padding: Spacing::MD,
        }
    }

    pub fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    #[allow(dead_code)]
    pub fn tight(mut self) -> Self {
        self.padding = Spacing::SM;
        self
    }

    pub fn show<R>(
        self,
        ui: &mut egui::Ui,
        theme: &Theme,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        let frame = Frame::NONE
            .fill(theme.palette.surface)
            .stroke(Stroke::new(Border::HAIRLINE, theme.palette.border))
            .corner_radius(CornerRadius::same(Border::RADIUS))
            .inner_margin(Margin::same(self.padding as i8));

        frame
            .show(ui, |ui| {
                if let Some(title) = self.title {
                    ui.label(
                        egui::RichText::new(title.to_uppercase())
                            .font(theme.small_font())
                            .color(theme.palette.text_muted),
                    );
                    ui.add_space(Spacing::SM);
                }
                add_contents(ui)
            })
            .inner
    }
}
