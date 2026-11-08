use eframe::egui::{self, RichText};

use crate::database::{queries, Database};
use crate::events::Event;
use crate::subjects::Subject;
use crate::tasks::Task;
use crate::theme::{Border, Spacing, Theme};
use crate::ui::components::{Input, Modal, Panel, RButton, TaskItem};

/// Fields for the create/edit task modal. `id: None` means
/// "creating". Feature request: "Better To Do Addition" — this is
/// now the *only* way to add a task (no separate quick-add row), so
/// every field a task can have is available up front rather than
/// needing a second "more options" step.
pub struct EditState {
    pub id: Option<i64>,
    pub title: String,
    pub description: String,
    pub due_date: String,
    /// The revision session this task is attached to, if any
    /// (Sprint 4).
    pub event_id: Option<i64>,
    /// The revision subject this task belongs to, if any (Sprint 9).
    pub subject_id: Option<i64>,
    /// Feature request: "Repeating Tasks".
    pub repeat_weekly: bool,
    /// Feature request: "Tasks Repeating By Day" — `1` for daily, `7`
    /// for weekly; meaningless (and hidden) unless `repeat_weekly` is
    /// on, same as `repeat_through_list` below. See
    /// `Task::repeat_interval_days`.
    pub repeat_interval_days: i64,
    /// Feature request: "Tasks Repeating Through A List" — a
    /// sub-option of `repeat_weekly`, revealed only once that's
    /// checked (see `edit_modal`).
    pub repeat_through_list: bool,
    /// Raw textarea contents, one topic per line — parsed on save via
    /// `tasks::repeat_list::parse_topics`.
    pub repeat_list_topics: String,
    pub repeat_list_mode: crate::tasks::repeat_list::RepeatListMode,
    /// Feature request: "Sub Tasks" — restricted to top-level tasks
    /// only (see the picker in `edit_modal`), so nesting never goes
    /// more than one level deep and cycles can't happen.
    pub parent_task_id: Option<i64>,
    /// Feature request: "Task Timings" — free-text minutes, parsed on
    /// save; empty means "no estimate".
    pub estimated_minutes: String,
}

impl EditState {
    fn new() -> Self {
        Self {
            id: None,
            title: String::new(),
            description: String::new(),
            due_date: String::new(),
            event_id: None,
            subject_id: None,
            repeat_weekly: false,
            repeat_interval_days: 7,
            repeat_through_list: false,
            repeat_list_topics: String::new(),
            repeat_list_mode: crate::tasks::repeat_list::RepeatListMode::default(),
            parent_task_id: None,
            estimated_minutes: String::new(),
        }
    }

    fn from_task(task: &Task) -> Self {
        Self {
            id: Some(task.id),
            // A repeat-list task's own `title` is the *derived*
            // "{base}: {topic}" combination (see
            // `queries::tasks::set_repeat_list`) — showing that back
            // in the Title field would let a later save mistake the
            // whole combined string for a fresh base title. Show
            // `repeat_list_base_title` instead whenever it applies;
            // it's exactly what the person actually typed.
            title: if task.repeat_through_list {
                task.repeat_list_base_title.clone().unwrap_or_else(|| task.title.clone())
            } else {
                task.title.clone()
            },
            description: task.description.clone(),
            due_date: task.due_date.clone().unwrap_or_default(),
            event_id: task.event_id,
            subject_id: task.subject_id,
            repeat_weekly: task.repeat_weekly,
            repeat_interval_days: task.repeat_interval_days,
            repeat_through_list: task.repeat_through_list,
            repeat_list_topics: task.repeat_list_topics.clone().unwrap_or_default(),
            repeat_list_mode: task
                .repeat_list_mode
                .as_deref()
                .and_then(crate::tasks::repeat_list::RepeatListMode::parse)
                .unwrap_or_default(),
            parent_task_id: task.parent_task_id,
            estimated_minutes: task.estimated_minutes.map(|m| m.to_string()).unwrap_or_default(),
        }
    }
}

/// UI-only state: no cached tasks/events here. Every render queries
/// the database fresh (see `show` below), which is what makes
/// "completing a task from any view updates all views" true without
/// any manual cache-invalidation code.
#[derive(Default)]
pub struct ToDoState {
    /// Sprint 9: narrows the list to one subject when set — same
    /// pattern as `CalendarState.subject_filter`.
    pub subject_filter: Option<i64>,
    pub editing: Option<EditState>,
    /// Sprint 11: which row the keyboard cursor is on, if any — Up/
    /// Down move it, Space toggles completion, Enter opens Edit.
    /// Index into the currently-displayed top-level (filtered) list —
    /// subtasks aren't part of keyboard navigation, only top-level
    /// tasks are (see `show`'s doc comment on nesting).
    pub selected_index: Option<usize>,
    /// Feature request: "Auto Assigning" — the summary of the last
    /// "Auto-assign" click ("Assigned 5 tasks."), shown under the header until the next click or the
    /// view is left. UI-only. The "Plan times" button reuses it for its own one-line summary.
    pub auto_assign_message: Option<String>,
}

pub fn show(ui: &mut egui::Ui, theme: &Theme, db: Option<&Database>, state: &mut ToDoState) {
    let p = &theme.palette;

    ui.add_space(Spacing::MD);
    let mut auto_assign_requested = false;
    let mut plan_times_requested = false;
    // Feature request: "Time-blocking" — the "Plan times" button only
    // exists once the person has turned time blocking on in Settings.
    let time_blocking_on =
        db.is_some_and(|db| queries::settings::time_blocking_enabled(&db.conn).unwrap_or(false));
    ui.horizontal(|ui| {
        ui.label(RichText::new("TO DO").font(theme.heading_font()).color(p.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Feature request: "Better To Do Addition" — a single,
            // fully-featured "Add Task" entry point (title,
            // description, subject, due date, repeat, subtask parent,
            // estimated time all available immediately) replacing the
            // old two-step quick-add-row-plus-"More options" flow.
            if RButton::new("Add Task").primary().show(ui, theme).clicked() {
                state.editing = Some(EditState::new());
                ui.ctx().memory_mut(|m| m.request_focus(egui::Id::new("edit_task_title")));
            }
            // Feature request: "Auto Assigning" — plans a day for
            // every unassigned task (deadline, daily time budget,
            // and the days fixed work already occupies). The budget,
            // default task length and how far ahead to plan are in
            // Settings.
            if RButton::new("Auto-assign").show(ui, theme).clicked() {
                auto_assign_requested = true;
            }
            // Feature request: "Time-blocking" — after Auto-assign has
            // chosen the days, this places each day's tasks at times
            // in the gaps between that day's events (see
            // `queries::time_blocking::plan_blocks`). The result shows
            // in the Calendar's Day view.
            if time_blocking_on && RButton::new("Plan times").show(ui, theme).clicked() {
                plan_times_requested = true;
            }
        });
    });
    ui.add_space(Spacing::MD);

    let Some(db) = db else {
        return;
    };

    if auto_assign_requested {
        let now = chrono::Local::now();
        state.auto_assign_message = Some(
            match queries::auto_assign::run_at(&db.conn, now.date_naive(), now.naive_local()) {
                Ok(report) => report.summary(),
                Err(err) => format!("Auto-assign failed: {err}"),
            },
        );
    }
    if plan_times_requested {
        let now = chrono::Local::now();
        state.auto_assign_message =
            Some(match queries::time_blocking::plan_blocks(&db.conn, now.date_naive(), now.naive_local()) {
                Ok(report) => report.summary(),
                Err(err) => format!("Plan times failed: {err}"),
            });
    }
    if let Some(message) = &state.auto_assign_message {
        ui.label(RichText::new(message).font(theme.small_font()).color(p.text_muted));
        ui.add_space(Spacing::SM);
    }
    let all_tasks = queries::tasks::list_tasks(&db.conn).unwrap_or_default();
    // Feature request: task "bars" — time spent per task (a parent's bar is its
    // subtasks' together). Worked out on every task, before any subject filter.
    let progress = queries::task_time::progress_for(&db.conn, &all_tasks).unwrap_or_default();
    let events = queries::events::list_visible_events(&db.conn).unwrap_or_default();
    let subjects = queries::subjects::list_all_subjects(&db.conn).unwrap_or_default();

    // --- Subject filter ----------------------------------------------------
    ui.horizontal(|ui| {
        ui.label(RichText::new("Subject").font(theme.small_font()).color(p.text_faint));
        let current_label = state
            .subject_filter
            .and_then(|id| subjects.iter().find(|s| s.id == id))
            .map(|s| s.name.as_str())
            .unwrap_or("All subjects");
        egui::ComboBox::from_id_salt("todo_subject_filter").selected_text(current_label).show_ui(ui, |ui| {
            ui.selectable_value(&mut state.subject_filter, None, "All subjects");
            for subject in &subjects {
                let label =
                    if subject.archived { format!("{} (archived)", subject.name) } else { subject.name.clone() };
                ui.selectable_value(&mut state.subject_filter, Some(subject.id), label);
            }
        });
    });
    ui.add_space(Spacing::MD);

    let filtered_tasks: Vec<Task> = match state.subject_filter {
        None => all_tasks,
        Some(id) => all_tasks.into_iter().filter(|t| t.subject_id == Some(id)).collect(),
    };

    // Feature request: "Sub Tasks" — nesting is exactly one level
    // deep (enforced by the parent picker in `edit_modal` only
    // offering top-level tasks), so the list itself is simple: every
    // top-level task, each optionally followed by its own children
    // indented beneath it. Keyboard navigation (Up/Down/Space/Enter)
    // only moves between top-level tasks — subtasks are still fully
    // usable with the mouse (their own checkbox, Edit, Delete), just
    // not part of the keyboard flow, to avoid a more complex
    // nested-index model for what's expected to be a fairly shallow,
    // occasional structure.
    let top_level: Vec<&Task> = filtered_tasks.iter().filter(|t| t.parent_task_id.is_none()).collect();

    // --- List --------------------------------------------------------------
    let mut delete_id: Option<i64> = None;
    let mut edit_id: Option<i64> = None;
    let mut toggle_id: Option<(i64, bool)> = None;
    let mut move_up: Option<usize> = None;
    let mut move_down: Option<usize> = None;

    if !top_level.is_empty() && ui.ctx().memory(|m| m.focused().is_none()) {
        ui.ctx().input(|input| {
            if input.key_pressed(egui::Key::ArrowDown) {
                state.selected_index = crate::ui::list_selection::move_selection(state.selected_index, 1, top_level.len());
            }
            if input.key_pressed(egui::Key::ArrowUp) {
                state.selected_index = crate::ui::list_selection::move_selection(state.selected_index, -1, top_level.len());
            }
            if let Some(index) = state.selected_index {
                if let Some(task) = top_level.get(index) {
                    if input.key_pressed(egui::Key::Space) {
                        toggle_id = Some((task.id, !task.completed));
                    }
                    if input.key_pressed(egui::Key::Enter) {
                        edit_id = Some(task.id);
                    }
                }
            }
        });
    }
    if let Some(index) = state.selected_index {
        if index >= top_level.len() {
            state.selected_index = if top_level.is_empty() { None } else { Some(top_level.len() - 1) };
        }
    }

    Panel::new().show(ui, theme, |ui| {
        if top_level.is_empty() {
            let message = if state.subject_filter.is_some() { "No tasks match." } else { "No tasks yet." };
            ui.label(RichText::new(message).font(theme.small_font()).color(p.text_faint));
        }

        let len = top_level.len();
        for (index, task) in top_level.iter().enumerate() {
            let children: Vec<&Task> = filtered_tasks.iter().filter(|t| t.parent_task_id == Some(task.id)).collect();
            let is_selected = state.selected_index == Some(index);

            let frame = egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 2)).stroke(
                if is_selected { egui::Stroke::new(Border::THICK, p.focus) } else { egui::Stroke::NONE },
            );
            frame.show(ui, |ui| {
                task_row(
                    ui,
                    theme,
                    task,
                    progress.get(&task.id).copied(),
                    &subjects,
                    &events,
                    &children,
                    &mut delete_id,
                    &mut edit_id,
                    &mut toggle_id,
                );
                if index + 1 < len || index > 0 {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_enabled_ui(index + 1 < len, |ui| {
                                if ui.small_button("▼").clicked() {
                                    move_down = Some(index);
                                }
                            });
                            ui.add_enabled_ui(index > 0, |ui| {
                                if ui.small_button("▲").clicked() {
                                    move_up = Some(index);
                                }
                            });
                        });
                    });
                }

                for child in &children {
                    ui.horizontal(|ui| {
                        ui.add_space(Spacing::LG);
                        let mut completed = child.completed;
                        if TaskItem::new(&child.title, &mut completed)
                            .bar(progress.get(&child.id).copied())
                            .show(ui, theme)
                            .clicked()
                        {
                            toggle_id = Some((child.id, completed));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Delete").clicked() {
                                delete_id = Some(child.id);
                            }
                            if ui.small_button("Edit").clicked() {
                                edit_id = Some(child.id);
                            }
                        });
                    });
                }
            });
        }
    });

    if let Some((id, completed)) = toggle_id {
        let _ = queries::tasks::set_completed(&db.conn, id, completed);
    }

    if let Some(id) = delete_id {
        let _ = queries::tasks::delete_task(&db.conn, id);
    }

    if let Some(id) = edit_id {
        if let Some(task) = filtered_tasks.iter().find(|t| t.id == id) {
            state.editing = Some(EditState::from_task(task));
            ui.ctx().memory_mut(|m| m.request_focus(egui::Id::new("edit_task_title")));
        }
    }

    if let Some(idx) = move_up {
        swap(db, &top_level, idx, idx - 1);
    }
    if let Some(idx) = move_down {
        swap(db, &top_level, idx, idx + 1);
    }

    edit_modal(ui, theme, db, state, &events, &subjects, &filtered_tasks);
}

/// One top-level task's own row (its subtasks, if any, are rendered
/// separately right after — see `show`).
#[allow(clippy::too_many_arguments)]
fn task_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    task: &Task,
    progress: Option<crate::tasks::time_bar::TaskProgress>,
    subjects: &[Subject],
    events: &[Event],
    children: &[&Task],
    delete_id: &mut Option<i64>,
    edit_id: &mut Option<i64>,
    toggle_id: &mut Option<(i64, bool)>,
) {
    let p = &theme.palette;
    ui.horizontal(|ui| {
        let mut completed = task.completed;
        let mut item = TaskItem::new(&task.title, &mut completed).bar(progress);
        if let Some(subject) = subject_name(subjects, task.subject_id) {
            item = item.tag(subject);
        }
        if item.show(ui, theme).clicked() {
            toggle_id.replace((task.id, completed));
        }

        if !children.is_empty() {
            let done = children.iter().filter(|c| c.completed).count();
            ui.add_space(Spacing::SM);
            ui.label(
                RichText::new(format!("{done}/{} subtasks", children.len())).font(theme.small_font()).color(p.text_faint),
            );
        }

        if task.repeat_weekly {
            ui.add_space(Spacing::SM);
            let label = if task.repeat_interval_days == 1 { "↻ daily" } else { "↻ weekly" };
            ui.label(RichText::new(label).font(theme.small_font()).color(p.text_faint));
        }

        if let Some(minutes) = task.estimated_minutes {
            ui.add_space(Spacing::SM);
            ui.label(RichText::new(format!("~{minutes}m")).font(theme.small_font()).color(p.text_faint));
        }

        if let Some(assigned) = &task.assigned_date {
            ui.add_space(Spacing::SM);
            let label = if task.auto_assigned { format!("FOR {assigned} (auto)") } else { format!("FOR {assigned}") };
            ui.label(RichText::new(label).font(theme.small_font()).color(p.text_faint));
        }

        if let Some(due) = &task.due_date {
            ui.add_space(Spacing::SM);
            ui.label(RichText::new(format!("DUE {due}")).font(theme.small_font()).color(p.text_faint));
        }

        if let Some(event_title) = task.event_id.and_then(|id| event_title(events, id)) {
            ui.add_space(Spacing::SM);
            ui.label(RichText::new(format!("→ {event_title}")).font(theme.small_font()).color(p.text_faint));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Delete").clicked() {
                delete_id.replace(task.id);
            }
            if ui.small_button("Edit").clicked() {
                edit_id.replace(task.id);
            }
        });
    });
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
    state: &mut ToDoState,
    events: &[Event],
    subjects: &[Subject],
    all_tasks: &[Task],
) {
    let p = &theme.palette;
    let mut modal_open = state.editing.is_some();
    let mut save_requested = false;
    let mut cancel_requested = false;
    let mut add_subtask_requested = false;

    let modal_title = if matches!(&state.editing, Some(e) if e.id.is_some()) { "Edit Task" } else { "New Task" };
    Modal::new("edit_task_modal", modal_title).show(ui.ctx(), theme, &mut modal_open, |ui| {
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if let Some(edit) = &mut state.editing {
            ui.label(RichText::new("Title").font(theme.small_font()).color(p.text_faint));
            Input::new(&mut edit.title, "Title").id("edit_task_title").show(ui, theme);
            ui.add_space(Spacing::SM);
            ui.label(RichText::new("Description").font(theme.small_font()).color(p.text_faint));
            Input::new(&mut edit.description, "Optional description").show(ui, theme);
            ui.add_space(Spacing::SM);
            ui.label(RichText::new("Due date").font(theme.small_font()).color(p.text_faint));
            let due_placeholder =
                format!("YYYY-MM-DD, 'today', 'tomorrow', 'next tuesday' — today is {}", crate::tasks::due_date::today_string());
            Input::new(&mut edit.due_date, &due_placeholder).show(ui, theme);
            ui.add_space(Spacing::SM);

            ui.label(RichText::new("Subject").font(theme.small_font()).color(p.text_faint));
            let current_subject_label = subject_name(subjects, edit.subject_id).unwrap_or("None");
            egui::ComboBox::from_id_salt("edit_task_subject").selected_text(current_subject_label).show_ui(ui, |ui| {
                ui.selectable_value(&mut edit.subject_id, None, "None");
                for subject in subjects {
                    let label =
                        if subject.archived { format!("{} (archived)", subject.name) } else { subject.name.clone() };
                    ui.selectable_value(&mut edit.subject_id, Some(subject.id), label);
                }
            });
            ui.add_space(Spacing::SM);

            ui.label(RichText::new("Revision session").font(theme.small_font()).color(p.text_faint));
            let current_label =
                edit.event_id.and_then(|id| event_title(events, id)).unwrap_or("None — standalone task");
            egui::ComboBox::from_id_salt("edit_task_event").selected_text(current_label).show_ui(ui, |ui| {
                ui.selectable_value(&mut edit.event_id, None, "None — standalone task");
                for event in events {
                    let label = format!("{} — {}", event.start_time.format("%d %b %H:%M"), event.title);
                    ui.selectable_value(&mut edit.event_id, Some(event.id), label);
                }
            });
            ui.add_space(Spacing::SM);

            // Feature request: "Sub Tasks". Only top-level tasks are
            // offered as a parent — see this file's `EditState` doc
            // comment for why (keeps nesting to exactly one level,
            // which rules out cycles entirely rather than needing to
            // detect them).
            ui.label(RichText::new("Parent task (optional)").font(theme.small_font()).color(p.text_faint));
            let current_parent_label = edit
                .parent_task_id
                .and_then(|id| all_tasks.iter().find(|t| t.id == id))
                .map(|t| t.title.as_str())
                .unwrap_or("None — top-level task");
            egui::ComboBox::from_id_salt("edit_task_parent").selected_text(current_parent_label).show_ui(ui, |ui| {
                ui.selectable_value(&mut edit.parent_task_id, None, "None — top-level task");
                for candidate in all_tasks {
                    if candidate.parent_task_id.is_some() {
                        continue;
                    }
                    if Some(candidate.id) == edit.id {
                        continue;
                    }
                    ui.selectable_value(&mut edit.parent_task_id, Some(candidate.id), candidate.title.clone());
                }
            });
            ui.add_space(Spacing::SM);

            ui.label(RichText::new("Repeats").font(theme.small_font()).color(p.text_faint));
            ui.checkbox(&mut edit.repeat_weekly, "Repeats — if ticked, resets and moves on when it falls due");

            // Feature request: "Tasks Repeating Through A List" — a
            // sub-option, meaningless (and hidden) unless the task
            // repeats at all.
            //
            // Issue report: "the enter button closes the popup... this
            // is challenging for the loop through tasks feature" — the
            // Topics field below is multiline (one topic per line), so
            // Enter needs to insert a newline rather than submit the
            // form while this field has focus. `topics_field_response`
            // is threaded out of the (conditionally-rendered) indent
            // block so `enter_pressed` below can check it the same way
            // `ui::notes::edit_modal` already does for its own
            // multiline "Content" field.
            let mut topics_field_response = None;
            if edit.repeat_weekly {
                ui.indent("repeat_list_indent", |ui| {
                    // Feature request: "Tasks Repeating By Day" —
                    // "like repeating events, tasks should have the
                    // option to repeat by day." Two fixed choices
                    // (matching `Task::repeat_interval_days`'s own
                    // doc comment for why this isn't a free-form
                    // number), same `selectable_value` picker style as
                    // "Order" below uses for `RepeatListMode`.
                    ui.add_space(Spacing::XS);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Every").font(theme.small_font()).color(p.text_faint));
                        ui.selectable_value(&mut edit.repeat_interval_days, 1, "Day");
                        ui.selectable_value(&mut edit.repeat_interval_days, 7, "Week");
                    });

                    ui.add_space(Spacing::XS);
                    ui.checkbox(&mut edit.repeat_through_list, "Repeat through a list of topics");

                    if edit.repeat_through_list {
                        ui.add_space(Spacing::XS);
                        ui.label(
                            RichText::new("Topics (one per line) — this task's title becomes \"<title>: <topic>\"")
                                .font(theme.small_font())
                                .color(p.text_faint),
                        );
                        topics_field_response = Some(ui.add(
                            egui::TextEdit::multiline(&mut edit.repeat_list_topics)
                                .hint_text("Topic 1\nTopic 2\nTopic 3")
                                .desired_rows(3)
                                .frame(egui::Frame::NONE.fill(p.surface).inner_margin(egui::Margin::same(Spacing::SM as i8))),
                        ));
                        ui.add_space(Spacing::XS);
                        ui.label(RichText::new("Order").font(theme.small_font()).color(p.text_faint));
                        ui.horizontal(|ui| {
                            for mode in [
                                crate::tasks::repeat_list::RepeatListMode::Sequential,
                                crate::tasks::repeat_list::RepeatListMode::Shuffle,
                                crate::tasks::repeat_list::RepeatListMode::SpacedRepetition,
                            ] {
                                ui.selectable_value(&mut edit.repeat_list_mode, mode, mode.label());
                            }
                        });

                        // Feature request: "tasks which loop through a
                        // list should only show the parent task, renamed
                        // daily to that day's topic" — this is the one
                        // task; it is never split into per-day tasks.
                        ui.add_space(Spacing::XS);
                        ui.label(
                            RichText::new(
                                "This one task is renamed to the next topic each time it comes round — no extra \
                                 tasks are created.",
                            )
                            .font(theme.small_font())
                            .color(p.text_faint),
                        );
                    }
                });
            }
            let topics_field_focused = topics_field_response.is_some_and(|r| r.has_focus());
            ui.add_space(Spacing::SM);

            ui.label(RichText::new("Estimated time (minutes, optional)").font(theme.small_font()).color(p.text_faint));
            Input::new(&mut edit.estimated_minutes, "e.g. 30").show(ui, theme);
            ui.label(
                RichText::new(
                    "Counts toward \"Hours Per Subject\" once completed, but only for standalone tasks \
                     (no linked revision session) — a session's own logged time already covers the rest.",
                )
                .font(theme.small_font())
                .color(p.text_faint),
            );

            if let Some(id) = edit.id {
                let subtask_count = all_tasks.iter().filter(|t| t.parent_task_id == Some(id)).count();
                if subtask_count > 0 || edit.parent_task_id.is_none() {
                    ui.add_space(Spacing::SM);
                    if ui.small_button("+ Add subtask").clicked() {
                        add_subtask_requested = true;
                    }
                }
            }

            ui.add_space(Spacing::MD);
            let save_via_enter = enter_pressed && !topics_field_focused;
            ui.horizontal(|ui| {
                if RButton::new("Save").primary().show(ui, theme).clicked() || save_via_enter {
                    save_requested = true;
                }
                if RButton::new("Cancel").show(ui, theme).clicked() {
                    cancel_requested = true;
                }
            });
        }
    });

    if add_subtask_requested {
        // Saves the current edit first (so the subtask has a real
        // parent id to point at, and nothing already typed here is
        // lost), then immediately opens a fresh child task form.
        let parent_id = save_edit(db, state);
        if let Some(parent_id) = parent_id {
            let mut child = EditState::new();
            child.parent_task_id = Some(parent_id);
            state.editing = Some(child);
            ui.ctx().memory_mut(|m| m.request_focus(egui::Id::new("edit_task_title")));
        }
        return;
    }

    if save_requested {
        save_edit(db, state);
        return;
    }

    if cancel_requested || !modal_open {
        state.editing = None;
    }
}

fn save_edit(db: &Database, state: &mut ToDoState) -> Option<i64> {
    let Some(edit) = state.editing.take() else { return None };
    let title = edit.title.trim();
    if title.is_empty() {
        return None;
    }
    let due = crate::tasks::due_date::normalize_due_date(&edit.due_date);
    let description = edit.description.trim();
    let estimated_minutes = edit.estimated_minutes.trim().parse::<i64>().ok().filter(|m| *m > 0);

    let id = match edit.id {
        Some(id) => {
            let _ = queries::tasks::update_task(
                &db.conn,
                id,
                title,
                description,
                due.as_deref(),
                edit.event_id,
                edit.subject_id,
                edit.repeat_weekly,
                edit.parent_task_id,
                estimated_minutes,
            );
            // Guarded on `repeat_weekly` too (not just
            // `repeat_through_list`) so the stored data can never end
            // up in the nonsensical "repeats through a list, but
            // doesn't actually repeat" state, even if `edit`'s own
            // checkbox state went stale after the person unchecked
            // "Weekly" without the (now-hidden) nested checkbox
            // getting reset alongside it.
            let _ = queries::tasks::set_repeat_list(
                &db.conn,
                id,
                edit.repeat_through_list && edit.repeat_weekly,
                title,
                &edit.repeat_list_topics,
                edit.repeat_list_mode,
            );
            // Feature request: "Tasks Repeating By Day" — a fast-path
            // setter of its own, same reasoning as `set_repeat_list`
            // right above (not a parameter on `update_task`). Set
            // unconditionally, same as `set_repeat_list` above it —
            // it's simply inert data whenever `repeat_weekly` is off.
            let _ = queries::tasks::set_repeat_interval_days(&db.conn, id, edit.repeat_interval_days);
            Some(id)
        }
        None => {
            // `create_task` doesn't take most of these fields (a
            // Sprint 2 signature many call sites already depend on)
            // — an immediate follow-up `update_task` fills the rest
            // in, the same "create, then a fast-path call for the
            // rest" shape used elsewhere in this codebase (e.g.
            // `set_exec_command`).
            match queries::tasks::create_task(&db.conn, title, due.as_deref(), edit.event_id, edit.subject_id) {
                Ok(task) => {
                    let _ = queries::tasks::update_task(
                        &db.conn,
                        task.id,
                        title,
                        description,
                        due.as_deref(),
                        edit.event_id,
                        edit.subject_id,
                        edit.repeat_weekly,
                        edit.parent_task_id,
                        estimated_minutes,
                    );
                    let _ = queries::tasks::set_repeat_list(
                        &db.conn,
                        task.id,
                        edit.repeat_through_list && edit.repeat_weekly,
                        title,
                        &edit.repeat_list_topics,
                        edit.repeat_list_mode,
                    );
                    let _ = queries::tasks::set_repeat_interval_days(&db.conn, task.id, edit.repeat_interval_days);
                    Some(task.id)
                }
                Err(_) => None,
            }
        }
    };

    id
}

fn swap(db: &Database, tasks: &[&Task], a: usize, b: usize) {
    if a >= tasks.len() || b >= tasks.len() {
        return;
    }
    let (id_a, pos_a) = (tasks[a].id, tasks[a].position);
    let (id_b, pos_b) = (tasks[b].id, tasks[b].position);
    let _ = queries::tasks::swap_positions(&db.conn, id_a, pos_a, id_b, pos_b);
}
