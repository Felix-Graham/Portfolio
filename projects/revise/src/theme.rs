//! Revise visual design system.
//!
//! Sprint 1 goal: establish a coherent, minimalist, monochromatic visual
//! language (inspired by Nothing OS / Nothing's web design) that every
//! shell view and component draws from. Nothing in this module should be
//! bypassed by ad-hoc colors/spacing elsewhere in the app.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, Vec2,
};

use crate::subjects::Subject;

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
    /// Right/wrong feedback (the startup quiz and its progress bar). The one
    /// place the palette carries meaning in colour, so each is always paired
    /// with a ✓ / ✗ rather than relying on hue alone.
    pub correct: Color32,
    pub incorrect: Color32,
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
            correct: Color32::from_rgb(0x3C, 0xB0, 0x5A),
            incorrect: Color32::from_rgb(0xE0, 0x4F, 0x4F),
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

/// A small, fixed set of visually distinct default colors, used
/// whenever a subject doesn't have its own explicitly chosen color
/// (every subject before the "Colour" feature request, and any new
/// one until the person picks a color for it in "Manage Subjects").
/// Originally the Improved Line Graph's own one-off palette
/// (`dashboard::SUBJECT_LINE_COLORS`); moved here and generalized to
/// every chart plus Calendar events once a subject's color became a
/// real, persisted, per-subject choice rather than something only
/// ever derived from its id. Indexed by `subject.id %
/// DEFAULT_SUBJECT_COLORS.len()`, not position in whatever subject
/// list happens to be showing, so a subject's default color can't
/// shift just because another subject was added, archived, or
/// removed — see `subject_color` below, the only place this should
/// actually be read from.
pub const DEFAULT_SUBJECT_COLORS: &[Color32] = &[
    Color32::from_rgb(0xE0, 0x6C, 0x5A), // terracotta
    Color32::from_rgb(0x5A, 0x9E, 0xE0), // steel blue
    Color32::from_rgb(0x6F, 0xB8, 0x6A), // sage green
    Color32::from_rgb(0xD9, 0xA9, 0x3B), // ochre
    Color32::from_rgb(0xA8, 0x7C, 0xD9), // lavender
    Color32::from_rgb(0xD9, 0x6F, 0xA8), // dusty pink
    Color32::from_rgb(0x4C, 0xB8, 0xAE), // teal
    Color32::from_rgb(0xB8, 0x8A, 0x5A), // tan
];

/// Parses a `"#rrggbb"` string (the only format `color_to_hex`
/// produces, and the only one `subjects.color` is ever set to) into a
/// `Color32`. Returns `None` for anything else, rather than panicking
/// or silently drawing black — `subject_color` below falls back to
/// this subject's default color in that case.
pub fn parse_hex_color(hex: &str) -> Option<Color32> {
    let hex = hex.trim().strip_prefix('#').unwrap_or(hex.trim());
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color32::from_rgb(r, g, b))
}

/// The inverse of `parse_hex_color` — always six lowercase hex digits
/// prefixed with `#`. The one format ever written to `subjects.color`
/// (via `queries::subjects::set_color`).
pub fn color_to_hex(color: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

/// The color to draw `subject_id` with, anywhere a subject needs
/// one — the person's own choice from "Manage Subjects" if they've
/// made one (parsed from `Subject::color`), else this id's entry in
/// `DEFAULT_SUBJECT_COLORS`. Feature request: "Colour" — "I like the
/// use of colours in the line graph... extend this theming to the
/// rest of the statistics as well as calendar events by subject."
/// `subjects` only needs to contain the one subject being looked
/// up — every call site already has a full subject list in hand for
/// other reasons (naming, filtering), so this takes a slice rather
/// than a single `&Subject` purely for caller convenience.
pub fn subject_color(subjects: &[Subject], subject_id: i64) -> Color32 {
    let explicit = subjects.iter().find(|s| s.id == subject_id).and_then(|s| s.color.as_deref()).and_then(parse_hex_color);
    explicit.unwrap_or_else(|| {
        DEFAULT_SUBJECT_COLORS[(subject_id.rem_euclid(DEFAULT_SUBJECT_COLORS.len() as i64)) as usize]
    })
}

#[cfg(test)]
mod subject_color_tests {
    use super::*;

    fn subject_with_color(id: i64, color: Option<&str>) -> Subject {
        Subject {
            id,
            name: "Test".to_owned(),
            archived: false,
            created_at: String::new(),
            parent_subject_id: None,
            color: color.map(|c| c.to_owned()),
        }
    }

    #[test]
    fn parse_hex_color_accepts_a_leading_hash() {
        assert_eq!(parse_hex_color("#a1b2c3"), Some(Color32::from_rgb(0xa1, 0xb2, 0xc3)));
    }

    #[test]
    fn parse_hex_color_accepts_no_leading_hash() {
        assert_eq!(parse_hex_color("a1b2c3"), Some(Color32::from_rgb(0xa1, 0xb2, 0xc3)));
    }

    #[test]
    fn parse_hex_color_rejects_the_wrong_length() {
        assert_eq!(parse_hex_color("#abc"), None);
    }

    #[test]
    fn parse_hex_color_rejects_non_hex_digits() {
        assert_eq!(parse_hex_color("#zzzzzz"), None);
    }

    #[test]
    fn color_to_hex_round_trips_through_parse_hex_color() {
        let color = Color32::from_rgb(0x12, 0x34, 0x56);
        assert_eq!(parse_hex_color(&color_to_hex(color)), Some(color));
    }

    #[test]
    fn subject_color_uses_the_explicit_choice_when_set() {
        let subjects = vec![subject_with_color(1, Some("#112233"))];
        assert_eq!(subject_color(&subjects, 1), Color32::from_rgb(0x11, 0x22, 0x33));
    }

    #[test]
    fn subject_color_falls_back_to_the_default_palette_when_unset() {
        let subjects = vec![subject_with_color(1, None)];
        assert_eq!(subject_color(&subjects, 1), DEFAULT_SUBJECT_COLORS[1]);
    }

    #[test]
    fn subject_color_falls_back_when_the_subject_is_missing_entirely() {
        // A defensive default for a stale/unknown id — every real call
        // site passes a list that actually contains the id it's
        // asking about, but this keeps the function total rather than
        // panicking if that ever isn't true.
        assert_eq!(subject_color(&[], 3), DEFAULT_SUBJECT_COLORS[3]);
    }

    #[test]
    fn subject_color_default_is_stable_regardless_of_other_subjects_present() {
        // The whole reason this is keyed by id rather than position:
        // adding an unrelated subject to the list must not shift
        // subject 1's own default color.
        let with_others = vec![subject_with_color(5, None), subject_with_color(1, None), subject_with_color(1, None)];
        assert_eq!(subject_color(&with_others, 1), subject_color(&[subject_with_color(1, None)], 1));
    }
}
