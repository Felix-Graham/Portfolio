pub mod palette;
pub mod settings_modal;
pub mod shortcuts_help;
pub mod state;
pub mod subjects_modal;
pub mod view;

pub use state::AppState;
pub use view::View;

use eframe::egui::{self, CentralPanel, Key, Panel, RichText};

use crate::theme::{Spacing, Theme};
use crate::ui::calendar::{CalendarSubview, EventEditState};
use crate::ui::components::NavigationBar;
use crate::ui::{calendar, dashboard, flashcards, notes, tasks};
use palette::{CommandId, PaletteState};

/// The permanent application shell: navigation bar + active view body.
/// Everything else (Dashboard/Calendar/To Do/Notes) renders inside it.
pub struct ReviseApp {
    theme: Theme,
    state: AppState,
    palette: PaletteState,
    /// A focus request raised while scanning input (see
    /// `handle_keyboard_navigation`), applied after that scan
    /// finishes. `ctx.input_mut` and `ctx.memory_mut` share the same
    /// underlying lock in egui 0.24, so calling `memory_mut` while
    /// still inside an `input_mut` closure would deadlock — this
    /// field defers it to a separate, later call instead.
    pending_focus: Option<&'static str>,
    /// A command raised while scanning input (see
    /// `handle_keyboard_navigation`'s N/T/D/W/M shortcuts), applied
    /// after that scan finishes — same reason `pending_focus` is
    /// deferred: `execute_command` needs `ctx.memory_mut`, and that
    /// can't be called from inside the `ctx.input_mut` closure these
    /// shortcuts are detected in.
    pending_command: Option<CommandId>,
    shortcuts_help: shortcuts_help::ShortcutsHelpState,
}

impl ReviseApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let theme = Theme::new();
        theme.apply(&cc.egui_ctx);
        Self {
            theme,
            state: AppState::default(),
            palette: PaletteState::default(),
            pending_focus: None,
            pending_command: None,
            shortcuts_help: shortcuts_help::ShortcutsHelpState::default(),
        }
    }

    /// Keyboard navigation, vim-esque per earlier feedback: shortcuts
    /// only apply in "normal mode" (no text field focused, and the
    /// command palette isn't up). Digits `1`-`4` jump to a section,
    /// Tab / Shift+Tab cycle, `i` enters "insert mode" by focusing the
    /// active view's primary input, and `/` opens the command palette
    /// (Issue 4 — a terminal-style search bar for the actions that
    /// used to be mouse-only). `Escape` returns to normal mode.
    ///
    /// Sprint 11 adds the plan's suggested single-letter shortcuts:
    /// `N`ew event, `T` new task, `D` today, `W`eek, `M`onth, and `?`
    /// to open the keyboard shortcuts reference — each just a
    /// same-effect shortcut for something already reachable via the
    /// command palette (see `execute_command`), so there's exactly
    /// one place that actually performs each action.
    ///
    /// Both `i` and `/` — and now `n`/`t`/`d`/`w`/`m`/`?` — are
    /// detected from raw typed-character events, not just `Key`
    /// presses, and immediately stripped from this frame's event
    /// queue via `input_mut`. Without that, the character would
    /// *also* land in the field we just focused, since that field
    /// starts receiving input the moment it's rendered later this
    /// same frame. (This was Issue 2's "the `i` key... does not
    /// work": it did focus the field, but silently left a stray "i"
    /// typed into it first.)
    fn handle_keyboard_navigation(&mut self, ctx: &egui::Context) {
        let focused = ctx.memory(|m| m.focused());

        if let Some(id) = focused {
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
            return;
        }

        if self.palette.open {
            // The palette handles its own Tab/Enter/Escape once open,
            // inside `palette::show`.
            return;
        }

        if self.state.flashcards.player.is_some() {
            // Feature request: "Flashcard Keyboard Shortcuts" —
            // Tab/→/←/Enter/Space/Esc all mean something different
            // while actively studying (see `ui::flashcards::show_player`),
            // so none of the shortcuts below (section-cycle on Tab,
            // digit-jump, `i`/`/`/`?`) should also fire from the same
            // keypress.
            return;
        }

        if self.shortcuts_help.open {
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.shortcuts_help.open = false;
            }
            return;
        }

        ctx.input_mut(|input| {
            for view in View::ALL {
                let digit_key = match view.shortcut_digit() {
                    1 => Key::Num1,
                    2 => Key::Num2,
                    3 => Key::Num3,
                    4 => Key::Num4,
                    _ => Key::Num5,
                };
                if input.key_pressed(digit_key) {
                    self.state.active_view = view;
                }
            }

            if input.key_pressed(Key::Tab) {
                let idx = View::ALL.iter().position(|v| *v == self.state.active_view).unwrap_or(0);
                let len = View::ALL.len();
                let next = if input.modifiers.shift {
                    (idx + len - 1) % len
                } else {
                    (idx + 1) % len
                };
                self.state.active_view = View::ALL[next];
            }

            if input.key_pressed(Key::I) {
                if let Some(id_name) = self.state.active_view.primary_input_id() {
                    input.events.retain(|e| !matches!(e, egui::Event::Text(t) if t == "i" || t == "I"));
                    self.pending_focus = Some(id_name);
                }
            }

            // No dedicated `Key::Slash` in egui — the character event
            // is the only way to detect it.
            if input.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t == "/")) {
                input.events.retain(|e| !matches!(e, egui::Event::Text(t) if t == "/"));
                self.palette.open_fresh();
                self.pending_focus = Some("command_palette_query");
            }

            // Single-letter shortcuts (Sprint 11). Each corresponds to
            // exactly one palette command, so the actual behavior
            // lives in `execute_command`, not duplicated here.
            let letter_commands = [
                (Key::N, "n", "N", CommandId::NewEvent),
                (Key::T, "t", "T", CommandId::NewTask),
                (Key::D, "d", "D", CommandId::Today),
                (Key::W, "w", "W", CommandId::Week),
                (Key::M, "m", "M", CommandId::Month),
            ];
            for (key, lower, upper, command) in letter_commands {
                if input.key_pressed(key) {
                    input.events.retain(|e| !matches!(e, egui::Event::Text(t) if t == lower || t == upper));
                    self.pending_command = Some(command);
                }
            }

            if input.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t == "?")) {
                input.events.retain(|e| !matches!(e, egui::Event::Text(t) if t == "?"));
                self.shortcuts_help.open = true;
            }
        });

        if let Some(id_name) = self.pending_focus.take() {
            ctx.memory_mut(|m| m.request_focus(egui::Id::new(id_name)));
        }
        if let Some(command) = self.pending_command.take() {
            self.execute_command(ctx, command);
        }
    }

    /// The full-screen Pomodoro focus overlay — everything else in
    /// the shell is skipped entirely while this renders (see the
    /// early return in `update`).
    fn show_focus_overlay(&mut self, ui: &mut egui::Ui) {
        use crate::pomodoro::{format_remaining, Phase};

        let ctx = &ui.ctx().clone();
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.state.pomodoro.focused = false;
            return;
        }

        let p = &self.theme.palette;
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(p.background)).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.12);

                let phase_label = match self.state.pomodoro.phase {
                    Phase::Work => "Focus",
                    Phase::Break => "Break",
                };
                crate::ui::components::pomodoro_circle::show(
                    ui,
                    &self.theme,
                    self.state.pomodoro.progress(),
                    &format_remaining(self.state.pomodoro.remaining),
                    phase_label,
                    160.0,
                );

                ui.add_space(Spacing::LG);
                ui.label(
                    RichText::new(if self.state.pomodoro.running { "Running" } else { "Paused" })
                        .font(self.theme.small_font())
                        .color(p.text_faint),
                );
                ui.add_space(Spacing::MD);

                ui.horizontal(|ui| {
                    ui.add_space(ui.available_width() / 2.0 - 140.0);
                    if self.state.pomodoro.running {
                        if crate::ui::components::RButton::new("Pause").show(ui, &self.theme).clicked() {
                            self.state.pomodoro.pause();
                        }
                    } else if crate::ui::components::RButton::new("Resume").primary().show(ui, &self.theme).clicked()
                    {
                        self.state.pomodoro.resume();
                    }
                    ui.add_space(Spacing::SM);
                    if crate::ui::components::RButton::new("Reset").show(ui, &self.theme).clicked() {
                        self.state.pomodoro.reset();
                        self.state.pomodoro.focused = false;
                    }
                    ui.add_space(Spacing::SM);
                    if crate::ui::components::RButton::new("Minimize").show(ui, &self.theme).clicked() {
                        self.state.pomodoro.focused = false;
                    }
                });

                ui.add_space(Spacing::MD);
                ui.label(
                    RichText::new("Esc to minimize — the timer keeps running in the background")
                        .font(self.theme.small_font())
                        .color(p.text_faint),
                );
            });
        });
    }

    /// Builds an `ActiveSession` for the given occurrence and stores it
    /// on `AppState`, entering focus mode. Looks up the event fresh
    /// from the database rather than trusting anything cached, since
    /// the click that led here came from a rendered frame that may be
    /// slightly stale by the time this runs.
    fn start_revision_session(&mut self, event_id: i64, occurrence_date: chrono::NaiveDate) {
        let Some(db) = &self.state.db else { return };
        let events = crate::database::queries::events::list_all_events(&db.conn).unwrap_or_default();
        let Some(event) = events.iter().find(|e| e.id == event_id) else { return };
        let subjects = crate::database::queries::subjects::list_all_subjects(&db.conn).unwrap_or_default();
        let subject_name =
            event.subject_id.and_then(|id| subjects.iter().find(|s| s.id == id)).map(|s| s.name.clone());
        // Fetched regardless of whether `occurrence_date` is this
        // event's anchor occurrence, unlike the Day timeline's card
        // display — focus mode is exactly where showing the shared
        // task list is the point, not incidental clutter.
        let tasks = crate::database::queries::tasks::list_tasks_for_event(&db.conn, event_id).unwrap_or_default();
        self.state.revision_session = Some(crate::revision_session::ActiveSession::new(
            Some(event_id),
            Some(occurrence_date),
            event.title.clone(),
            event.subject_id,
            subject_name,
            event.duration().num_minutes().max(0),
            tasks,
        ));
        self.state.revision_session_minimized = false;
    }

    /// The full-screen Revision Session focus overlay — mirrors
    /// `show_focus_overlay`'s Pomodoro pattern almost exactly, since
    /// both need to override the entire shell while active. Escape
    /// minimizes (the session keeps running in the background,
    /// reachable again via the nav bar's "Resume" button) rather than
    /// canceling, for the same reason Pomodoro's Escape minimizes
    /// rather than resets — Cancel/Finish are deliberate, explicit
    /// actions the person shouldn't be able to trigger by accident.
    fn show_revision_session_overlay(&mut self, ui: &mut egui::Ui) {
        use crate::revision_session::format_elapsed;

        let ctx = &ui.ctx().clone();
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.state.revision_session_minimized = true;
            return;
        }

        let p = &self.theme.palette;
        let Some(session) = &self.state.revision_session else { return };

        let mut toggle_task: Option<(i64, bool)> = None;
        let mut finish_clicked = false;
        let mut cancel_clicked = false;
        let mut pause_clicked = false;
        let mut resume_clicked = false;

        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(p.background)).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.1);

                if let Some(subject) = &session.subject_name {
                    ui.label(
                        RichText::new(subject.to_uppercase()).font(self.theme.small_font()).color(p.text_faint),
                    );
                    ui.add_space(Spacing::SM);
                }
                ui.label(
                    RichText::new(session.title.to_uppercase())
                        .font(self.theme.heading_font())
                        .color(p.text),
                );
                ui.add_space(Spacing::MD);
                ui.label(RichText::new(format_elapsed(session.elapsed)).font(self.theme.mono(56.0)).color(p.text));
                ui.add_space(Spacing::SM);
                ui.label(
                    RichText::new(if session.running { "Running" } else { "Paused" })
                        .font(self.theme.small_font())
                        .color(p.text_faint),
                );

                ui.add_space(Spacing::LG);
                ui.separator();
                ui.add_space(Spacing::MD);

                if session.tasks.is_empty() {
                    ui.label(RichText::new("No tasks attached").font(self.theme.small_font()).color(p.text_faint));
                } else {
                    let completed = session.tasks.iter().filter(|t| t.completed).count();
                    ui.vertical_centered(|ui| {
                        for task in &session.tasks {
                            let mut is_completed = task.completed;
                            if crate::ui::components::checkbox(ui, &self.theme, &mut is_completed, &task.title)
                                .clicked()
                            {
                                toggle_task = Some((task.id, is_completed));
                            }
                        }
                    });
                    ui.add_space(Spacing::SM);
                    ui.label(
                        RichText::new(format!("{completed} / {} TASKS", session.tasks.len()))
                            .font(self.theme.small_font())
                            .color(p.text_faint),
                    );
                }

                ui.add_space(Spacing::MD);
                ui.separator();
                ui.add_space(Spacing::LG);

                ui.horizontal(|ui| {
                    ui.add_space(ui.available_width() / 2.0 - 160.0);
                    if session.running {
                        if crate::ui::components::RButton::new("Pause").show(ui, &self.theme).clicked() {
                            pause_clicked = true;
                        }
                    } else if crate::ui::components::RButton::new("Resume").primary().show(ui, &self.theme).clicked()
                    {
                        resume_clicked = true;
                    }
                    ui.add_space(Spacing::SM);
                    if crate::ui::components::RButton::new("Finish").primary().show(ui, &self.theme).clicked() {
                        finish_clicked = true;
                    }
                    ui.add_space(Spacing::SM);
                    if crate::ui::components::RButton::new("Cancel").show(ui, &self.theme).clicked() {
                        cancel_clicked = true;
                    }
                });

                ui.add_space(Spacing::MD);
                ui.label(
                    RichText::new("Esc to minimize — the session keeps running in the background")
                        .font(self.theme.small_font())
                        .color(p.text_faint),
                );
            });
        });

        if let Some((id, completed)) = toggle_task {
            if let Some(db) = &self.state.db {
                let _ = crate::database::queries::tasks::set_completed(&db.conn, id, completed);
            }
            // Refresh the snapshot so the overlay reflects the change
            // immediately without needing to leave focus mode. A
            // no-op for a stopwatch session (`event_id` is `None`
            // there, and it never has tasks to toggle in the first
            // place).
            if let Some(db) = &self.state.db {
                if let Some(session) = &mut self.state.revision_session {
                    if let Some(event_id) = session.event_id {
                        session.tasks =
                            crate::database::queries::tasks::list_tasks_for_event(&db.conn, event_id)
                                .unwrap_or_default();
                    }
                }
            }
        }
        if pause_clicked {
            if let Some(session) = &mut self.state.revision_session {
                session.pause();
            }
        }
        if resume_clicked {
            if let Some(session) = &mut self.state.revision_session {
                session.resume();
            }
        }
        if finish_clicked {
            if let (Some(db), Some(session)) = (&self.state.db, &self.state.revision_session) {
                match (session.event_id, session.occurrence_date) {
                    (Some(event_id), Some(occurrence_date)) => {
                        let _ = crate::database::queries::session_log::log_session(
                            &db.conn,
                            event_id,
                            occurrence_date,
                            crate::events::SessionStatus::Adjusted,
                            Some(session.elapsed_minutes()),
                        );
                    }
                    _ => {
                        // Stopwatch mode: only worth recording if a
                        // subject was attached — an unsubjected
                        // stopwatch run has nothing meaningful to log
                        // against, mirroring how a plain Pomodoro
                        // (no subject picked) also logs nothing beyond
                        // its own "Pomodoros completed" counter.
                        // Synthesizes a one-off Calendar event for the
                        // block just timed, exactly like Pomodoro
                        // Subject Links does for a completed Work
                        // phase — same reasoning: reuse the pipeline
                        // `stats::minutes_per_subject` already sums,
                        // rather than a second one.
                        if let Some(subject_id) = session.subject_id {
                            let end = chrono::Local::now();
                            let start = end - chrono::Duration::minutes(session.elapsed_minutes().max(1));
                            if let Ok(event) = crate::database::queries::events::create_event(
                                &db.conn,
                                &session.title,
                                "",
                                start,
                                end,
                                Some(subject_id),
                                None,
                            ) {
                                let _ = crate::database::queries::session_log::log_session(
                                    &db.conn,
                                    event.id,
                                    event.day(),
                                    crate::events::SessionStatus::Completed,
                                    None,
                                );
                            }
                        }
                    }
                }
            }
            self.state.revision_session = None;
            self.state.revision_session_minimized = false;
        }
        if cancel_clicked {
            // No write to session_log — "I started this by mistake" /
            // "I don't want to record this session," distinct from
            // Finish.
            self.state.revision_session = None;
            self.state.revision_session_minimized = false;
        }
    }

    fn execute_command(&mut self, ctx: &egui::Context, command: CommandId) {
        match command {
            CommandId::GoToDashboard => self.state.active_view = View::Dashboard,
            CommandId::GoToCalendar => self.state.active_view = View::Calendar,
            CommandId::GoToToDo => self.state.active_view = View::ToDo,
            CommandId::GoToNotes => self.state.active_view = View::Notes,
            CommandId::GoToFlashcards => self.state.active_view = View::Flashcards,
            CommandId::NewTask => {
                self.state.active_view = View::ToDo;
                self.pending_focus = Some("todo_new_task_title");
            }
            CommandId::NewEvent => {
                self.state.active_view = View::Calendar;
                self.state.calendar.subview = CalendarSubview::Day;
                let day = self.state.calendar.selected_day;
                self.state.calendar.editing = Some(EventEditState::new_for(day));
                self.pending_focus = Some("event_title");
            }
            CommandId::NewNote => {
                self.state.active_view = View::Notes;
                self.state.notes.editing = Some(crate::ui::notes::NoteEditState::new());
                self.pending_focus = Some("note_title");
            }
            CommandId::ManageSubjects => {
                self.state.subjects_modal.open_fresh();
            }
            CommandId::OpenSettings => {
                if let Some(db) = &self.state.db {
                    self.state.settings_modal.open_from(db);
                }
            }
            CommandId::Today => {
                self.state.active_view = View::Calendar;
                self.state.calendar.subview = CalendarSubview::Day;
                self.state.calendar.selected_day = chrono::Local::now().date_naive();
            }
            CommandId::Week => {
                self.state.active_view = View::Calendar;
                self.state.calendar.subview = CalendarSubview::Week;
            }
            CommandId::Month => {
                self.state.active_view = View::Calendar;
                self.state.calendar.subview = CalendarSubview::Month;
            }
            CommandId::ShowShortcuts => {
                self.shortcuts_help.open = true;
            }
        }
        if let Some(id_name) = self.pending_focus.take() {
            ctx.memory_mut(|m| m.request_focus(egui::Id::new(id_name)));
        }
    }
}

impl eframe::App for ReviseApp {
    // eframe 0.36 renamed `update(&mut self, ctx: &Context, frame: &mut
    // Frame)` to `ui(&mut self, ui: &mut Ui, frame: &mut Frame)` —
    // caught only by a real compile error on the machine this was
    // actually verified against (see diagnostics/eframe-wgpu-smoke-test/
    // and HANDOFF.md's "NoGlutinConfigs" section for the full story).
    // Converting straight back to a `ctx` binding here, rather than
    // rewriting this function's ~500 lines to thread a `&mut Ui`
    // through everywhere instead, since `Context` is what essentially
    // all of it actually needs (input, memory, repaint requests,
    // spawning panels/modals via `egui::Window`/`Modal::show`, which
    // all take `&Context`, not a specific `Ui`) — `Context::clone()`
    // is a cheap handle clone (internally `Arc`-based), not a deep
    // copy, so this has no real cost.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        self.state.run_daily_cleanup_if_needed();
        self.state.check_reminders();
        self.state.tick_pomodoro();

        // Full-screen focus mode takes over the entire shell — nav
        // bar, hint line, everything — while running, per the feature
        // request that starting the timer should "fill the screen".
        // Repainting continuously (rather than only on input) is what
        // keeps the countdown visibly ticking down each second.
        if self.state.pomodoro.focused {
            ctx.request_repaint();
            self.show_focus_overlay(ui);
            return;
        }

        self.state.tick_revision_session();
        if self.state.revision_session.is_some() {
            // Keep repainting even while minimized so the nav bar's
            // "Resume" readout stays live.
            ctx.request_repaint();
            if !self.state.revision_session_minimized {
                self.show_revision_session_overlay(ui);
                return;
            }
        }

        self.handle_keyboard_navigation(ctx);

        if let Some(result) = palette::show(ctx, &self.theme, self.state.db.as_ref(), &mut self.palette) {
            match result {
                palette::PaletteResult::Command(command) => self.execute_command(ctx, command),
                palette::PaletteResult::StudySet(set_ref) => {
                    if let Some(db) = &self.state.db {
                        if let Some(player) = flashcards::start_study_session(db, set_ref) {
                            self.state.flashcards.player = Some(player);
                        }
                    }
                    self.state.active_view = View::Flashcards;
                }
            }
        }

        subjects_modal::show(ctx, &self.theme, self.state.db.as_ref(), &mut self.state.subjects_modal);
        settings_modal::show(ctx, &self.theme, self.state.db.as_ref(), &mut self.state.settings_modal);
        shortcuts_help::show(ctx, &self.theme, &mut self.shortcuts_help);

        Panel::top("nav_bar")
            .exact_size(80.0)
            .show_separator_line(true)
            .show(ui, |ui| {
                ui.add_space(Spacing::SM);
                ui.horizontal(|ui| {
                    ui.add_space(Spacing::MD);
                    if let Some(selected) = NavigationBar::show(ui, &self.theme, self.state.active_view) {
                        self.state.active_view = selected;
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_space(Spacing::MD);
                    ui.label(
                        RichText::new("1–5 sections · n/t new event/task · d/w/m calendar · i insert · / command · ? help")
                            .font(self.theme.small_font())
                            .color(self.theme.palette.text_faint),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(Spacing::MD);
                        if ui.small_button("Settings").clicked() {
                            if let Some(db) = &self.state.db {
                                self.state.settings_modal.open_from(db);
                            }
                        }
                        ui.add_space(Spacing::SM);
                        if ui.small_button("Subjects").clicked() {
                            self.state.subjects_modal.open_fresh();
                        }
                        // A minimized Revision Session keeps running
                        // in the background (see
                        // `show_revision_session_overlay`'s Escape
                        // handling) — this is the only way back to
                        // its full-screen overlay short of Finish or
                        // Cancel, which aren't reachable from here on
                        // purpose.
                        if self.state.revision_session_minimized {
                            if let Some(session) = &self.state.revision_session {
                                let label = format!(
                                    "⏱ {} — Resume",
                                    crate::revision_session::format_elapsed(session.elapsed)
                                );
                                ui.add_space(Spacing::SM);
                                if ui.small_button(label).clicked() {
                                    self.state.revision_session_minimized = false;
                                }
                            }
                        }
                    });
                });
            });

        let mut start_session_request: Option<(i64, chrono::NaiveDate)> = None;

        CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(Spacing::MD);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - Spacing::MD);

                        if let Some(err) = &self.state.db_error {
                            crate::ui::components::Panel::new().show(ui, &self.theme, |ui| {
                                ui.label(
                                    RichText::new(err)
                                        .font(self.theme.small_font())
                                        .color(self.theme.palette.text_faint),
                                );
                            });
                            ui.add_space(Spacing::MD);
                        }

                        let db = self.state.db.as_ref();
                        match self.state.active_view {
                            View::Dashboard => {
                                dashboard::show(
                                    ui,
                                    &self.theme,
                                    db,
                                    &mut self.state.dashboard,
                                    &mut self.state.pomodoro,
                                    &mut self.state.revision_session,
                                    &mut self.state.calendar,
                                    &mut self.state.active_view,
                                )
                            }
                            View::Calendar => {
                                start_session_request = calendar::show(ui, &self.theme, db, &mut self.state.calendar);
                            }
                            View::ToDo => tasks::show(ui, &self.theme, db, &mut self.state.todo),
                            View::Notes => notes::show(ui, &self.theme, db, &mut self.state.notes),
                            View::Flashcards => flashcards::show(ui, &self.theme, db, &mut self.state.flashcards),
                        }
                    });
                });
                ui.add_space(Spacing::LG);
            });
        });

        if let Some((event_id, occurrence_date)) = start_session_request {
            self.start_revision_session(event_id, occurrence_date);
        }
    }
}
