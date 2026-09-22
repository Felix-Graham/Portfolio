use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Work,
    Break,
}

/// The result of one `tick()` call — whether a phase just finished,
/// and if so which one (so the caller knows whether to log a
/// completed focus session and/or send a notification).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickResult {
    Running,
    PhaseCompleted(Phase),
}

/// A Pomodoro timer: alternates fixed-length Work/Break phases,
/// counting down in real (wall-clock) time. Timing itself uses
/// `Instant`/`Duration` rather than `chrono` — this is a short-lived,
/// in-session countdown with no calendar semantics, so a monotonic
/// clock is the right tool and sidesteps any DST/clock-change
/// weirdness a wall-clock timestamp comparison could hit.
pub struct PomodoroState {
    pub phase: Phase,
    pub running: bool,
    pub remaining: Duration,
    pub work_minutes: u32,
    pub break_minutes: u32,
    pub sessions_completed_this_run: u32,
    /// Whether the full-screen focus overlay is showing. Independent
    /// of `running` — pausing doesn't exit focus, and "minimize" exits
    /// focus without pausing.
    pub focused: bool,
    /// Feature request: a subject to attribute completed Work phases
    /// to. `None` means "just a plain Pomodoro," matching the
    /// original behavior exactly — this is opt-in. Deliberately not
    /// reset by `.reset()` (unlike `phase`/`remaining`/`running`),
    /// since picking a subject once and running several consecutive
    /// Pomodoros against it is the expected use, not something to
    /// re-pick every cycle.
    pub subject_id: Option<i64>,
    last_tick: Option<Instant>,
}

impl Default for PomodoroState {
    fn default() -> Self {
        let work_minutes = 25;
        Self {
            phase: Phase::Work,
            running: false,
            remaining: Duration::from_secs(work_minutes as u64 * 60),
            work_minutes,
            break_minutes: 5,
            sessions_completed_this_run: 0,
            focused: false,
            subject_id: None,
            last_tick: None,
        }
    }
}

impl PomodoroState {
    pub fn start(&mut self) {
        self.running = true;
        self.focused = true;
        self.last_tick = Some(Instant::now());
    }

    pub fn pause(&mut self) {
        self.running = false;
        self.last_tick = None;
    }

    pub fn resume(&mut self) {
        self.running = true;
        self.last_tick = Some(Instant::now());
    }

    pub fn reset(&mut self) {
        self.running = false;
        self.phase = Phase::Work;
        self.remaining = Duration::from_secs(self.work_minutes as u64 * 60);
        self.last_tick = None;
    }

    /// Advances the countdown by however much wall-clock time has
    /// passed since the last call, switching phases (and looping back
    /// to Work) whenever the current one runs out. Safe to call every
    /// frame regardless of `running` — it's a no-op while paused.
    pub fn tick(&mut self) -> TickResult {
        if !self.running {
            self.last_tick = None;
            return TickResult::Running;
        }

        let now = Instant::now();
        let elapsed = self.last_tick.map(|t| now.duration_since(t)).unwrap_or_default();
        self.last_tick = Some(now);

        if elapsed < self.remaining {
            self.remaining -= elapsed;
            return TickResult::Running;
        }

        let finished_phase = self.phase;
        match self.phase {
            Phase::Work => {
                self.sessions_completed_this_run += 1;
                self.phase = Phase::Break;
                self.remaining = Duration::from_secs(self.break_minutes as u64 * 60);
            }
            Phase::Break => {
                self.phase = Phase::Work;
                self.remaining = Duration::from_secs(self.work_minutes as u64 * 60);
            }
        }
        TickResult::PhaseCompleted(finished_phase)
    }

    /// This phase's total duration, for computing progress fraction.
    pub fn phase_total(&self) -> Duration {
        match self.phase {
            Phase::Work => Duration::from_secs(self.work_minutes as u64 * 60),
            Phase::Break => Duration::from_secs(self.break_minutes as u64 * 60),
        }
    }

    /// Fraction of the current phase elapsed, `0.0` to `1.0` — what
    /// the circular progress indicator draws.
    pub fn progress(&self) -> f32 {
        let total = self.phase_total().as_secs_f32();
        if total <= 0.0 {
            return 0.0;
        }
        (1.0 - self.remaining.as_secs_f32() / total).clamp(0.0, 1.0)
    }
}

/// Formats a duration as `MM:SS` for the timer's center display.
/// Pure and separate from `PomodoroState` so the formatting rule
/// itself — in particular, rounding up rather than down, so the last
/// visible second reads "0:01" instead of jumping straight from
/// "0:01" to "0:00" a full second early — is tested directly.
pub fn format_remaining(remaining: Duration) -> String {
    let total_seconds = remaining.as_secs_f64().ceil() as u64;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_in_work_phase_not_running() {
        let state = PomodoroState::default();
        assert_eq!(state.phase, Phase::Work);
        assert!(!state.running);
        assert_eq!(state.remaining, Duration::from_secs(25 * 60));
    }

    #[test]
    fn tick_while_paused_is_a_no_op() {
        let mut state = PomodoroState::default();
        let before = state.remaining;
        assert_eq!(state.tick(), TickResult::Running);
        assert_eq!(state.remaining, before);
    }

    #[test]
    fn starting_then_immediately_ticking_barely_changes_remaining() {
        let mut state = PomodoroState::default();
        state.start();
        let result = state.tick();
        assert_eq!(result, TickResult::Running);
        // Negligible time has passed — remaining should be at (or
        // extremely close to) its starting value, not equal to zero.
        assert!(state.remaining > Duration::from_secs(60 * 24));
    }

    #[test]
    fn phase_completes_and_switches_to_break() {
        let mut state = PomodoroState::default();
        state.start();
        state.remaining = Duration::from_millis(1);
        std::thread::sleep(Duration::from_millis(5));
        let result = state.tick();
        assert_eq!(result, TickResult::PhaseCompleted(Phase::Work));
        assert_eq!(state.phase, Phase::Break);
        assert_eq!(state.sessions_completed_this_run, 1);
        assert_eq!(state.remaining, Duration::from_secs(5 * 60));
    }

    #[test]
    fn break_completes_and_loops_back_to_work() {
        let mut state = PomodoroState::default();
        state.phase = Phase::Break;
        state.start();
        state.remaining = Duration::from_millis(1);
        std::thread::sleep(Duration::from_millis(5));
        let result = state.tick();
        assert_eq!(result, TickResult::PhaseCompleted(Phase::Break));
        assert_eq!(state.phase, Phase::Work);
        assert_eq!(state.remaining, Duration::from_secs(25 * 60));
    }

    #[test]
    fn reset_returns_to_the_start_state() {
        let mut state = PomodoroState::default();
        state.phase = Phase::Break;
        state.running = true;
        state.remaining = Duration::from_secs(10);
        state.reset();
        assert_eq!(state.phase, Phase::Work);
        assert!(!state.running);
        assert_eq!(state.remaining, Duration::from_secs(25 * 60));
    }

    #[test]
    fn reset_does_not_clear_the_selected_subject() {
        // Picking a subject once and running several consecutive
        // Pomodoros against it is the expected flow — resetting the
        // timer between them shouldn't force re-picking it each time.
        let mut state = PomodoroState::default();
        state.subject_id = Some(7);
        state.reset();
        assert_eq!(state.subject_id, Some(7));
    }

    #[test]
    fn progress_is_zero_at_the_start_of_a_phase() {
        let state = PomodoroState::default();
        assert_eq!(state.progress(), 0.0);
    }

    #[test]
    fn progress_is_one_when_time_is_up() {
        let mut state = PomodoroState::default();
        state.remaining = Duration::ZERO;
        assert_eq!(state.progress(), 1.0);
    }

    #[test]
    fn progress_is_roughly_half_when_half_the_time_remains() {
        let mut state = PomodoroState::default();
        state.remaining = state.phase_total() / 2;
        assert!((state.progress() - 0.5).abs() < 0.01);
    }

    #[test]
    fn format_remaining_pads_seconds_to_two_digits() {
        assert_eq!(format_remaining(Duration::from_secs(65)), "1:05");
        assert_eq!(format_remaining(Duration::from_secs(60)), "1:00");
        assert_eq!(format_remaining(Duration::from_secs(9)), "0:09");
    }

    #[test]
    fn format_remaining_rounds_up_so_the_final_second_shows_before_zero() {
        // 0.4 seconds left should still read "0:01", not "0:00" —
        // the countdown shouldn't visually hit zero before it's
        // actually over.
        assert_eq!(format_remaining(Duration::from_millis(400)), "0:01");
    }
}
