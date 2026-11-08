//! Feature request: "Auto Assigning" — the database side of
//! `tasks::auto_assign`. `run` does two things, in one transaction:
//!
//! 1. Works out how many minutes each day already holds from tasks
//!    that are fixed to it (manual assignments, repeating tasks due
//!    that day).
//! 2. Lets the planner place every remaining unassigned task around
//!    that, and writes the result back.
//!
//! Only tasks nobody has placed are moved: a manual assignment, a
//! session-linked task and a top-level repeating task (including one
//! that loops through a list of topics — a single task renamed by the
//! reset pass, never spawning per-day tasks) are all left exactly where
//! they are. Subtasks — repeating ones included — are planned like any
//! other task; a parent with incomplete subtasks is a container and is
//! skipped (its subtasks carry the work). A task the
//! planner itself placed earlier (`auto_assigned`) is re-planned on
//! every run, so new deadlines and missed days get folded in.
//!
//! With time blocking on (feature request: "Time-blocking", Phase 3),
//! `run_at` also (a) caps each day's budget by the free time the block
//! planner actually has that day, so a day of meetings takes less work,
//! and (b) finishes by planning times (`time_blocking::plan_blocks`),
//! with one feedback round: if a task the planner placed on a day then
//! turns out not to fit it, that day's budget is lowered to what really
//! fitted and Auto-assign runs once more.

use std::collections::{HashMap, HashSet};

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use rusqlite::{Connection, Result};

use super::time_blocking::{self, BlockReport};
use super::{settings, task_blocks, tasks};
use crate::tasks::auto_assign::{self, FlexTask, PlanConfig};
use crate::tasks::Task;

/// What one `run` did, for the To Do view's one-line summary.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AutoAssignReport {
    /// Tasks given a day (including ones moved to a new day).
    pub assigned: usize,
    /// Of `assigned`, how many went past the daily budget because a
    /// deadline left nowhere else.
    pub over_capacity: usize,
    /// Tasks that found no day within the horizon.
    pub unplaced: usize,
    /// The horizon used, so the summary can say "next 14 days".
    pub horizon_days: i64,
    /// What planning times did, when time blocking is on (`None` when it
    /// is off, so the summary is exactly what it always was).
    pub blocks: Option<BlockReport>,
}

impl AutoAssignReport {
    /// A short human-readable summary.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        parts.push(format!("Assigned {} task{}", self.assigned, if self.assigned == 1 { "" } else { "s" }));
        let mut text = parts.join(", ");
        text.push('.');
        if self.over_capacity > 0 {
            text.push_str(&format!(
                " {} over the daily budget (a deadline left no other room).",
                self.over_capacity
            ));
        }
        if self.unplaced > 0 {
            text.push_str(&format!(
                " {} didn't fit in the next {} days.",
                self.unplaced, self.horizon_days
            ));
        }
        if let Some(blocks) = &self.blocks {
            text.push(' ');
            text.push_str(&blocks.summary());
        }
        text
    }
}

/// The ids of tasks that are *containers*: top-level tasks with at
/// least one incomplete subtask. A container's real work is its
/// subtasks (each planned on its own — see `is_flexible`), so the
/// container itself is neither placed nor counted toward a day's
/// budget; doing both would count the same work twice.
pub fn container_ids(tasks: &[Task]) -> HashSet<i64> {
    tasks.iter().filter(|t| !t.completed).filter_map(|t| t.parent_task_id).collect()
}

/// A task the planner may place or move: not done, not linked to a
/// session (the session already covers its time), not a template or
/// generated topic task, not a container (see `container_ids`), not
/// a top-level repeating task (its day is its due date: it is a fixed
/// commitment that counts against that day, see `fixed_load`), and
/// either unassigned or previously placed by the planner rather than
/// a person.
///
/// Subtasks are planned exactly like top-level tasks — each gets its
/// own day from its own deadline, estimate and the daily budget, so a
/// large multipart task can spread across days. (Feature request,
/// answering "is the auto assignation accounting for subtasks?":
/// "a subtask should be assigned like it is not a subtask".)
///
/// That includes *repeating* subtasks. Bug fix ("subtasks are not
/// assigned with auto assign"): a repeating parent's sections repeat
/// too (they have to, to reset as a unit — see the weekly-parent tests
/// in `queries::tasks`), and every one of them used to be skipped by
/// the repeating rule above, so a weekly multipart task never got
/// planned at all. The rule's stated reason — a reset moving the due
/// date would strand a fixed assignment — doesn't hold: the reset
/// clears `assigned_date`/`auto_assigned` each cycle
/// (`reset_due_repeating_tasks`), so the next run simply plans the new
/// occurrence. Top-level repeating tasks keep the old rule.
fn is_flexible(task: &Task, containers: &HashSet<i64>) -> bool {
    !task.completed
        && task.event_id.is_none()
        && !task.plan_topics_ahead
        && task.generated_from.is_none()
        && (!task.repeat_weekly || task.parent_task_id.is_some())
        && !containers.contains(&task.id)
        && (task.assigned_date.is_none() || task.auto_assigned)
}

pub(super) fn parse_day(text: Option<&str>) -> Option<NaiveDate> {
    text.and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
}

/// The deadline the planner uses for `task`: its own due date, or —
/// for a subtask with none — its parent's, so a part of a multipart
/// task is planned in time for the whole thing ("a subtask should
/// inherit its parent's due date"). `due_by_id` maps every task id to
/// its parsed due date. A subtask's own date always wins; a parent
/// with no date leaves the subtask undated.
pub(super) fn effective_deadline(task: &Task, due_by_id: &HashMap<i64, NaiveDate>) -> Option<NaiveDate> {
    due_by_id.get(&task.id).copied().or_else(|| task.parent_task_id.and_then(|parent| due_by_id.get(&parent).copied()))
}

pub(super) fn task_minutes(task: &Task, default_minutes: i64) -> i64 {
    task.estimated_minutes.filter(|m| *m > 0).unwrap_or(default_minutes).max(1)
}

/// Minutes already committed to each day in the horizon by tasks the
/// planner won't move: whatever day a task is assigned to, or — if
/// unassigned — the day it's due (that's the day it shows up on by
/// default). Session-linked tasks, templates and containers (whose
/// subtasks carry the time instead) don't count.
fn fixed_load(
    tasks: &[Task],
    containers: &HashSet<i64>,
    config: &PlanConfig,
    default_minutes: i64,
) -> HashMap<NaiveDate, i64> {
    let horizon_end = config.horizon_end();
    let mut load: HashMap<NaiveDate, i64> = HashMap::new();
    for task in tasks {
        if is_flexible(task, containers)
            || task.plan_topics_ahead
            || task.event_id.is_some()
            || containers.contains(&task.id)
        {
            continue;
        }
        let day = parse_day(task.assigned_date.as_deref()).or_else(|| parse_day(task.due_date.as_deref()));
        if let Some(day) = day {
            if day >= config.today && day <= horizon_end {
                *load.entry(day).or_insert(0) += task_minutes(task, default_minutes);
            }
        }
    }
    load
}

/// Runs Auto-assign as of `today`, using the daily budget, default
/// task length and horizon from Settings. With time blocking on it is
/// treated as the start of `today` (nothing already past); callers that
/// know the time use [`run_at`].
pub fn run(conn: &Connection, today: NaiveDate) -> Result<AutoAssignReport> {
    run_at(conn, today, today.and_time(NaiveTime::MIN))
}

/// [`run`] as of a particular moment. With time blocking off this is
/// just the day assignment. With it on:
///
/// 1. each day's budget becomes `min(daily budget, free minutes that
///    day)` ([`time_blocking::day_capacities`]);
/// 2. the days are assigned;
/// 3. times are planned ([`time_blocking::plan_blocks`]);
/// 4. if a task *the planner placed* did not fit its day after all
///    (the free time was too fragmented, say), that day's budget is
///    lowered to the time actually placed on it and steps 2–3 run once
///    more. One round only — it is a repair, not a search — and a
///    manually assigned task that doesn't fit is just reported.
pub fn run_at(conn: &Connection, today: NaiveDate, now: NaiveDateTime) -> Result<AutoAssignReport> {
    let blocking = settings::time_blocking_enabled(conn)?;
    let mut capacity = if blocking { time_blocking::day_capacities(conn, today, now)? } else { HashMap::new() };

    let mut report = assign_pass(conn, today, &capacity)?;
    if !blocking {
        return Ok(report);
    }

    let mut blocks = time_blocking::plan_blocks(conn, today, now)?;
    let stranded = stranded_days(conn, &blocks)?;
    if !stranded.is_empty() {
        let completed: HashSet<i64> =
            tasks::list_tasks(conn)?.into_iter().filter(|t| t.completed).map(|t| t.id).collect();
        for day in stranded {
            let placed: i64 = task_blocks::list_blocks_between(conn, day, day)?
                .iter()
                .filter(|b| !b.locked && !completed.contains(&b.task_id))
                .map(|b| b.duration().num_minutes().max(0))
                .sum();
            capacity.entry(day).and_modify(|free| *free = (*free).min(placed)).or_insert(placed);
        }
        report = assign_pass(conn, today, &capacity)?;
        blocks = time_blocking::plan_blocks(conn, today, now)?;
    }
    report.blocks = Some(blocks);
    Ok(report)
}

/// The days on which a task *the planner placed* did not fit (see
/// [`run_at`] step 4), without duplicates.
fn stranded_days(conn: &Connection, blocks: &BlockReport) -> Result<Vec<NaiveDate>> {
    if blocks.unplaced.is_empty() {
        return Ok(Vec::new());
    }
    let auto: HashSet<i64> = tasks::list_tasks(conn)?.into_iter().filter(|t| t.auto_assigned).map(|t| t.id).collect();
    let mut days: Vec<NaiveDate> =
        blocks.unplaced.iter().filter(|u| auto.contains(&u.task_id)).map(|u| u.day).collect();
    days.sort();
    days.dedup();
    Ok(days)
}

/// One pass of day assignment, with each day's budget capped by
/// `capacity` (empty = no cap).
fn assign_pass(conn: &Connection, today: NaiveDate, capacity: &HashMap<NaiveDate, i64>) -> Result<AutoAssignReport> {
    let config = PlanConfig {
        today,
        daily_minutes: settings::auto_assign_daily_minutes(conn)?,
        horizon_days: settings::auto_assign_horizon_days(conn)?,
    };
    let default_minutes = settings::auto_assign_default_task_minutes(conn)?;

    let tx = conn.unchecked_transaction()?;

    let all = tasks::list_tasks(&tx)?;

    let containers = container_ids(&all);
    let mut load = fixed_load(&all, &containers, &config, default_minutes);
    let flexible: Vec<&Task> = all.iter().filter(|t| is_flexible(t, &containers)).collect();
    let due_by_id: HashMap<i64, NaiveDate> =
        all.iter().filter_map(|t| parse_day(t.due_date.as_deref()).map(|d| (t.id, d))).collect();
    let flex_input: Vec<FlexTask> = flexible
        .iter()
        .map(|t| FlexTask {
            id: t.id,
            minutes: task_minutes(t, default_minutes),
            due: effective_deadline(t, &due_by_id),
            position: t.position,
        })
        .collect();

    let placements = auto_assign::plan_with_capacity(&config, capacity, &mut load, &flex_input);
    let placed: HashMap<i64, &auto_assign::Placement> = placements.iter().map(|p| (p.task_id, p)).collect();

    let mut report = AutoAssignReport { horizon_days: config.horizon_days, ..Default::default() };
    for task in &flexible {
        match placed.get(&task.id) {
            Some(placement) => {
                let day = placement.day.format("%Y-%m-%d").to_string();
                tasks::set_auto_assignment(&tx, task.id, Some(&day))?;
                report.assigned += 1;
                if placement.over_capacity {
                    report.over_capacity += 1;
                }
            }
            None => {
                // No room this time: drop any earlier planner
                // assignment so the task falls back to showing on its
                // due date, rather than sitting on a stale day.
                if task.auto_assigned {
                    tasks::set_auto_assignment(&tx, task.id, None)?;
                }
                report.unplaced += 1;
            }
        }
    }

    // A task the planner placed earlier that has since become a
    // container (it gained an incomplete subtask) is no longer
    // planned itself; drop its stale day so its subtasks carry it.
    for task in all.iter().filter(|t| containers.contains(&t.id) && t.auto_assigned) {
        tasks::set_auto_assignment(&tx, task.id, None)?;
    }

    tx.commit()?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use crate::database::Database;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn today() -> NaiveDate {
        d(2026, 9, 29)
    }

    fn add_task(db: &Database, title: &str, due: Option<&str>, minutes: Option<i64>) -> i64 {
        let task = tasks::create_task(&db.conn, title, due, None, None).expect("create");
        tasks::update_task(&db.conn, task.id, title, "", due, None, None, false, None, minutes).expect("update");
        task.id
    }

    fn get(db: &Database, id: i64) -> Task {
        tasks::list_tasks(&db.conn).expect("list").into_iter().find(|t| t.id == id).expect("task exists")
    }

    #[test]
    fn assigns_unassigned_tasks_by_deadline_within_the_budget() {
        let db = Database::open_in_memory().expect("open");
        let late = add_task(&db, "Late", Some("2026-10-10"), Some(120));
        let soon = add_task(&db, "Soon", Some("2026-09-30"), Some(120));

        let report = run(&db.conn, today()).expect("run");
        assert_eq!(report.assigned, 2);
        assert_eq!(get(&db, soon).assigned_date.as_deref(), Some("2026-09-29"));
        assert_eq!(get(&db, late).assigned_date.as_deref(), Some("2026-09-30"));
        assert!(get(&db, soon).auto_assigned);
    }

    #[test]
    fn a_manual_assignment_is_never_moved_and_counts_against_its_day() {
        let db = Database::open_in_memory().expect("open");
        let manual = add_task(&db, "Manual", None, Some(90));
        tasks::set_assigned_date(&db.conn, manual, Some("2026-09-29")).expect("assign");
        let other = add_task(&db, "Other", None, Some(60));

        run(&db.conn, today()).expect("run");
        assert_eq!(get(&db, manual).assigned_date.as_deref(), Some("2026-09-29"));
        assert!(!get(&db, manual).auto_assigned);
        // 90 already fixed today, so 60 doesn't fit and moves on.
        assert_eq!(get(&db, other).assigned_date.as_deref(), Some("2026-09-30"));
    }

    fn add_subtask(db: &Database, parent: i64, title: &str, due: Option<&str>, minutes: Option<i64>) -> i64 {
        let id = add_task(db, title, due, minutes);
        tasks::update_task(&db.conn, id, title, "", due, None, None, false, Some(parent), minutes).expect("nest");
        id
    }

    #[test]
    fn completed_session_linked_and_repeating_tasks_are_not_assigned() {
        let db = Database::open_in_memory().expect("open");
        let done = add_task(&db, "Done", None, None);
        tasks::set_completed(&db.conn, done, true).expect("complete");
        let repeating = add_task(&db, "Repeating", Some("2026-10-01"), None);
        tasks::update_task(&db.conn, repeating, "Repeating", "", Some("2026-10-01"), None, None, true, None, None)
            .expect("repeat");

        run(&db.conn, today()).expect("run");
        assert_eq!(get(&db, done).assigned_date, None);
        assert_eq!(get(&db, repeating).assigned_date, None);
    }

    #[test]
    fn subtasks_are_planned_like_ordinary_tasks_and_can_spread_across_days() {
        let db = Database::open_in_memory().expect("open");
        let parent = add_task(&db, "Essay", None, None);
        let outline = add_subtask(&db, parent, "Outline", Some("2026-10-02"), Some(120));
        let draft = add_subtask(&db, parent, "Draft", Some("2026-10-05"), Some(120));
        let polish = add_subtask(&db, parent, "Polish", Some("2026-10-09"), Some(120));

        let report = run(&db.conn, today()).expect("run");
        assert_eq!(report.assigned, 3);
        // One part per day, earliest deadline first.
        assert_eq!(get(&db, outline).assigned_date.as_deref(), Some("2026-09-29"));
        assert_eq!(get(&db, draft).assigned_date.as_deref(), Some("2026-09-30"));
        assert_eq!(get(&db, polish).assigned_date.as_deref(), Some("2026-10-01"));
        assert!(get(&db, outline).auto_assigned);
    }

    #[test]
    fn an_undated_subtask_inherits_its_parents_due_date_as_its_deadline() {
        let db = Database::open_in_memory().expect("open");
        // `other` is older (earlier in the list) and undated; without
        // inheritance it would win today over the undated subtask.
        let other = add_task(&db, "Other", None, Some(120));
        let parent = add_task(&db, "Essay", Some("2026-09-30"), None);
        let part = add_subtask(&db, parent, "Part", None, Some(120));

        run(&db.conn, today()).expect("run");
        assert_eq!(get(&db, part).assigned_date.as_deref(), Some("2026-09-29"));
        assert_eq!(get(&db, other).assigned_date.as_deref(), Some("2026-09-30"));
    }

    #[test]
    fn an_inherited_deadline_binds_the_subtask_like_its_own() {
        let db = Database::open_in_memory().expect("open");
        // Today is full of fixed work and the parent is due today, so
        // the (undated) subtask can only go over budget on today.
        let fixed = add_task(&db, "Fixed", None, Some(120));
        tasks::set_assigned_date(&db.conn, fixed, Some("2026-09-29")).expect("assign");
        let parent = add_task(&db, "Essay", Some("2026-09-29"), None);
        let part = add_subtask(&db, parent, "Part", None, Some(60));

        let report = run(&db.conn, today()).expect("run");
        assert_eq!(get(&db, part).assigned_date.as_deref(), Some("2026-09-29"));
        assert_eq!(report.over_capacity, 1);
    }

    #[test]
    fn a_subtasks_own_due_date_wins_over_its_parents() {
        let db = Database::open_in_memory().expect("open");
        let parent = add_task(&db, "Essay", Some("2026-10-10"), None);
        let part = add_subtask(&db, parent, "Part", Some("2026-09-30"), Some(120));
        let rival = add_task(&db, "Rival", Some("2026-10-01"), Some(120));

        run(&db.conn, today()).expect("run");
        // Part (due 09-30) is more urgent than Rival (10-01), not
        // than-or-equal to its parent's 10-10.
        assert_eq!(get(&db, part).assigned_date.as_deref(), Some("2026-09-29"));
        assert_eq!(get(&db, rival).assigned_date.as_deref(), Some("2026-09-30"));
    }

    #[test]
    fn a_subtask_of_an_undated_parent_stays_undated() {
        let db = Database::open_in_memory().expect("open");
        let parent = add_task(&db, "Essay", None, None);
        let part = add_subtask(&db, parent, "Part", None, Some(120));
        let dated = add_task(&db, "Dated", Some("2026-10-10"), Some(120));

        run(&db.conn, today()).expect("run");
        // Dated tasks come before undated ones.
        assert_eq!(get(&db, dated).assigned_date.as_deref(), Some("2026-09-29"));
        assert_eq!(get(&db, part).assigned_date.as_deref(), Some("2026-09-30"));
    }

    #[test]
    fn a_parent_with_incomplete_subtasks_is_a_container_and_is_not_planned_or_counted() {
        let db = Database::open_in_memory().expect("open");
        let parent = add_task(&db, "Essay", Some("2026-10-02"), Some(120));
        let part = add_subtask(&db, parent, "Part", None, Some(120));

        run(&db.conn, today()).expect("run");
        assert_eq!(get(&db, parent).assigned_date, None);
        // The parent's own 120 minutes (due Oct 2) must not crowd
        // anything; the part takes today in full.
        assert_eq!(get(&db, part).assigned_date.as_deref(), Some("2026-09-29"));
    }

    #[test]
    fn a_container_does_not_use_up_its_due_days_budget() {
        let db = Database::open_in_memory().expect("open");
        // The parent is due today with a 120-minute estimate. If it
        // were counted, it would fill today and push `urgent` out.
        let parent = add_task(&db, "Essay", Some("2026-09-29"), Some(120));
        // The part has its own, later deadline, so it doesn't inherit
        // the parent's "due today" and ranks after `urgent`.
        let part = add_subtask(&db, parent, "Part", Some("2026-10-20"), Some(30));
        let urgent = add_task(&db, "Urgent", Some("2026-09-30"), Some(120));

        run(&db.conn, today()).expect("run");
        assert_eq!(get(&db, urgent).assigned_date.as_deref(), Some("2026-09-29"));
        // Today is now full, so the part moves to tomorrow.
        assert_eq!(get(&db, part).assigned_date.as_deref(), Some("2026-09-30"));
        assert_eq!(get(&db, parent).assigned_date, None);
    }

    #[test]
    fn a_parent_whose_subtasks_are_all_done_is_planned_like_any_task() {
        let db = Database::open_in_memory().expect("open");
        let parent = add_task(&db, "Essay", None, Some(30));
        let part = add_subtask(&db, parent, "Part", None, Some(30));
        tasks::set_completed(&db.conn, part, true).expect("complete part");

        run(&db.conn, today()).expect("run");
        assert_eq!(get(&db, parent).assigned_date.as_deref(), Some("2026-09-29"));
    }

    #[test]
    fn a_planned_parent_that_gains_a_subtask_loses_its_own_assignment() {
        let db = Database::open_in_memory().expect("open");
        let parent = add_task(&db, "Essay", None, Some(30));
        run(&db.conn, today()).expect("first run");
        assert!(get(&db, parent).auto_assigned);

        add_subtask(&db, parent, "Part", None, Some(30));
        run(&db.conn, today()).expect("second run");
        assert_eq!(get(&db, parent).assigned_date, None);
        assert!(!get(&db, parent).auto_assigned);
    }

    #[test]
    fn a_hand_assigned_subtask_is_never_moved_and_counts_against_its_day() {
        let db = Database::open_in_memory().expect("open");
        let parent = add_task(&db, "Essay", None, None);
        let manual = add_subtask(&db, parent, "Manual part", None, Some(90));
        tasks::set_assigned_date(&db.conn, manual, Some("2026-09-29")).expect("assign");
        let other = add_subtask(&db, parent, "Other part", None, Some(60));

        run(&db.conn, today()).expect("run");
        assert_eq!(get(&db, manual).assigned_date.as_deref(), Some("2026-09-29"));
        assert_eq!(get(&db, other).assigned_date.as_deref(), Some("2026-09-30"));
    }

    #[test]
    fn a_repeating_subtask_is_planned_like_any_other() {
        let db = Database::open_in_memory().expect("open");
        // A weekly parent whose sections repeat with it — the shape the
        // To Do view produces — used to get no day at all.
        let parent = add_task(&db, "Weekly", Some("2026-10-05"), None);
        tasks::update_task(&db.conn, parent, "Weekly", "", Some("2026-10-05"), None, None, true, None, None)
            .expect("repeat parent");
        let part = add_subtask(&db, parent, "Part", Some("2026-10-05"), Some(60));
        tasks::update_task(&db.conn, part, "Part", "", Some("2026-10-05"), None, None, true, Some(parent), Some(60))
            .expect("repeat part");

        let report = run(&db.conn, today()).expect("run");
        assert_eq!(report.assigned, 1);
        let planned = get(&db, part);
        assert_eq!(planned.assigned_date.as_deref(), Some("2026-09-29"));
        assert!(planned.auto_assigned);
        // The container itself is still not planned, repeating or not.
        assert_eq!(get(&db, parent).assigned_date, None);
    }

    #[test]
    fn a_repeating_subtasks_plan_does_not_outlive_its_occurrence() {
        let db = Database::open_in_memory().expect("open");
        let parent = add_task(&db, "Weekly", Some("2026-09-22"), None);
        tasks::update_task(&db.conn, parent, "Weekly", "", Some("2026-09-22"), None, None, true, None, None)
            .expect("repeat parent");
        let part = add_subtask(&db, parent, "Part", Some("2026-09-22"), Some(60));
        tasks::update_task(&db.conn, part, "Part", "", Some("2026-09-22"), None, None, true, Some(parent), Some(60))
            .expect("repeat part");
        // Planned for an earlier day in the current occurrence...
        tasks::set_auto_assignment(&db.conn, part, Some("2026-09-21")).expect("plan");

        // ...then the cycle rolls over: the reset moves the due date on
        // and drops the stale plan, and the next run plans the new one.
        tasks::reset_due_repeating_tasks(&db.conn, today()).expect("reset");
        assert_eq!(get(&db, part).assigned_date, None);
        assert_eq!(get(&db, part).due_date.as_deref(), Some("2026-09-29"));
        run(&db.conn, today()).expect("run");
        assert_eq!(get(&db, part).assigned_date.as_deref(), Some("2026-09-29"));
    }

    #[test]
    fn a_repeating_top_level_task_is_still_left_alone_and_counts_against_its_day() {
        let db = Database::open_in_memory().expect("open");
        let repeating = add_task(&db, "Weekly", Some("2026-09-29"), Some(300));
        tasks::update_task(&db.conn, repeating, "Weekly", "", Some("2026-09-29"), None, None, true, None, Some(300))
            .expect("repeat");
        let other = add_task(&db, "Other", Some("2026-10-05"), Some(60));

        run(&db.conn, today()).expect("run");
        assert_eq!(get(&db, repeating).assigned_date, None);
        // Today is already full (300 minutes of fixed work in a default
        // 5-hour budget), so the other task goes to the next day.
        assert_eq!(get(&db, other).assigned_date.as_deref(), Some("2026-09-30"));
    }

    #[test]
    fn running_twice_gives_the_same_plan() {
        let db = Database::open_in_memory().expect("open");
        let a = add_task(&db, "A", Some("2026-10-02"), Some(100));
        let b = add_task(&db, "B", Some("2026-10-05"), Some(100));
        let c = add_task(&db, "C", None, Some(45));

        run(&db.conn, today()).expect("first run");
        let first: Vec<_> = [a, b, c].iter().map(|id| get(&db, *id).assigned_date).collect();
        run(&db.conn, today()).expect("second run");
        let second: Vec<_> = [a, b, c].iter().map(|id| get(&db, *id).assigned_date).collect();
        assert_eq!(first, second);
    }

    #[test]
    fn a_task_that_no_longer_fits_loses_its_planner_assignment() {
        let db = Database::open_in_memory().expect("open");
        settings::set_auto_assign_horizon_days(&db.conn, 1).expect("horizon");
        let a = add_task(&db, "A", None, Some(120));
        run(&db.conn, today()).expect("first run");
        assert_eq!(get(&db, a).assigned_date.as_deref(), Some("2026-09-29"));

        // Something fixed now fills the only day in the horizon.
        let fixed = add_task(&db, "Fixed", None, Some(120));
        tasks::set_assigned_date(&db.conn, fixed, Some("2026-09-29")).expect("assign");
        let report = run(&db.conn, today()).expect("second run");
        assert_eq!(report.unplaced, 1);
        assert_eq!(get(&db, a).assigned_date, None);
        assert!(!get(&db, a).auto_assigned);
    }

    #[test]
    fn a_missed_planner_assignment_is_replanned_from_today() {
        let db = Database::open_in_memory().expect("open");
        let a = add_task(&db, "A", None, Some(30));
        tasks::set_auto_assignment(&db.conn, a, Some("2026-09-20")).expect("old plan");

        run(&db.conn, today()).expect("run");
        assert_eq!(get(&db, a).assigned_date.as_deref(), Some("2026-09-29"));
    }

    #[test]
    fn settings_change_the_budget_and_default_task_length() {
        let db = Database::open_in_memory().expect("open");
        settings::set_auto_assign_daily_minutes(&db.conn, 60).expect("budget");
        settings::set_auto_assign_default_task_minutes(&db.conn, 60).expect("default");
        let a = add_task(&db, "A", None, None);
        let b = add_task(&db, "B", None, None);

        run(&db.conn, today()).expect("run");
        // Each defaults to 60 minutes and the day holds 60.
        assert_eq!(get(&db, a).assigned_date.as_deref(), Some("2026-09-29"));
        assert_eq!(get(&db, b).assigned_date.as_deref(), Some("2026-09-30"));
    }

    #[test]
    fn auto_assign_never_creates_per_day_tasks_for_a_topic_list_task() {
        // "Tasks which repeat and loop through a list of topics should
        // only show the parent task" — Auto-assign no longer spawns a
        // task per day, and leaves the single repeating task alone.
        let db = Database::open_in_memory().expect("open");
        let id = add_task(&db, "Vocab", Some("2026-09-29"), Some(20));
        tasks::update_task(&db.conn, id, "Vocab", "", Some("2026-09-29"), None, None, true, None, Some(20)).expect("repeat");
        tasks::set_repeat_interval_days(&db.conn, id, 1).expect("daily");
        tasks::set_repeat_list(
            &db.conn,
            id,
            true,
            "Vocab",
            "A\nB\nC",
            crate::tasks::repeat_list::RepeatListMode::Sequential,
        )
        .expect("list");

        let report = run(&db.conn, today()).expect("run");

        assert_eq!(report.assigned, 0);
        assert_eq!(tasks::list_tasks(&db.conn).expect("list").len(), 1, "still just the one task");
        assert_eq!(get(&db, id).assigned_date, None);
    }

    #[test]
    fn a_manual_assignment_clears_the_auto_flag_and_the_planner_leaves_it_alone() {
        let db = Database::open_in_memory().expect("open");
        let id = add_task(&db, "A", None, Some(30));
        run(&db.conn, today()).expect("run");
        assert!(get(&db, id).auto_assigned);

        tasks::set_assigned_date(&db.conn, id, Some("2026-10-05")).expect("manual");
        assert!(!get(&db, id).auto_assigned);
        run(&db.conn, today()).expect("rerun");
        assert_eq!(get(&db, id).assigned_date.as_deref(), Some("2026-10-05"));
    }

    #[test]
    fn summary_reads_naturally() {
        let report = AutoAssignReport { assigned: 9, over_capacity: 1, unplaced: 2, horizon_days: 14, ..Default::default() };
        assert_eq!(
            report.summary(),
            "Assigned 9 tasks. 1 over the daily budget (a deadline left no other room). 2 didn't fit in the next 14 days."
        );
        let quiet = AutoAssignReport { assigned: 1, horizon_days: 14, ..Default::default() };
        assert_eq!(quiet.summary(), "Assigned 1 task.");
        assert_eq!(AutoAssignReport::default().summary(), "Assigned 0 tasks.");
    }

    #[test]
    fn the_horizon_helper_is_inclusive_of_today() {
        let config = PlanConfig { today: today(), daily_minutes: 120, horizon_days: 3 };
        assert_eq!(config.horizon_end(), today() + Duration::days(2));
    }

    // --- Time-blocking integration (Phase 3) ---------------------------------------------

    use chrono::{Local, TimeZone};

    fn local_at(day: NaiveDate, h: u32, m: u32) -> chrono::DateTime<Local> {
        Local.from_local_datetime(&day.and_hms_opt(h, m, 0).unwrap()).earliest().unwrap()
    }

    fn early() -> NaiveDateTime {
        today().pred_opt().unwrap().and_hms_opt(20, 0, 0).unwrap()
    }

    fn block_event(db: &Database, day: NaiveDate, h1: u32, h2: u32) {
        crate::database::queries::events::create_event(
            &db.conn,
            "Busy",
            "",
            local_at(day, h1, 0),
            local_at(day, h2, 0),
            None,
            None,
        )
        .expect("create event");
    }

    #[test]
    fn with_time_blocking_off_nothing_about_the_run_changes() {
        let db = Database::open_in_memory().expect("open");
        add_task(&db, "A", None, Some(60));

        let report = run_at(&db.conn, today(), early()).expect("run");

        assert_eq!(report.blocks, None);
        assert_eq!(report.summary(), "Assigned 1 task.");
        assert!(task_blocks::list_blocks_between(&db.conn, today(), today() + Duration::days(30)).unwrap().is_empty());
    }

    #[test]
    fn with_time_blocking_on_the_run_also_plans_times_and_says_so() {
        let db = Database::open_in_memory().expect("open");
        settings::set_time_blocking_enabled(&db.conn, true).expect("enable");
        // 30 minutes is not over the split threshold, so it is one block.
        let t = add_task(&db, "A", None, Some(30));

        let report = run_at(&db.conn, today(), early()).expect("run");

        let blocks = task_blocks::list_blocks_for_task(&db.conn, t).expect("blocks");
        assert_eq!(blocks.len(), 1);
        assert_eq!(report.blocks.as_ref().map(|b| b.placed_blocks), Some(1));
        assert_eq!(report.summary(), "Assigned 1 task. Placed 1 block for 1 task.");
    }

    #[test]
    fn a_day_full_of_meetings_takes_less_work() {
        let db = Database::open_in_memory().expect("open");
        settings::set_time_blocking_enabled(&db.conn, true).expect("enable");
        // Today is booked 09:00-20:00, leaving one free hour of a 12-hour window.
        block_event(&db, today(), 9, 20);
        let a = add_task(&db, "A", None, Some(60));
        let b = add_task(&db, "B", None, Some(60));

        run_at(&db.conn, today(), early()).expect("run");

        let days: Vec<_> = [a, b].iter().map(|id| get(&db, *id).assigned_date.unwrap()).collect();
        assert!(days.contains(&"2026-09-29".to_string()), "one hour fits today: {days:?}");
        assert!(days.contains(&"2026-09-30".to_string()), "the other moves on: {days:?}");
    }

    #[test]
    fn without_time_blocking_the_same_day_takes_both() {
        // The control for the test above: the plain 2-hour budget ignores the meetings.
        let db = Database::open_in_memory().expect("open");
        block_event(&db, today(), 9, 20);
        let a = add_task(&db, "A", None, Some(60));
        let b = add_task(&db, "B", None, Some(60));

        run_at(&db.conn, today(), early()).expect("run");

        assert_eq!(get(&db, a).assigned_date.as_deref(), Some("2026-09-29"));
        assert_eq!(get(&db, b).assigned_date.as_deref(), Some("2026-09-29"));
    }

    #[test]
    fn a_task_that_does_not_fit_its_fragmented_day_is_moved_on_by_the_feedback_round() {
        let db = Database::open_in_memory().expect("open");
        settings::set_time_blocking_enabled(&db.conn, true).expect("enable");
        settings::set_max_chunks(&db.conn, 1).expect("never split");
        // Two separate 60-minute gaps on today (09:00-10:00, 11:00-12:00): 120 free
        // minutes, so the day looks big enough for a 90-minute task — but no single gap holds it.
        block_event(&db, today(), 10, 11);
        block_event(&db, today(), 12, 21);
        let t = add_task(&db, "Ninety minutes", None, Some(90));

        let report = run_at(&db.conn, today(), early()).expect("run");

        assert_eq!(get(&db, t).assigned_date.as_deref(), Some("2026-09-30"), "moved to a day it fits");
        let blocks = task_blocks::list_blocks_for_task(&db.conn, t).expect("blocks");
        assert_eq!(blocks.len(), 1);
        assert!(report.blocks.expect("planned").unplaced.is_empty(), "nothing left stranded");
    }

    #[test]
    fn a_manually_assigned_task_that_does_not_fit_is_reported_not_moved() {
        let db = Database::open_in_memory().expect("open");
        settings::set_time_blocking_enabled(&db.conn, true).expect("enable");
        settings::set_max_chunks(&db.conn, 1).expect("never split");
        block_event(&db, today(), 10, 11);
        block_event(&db, today(), 12, 21);
        let t = add_task(&db, "Manual", None, Some(90));
        tasks::set_assigned_date(&db.conn, t, Some("2026-09-29")).expect("assign by hand");

        let report = run_at(&db.conn, today(), early()).expect("run");

        assert_eq!(get(&db, t).assigned_date.as_deref(), Some("2026-09-29"), "the person's day stands");
        assert_eq!(report.blocks.expect("planned").unplaced.len(), 1);
    }

    #[test]
    fn the_summary_appends_the_block_report_only_when_there_is_one() {
        let block_report = BlockReport { placed_blocks: 5, tasks_placed: 4, horizon_days: 14, ..Default::default() };
        let with = AutoAssignReport { assigned: 4, horizon_days: 14, blocks: Some(block_report), ..Default::default() };
        assert_eq!(with.summary(), "Assigned 4 tasks. Placed 5 blocks for 4 tasks.");
    }

    #[test]
    fn auto_assign_steers_work_away_from_a_day_off() {
        use crate::tasks::time_blocking::WorkDay;
        let db = Database::open_in_memory().expect("open");
        settings::set_time_blocking_enabled(&db.conn, true).expect("enable");
        // "Today" in these tests is Tuesday 29 Sep 2026; make Tuesdays a day off.
        settings::set_weekday_override(&db.conn, chrono::Weekday::Tue, Some(WorkDay::Off)).expect("tuesday off");
        let t = add_task(&db, "A", None, Some(60));

        run_at(&db.conn, today(), early()).expect("run");

        assert_eq!(get(&db, t).assigned_date.as_deref(), Some("2026-09-30"), "Wednesday, not the day off");
    }

    #[test]
    fn a_day_off_does_nothing_to_auto_assign_while_time_blocking_is_off() {
        use crate::tasks::time_blocking::WorkDay;
        let db = Database::open_in_memory().expect("open");
        settings::set_weekday_override(&db.conn, chrono::Weekday::Tue, Some(WorkDay::Off)).expect("tuesday off");
        let t = add_task(&db, "A", None, Some(60));

        run_at(&db.conn, today(), early()).expect("run");

        assert_eq!(get(&db, t).assigned_date.as_deref(), Some("2026-09-29"), "the setting is inert without time blocking");
    }
}
