use std::path::{Path, PathBuf};

use eframe::egui::{self, Key, RichText};

use crate::database::{queries, Database};
use crate::flashcards::{self, Flashcard};
use crate::notes::Note;
use crate::theme::{Spacing, Theme};
use crate::ui::components::{Input, Panel, RButton};

/// In-progress study session over one set's cards — either an
/// imported deck or a flashcard-set Note (feature request: "Built-In
/// Flashcard Editor"), both resolved to the same shape by
/// `start_study_session` so the player itself doesn't need to know
/// which kind it's showing. Created when "Study" is clicked, or a set
/// is picked from the command palette's `/flashcards [set]`; cleared
/// on "End session" — the deck list is hidden while this is `Some`,
/// same shape as a Pomodoro focus session taking over its own view.
pub struct PlayerState {
    pub deck_title: String,
    pub cards: Vec<Flashcard>,
    pub index: usize,
    pub revealed: bool,
    /// Local, resets every session — separate from the durable
    /// all-time "Flashcards seen" total the Dashboard shows.
    pub seen_this_session: usize,
}

/// UI-only state: the import field, any import error, and the
/// current study session if one is open. Decks and cards are queried
/// fresh every render, same as every other view since Sprint 4.
#[derive(Default)]
pub struct FlashcardsState {
    pub import_path: String,
    pub import_error: Option<String>,
    pub player: Option<PlayerState>,
}

/// A reference to something studyable, resolved lazily so listing
/// available sets (cheap: id + label) stays separate from loading a
/// set's full cards (only needed once one is actually picked). Used
/// by both the deck list's "Study" button and the command palette's
/// `/flashcards [set]` picker (feature request: "Navigation Bar
/// Update") — the two ways into a study session share one pipeline
/// (`start_study_session`) rather than each building a `PlayerState`
/// its own way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetRef {
    Deck(i64),
    Note(i64),
}

/// One entry in the palette's `/flashcards [set]` picker.
#[derive(Debug, Clone)]
pub struct SetOption {
    pub set_ref: SetRef,
    pub label: String,
}

/// Every studyable set: imported decks plus flashcard-set Notes, most
/// recent within each source first. A plain `Vec` rather than
/// anything paged/lazy — this app's data volume (tens of decks/notes
/// at most) doesn't justify it, same reasoning as every other
/// "query fresh, don't cache" list in the app.
pub fn available_study_sets(db: &Database) -> Vec<SetOption> {
    let mut options = Vec::new();

    for deck in queries::flashcards::list_all_decks(&db.conn).unwrap_or_default() {
        let count = queries::flashcards::count_cards(&db.conn, deck.id).unwrap_or(0);
        options.push(SetOption {
            set_ref: SetRef::Deck(deck.id),
            label: format!("{} ({count} card{})", deck.title, if count == 1 { "" } else { "s" }),
        });
    }

    for note in queries::notes::list_flashcard_set_notes(&db.conn).unwrap_or_default() {
        let count = flashcards::parse_pairs(&note.content).len();
        options.push(SetOption {
            set_ref: SetRef::Note(note.id),
            label: format!("{} ({count} card{}) — Notes", note.title, if count == 1 { "" } else { "s" }),
        });
    }

    options
}

/// Builds a fresh study session for a picked set. Returns `None` if
/// it no longer exists or has no cards — e.g. deleted, or (for a
/// Note) edited down to nothing, between listing it and picking it.
pub fn start_study_session(db: &Database, set_ref: SetRef) -> Option<PlayerState> {
    match set_ref {
        SetRef::Deck(id) => {
            let deck = queries::flashcards::get_deck(&db.conn, id).ok().flatten()?;
            let cards = queries::flashcards::list_cards_for_deck(&db.conn, id).ok()?;
            if cards.is_empty() {
                return None;
            }
            Some(PlayerState { deck_title: deck.title, cards, index: 0, revealed: false, seen_this_session: 0 })
        }
        SetRef::Note(id) => {
            let note: Note = queries::notes::get_note(&db.conn, id).ok().flatten()?;
            let pairs = flashcards::parse_pairs(&note.content);
            if pairs.is_empty() {
                return None;
            }
            let cards = pairs
                .into_iter()
                .enumerate()
                .map(|(position, (question, answer))| Flashcard {
                    id: position as i64,
                    deck_id: note.id,
                    question,
                    answer,
                    position: position as i64,
                })
                .collect();
            Some(PlayerState { deck_title: note.title, cards, index: 0, revealed: false, seen_this_session: 0 })
        }
    }
}

/// Flashcards shell. Decks come in by importing a `.txt` file (drag
/// it onto the window, or paste its path); flashcard-set Notes
/// (feature request: "Built-In Flashcard Editor") are created and
/// edited from the Notes view instead, and just show up here
/// alongside imported decks, read-only, ready to study.
pub fn show(ui: &mut egui::Ui, theme: &Theme, db: Option<&Database>, state: &mut FlashcardsState) {
    let p = &theme.palette;

    ui.add_space(Spacing::MD);
    ui.label(RichText::new("FLASHCARDS").font(theme.heading_font()).color(p.text));
    ui.add_space(Spacing::MD);

    let Some(db) = db else {
        return;
    };

    // Native drag-and-drop is built into egui/eframe already — no
    // extra dependency needed for "drop a .txt file here to import".
    let dropped_paths: Vec<PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
    for path in dropped_paths {
        import_from_path(db, state, &path);
    }

    if let Some(player) = &mut state.player {
        let end_session = show_player(ui, theme, db, player);
        if end_session {
            state.player = None;
        }
        return;
    }

    Panel::new().title("Import a deck").show(ui, theme, |ui| {
        ui.label(
            RichText::new("Drag a .txt file here, or paste its path below").font(theme.small_font()).color(p.text_faint),
        );
        ui.add_space(Spacing::SM);
        // Issue: "Button Position Fix" — Input fills all available
        // horizontal space by default, so a button placed after it in
        // the same `ui.horizontal` had nowhere left to go and rendered
        // ~20% off the right edge of the screen. Stacking the button
        // on its own line below (same layout every other form in the
        // app already uses for a text field + its action button)
        // fixes it without needing to hand-compute a narrower width.
        Input::new(&mut state.import_path, "/path/to/deck.txt").id("flashcards_import_path").show(ui, theme);
        ui.add_space(Spacing::SM);
        if RButton::new("Import").primary().show(ui, theme).clicked() {
            let path = state.import_path.trim().to_owned();
            if path.is_empty() {
                state.import_error = Some("Enter a file path, or drag a .txt file above.".to_owned());
            } else {
                import_from_path(db, state, Path::new(&path));
            }
        }
        if let Some(error) = &state.import_error {
            ui.add_space(Spacing::XS);
            ui.label(RichText::new(error).font(theme.small_font()).color(p.text));
        }
    });

    ui.add_space(Spacing::MD);

    let decks = queries::flashcards::list_all_decks(&db.conn).unwrap_or_default();
    let note_sets = queries::notes::list_flashcard_set_notes(&db.conn).unwrap_or_default();
    if decks.is_empty() && note_sets.is_empty() {
        ui.label(
            RichText::new("No sets yet — import a .txt file above, or flag a note as a flashcard set in Notes.")
                .font(theme.small_font())
                .color(p.text_faint),
        );
    }

    let mut delete_id: Option<i64> = None;
    let mut study_ref: Option<SetRef> = None;

    for deck in &decks {
        Panel::new().title(&deck.title).show(ui, theme, |ui| {
            let count = queries::flashcards::count_cards(&db.conn, deck.id).unwrap_or(0);
            let mut summary = format!("{count} card{}", if count == 1 { "" } else { "s" });
            if let Some(source) = &deck.source_name {
                summary.push_str(&format!(" — from {source}"));
            }
            ui.label(RichText::new(summary).font(theme.small_font()).color(p.text_faint));
            ui.add_space(Spacing::XS);
            ui.horizontal(|ui| {
                if count > 0 && RButton::new("Study").primary().show(ui, theme).clicked() {
                    study_ref = Some(SetRef::Deck(deck.id));
                }
                if ui.small_button("Delete").clicked() {
                    delete_id = Some(deck.id);
                }
            });
        });
        ui.add_space(Spacing::SM);
    }

    for note in &note_sets {
        let count = flashcards::parse_pairs(&note.content).len();
        Panel::new().title(&note.title).show(ui, theme, |ui| {
            ui.label(
                RichText::new(format!("{count} card{} — from Notes", if count == 1 { "" } else { "s" }))
                    .font(theme.small_font())
                    .color(p.text_faint),
            );
            ui.add_space(Spacing::XS);
            // No Delete here — a flashcard-set Note is edited and
            // removed from the Notes view, its actual owner; this
            // view only ever reads it.
            if count > 0 && RButton::new("Study").primary().show(ui, theme).clicked() {
                study_ref = Some(SetRef::Note(note.id));
            }
        });
        ui.add_space(Spacing::SM);
    }

    if let Some(id) = delete_id {
        let _ = queries::flashcards::delete_deck(&db.conn, id);
    }
    if let Some(set_ref) = study_ref {
        if let Some(player) = start_study_session(db, set_ref) {
            state.player = Some(player);
        }
    }
}

/// Reads and parses the file at `path`, saving it as a new deck on
/// success. Failures (unreadable file, bad format) are surfaced via
/// `state.import_error` rather than a panic or a silent no-op — this
/// is very likely a hand-edited file, and the message should say
/// which of the two went wrong.
fn import_from_path(db: &Database, state: &mut FlashcardsState, path: &Path) {
    state.import_error = None;

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) => {
            state.import_error = Some(format!("Could not read {}: {err}", path.display()));
            return;
        }
    };

    match flashcards::parse_deck_text(&content) {
        Ok((title, pairs)) => {
            let source_name = path.file_name().map(|name| name.to_string_lossy().into_owned());
            match queries::flashcards::import_deck(&db.conn, &title, source_name.as_deref(), &pairs) {
                Ok(_) => state.import_path.clear(),
                Err(err) => state.import_error = Some(format!("Could not save deck: {err}")),
            }
        }
        Err(err) => state.import_error = Some(err.to_string()),
    }
}

/// Marks the current card seen and reveals its answer — shared by the
/// "Show answer" button and every keyboard shortcut that can trigger
/// a reveal, so "seen" is logged in exactly one place. A no-op if
/// already revealed, since re-revealing an already-shown answer isn't
/// a second "seeing" of it.
fn reveal_answer(db: &Database, player: &mut PlayerState) {
    if player.revealed {
        return;
    }
    player.revealed = true;
    let _ = queries::flashcards::log_seen(&db.conn, chrono::Local::now().date_naive());
    player.seen_this_session += 1;
}

/// Moves to the next (`forward`) or previous card, wrapping forward
/// from the last card back to the first (there's no natural "end" to
/// a study loop) but not wrapping backward past the first — shared by
/// the Previous/Next buttons and their keyboard equivalents.
fn advance(player: &mut PlayerState, forward: bool) {
    if forward {
        player.index = if player.index + 1 < player.cards.len() { player.index + 1 } else { 0 };
    } else if player.index > 0 {
        player.index -= 1;
    }
    player.revealed = false;
}

/// Renders the active study session. Returns `true` if the person
/// asked to end it (the caller clears `state.player`, rather than
/// this function touching `FlashcardsState` directly, to keep the
/// "which session is active" ownership in one place).
///
/// Feature request: "Flashcard Keyboard Shortcuts" — `Tab` for the
/// next card, `→`/`←` reveal-then-advance/un-reveal-then-retreat,
/// `Enter`/`Space` flip in place, `Esc` ends the session. These only
/// fire while a session is active and nothing is focused (there's no
/// text field in the player, so that's normally true anyway; the
/// check is just consistency with how `ui::notes`'s own list-arrow-key
/// handling guards itself). To keep `Tab`/`Esc` from *also* triggering
/// the global section-cycle/palette shortcuts in the same keypress,
/// `ReviseApp::handle_keyboard_navigation` skips its own handling
/// entirely whenever a session is active — see the check there.
fn show_player(ui: &mut egui::Ui, theme: &Theme, db: &Database, player: &mut PlayerState) -> bool {
    let p = &theme.palette;
    let mut end_session = false;

    if ui.ctx().memory(|m| m.focused().is_none()) {
        ui.ctx().input(|input| {
            if input.key_pressed(Key::Escape) {
                end_session = true;
            }
            if input.key_pressed(Key::Enter) || input.key_pressed(Key::Space) {
                if player.revealed {
                    player.revealed = false;
                } else {
                    reveal_answer(db, player);
                }
            }
            if input.key_pressed(Key::Tab) {
                advance(player, true);
            }
            if input.key_pressed(Key::ArrowRight) {
                if player.revealed {
                    advance(player, true);
                } else {
                    reveal_answer(db, player);
                }
            }
            if input.key_pressed(Key::ArrowLeft) {
                if player.revealed {
                    player.revealed = false;
                } else {
                    advance(player, false);
                }
            }
        });
    }

    ui.horizontal(|ui| {
        ui.label(RichText::new(&player.deck_title).font(theme.label_font()).color(p.text_muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("End session").clicked() {
                end_session = true;
            }
        });
    });
    ui.add_space(Spacing::SM);

    Panel::new().show(ui, theme, |ui| {
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(format!("Card {} / {}", player.index + 1, player.cards.len()))
                    .font(theme.small_font())
                    .color(p.text_faint),
            );
            ui.add_space(Spacing::MD);

            let card = &player.cards[player.index];
            ui.label(RichText::new(&card.question).font(theme.heading_font()).color(p.text));
            ui.add_space(Spacing::MD);

            if player.revealed {
                ui.separator();
                ui.add_space(Spacing::SM);
                ui.label(RichText::new(&card.answer).font(theme.label_font()).color(p.text_muted));
            } else if RButton::new("Show answer").primary().show(ui, theme).clicked() {
                reveal_answer(db, player);
            }

            ui.add_space(Spacing::LG);
            ui.horizontal(|ui| {
                if RButton::new("Previous").show(ui, theme).clicked() {
                    advance(player, false);
                }
                ui.add_space(Spacing::SM);
                if RButton::new("Next").show(ui, theme).clicked() {
                    advance(player, true);
                }
                ui.add_space(Spacing::SM);
                if ui.small_button("Shuffle").clicked() {
                    let seed = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_nanos() as u64)
                        .unwrap_or(1);
                    let order = flashcards::shuffled_order(player.cards.len(), seed);
                    let cards = std::mem::take(&mut player.cards);
                    player.cards = order.into_iter().map(|i| cards[i].clone()).collect();
                    player.index = 0;
                    player.revealed = false;
                }
            });

            ui.add_space(Spacing::SM);
            ui.label(
                RichText::new(format!("Seen this session: {}", player.seen_this_session))
                    .font(theme.small_font())
                    .color(p.text_faint),
            );
            ui.add_space(Spacing::SM);
            ui.label(
                RichText::new("Tab next · ← → reveal/next · Enter/Space flip · Esc end")
                    .font(theme.small_font())
                    .color(p.text_faint),
            );
        });
    });

    end_session
}
