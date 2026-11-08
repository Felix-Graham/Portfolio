//! How a planned time block meets what actually happens: a stopwatch or
//! timer is started near it, it adjusts to the time really spent, a task
//! is ticked off early, or the block passes with the work unfinished.
//!
//! The rules, as the person gave them:
//!
//! 1. **A block is started when a stopwatch or timer is started within a
//!    tolerance of its location** ([`link_session_start`]). Near means
//!    the session began within `block_start_tolerance_minutes` either
//!    side of the block's start. A session that names a subject only
//!    matches a block whose task has that subject; one with no subject
//!    matches any. The nearest block wins. The block goes *in progress*.
//! 2. **A stopwatch's block adjusts for overflow**
//!    ([`finish_linked_block`]): when the session ends the block becomes
//!    the span actually worked — longer if it ran over, shorter if it
//!    stopped early — and is *done*. (A timer's block is settled the same
//!    way, to the span it really ran; its length is fixed, so "overflow"
//!    only means it was set longer than the block.)
//! 3. **Completing the task before the block ends updates the block to
//!    the time spent** ([`on_task_completed`]): a block under way is cut
//!    off at the moment of completion; one still in the future had no
//!    time spent on it and is removed; one already over is kept as it was.
//! 4. **If the task is not complete at the end of the block, a new block
//!    is created at least one day ahead** ([`carry_over`]): the task's
//!    estimate drops by the time spent, it moves to tomorrow (or stays on a
//!    later day it already had), and one block is planned for what is left
//!    without disturbing any other block. A block that simply passes
//!    unworked is handled the same way ([`carry_over_missed`]).
//! 5. **Time in blocks from stopwatches or timers is added to stats.** The
//!    session takes the block's *task's* subject when it has none of its
//!    own, so the existing pipeline (the one-off event a finished
//!    stopwatch/timer already creates for its subject, which Hours Per
//!    Subject sums) counts it — once, with no second source of subject time.

use chrono::{DateTime, Duration, Local, NaiveDate};
use rusqlite::{params, Connection, Result};

use super::time_blocking::{block_config, plan_task_blocks};
use super::{settings, task_blocks, tasks};
use crate::tasks::BlockStatus;

/// The block a session was matched to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedBlock {
    pub block_id: i64,
    pub task_id: i64,
    pub task_title: String,
    /// The task's subject, for a session that has none of its own.
    pub subject_id: Option<i64>,
}

/// What carrying unfinished work forward did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarryOutcome {
    pub task_id: i64,
    /// The task's estimate afterwards: what is still to do.
    pub remaining_minutes: i64,
    /// The day the task now belongs to.
    pub new_day: NaiveDate,
    /// Blocks planned for the remainder (0 if there was no room before the
    /// deadline or the end of the horizon — the task then shows as
    /// "unscheduled").
    pub placed_blocks: usize,
}

/// Matches a stopwatch or timer that has just started at `now` to a
/// planned block, and marks that block in progress. `subject_id` is the
/// subject the session was started with, if any. Does nothing (returns
/// `None`) while time blocking is off, or when no block is close enough.
pub fn link_session_start(
    conn: &Connection,
    now: DateTime<Local>,
    subject_id: Option<i64>,
) -> Result<Option<LinkedBlock>> {
    link_session_start_for(conn, now, subject_id, None)
}

/// [`link_session_start`] for a session that may name a **task** (the
/// `task` picker on the stopwatch and timer): it then only matches that
/// task's blocks — the task is the more specific claim, so the subject
/// isn't also required to match. With no task it is the original rule.
pub fn link_session_start_for(
    conn: &Connection,
    now: DateTime<Local>,
    subject_id: Option<i64>,
    task_id: Option<i64>,
) -> Result<Option<LinkedBlock>> {
    if !settings::time_blocking_enabled(conn)? {
        return Ok(None);
    }
    let tolerance = Duration::minutes(settings::block_start_tolerance_minutes(conn)?);
    let today = now.date_naive();
    let blocks = task_blocks::list_blocks_between(conn, today - Duration::days(1), today + Duration::days(1))?;
    let all_tasks = tasks::list_tasks(conn)?;

    let best = blocks
        .iter()
        .filter(|b| b.status == BlockStatus::Planned)
        .filter_map(|b| {
            let task = all_tasks.iter().find(|t| t.id == b.task_id)?;
            if task.completed {
                return None;
            }
            let distance = (b.start_time - now).abs();
            if distance > tolerance {
                return None;
            }
            if let Some(wanted) = task_id {
                // A session that names a task belongs to that task's blocks.
                if task.id != wanted {
                    return None;
                }
            } else if subject_id.is_some() && task.subject_id != subject_id {
                // A session that names a subject belongs to blocks of that subject.
                return None;
            }
            Some((distance, b.id, b, task))
        })
        .min_by_key(|(distance, id, ..)| (*distance, *id));

    let Some((_, _, block, task)) = best else { return Ok(None) };
    task_blocks::set_block_status(conn, block.id, BlockStatus::InProgress)?;
    Ok(Some(LinkedBlock {
        block_id: block.id,
        task_id: task.id,
        task_title: task.title.clone(),
        subject_id: task.subject_id,
    }))
}

/// A linked session was cancelled: the block goes back to being a plan.
pub fn unlink_block(conn: &Connection, block_id: i64) -> Result<()> {
    if let Some(block) = task_blocks::find_block(conn, block_id)? {
        if block.status == BlockStatus::InProgress {
            task_blocks::set_block_status(conn, block_id, BlockStatus::Planned)?;
        }
    }
    Ok(())
}

/// A linked session ended. The block becomes the span actually worked
/// (`actual_start` for `worked_minutes`, at least one minute) and *done*;
/// then, if the task is still open, its remaining work is carried forward.
/// Returns the carry-over if one happened.
pub fn finish_linked_block(
    conn: &Connection,
    block_id: i64,
    actual_start: DateTime<Local>,
    worked_minutes: i64,
    now: DateTime<Local>,
) -> Result<Option<CarryOutcome>> {
    let Some(block) = task_blocks::find_block(conn, block_id)? else { return Ok(None) };
    let minutes = worked_minutes.max(1);
    task_blocks::settle_block(
        conn,
        block_id,
        actual_start,
        actual_start + Duration::minutes(minutes),
        BlockStatus::Done,
    )?;
    carry_over(conn, block.task_id, minutes, now)
}

/// The task was just ticked off at `now`. Its blocks that haven't been
/// worked are brought into line with the time actually spent (rule 3).
pub fn on_task_completed(conn: &Connection, task_id: i64, now: DateTime<Local>) -> Result<()> {
    for block in task_blocks::list_blocks_for_task(conn, task_id)? {
        // A block being worked is settled by its own session when it ends;
        // one already done is history.
        if block.status != BlockStatus::Planned {
            continue;
        }
        if block.end_time <= now {
            // Already over: keep it as the record of that slot.
            task_blocks::set_block_status(conn, block.id, BlockStatus::Done)?;
        } else if block.start_time <= now {
            // Under way: it lasted until the task was finished.
            if (now - block.start_time).num_seconds() >= 60 {
                task_blocks::settle_block(conn, block.id, block.start_time, now, BlockStatus::Done)?;
            } else {
                task_blocks::delete_block(conn, block.id)?;
            }
        } else {
            // Still ahead: no time was spent on it.
            task_blocks::delete_block(conn, block.id)?;
        }
    }
    Ok(())
}

/// Carries a task's unfinished work to at least tomorrow (rule 4).
///
/// `spent_minutes` is what was just worked on it (0 for a block that
/// passed unworked). The estimate drops by that — if it is used up and the
/// task is still open, one more minimum-size session is planned rather than
/// nothing. The task moves to tomorrow unless it already belongs to a later
/// day (an Auto-assign task stays the planner's; one assigned by hand stays
/// manual), and one follow-up block is planned from tomorrow on.
///
/// Returns `None` when there is nothing to carry (the task is gone or done).
pub fn carry_over(
    conn: &Connection,
    task_id: i64,
    spent_minutes: i64,
    now: DateTime<Local>,
) -> Result<Option<CarryOutcome>> {
    let all_tasks = tasks::list_tasks(conn)?;
    let Some(task) = all_tasks.iter().find(|t| t.id == task_id) else { return Ok(None) };
    if task.completed {
        return Ok(None);
    }

    // Feature request: task "bars" — the estimate is about to fall by the time
    // spent; the bar's budget is fixed first so that doesn't move it.
    if spent_minutes > 0 {
        super::task_time::ensure_budget(conn, task_id)?;
    }
    let today = now.date_naive();
    let tomorrow = today + Duration::days(1);
    let default_minutes = settings::auto_assign_default_task_minutes(conn)?;
    let current = super::auto_assign::task_minutes(task, default_minutes);
    let remaining = match current - spent_minutes {
        left if left > 0 => left,
        // The estimate is used up but the work isn't: one more minimal session.
        _ => block_config(conn)?.effective_min_block(),
    };
    conn.execute("UPDATE tasks SET estimated_minutes = ?1 WHERE id = ?2", params![remaining, task_id])?;

    let assigned = super::auto_assign::parse_day(task.assigned_date.as_deref());
    let new_day = match assigned {
        Some(day) if day >= tomorrow => day,
        _ => tomorrow,
    };
    let new_day_text = new_day.format("%Y-%m-%d").to_string();
    if task.auto_assigned {
        tasks::set_auto_assignment(conn, task_id, Some(&new_day_text))?;
    } else {
        tasks::set_assigned_date(conn, task_id, Some(&new_day_text))?;
    }

    let placed_blocks = plan_task_blocks(conn, today, now.naive_local(), task_id, tomorrow)?;
    Ok(Some(CarryOutcome { task_id, remaining_minutes: remaining, new_day, placed_blocks }))
}

/// Puts every block that is marked in progress back to planned. For use
/// when nothing is running — at launch, after a crash or a quit that left
/// a stopwatch or timer mid-session — because an in-progress block with no
/// session behind it would otherwise be stuck forever, never carried over
/// and never re-planned. Returns how many were released.
pub fn release_in_progress(conn: &Connection) -> Result<usize> {
    conn.execute(
        "UPDATE task_blocks SET status = ?1 WHERE status = ?2",
        params![BlockStatus::Planned.to_db(), BlockStatus::InProgress.to_db()],
    )
}

/// Blocks that ended without the task being finished and without a session
/// working on them: the work didn't happen, so it moves on. A task is only
/// carried once its **last** planned block has ended — an earlier missed
/// piece of a split task doesn't move it while later pieces are still to
/// come. The missed blocks are removed (nothing was worked on them), the
/// estimate is left as it was, and the carry-over plans what is left.
///
/// Call it periodically; it does nothing while time blocking is off and is
/// safe to run repeatedly (a carried task has no ended planned block left).
pub fn carry_over_missed(conn: &Connection, now: DateTime<Local>) -> Result<Vec<CarryOutcome>> {
    if !settings::time_blocking_enabled(conn)? {
        return Ok(Vec::new());
    }
    let today = now.date_naive();
    let blocks = task_blocks::list_blocks_between(conn, today - Duration::days(30), today + Duration::days(3650))?;
    let all_tasks = tasks::list_tasks(conn)?;

    let mut missed_tasks: Vec<i64> = blocks
        .iter()
        .filter(|b| b.status == BlockStatus::Planned && b.end_time <= now)
        .map(|b| b.task_id)
        .collect();
    missed_tasks.sort_unstable();
    missed_tasks.dedup();

    let mut outcomes = Vec::new();
    for task_id in missed_tasks {
        let Some(task) = all_tasks.iter().find(|t| t.id == task_id) else { continue };
        if task.completed {
            continue;
        }
        let still_to_come = blocks
            .iter()
            .any(|b| b.task_id == task_id && b.status != BlockStatus::Done && b.end_time > now);
        if still_to_come {
            continue;
        }
        // Nothing was worked on the missed blocks: remove them, then carry.
        for block in blocks.iter().filter(|b| b.task_id == task_id && b.status == BlockStatus::Planned) {
            task_blocks::delete_block(conn, block.id)?;
        }
        if let Some(outcome) = carry_over(conn, task_id, 0, now)? {
            outcomes.push(outcome);
        }
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use chrono::{NaiveDateTime, TimeZone};

    /// The tests' "today": Wednesday 14 October 2026.
    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 14).unwrap()
    }

    fn at(offset: i64, h: u32, m: u32) -> DateTime<Local> {
        let naive: NaiveDateTime = (today() + Duration::days(offset)).and_hms_opt(h, m, 0).unwrap();
        Local.from_local_datetime(&naive).earliest().unwrap()
    }

    fn db() -> Database {
        let db = Database::open_in_memory().expect("open in-memory db");
        settings::set_time_blocking_enabled(&db.conn, true).expect("enable");
        settings::set_split_over_minutes(&db.conn, 100_000).expect("no threshold splitting");
        db
    }

    /// A task assigned `offset` days from today with an estimate, planner-owned.
    fn task(db: &Database, title: &str, offset: i64, minutes: i64, subject: Option<i64>) -> i64 {
        let id = tasks::create_task(&db.conn, title, None, None, subject).expect("create").id;
        db.conn.execute("UPDATE tasks SET estimated_minutes = ?1 WHERE id = ?2", params![minutes, id]).unwrap();
        let day = (today() + Duration::days(offset)).format("%Y-%m-%d").to_string();
        tasks::set_auto_assignment(&db.conn, id, Some(&day)).expect("assign");
        id
    }

    /// A planned (unpinned) block for a task.
    fn block(db: &Database, task_id: i64, start: DateTime<Local>, end: DateTime<Local>) -> i64 {
        let day = start.date_naive();
        let naive = |dt: DateTime<Local>| dt.naive_local();
        task_blocks::insert_auto_blocks(
            &db.conn,
            &[crate::tasks::time_blocking::NewBlock {
                task_id,
                start: naive(start),
                end: naive(end),
                chunk_index: 0,
            }],
        )
        .unwrap();
        task_blocks::list_blocks_between(&db.conn, day, day)
            .unwrap()
            .into_iter()
            .filter(|b| b.task_id == task_id && b.start_time == start)
            .map(|b| b.id)
            .next()
            .expect("block inserted")
    }

    fn get(db: &Database, id: i64) -> crate::tasks::TimeBlock {
        task_blocks::find_block(&db.conn, id).unwrap().expect("block exists")
    }

    fn task_row(db: &Database, id: i64) -> crate::tasks::Task {
        tasks::list_tasks(&db.conn).unwrap().into_iter().find(|t| t.id == id).expect("task")
    }

    fn subject(db: &Database, name: &str) -> i64 {
        crate::database::queries::subjects::create_subject(&db.conn, name).expect("subject").id
    }

    // --- 1. linking ---------------------------------------------------------------------

    #[test]
    fn a_session_started_near_a_block_starts_it() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));

        let linked = link_session_start(&db.conn, at(0, 14, 5), None).unwrap().expect("linked");

        assert_eq!((linked.block_id, linked.task_id, linked.task_title.as_str()), (b, t, "Essay"));
        assert_eq!(get(&db, b).status, BlockStatus::InProgress);
    }

    #[test]
    fn early_and_late_starts_within_the_tolerance_both_count_and_outside_it_does_not() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));

        assert!(link_session_start(&db.conn, at(0, 14, 16), None).unwrap().is_none(), "16 min late");
        assert!(link_session_start(&db.conn, at(0, 13, 44), None).unwrap().is_none(), "16 min early");
        assert_eq!(get(&db, b).status, BlockStatus::Planned, "an unmatched start changes nothing");

        assert!(link_session_start(&db.conn, at(0, 13, 45), None).unwrap().is_some(), "exactly 15 early");
    }

    #[test]
    fn the_tolerance_setting_is_respected() {
        let db = db();
        settings::set_block_start_tolerance_minutes(&db.conn, 5).unwrap();
        let t = task(&db, "Essay", 0, 60, None);
        block(&db, t, at(0, 14, 0), at(0, 15, 0));

        assert!(link_session_start(&db.conn, at(0, 14, 10), None).unwrap().is_none());
        assert!(link_session_start(&db.conn, at(0, 14, 4), None).unwrap().is_some());
    }

    #[test]
    fn nothing_links_while_time_blocking_is_off() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));
        settings::set_time_blocking_enabled(&db.conn, false).unwrap();

        assert!(link_session_start(&db.conn, at(0, 14, 0), None).unwrap().is_none());
        assert_eq!(get(&db, b).status, BlockStatus::Planned);
    }

    #[test]
    fn the_nearest_block_wins() {
        let db = db();
        let a = task(&db, "A", 0, 30, None);
        let b = task(&db, "B", 0, 30, None);
        block(&db, a, at(0, 14, 0), at(0, 14, 30));
        let nearer = block(&db, b, at(0, 14, 10), at(0, 14, 40));

        let linked = link_session_start(&db.conn, at(0, 14, 9), None).unwrap().expect("linked");

        assert_eq!(linked.block_id, nearer);
    }

    #[test]
    fn a_session_with_a_subject_only_matches_blocks_of_that_subject() {
        let db = db();
        let maths = subject(&db, "Maths");
        let history = subject(&db, "History");
        let maths_task = task(&db, "Calculus", 0, 30, Some(maths));
        let history_task = task(&db, "Essay", 0, 30, Some(history));
        block(&db, maths_task, at(0, 14, 0), at(0, 14, 30));
        let wanted = block(&db, history_task, at(0, 14, 10), at(0, 14, 40));

        // The maths block is nearer, but the session is a history one.
        let linked = link_session_start(&db.conn, at(0, 14, 1), Some(history)).unwrap().expect("linked");

        assert_eq!(linked.block_id, wanted);
        assert_eq!(linked.subject_id, Some(history));
    }

    #[test]
    fn a_session_with_a_subject_does_not_link_to_a_block_of_another_subject() {
        let db = db();
        let maths = subject(&db, "Maths");
        let history = subject(&db, "History");
        let t = task(&db, "Calculus", 0, 30, Some(maths));
        block(&db, t, at(0, 14, 0), at(0, 14, 30));

        assert!(link_session_start(&db.conn, at(0, 14, 0), Some(history)).unwrap().is_none());
    }

    #[test]
    fn a_session_with_no_subject_matches_any_block_and_reports_the_tasks_subject() {
        let db = db();
        let maths = subject(&db, "Maths");
        let t = task(&db, "Calculus", 0, 30, Some(maths));
        block(&db, t, at(0, 14, 0), at(0, 14, 30));

        let linked = link_session_start(&db.conn, at(0, 14, 0), None).unwrap().expect("linked");

        assert_eq!(linked.subject_id, Some(maths), "so the session can count toward Maths in the stats");
    }

    #[test]
    fn started_finished_and_completed_work_is_never_linked() {
        let db = db();
        let running = task(&db, "Running", 0, 30, None);
        let done = task(&db, "Done", 0, 30, None);
        let completed = task(&db, "Completed", 0, 30, None);
        let r = block(&db, running, at(0, 14, 0), at(0, 14, 30));
        let d = block(&db, done, at(0, 14, 0), at(0, 14, 30));
        let c = block(&db, completed, at(0, 14, 0), at(0, 14, 30));
        task_blocks::set_block_status(&db.conn, r, BlockStatus::InProgress).unwrap();
        task_blocks::set_block_status(&db.conn, d, BlockStatus::Done).unwrap();
        db.conn.execute("UPDATE tasks SET completed = 1 WHERE id = ?1", params![completed]).unwrap();
        let _ = c;

        assert!(link_session_start(&db.conn, at(0, 14, 0), None).unwrap().is_none());
    }

    #[test]
    fn a_block_just_after_midnight_links_from_the_evening_before() {
        let db = db();
        let t = task(&db, "Night owl", 1, 30, None);
        block(&db, t, at(1, 0, 5), at(1, 0, 35));

        assert!(link_session_start(&db.conn, at(0, 23, 55), None).unwrap().is_some());
    }

    #[test]
    fn cancelling_a_linked_session_returns_the_block_to_a_plan() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));
        let linked = link_session_start(&db.conn, at(0, 14, 0), None).unwrap().expect("linked");

        unlink_block(&db.conn, linked.block_id).unwrap();

        assert_eq!(get(&db, b).status, BlockStatus::Planned);
        assert_eq!(get(&db, b).start_time, at(0, 14, 0), "untouched");
    }

    #[test]
    fn unlinking_never_reopens_a_block_that_was_finished() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));
        task_blocks::set_block_status(&db.conn, b, BlockStatus::Done).unwrap();

        unlink_block(&db.conn, b).unwrap();

        assert_eq!(get(&db, b).status, BlockStatus::Done);
    }

    // --- 2. finishing: overflow ------------------------------------------------------------

    #[test]
    fn a_stopwatch_that_ran_over_extends_its_block_to_the_time_really_worked() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));
        link_session_start(&db.conn, at(0, 14, 5), None).unwrap();

        // Started 14:05, ran 95 minutes (to 15:40): 40 past the planned end.
        finish_linked_block(&db.conn, b, at(0, 14, 5), 95, at(0, 15, 40)).unwrap();

        let done = get(&db, b);
        assert_eq!((done.start_time, done.end_time), (at(0, 14, 5), at(0, 15, 40)));
        assert_eq!(done.status, BlockStatus::Done);
    }

    #[test]
    fn stopping_early_shortens_the_block_to_the_time_worked() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));
        link_session_start(&db.conn, at(0, 14, 0), None).unwrap();

        finish_linked_block(&db.conn, b, at(0, 14, 0), 25, at(0, 14, 25)).unwrap();

        assert_eq!((get(&db, b).start_time, get(&db, b).end_time), (at(0, 14, 0), at(0, 14, 25)));
    }

    #[test]
    fn a_session_shorter_than_a_minute_still_leaves_a_one_minute_block() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));

        finish_linked_block(&db.conn, b, at(0, 14, 0), 0, at(0, 14, 0)).unwrap();

        assert_eq!(get(&db, b).end_time, at(0, 14, 1));
    }

    #[test]
    fn finishing_does_not_pin_the_block_or_take_the_task_off_the_planner() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));

        finish_linked_block(&db.conn, b, at(0, 14, 0), 30, at(0, 14, 30)).unwrap();

        assert!(!get(&db, b).locked);
        assert!(task_row(&db, t).auto_assigned);
    }

    // --- 3. completing early ---------------------------------------------------------------

    #[test]
    fn completing_a_task_during_its_block_ends_the_block_at_that_moment() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));

        on_task_completed(&db.conn, t, at(0, 14, 40)).unwrap();

        let after = get(&db, b);
        assert_eq!((after.start_time, after.end_time), (at(0, 14, 0), at(0, 14, 40)));
        assert_eq!(after.status, BlockStatus::Done);
    }

    #[test]
    fn completing_a_task_before_its_block_starts_removes_the_block() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));

        on_task_completed(&db.conn, t, at(0, 9, 0)).unwrap();

        assert!(task_blocks::find_block(&db.conn, b).unwrap().is_none(), "no time was spent on it");
    }

    #[test]
    fn completing_a_task_whose_block_is_over_keeps_the_block_as_done() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 9, 0), at(0, 10, 0));

        on_task_completed(&db.conn, t, at(0, 11, 0)).unwrap();

        let after = get(&db, b);
        assert_eq!((after.start_time, after.end_time), (at(0, 9, 0), at(0, 10, 0)));
        assert_eq!(after.status, BlockStatus::Done);
    }

    #[test]
    fn completing_within_a_minute_of_the_start_removes_the_block() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));

        on_task_completed(&db.conn, t, at(0, 14, 0) + Duration::seconds(20)).unwrap();

        assert!(task_blocks::find_block(&db.conn, b).unwrap().is_none());
    }

    #[test]
    fn completing_leaves_a_running_blocks_session_to_settle_it() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));
        task_blocks::set_block_status(&db.conn, b, BlockStatus::InProgress).unwrap();

        on_task_completed(&db.conn, t, at(0, 14, 40)).unwrap();

        let after = get(&db, b);
        assert_eq!(after.status, BlockStatus::InProgress);
        assert_eq!(after.end_time, at(0, 15, 0), "untouched until the stopwatch finishes");
    }

    #[test]
    fn completing_handles_every_piece_of_a_split_task() {
        let db = db();
        let t = task(&db, "Maths assignment", 0, 120, None);
        let over = block(&db, t, at(0, 9, 0), at(0, 10, 0));
        let now_piece = block(&db, t, at(0, 14, 0), at(0, 15, 0));
        let ahead = block(&db, t, at(1, 9, 0), at(1, 10, 0));

        on_task_completed(&db.conn, t, at(0, 14, 30)).unwrap();

        assert_eq!(get(&db, over).status, BlockStatus::Done);
        assert_eq!(get(&db, now_piece).end_time, at(0, 14, 30));
        assert!(task_blocks::find_block(&db.conn, ahead).unwrap().is_none());
    }

    #[test]
    fn ticking_the_task_off_in_the_app_trims_its_block_through_set_completed() {
        // The real path: the checkbox calls `tasks::set_completed`, which is wired to the rule.
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let started = Local::now() - Duration::minutes(30);
        task_blocks::insert_auto_blocks(
            &db.conn,
            &[crate::tasks::time_blocking::NewBlock {
                task_id: t,
                start: started.naive_local(),
                end: (started + Duration::minutes(60)).naive_local(),
                chunk_index: 0,
            }],
        )
        .unwrap();

        tasks::set_completed(&db.conn, t, true).unwrap();

        let blocks = task_blocks::list_blocks_for_task(&db.conn, t).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].status, BlockStatus::Done);
        assert!(blocks[0].end_time <= Local::now(), "cut off at the moment it was ticked");
        assert!(blocks[0].duration() >= Duration::minutes(29));
    }

    // --- 4. carrying unfinished work forward -----------------------------------------------

    #[test]
    fn an_unfinished_task_gets_a_new_block_at_least_a_day_ahead_for_what_is_left() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));
        link_session_start(&db.conn, at(0, 14, 0), None).unwrap();

        let outcome = finish_linked_block(&db.conn, b, at(0, 14, 0), 25, at(0, 14, 25)).unwrap().expect("carried");

        assert_eq!(outcome.remaining_minutes, 35, "60 - 25");
        assert_eq!(outcome.new_day, today() + Duration::days(1));
        assert_eq!(outcome.placed_blocks, 1);
        let blocks = task_blocks::list_blocks_for_task(&db.conn, t).unwrap();
        let planned: Vec<_> = blocks.iter().filter(|x| x.status == BlockStatus::Planned).collect();
        assert_eq!(planned.len(), 1);
        assert!(planned[0].day() >= today() + Duration::days(1), "never the same day");
        assert_eq!(planned[0].duration(), Duration::minutes(45), "35 rounds up to the 15-minute grid");
        assert!(blocks.iter().any(|x| x.id == b && x.status == BlockStatus::Done), "the worked block is kept");
        assert_eq!(task_row(&db, t).estimated_minutes, Some(35));
    }

    #[test]
    fn a_task_that_used_up_its_estimate_but_is_not_done_gets_one_more_minimum_session() {
        let db = db();
        let t = task(&db, "Essay", 0, 30, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 14, 30));

        let outcome = finish_linked_block(&db.conn, b, at(0, 14, 0), 45, at(0, 14, 45)).unwrap().expect("carried");

        assert_eq!(outcome.remaining_minutes, 30, "the minimum block, not nothing");
        assert_eq!(outcome.placed_blocks, 1);
    }

    #[test]
    fn a_task_that_is_already_complete_is_not_carried() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));
        db.conn.execute("UPDATE tasks SET completed = 1 WHERE id = ?1", params![t]).unwrap();

        let outcome = finish_linked_block(&db.conn, b, at(0, 14, 0), 40, at(0, 14, 40)).unwrap();

        assert_eq!(outcome, None);
        assert_eq!(get(&db, b).status, BlockStatus::Done);
        assert_eq!(task_blocks::list_blocks_for_task(&db.conn, t).unwrap().len(), 1, "no follow-up block");
    }

    #[test]
    fn carrying_keeps_a_planner_task_the_planners_and_a_manual_task_manual() {
        let db = db();
        let auto = task(&db, "Auto", 0, 60, None);
        let manual = task(&db, "Manual", 0, 60, None);
        tasks::set_assigned_date(&db.conn, manual, Some("2026-10-14")).unwrap(); // by hand
        assert!(task_row(&db, auto).auto_assigned);
        assert!(!task_row(&db, manual).auto_assigned);

        carry_over(&db.conn, auto, 20, at(0, 14, 20)).unwrap();
        carry_over(&db.conn, manual, 20, at(0, 14, 20)).unwrap();

        assert!(task_row(&db, auto).auto_assigned);
        assert!(!task_row(&db, manual).auto_assigned);
        for id in [auto, manual] {
            assert_eq!(task_row(&db, id).assigned_date.as_deref(), Some("2026-10-15"), "moved to tomorrow");
        }
    }

    #[test]
    fn a_task_already_on_a_later_day_stays_there() {
        let db = db();
        let t = task(&db, "Essay", 3, 60, None);

        let outcome = carry_over(&db.conn, t, 20, at(0, 14, 20)).unwrap().expect("carried");

        assert_eq!(outcome.new_day, today() + Duration::days(3));
        assert_eq!(task_row(&db, t).assigned_date.as_deref(), Some("2026-10-17"));
    }

    #[test]
    fn carrying_one_task_does_not_disturb_any_other_block() {
        let db = db();
        let other = task(&db, "Other", 1, 60, None);
        let other_block = block(&db, other, at(1, 9, 0), at(1, 10, 0));
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));

        carry_over(&db.conn, t, 25, at(0, 14, 25)).unwrap();
        let _ = b;

        let unchanged = get(&db, other_block);
        assert_eq!((unchanged.start_time, unchanged.end_time), (at(1, 9, 0), at(1, 10, 0)));
        // And the follow-up block went around it, not on top of it.
        for follow_up in task_blocks::list_blocks_for_task(&db.conn, t).unwrap().iter().filter(|x| x.status == BlockStatus::Planned) {
            assert!(!follow_up.overlaps(&unchanged.interval()), "no double booking");
        }
    }

    #[test]
    fn a_follow_up_is_planned_around_events_on_the_new_day() {
        let db = db();
        crate::database::queries::events::create_event(&db.conn, "Lecture", "", at(1, 9, 0), at(1, 11, 0), None, None)
            .unwrap();
        let t = task(&db, "Essay", 0, 60, None);

        carry_over(&db.conn, t, 30, at(0, 14, 30)).unwrap();

        let planned = task_blocks::list_blocks_for_task(&db.conn, t).unwrap();
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].start_time, at(1, 11, 0), "after tomorrow's lecture");
    }

    #[test]
    fn the_follow_up_is_not_planned_when_nothing_fits_before_the_deadline() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        db.conn
            .execute("UPDATE tasks SET due_date = ?1 WHERE id = ?2", params!["2026-10-14", t])
            .unwrap(); // due today, so there is no day "at least a day ahead" that is in time

        let outcome = carry_over(&db.conn, t, 20, at(0, 14, 20)).unwrap().expect("carried");

        assert_eq!(outcome.placed_blocks, 0);
        assert!(task_blocks::list_blocks_for_task(&db.conn, t).unwrap().is_empty());
        assert_eq!(task_row(&db, t).estimated_minutes, Some(40), "the remaining work is still recorded");
    }

    // --- 4b. a block that simply passes ------------------------------------------------------

    #[test]
    fn a_block_that_passes_unworked_is_replaced_by_one_at_least_a_day_ahead() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let missed = block(&db, t, at(0, 9, 0), at(0, 10, 0));

        let outcomes = carry_over_missed(&db.conn, at(0, 10, 30)).unwrap();

        assert_eq!(outcomes.len(), 1);
        assert!(task_blocks::find_block(&db.conn, missed).unwrap().is_none(), "the missed plan is gone");
        let blocks = task_blocks::list_blocks_for_task(&db.conn, t).unwrap();
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].day() >= today() + Duration::days(1));
        assert_eq!(blocks[0].duration(), Duration::minutes(60), "nothing was worked, so the whole estimate");
        assert_eq!(task_row(&db, t).estimated_minutes, Some(60));
    }

    #[test]
    fn a_block_that_has_not_ended_yet_is_left_alone() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));

        assert!(carry_over_missed(&db.conn, at(0, 14, 30)).unwrap().is_empty());
        assert_eq!(get(&db, b).status, BlockStatus::Planned);
    }

    #[test]
    fn a_finished_task_is_not_carried() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        block(&db, t, at(0, 9, 0), at(0, 10, 0));
        db.conn.execute("UPDATE tasks SET completed = 1 WHERE id = ?1", params![t]).unwrap();

        assert!(carry_over_missed(&db.conn, at(0, 10, 30)).unwrap().is_empty());
    }

    #[test]
    fn a_split_task_is_only_carried_once_its_last_piece_has_passed() {
        let db = db();
        let t = task(&db, "Maths assignment", 0, 120, None);
        let early = block(&db, t, at(0, 9, 0), at(0, 10, 0));
        let later = block(&db, t, at(0, 16, 0), at(0, 17, 0));

        assert!(carry_over_missed(&db.conn, at(0, 10, 30)).unwrap().is_empty(), "a piece is still to come");
        assert!(task_blocks::find_block(&db.conn, early).unwrap().is_some());
        assert!(task_blocks::find_block(&db.conn, later).unwrap().is_some());

        assert_eq!(carry_over_missed(&db.conn, at(0, 17, 30)).unwrap().len(), 1, "now both have passed");
    }

    #[test]
    fn a_block_being_worked_is_never_treated_as_missed() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));
        task_blocks::set_block_status(&db.conn, b, BlockStatus::InProgress).unwrap();

        // The planned end has passed, but the stopwatch is still running.
        assert!(carry_over_missed(&db.conn, at(0, 15, 30)).unwrap().is_empty());
        assert_eq!(get(&db, b).status, BlockStatus::InProgress);
    }

    #[test]
    fn carrying_missed_blocks_twice_carries_once() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        block(&db, t, at(0, 9, 0), at(0, 10, 0));

        assert_eq!(carry_over_missed(&db.conn, at(0, 10, 30)).unwrap().len(), 1);
        assert!(carry_over_missed(&db.conn, at(0, 10, 31)).unwrap().is_empty());
        assert_eq!(task_blocks::list_blocks_for_task(&db.conn, t).unwrap().len(), 1);
    }

    #[test]
    fn missed_blocks_are_ignored_while_time_blocking_is_off() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 9, 0), at(0, 10, 0));
        settings::set_time_blocking_enabled(&db.conn, false).unwrap();

        assert!(carry_over_missed(&db.conn, at(0, 10, 30)).unwrap().is_empty());
        assert!(task_blocks::find_block(&db.conn, b).unwrap().is_some());
    }

    #[test]
    fn a_missed_block_on_an_earlier_day_is_carried_before_the_daily_sweep_would_drop_it() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        block(&db, t, at(-1, 9, 0), at(-1, 10, 0));

        let outcomes = carry_over_missed(&db.conn, at(0, 8, 0)).unwrap();

        assert_eq!(outcomes.len(), 1, "yesterday's missed block is not silently lost");
        assert!(task_blocks::list_blocks_for_task(&db.conn, t).unwrap()[0].day() >= today() + Duration::days(1));
    }

    #[test]
    fn releasing_orphaned_in_progress_blocks_makes_them_plans_again_and_leaves_done_ones() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let running = block(&db, t, at(0, 9, 0), at(0, 10, 0));
        let done = block(&db, t, at(0, 11, 0), at(0, 12, 0));
        task_blocks::set_block_status(&db.conn, running, BlockStatus::InProgress).unwrap();
        task_blocks::set_block_status(&db.conn, done, BlockStatus::Done).unwrap();

        assert_eq!(release_in_progress(&db.conn).unwrap(), 1);

        assert_eq!(get(&db, running).status, BlockStatus::Planned);
        assert_eq!(get(&db, done).status, BlockStatus::Done);
        assert_eq!(release_in_progress(&db.conn).unwrap(), 0, "nothing left to release");
    }

    // --- 5. time in blocks is added to the stats ------------------------------------------------

    use crate::database::queries::pomodoro::{record_focus_run, FocusSource};

    fn subject_minutes(db: &Database, subject_id: i64, from: NaiveDate, to: NaiveDate) -> i64 {
        crate::stats::minutes_per_subject_in_range(db, from, to).get(&subject_id).copied().unwrap_or(0)
    }

    #[test]
    fn a_timer_run_linked_to_a_block_adds_its_time_to_the_blocks_subject() {
        // The whole path, with a real countdown: start near a block, run out, settle.
        let db = db();
        let maths = subject(&db, "Maths");
        let t = task(&db, "Calculus", 0, 60, Some(maths));
        let now = Local::now();
        let planned_start = now + Duration::minutes(2);
        let b = block(&db, t, planned_start, planned_start + Duration::minutes(60));

        let mut timer = crate::timer::TimerState::default();
        timer.minutes = 25;
        timer.start(); // no subject picked
        let link = link_session_start(&db.conn, timer.started_at.expect("started"), timer.subject_id)
            .unwrap()
            .expect("linked: the block starts 2 minutes later");
        timer.linked_block = Some(link.block_id);
        timer.link_subject = link.subject_id;
        assert!(matches!(timer.advance(std::time::Duration::from_secs(25 * 60)), crate::timer::TimerTick::Completed { .. }));

        // What `AppState::tick_timer` does on completion.
        let finished = timer.take_finished_link().expect("it was linked");
        let subject_id = crate::timer::attributed_subject(timer.subject_id, Some(finished));
        record_focus_run(&db.conn, FocusSource::Timer, "Timer focus session", subject_id, 25, 25, now + Duration::minutes(25), true)
            .unwrap();
        finish_linked_block(&db.conn, finished.block_id, finished.started_at, 25, now + Duration::minutes(25)).unwrap();

        let (from, to) = (now.date_naive() - Duration::days(1), now.date_naive() + Duration::days(1));
        assert_eq!(subject_minutes(&db, maths, from, to), 25, "counted once, under the task's subject");
        assert!(
            crate::database::queries::events::list_visible_events(&db.conn).unwrap().is_empty(),
            "and the event that carries it is kept off the calendar"
        );
        let after = get(&db, b);
        assert_eq!(after.status, BlockStatus::Done);
        assert_eq!(after.duration(), Duration::minutes(25));
    }

    #[test]
    fn a_stopwatch_that_overran_adds_the_full_time_worked_and_extends_the_block() {
        let db = db();
        let maths = subject(&db, "Maths");
        let t = task(&db, "Calculus", 0, 60, Some(maths));
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));
        let linked = link_session_start(&db.conn, at(0, 14, 0), None).unwrap().expect("linked");

        // 95 minutes: 35 over the planned hour. The stopwatch took the task's subject at start.
        record_focus_run(&db.conn, FocusSource::Stopwatch, "Stopwatch session", linked.subject_id, 95, 95, at(0, 15, 35), true)
            .unwrap();
        finish_linked_block(&db.conn, b, at(0, 14, 0), 95, at(0, 15, 35)).unwrap();

        assert_eq!(subject_minutes(&db, maths, today(), today()), 95);
        assert_eq!(get(&db, b).end_time, at(0, 15, 35));
    }

    #[test]
    fn time_in_a_block_is_not_counted_twice_by_the_block_itself() {
        // The settled block carries the real span, but the stats come from the one-off
        // event alone. Planning, settling and re-planning must never add subject time.
        let db = db();
        let maths = subject(&db, "Maths");
        let t = task(&db, "Calculus", 0, 60, Some(maths));
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));
        let linked = link_session_start(&db.conn, at(0, 14, 0), None).unwrap().expect("linked");
        record_focus_run(&db.conn, FocusSource::Timer, "Timer focus session", linked.subject_id, 25, 25, at(0, 14, 25), true)
            .unwrap();
        finish_linked_block(&db.conn, b, at(0, 14, 0), 25, at(0, 14, 25)).unwrap();
        super::super::time_blocking::plan_blocks(&db.conn, today(), at(0, 14, 30).naive_local()).unwrap();

        assert_eq!(subject_minutes(&db, maths, today(), today() + Duration::days(30)), 25);
    }

    #[test]
    fn planned_blocks_that_were_never_worked_add_nothing_to_the_stats() {
        let db = db();
        let maths = subject(&db, "Maths");
        let t = task(&db, "Calculus", 1, 60, Some(maths));
        block(&db, t, at(1, 9, 0), at(1, 10, 0));
        super::super::time_blocking::plan_blocks(&db.conn, today(), at(0, 8, 0).naive_local()).unwrap();

        assert_eq!(subject_minutes(&db, maths, today(), today() + Duration::days(30)), 0);
    }

    #[test]
    fn a_linked_run_for_a_task_with_no_subject_adds_no_subject_time() {
        let db = db();
        let t = task(&db, "Reading", 0, 60, None);
        let b = block(&db, t, at(0, 14, 0), at(0, 15, 0));
        let linked = link_session_start(&db.conn, at(0, 14, 0), None).unwrap().expect("linked");
        assert_eq!(linked.subject_id, None);

        record_focus_run(&db.conn, FocusSource::Timer, "Timer focus session", linked.subject_id, 25, 25, at(0, 14, 25), true)
            .unwrap();
        finish_linked_block(&db.conn, b, at(0, 14, 0), 25, at(0, 14, 25)).unwrap();

        assert!(crate::stats::minutes_per_subject_in_range(&db, today(), today()).is_empty());
        assert_eq!(get(&db, b).status, BlockStatus::Done, "the block still settles");
    }

    #[test]
    fn record_focus_run_without_a_subject_logs_focus_time_only() {
        let db = db();
        record_focus_run(&db.conn, FocusSource::Timer, "Timer focus session", None, 25, 25, at(0, 14, 25), false).unwrap();
        assert!(crate::stats::minutes_per_subject_in_range(&db, today(), today()).is_empty());
        let logged: i64 = db
            .conn
            .query_row("SELECT COALESCE(SUM(minutes), 0) FROM pomodoro_sessions WHERE source = 'timer'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(logged, 25);
    }

    #[test]
    fn a_sub_minute_run_logs_no_focus_time_but_still_gets_a_one_minute_event() {
        // The stopwatch's existing behaviour, preserved by the extraction.
        let db = db();
        let maths = subject(&db, "Maths");
        record_focus_run(&db.conn, FocusSource::Stopwatch, "Stopwatch session", Some(maths), 0, 1, at(0, 14, 0), false).unwrap();
        let logged: i64 = db
            .conn
            .query_row("SELECT COALESCE(SUM(minutes), 0) FROM pomodoro_sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(logged, 0);
        assert_eq!(subject_minutes(&db, maths, today(), today()), 1);
    }

    // --- the event behind a linked run is invisible, but the time is registered -----------------

    use crate::database::queries::events;

    #[test]
    fn a_linked_runs_event_is_hidden_from_the_calendar_but_its_time_is_still_registered() {
        let db = db();
        let maths = subject(&db, "Maths");
        record_focus_run(&db.conn, FocusSource::Timer, "Timer focus session", Some(maths), 25, 25, at(0, 14, 25), true)
            .unwrap();

        assert!(events::list_visible_events(&db.conn).unwrap().is_empty(), "nothing on the calendar");
        assert_eq!(events::list_all_events(&db.conn).unwrap().len(), 1, "but the event exists");
        assert_eq!(subject_minutes(&db, maths, today(), today()), 25, "and the minutes are counted");
    }

    #[test]
    fn a_run_not_linked_to_a_block_still_shows_its_event() {
        let db = db();
        let maths = subject(&db, "Maths");
        record_focus_run(&db.conn, FocusSource::Stopwatch, "Stopwatch session", Some(maths), 25, 25, at(0, 14, 25), false)
            .unwrap();

        assert_eq!(events::list_visible_events(&db.conn).unwrap().len(), 1, "behaves exactly as before");
        assert_eq!(subject_minutes(&db, maths, today(), today()), 25);
    }

    #[test]
    fn hiding_only_affects_the_linked_runs_event_not_the_persons_own() {
        let db = db();
        let maths = subject(&db, "Maths");
        events::create_event(&db.conn, "Lecture", "", at(0, 9, 0), at(0, 10, 0), Some(maths), None).unwrap();
        record_focus_run(&db.conn, FocusSource::Timer, "Timer focus session", Some(maths), 25, 25, at(0, 14, 25), true)
            .unwrap();

        let titles: Vec<String> =
            events::list_visible_events(&db.conn).unwrap().into_iter().map(|e| e.title).collect();
        assert_eq!(titles, vec!["Lecture".to_owned()]);
    }

    #[test]
    fn a_hidden_event_still_counts_as_busy_time_for_the_planner() {
        let db = db();
        let maths = subject(&db, "Maths");
        record_focus_run(&db.conn, FocusSource::Timer, "Timer focus session", Some(maths), 60, 60, at(1, 10, 0), true)
            .unwrap(); // a hidden event 09:00-10:00 tomorrow
        let t = task(&db, "Essay", 1, 60, None);

        super::super::time_blocking::plan_blocks(&db.conn, today(), at(0, 8, 0).naive_local()).unwrap();

        let blocks = task_blocks::list_blocks_for_task(&db.conn, t).unwrap();
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].start_time >= at(1, 10, 0), "planned after the hidden event, not on top of it");
    }

    #[test]
    fn the_event_stays_hidden_across_the_session_log_so_it_is_not_double_listed_anywhere() {
        let db = db();
        let maths = subject(&db, "Maths");
        record_focus_run(&db.conn, FocusSource::Timer, "Timer focus session", Some(maths), 25, 25, at(0, 14, 25), true)
            .unwrap();
        let hidden = events::list_all_events(&db.conn).unwrap().remove(0);

        // Its session-log entry exists (that is what the stats read)...
        let logged = crate::database::queries::session_log::get_log(&db.conn, hidden.id, hidden.day()).unwrap();
        assert!(logged.is_some());
        // ...while the event is in no list a person looks at.
        assert!(events::list_visible_events(&db.conn).unwrap().iter().all(|e| e.id != hidden.id));
    }

    // --- task bars: linking by task, and the budget surviving a worked block ---

    #[test]
    fn a_session_that_names_a_task_links_to_that_tasks_block_not_a_nearer_one() {
        let db = db();
        let other = task(&db, "Other", 0, 60, None);
        let mine = task(&db, "Mine", 0, 60, None);
        block(&db, other, at(0, 9, 0), at(0, 10, 0));
        let mine_block = block(&db, mine, at(0, 9, 20), at(0, 10, 20));

        let link = link_session_start_for(&db.conn, at(0, 9, 5), None, Some(mine)).unwrap().expect("a match");

        assert_eq!((link.block_id, link.task_id), (mine_block, mine));
    }

    #[test]
    fn a_session_that_names_a_task_with_no_block_nearby_links_nothing() {
        let db = db();
        let other = task(&db, "Other", 0, 60, None);
        let mine = task(&db, "Mine", 0, 60, None);
        block(&db, other, at(0, 9, 0), at(0, 10, 0));
        assert_eq!(link_session_start_for(&db.conn, at(0, 9, 5), None, Some(mine)).unwrap(), None);
    }

    #[test]
    fn naming_no_task_still_links_as_before() {
        let db = db();
        let t = task(&db, "Any", 0, 60, None);
        let b = block(&db, t, at(0, 9, 0), at(0, 10, 0));
        assert_eq!(link_session_start(&db.conn, at(0, 9, 5), None).unwrap().map(|l| l.block_id), Some(b));
    }

    #[test]
    fn working_a_block_lowers_the_estimate_but_not_the_bars_budget() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 9, 0), at(0, 10, 0));
        // What the stopwatch/timer finish path does, in its order: log, then settle.
        super::super::task_time::log_task_time(
            &db.conn,
            t,
            today(),
            30,
            super::super::pomodoro::FocusSource::Stopwatch,
        )
        .unwrap();
        finish_linked_block(&db.conn, b, at(0, 9, 0), 30, at(0, 9, 30)).unwrap();

        assert_eq!(task_row(&db, t).estimated_minutes, Some(30), "30 minutes still to do");
        let bar = super::super::task_time::progress_for_all(&db.conn).unwrap()[&t];
        assert_eq!((bar.spent, bar.budget), (30, 60), "half the original hour");
    }

    #[test]
    fn the_budget_is_fixed_by_carry_over_even_if_nothing_was_logged() {
        let db = db();
        let t = task(&db, "Essay", 0, 60, None);
        let b = block(&db, t, at(0, 9, 0), at(0, 10, 0));
        finish_linked_block(&db.conn, b, at(0, 9, 0), 20, at(0, 9, 20)).unwrap();
        let budget: Option<i64> = db
            .conn
            .query_row("SELECT time_budget_minutes FROM tasks WHERE id = ?1", params![t], |r| r.get(0))
            .unwrap();
        assert_eq!(budget, Some(60));
    }
}

