//! Feature request: "Auto Assigning" — the pure planning logic for
//! choosing which day each unassigned task should be worked on.
//! Everything here is a plain function over plain values (no database,
//! no `egui`), in the same spirit as `tasks::repeat_list` and
//! `stats::period_bounds`, so the interesting decisions are
//! unit-tested directly. `database::queries::auto_assign` is the thin
//! layer that feeds it real tasks and writes its answers back.
//!
//! The request's three criteria, and where each lives:
//!
//! 1. **Proximity to deadline** — `plan` places tasks earliest-deadline
//!    first, each on the earliest day with room, and never later than
//!    its deadline if any day up to then can take it.
//! 2. **A daily task-time budget** (2 hours by default, a setting) —
//!    `plan` fills a day only up to `PlanConfig::daily_minutes`.
//! 3. **Tasks already set for a day** — the caller passes those days'
//!    existing minutes in as `load`, so flexible tasks flow around them
//!    rather than on top of them (manual assignments and top-level
//!    repeating tasks; topic-list tasks are single repeating tasks and
//!    are no longer expanded into per-day tasks).

use std::collections::HashMap;

use chrono::{Duration, NaiveDate};


/// "a day should involve 2 hours of task doing" — the default for the
/// configurable daily budget.
pub const DEFAULT_DAILY_MINUTES: i64 = 120;
/// What a task with no estimate is assumed to take. The request didn't
/// say; half an hour is a middle-of-the-road guess, and it's a setting.
pub const DEFAULT_TASK_MINUTES: i64 = 30;
/// How many days ahead (starting today) to plan.
pub const DEFAULT_HORIZON_DAYS: i64 = 14;

/// The knobs `plan` needs, gathered so a call site can't mix up three
/// bare numbers.
#[derive(Debug, Clone, Copy)]
pub struct PlanConfig {
    pub today: NaiveDate,
    /// Minutes of task work a day should hold.
    pub daily_minutes: i64,
    /// Planning covers `today ..= today + horizon_days - 1`.
    pub horizon_days: i64,
}

impl PlanConfig {
    /// The last day planning may place anything on.
    pub fn horizon_end(&self) -> NaiveDate {
        self.today + Duration::days(self.horizon_days.max(1) - 1)
    }
}

/// A task the planner is free to place on any day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlexTask {
    pub id: i64,
    /// Estimated (or defaulted) minutes; treated as at least 1.
    pub minutes: i64,
    pub due: Option<NaiveDate>,
    /// The To Do list's manual order — the final tie-breaker, so a
    /// person's own ordering still counts among equal deadlines.
    pub position: i64,
}

/// Where the planner decided a task goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    pub task_id: i64,
    pub day: NaiveDate,
    /// The day was pushed past the daily budget because the task's
    /// deadline (or an already-blown one) left nowhere else. Surfaced
    /// so the UI can say "N tasks over budget" rather than silently
    /// overloading a day.
    pub over_capacity: bool,
}

/// Whether adding `minutes` to a day already holding `load` fits.
/// A task longer than the whole budget can still take a day that's
/// otherwise empty — it's never split, and it would otherwise never be
/// placeable at all.
fn fits(load: i64, minutes: i64, daily_minutes: i64) -> bool {
    load + minutes <= daily_minutes || load == 0
}

/// Places `flexible` tasks onto days, given the minutes each day
/// already holds (`load`, updated in place as tasks are placed).
/// Returns one `Placement` per task that found a day; a task with no
/// room anywhere it's allowed to go (and no deadline forcing the
/// issue) is simply absent from the result.
///
/// Order of decisions:
/// - Tasks are taken earliest-deadline first (no deadline last, then
///   the To Do list's own order), so scarce early room goes to the
///   most urgent work.
/// - Each task goes on the earliest day, from today, that has room —
///   but not past its deadline while any day up to it can take it.
/// - If nothing up to the deadline has room, the deadline wins over
///   the budget: it goes on the least-loaded day before the deadline,
///   flagged `over_capacity`. An already-overdue task with no room
///   anywhere lands on today, likewise flagged.
/// - A task with no deadline (or one beyond the horizon) that finds no
///   room inside the horizon is left unplaced.
pub fn plan(config: &PlanConfig, load: &mut HashMap<NaiveDate, i64>, flexible: &[FlexTask]) -> Vec<Placement> {
    plan_with_capacity(config, &HashMap::new(), load, flexible)
}

/// [`plan`] with a **per-day capacity** (feature request:
/// "Time-blocking", D2). `capacity` maps a day to the free minutes the
/// time-block planner actually has that day — the working window minus
/// the calendar's events — and a day's budget becomes
/// `min(daily_minutes, capacity)`, so Auto-assign stops piling work onto
/// a day that is mostly meetings. A day with **no entry** is
/// unconstrained (the plain `daily_minutes`), and a day with a capacity
/// of `0` is skipped outright rather than taking the one oversized task
/// that [`fits`] would otherwise let an empty day absorb. An empty map
/// gives exactly [`plan`]'s behaviour.
pub fn plan_with_capacity(
    config: &PlanConfig,
    capacity: &HashMap<NaiveDate, i64>,
    load: &mut HashMap<NaiveDate, i64>,
    flexible: &[FlexTask],
) -> Vec<Placement> {
    let horizon_end = config.horizon_end();
    let daily = config.daily_minutes.max(1);
    // The budget for one day, and whether the day can take work at all.
    let budget_of = |day: NaiveDate| -> (i64, bool) {
        match capacity.get(&day) {
            Some(free) => ((*free).min(daily).max(1), *free > 0),
            None => (daily, true),
        }
    };

    let mut ordered: Vec<&FlexTask> = flexible.iter().collect();
    ordered.sort_by_key(|t| (t.due.is_none(), t.due, t.position, t.id));

    let mut placements = Vec::new();
    for task in ordered {
        let minutes = task.minutes.max(1);
        let overdue = task.due.is_some_and(|d| d < config.today);
        // A deadline only bounds the search if it's still ahead of us
        // and inside the horizon.
        let hard_deadline = task.due.filter(|d| *d >= config.today && *d <= horizon_end);
        let window_end = hard_deadline.unwrap_or(horizon_end);

        let mut chosen: Option<(NaiveDate, bool)> = None;
        let mut day = config.today;
        while day <= window_end {
            let (budget, open) = budget_of(day);
            if open && fits(load.get(&day).copied().unwrap_or(0), minutes, budget) {
                chosen = Some((day, false));
                break;
            }
            day += Duration::days(1);
        }

        if chosen.is_none() {
            if overdue {
                chosen = Some((config.today, true));
            } else if hard_deadline.is_some() {
                // Least-loaded day before the deadline, earliest on a
                // tie (min_by_key keeps the first minimum).
                let mut days = Vec::new();
                let mut d = config.today;
                while d <= window_end {
                    days.push(d);
                    d += Duration::days(1);
                }
                // Prefer a day that has any room at all; if every day up to
                // the deadline is fully booked, fall back to all of them.
                let open_days: Vec<NaiveDate> = days.iter().copied().filter(|d| budget_of(*d).1).collect();
                let candidates = if open_days.is_empty() { days } else { open_days };
                chosen = candidates
                    .into_iter()
                    .min_by_key(|d| load.get(d).copied().unwrap_or(0))
                    .map(|d| (d, true));
            }
        }

        if let Some((day, over_capacity)) = chosen {
            *load.entry(day).or_insert(0) += minutes;
            placements.push(Placement { task_id: task.id, day, over_capacity });
        }
    }
    placements
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn config() -> PlanConfig {
        // Tuesday 2026-09-29, 2h budget, 14-day horizon.
        PlanConfig { today: d(2026, 9, 29), daily_minutes: 120, horizon_days: 14 }
    }

    fn task(id: i64, minutes: i64, due: Option<NaiveDate>) -> FlexTask {
        FlexTask { id, minutes, due, position: id }
    }

    fn day_of(placements: &[Placement], id: i64) -> Option<NaiveDate> {
        placements.iter().find(|p| p.task_id == id).map(|p| p.day)
    }

    #[test]
    fn horizon_end_is_horizon_days_minus_one_days_out() {
        assert_eq!(config().horizon_end(), d(2026, 10, 12));
        let one_day = PlanConfig { horizon_days: 1, ..config() };
        assert_eq!(one_day.horizon_end(), d(2026, 9, 29));
        let zero = PlanConfig { horizon_days: 0, ..config() };
        assert_eq!(zero.horizon_end(), d(2026, 9, 29));
    }

    #[test]
    fn tasks_fill_a_day_up_to_the_budget_then_spill_to_the_next() {
        let mut load = HashMap::new();
        let tasks = vec![task(1, 60, None), task(2, 60, None), task(3, 60, None)];
        let placed = plan(&config(), &mut load, &tasks);
        assert_eq!(day_of(&placed, 1), Some(d(2026, 9, 29)));
        assert_eq!(day_of(&placed, 2), Some(d(2026, 9, 29)));
        assert_eq!(day_of(&placed, 3), Some(d(2026, 9, 30)));
        assert_eq!(load[&d(2026, 9, 29)], 120);
    }

    #[test]
    fn nearer_deadlines_are_placed_first() {
        let mut load = HashMap::new();
        // Only one 120-minute slot per day; the earlier deadline must
        // get today even though it has the higher id.
        let tasks = vec![task(1, 120, Some(d(2026, 10, 5))), task(2, 120, Some(d(2026, 9, 30)))];
        let placed = plan(&config(), &mut load, &tasks);
        assert_eq!(day_of(&placed, 2), Some(d(2026, 9, 29)));
        assert_eq!(day_of(&placed, 1), Some(d(2026, 9, 30)));
    }

    #[test]
    fn tasks_without_a_deadline_come_after_dated_ones() {
        let mut load = HashMap::new();
        let tasks = vec![task(1, 120, None), task(2, 120, Some(d(2026, 10, 10)))];
        let placed = plan(&config(), &mut load, &tasks);
        assert_eq!(day_of(&placed, 2), Some(d(2026, 9, 29)));
        assert_eq!(day_of(&placed, 1), Some(d(2026, 9, 30)));
    }

    #[test]
    fn equal_deadlines_keep_the_lists_manual_order() {
        let mut load = HashMap::new();
        let due = Some(d(2026, 10, 10));
        let tasks = vec![
            FlexTask { id: 1, minutes: 120, due, position: 5 },
            FlexTask { id: 2, minutes: 120, due, position: 1 },
        ];
        let placed = plan(&config(), &mut load, &tasks);
        assert_eq!(day_of(&placed, 2), Some(d(2026, 9, 29)));
        assert_eq!(day_of(&placed, 1), Some(d(2026, 9, 30)));
    }

    #[test]
    fn existing_load_is_planned_around() {
        // Today already holds 90 minutes of fixed work (e.g. a topic
        // task): a 60-minute task no longer fits and moves on.
        let mut load = HashMap::new();
        load.insert(d(2026, 9, 29), 90);
        let placed = plan(&config(), &mut load, &[task(1, 60, None), task(2, 30, None)]);
        assert_eq!(day_of(&placed, 1), Some(d(2026, 9, 30)));
        // The 30-minute one still fits today's remaining 30.
        assert_eq!(day_of(&placed, 2), Some(d(2026, 9, 29)));
        assert_eq!(load[&d(2026, 9, 29)], 120);
    }

    #[test]
    fn a_task_longer_than_the_budget_takes_an_empty_day_alone() {
        let mut load = HashMap::new();
        load.insert(d(2026, 9, 29), 30);
        let placed = plan(&config(), &mut load, &[task(1, 300, None)]);
        // Today isn't empty, tomorrow is.
        assert_eq!(day_of(&placed, 1), Some(d(2026, 9, 30)));
        assert!(!placed[0].over_capacity);
    }

    #[test]
    fn a_deadline_outranks_the_budget_and_is_flagged() {
        let mut load = HashMap::new();
        load.insert(d(2026, 9, 29), 120);
        load.insert(d(2026, 9, 30), 100);
        // Due tomorrow; both days are full enough that 60 doesn't fit.
        let placed = plan(&config(), &mut load, &[task(1, 60, Some(d(2026, 9, 30)))]);
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].day, d(2026, 9, 30)); // the least-loaded day
        assert!(placed[0].over_capacity);
    }

    #[test]
    fn a_task_is_never_placed_after_its_deadline_when_a_day_before_it_has_room() {
        let mut load = HashMap::new();
        load.insert(d(2026, 9, 29), 120);
        let placed = plan(&config(), &mut load, &[task(1, 60, Some(d(2026, 9, 30)))]);
        assert_eq!(placed[0].day, d(2026, 9, 30));
        assert!(!placed[0].over_capacity);
    }

    #[test]
    fn an_overdue_task_goes_to_the_earliest_day_with_room() {
        let mut load = HashMap::new();
        let placed = plan(&config(), &mut load, &[task(1, 30, Some(d(2026, 9, 1)))]);
        assert_eq!(placed[0].day, d(2026, 9, 29));
        assert!(!placed[0].over_capacity);
    }

    #[test]
    fn an_overdue_task_with_no_room_anywhere_lands_on_today_over_budget() {
        let mut load = HashMap::new();
        for offset in 0..14 {
            load.insert(config().today + Duration::days(offset), 120);
        }
        let placed = plan(&config(), &mut load, &[task(1, 30, Some(d(2026, 9, 1)))]);
        assert_eq!(placed[0].day, d(2026, 9, 29));
        assert!(placed[0].over_capacity);
    }

    #[test]
    fn an_undated_task_with_no_room_in_the_horizon_is_left_unplaced() {
        let mut load = HashMap::new();
        for offset in 0..14 {
            load.insert(config().today + Duration::days(offset), 120);
        }
        assert!(plan(&config(), &mut load, &[task(1, 30, None)]).is_empty());
    }

    #[test]
    fn a_deadline_beyond_the_horizon_does_not_bind_the_search() {
        let mut load = HashMap::new();
        let placed = plan(&config(), &mut load, &[task(1, 30, Some(d(2027, 3, 1)))]);
        assert_eq!(placed[0].day, d(2026, 9, 29));
    }

    #[test]
    fn a_zero_minute_task_still_counts_as_a_minute() {
        let mut load = HashMap::new();
        plan(&config(), &mut load, &[task(1, 0, None)]);
        assert_eq!(load[&d(2026, 9, 29)], 1);
    }

    #[test]
    fn planning_is_deterministic() {
        let tasks = vec![task(3, 45, None), task(1, 60, Some(d(2026, 10, 2))), task(2, 90, Some(d(2026, 10, 2)))];
        let first = plan(&config(), &mut HashMap::new(), &tasks);
        let second = plan(&config(), &mut HashMap::new(), &tasks);
        assert_eq!(first, second);
    }

    // --- plan_with_capacity (time-blocking D2) ------------------------------------

    fn capacity(entries: &[(NaiveDate, i64)]) -> HashMap<NaiveDate, i64> {
        entries.iter().copied().collect()
    }

    #[test]
    fn an_empty_capacity_map_is_exactly_the_plain_planner() {
        let tasks = [task(1, 90, Some(d(2026, 10, 2))), task(2, 60, None), task(3, 120, Some(d(2026, 9, 30))), task(4, 45, None)];
        let mut plain_load = HashMap::new();
        let mut cap_load = HashMap::new();
        let plain = plan(&config(), &mut plain_load, &tasks);
        let with = plan_with_capacity(&config(), &HashMap::new(), &mut cap_load, &tasks);
        assert_eq!(plain, with);
        assert_eq!(plain_load, cap_load);
    }

    #[test]
    fn a_day_with_little_free_time_takes_less_work() {
        // Today has only 60 free minutes (a day of meetings); the daily budget is 120.
        let cap = capacity(&[(d(2026, 9, 29), 60)]);
        let tasks = [task(1, 45, None), task(2, 45, None)];
        let placements = plan_with_capacity(&config(), &cap, &mut HashMap::new(), &tasks);
        assert_eq!(day_of(&placements, 1), Some(d(2026, 9, 29)));
        assert_eq!(day_of(&placements, 2), Some(d(2026, 9, 30)), "45 + 45 does not fit in 60 free minutes");
    }

    #[test]
    fn capacity_never_raises_the_budget_above_the_daily_minutes() {
        // 8 free hours do not let more than the 2-hour budget in.
        let cap = capacity(&[(d(2026, 9, 29), 480)]);
        let tasks = [task(1, 90, None), task(2, 90, None)];
        let placements = plan_with_capacity(&config(), &cap, &mut HashMap::new(), &tasks);
        assert_eq!(day_of(&placements, 2), Some(d(2026, 9, 30)));
    }

    #[test]
    fn a_fully_booked_day_is_skipped_even_for_one_task() {
        // Without capacity an empty day takes any single task; with 0 free minutes it must not.
        let cap = capacity(&[(d(2026, 9, 29), 0)]);
        let placements = plan_with_capacity(&config(), &cap, &mut HashMap::new(), &[task(1, 30, None)]);
        assert_eq!(day_of(&placements, 1), Some(d(2026, 9, 30)));
    }

    #[test]
    fn an_oversized_task_still_takes_an_open_empty_day() {
        // 200 minutes exceeds both the budget and the 90 free minutes, but the day has *some*
        // room and the time planner can split it across days, so it is not stranded.
        let cap = capacity(&[(d(2026, 9, 29), 90)]);
        let placements = plan_with_capacity(&config(), &cap, &mut HashMap::new(), &[task(1, 200, None)]);
        assert_eq!(day_of(&placements, 1), Some(d(2026, 9, 29)));
    }

    #[test]
    fn a_deadline_beats_capacity_and_prefers_a_day_that_has_room() {
        // Due tomorrow; today and tomorrow are both full for this task, but tomorrow has
        // some free time while today has none, so the forced placement goes to tomorrow.
        let cap = capacity(&[(d(2026, 9, 29), 0), (d(2026, 9, 30), 30)]);
        let mut load = HashMap::new();
        load.insert(d(2026, 9, 30), 30);
        let placements = plan_with_capacity(&config(), &cap, &mut load, &[task(1, 120, Some(d(2026, 9, 30)))]);
        assert_eq!(day_of(&placements, 1), Some(d(2026, 9, 30)));
        assert!(placements[0].over_capacity, "flagged: the deadline overrode the budget");
    }

    #[test]
    fn days_missing_from_the_capacity_map_are_unconstrained() {
        let cap = capacity(&[(d(2026, 9, 30), 0)]);
        let placements = plan_with_capacity(&config(), &cap, &mut HashMap::new(), &[task(1, 60, None)]);
        assert_eq!(day_of(&placements, 1), Some(d(2026, 9, 29)), "today has no entry, so the plain budget applies");
    }
}
