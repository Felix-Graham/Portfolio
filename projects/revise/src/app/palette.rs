use eframe::egui::{self, RichText};

use crate::database::Database;
use crate::theme::{Spacing, Theme};
use crate::ui::components::{Input, Modal};
use crate::ui::flashcards::{self, SetOption, SetRef};

/// A global action the command palette can run. Deliberately a short,
/// curated list of page-level actions (navigate, create) rather than
/// every clickable element on screen — see docs/architecture.md for
/// why this approach was chosen over per-widget hint tags or a Vim-style
/// `/`-search-and-jump over arbitrary UI text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandId {
    GoToDashboard,
    GoToCalendar,
    GoToToDo,
    GoToNotes,
    GoToFlashcards,
    NewTask,
    NewEvent,
    NewNote,
    ManageSubjects,
    ManageSources,
    OpenSettings,
    Today,
    Week,
    Month,
    ShowShortcuts,
    PlanTimes,
    ClearAutoBlocks,
}

pub struct Command {
    pub label: &'static str,
    pub id: CommandId,
}

pub const ALL_COMMANDS: &[Command] = &[
    Command { label: "Go to Dashboard", id: CommandId::GoToDashboard },
    Command { label: "Go to Calendar", id: CommandId::GoToCalendar },
    Command { label: "Go to To Do", id: CommandId::GoToToDo },
    Command { label: "Go to Notes", id: CommandId::GoToNotes },
    Command { label: "Go to Flashcards", id: CommandId::GoToFlashcards },
    Command { label: "New Task", id: CommandId::NewTask },
    Command { label: "New Event", id: CommandId::NewEvent },
    Command { label: "New Note", id: CommandId::NewNote },
    Command { label: "Manage Subjects", id: CommandId::ManageSubjects },
    Command { label: "Manage Sources", id: CommandId::ManageSources },
    Command { label: "Settings", id: CommandId::OpenSettings },
    Command { label: "Go to Today", id: CommandId::Today },
    Command { label: "Go to Week", id: CommandId::Week },
    Command { label: "Go to Month", id: CommandId::Month },
    Command { label: "Keyboard Shortcuts", id: CommandId::ShowShortcuts },
    // Time-blocking (docs/planning/time-blocking.md §8.4). Last, so they
    // never change which entry a shorter, older query matches first.
    Command { label: "Plan Times", id: CommandId::PlanTimes },
    Command { label: "Clear Auto Blocks", id: CommandId::ClearAutoBlocks },
];

/// Case-insensitive substring filter over `ALL_COMMANDS`, preserving
/// their declared order. Kept free of any egui/UI dependency so the
/// matching behavior itself is unit-tested directly.
pub fn filter_commands(query: &str) -> Vec<&'static Command> {
    let needle = query.trim().to_lowercase();
    ALL_COMMANDS.iter().filter(|c| needle.is_empty() || c.label.to_lowercase().contains(&needle)).collect()
}

/// Case-insensitive substring filter over a list of studyable sets,
/// same shape as `filter_commands` — kept just as free of any
/// egui/UI dependency, for the same reason.
pub fn filter_set_options<'a>(options: &'a [SetOption], query: &str) -> Vec<&'a SetOption> {
    let needle = query.trim().to_lowercase();
    options.iter().filter(|o| needle.is_empty() || o.label.to_lowercase().contains(&needle)).collect()
}

/// Whether `query` should open the `/flashcards [set]` set-picker
/// (feature request: "Navigation Bar Update") instead of the normal
/// command list — the first (and possibly only) word is exactly
/// "flashcards". `"flash"` alone still falls through to the normal
/// list, where it matches "Go to Flashcards" by ordinary substring
/// search; this only triggers once the word is complete.
fn is_flashcards_set_query(query: &str) -> bool {
    let lower = query.trim().to_lowercase();
    lower == "flashcards" || lower.starts_with("flashcards ")
}

/// What the person chose from the palette — either a page-level
/// `Command`, or (from the `/flashcards [set]` picker) a specific set
/// to jump straight into studying. Kept as one return type, rather
/// than two separate optional outputs, so the caller can't
/// accidentally act on both in the same frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteResult {
    Command(CommandId),
    StudySet(SetRef),
}

/// State for the command palette. Opened with `/` (see
/// `app::ReviseApp::handle_keyboard_navigation`), closed with Escape,
/// its own close button, or by picking a command.
#[derive(Default)]
pub struct PaletteState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
}

impl PaletteState {
    pub fn open_fresh(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
    }
}

/// Renders the palette if it's open. Returns what the person picked
/// this frame (Enter, or a click), if anything — applying it is left
/// to the caller, since it means touching `AppState` fields this
/// module has no reason to know about.
///
/// `db` is only used for the `/flashcards [set]` picker (feature
/// request: "Navigation Bar Update"), and only queried once the
/// person has actually typed "flashcards" — not on every keystroke of
/// an unrelated command search.
pub fn show(ctx: &egui::Context, theme: &Theme, db: Option<&Database>, state: &mut PaletteState) -> Option<PaletteResult> {
    if !state.open {
        return None;
    }

    let p = &theme.palette;
    let mut chosen = None;
    let mut modal_open = true;
    let flashcards_mode = is_flashcards_set_query(&state.query);
    let available_sets = if flashcards_mode { db.map(flashcards::available_study_sets).unwrap_or_default() } else { Vec::new() };

    Modal::new("command_palette", "Command").show(ctx, theme, &mut modal_open, |ui| {
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
        // Tab/Shift+Tab and Down/Up arrows are equivalent — arrow
        // keys are the more natural fit for stepping through a list.
        let next_pressed = ui.input(|i| {
            (i.key_pressed(egui::Key::Tab) && !i.modifiers.shift) || i.key_pressed(egui::Key::ArrowDown)
        });
        let prev_pressed = ui.input(|i| {
            (i.key_pressed(egui::Key::Tab) && i.modifiers.shift) || i.key_pressed(egui::Key::ArrowUp)
        });

        Input::new(&mut state.query, "Type a command… (try \"flashcards\")").id("command_palette_query").show(ui, theme);
        ui.add_space(Spacing::SM);

        if flashcards_mode {
            let set_query = state.query.trim().strip_prefix("flashcards").unwrap_or("").trim();
            let results = filter_set_options(&available_sets, set_query);
            if results.is_empty() {
                ui.label(
                    RichText::new("No flashcard sets yet — import one, or flag a note as a flashcard set.")
                        .font(theme.small_font())
                        .color(p.text_faint),
                );
            } else {
                if state.selected >= results.len() {
                    state.selected = 0;
                }
                if next_pressed {
                    state.selected = (state.selected + 1) % results.len();
                }
                if prev_pressed {
                    state.selected = (state.selected + results.len() - 1) % results.len();
                }

                for (index, option) in results.iter().enumerate() {
                    let is_selected = index == state.selected;
                    let text = RichText::new(&option.label)
                        .font(theme.label_font())
                        .color(if is_selected { p.text } else { p.text_muted });
                    if ui.selectable_label(is_selected, text).clicked() {
                        chosen = Some(PaletteResult::StudySet(option.set_ref));
                    }
                }

                if enter_pressed {
                    chosen = Some(PaletteResult::StudySet(results[state.selected].set_ref));
                }
            }
        } else {
            let results = filter_commands(&state.query);
            if results.is_empty() {
                ui.label(RichText::new("No matching commands.").font(theme.small_font()).color(p.text_faint));
            } else {
                if state.selected >= results.len() {
                    state.selected = 0;
                }
                if next_pressed {
                    state.selected = (state.selected + 1) % results.len();
                }
                if prev_pressed {
                    state.selected = (state.selected + results.len() - 1) % results.len();
                }

                for (index, command) in results.iter().enumerate() {
                    let is_selected = index == state.selected;
                    let text = RichText::new(command.label)
                        .font(theme.label_font())
                        .color(if is_selected { p.text } else { p.text_muted });
                    if ui.selectable_label(is_selected, text).clicked() {
                        chosen = Some(PaletteResult::Command(command.id));
                    }
                }

                if enter_pressed {
                    chosen = Some(PaletteResult::Command(results[state.selected].id));
                }
            }
        }
    });

    if chosen.is_some() || !modal_open {
        state.open = false;
    }

    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_returns_everything_in_declared_order() {
        let results = filter_commands("");
        assert_eq!(results.len(), ALL_COMMANDS.len());
        assert_eq!(results[0].id, CommandId::GoToDashboard);
    }

    #[test]
    fn filters_case_insensitively_by_substring() {
        let results = filter_commands("new ev");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, CommandId::NewEvent);
    }

    #[test]
    fn no_match_returns_empty() {
        assert!(filter_commands("xyz123").is_empty());
    }

    #[test]
    fn matches_across_multiple_commands() {
        let results = filter_commands("go to");
        assert_eq!(results.len(), 8);
    }

    #[test]
    fn manage_subjects_is_discoverable_by_search() {
        let results = filter_commands("subject");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, CommandId::ManageSubjects);
    }

    #[test]
    fn manage_sources_is_discoverable_by_search() {
        let results = filter_commands("source");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, CommandId::ManageSources);
    }

    #[test]
    fn today_week_month_and_shortcuts_are_discoverable_by_search() {
        assert_eq!(filter_commands("today").first().map(|c| c.id), Some(CommandId::Today));
        assert_eq!(filter_commands("go to week").first().map(|c| c.id), Some(CommandId::Week));
        assert_eq!(filter_commands("month").first().map(|c| c.id), Some(CommandId::Month));
        assert_eq!(filter_commands("shortcut").first().map(|c| c.id), Some(CommandId::ShowShortcuts));
    }

    #[test]
    fn sprint_11_shortcuts_are_all_discoverable() {
        assert_eq!(filter_commands("today").first().map(|c| c.id), Some(CommandId::Today));
        assert_eq!(filter_commands("week").first().map(|c| c.id), Some(CommandId::Week));
        assert_eq!(filter_commands("month").first().map(|c| c.id), Some(CommandId::Month));
        assert_eq!(filter_commands("shortcut").first().map(|c| c.id), Some(CommandId::ShowShortcuts));
    }

    fn sample_sets() -> Vec<SetOption> {
        vec![
            SetOption { set_ref: SetRef::Deck(1), label: "French Vocab (12 cards)".to_owned() },
            SetOption { set_ref: SetRef::Note(2), label: "German Basics (5 cards) — Notes".to_owned() },
        ]
    }


    #[test]
    fn the_time_blocking_commands_are_found_by_name_and_do_not_disturb_older_matches() {
        assert_eq!(filter_commands("plan times").first().map(|c| c.id), Some(CommandId::PlanTimes));
        assert_eq!(filter_commands("clear auto").first().map(|c| c.id), Some(CommandId::ClearAutoBlocks));
        assert_eq!(filter_commands("plan").len(), 1, "no other command matches 'plan'");
        // Queries that existing behaviour relies on still resolve to the same first entry.
        assert_eq!(filter_commands("today").first().map(|c| c.id), Some(CommandId::Today));
        assert_eq!(filter_commands("go to").len(), 8);
    }

    #[test]
    fn is_flashcards_set_query_matches_the_bare_word_and_word_plus_argument() {
        assert!(is_flashcards_set_query("flashcards"));
        assert!(is_flashcards_set_query("Flashcards french"));
        assert!(is_flashcards_set_query("  flashcards  french"));
        assert!(!is_flashcards_set_query("flash"));
        assert!(!is_flashcards_set_query("flashcardsfrench"));
    }

    #[test]
    fn filter_set_options_matches_by_label_substring_case_insensitively() {
        let sets = sample_sets();
        let results = filter_set_options(&sets, "french");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].set_ref, SetRef::Deck(1));
    }

    #[test]
    fn filter_set_options_empty_query_returns_everything() {
        let sets = sample_sets();
        assert_eq!(filter_set_options(&sets, "").len(), 2);
    }
}
