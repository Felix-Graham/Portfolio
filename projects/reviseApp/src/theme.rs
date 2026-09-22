//! Revise visual design system.
//!
//! Sprint 1 goal: establish a coherent, minimalist, monochromatic visual
//! language (inspired by Nothing OS / Nothing's web design) that every
//! shell view and component draws from. Nothing in this module should be
//! bypassed by ad-hoc colors/spacing elsewhere in the app.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, Vec2,
};

/// Black / white / gray palette. No color accents in Sprint 1.
pub struct Palette {
    pub background: Color32,
    pub surface: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    pub focus: Color32,
}

impl Palette {
    pub const fn dark() -> Self {
        Self {
            background: Color32::from_rgb(0x0A, 0x0A, 0x0A),
            surface: Color32::from_rgb(0x12, 0x12, 0x12),
            border: Color32::from_rgb(0x2A, 0x2A, 0x2A),
            border_strong: Color32::from_rgb(0x55, 0x55, 0x55),
            text: Color32::from_rgb(0xF2, 0xF2, 0xF2),
            text_muted: Color32::from_rgb(0x9A, 0x9A, 0x9A),
            text_faint: Color32::from_rgb(0x5A, 0x5A, 0x5A),
            focus: Color32::from_rgb(0xFF, 0xFF, 0xFF),
        }
    }
}

/// Spacing scale, in points. Generous whitespace, consistent rhythm.
pub struct Spacing;
impl Spacing {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 16.0;
    pub const LG: f32 = 24.0;
    #[allow(dead_code)]
    pub const XL: f32 = 40.0;
}

/// Border weights. Thin, technical, never decorative.
pub struct Border;
impl Border {
    pub const HAIRLINE: f32 = 1.0;
    pub const THICK: f32 = 1.5;
    /// Sharp/near-sharp corners per the visual direction.
    // u8, not f32 — egui's CornerRadius::same() (renamed from
    // Rounding::same() in the same egui version bump that fixed
    // NoGlutinConfigs) takes u8, storing corner radii compactly. Only
    // ever used via CornerRadius::same(Border::RADIUS) below and at
    // every ui::components call site — never as a plain float.
    pub const RADIUS: u8 = 2;
}

/// Central theme handle. One instance, threaded through the app.
pub struct Theme {
    pub palette: Palette,
}

impl Theme {
    pub fn new() -> Self {
        Self {
            palette: Palette::dark(),
        }
    }

    pub fn mono(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Monospace)
    }

    pub fn heading_font(&self) -> FontId {
        self.mono(22.0)
    }

    pub fn label_font(&self) -> FontId {
        self.mono(14.0)
    }

    pub fn small_font(&self) -> FontId {
        self.mono(11.0)
    }

    /// Apply the design system to the egui context. Called once at
    /// startup; every widget style flows from here rather than from
    /// scattered per-widget overrides.
    ///
    /// `Context::style()`/`set_style()` (read-modify-write the *one*
    /// active style) were removed in favor of `style_of`/`set_style_of`
    /// (Light/Dark are tracked as two separate styles now, since egui
    /// can follow the OS's own light/dark preference) — this app has
    /// never had a light palette and never will, so rather than pick
    /// one of the two, `ctx.set_theme(ThemePreference::Dark)` locks
    /// egui into Dark unconditionally (ignoring the OS entirely, same
    /// as before this rename existed), and `all_styles_mut` applies
    /// this one style to both slots identically, so nothing changes
    /// even in the case some other widget code queries the Light one.
    pub fn apply(&self, ctx: &egui::Context) {
        let p = &self.palette;
        ctx.set_theme(egui::ThemePreference::Dark);
        ctx.all_styles_mut(|style| {
            style.visuals.dark_mode = true;
            style.visuals.override_text_color = Some(p.text);
            style.visuals.window_fill = p.background;
            style.visuals.panel_fill = p.background;
            style.visuals.faint_bg_color = p.surface;
            style.visuals.extreme_bg_color = p.surface;

            style.visuals.window_stroke = Stroke::new(Border::HAIRLINE, p.border);
            style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(Border::HAIRLINE, p.border);
            style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text_muted);
            style.visuals.widgets.inactive.bg_fill = p.surface;
            style.visuals.widgets.inactive.bg_stroke = Stroke::new(Border::HAIRLINE, p.border);
            style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text_muted);
            style.visuals.widgets.hovered.bg_fill = p.surface;
            style.visuals.widgets.hovered.bg_stroke = Stroke::new(Border::THICK, p.border_strong);
            style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text);
            style.visuals.widgets.active.bg_fill = p.surface;
            style.visuals.widgets.active.bg_stroke = Stroke::new(Border::THICK, p.focus);
            style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, p.text);
            style.visuals.selection.bg_fill = p.border_strong;
            style.visuals.selection.stroke = Stroke::new(Border::HAIRLINE, p.focus);

            // Restrained: no rounded corners, near-square throughout.
            for w in [
                &mut style.visuals.widgets.noninteractive,
                &mut style.visuals.widgets.inactive,
                &mut style.visuals.widgets.hovered,
                &mut style.visuals.widgets.active,
                &mut style.visuals.widgets.open,
            ] {
                w.corner_radius = CornerRadius::same(Border::RADIUS);
            }
            style.visuals.window_corner_radius = CornerRadius::same(Border::RADIUS);
            style.visuals.menu_corner_radius = CornerRadius::same(Border::RADIUS);

            // Strong, visible keyboard focus ring (Sprint 1 acceptance criterion).
            style.visuals.widgets.active.fg_stroke = Stroke::new(2.0, p.focus);

            // Restrained animation.
            style.animation_time = 0.08;

            style.spacing.item_spacing = Vec2::new(Spacing::SM, Spacing::SM);
            style.spacing.window_margin = Margin::same(Spacing::MD as i8);
            style.spacing.button_padding = Vec2::new(Spacing::SM, Spacing::XS);

            style.text_styles.insert(egui::TextStyle::Heading, self.heading_font());
            style.text_styles.insert(egui::TextStyle::Body, self.label_font());
            style.text_styles.insert(egui::TextStyle::Button, self.label_font());
            style.text_styles.insert(egui::TextStyle::Small, self.small_font());
            style.text_styles.insert(egui::TextStyle::Monospace, self.label_font());
        });
    }
}
