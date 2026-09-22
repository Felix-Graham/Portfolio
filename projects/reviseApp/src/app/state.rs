use chrono::{Duration as ChronoDuration, Local};

use crate::database::Database;
use crate::ui::calendar::CalendarState;
use crate::ui::dashboard::DashboardState;
use crate::ui::flashcards::FlashcardsState;
use crate::ui::notes::NotesState;
use crate::ui::tasks::ToDoState;

use crate::notifications::NotificationScanState;
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
    /// Sprint 10: the notification settings panel.
    pub settings_modal: crate::app::settings_modal::SettingsModalState,
    /// Feature request pass: the Pomodoro focus timer. Lives at the
    /// app level (not inside `dashboard`) since its full-screen focus
    /// mode needs to override the *entire* shell, nav bar included —
    /// something a single view's `show` function has no way to do.
    pub pomodoro: crate::pomodoro::PomodoroState,
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
}

impl AppState {
    /// Deletes past non-recurring events and past fulfilled to-do
    /// items (see `queries::events::delete_past_events` and
    /// `queries::tasks::delete_past_completed_tasks`), and resets any
    /// completed repeating task whose next due date has arrived (see
    /// `queries::tasks::reset_due_repeating_tasks` — bug fix:
    /// "repeating events reoccur after marked complete"). Cheap to
    /// call unconditionally — it's a handful of SQL statements — but
    /// callers only need it once per calendar day, hence
    /// `run_daily_cleanup_if_needed`.
    fn run_cleanup(&mut self) {
        let Some(db) = &self.db else { return };
        let today = Local::now().date_naive();
        let _ = crate::database::queries::tasks::reset_due_repeating_tasks(&db.conn, today);
        let _ = crate::database::queries::events::delete_past_events(&db.conn, today);
        let _ = crate::database::queries::tasks::delete_past_completed_tasks(&db.conn, today);
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
                    if let Some(subject_id) = self.pomodoro.subject_id {
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
            // Deliberately not "today" — forces `run_cleanup` to run
            // once immediately below, rather than waiting a day.
            last_cleanup_day: chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("valid epoch date"),
            notification_scan: NotificationScanState::default(),
            settings_modal: crate::app::settings_modal::SettingsModalState::default(),
            pomodoro: crate::pomodoro::PomodoroState::default(),
            revision_session: None,
            revision_session_minimized: false,
        };
        state.run_cleanup();
        state
    }
}
