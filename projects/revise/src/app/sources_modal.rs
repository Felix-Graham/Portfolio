use eframe::egui::{self, RichText};

use crate::database::{queries, Database};
use crate::theme::{Spacing, Theme};
use crate::ui::components::{Input, Modal, RButton};

/// State for the global "Manage Sources" modal — direct request: "an
/// API... other revision applications... to be registered and
/// accounted for." Mirrors `SubjectsModalState` closely: a source is a
/// small, shared, named thing much like a subject, just registered
/// from `revise-cli register` more often than from here. Registering
/// from the GUI too (rather than making the CLI the only way in) costs
/// little and means someone doesn't strictly need a terminal open just
/// to set a name aside for a script they'll write later.
#[derive(Default)]
pub struct SourcesModalState {
    pub open: bool,
    pub new_source_name: String,
    pub renaming_id: Option<i64>,
    pub rename_input: String,
    /// Confirmation step before `delete_source` — unlike archiving a
    /// subject, deleting a source also deletes every contribution it
    /// ever logged (see `queries::sources::delete_source`), which
    /// isn't reversible the way un-archiving is, so this gets an
    /// explicit "are you sure" rather than a single click.
    pub confirming_delete_id: Option<i64>,
    pub error: Option<String>,
}

impl SourcesModalState {
    pub fn open_fresh(&mut self) {
        self.open = true;
        self.new_source_name.clear();
        self.renaming_id = None;
        self.rename_input.clear();
        self.confirming_delete_id = None;
        self.error = None;
    }
}

pub fn show(ctx: &egui::Context, theme: &Theme, db: Option<&Database>, state: &mut SourcesModalState) {
    if !state.open {
        return;
    }
    let Some(db) = db else {
        return;
    };

    let p = &theme.palette;
    let mut modal_open = true;
    let sources = queries::sources::list_all_sources(&db.conn).unwrap_or_default();
    let all_contributions = queries::contributions::list_all_contributions(&db.conn).unwrap_or_default();

    let mut create_requested = false;
    let mut rename_requested: Option<i64> = None;
    let mut start_rename: Option<(i64, String)> = None;
    let mut cancel_rename = false;
    let mut delete_requested: Option<i64> = None;

    Modal::new("sources_modal", "Manage Sources").show(ctx, theme, &mut modal_open, |ui| {
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));

        ui.label(
            RichText::new(
                "Sources are external tools — a script, a different app — that log study time into Revise \
                 from the terminal. Register one here or with `revise-cli register <name>`, then have it call \
                 `revise-cli log --source <name> --subject <name> --minutes <n>`. Logged time counts toward \
                 that subject's totals everywhere in the app, same as any other logged session.",
            )
            .font(theme.small_font())
            .color(p.text_faint),
        );
        ui.add_space(Spacing::SM);

        if sources.is_empty() {
            ui.label(RichText::new("No sources registered yet.").font(theme.small_font()).color(p.text_faint));
        }

        for source in &sources {
            ui.horizontal(|ui| {
                if state.renaming_id == Some(source.id) {
                    let response = Input::new(&mut state.rename_input, "Name").show(ui, theme);
                    if (response.lost_focus() && enter_pressed) || RButton::new("Save").show(ui, theme).clicked() {
                        rename_requested = Some(source.id);
                    }
                    if RButton::new("Cancel").show(ui, theme).clicked() {
                        cancel_rename = true;
                    }
                } else {
                    let total_minutes: i64 =
                        all_contributions.iter().filter(|c| c.source_id == source.id).map(|c| c.minutes).sum();
                    ui.label(RichText::new(&source.name).font(theme.label_font()).color(p.text));
                    ui.label(
                        RichText::new(format!("id {} · {}h {}m logged", source.id, total_minutes / 60, total_minutes % 60))
                            .font(theme.small_font())
                            .color(p.text_faint),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if state.confirming_delete_id == Some(source.id) {
                            if ui.small_button("Confirm delete").clicked() {
                                delete_requested = Some(source.id);
                            }
                            if ui.small_button("Cancel").clicked() {
                                state.confirming_delete_id = None;
                            }
                        } else if ui.small_button("Delete").clicked() {
                            state.confirming_delete_id = Some(source.id);
                        }
                        if ui.small_button("Rename").clicked() {
                            start_rename = Some((source.id, source.name.clone()));
                        }
                    });
                }
            });
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
                let add_clicked = RButton::new("Register").primary().show(ui, theme).clicked();
                ui.add_space(Spacing::SM);
                let response =
                    Input::new(&mut state.new_source_name, "New source name…").id("sources_modal_new").show(ui, theme);
                if (response.lost_focus() && enter_pressed) || add_clicked {
                    create_requested = true;
                }
            });
        });
    });

    if let Some((id, name)) = start_rename {
        state.renaming_id = Some(id);
        state.rename_input = name;
        state.confirming_delete_id = None;
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
            match queries::sources::rename_source(&db.conn, id, new_name) {
                Ok(()) => {
                    state.renaming_id = None;
                    state.rename_input.clear();
                    state.error = None;
                }
                Err(err) => state.error = Some(format!("Could not rename (name may already exist): {err}")),
            }
        }
    }
    if let Some(id) = delete_requested {
        let _ = queries::sources::delete_source(&db.conn, id);
        state.confirming_delete_id = None;
    }
    if create_requested {
        let name = state.new_source_name.trim();
        if name.is_empty() {
            state.error = Some("Name can't be empty.".to_owned());
        } else {
            match queries::sources::create_source(&db.conn, name) {
                Ok(_) => {
                    state.new_source_name.clear();
                    state.error = None;
                }
                Err(err) => state.error = Some(format!("Could not register (name may already exist): {err}")),
            }
        }
    }

    if !modal_open {
        state.open = false;
    }
}
