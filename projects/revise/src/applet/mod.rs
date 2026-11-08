//! Feature request: a separate applet — "a smaller unit for the stopwatch
//! and timer section" — launched on its own (`revise-applet`, with its own
//! `.desktop` file), following the main app's theme, and logging the time it
//! measures through Revise's "API" (`crate::cli`, what `revise-cli` runs).
//!
//! This module is the applet's brain, in the project's usual shape: plain
//! state and functions with the clock and the database kept at the edges, so
//! the decisions are unit-tested. [`ui`] draws it and owns the window.
//!
//! ## What the applet does
//!
//! It opens on a menu of three sections (Timer, Pomodoro, Stopwatch). Starting
//! one makes that unit fill the window until it finishes, then the menu comes
//! back. Time spent is logged when the unit finishes — through [`FocusLogger`],
//! whose real implementation ([`ApiLogger`]) runs `revise-cli focus` in
//! process, so a run is recorded exactly like one from the main app (Focus
//! time always; the subject's hours too if a subject was picked).
//!
//! ## When something "finishes"
//!
//! * **Timer** — the countdown reaching zero logs its whole length.
//! * **Pomodoro** — one cycle: the focus phase, then its break. The focus
//!   minutes are logged as the focus phase ends; the menu returns when the
//!   break ends (or is skipped).
//! * **Stopwatch** — when you press Finish.
//!
//! **Stopping early** (the Stop button, or closing the window mid-run) logs
//! the time actually spent so far — rounded to the nearest minute, nothing
//! under half a minute — rather than throwing it away. (The main app's
//! dashboard discards a cancelled timer; here the applet *is* the record of
//! the time, so closing it shouldn't lose a 40-minute session.) A Pomodoro
//! stopped during its break has nothing more to log.

pub mod ui;

use std::time::Duration;

use crate::cli::{self, Command};
use crate::database::queries::pomodoro::FocusSource;
use crate::database::Database;
use crate::pomodoro::{Phase, PomodoroState, TickResult};
use crate::revision_session::ActiveSession;
use crate::subjects::Subject;
use crate::timer::{self, TimerState, TimerTick};

/// The three things the applet can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Timer,
    Pomodoro,
    Stopwatch,
}

impl Unit {
    pub fn label(self) -> &'static str {
        match self {
            Unit::Timer => "Timer",
            Unit::Pomodoro => "Pomodoro",
            Unit::Stopwatch => "Stopwatch",
        }
    }

    fn focus_source(self) -> FocusSource {
        match self {
            Unit::Timer => FocusSource::Timer,
            Unit::Pomodoro => FocusSource::Pomodoro,
            Unit::Stopwatch => FocusSource::Stopwatch,
        }
    }
}

/// What is showing: the menu, or one unit filling the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Menu,
    Running(Unit),
}

/// One finished stretch of time to record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusRun {
    pub kind: FocusSource,
    pub minutes: i64,
    /// The subject's *name* — the API takes names, like `revise-cli`.
    pub subject: Option<String>,
}

/// Where finished runs go. The applet logs through the API; tests use a fake.
pub trait FocusLogger {
    /// Records the run, returning the line to show on success.
    fn log(&self, run: &FocusRun) -> Result<String, String>;
}

/// Logs through Revise's API: the same `cli::run` that `revise-cli focus`
/// calls, in process (no `PATH` to resolve, no process to spawn) against the
/// same database file. Notifications are the applet's own, so the API's is
/// switched off here.
pub struct ApiLogger<'a> {
    pub db: &'a Database,
}

impl FocusLogger for ApiLogger<'_> {
    fn log(&self, run: &FocusRun) -> Result<String, String> {
        cli::run(
            self.db,
            Command::Focus { kind: run.kind, minutes: run.minutes, subject: run.subject.clone(), title: None, notify: false },
        )
    }
}

/// A desktop notification the applet wants sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
}

impl Notice {
    fn new(title: &str, body: &str) -> Self {
        Self { title: title.to_owned(), body: body.to_owned() }
    }
}

/// Shortest and longest the Pomodoro's two lengths can be set to, and the
/// step the +/- buttons take.
pub const WORK_RANGE: (u32, u32) = (5, 120);
pub const BREAK_RANGE: (u32, u32) = (1, 60);
pub const POMODORO_STEP: u32 = 5;
/// The timer's presets (minutes), as chips under its digits.
pub const TIMER_PRESETS: [u32; 6] = [5, 10, 15, 25, 45, 60];

pub struct AppletState {
    pub screen: Screen,
    /// The main app's own state machines, reused as they are. Their
    /// `subject_id` is the subject picked in that section.
    pub pomodoro: PomodoroState,
    pub timer: TimerState,
    /// The stopwatch run in progress (`None` on the menu), and the subject
    /// picked for the next one.
    pub stopwatch: Option<ActiveSession>,
    pub stopwatch_subject: Option<i64>,
    /// Active subjects, for the pickers.
    pub subjects: Vec<Subject>,
    /// The result of the last attempt to log, shown under the menu.
    pub status: Option<Result<String, String>>,
}

impl Default for AppletState {
    fn default() -> Self {
        Self {
            screen: Screen::Menu,
            pomodoro: PomodoroState::default(),
            timer: TimerState::default(),
            stopwatch: None,
            stopwatch_subject: None,
            subjects: Vec::new(),
            status: None,
        }
    }
}

/// Whole minutes for a stretch of time, rounded to the nearest (so 44:40
/// is 45), the rule the stopwatch already uses. Under half a minute is 0.
pub fn rounded_minutes(elapsed: Duration) -> i64 {
    (elapsed.as_secs_f64() / 60.0).round() as i64
}

impl AppletState {
    /// Replaces the subject list (the pickers' choices), clearing any pick
    /// that no longer exists — a subject archived while the applet was open.
    pub fn set_subjects(&mut self, subjects: Vec<Subject>) {
        let exists = |id: Option<i64>| id.is_none_or(|id| subjects.iter().any(|s| s.id == id));
        if !exists(self.pomodoro.subject_id) {
            self.pomodoro.subject_id = None;
        }
        if !exists(self.timer.subject_id) {
            self.timer.subject_id = None;
        }
        if !exists(self.stopwatch_subject) {
            self.stopwatch_subject = None;
        }
        self.subjects = subjects;
    }

    pub fn subject_name(&self, id: Option<i64>) -> Option<String> {
        id.and_then(|id| self.subjects.iter().find(|s| s.id == id)).map(|s| s.name.clone())
    }

    /// The subject picked for a unit.
    pub fn subject_of(&self, unit: Unit) -> Option<i64> {
        match unit {
            Unit::Timer => self.timer.subject_id,
            Unit::Pomodoro => self.pomodoro.subject_id,
            Unit::Stopwatch => self.stopwatch_subject,
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self.screen, Screen::Running(_))
    }

    // --- starting ----------------------------------------------------------

    /// Whether `unit` could be started now (a 0:00:00 timer can't).
    pub fn can_start(&self, unit: Unit) -> bool {
        !self.is_running() && (unit != Unit::Timer || self.timer.can_start())
    }

    /// Starts `unit` and gives it the window. Does nothing if something is
    /// already running or the unit can't start.
    pub fn start(&mut self, unit: Unit) {
        if !self.can_start(unit) {
            return;
        }
        match unit {
            Unit::Timer => self.timer.start(),
            Unit::Pomodoro => {
                self.pomodoro.reset();
                self.pomodoro.start();
            }
            Unit::Stopwatch => {
                let name = self.subject_name(self.stopwatch_subject);
                self.stopwatch = Some(ActiveSession::new(
                    None,
                    None,
                    "Stopwatch".to_owned(),
                    self.stopwatch_subject,
                    name,
                    0,
                    Vec::new(),
                ));
            }
        }
        self.status = None;
        self.screen = Screen::Running(unit);
    }

    // --- the picker controls ----------------------------------------------

    /// Sets the timer's picker to `minutes` (whole), keeping it within the
    /// timer's own 23:59 range.
    pub fn set_timer_minutes(&mut self, minutes: u32) {
        let minutes = minutes.clamp(1, timer::MAX_HOURS * 60 + 59);
        self.timer.hours = minutes / 60;
        self.timer.minutes = minutes % 60;
        self.timer.seconds = 0;
    }

    /// Moves the timer's selection by `delta` minutes (never below one).
    pub fn nudge_timer_minutes(&mut self, delta: i32) {
        let current = (self.timer.hours * 60 + self.timer.minutes) as i32;
        self.set_timer_minutes((current + delta).max(1) as u32);
    }

    pub fn nudge_work_minutes(&mut self, delta: i32) {
        self.pomodoro.work_minutes = (self.pomodoro.work_minutes as i32 + delta)
            .clamp(WORK_RANGE.0 as i32, WORK_RANGE.1 as i32) as u32;
    }

    pub fn nudge_break_minutes(&mut self, delta: i32) {
        self.pomodoro.break_minutes = (self.pomodoro.break_minutes as i32 + delta)
            .clamp(BREAK_RANGE.0 as i32, BREAK_RANGE.1 as i32) as u32;
    }

    // --- running -----------------------------------------------------------

    /// Pauses or resumes the unit on screen.
    pub fn toggle_pause(&mut self) {
        match self.screen {
            Screen::Running(Unit::Timer) => {
                if self.timer.running {
                    self.timer.pause();
                } else {
                    self.timer.resume();
                }
            }
            Screen::Running(Unit::Pomodoro) => {
                if self.pomodoro.running {
                    self.pomodoro.pause();
                } else {
                    self.pomodoro.resume();
                }
            }
            Screen::Running(Unit::Stopwatch) => {
                if let Some(session) = &mut self.stopwatch {
                    if session.running {
                        session.pause();
                    } else {
                        session.resume();
                    }
                }
            }
            Screen::Menu => {}
        }
    }

    /// Whether the unit on screen is currently counting.
    pub fn is_ticking(&self) -> bool {
        match self.screen {
            Screen::Running(Unit::Timer) => self.timer.running,
            Screen::Running(Unit::Pomodoro) => self.pomodoro.running,
            Screen::Running(Unit::Stopwatch) => self.stopwatch.as_ref().is_some_and(|s| s.running),
            Screen::Menu => false,
        }
    }

    /// Advances the unit on screen by the real time since the last call —
    /// safe to call every frame. Logs and returns to the menu when the unit
    /// finishes; returns any notifications to send.
    pub fn tick(&mut self, logger: &dyn FocusLogger) -> Vec<Notice> {
        let mut notices = Vec::new();
        match self.screen {
            Screen::Menu => {}
            Screen::Running(Unit::Timer) => {
                if let TimerTick::Completed { total } = self.timer.tick() {
                    self.log(logger, Unit::Timer, timer::focus_minutes(total), self.timer.subject_id);
                    notices.push(Notice::new("Timer finished", &format!("{} done.", timer::format_remaining(total))));
                    self.finish();
                }
            }
            Screen::Running(Unit::Pomodoro) => match self.pomodoro.tick() {
                TickResult::Running => {}
                TickResult::PhaseCompleted(Phase::Work) => {
                    self.log(logger, Unit::Pomodoro, i64::from(self.pomodoro.work_minutes), self.pomodoro.subject_id);
                    notices.push(Notice::new("Focus session complete", "Time for a break."));
                }
                TickResult::PhaseCompleted(Phase::Break) => {
                    notices.push(Notice::new("Break's over", "Ready for another focus session?"));
                    self.finish();
                }
            },
            Screen::Running(Unit::Stopwatch) => {
                if let Some(session) = &mut self.stopwatch {
                    session.tick();
                }
            }
        }
        notices
    }

    /// Ends the unit on screen now — the Stop/Finish button — logging the
    /// time spent so far (see the module docs), and returns to the menu.
    pub fn stop(&mut self, logger: &dyn FocusLogger) {
        match self.screen {
            Screen::Menu => return,
            Screen::Running(Unit::Timer) => {
                let spent = self.timer.total.saturating_sub(self.timer.remaining);
                self.log(logger, Unit::Timer, rounded_minutes(spent), self.timer.subject_id);
            }
            Screen::Running(Unit::Pomodoro) => {
                if self.pomodoro.phase == Phase::Work {
                    let spent = self.pomodoro.phase_total().saturating_sub(self.pomodoro.remaining);
                    self.log(logger, Unit::Pomodoro, rounded_minutes(spent), self.pomodoro.subject_id);
                }
            }
            Screen::Running(Unit::Stopwatch) => {
                let minutes = self.stopwatch.as_ref().map_or(0, |s| s.elapsed_minutes());
                self.log(logger, Unit::Stopwatch, minutes, self.stopwatch_subject);
            }
        }
        self.finish();
    }

    /// Skips the rest of a Pomodoro's break, ending the cycle. (During the
    /// focus phase it is the same as Stop.)
    pub fn skip_break(&mut self, logger: &dyn FocusLogger) {
        if self.screen == Screen::Running(Unit::Pomodoro) {
            self.stop(logger);
        }
    }

    /// The window is closing: don't lose a run in progress.
    pub fn close(&mut self, logger: &dyn FocusLogger) {
        self.stop(logger);
    }

    /// Records `minutes` (skipping a run too short to count) and remembers the
    /// outcome for the menu's status line.
    fn log(&mut self, logger: &dyn FocusLogger, unit: Unit, minutes: i64, subject: Option<i64>) {
        if minutes < 1 {
            return;
        }
        let run = FocusRun { kind: unit.focus_source(), minutes, subject: self.subject_name(subject) };
        self.status = Some(logger.log(&run));
    }

    /// Back to the menu with every unit reset (the pickers keep their values).
    fn finish(&mut self) {
        self.timer.cancel();
        self.pomodoro.reset();
        self.stopwatch = None;
        self.screen = Screen::Menu;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Records what would have been logged.
    #[derive(Default)]
    struct Fake {
        runs: RefCell<Vec<FocusRun>>,
        fail: bool,
    }

    impl FocusLogger for Fake {
        fn log(&self, run: &FocusRun) -> Result<String, String> {
            if self.fail {
                return Err("the database is locked".to_owned());
            }
            self.runs.borrow_mut().push(run.clone());
            Ok(format!("Logged {} min", run.minutes))
        }
    }

    impl Fake {
        fn runs(&self) -> Vec<FocusRun> {
            self.runs.borrow().clone()
        }
    }

    fn subject(id: i64, name: &str) -> Subject {
        Subject { id, name: name.to_owned(), archived: false, created_at: String::new(), parent_subject_id: None, color: None }
    }

    fn state() -> AppletState {
        let mut s = AppletState::default();
        s.set_subjects(vec![subject(1, "French"), subject(2, "Maths")]);
        s
    }

    // --- the menu and starting --------------------------------------------

    #[test]
    fn it_opens_on_the_menu_with_nothing_running() {
        let s = AppletState::default();
        assert_eq!(s.screen, Screen::Menu);
        assert!(!s.is_running());
    }

    #[test]
    fn starting_a_unit_gives_it_the_window() {
        for unit in [Unit::Timer, Unit::Pomodoro, Unit::Stopwatch] {
            let mut s = state();
            s.start(unit);
            assert_eq!(s.screen, Screen::Running(unit));
            assert!(s.is_ticking());
        }
    }

    #[test]
    fn only_one_unit_runs_at_a_time() {
        let mut s = state();
        s.start(Unit::Timer);
        s.start(Unit::Stopwatch);
        assert_eq!(s.screen, Screen::Running(Unit::Timer));
        assert!(s.stopwatch.is_none());
    }

    #[test]
    fn a_timer_of_nothing_cannot_start() {
        let mut s = state();
        s.timer.hours = 0;
        s.timer.minutes = 0;
        s.timer.seconds = 0;
        assert!(!s.can_start(Unit::Timer));
        s.start(Unit::Timer);
        assert_eq!(s.screen, Screen::Menu);
    }

    // --- the pickers -------------------------------------------------------

    #[test]
    fn the_timer_steppers_and_presets_set_the_picker_in_minutes() {
        let mut s = state();
        s.set_timer_minutes(90);
        assert_eq!((s.timer.hours, s.timer.minutes, s.timer.seconds), (1, 30, 0));
        s.nudge_timer_minutes(5);
        assert_eq!((s.timer.hours, s.timer.minutes), (1, 35));
        s.nudge_timer_minutes(-1000);
        assert_eq!((s.timer.hours, s.timer.minutes), (0, 1), "never below a minute");
        s.set_timer_minutes(100_000);
        assert_eq!((s.timer.hours, s.timer.minutes), (23, 59), "the timer's own ceiling");
    }

    #[test]
    fn the_pomodoro_lengths_stay_in_range() {
        let mut s = state();
        s.nudge_work_minutes(-1000);
        assert_eq!(s.pomodoro.work_minutes, WORK_RANGE.0);
        s.nudge_work_minutes(1000);
        assert_eq!(s.pomodoro.work_minutes, WORK_RANGE.1);
        s.nudge_break_minutes(-1000);
        assert_eq!(s.pomodoro.break_minutes, BREAK_RANGE.0);
        s.nudge_break_minutes(1000);
        assert_eq!(s.pomodoro.break_minutes, BREAK_RANGE.1);
    }

    #[test]
    fn a_subject_that_disappears_is_dropped_from_the_pickers() {
        let mut s = state();
        s.timer.subject_id = Some(2);
        s.pomodoro.subject_id = Some(1);
        s.stopwatch_subject = Some(2);
        s.set_subjects(vec![subject(1, "French")]);
        assert_eq!((s.timer.subject_id, s.pomodoro.subject_id, s.stopwatch_subject), (None, Some(1), None));
    }

    // --- the timer ---------------------------------------------------------

    #[test]
    fn a_finished_timer_logs_its_whole_length_and_returns_to_the_menu() {
        let mut s = state();
        let log = Fake::default();
        s.set_timer_minutes(25);
        s.timer.subject_id = Some(1);
        s.start(Unit::Timer);
        s.timer.remaining = Duration::from_millis(1);
        std::thread::sleep(Duration::from_millis(10));

        let notices = s.tick(&log);

        assert_eq!(log.runs(), vec![FocusRun { kind: FocusSource::Timer, minutes: 25, subject: Some("French".to_owned()) }]);
        assert_eq!(s.screen, Screen::Menu);
        assert_eq!(notices.len(), 1);
        assert_eq!(s.status, Some(Ok("Logged 25 min".to_owned())));
    }

    #[test]
    fn stopping_a_timer_early_logs_the_time_actually_spent() {
        let mut s = state();
        let log = Fake::default();
        s.set_timer_minutes(25);
        s.start(Unit::Timer);
        s.timer.remaining = Duration::from_secs(25 * 60 - 10 * 60);

        s.stop(&log);

        assert_eq!(log.runs()[0].minutes, 10);
        assert_eq!(log.runs()[0].subject, None, "no subject picked");
        assert_eq!(s.screen, Screen::Menu);
        assert!(!s.timer.active, "the countdown is reset");
    }

    #[test]
    fn a_run_stopped_within_seconds_logs_nothing() {
        let mut s = state();
        let log = Fake::default();
        s.start(Unit::Timer);
        s.stop(&log);
        assert!(log.runs().is_empty());
        assert_eq!(s.screen, Screen::Menu);
        assert_eq!(s.status, None);
    }

    // --- the Pomodoro ------------------------------------------------------

    #[test]
    fn a_pomodoro_logs_its_focus_phase_then_runs_the_break_then_returns() {
        let mut s = state();
        let log = Fake::default();
        s.pomodoro.subject_id = Some(2);
        s.start(Unit::Pomodoro);

        // The focus phase ends: logged, and the break begins on the same screen.
        s.pomodoro.remaining = Duration::from_millis(1);
        std::thread::sleep(Duration::from_millis(10));
        let notices = s.tick(&log);
        assert_eq!(log.runs(), vec![FocusRun { kind: FocusSource::Pomodoro, minutes: 25, subject: Some("Maths".to_owned()) }]);
        assert_eq!(s.screen, Screen::Running(Unit::Pomodoro), "still on the Pomodoro through its break");
        assert_eq!(s.pomodoro.phase, Phase::Break);
        assert_eq!(notices[0].title, "Focus session complete");

        // The break ends: back to the menu, nothing more logged.
        s.pomodoro.remaining = Duration::from_millis(1);
        std::thread::sleep(Duration::from_millis(10));
        let notices = s.tick(&log);
        assert_eq!(s.screen, Screen::Menu);
        assert_eq!(log.runs().len(), 1);
        assert_eq!(notices[0].title, "Break's over");
        assert_eq!(s.pomodoro.phase, Phase::Work, "reset for next time");
    }

    #[test]
    fn stopping_a_pomodoro_mid_focus_logs_the_minutes_so_far() {
        let mut s = state();
        let log = Fake::default();
        s.start(Unit::Pomodoro);
        s.pomodoro.remaining = Duration::from_secs(25 * 60 - 12 * 60);
        s.stop(&log);
        assert_eq!(log.runs()[0].minutes, 12);
        assert_eq!(s.screen, Screen::Menu);
    }

    #[test]
    fn skipping_the_break_ends_the_cycle_without_logging_again() {
        let mut s = state();
        let log = Fake::default();
        s.start(Unit::Pomodoro);
        s.pomodoro.remaining = Duration::from_millis(1);
        std::thread::sleep(Duration::from_millis(10));
        s.tick(&log);
        assert_eq!(s.pomodoro.phase, Phase::Break);

        s.skip_break(&log);

        assert_eq!(s.screen, Screen::Menu);
        assert_eq!(log.runs().len(), 1, "only the focus phase");
    }

    // --- the stopwatch -----------------------------------------------------

    #[test]
    fn finishing_the_stopwatch_logs_the_elapsed_time_for_its_subject() {
        let mut s = state();
        let log = Fake::default();
        s.stopwatch_subject = Some(1);
        s.start(Unit::Stopwatch);
        s.stopwatch.as_mut().unwrap().elapsed = Duration::from_secs(44 * 60 + 40);

        s.stop(&log);

        assert_eq!(log.runs(), vec![FocusRun { kind: FocusSource::Stopwatch, minutes: 45, subject: Some("French".to_owned()) }]);
        assert_eq!(s.screen, Screen::Menu);
        assert!(s.stopwatch.is_none());
    }

    #[test]
    fn pausing_stops_the_stopwatch_counting() {
        let mut s = state();
        s.start(Unit::Stopwatch);
        s.toggle_pause();
        assert!(!s.is_ticking());
        s.toggle_pause();
        assert!(s.is_ticking());
    }

    // --- logging problems and closing ---------------------------------------

    #[test]
    fn a_logging_failure_is_shown_not_swallowed_and_the_menu_still_returns() {
        let mut s = state();
        let log = Fake { fail: true, ..Fake::default() };
        s.start(Unit::Stopwatch);
        s.stopwatch.as_mut().unwrap().elapsed = Duration::from_secs(600);

        s.stop(&log);

        assert_eq!(s.screen, Screen::Menu);
        assert_eq!(s.status, Some(Err("the database is locked".to_owned())));
    }

    #[test]
    fn closing_the_window_mid_run_saves_the_time_so_far() {
        let mut s = state();
        let log = Fake::default();
        s.start(Unit::Stopwatch);
        s.stopwatch.as_mut().unwrap().elapsed = Duration::from_secs(20 * 60);
        s.close(&log);
        assert_eq!(log.runs()[0].minutes, 20);
    }

    #[test]
    fn closing_from_the_menu_logs_nothing() {
        let mut s = state();
        let log = Fake::default();
        s.close(&log);
        assert!(log.runs().is_empty());
    }

    #[test]
    fn rounding_matches_the_stopwatchs_rule() {
        assert_eq!(rounded_minutes(Duration::from_secs(29)), 0);
        assert_eq!(rounded_minutes(Duration::from_secs(30)), 1);
        assert_eq!(rounded_minutes(Duration::from_secs(44 * 60 + 40)), 45);
    }

    // --- through the real API ----------------------------------------------

    #[test]
    fn the_api_logger_records_a_run_the_way_the_main_app_does() {
        use crate::database::queries;
        let db = Database::open_in_memory().expect("open in-memory db");
        let french = queries::subjects::create_subject(&db.conn, "French").unwrap();
        let logger = ApiLogger { db: &db };

        let message = logger
            .log(&FocusRun { kind: FocusSource::Timer, minutes: 25, subject: Some("French".to_owned()) })
            .expect("logged");

        assert!(message.contains("25 minutes"), "{message}");
        assert_eq!(queries::pomodoro::total_minutes(&db.conn).unwrap(), 25);
        let events = queries::events::list_all_events(&db.conn).unwrap();
        assert_eq!(events[0].subject_id, Some(french.id));
    }

    #[test]
    fn the_api_logger_logs_a_subjectless_run_to_focus_time_only() {
        use crate::database::queries;
        let db = Database::open_in_memory().expect("open in-memory db");
        ApiLogger { db: &db }
            .log(&FocusRun { kind: FocusSource::Stopwatch, minutes: 12, subject: None })
            .expect("logged");
        assert_eq!(queries::pomodoro::total_minutes(&db.conn).unwrap(), 12);
        assert!(queries::events::list_all_events(&db.conn).unwrap().is_empty());
    }
}
