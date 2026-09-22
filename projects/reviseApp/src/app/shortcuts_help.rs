use eframe::egui::{self, RichText};

use crate::theme::{Spacing, Theme};
use crate::ui::components::Modal;

/// Whether the keyboard shortcuts reference is open. A `?` press or
/// the "Keyboard Shortcuts" palette command opens it; Escape or its
/// own close button closes it — same pattern as every other modal.
#[derive(Default)]
pub struct ShortcutsHelpState {
    pub open: bool,
}

const GLOBAL_SHORTCUTS: &[(&str, &str)] = &[
    ("1 – 5", "Go to Dashboard / Calendar / To Do / Notes / Flashcards"),
    ("N", "New event"),
    ("T", "New task"),
    ("D", "Today (opens Calendar's Day view)"),
    ("W", "Week (opens Calendar's Week view)"),
    ("M", "Month (opens Calendar's Month view)"),
    ("Tab / Shift+Tab", "Cycle sections"),
    ("i", "Insert mode — focus the active view's main input"),
    ("/", "Open the command palette"),
    ("Esc", "Close a modal, or leave insert mode"),
    ("Enter", "Submit a form, or open the selected list item"),
    ("?", "Show this reference"),
];

const LIST_SHORTCUTS: &[(&str, &str)] = &[
    ("↑ / ↓", "Move the selection in To Do or Notes"),
    ("Space", "Complete/uncomplete the selected task"),
    ("Enter", "Open the selected item for editing"),
];

const CALENDAR_SHORTCUTS: &[(&str, &str)] = &[
    ("← / →", "Previous / next day, week, or month"),
    ("↑ / ↓", "Move the selection in the command palette"),
    ("/flashcards [set]", "Palette: jump straight into studying a set"),
];

const FLASHCARDS_PLAYER_SHORTCUTS: &[(&str, &str)] = &[
    ("Tab", "Next card"),
    ("→", "Reveal the answer, or (if already revealed) next card"),
    ("←", "Hide the answer, or (if not revealed) previous card"),
    ("Enter / Space", "Flip the current card in place"),
    ("Esc", "End the study session"),
];

pub fn show(ctx: &egui::Context, theme: &Theme, state: &mut ShortcutsHelpState) {
    if !state.open {
        return;
    }

    let p = &theme.palette;
    let mut modal_open = true;

    Modal::new("shortcuts_help", "Keyboard Shortcuts").show(ctx, theme, &mut modal_open, |ui| {
        for (title, rows) in [
            ("Anywhere", GLOBAL_SHORTCUTS),
            ("To Do & Notes lists", LIST_SHORTCUTS),
            ("Calendar & palette", CALENDAR_SHORTCUTS),
            ("Flashcards player", FLASHCARDS_PLAYER_SHORTCUTS),
        ] {
            ui.label(RichText::new(title.to_uppercase()).font(theme.small_font()).color(p.text_faint));
            ui.add_space(Spacing::XS);
            for (keys, description) in rows {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [140.0, 0.0],
                        egui::Label::new(RichText::new(*keys).font(theme.label_font()).color(p.text)),
                    );
                    ui.label(RichText::new(*description).font(theme.small_font()).color(p.text_muted));
                });
            }
            ui.add_space(Spacing::MD);
        }
    });

    if !modal_open {
        state.open = false;
    }
}
