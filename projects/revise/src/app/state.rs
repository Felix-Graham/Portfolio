use chrono::{Duration as ChronoDuration, Local};

use crate::database::Database;
use crate::ui::calendar::CalendarState;
use crate::ui::dashboard::DashboardState;
use crate::ui::flashcards::FlashcardsState;
use crate::ui::notes::NotesState;
use crate::ui::tasks::ToDoState;

use crate::notifications::NotificationScanState;
use super::sources_modal::SourcesModalState;
use super::subjects_modal::SubjectsModalState;
use super::View;

/// All shell-level and per-view state for the running application.
///
/// As of Sprint 4, `db` is the *only* connection to SQLite, owned
/// once here rather than per-view. Views no longer cache `Vec<Task>` /
/// `Vec<Event>` between frames — they query fresh each render — so
/// completing a task on the Dashboard is immediately visible on the
/// To Do list and inside its Calendar event, with no manual
/// cache-invalidation bookkeeping required.
pub struct AppState {
    pub active_view: View,
    pub db: Option<Database>,
    pub db_error: Option<String>,
    pub dashboard: DashboardState,
    pub calendar: CalendarState,
    pub todo: ToDoState,
    pub notes: NotesState,
    /// Feature request: "Flashcards" — imported decks plus any active
    /// study session.
    pub flashcards: FlashcardsState,
    /// Sprint 9: global, cross-view — subjects aren't owned by any
    /// one screen. See `app::subjects_modal`.
    pub subjects_modal: SubjectsModalState,
    /// Direct request: "an API... other revision applications... to
    /// be registered and accounted for." Global for the same reason
    /// `subjects_modal` is — a source isn't owned by any one view
    /// either. See `app::sources_modal`.
    pub sources_modal: SourcesModalState,
    /// The last calendar day cleanup ran on (feedback pass — past
    /// events and fulfilled to-dos get deleted at day's end). A
    /// desktop app isn't a daemon, so "at the end of the day" really
    /// means "the next time anything notices the day changed": once
    /// at startup, and once more per frame afterward by comparing
    /// this against today, so a session left open across midnight
    /// still cleans up promptly instead of waiting for the next launch.
    pub last_cleanup_day: chrono::NaiveDate,
    /// Sprint 10: which reminders have already fired this session,
    /// and when reminders were last checked for. See
    /// `notifications::check_reminders`.
    pub notification_scan: NotificationScanState,
    /// When blocks that passed unworked were last carried forward (see
    /// `queries::block_sessions::carry_over_missed`); throttles that check
    /// to about once a minute.
    pub last_block_check: Option<chrono::DateTime<Local>>,
    /// Sprint 10: the notification settings panel.
    pub settings_modal: crate::app::settings_modal::SettingsModalState,
    /// Feature request pass: the Pomodoro focus timer. Lives at the
    /// app level (not inside `dashboard`) since its full-screen focus
    /// mode needs to override the *entire* shell, nav bar included —
    /// something a single view's `show` function has no way to do.
    pub pomodoro: crate::pomodoro::PomodoroState,
    /// Feature request: "There should also be a timer ... with a
    /// customisable time." App-level for the same reason `pomodoro`
    /// is: a running timer must keep counting (and finish, and
    /// notify) while another view is showing.
    pub timer: crate::timer::TimerState,
    /// Sprint 12: an in-progress revision session started from a
    /// specific Calendar event occurrence, if any. Also lives at the
    /// app level for the same reason `pomodoro` does — its focus mode
    /// overrides the entire shell.
    pub revision_session: Option<crate::revision_session::ActiveSession>,
    /// Whether the Revision Session overlay has been minimized (Esc)
    /// while a session is still active — the session keeps running
    /// in the background; the nav bar surfaces a "Resume" control to
    /// bring the overlay back. Kept separate from `revision_session`
    /// itself so the tested domain type doesn't need a UI-only flag.
    pub revision_session_minimized: bool,
    /// Feature request: "Startup Questions" — the quiz shown full-screen at
    /// launch, if the setting is on and there are flashcards to ask about.
    /// `None` once it's finished (or when there never was one), and the shell
    /// carries on as normal.
    pub startup_quiz: Option<crate::flashcards::quiz::Quiz>,
}

impl AppState {
    /// Deletes past non-recurring events and past fulfilled to-do
    /// items (see `queries::events::delete_past_events` and
    /// `queries::tasks::delete_past_completed_tasks`), and re-dates any
    /// repeating task that is due and was ticked on an earlier day, or
    /// whose next occurrence has arrived (see `queries::tasks::reset_due_repeating_tasks` — bug
    /// fixes: "repeating events reoccur after marked complete" and
    /// "a task due every Monday is always late the next Mondays").
    /// Cheap to
    /// call unconditionally — it's a handful of SQL statements — but
    /// callers only need it once per calendar day, hence
    /// `run_daily_cleanup_if_needed`.
    fn run_cleanup(&mut self) {
        let Some(db) = &self.db else { return };
        let today = Local::now().date_naive();
        // Convert any old "plan topics ahead" templates first, so the
        // reset pass right after re-dates them like any other task.
        let _ = crate::database::queries::tasks::retire_topic_planning(&db.conn);
        let _ = crate::database::queries::tasks::reset_due_repeating_tasks(&db.conn, today);
        let _ = crate::database::queries::events::delete_past_events(&db.conn, today);
        let _ = crate::database::queries::tasks::delete_past_completed_tasks(&db.conn, today);
        // Time-blocking. A block left "in progress" with nothing running (a quit
        // or crash mid-session) would be stuck forever, so release it — but never
        // while a stopwatch or timer really is running, which a day change can
        // happen in the middle of.
        if self.revision_session.is_none() && !self.timer.active {
            let _ = crate::database::queries::block_sessions::release_in_progress(&db.conn);
        }
        // Blocks that passed unworked carry their task forward; this must come
        // BEFORE the sweep below, which would otherwise delete yesterday's missed
        // blocks and lose the work they stood for.
        let _ = crate::database::queries::block_sessions::carry_over_missed(&db.conn, Local::now());
        // Drop plans for days that are over, and any block whose task is gone
        // (the safety net behind the per-path cascades).
        let _ = crate::database::queries::task_blocks::sweep_stale_blocks(&db.conn, today);
        // Time-blocking, if the person asked for it (off by default): refresh
        // the plan so yesterday's doesn't sit in the past. Runs at launch and
        // whenever the day changes, since that is when this routine runs.
        let now = Local::now();
        let _ = crate::database::queries::time_blocking::replan_if_enabled(&db.conn, today, now.naive_local());
        self.last_cleanup_day = today;
    }

    /// Re-runs cleanup if the calendar day has changed since it last
    /// ran — cheap to call every frame from `ReviseApp::update`, since
    /// the date comparison short-circuits almost all of those calls.
    pub fn run_daily_cleanup_if_needed(&mut self) {
        if Local::now().date_naive() != self.last_cleanup_day {
            self.run_cleanup();
        }
    }

    /// Polls for due session/task reminders (see
    /// `notifications::check_reminders`) and sends any that are newly
    /// due. Safe and cheap to call every frame — it internally
    /// throttles itself.
    pub fn check_reminders(&mut self) {
        let Some(db) = &self.db else { return };
        if crate::notifications::check_reminders(db, &mut self.notification_scan) {
            crate::gui_heartbeat::touch();
        }
    }

    /// Carries forward the work of any block that has just passed
    /// unworked (see `queries::block_sessions::carry_over_missed`), at most
    /// about once a minute. Safe to call every frame.
    pub fn check_blocks(&mut self) {
        let now = Local::now();
        if self.last_block_check.is_some_and(|last| (now - last).num_seconds() < 60) {
            return;
        }
        self.last_block_check = Some(now);
        let Some(db) = &self.db else { return };
        let _ = crate::database::queries::block_sessions::carry_over_missed(&db.conn, now);
    }

    /// Advances the Pomodoro timer by however much real time has
    /// passed since the last call (a no-op while paused). When a Work
    /// phase completes, durably logs it (so it survives even if the
    /// app is closed mid-break) and fires a notification either way —
    /// safe and cheap to call every frame.
    pub fn tick_pomodoro(&mut self) {
        use crate::pomodoro::{Phase, TickResult};

        match self.pomodoro.tick() {
            TickResult::Running => {}
            TickResult::PhaseCompleted(Phase::Work) => {
                if let Some(db) = &self.db {
                    let _ = crate::database::queries::pomodoro::log_session(
                        &db.conn,
                        Local::now().date_naive(),
                        self.pomodoro.work_minutes as i64,
                    );

                    // Feature request: "Pomodoro Subject Links" — when
                    // a subject is attached, register the completed
                    // Work phase as subject time by reusing the exact
                    // same Calendar-event + session-log pipeline a
                    // manually-scheduled revision session already
                    // uses, rather than adding a second, parallel
                    // subject-time data source. This is what makes it
                    // "show up on the calendar" *and* count toward
                    // `stats::minutes_per_subject` (the Dashboard's
                    // subject-time page) for free, with no new
                    // aggregation logic required.
                    // Feature request: task "bars" — a Pomodoro with a `task` picked logs
                    // its Work minutes against that task, and counts toward the task's
                    // subject when it has none of its own.
                    if let Some(task_id) = self.pomodoro.task_id {
                        let _ = crate::database::queries::task_time::log_task_time(
                            &db.conn,
                            task_id,
                            Local::now().date_naive(),
                            self.pomodoro.work_minutes as i64,
                            crate::database::queries::pomodoro::FocusSource::Pomodoro,
                        );
                    }
                    let pomodoro_subject = self.pomodoro.subject_id.or_else(|| {
                        self.pomodoro
                            .task_id
                            .and_then(|id| crate::database::queries::task_time::task_subject(&db.conn, id).ok().flatten())
                    });
                    if let Some(subject_id) = pomodoro_subject {
                        let end = Local::now();
                        let start = end - ChronoDuration::minutes(self.pomodoro.work_minutes as i64);
                        if let Ok(event) = crate::database::queries::events::create_event(
                            &db.conn,
                            "Pomodoro focus session",
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
                crate::notifications::send(
                    self.db.as_ref().map(|db| &db.conn),
                    "Focus session complete",
                    "Time for a break.",
                );
            }
            TickResult::PhaseCompleted(Phase::Break) => {
                crate::notifications::send(
                    self.db.as_ref().map(|db| &db.conn),
                    "Break's over",
                    "Ready for another focus session?",
                );
            }
        }
    }

    /// Advances the countdown timer by the real time since the last
    /// call (a no-op unless it's running). When it reaches zero, the
    /// finished block is logged as focus time (source `timer`),
    /// attributed to the chosen subject the same way a completed
    /// Pomodoro is, and a notification fires. A timer that is
    /// cancelled, or still counting down when the app closes, logs
    /// nothing — like an unfinished Pomodoro. Safe to call every frame.
    pub fn tick_timer(&mut self) {
        use crate::database::queries::pomodoro::FocusSource;
        use crate::timer::TimerTick;

        let TimerTick::Completed { total } = self.timer.tick() else { return };
        let minutes = crate::timer::focus_minutes(total);
        // If this run was started against a planned block, the run counts toward
        // that block's task's subject when it has none of its own (so the time is
        // added to the stats), and the block is settled to what actually ran.
        let link = self.timer.take_finished_link();
        let picked_task = self.timer.task_id;

        if let Some(db) = &self.db {
            // Feature request: task "bars" — the run is logged against the picked
            // task, or failing that the task of the block it was started against, and
            // (like a block's task) lends its subject to a run that has none. This
            // happens before the block is settled, which lowers the task's estimate.
            let linked_task = link
                .and_then(|l| crate::database::queries::task_blocks::find_block(&db.conn, l.block_id).ok().flatten())
                .map(|b| b.task_id);
            let task_id = picked_task.or(linked_task);
            if let Some(task_id) = task_id {
                let _ = crate::database::queries::task_time::log_task_time(
                    &db.conn,
                    task_id,
                    Local::now().date_naive(),
                    minutes,
                    FocusSource::Timer,
                );
            }
            let subject_id = crate::timer::attributed_subject(self.timer.subject_id, link).or_else(|| {
                task_id.and_then(|id| crate::database::queries::task_time::task_subject(&db.conn, id).ok().flatten())
            });

            // Focus time always; and, if there is a subject (the picker's, or the
            // linked block's task's), the one-off event Hours Per Subject sums.
            let _ = crate::database::queries::pomodoro::record_focus_run(
                &db.conn,
                FocusSource::Timer,
                "Timer focus session",
                subject_id,
                minutes,
                minutes,
                Local::now(),
                // A run started against a planned block is shown by the block, so
                // its event stays off the calendar (the time is still counted).
                link.is_some(),
            );

            if let Some(link) = link {
                let _ = crate::database::queries::block_sessions::finish_linked_block(
                    &db.conn,
                    link.block_id,
                    link.started_at,
                    minutes,
                    Local::now(),
                );
            }
        }
        crate::notifications::send(
            self.db.as_ref().map(|db| &db.conn),
            "Timer finished",
            &format!("{} elapsed.", crate::timer::format_remaining(total)),
        );
    }

    /// Whether one of the app's own editing dialogs is currently open
    /// — every `Modal` this app shows lives behind one of these flags.
    /// Used by `ReviseApp::handle_keyboard_navigation` to suppress the
    /// shell's normal-mode shortcuts while a dialog has the person's
    /// attention. Issue report: "When in a dialogue, keyboard
    /// navigation and shortcuts should be disabled." That function
    /// already stops shortcuts from firing whenever a text field has
    /// focus, which covers most of a dialog's lifetime, but not every
    /// moment inside one — right after clicking a checkbox, a
    /// `ComboBox` selection, or the Save/Cancel buttons themselves,
    /// none of which register as "focused" the way a text field does.
    /// The command palette, the shortcuts-help modal, and the
    /// Flashcards study player are deliberately not listed here — each
    /// already has its own dedicated handling a few lines below this
    /// method's call site, with its own rules for what Escape/Tab/etc.
    /// should do while it's open, rather than just "nothing."
    pub fn any_modal_open(&self) -> bool {
        self.todo.editing.is_some()
            || self.calendar.editing.is_some()
            || self.calendar.log_editing.is_some()
            || self.notes.editing.is_some()
            || self.notes.editing_folder.is_some()
            || self.dashboard.assigning_task.is_some()
            || self.subjects_modal.open
            || self.settings_modal.open
            || self.sources_modal.open
    }

    /// Advances the active revision session's elapsed counter by
    /// however much real time has passed since the last call
    /// (a no-op while paused, and a no-op if no session is active).
    /// Unlike the Pomodoro, there's no fixed length to complete —
    /// this just counts up until "Finish" is clicked.
    pub fn tick_revision_session(&mut self) {
        if let Some(session) = &mut self.revision_session {
            session.tick();
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        let (db, db_error) = match Database::open_default() {
            Ok(db) => (Some(db), None),
            Err(err) => (None, Some(format!("Could not open the local database: {err}"))),
        };

        let mut state = Self {
            active_view: View::Dashboard,
            db,
            db_error,
            dashboard: DashboardState::default(),
            calendar: CalendarState::default(),
            todo: ToDoState::default(),
            notes: NotesState::default(),
            flashcards: FlashcardsState::default(),
            subjects_modal: SubjectsModalState::default(),
            sources_modal: crate::app::sources_modal::SourcesModalState::default(),
            // Deliberately not "today" — forces `run_cleanup` to run
            // once immediately below, rather than waiting a day.
            last_cleanup_day: chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("valid epoch date"),
            notification_scan: NotificationScanState::default(),
            last_block_check: None,
            settings_modal: crate::app::settings_modal::SettingsModalState::default(),
            pomodoro: crate::pomodoro::PomodoroState::default(),
            timer: crate::timer::TimerState::default(),
            revision_session: None,
            revision_session_minimized: false,
            startup_quiz: None,
        };
        state.run_cleanup();
        // Once, at launch. Seeded from the clock so each launch asks different cards.
        if let Some(db) = &state.db {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1);
            state.startup_quiz = crate::ui::flashcards::build_startup_quiz(db, seed);
        }
        state
    }
}
