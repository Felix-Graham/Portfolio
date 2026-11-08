use eframe::egui::{self, containers::scroll_area::ScrollBarVisibility, CornerRadius, Frame, Margin, Stroke, Vec2};

use crate::theme::{Border, Spacing, Theme};
use crate::ui::pager;

/// A bordered content container. The base building block for shell
/// sections (Dashboard cards, Calendar columns, list rows, etc).
pub struct Panel<'a> {
    title: Option<&'a str>,
    padding: f32,
    /// When set, the panel is exactly this size (border included)
    /// instead of shrinking to its content — see `fill`.
    fill: Option<Vec2>,
}

impl<'a> Panel<'a> {
    pub fn new() -> Self {
        Self {
            title: None,
            padding: Spacing::MD,
            fill: None,
        }
    }

    /// Makes the panel exactly `size` (its outer size, border
    /// included), for the paged Dashboard where every page's items
    /// are arranged to fill the screen. The title stays put and the
    /// contents sit in a scroll area with no visible bar, so a cell
    /// that is too small for its contents scrolls instead of
    /// overflowing; it reports to `ui::pager` whether it can still
    /// scroll, so a scroll gesture at its edge can turn the page.
    pub fn fill(mut self, size: Vec2) -> Self {
        self.fill = Some(size);
        self
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
        self.show_sized(ui, theme, |ui, _visible| add_contents(ui))
    }

    /// Like `show`, but also tells the contents how much room is
    /// visible below the title: for a `fill` panel that is the cell's
    /// own content box (a scroll area's `available_size` is unbounded
    /// in the directions it scrolls, so contents that want to fit
    /// themselves to the cell — the pie chart — can't ask the `Ui`);
    /// otherwise it is simply `ui.available_size()`.
    pub fn show_sized<R>(
        self,
        ui: &mut egui::Ui,
        theme: &Theme,
        add_contents: impl FnOnce(&mut egui::Ui, Vec2) -> R,
    ) -> R {
        let frame = Frame::NONE
            .fill(theme.palette.surface)
            .stroke(Stroke::new(Border::HAIRLINE, theme.palette.border))
            .corner_radius(CornerRadius::same(Border::RADIUS))
            .inner_margin(Margin::same(self.padding as i8));

        let fill = self.fill;
        let padding = self.padding;
        frame
            .show(ui, |ui| {
                if let Some(size) = fill {
                    // The frame adds `padding` and the border stroke
                    // on every side, so the content box is smaller
                    // than the requested outer size by that much.
                    let inset = 2.0 * (padding + Border::HAIRLINE);
                    let inner = Vec2::new((size.x - inset).max(0.0), (size.y - inset).max(0.0));
                    ui.set_min_size(inner);
                    ui.set_max_size(inner);
                }
                if let Some(title) = self.title {
                    ui.label(
                        egui::RichText::new(title.to_uppercase())
                            .font(theme.small_font())
                            .color(theme.palette.text_muted),
                    );
                    ui.add_space(Spacing::SM);
                }
                let visible = ui.available_size();
                if fill.is_none() {
                    return add_contents(ui, visible);
                }
                // `both`, not `vertical`: a vertical-only scroll area
                // grows sideways to fit wide contents, which would
                // push the panel past its cell. Neither bar is shown.
                // `min_scrolled_*` default to 64 points, which would
                // make a short cell (e.g. a stats figure) taller than
                // the space it was given.
                let output = egui::ScrollArea::both()
                    .min_scrolled_width(0.0)
                    .min_scrolled_height(0.0)
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| add_contents(ui, visible));
                let (can_up, can_down) =
                    pager::can_scroll(output.state.offset.y, output.content_size.y, output.inner_rect.height());
                pager::push_report(
                    ui.ctx(),
                    pager::ScrollReport { rect: output.inner_rect, can_up, can_down },
                );
                output.inner
            })
            .inner
    }
}
