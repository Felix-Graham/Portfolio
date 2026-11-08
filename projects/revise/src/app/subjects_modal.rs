use eframe::egui::{self, RichText};

use crate::database::{queries, Database};
use crate::theme::{Spacing, Theme};
use crate::ui::components::{Input, Modal, RButton};

/// State for the global "Manage Subjects" modal (Sprint 9). Lives at
/// the `AppState` level rather than inside any one view — subjects
/// are shared by events, tasks, and notes alike, so no single view is
/// the right place to own creating/renaming/archiving them.
#[derive(Default)]
pub struct SubjectsModalState {
    pub open: bool,
    pub new_subject_name: String,
    pub renaming_id: Option<i64>,
    pub rename_input: String,
    /// Feature request: "Subject Sub Topics" — which top-level
    /// subject's subtopics panel is currently expanded, if any. Only
    /// one at a time, same "one thing open at once" shape as
    /// `renaming_id`.
    pub expanded_subject_id: Option<i64>,
    /// Scratch field for the "add a subtopic" input, only meaningful
    /// while a subject is expanded.
    pub new_subtopic_name: String,
    pub error: Option<String>,
}

impl SubjectsModalState {
    pub fn open_fresh(&mut self) {
        self.open = true;
        self.new_subject_name.clear();
        self.renaming_id = None;
        self.rename_input.clear();
        self.expanded_subject_id = None;
        self.new_subtopic_name.clear();
        self.error = None;
    }
}

pub fn show(ctx: &egui::Context, theme: &Theme, db: Option<&Database>, state: &mut SubjectsModalState) {
    if !state.open {
        return;
    }
    let Some(db) = db else {
        return;
    };

    let p = &theme.palette;
    let mut modal_open = true;
    let subjects = queries::subjects::list_all_subjects(&db.conn).unwrap_or_default();
    // Feature request: "Subject Sub Topics" — subtopics live nested
    // under their parent's own row (below), not as separate top-level
    // entries in this list. They're still ordinary rows in the same
    // table otherwise, so every *other* subject picker in the app
    // (which just lists every row here) already shows them fine with
    // no changes needed there.
    let top_level: Vec<&crate::subjects::Subject> = subjects.iter().filter(|s| s.parent_subject_id.is_none()).collect();

    let mut create_requested = false;
    let mut create_subtopic_requested: Option<i64> = None;
    let mut rename_requested: Option<i64> = None;
    let mut archive_toggle: Option<(i64, bool)> = None;
    let mut start_rename: Option<(i64, String)> = None;
    let mut cancel_rename = false;
    let mut toggle_expanded: Option<i64> = None;
    // Feature request: "Colour" — "the user should be able to pick
    // the colour of the subject." `ui.color_edit_button_srgba` mutates
    // its `&mut Color32` argument in place as soon as the person picks
    // a new color from its popup, but doesn't itself persist anything —
    // `response.changed()` is what actually says a pick just happened,
    // captured here the same way every other "something happened in
    // this row" outcome in this modal is: as a local, then acted on
    // once outside the `Modal::show` closure.
    let mut color_change_requested: Option<(i64, egui::Color32)> = None;

    Modal::new("subjects_modal", "Manage Subjects").show(ctx, theme, &mut modal_open, |ui| {
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));

        ui.label(
            RichText::new(
                "Subjects are shared across events, tasks, and notes. Archiving keeps existing data intact. \
                 Click a subject to manage its subtopics — a subtopic's logged time counts toward its parent's \
                 total too, and it can be assigned to anything a subject can.",
            )
            .font(theme.small_font())
            .color(p.text_faint),
        );
        ui.add_space(Spacing::SM);

        if top_level.is_empty() {
            ui.label(RichText::new("No subjects yet.").font(theme.small_font()).color(p.text_faint));
        }

        for subject in &top_level {
            ui.horizontal(|ui| {
                if state.renaming_id == Some(subject.id) {
                    let response = Input::new(&mut state.rename_input, "Name").show(ui, theme);
                    if (response.lost_focus() && enter_pressed) || RButton::new("Save").show(ui, theme).clicked() {
                        rename_requested = Some(subject.id);
                    }
                    if RButton::new("Cancel").show(ui, theme).clicked() {
                        cancel_rename = true;
                    }
                } else {
                    let label =
                        if subject.archived { format!("{} (archived)", subject.name) } else { subject.name.clone() };
                    let is_expanded = state.expanded_subject_id == Some(subject.id);
                    // Clickable, rather than a plain label — this is
                    // the "clicking on a subject should show a list
                    // of subtopics" the request asked for.
                    if ui
                        .selectable_label(is_expanded, RichText::new(label).font(theme.label_font()).color(p.text))
                        .clicked()
                    {
                        toggle_expanded = Some(subject.id);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let archive_label = if subject.archived { "Unarchive" } else { "Archive" };
                        if ui.small_button(archive_label).clicked() {
                            archive_toggle = Some((subject.id, !subject.archived));
                        }
                        if ui.small_button("Rename").clicked() {
                            start_rename = Some((subject.id, subject.name.clone()));
                        }
                        let mut color = crate::theme::subject_color(&subjects, subject.id);
                        if ui.color_edit_button_srgba(&mut color).changed() {
                            color_change_requested = Some((subject.id, color));
                        }
                    });
                }
            });

            if state.expanded_subject_id == Some(subject.id) {
                ui.indent(("subtopics", subject.id), |ui| {
                    ui.add_space(Spacing::XS);
                    let subtopics: Vec<&crate::subjects::Subject> =
                        subjects.iter().filter(|s| s.parent_subject_id == Some(subject.id)).collect();

                    if subtopics.is_empty() {
                        ui.label(RichText::new("No subtopics yet.").font(theme.small_font()).color(p.text_faint));
                    }

                    for subtopic in &subtopics {
                        ui.horizontal(|ui| {
                            if state.renaming_id == Some(subtopic.id) {
                                let response = Input::new(&mut state.rename_input, "Name").show(ui, theme);
                                if (response.lost_focus() && enter_pressed)
                                    || RButton::new("Save").show(ui, theme).clicked()
                                {
                                    rename_requested = Some(subtopic.id);
                                }
                                if RButton::new("Cancel").show(ui, theme).clicked() {
                                    cancel_rename = true;
                                }
                            } else {
                                let label = if subtopic.archived {
                                    format!("{} (archived)", subtopic.name)
                                } else {
                                    subtopic.name.clone()
                                };
                                ui.label(RichText::new(label).font(theme.small_font()).color(p.text_muted));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let archive_label = if subtopic.archived { "Unarchive" } else { "Archive" };
                                    if ui.small_button(archive_label).clicked() {
                                        archive_toggle = Some((subtopic.id, !subtopic.archived));
                                    }
                                    if ui.small_button("Rename").clicked() {
                                        start_rename = Some((subtopic.id, subtopic.name.clone()));
                                    }
                                    let mut color = crate::theme::subject_color(&subjects, subtopic.id);
                                    if ui.color_edit_button_srgba(&mut color).changed() {
                                        color_change_requested = Some((subtopic.id, color));
                                    }
                                });
                            }
                        });
                    }

                    ui.add_space(Spacing::XS);
                    ui.horizontal(|ui| {
                        Input::new(&mut state.new_subtopic_name, "New subtopic name…").show(ui, theme);
                        if RButton::new("Add subtopic").show(ui, theme).clicked() {
                            create_subtopic_requested = Some(subject.id);
                        }
                    });
                    ui.add_space(Spacing::XS);
                });
            }
        }

        if let Some(error) = &state.error {
            ui.add_space(Spacing::SM);
            ui.label(RichText::new(error).font(theme.small_font()).color(p.text));
        }

        ui.add_space(Spacing::MD);
        ui.separator();
        ui.add_space(Spacing::SM);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let add_clicked = RButton::new("Add").primary().show(ui, theme).clicked();
                ui.add_space(Spacing::SM);
                let response = Input::new(&mut state.new_subject_name, "New subject name…")
                    .id("subjects_modal_new")
                    .show(ui, theme);
                if (response.lost_focus() && enter_pressed) || add_clicked {
                    create_requested = true;
                }
            });
        });
    });

    if let Some(id) = toggle_expanded {
        state.expanded_subject_id = if state.expanded_subject_id == Some(id) { None } else { Some(id) };
        state.new_subtopic_name.clear();
    }
    if let Some((id, name)) = start_rename {
        state.renaming_id = Some(id);
        state.rename_input = name;
        state.error = None;
    }
    if cancel_rename {
        state.renaming_id = None;
        state.rename_input.clear();
    }
    if let Some(id) = rename_requested {
        let new_name = state.rename_input.trim();
        if new_name.is_empty() {
            state.error = Some("Name can't be empty.".to_owned());
        } else {
            match queries::subjects::rename_subject(&db.conn, id, new_name) {
                Ok(()) => {
                    state.renaming_id = None;
                    state.rename_input.clear();
                    state.error = None;
                }
                Err(err) => state.error = Some(format!("Could not rename (name may already exist): {err}")),
            }
        }
    }
    if let Some((id, archived)) = archive_toggle {
        let _ = queries::subjects::set_archived(&db.conn, id, archived);
    }
    if let Some((id, color)) = color_change_requested {
        let _ = queries::subjects::set_color(&db.conn, id, &crate::theme::color_to_hex(color));
    }
    if create_requested {
        let name = state.new_subject_name.trim();
        if name.is_empty() {
            state.error = Some("Name can't be empty.".to_owned());
        } else {
            match queries::subjects::create_subject(&db.conn, name) {
                Ok(_) => {
                    state.new_subject_name.clear();
                    state.error = None;
                }
                Err(err) => state.error = Some(format!("Could not create (name may already exist): {err}")),
            }
        }
    }
    if let Some(parent_id) = create_subtopic_requested {
        let name = state.new_subtopic_name.trim();
        if name.is_empty() {
            state.error = Some("Name can't be empty.".to_owned());
        } else {
            match queries::subjects::create_subtopic(&db.conn, parent_id, name) {
                Ok(_) => {
                    state.new_subtopic_name.clear();
                    state.error = None;
                }
                Err(err) => state.error = Some(format!("Could not create (name may already exist): {err}")),
            }
        }
    }

    if !modal_open {
        state.open = false;
    }
}
