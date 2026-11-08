use std::time::{Duration, Instant};

use chrono::{DateTime, Local, NaiveDate};

use crate::tasks::Task;

/// An in-progress revision session — either started from a specific
/// Calendar event occurrence, or (feature request: "Pomodoro
/// Stopwatch") as a free-standing, event-less stopwatch someone
/// starts directly without first creating a Calendar event. Distinct
/// from the Pomodoro timer (`pomodoro::PomodoroState`): a Pomodoro is
/// a fixed-length, auto-cycling productivity timer; a session counts
/// *up* with no fixed end — it runs until the person says "Finish",
/// at which point the elapsed time becomes the recorded actual
/// duration (via `events::session_log`), for either variant.
///
/// `event_id`/`occurrence_date` are `None` for the stopwatch case —
/// on Finish, a stopwatch with a subject attached synthesizes a
/// one-off Calendar event for the block it just timed (see
/// `app::ReviseApp::show_revision_session_overlay`), the same way
/// Pomodoro Subject Links already does for completed Pomodoro Work
/// phases — rather than this type needing a second "how do I count
/// toward subject time" code path of its own.
pub struct ActiveSession {
    pub event_id: Option<i64>,
    pub occurrence_date: Option<NaiveDate>,
    pub title: String,
    /// The subject id itself, kept alongside `subject_name` (which is
    /// what's actually displayed) so Finish can attribute a
    /// synthesized event to the right subject without a second
    /// database lookup — `subject_name` alone isn't enough for that.
    pub subject_id: Option<i64>,
    pub subject_name: Option<String>,
    /// Feature request: task "bars" — the task a free-standing stopwatch
    /// logs its time against, if one was picked. Always `None` for a session
    /// tied to a Calendar event (that time is already on the Calendar).
    pub task_id: Option<i64>,
    pub scheduled_minutes: i64,
    pub elapsed: Duration,
    pub running: bool,
    /// Snapshotted when the session starts and refreshed whenever a
    /// task's completion is toggled from inside focus mode, so
    /// "tasks remain accessible" (the acceptance criterion) doesn't
    /// require leaving the overlay to see the effect. Always empty
    /// for a stopwatch session, since there's no linked event to
    /// pull tasks from.
    pub tasks: Vec<Task>,
    /// When the session began, on the wall clock — what a time block is
    /// matched against (the elapsed counter uses the monotonic clock).
    pub started_at: DateTime<Local>,
    /// The planned time block this stopwatch was started against, if one
    /// was near enough (see `queries::block_sessions`). Only ever set for a
    /// free-standing stopwatch: an event-linked session already has its
    /// time on the Calendar.
    pub linked_block: Option<i64>,
    last_tick: Option<Instant>,
}

impl ActiveSession {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        event_id: Option<i64>,
        occurrence_date: Option<NaiveDate>,
        title: String,
        subject_id: Option<i64>,
        subject_name: Option<String>,
        scheduled_minutes: i64,
        tasks: Vec<Task>,
    ) -> Self {
        Self {
            event_id,
            occurrence_date,
            title,
            subject_id,
            subject_name,
            task_id: None,
            scheduled_minutes,
            elapsed: Duration::ZERO,
            running: true,
            tasks,
            started_at: Local::now(),
            linked_block: None,
            last_tick: Some(Instant::now()),
        }
    }

    pub fn pause(&mut self) {
        self.running = false;
        self.last_tick = None;
    }

    pub fn resume(&mut self) {
        self.running = true;
        self.last_tick = Some(Instant::now());
    }

    /// Advances the elapsed counter by however much wall-clock time
    /// has passed since the last call. A no-op while paused. Uses
    /// `Instant`, not `chrono`, for the same reason the Pomodoro timer
    /// does — this is a short-lived, in-session count, not something
    /// with calendar semantics.
    pub fn tick(&mut self) {
        if !self.running {
            self.last_tick = None;
            return;
        }
        let now = Instant::now();
        let elapsed_since_last = self.last_tick.map(|t| now.duration_since(t)).unwrap_or_default();
        self.last_tick = Some(now);
        self.elapsed += elapsed_since_last;
    }

    /// Elapsed time in whole minutes, rounded to the nearest minute —
    /// what gets recorded as the actual duration on Finish. Rounding
    /// (rather than truncating) means a session run for 44 minutes 40
    /// seconds logs as 45, not 44 — closer to what someone would
    /// actually say if asked.
    pub fn elapsed_minutes(&self) -> i64 {
        (self.elapsed.as_secs_f64() / 60.0).round() as i64
    }
}

/// Formats elapsed time as `MM:SS`, or `H:MM:SS` once it reaches an
/// hour — a session can run much longer than a Pomodoro's 25 minutes,
/// so unlike `pomodoro::format_remaining` this needs an hours place.
pub fn format_elapsed(elapsed: Duration) -> String {
    let total_seconds = elapsed.as_secs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> ActiveSession {
        ActiveSession::new(
            Some(1),
            Some(NaiveDate::from_ymd_opt(2026, 8, 25).unwrap()),
            "Integration".to_owned(),
            Some(2),
            Some("Mathematics".to_owned()),
            60,
            Vec::new(),
        )
    }

    fn stopwatch_session() -> ActiveSession {
        ActiveSession::new(None, None, "Ad-hoc revision".to_owned(), None, None, 0, Vec::new())
    }

    #[test]
    fn a_stopwatch_session_has_no_linked_event() {
        let s = stopwatch_session();
        assert_eq!(s.event_id, None);
        assert_eq!(s.occurrence_date, None);
        assert!(s.running);
        assert_eq!(s.elapsed, Duration::ZERO);
    }

    #[test]
    fn starts_running_at_zero_elapsed() {
        let s = session();
        assert!(s.running);
        assert_eq!(s.elapsed, Duration::ZERO);
    }

    #[test]
    fn tick_while_paused_does_not_advance_elapsed() {
        let mut s = session();
        s.pause();
        s.tick();
        assert_eq!(s.elapsed, Duration::ZERO);
    }

    #[test]
    fn tick_while_running_advances_elapsed() {
        let mut s = session();
        std::thread::sleep(Duration::from_millis(5));
        s.tick();
        assert!(s.elapsed > Duration::ZERO);
    }

    #[test]
    fn pause_then_resume_does_not_double_count_the_paused_interval() {
        let mut s = session();
        std::thread::sleep(Duration::from_millis(5));
        s.tick();
        let after_first_tick = s.elapsed;
        s.pause();
        std::thread::sleep(Duration::from_millis(20));
        s.tick(); // no-op, paused
        assert_eq!(s.elapsed, after_first_tick, "elapsed must not grow while paused");
        s.resume();
        std::thread::sleep(Duration::from_millis(5));
        s.tick();
        assert!(s.elapsed > after_first_tick);
    }

    #[test]
    fn elapsed_minutes_rounds_to_the_nearest_minute() {
        let mut s = session();
        s.elapsed = Duration::from_secs(44 * 60 + 40); // 44:40 -> rounds up to 45
        assert_eq!(s.elapsed_minutes(), 45);
        s.elapsed = Duration::from_secs(44 * 60 + 20); // 44:20 -> rounds down to 44
        assert_eq!(s.elapsed_minutes(), 44);
    }

    #[test]
    fn format_elapsed_under_an_hour_omits_the_hours_place() {
        assert_eq!(format_elapsed(Duration::from_secs(47 * 60 + 32)), "47:32");
        assert_eq!(format_elapsed(Duration::from_secs(9)), "0:09");
    }

    #[test]
    fn format_elapsed_over_an_hour_includes_the_hours_place() {
        assert_eq!(format_elapsed(Duration::from_secs(3661)), "1:01:01");
    }
}
