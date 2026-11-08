use eframe::egui::{self, Color32, RichText};

use crate::database::{queries, Database};
use crate::events::Event;
use crate::folders::Folder;
use crate::notes::{search_notes, Note};
use crate::subjects::Subject;
use crate::theme::{Spacing, Theme};
use crate::ui::components::{Input, Modal, Panel, RButton};

/// A small, curated set of muted swatches for folder colours — rather
/// than a full RGB picker. Sprint 1's design thesis is explicitly "no
/// color accents" in an otherwise black/white/gray app; a handful of
/// desaturated tones lets folders stay visually distinguishable
/// without breaking that language the way a vivid color wheel would.
const FOLDER_SWATCHES: &[(&str, &str)] =
    &[("Gray", "#5A5A5A"), ("Slate", "#6E7681"), ("Sand", "#8A7A5C"), ("Sage", "#5C7A63"), ("Clay", "#8A5C5C"), ("Steel", "#5C6E8A")];

fn hex_to_color32(hex: &str) -> Color32 {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return Color32::GRAY;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0x5A);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0x5A);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0x5A);
    Color32::from_rgb(r, g, b)
}

/// Which subset of notes the folder tab row is narrowed to. Distinct
/// from a plain `Option<i64>` (as `subject_filter` uses) because
/// folders need a third state beyond "all" / "this one": "Unfiled" —
/// notes with no folder at all — which isn't expressible as "some
/// particular folder id".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FolderFilter {
    #[default]
    All,
    Unfiled,
    Folder(i64),
}

/// Feature request: "grid view and list view" for Notes. `List` (the
/// pre-existing, only look before this request) keeps each note as a
/// full-width `Panel` card, one per row; `Grid` lays the same cards
/// out in several fixed-width columns per row instead, computed from
/// the available width — see `grid_columns`. Kept as its own small
/// enum rather than a `bool` (`is_grid`) so the render code and any
/// future third layout read as a real choice rather than a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotesViewMode {
    #[default]
    List,
    Grid,
}

/// In-progress fields for the create/edit note modal. `id: None`
/// means "creating"; `Some(id)` means "editing that note".
pub struct NoteEditState {
    pub id: Option<i64>,
    pub title: String,
    pub content: String,
    pub subject_id: Option<i64>,
    pub event_id: Option<i64>,
    pub folder_id: Option<i64>,
    /// Feature request: "Built-In Flashcard Editor" — when set,
    /// `content` is read as flashcard pairs
    /// (`flashcards::import::parse_pairs`) and `folder_id` is
    /// constrained to a subject subfolder under the Flashcards root
    /// rather than any ordinary top-level folder.
    pub is_flashcard_set: bool,
    /// Scratch field for the inline "+ Add subject" action, only used
    /// while `is_flashcard_set` is true.
    pub new_subject_name: String,
    pub error: Option<String>,
}

impl NoteEditState {
    pub(crate) fn new() -> Self {
        Self {
            id: None,
            title: String::new(),
            content: String::new(),
            subject_id: None,
            event_id: None,
            folder_id: None,
            is_flashcard_set: false,
            new_subject_name: String::new(),
            error: None,
        }
    }

    fn from_note(note: &Note) -> Self {
        Self {
            id: Some(note.id),
            title: note.title.clone(),
            content: note.content.clone(),
            subject_id: note.subject_id,
            event_id: note.event_id,
            folder_id: note.folder_id,
            is_flashcard_set: note.is_flashcard_set,
            new_subject_name: String::new(),
            error: None,
        }
    }
}

/// In-progress fields for the create/edit folder modal (feature
/// request: "Notes Folders"). `id: None` means "creating".
pub struct FolderEditState {
    pub id: Option<i64>,
    pub name: String,
    pub color: String,
    pub error: Option<String>,
}

impl FolderEditState {
    fn new() -> Self {
        Self { id: None, name: String::new(), color: FOLDER_SWATCHES[0].1.to_owned(), error: None }
    }

    fn from_folder(folder: &Folder) -> Self {
        Self { id: Some(folder.id), name: folder.name.clone(), color: folder.color.clone(), error: None }
    }
}

/// UI-only state: search text, subject/folder filters, and any
/// in-progress edit. Notes are queried fresh every render, same as
/// tasks and events since Sprint 4.
#[derive(Default)]
pub struct NotesState {
    pub search: String,
    /// Sprint 9: narrows the list to one subject when set — same
    /// pattern as `CalendarState.subject_filter`.
    pub subject_filter: Option<i64>,
    /// Feature request: "Notes Folders".
    pub folder_filter: FolderFilter,
    /// Feature request: "grid view and list view" for Notes. Defaults
    /// to `List` — the view's pre-existing, only look before this
    /// request — so nobody's Notes view changes layout unless they
    /// explicitly pick Grid.
    pub view_mode: NotesViewMode,
    pub editing: Option<NoteEditState>,
    pub editing_folder: Option<FolderEditState>,
    /// Sprint 11: which note the keyboard cursor is on, if any —
    /// Up/Down move it, Enter opens it for editing. Index into the
    /// currently-displayed (searched + filtered) list.
    pub selected_index: Option<usize>,
}

/// Notes shell. Sprint 1 laid out the view; Sprint 8 wired it to
/// SQLite; Sprint 9 added a real subject (replacing the free-text
/// label) and a subject filter alongside the existing search. This
/// pass adds Notes Folders: a row of folder tabs above the subject
/// filter, and a folder picker in the note edit modal.
pub fn show(ui: &mut egui::Ui, theme: &Theme, db: Option<&Database>, state: &mut NotesState) -> Option<i64> {
    let p = &theme.palette;

    ui.add_space(Spacing::MD);
    ui.horizontal(|ui| {
        ui.label(RichText::new("NOTES").font(theme.heading_font()).color(p.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if RButton::new("New Note").primary().show(ui, theme).clicked() {
                state.editing = Some(NoteEditState::new());
                ui.ctx().memory_mut(|m| m.request_focus(egui::Id::new("note_title")));
            }
        });
    });
    ui.add_space(Spacing::MD);

    Input::new(&mut state.search, "Search notes… (i)").id("notes_search").show(ui, theme);
    ui.add_space(Spacing::SM);

    let Some(db) = db else {
        return None;
    };

    let folders = queries::folders::list_all_folders(&db.conn).unwrap_or_default();
    // Subfolders (currently only ever a flashcard-set note's subject
    // subfolder under the Flashcards root — feature request:
    // "Built-In Flashcard Editor") don't get their own tab here; they
    // aren't a general-purpose organizing tool the way top-level
    // folders are; `folders` (the full, unfiltered list) is still
    // used below wherever a note's *own* folder needs to be looked up
    // by id, since a flashcard-set note's folder can be a subfolder.
    let top_level_folders: Vec<&Folder> = folders.iter().filter(|f| f.parent_id.is_none()).collect();

    // Folder tabs: All / Unfiled / each folder, plus a way to create
    // one. A colored dot rather than a colored label/button keeps the
    // rest of each tab in the app's usual monochrome text, so a
    // folder's "accent" stays a small, deliberate touch rather than
    // recoloring the whole row.
    ui.horizontal_wrapped(|ui| {
        if ui.selectable_label(state.folder_filter == FolderFilter::All, "All").clicked() {
            state.folder_filter = FolderFilter::All;
        }
        if ui.selectable_label(state.folder_filter == FolderFilter::Unfiled, "Unfiled").clicked() {
            state.folder_filter = FolderFilter::Unfiled;
        }
        for folder in &top_level_folders {
            ui.horizontal(|ui| {
                let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(dot_rect.center(), 4.0, hex_to_color32(&folder.color));
                if ui.selectable_label(state.folder_filter == FolderFilter::Folder(folder.id), &folder.name).clicked()
                {
                    state.folder_filter = FolderFilter::Folder(folder.id);
                }
            });
        }
        if ui.small_button("+ Folder").clicked() {
            state.editing_folder = Some(FolderEditState::new());
        }
        if let FolderFilter::Folder(id) = state.folder_filter {
            if ui.small_button("Edit folder").clicked() {
                if let Some(folder) = folders.iter().find(|f| f.id == id) {
                    state.editing_folder = Some(FolderEditState::from_folder(folder));
                }
            }
        }
    });
    ui.add_space(Spacing::SM);

    let subjects = queries::subjects::list_all_subjects(&db.conn).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Subject").font(theme.small_font()).color(p.text_faint));
        let current_label = state
            .subject_filter
            .and_then(|id| subjects.iter().find(|s| s.id == id))
            .map(|s| s.name.as_str())
            .unwrap_or("All subjects");
        egui::ComboBox::from_id_salt("notes_subject_filter").selected_text(current_label).show_ui(ui, |ui| {
            ui.selectable_value(&mut state.subject_filter, None, "All subjects");
            for subject in &subjects {
                let label =
                    if subject.archived { format!("{} (archived)", subject.name) } else { subject.name.clone() };
                ui.selectable_value(&mut state.subject_filter, Some(subject.id), label);
            }
        });
        // Feature request: "the choice between a grid view and a list
        // view." A small two-way toggle rather than a `ComboBox` — only
        // ever two options, and `selectable_label` already shows which
        // one is active at a glance, the same way the Repeat List
        // "Order" picker and the folder tabs above do.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.selectable_label(state.view_mode == NotesViewMode::Grid, "Grid").clicked() {
                state.view_mode = NotesViewMode::Grid;
            }
            if ui.selectable_label(state.view_mode == NotesViewMode::List, "List").clicked() {
                state.view_mode = NotesViewMode::List;
            }
        });
    });
    ui.add_space(Spacing::MD);

    let notes = queries::notes::list_all_notes(&db.conn).unwrap_or_default();
    let events = queries::events::list_visible_events(&db.conn).unwrap_or_default();

    let searched = search_notes(&notes, &state.search);
    let by_folder: Vec<&Note> = match state.folder_filter {
        FolderFilter::All => searched,
        FolderFilter::Unfiled => searched.into_iter().filter(|n| n.folder_id.is_none()).collect(),
        FolderFilter::Folder(id) => searched.into_iter().filter(|n| n.folder_id == Some(id)).collect(),
    };
    let results: Vec<&Note> = match state.subject_filter {
        None => by_folder,
        Some(id) => by_folder.into_iter().filter(|n| n.subject_id == Some(id)).collect(),
    };

    if results.is_empty() {
        let message = if state.search.trim().is_empty()
            && state.subject_filter.is_none()
            && state.folder_filter == FolderFilter::All
        {
            "No notes yet."
        } else {
            "No notes match."
        };
        ui.label(RichText::new(message).font(theme.small_font()).color(p.text_faint));
    }

    let mut delete_id: Option<i64> = None;
    let mut edit_id: Option<i64> = None;
    let mut move_to_folder: Option<(i64, Option<i64>)> = None;
    // Feature request: "Flashcard tagged notes should have a study
    // now button." Returned from `show` itself (same shape as
    // `ui::calendar::show`'s own `Option<(i64, NaiveDate)>` "start a
    // session" signal) — actually starting a study session means
    // building a `PlayerState` and switching the active view to
    // Flashcards, neither of which this function has the `AppState`
    // access to do itself.
    let mut study_note_id: Option<i64> = None;

    // Keyboard list navigation (Sprint 11): Up/Down move the
    // selection, Enter opens it for editing — no "Space" action here,
    // unlike To Do, since a note has nothing analogous to
    // "completed" to toggle.
    if !results.is_empty() && ui.ctx().memory(|m| m.focused().is_none()) {
        ui.ctx().input(|input| {
            if input.key_pressed(egui::Key::ArrowDown) {
                state.selected_index = crate::ui::list_selection::move_selection(state.selected_index, 1, results.len());
            }
            if input.key_pressed(egui::Key::ArrowUp) {
                state.selected_index = crate::ui::list_selection::move_selection(state.selected_index, -1, results.len());
            }
            if input.key_pressed(egui::Key::Enter) {
                if let Some(note) = state.selected_index.and_then(|i| results.get(i)) {
                    edit_id = Some(note.id);
                }
            }
        });
    }
    if let Some(index) = state.selected_index {
        if index >= results.len() {
            state.selected_index = if results.is_empty() { None } else { Some(results.len() - 1) };
        }
    }

    let selected_index = state.selected_index;
    // Shared by both layouts below — a note's own card is identical
    // either way, only how many of them share a row differs. Kept as
    // one closure rather than two near-duplicate loops, the same
    // reasoning `ui::calendar`'s `event_block` is factored out for.
    let mut render_note = |ui: &mut egui::Ui, index: usize, note: &Note| {
        let is_selected = selected_index == Some(index);
        let highlight = egui::Frame::NONE.stroke(if is_selected {
            egui::Stroke::new(crate::theme::Border::THICK, p.focus)
        } else {
            egui::Stroke::NONE
        });
        highlight.show(ui, |ui| {
            Panel::new().title(&note.title).show(ui, theme, |ui| {
                ui.horizontal(|ui| {
                    if let Some(folder) = note.folder_id.and_then(|id| folders.iter().find(|f| f.id == id)) {
                        let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                        ui.painter().circle_filled(dot_rect.center(), 4.0, hex_to_color32(&folder.color));
                        ui.label(RichText::new(&folder.name).font(theme.small_font()).color(p.text_faint));
                        ui.add_space(Spacing::SM);
                    }
                    if let Some(subject) = subject_name(&subjects, note.subject_id) {
                        ui.label(RichText::new(subject.to_uppercase()).font(theme.small_font()).color(p.text_faint));
                        ui.add_space(Spacing::SM);
                    }
                    if let Some(event_title) = note.event_id.and_then(|id| event_title(&events, id)) {
                        ui.label(
                            RichText::new(format!("→ {event_title}")).font(theme.small_font()).color(p.text_faint),
                        );
                    }
                });
                let preview = note.preview(160);
                if !preview.is_empty() {
                    ui.label(RichText::new(preview).font(theme.small_font()).color(p.text_muted));
                }
                ui.add_space(Spacing::XS);
                ui.horizontal(|ui| {
                    if ui.small_button("Delete").clicked() {
                        delete_id = Some(note.id);
                    }
                    if ui.small_button("Edit").clicked() {
                        edit_id = Some(note.id);
                    }
                    let move_label = note.folder_id.and_then(|id| folders.iter().find(|f| f.id == id)).map(|f| f.name.as_str()).unwrap_or("Move to folder");
                    egui::ComboBox::from_id_salt(("note_move_folder", note.id)).selected_text(move_label).show_ui(ui, |ui| {
                        let mut selection = note.folder_id;
                        if ui.selectable_value(&mut selection, None, "No folder").clicked() {
                            move_to_folder = Some((note.id, None));
                        }
                        for folder in &folders {
                            if ui.selectable_value(&mut selection, Some(folder.id), &folder.name).clicked() {
                                move_to_folder = Some((note.id, Some(folder.id)));
                            }
                        }
                    });
                });
                // Feature request: "Flashcard tagged notes should have
                // a study now button." On its own row rather than
                // squeezed into the Delete/Edit/move-folder row above
                // — that row is already the busiest one on the card,
                // and this button matters enough (a primary action, in
                // both layouts) to not get crowded there, especially
                // at Grid's narrower card width.
                if note.is_flashcard_set {
                    ui.add_space(Spacing::XS);
                    if RButton::new("Study Now").primary().show(ui, theme).clicked() {
                        study_note_id = Some(note.id);
                    }
                }
            });
        });
    };

    match state.view_mode {
        NotesViewMode::List => {
            for (index, note) in results.iter().copied().enumerate() {
                render_note(ui, index, note);
                ui.add_space(Spacing::SM);
            }
        }
        NotesViewMode::Grid => {
            // Feature request: "grid view" — several notes per row
            // instead of one. `ui.columns` is the same mechanism
            // `ui::calendar`'s day/week grids and `ui::dashboard`'s
            // stat panels already use for exactly this ("split the
            // available width into N equal columns, let each column's
            // content size its own height") rather than something
            // hand-rolled. `TARGET_CARD_WIDTH` only decides how many
            // columns fit — the columns themselves then share
            // whatever width is actually available, same as those
            // other views' columns do.
            const TARGET_CARD_WIDTH: f32 = 260.0;
            let available = ui.available_width();
            let columns = ((available + Spacing::SM) / (TARGET_CARD_WIDTH + Spacing::SM)).floor().max(1.0) as usize;
            for (row_index, row) in results.chunks(columns).enumerate() {
                ui.columns(columns, |cols| {
                    for (col_index, note) in row.iter().copied().enumerate() {
                        let index = row_index * columns + col_index;
                        render_note(&mut cols[col_index], index, note);
                    }
                });
                ui.add_space(Spacing::SM);
            }
        }
    }

    if let Some(id) = delete_id {
        let _ = queries::notes::delete_note(&db.conn, id);
    }
    if let Some(id) = edit_id {
        if let Some(note) = notes.iter().find(|n| n.id == id) {
            state.editing = Some(NoteEditState::from_note(note));
            ui.ctx().memory_mut(|m| m.request_focus(egui::Id::new("note_title")));
        }
    }
    if let Some((note_id, folder_id)) = move_to_folder {
        let _ = queries::notes::set_folder(&db.conn, note_id, folder_id);
    }

    edit_modal(ui, theme, db, state, &events, &subjects, &folders);
    folder_modal(ui, theme, db, state);

    study_note_id
}

fn event_title(events: &[Event], id: i64) -> Option<&str> {
    events.iter().find(|e| e.id == id).map(|e| e.title.as_str())
}

fn subject_name(subjects: &[Subject], id: Option<i64>) -> Option<&str> {
    id.and_then(|id| subjects.iter().find(|s| s.id == id)).map(|s| s.name.as_str())
}

fn edit_modal(
    ui: &mut egui::Ui,
    theme: &Theme,
    db: &Database,
    state: &mut NotesState,
    events: &[Event],
    subjects: &[Subject],
    folders: &[Folder],
) {
    let p = &theme.palette;
    let mut modal_open = state.editing.is_some();
    let mut save_requested = false;
    let mut cancel_requested = false;
    let mut delete_requested = false;
    let mut add_subject_requested: Option<String> = None;

    let title = if matches!(&state.editing, Some(e) if e.id.is_some()) { "Edit Note" } else { "New Note" };

    Modal::new("note_modal", title).show(ui.ctx(), theme, &mut modal_open, |ui| {
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if let Some(edit) = &mut state.editing {
            ui.label(RichText::new("Title").font(theme.small_font()).color(p.text_faint));
            Input::new(&mut edit.title, "Title").id("note_title").show(ui, theme);
            ui.add_space(Spacing::SM);

            // Feature request: "Built-In Flashcard Editor".
            crate::ui::components::checkbox(ui, theme, &mut edit.is_flashcard_set, "This is a flashcard set");
            ui.add_space(Spacing::SM);

            if edit.is_flashcard_set {
                ui.label(RichText::new("Subject").font(theme.small_font()).color(p.text_faint));
                let flashcards_root_id =
                    folders.iter().find(|f| f.parent_id.is_none() && f.name == queries::folders::FLASHCARDS_ROOT_NAME).map(|f| f.id);
                let subject_folders: Vec<&Folder> =
                    folders.iter().filter(|f| flashcards_root_id.is_some() && f.parent_id == flashcards_root_id).collect();
                let current_label = edit
                    .folder_id
                    .and_then(|id| folders.iter().find(|f| f.id == id))
                    .map(|f| f.name.as_str())
                    .unwrap_or("Uncategorized");
                egui::ComboBox::from_id_salt("note_flashcard_subject").selected_text(current_label).show_ui(ui, |ui| {
                    ui.selectable_value(&mut edit.folder_id, None, "Uncategorized");
                    for folder in &subject_folders {
                        ui.selectable_value(&mut edit.folder_id, Some(folder.id), folder.name.clone());
                    }
                });
                ui.add_space(Spacing::XS);
                Input::new(&mut edit.new_subject_name, "New subject name").id("note_new_subject_name").show(ui, theme);
                ui.add_space(Spacing::XS);
                if RButton::new("+ Add subject").show(ui, theme).clicked() && !edit.new_subject_name.trim().is_empty() {
                    add_subject_requested = Some(edit.new_subject_name.trim().to_owned());
                }
                ui.add_space(Spacing::SM);

                let pair_count = crate::flashcards::parse_pairs(&edit.content).len();
                ui.label(
                    RichText::new(format!(
                        "{pair_count} question/answer pair{} detected — one question per line, then its answer on the next line.",
                        if pair_count == 1 { "" } else { "s" }
                    ))
                    .font(theme.small_font())
                    .color(p.text_faint),
                );
            } else {
                ui.label(RichText::new("Folder").font(theme.small_font()).color(p.text_faint));
                let top_level_folders: Vec<&Folder> = folders.iter().filter(|f| f.parent_id.is_none()).collect();
                let current_folder_label = edit
                    .folder_id
                    .and_then(|id| folders.iter().find(|f| f.id == id))
                    .map(|f| f.name.as_str())
                    .unwrap_or("None");
                egui::ComboBox::from_id_salt("note_folder").selected_text(current_folder_label).show_ui(ui, |ui| {
                    ui.selectable_value(&mut edit.folder_id, None, "None");
                    for folder in &top_level_folders {
                        ui.selectable_value(&mut edit.folder_id, Some(folder.id), folder.name.clone());
                    }
                });
            }
            ui.add_space(Spacing::SM);

            ui.label(RichText::new("Subject").font(theme.small_font()).color(p.text_faint));
            let current_label = subject_name(subjects, edit.subject_id).unwrap_or("None");
            egui::ComboBox::from_id_salt("note_subject").selected_text(current_label).show_ui(ui, |ui| {
                ui.selectable_value(&mut edit.subject_id, None, "None");
                for subject in subjects {
                    let label =
                        if subject.archived { format!("{} (archived)", subject.name) } else { subject.name.clone() };
                    ui.selectable_value(&mut edit.subject_id, Some(subject.id), label);
                }
            });
            ui.add_space(Spacing::SM);

            ui.label(RichText::new("Revision session").font(theme.small_font()).color(p.text_faint));
            let current_event_label = edit
                .event_id
                .and_then(|id| event_title(events, id))
                .unwrap_or("None — standalone note");
            egui::ComboBox::from_id_salt("note_event")
                .selected_text(current_event_label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut edit.event_id, None, "None — standalone note");
                    for event in events {
                        let label = format!("{} — {}", event.start_time.format("%d %b %H:%M"), event.title);
                        ui.selectable_value(&mut edit.event_id, Some(event.id), label);
                    }
                });
            ui.add_space(Spacing::SM);

            ui.label(RichText::new("Content").font(theme.small_font()).color(p.text_faint));
            let content_response = Input::new(&mut edit.content, "Write your notes here…").multiline(10).show(ui, theme);

            if let Some(error) = &edit.error {
                ui.add_space(Spacing::SM);
                ui.label(RichText::new(error).font(theme.small_font()).color(p.text));
            }

            ui.add_space(Spacing::MD);
            let save_via_enter = enter_pressed && !content_response.has_focus();
            ui.horizontal(|ui| {
                if RButton::new("Save").primary().show(ui, theme).clicked() || save_via_enter {
                    save_requested = true;
                }
                if RButton::new("Cancel").show(ui, theme).clicked() {
                    cancel_requested = true;
                }
                if edit.id.is_some() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Delete").clicked() {
                            delete_requested = true;
                        }
                    });
                }
            });
        }
    });

    if let Some(name) = add_subject_requested {
        if let Ok(root) = queries::folders::find_or_create_flashcards_root(&db.conn) {
            if let Ok(new_folder) = queries::folders::create_subfolder(&db.conn, &name, &root.color, root.id) {
                if let Some(edit) = &mut state.editing {
                    edit.folder_id = Some(new_folder.id);
                    edit.new_subject_name.clear();
                }
            }
        }
        return;
    }

    if delete_requested {
        if let Some(edit) = &state.editing {
            if let Some(id) = edit.id {
                let _ = queries::notes::delete_note(&db.conn, id);
            }
        }
        state.editing = None;
        return;
    }

    if save_requested {
        try_save_note(db, state);
        return;
    }

    if cancel_requested || !modal_open {
        state.editing = None;
    }
}

/// Create/edit/delete a folder (feature request: "Notes Folders").
/// Deliberately its own small modal rather than folded into the note
/// edit modal — folders are managed from the tab row, independent of
/// any one note being open.
fn folder_modal(ui: &mut egui::Ui, theme: &Theme, db: &Database, state: &mut NotesState) {
    let p = &theme.palette;
    let mut modal_open = state.editing_folder.is_some();
    let mut save_requested = false;
    let mut cancel_requested = false;
    let mut delete_requested = false;

    let title = if matches!(&state.editing_folder, Some(f) if f.id.is_some()) { "Edit Folder" } else { "New Folder" };

    Modal::new("folder_modal", title).show(ui.ctx(), theme, &mut modal_open, |ui| {
        if let Some(edit) = &mut state.editing_folder {
            ui.label(RichText::new("Name").font(theme.small_font()).color(p.text_faint));
            Input::new(&mut edit.name, "Folder name").id("folder_name").show(ui, theme);
            ui.add_space(Spacing::SM);

            ui.label(RichText::new("Colour").font(theme.small_font()).color(p.text_faint));
            ui.horizontal(|ui| {
                for (name, hex) in FOLDER_SWATCHES {
                    let selected = edit.color == *hex;
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
                    ui.painter().circle_filled(rect.center(), 10.0, hex_to_color32(hex));
                    if selected {
                        ui.painter().circle_stroke(rect.center(), 11.0, egui::Stroke::new(2.0, p.focus));
                    }
                    let response = response.on_hover_text(*name);
                    if response.clicked() {
                        edit.color = (*hex).to_owned();
                    }
                }
            });

            if let Some(error) = &edit.error {
                ui.add_space(Spacing::SM);
                ui.label(RichText::new(error).font(theme.small_font()).color(p.text));
            }

            ui.add_space(Spacing::MD);
            ui.horizontal(|ui| {
                if RButton::new("Save").primary().show(ui, theme).clicked() {
                    save_requested = true;
                }
                if RButton::new("Cancel").show(ui, theme).clicked() {
                    cancel_requested = true;
                }
                if edit.id.is_some() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Delete").clicked() {
                            delete_requested = true;
                        }
                    });
                }
            });
        }
    });

    if delete_requested {
        if let Some(edit) = &state.editing_folder {
            if let Some(id) = edit.id {
                let _ = queries::folders::delete_folder(&db.conn, id);
                if state.folder_filter == FolderFilter::Folder(id) {
                    state.folder_filter = FolderFilter::All;
                }
            }
        }
        state.editing_folder = None;
        return;
    }

    if save_requested {
        if let Some(edit) = &mut state.editing_folder {
            if edit.name.trim().is_empty() {
                edit.error = Some("Name can't be empty.".to_owned());
            } else {
                let name = edit.name.trim();
                let result = match edit.id {
                    Some(id) => {
                        queries::folders::rename_folder(&db.conn, id, name)
                            .and_then(|()| queries::folders::recolor_folder(&db.conn, id, &edit.color))
                    }
                    None => queries::folders::create_folder(&db.conn, name, &edit.color).map(|_| ()),
                };
                match result {
                    Ok(()) => state.editing_folder = None,
                    Err(err) => edit.error = Some(format!("Could not save: {err}")),
                }
            }
        }
        return;
    }

    if cancel_requested || !modal_open {
        state.editing_folder = None;
    }
}

fn try_save_note(db: &Database, state: &mut NotesState) {
    let Some(edit) = &mut state.editing else { return };

    if edit.title.trim().is_empty() {
        edit.error = Some("Title can't be empty.".to_owned());
        return;
    }

    let subject_id = edit.subject_id;
    let folder_id = edit.folder_id;
    let event_id = edit.event_id;
    let is_flashcard_set = edit.is_flashcard_set;
    let title = edit.title.trim();
    let content = edit.content.trim();

    let result = match edit.id {
        Some(id) => queries::notes::update_note(&db.conn, id, title, content, subject_id, event_id, folder_id)
            .and_then(|()| queries::notes::set_is_flashcard_set(&db.conn, id, is_flashcard_set)),
        None => queries::notes::create_note(&db.conn, title, content, subject_id, event_id, folder_id)
            .and_then(|note| queries::notes::set_is_flashcard_set(&db.conn, note.id, is_flashcard_set)),
    };

    match result {
        Ok(()) => state.editing = None,
        Err(err) => {
            if let Some(edit) = &mut state.editing {
                edit.error = Some(format!("Could not save: {err}"));
            }
        }
    }
}
