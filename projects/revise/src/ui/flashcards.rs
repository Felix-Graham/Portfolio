use std::collections::HashSet;
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
    /// Feature request: "Flashcards As A Grid View" — how the list of
    /// sets is laid out. UI-only, resets each launch (same as
    /// `NotesState::view_mode`).
    pub view_mode: FlashcardsViewMode,
    /// Which sets are expanded in `Mixed` mode. Only meaningful
    /// there; kept across mode switches so flipping away and back
    /// doesn't collapse everything.
    pub expanded: HashSet<SetRef>,
}

/// Feature request: "Flashcards As A Grid View" — "a grid view, a
/// single list (no preview, only title), or a mix of both."
///
/// - `Grid`: several cards per row, each with title, card count,
///   source and a short preview of its first few questions.
/// - `List`: one compact row per set — title and buttons only, no
///   preview.
/// - `Mixed`: the same compact rows as `List`, but each has a
///   "Preview" toggle that opens that set's full grid-style card
///   in place, so a long collection stays scannable while any one
///   set can still be inspected without leaving the list.
///
/// `Grid` is the default: it's the closest to the pre-existing look
/// (a card per set, with its count and source) while adding the
/// preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlashcardsViewMode {
    #[default]
    Grid,
    List,
    Mixed,
}

/// A reference to something studyable, resolved lazily so listing
/// available sets (cheap: id + label) stays separate from loading a
/// set's full cards (only needed once one is actually picked). Used
/// by both the deck list's "Study" button and the command palette's
/// `/flashcards [set]` picker (feature request: "Navigation Bar
/// Update") — the two ways into a study session share one pipeline
/// (`start_study_session`) rather than each building a `PlayerState`
/// its own way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// Feature request: "Startup Questions" — every card the app holds, flattened for
/// the quiz: imported decks and flashcard-set Notes alike, each set's cards tagged
/// with a shared set number so wrong answers can come from the same set.
pub fn load_quiz_pool(db: &Database) -> Vec<flashcards::quiz::QuizCard> {
    use flashcards::quiz::QuizCard;

    let mut pool = Vec::new();
    let mut set = 0;
    for deck in queries::flashcards::list_all_decks(&db.conn).unwrap_or_default() {
        for card in queries::flashcards::list_cards_for_deck(&db.conn, deck.id).unwrap_or_default() {
            pool.push(QuizCard { set, question: card.question, answer: card.answer });
        }
        set += 1;
    }
    for note in queries::notes::list_flashcard_set_notes(&db.conn).unwrap_or_default() {
        for (question, answer) in flashcards::parse_pairs(&note.content) {
            pool.push(QuizCard { set, question, answer });
        }
        set += 1;
    }
    pool
}

/// Builds the quiz shown at launch, or `None` when there isn't one to show: the
/// setting is off, or the stored cards can't make a single question (none, or no
/// card has a different answer to sit beside). Asks for as many questions as the
/// setting says, or as many as the cards allow when that's fewer.
pub fn build_startup_quiz(db: &Database, seed: u64) -> Option<flashcards::quiz::Quiz> {
    if !queries::settings::startup_quiz_enabled(&db.conn).unwrap_or(true) {
        return None;
    }
    let count = queries::settings::startup_quiz_questions(&db.conn).unwrap_or(flashcards::quiz::DEFAULT_QUESTIONS);
    let questions = flashcards::quiz::build_questions(&load_quiz_pool(db), count, seed);
    (!questions.is_empty()).then(|| flashcards::quiz::Quiz::new(questions))
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

    // Feature request: "Flashcards As A Grid View" — a three-way
    // toggle, same `selectable_label` pattern as Notes' Grid/List
    // switch. Added right-to-left, so listed in reverse.
    ui.horizontal(|ui| {
        ui.label(RichText::new("Sets").font(theme.label_font()).color(p.text_muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.selectable_label(state.view_mode == FlashcardsViewMode::Mixed, "Mixed").clicked() {
                state.view_mode = FlashcardsViewMode::Mixed;
            }
            if ui.selectable_label(state.view_mode == FlashcardsViewMode::List, "List").clicked() {
                state.view_mode = FlashcardsViewMode::List;
            }
            if ui.selectable_label(state.view_mode == FlashcardsViewMode::Grid, "Grid").clicked() {
                state.view_mode = FlashcardsViewMode::Grid;
            }
        });
    });
    ui.add_space(Spacing::SM);

    // `List` never shows a preview, so skip the extra queries there.
    let summaries = collect_summaries(db, state.view_mode != FlashcardsViewMode::List);
    if summaries.is_empty() {
        ui.label(
            RichText::new("No sets yet — import a .txt file above, or flag a note as a flashcard set in Notes.")
                .font(theme.small_font())
                .color(p.text_faint),
        );
    }

    let mut actions = SetActions::default();

    match state.view_mode {
        FlashcardsViewMode::Grid => {
            let columns = grid_columns(ui.available_width(), TARGET_CARD_WIDTH, Spacing::SM);
            for row in summaries.chunks(columns) {
                ui.columns(columns, |cols| {
                    for (col_index, set) in row.iter().enumerate() {
                        show_card(&mut cols[col_index], theme, set, &mut actions);
                    }
                });
                ui.add_space(Spacing::SM);
            }
        }
        FlashcardsViewMode::List => {
            for set in &summaries {
                show_row(ui, theme, set, None, &mut actions);
                ui.add_space(Spacing::XS);
            }
        }
        FlashcardsViewMode::Mixed => {
            for set in &summaries {
                let open = state.expanded.contains(&set.set_ref);
                show_row(ui, theme, set, Some(open), &mut actions);
                if open {
                    show_card(ui, theme, set, &mut actions);
                }
                ui.add_space(Spacing::XS);
            }
        }
    }

    if let Some(set_ref) = actions.toggle {
        if !state.expanded.remove(&set_ref) {
            state.expanded.insert(set_ref);
        }
    }
    if let Some(id) = actions.delete {
        let _ = queries::flashcards::delete_deck(&db.conn, id);
        state.expanded.remove(&SetRef::Deck(id));
    }
    if let Some(set_ref) = actions.study {
        if let Some(player) = start_study_session(db, set_ref) {
            state.player = Some(player);
        }
    }
}

/// How many cards' questions a set's preview shows.
const PREVIEW_CARDS: usize = 3;
/// Longest a previewed question gets before it's cut with "…".
const PREVIEW_CHARS: usize = 60;
/// Roughly how wide a grid card wants to be — only decides how many
/// columns fit (see `grid_columns`); the columns themselves then
/// share the real available width, same as Notes' grid.
const TARGET_CARD_WIDTH: f32 = 260.0;

/// One studyable set as the grid/list/mixed layouts see it — decks
/// and flashcard-set Notes flattened into the same shape, so the
/// three layouts only ever render one kind of thing.
struct SetSummary {
    set_ref: SetRef,
    title: String,
    count: usize,
    /// Where it came from, e.g. `"from french.txt"` or `"from Notes"`.
    origin: String,
    /// The first few questions, already truncated. Empty when
    /// previews weren't requested (`List` mode).
    preview: Vec<String>,
}

impl SetSummary {
    fn summary_line(&self) -> String {
        format!("{} — {}", card_count_label(self.count), self.origin)
    }
}

/// What a frame's clicks asked for, gathered while rendering and
/// applied afterward (the same collect-then-apply shape the view
/// already used for `delete_id`/`study_ref`), so nothing mutates the
/// database or state partway through drawing a list of it.
#[derive(Default)]
struct SetActions {
    delete: Option<i64>,
    study: Option<SetRef>,
    toggle: Option<SetRef>,
}

/// `"1 card"` / `"3 cards"`.
fn card_count_label(count: usize) -> String {
    format!("{count} card{}", if count == 1 { "" } else { "s" })
}

/// How many equal-width columns fit in `available_width` when each
/// wants about `target_width` and columns are `gap` apart — always at
/// least one. Pure, so the Grid layout's one piece of arithmetic is
/// unit-tested rather than only visible on screen.
fn grid_columns(available_width: f32, target_width: f32, gap: f32) -> usize {
    ((available_width + gap) / (target_width + gap)).floor().max(1.0) as usize
}

/// `text` cut to at most `max_chars` characters (not bytes — safe on
/// non-ASCII vocab), with a trailing "…" if anything was cut.
fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    cut.push('\u{2026}');
    cut
}

/// Every studyable set — imported decks first, then flashcard-set
/// Notes (same order the view always listed them in) — as
/// `SetSummary`s. `with_preview` controls whether each one's first
/// few questions are loaded.
fn collect_summaries(db: &Database, with_preview: bool) -> Vec<SetSummary> {
    let mut sets = Vec::new();

    for deck in queries::flashcards::list_all_decks(&db.conn).unwrap_or_default() {
        let count = queries::flashcards::count_cards(&db.conn, deck.id).unwrap_or(0) as usize;
        let preview = if with_preview {
            queries::flashcards::list_cards_preview(&db.conn, deck.id, PREVIEW_CARDS as i64)
                .unwrap_or_default()
                .iter()
                .map(|card| truncate_chars(&card.question, PREVIEW_CHARS))
                .collect()
        } else {
            Vec::new()
        };
        let origin = match &deck.source_name {
            Some(source) => format!("from {source}"),
            None => "imported".to_owned(),
        };
        sets.push(SetSummary { set_ref: SetRef::Deck(deck.id), title: deck.title, count, origin, preview });
    }

    for note in queries::notes::list_flashcard_set_notes(&db.conn).unwrap_or_default() {
        let pairs = flashcards::parse_pairs(&note.content);
        let preview = if with_preview {
            pairs.iter().take(PREVIEW_CARDS).map(|(question, _)| truncate_chars(question, PREVIEW_CHARS)).collect()
        } else {
            Vec::new()
        };
        sets.push(SetSummary {
            set_ref: SetRef::Note(note.id),
            title: note.title,
            count: pairs.len(),
            origin: "from Notes".to_owned(),
            preview,
        });
    }

    sets
}

/// A set as a full card: title, count/origin, preview questions, and
/// Study (plus Delete for imported decks — a flashcard-set Note is
/// edited and removed from the Notes view, its actual owner, so it
/// gets no Delete here, as before).
fn show_card(ui: &mut egui::Ui, theme: &Theme, set: &SetSummary, actions: &mut SetActions) {
    let p = &theme.palette;
    Panel::new().title(&set.title).show(ui, theme, |ui| {
        ui.label(RichText::new(set.summary_line()).font(theme.small_font()).color(p.text_faint));
        if !set.preview.is_empty() {
            ui.add_space(Spacing::XS);
            for question in &set.preview {
                ui.label(RichText::new(question).font(theme.small_font()).color(p.text_muted));
            }
            if set.count > set.preview.len() {
                ui.label(
                    RichText::new(format!("+ {} more", set.count - set.preview.len()))
                        .font(theme.small_font())
                        .color(p.text_faint),
                );
            }
        }
        ui.add_space(Spacing::XS);
        ui.horizontal(|ui| {
            if set.count > 0 && RButton::new("Study").primary().show(ui, theme).clicked() {
                actions.study = Some(set.set_ref);
            }
            if let SetRef::Deck(id) = set.set_ref {
                if ui.small_button("Delete").clicked() {
                    actions.delete = Some(id);
                }
            }
        });
    });
}

/// A set as one compact row: title only, plus its buttons on the
/// right. `expanded` is `None` in `List` mode (no toggle); in `Mixed`
/// it's `Some(is_open)` and adds a Preview/Hide toggle before the
/// title.
fn show_row(ui: &mut egui::Ui, theme: &Theme, set: &SetSummary, expanded: Option<bool>, actions: &mut SetActions) {
    let p = &theme.palette;
    ui.horizontal(|ui| {
        if let Some(is_open) = expanded {
            if ui.small_button(if is_open { "Hide" } else { "Preview" }).clicked() {
                actions.toggle = Some(set.set_ref);
            }
        }
        ui.label(RichText::new(&set.title).font(theme.label_font()).color(p.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if set.count > 0 && RButton::new("Study").primary().show(ui, theme).clicked() {
                actions.study = Some(set.set_ref);
            }
            if let SetRef::Deck(id) = set.set_ref {
                if ui.small_button("Delete").clicked() {
                    actions.delete = Some(id);
                }
            }
        });
    });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_columns_fits_as_many_target_width_cards_as_the_width_allows() {
        assert_eq!(grid_columns(800.0, 260.0, 8.0), 3);
        assert_eq!(grid_columns(536.0, 260.0, 8.0), 2);
    }

    #[test]
    fn grid_columns_is_never_less_than_one() {
        assert_eq!(grid_columns(100.0, 260.0, 8.0), 1);
        assert_eq!(grid_columns(0.0, 260.0, 8.0), 1);
        assert_eq!(grid_columns(f32::NAN, 260.0, 8.0), 1);
    }

    #[test]
    fn truncate_chars_leaves_short_text_alone() {
        assert_eq!(truncate_chars("le chat", 60), "le chat");
        assert_eq!(truncate_chars("abcde", 5), "abcde");
    }

    #[test]
    fn truncate_chars_cuts_long_text_with_an_ellipsis_within_the_limit() {
        let cut = truncate_chars("abcdefghij", 5);
        assert_eq!(cut, "abcd\u{2026}");
        assert_eq!(cut.chars().count(), 5);
    }

    #[test]
    fn truncate_chars_counts_characters_not_bytes() {
        // Multi-byte characters must not be split mid-character.
        assert_eq!(truncate_chars("ééééé", 5), "ééééé");
        assert_eq!(truncate_chars("éééééé", 5), "éééé\u{2026}");
    }

    #[test]
    fn card_count_label_pluralises() {
        assert_eq!(card_count_label(0), "0 cards");
        assert_eq!(card_count_label(1), "1 card");
        assert_eq!(card_count_label(2), "2 cards");
    }

    fn db_with_deck() -> Database {
        let db = Database::open_in_memory().expect("open in-memory db");
        let pairs: Vec<(String, String)> = (1..=5).map(|n| (format!("q{n}"), format!("a{n}"))).collect();
        queries::flashcards::import_deck(&db.conn, "Vocab", Some("vocab.txt"), &pairs).expect("import");
        db
    }

    #[test]
    fn collect_summaries_previews_only_the_first_few_questions() {
        let db = db_with_deck();
        let sets = collect_summaries(&db, true);
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].title, "Vocab");
        assert_eq!(sets[0].count, 5);
        assert_eq!(sets[0].origin, "from vocab.txt");
        assert_eq!(sets[0].preview, vec!["q1", "q2", "q3"]);
    }

    #[test]
    fn collect_summaries_skips_previews_when_not_requested() {
        let db = db_with_deck();
        let sets = collect_summaries(&db, false);
        assert_eq!(sets[0].count, 5);
        assert!(sets[0].preview.is_empty());
    }

    #[test]
    fn view_mode_defaults_to_grid() {
        assert_eq!(FlashcardsState::default().view_mode, FlashcardsViewMode::Grid);
    }

    // --- Startup questions ---------------------------------------------------

    fn deck_with(db: &Database, name: &str, cards: &[(&str, &str)]) {
        let pairs: Vec<(String, String)> = cards.iter().map(|(q, a)| ((*q).to_owned(), (*a).to_owned())).collect();
        queries::flashcards::import_deck(&db.conn, name, None, &pairs).expect("deck");
    }

    fn numbered_deck(db: &Database, n: usize) {
        let cards: Vec<(String, String)> = (0..n).map(|i| (format!("q{i}"), format!("a{i}"))).collect();
        let refs: Vec<(&str, &str)> = cards.iter().map(|(q, a)| (q.as_str(), a.as_str())).collect();
        deck_with(db, "Big deck", &refs);
    }

    #[test]
    fn no_flashcards_means_no_startup_quiz() {
        let db = Database::open_in_memory().expect("db");
        assert!(build_startup_quiz(&db, 1).is_none());
    }

    #[test]
    fn the_startup_quiz_asks_five_questions_by_default() {
        let db = Database::open_in_memory().expect("db");
        numbered_deck(&db, 12);
        assert_eq!(build_startup_quiz(&db, 1).expect("a quiz").total(), 5);
    }

    #[test]
    fn the_startup_quiz_follows_the_question_count_setting() {
        let db = Database::open_in_memory().expect("db");
        numbered_deck(&db, 12);
        queries::settings::set_startup_quiz_questions(&db.conn, 9).expect("set");
        assert_eq!(build_startup_quiz(&db, 1).expect("a quiz").total(), 9);
    }

    #[test]
    fn the_startup_quiz_asks_fewer_when_there_are_not_enough_cards() {
        let db = Database::open_in_memory().expect("db");
        deck_with(&db, "Tiny", &[("one", "1"), ("two", "2"), ("three", "3")]);
        assert_eq!(build_startup_quiz(&db, 1).expect("a quiz").total(), 3);
    }

    #[test]
    fn the_setting_can_turn_the_startup_quiz_off() {
        let db = Database::open_in_memory().expect("db");
        numbered_deck(&db, 12);
        queries::settings::set_startup_quiz_enabled(&db.conn, false).expect("set");
        assert!(build_startup_quiz(&db, 1).is_none());
    }

    #[test]
    fn the_pool_holds_imported_decks_and_flashcard_set_notes_alike() {
        let db = Database::open_in_memory().expect("db");
        deck_with(&db, "Deck", &[("deck q", "deck a"), ("deck q2", "deck a2")]);
        let note = queries::notes::create_note(&db.conn, "Set", "note q\nnote a\nnote q2\nnote a2", None, None, None)
            .expect("note");
        queries::notes::set_is_flashcard_set(&db.conn, note.id, true).expect("flag");
        let pool = load_quiz_pool(&db);
        let questions: HashSet<_> = pool.iter().map(|c| c.question.as_str()).collect();
        assert!(questions.contains("deck q") && questions.contains("note q"), "{questions:?}");
        let sets: HashSet<_> = pool.iter().map(|c| c.set).collect();
        assert_eq!(sets.len(), 2, "each set gets its own number");
    }

    #[test]
    fn an_ordinary_note_is_not_quiz_material() {
        let db = Database::open_in_memory().expect("db");
        queries::notes::create_note(&db.conn, "Diary", "line one\nline two", None, None, None).expect("note");
        assert!(load_quiz_pool(&db).is_empty());
    }
}
