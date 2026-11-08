//! Feature request: "There should also be a timer. Like the pomodoro
//! but with a customisable time." A single countdown for a length the
//! person picks (hours / minutes / seconds), as opposed to
//! `pomodoro::PomodoroState`'s fixed, auto-cycling work/break phases.
//!
//! Like the Pomodoro, timing uses the monotonic `Instant` clock (a
//! short-lived in-session countdown with no calendar semantics), and
//! the state lives at the app level so a running timer keeps counting
//! — and can finish and notify — while another view is showing. The
//! counting itself is split from the clock: `advance` takes an elapsed
//! `Duration` and is what's unit-tested; `tick` just measures the
//! elapsed time and calls it.

use std::time::{Duration, Instant};

use chrono::{DateTime, Local};

/// Longest a timer may be set for: 23:59:59, i.e. the picker's own
/// range (hours 0–23, minutes and seconds 0–59).
pub const MAX_HOURS: u32 = 23;

/// The outcome of one `tick`/`advance`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerTick {
    /// Nothing finished (including "not running at all").
    Running,
    /// The countdown reached zero this tick. Carries the full length
    /// that was counted down, which is what gets logged as focus time.
    Completed { total: Duration },
}

pub struct TimerState {
    /// The picker's selection — what `start` will count down from.
    /// Kept across runs, so a person who always does 45 minutes sets it
    /// once.
    pub hours: u32,
    pub minutes: u32,
    pub seconds: u32,
    /// A countdown is in progress (running or paused). While `false`
    /// the Dashboard shows the picker; while `true`, the countdown.
    pub active: bool,
    pub running: bool,
    /// Time left in the countdown in progress.
    pub remaining: Duration,
    /// The length the countdown in progress started from.
    pub total: Duration,
    /// A subject to attribute a completed timer to, exactly as with a
    /// Pomodoro (`None` = just a timer). Not cleared between runs.
    pub subject_id: Option<i64>,
    /// Feature request: task "bars" — a task to log the run against
    /// (`None` = none). Not cleared between runs, like `subject_id`.
    pub task_id: Option<i64>,
    /// When the countdown in progress began, on the wall clock — what a
    /// time block is matched against (the countdown itself uses the
    /// monotonic clock and has no calendar time). `None` while idle.
    pub started_at: Option<DateTime<Local>>,
    /// The planned time block this countdown was started against, if one
    /// was near enough (see `queries::block_sessions`).
    pub linked_block: Option<i64>,
    /// The subject of the linked block's task, used for a run that has no
    /// subject of its own. Kept apart from `subject_id` so linking never
    /// overwrites the picker's selection, which persists between runs.
    pub link_subject: Option<i64>,
    /// What the last *finished* countdown was linked to, kept past the
    /// reset that `advance` does on completion so the caller can still
    /// settle the block. Taken (and cleared) by `take_finished_link`.
    finished_link: Option<FinishedLink>,
    last_tick: Option<Instant>,
}

/// The subject a finished countdown counts toward: the one picked in the
/// timer, or failing that the subject of the block's task it was started
/// against. A picked subject always wins, so linking never reassigns time
/// the person deliberately put elsewhere.
pub fn attributed_subject(picked: Option<i64>, link: Option<FinishedLink>) -> Option<i64> {
    picked.or(link.and_then(|l| l.link_subject))
}

/// What a countdown that just ran out had been linked to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinishedLink {
    pub started_at: DateTime<Local>,
    pub block_id: i64,
    pub link_subject: Option<i64>,
}

impl Default for TimerState {
    fn default() -> Self {
        Self {
            hours: 0,
            minutes: 5,
            seconds: 0,
            active: false,
            running: false,
            remaining: Duration::ZERO,
            total: Duration::ZERO,
            subject_id: None,
            task_id: None,
            started_at: None,
            linked_block: None,
            link_subject: None,
            finished_link: None,
            last_tick: None,
        }
    }
}

impl TimerState {
    /// The length currently selected in the picker.
    pub fn selected(&self) -> Duration {
        Duration::from_secs(u64::from(self.hours.min(MAX_HOURS)) * 3600 + u64::from(self.minutes.min(59)) * 60 + u64::from(self.seconds.min(59)))
    }

    /// Whether `start` would do anything: a 0:00:00 timer can't run.
    pub fn can_start(&self) -> bool {
        !self.active && self.selected() > Duration::ZERO
    }

    /// Begins counting down from the picker's selection. Does nothing
    /// if a countdown is already in progress or nothing is selected.
    pub fn start(&mut self) {
        if !self.can_start() {
            return;
        }
        self.total = self.selected();
        self.remaining = self.total;
        self.active = true;
        self.running = true;
        self.started_at = Some(Local::now());
        self.linked_block = None;
        self.link_subject = None;
        self.last_tick = Some(Instant::now());
    }

    pub fn pause(&mut self) {
        self.running = false;
        self.last_tick = None;
    }

    pub fn resume(&mut self) {
        if self.active {
            self.running = true;
            self.last_tick = Some(Instant::now());
        }
    }

    /// Abandons the countdown (nothing is logged) and returns to the
    /// picker, with its selection untouched.
    pub fn cancel(&mut self) {
        self.active = false;
        self.running = false;
        self.remaining = Duration::ZERO;
        self.total = Duration::ZERO;
        self.started_at = None;
        self.linked_block = None;
        self.link_subject = None;
        self.last_tick = None;
    }

    /// The link of the countdown that just finished, once. `advance`
    /// drops back to the picker (clearing the live link) the moment the
    /// countdown reaches zero, so the block it was started against has to
    /// be read from here.
    pub fn take_finished_link(&mut self) -> Option<FinishedLink> {
        self.finished_link.take()
    }

    /// Advances by the real time since the last call. Safe to call
    /// every frame; a no-op unless running.
    pub fn tick(&mut self) -> TimerTick {
        if !self.running {
            self.last_tick = None;
            return TimerTick::Running;
        }
        let now = Instant::now();
        let elapsed = self.last_tick.map(|t| now.duration_since(t)).unwrap_or_default();
        self.last_tick = Some(now);
        self.advance(elapsed)
    }

    /// Counts `elapsed` off the remaining time. When that reaches zero
    /// the timer finishes: it stops, drops back to the picker, and
    /// reports `Completed`. Does nothing unless running.
    pub fn advance(&mut self, elapsed: Duration) -> TimerTick {
        if !self.running {
            return TimerTick::Running;
        }
        if elapsed < self.remaining {
            self.remaining -= elapsed;
            return TimerTick::Running;
        }
        let total = self.total;
        self.finished_link = match (self.started_at, self.linked_block) {
            (Some(started_at), Some(block_id)) => {
                Some(FinishedLink { started_at, block_id, link_subject: self.link_subject })
            }
            _ => None,
        };
        self.cancel();
        TimerTick::Completed { total }
    }

    /// Fraction of the countdown elapsed, `0.0..=1.0`, for the ring.
    pub fn progress(&self) -> f32 {
        let total = self.total.as_secs_f32();
        if total <= 0.0 {
            return 0.0;
        }
        (1.0 - self.remaining.as_secs_f32() / total).clamp(0.0, 1.0)
    }
}

/// Formats a remaining time as `H:MM:SS` once it's an hour or more,
/// else `MM:SS` — rounding *up* to the second (like
/// `pomodoro::format_remaining`) so the last second reads `0:01`
/// instead of jumping to `0:00` a second early.
pub fn format_remaining(remaining: Duration) -> String {
    let total = remaining.as_secs_f64().ceil() as u64;
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// How a finished countdown of `total` is logged as focus minutes:
/// rounded to the nearest minute, but never below one, so even a
/// 20-second timer that ran to the end leaves a trace.
pub fn focus_minutes(total: Duration) -> i64 {
    (((total.as_secs() + 30) / 60) as i64).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn timer(h: u32, m: u32, s: u32) -> TimerState {
        TimerState { hours: h, minutes: m, seconds: s, ..TimerState::default() }
    }

    #[test]
    fn selected_combines_hours_minutes_and_seconds() {
        assert_eq!(timer(1, 2, 3).selected(), secs(3723));
        assert_eq!(timer(0, 5, 0).selected(), secs(300));
        assert_eq!(timer(23, 59, 59).selected(), secs(86_399));
    }

    #[test]
    fn selected_clamps_out_of_range_fields() {
        assert_eq!(timer(99, 99, 99).selected(), secs(23 * 3600 + 59 * 60 + 59));
    }

    #[test]
    fn a_zero_timer_cannot_start() {
        let mut t = timer(0, 0, 0);
        assert!(!t.can_start());
        t.start();
        assert!(!t.active && !t.running);
    }

    #[test]
    fn start_begins_a_countdown_from_the_selection() {
        let mut t = timer(0, 1, 30);
        t.start();
        assert!(t.active && t.running);
        assert_eq!(t.total, secs(90));
        assert_eq!(t.remaining, secs(90));
        assert!(!t.can_start(), "can't start again while counting down");
    }

    #[test]
    fn advance_counts_down_while_running() {
        let mut t = timer(0, 1, 0);
        t.start();
        assert_eq!(t.advance(secs(20)), TimerTick::Running);
        assert_eq!(t.remaining, secs(40));
        assert!((t.progress() - (20.0 / 60.0)).abs() < 1e-5);
    }


    #[test]
    fn starting_records_the_wall_clock_start_and_clears_any_old_link() {
        let mut t = timer(0, 1, 0);
        t.linked_block = Some(9);
        t.link_subject = Some(3);
        t.start();
        assert!(t.started_at.is_some());
        assert_eq!((t.linked_block, t.link_subject), (None, None), "a fresh run starts unlinked");
    }

    #[test]
    fn cancelling_clears_the_start_and_the_link() {
        let mut t = timer(0, 1, 0);
        t.start();
        t.linked_block = Some(9);
        t.cancel();
        assert_eq!((t.started_at, t.linked_block, t.link_subject), (None, None, None));
        assert_eq!(t.take_finished_link(), None, "a cancelled run leaves nothing to settle");
    }

    #[test]
    fn a_countdown_that_finishes_hands_back_its_link_exactly_once() {
        let mut t = timer(0, 1, 0);
        t.start();
        t.linked_block = Some(9);
        t.link_subject = Some(3);
        let started = t.started_at.expect("started");

        assert!(matches!(t.advance(secs(60)), TimerTick::Completed { .. }));

        assert_eq!(t.linked_block, None, "the live state has reset");
        let link = t.take_finished_link().expect("the link survives the reset");
        assert_eq!((link.block_id, link.link_subject, link.started_at), (9, Some(3), started));
        assert_eq!(t.take_finished_link(), None, "taken once");
    }

    #[test]
    fn a_countdown_that_finishes_unlinked_has_no_link_to_hand_back() {
        let mut t = timer(0, 1, 0);
        t.start();
        t.advance(secs(60));
        assert_eq!(t.take_finished_link(), None);
    }


    #[test]
    fn a_picked_subject_beats_the_blocks_and_the_blocks_fills_in_when_none_was_picked() {
        let started = Local::now();
        let link = Some(FinishedLink { started_at: started, block_id: 1, link_subject: Some(7) });
        assert_eq!(attributed_subject(Some(3), link), Some(3), "never reassigns what the person chose");
        assert_eq!(attributed_subject(None, link), Some(7));
        assert_eq!(attributed_subject(None, None), None);
        let no_subject_link = Some(FinishedLink { started_at: started, block_id: 1, link_subject: None });
        assert_eq!(attributed_subject(None, no_subject_link), None);
    }

    #[test]
    fn advance_does_nothing_while_paused() {
        let mut t = timer(0, 1, 0);
        t.start();
        t.pause();
        assert_eq!(t.advance(secs(30)), TimerTick::Running);
        assert_eq!(t.remaining, secs(60));
        t.resume();
        t.advance(secs(10));
        assert_eq!(t.remaining, secs(50));
    }

    #[test]
    fn reaching_zero_completes_and_returns_to_the_picker() {
        let mut t = timer(0, 0, 10);
        t.start();
        assert_eq!(t.advance(secs(10)), TimerTick::Completed { total: secs(10) });
        assert!(!t.active && !t.running);
        // The picker's selection survives for the next run.
        assert_eq!((t.hours, t.minutes, t.seconds), (0, 0, 10));
        assert!(t.can_start());
    }

    #[test]
    fn overshooting_still_completes_once() {
        let mut t = timer(0, 0, 10);
        t.start();
        assert_eq!(t.advance(secs(500)), TimerTick::Completed { total: secs(10) });
        assert_eq!(t.advance(secs(500)), TimerTick::Running);
    }

    #[test]
    fn cancel_abandons_the_countdown_without_completing() {
        let mut t = timer(0, 1, 0);
        t.start();
        t.advance(secs(30));
        t.cancel();
        assert!(!t.active && !t.running);
        assert_eq!(t.progress(), 0.0);
        assert_eq!(t.advance(secs(60)), TimerTick::Running);
    }

    #[test]
    fn resume_does_nothing_without_an_active_countdown() {
        let mut t = timer(0, 1, 0);
        t.resume();
        assert!(!t.running);
    }

    #[test]
    fn the_subject_survives_a_completed_run() {
        let mut t = timer(0, 0, 5);
        t.subject_id = Some(3);
        t.start();
        t.advance(secs(5));
        assert_eq!(t.subject_id, Some(3));
    }

    #[test]
    fn format_remaining_uses_minutes_and_seconds_under_an_hour() {
        assert_eq!(format_remaining(secs(0)), "0:00");
        assert_eq!(format_remaining(secs(59)), "0:59");
        assert_eq!(format_remaining(secs(25 * 60)), "25:00");
    }

    #[test]
    fn format_remaining_adds_an_hours_place_from_one_hour() {
        assert_eq!(format_remaining(secs(3600)), "1:00:00");
        assert_eq!(format_remaining(secs(3723)), "1:02:03");
        assert_eq!(format_remaining(secs(86_399)), "23:59:59");
    }

    #[test]
    fn format_remaining_rounds_the_last_second_up() {
        assert_eq!(format_remaining(Duration::from_millis(200)), "0:01");
        assert_eq!(format_remaining(Duration::from_millis(59_100)), "1:00");
    }

    #[test]
    fn focus_minutes_rounds_to_nearest_with_a_floor_of_one() {
        assert_eq!(focus_minutes(secs(20)), 1);
        assert_eq!(focus_minutes(secs(89)), 1);
        assert_eq!(focus_minutes(secs(90)), 2);
        assert_eq!(focus_minutes(secs(25 * 60)), 25);
        assert_eq!(focus_minutes(secs(3600)), 60);
    }
}
