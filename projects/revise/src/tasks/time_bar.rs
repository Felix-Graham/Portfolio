//! Feature request: task "bars" — a small bar next to a task's name that
//! fills as time is logged against it (a stopwatch, timer or Pomodoro run
//! with the task picked), measured against the task's time budget.
//!
//! This is the maths only, so it can be tested without a screen:
//!
//! * [`TaskProgress`] — minutes spent against a budget, and the words for it;
//! * [`ratio`] / [`fill`] — how much of the bar is filled, and how far the
//!   fill has broken out past the right-hand border;
//! * [`color_at`] — one continuous colour scale: red → orange → green as the
//!   budget is used up (0%–100%), then green → blue → light blue as it is
//!   exceeded (100%–200%).
//!
//! `ui::components::task_bar` draws it.

/// Where the overfill stops growing: at twice the budget the fill reaches
/// one extra bar-width past the border and the colour is light blue. Time
/// beyond that still counts (the tooltip says so) but the bar stays put, so
/// a long overrun can't push a row's text off the screen.
pub const OVERFILL_CAP: f32 = 2.0;

pub type Rgb = [u8; 3];

const RED: Rgb = [0xE0, 0x4F, 0x4F];
const ORANGE: Rgb = [0xF0, 0x9B, 0x3A];
const GREEN: Rgb = [0x3C, 0xB0, 0x5A];
const BLUE: Rgb = [0x3B, 0x82, 0xF0];
const LIGHT_BLUE: Rgb = [0x9A, 0xD8, 0xFF];

/// The colour scale: `(ratio, colour)` stops, joined by straight lines.
const STOPS: [(f32, Rgb); 5] = [(0.0, RED), (0.5, ORANGE), (1.0, GREEN), (1.5, BLUE), (OVERFILL_CAP, LIGHT_BLUE)];

/// Time spent on a task against what it was budgeted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TaskProgress {
    /// Minutes logged against the task.
    pub spent: i64,
    /// The task's budget in minutes (at least one).
    pub budget: i64,
}

impl TaskProgress {
    pub fn new(spent: i64, budget: i64) -> Self {
        Self { spent: spent.max(0), budget: budget.max(1) }
    }

    /// Spent ÷ budget, **not** capped: `1.0` is exactly on budget.
    pub fn ratio(&self) -> f32 {
        ratio(self.spent, self.budget)
    }

    /// Whole percent, rounded.
    pub fn percent(&self) -> i64 {
        (self.spent * 100 + self.budget / 2) / self.budget.max(1)
    }

    pub fn is_over(&self) -> bool {
        self.spent > self.budget
    }

    /// Several tasks as one (a parent's bar is its subtasks' together).
    pub fn combine(parts: impl IntoIterator<Item = TaskProgress>) -> TaskProgress {
        let (spent, budget) = parts.into_iter().fold((0, 0), |(s, b), p| (s + p.spent, b + p.budget));
        TaskProgress::new(spent, budget)
    }

    /// The hover text.
    pub fn describe(&self) -> String {
        let base = format!("{} of {} min ({}%)", self.spent, self.budget, self.percent());
        if self.is_over() {
            format!("{base} — {} min over", self.spent - self.budget)
        } else {
            base
        }
    }
}

/// Spent ÷ budget; a budget of nothing counts as fully overfilled if any
/// time was spent and empty if none was.
pub fn ratio(spent: i64, budget: i64) -> f32 {
    if budget <= 0 {
        return if spent > 0 { OVERFILL_CAP } else { 0.0 };
    }
    spent.max(0) as f32 / budget as f32
}

/// How much of the bar is filled, as fractions of the bar's own width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarFill {
    /// Filled inside the border, `0.0..=1.0`.
    pub inside: f32,
    /// Filled *past* the right-hand border, `0.0..=1.0` of one bar width.
    pub overflow: f32,
}

pub fn fill(ratio: f32) -> BarFill {
    let ratio = if ratio.is_finite() { ratio.max(0.0) } else { 0.0 };
    BarFill { inside: ratio.min(1.0), overflow: (ratio - 1.0).clamp(0.0, OVERFILL_CAP - 1.0) }
}

/// The fill colour at `ratio` on the scale described in the module docs.
pub fn color_at(ratio: f32) -> Rgb {
    let ratio = if ratio.is_finite() { ratio.clamp(0.0, OVERFILL_CAP) } else { 0.0 };
    for pair in STOPS.windows(2) {
        let ((a_at, a), (b_at, b)) = (pair[0], pair[1]);
        if ratio <= b_at {
            let t = (ratio - a_at) / (b_at - a_at);
            let mix = |i: usize| (f32::from(a[i]) + (f32::from(b[i]) - f32::from(a[i])) * t).round() as u8;
            return [mix(0), mix(1), mix(2)];
        }
    }
    LIGHT_BLUE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untouched_task_is_an_empty_red_bar() {
        let p = TaskProgress::new(0, 60);
        assert_eq!(p.ratio(), 0.0);
        assert_eq!(fill(p.ratio()), BarFill { inside: 0.0, overflow: 0.0 });
        assert_eq!(color_at(p.ratio()), RED);
    }

    #[test]
    fn the_fill_is_the_share_of_the_budget_spent() {
        let half = fill(TaskProgress::new(30, 60).ratio());
        assert_eq!(half, BarFill { inside: 0.5, overflow: 0.0 });
        assert_eq!(color_at(0.5), ORANGE, "half way is orange");
    }

    #[test]
    fn a_task_exactly_on_budget_is_full_and_green_but_has_not_broken_out() {
        let p = TaskProgress::new(60, 60);
        assert_eq!(fill(p.ratio()), BarFill { inside: 1.0, overflow: 0.0 });
        assert_eq!(color_at(p.ratio()), GREEN);
        assert!(!p.is_over());
    }

    #[test]
    fn spending_more_than_the_budget_breaks_out_past_the_border() {
        let p = TaskProgress::new(90, 60);
        let f = fill(p.ratio());
        assert_eq!(f.inside, 1.0);
        assert!((f.overflow - 0.5).abs() < 1e-6, "half a bar-width past the border");
        assert!(p.is_over());
        assert_eq!(color_at(p.ratio()), BLUE, "and blue half way through the overfill");
    }

    #[test]
    fn the_overfill_stops_growing_at_double_the_budget() {
        let f = fill(TaskProgress::new(600, 60).ratio());
        assert_eq!(f.overflow, OVERFILL_CAP - 1.0);
        assert_eq!(color_at(10.0), LIGHT_BLUE);
    }

    #[test]
    fn the_colour_runs_red_orange_green_blue_light_blue_and_never_jumps() {
        assert_eq!(color_at(0.0), RED);
        assert_eq!(color_at(0.5), ORANGE);
        assert_eq!(color_at(1.0), GREEN);
        assert_eq!(color_at(1.5), BLUE);
        assert_eq!(color_at(2.0), LIGHT_BLUE);
        // Between two stops it is a blend, step by step.
        let a = color_at(0.25);
        assert!(a != RED && a != ORANGE);
        let mut last = color_at(0.0);
        for i in 1..=200 {
            let c = color_at(i as f32 / 100.0);
            let jump = (0..3).map(|k| (i16::from(c[k]) - i16::from(last[k])).abs()).max().unwrap();
            assert!(jump <= 12, "colour jumped by {jump} at ratio {}", i as f32 / 100.0);
            last = c;
        }
    }

    #[test]
    fn nonsense_ratios_are_tamed() {
        assert_eq!(color_at(f32::NAN), RED);
        assert_eq!(color_at(-3.0), RED);
        assert_eq!(fill(f32::INFINITY), BarFill { inside: 0.0, overflow: 0.0 });
        assert_eq!(ratio(5, 0), OVERFILL_CAP);
        assert_eq!(ratio(0, 0), 0.0);
    }

    #[test]
    fn the_hover_text_gives_minutes_percent_and_any_overrun() {
        assert_eq!(TaskProgress::new(45, 60).describe(), "45 of 60 min (75%)");
        assert_eq!(TaskProgress::new(80, 60).describe(), "80 of 60 min (133%) — 20 min over");
    }

    #[test]
    fn a_parents_progress_is_its_subtasks_together() {
        let total = TaskProgress::combine([TaskProgress::new(30, 60), TaskProgress::new(30, 30)]);
        assert_eq!(total, TaskProgress::new(60, 90));
    }
}
