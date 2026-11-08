//! Feature request: "Time-blocking" — the database side of
//! `tasks::time_blocking` (docs/planning/time-blocking.md §7). Stage A
//! (`queries::auto_assign::run`) decides which *day* a task is on;
//! [`plan_blocks`] decides *what time of day*, by placing each day's
//! tasks into the free gaps between that day's calendar events.
//!
//! It is a separate command ("Plan times") run after Auto-assign. A
//! task goes whole in the first gap on its day that holds it; if none
//! does it is split across that day's gaps; if that is not enough
//! either, the rest carries to the following days (up to its deadline)
//! — "split as necessary, pieces may span days". With splitting off
//! (`max_chunks = 1`) a task that doesn't fit its day is reported in
//! [`BlockReport::unplaced`] and left alone. A task is placed in full
//! or not at all. (Feeding the misses back into Stage A is Phase 3.)
//!
//! What counts as busy: every calendar event (recurring ones expanded,
//! including the part of a midnight-crossing event that spills into
//! the next day), every **locked** block (the person placed it, so it
//! stays), and every block of a **completed** task (history of the
//! plan, drawn as done — it must neither vanish when the day is
//! re-planned nor be double-booked).
//!
//! What gets placed: an incomplete task assigned to a day inside the
//! horizon (manual and planner assignments alike) that isn't a
//! container (its subtasks carry the work), a template, or linked to a
//! revision session (the session already covers its time — the same
//! rule Auto-assign uses; confirmed as Q7). Time the person has
//! pinned counts toward the task: only the remainder is planned, and
//! a task whose pinned pieces already cover its estimate is left alone.
//!
//! The reads happen first, then one transactional write
//! (`task_blocks::replace_auto_blocks_for_days_keeping`): the day is
//! either re-planned or left as it was.

use std::collections::{HashMap, HashSet};

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use rusqlite::{Connection, Result};

use super::auto_assign::{container_ids, effective_deadline, parse_day, task_minutes};
use super::{events, settings, task_blocks, tasks};
use crate::events::occurrence;
use crate::tasks::time_blocking::{self, BlockConfig, DayWindow, DropPlacement, HorizonTask, Interval};
use crate::tasks::Task;

/// A task that did not fit on its day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unplaced {
    pub task_id: i64,
    pub day: NaiveDate,
}

/// What one [`plan_blocks`] run did, for a one-line summary.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BlockReport {
    /// Blocks written (a split task counts once per chunk).
    pub placed_blocks: usize,
    /// Distinct tasks that got at least one block.
    pub tasks_placed: usize,
    /// Tasks that had no room on their day, in day order.
    pub unplaced: Vec<Unplaced>,
    /// Ids of locked blocks that overlap a calendar event (an event
    /// landed on a block the person had placed; it is flagged, never moved).
    pub conflicts: Vec<i64>,
    /// The horizon used, so the summary can say "next 14 days".
    pub horizon_days: i64,
}

impl BlockReport {
    /// A short human-readable summary.
    pub fn summary(&self) -> String {
        let plural = |n: usize, one: &str, many: &str| if n == 1 { one.to_string() } else { many.to_string() };
        let mut text = format!(
            "Placed {} {} for {} {}.",
            self.placed_blocks,
            plural(self.placed_blocks, "block", "blocks"),
            self.tasks_placed,
            plural(self.tasks_placed, "task", "tasks"),
        );
        if !self.unplaced.is_empty() {
            text.push_str(&format!(" {} didn't fit on their day.", self.unplaced.len()));
        }
        if !self.conflicts.is_empty() {
            text.push_str(&format!(
                " {} locked {} clash{} with an event.",
                self.conflicts.len(),
                plural(self.conflicts.len(), "block", "blocks"),
                if self.conflicts.len() == 1 { "es" } else { "" },
            ));
        }
        text
    }
}

/// The planner tuning from Settings.
pub fn block_config(conn: &Connection) -> Result<BlockConfig> {
    Ok(BlockConfig {
        granularity: settings::slot_granularity_minutes(conn)?,
        min_block: settings::min_block_minutes(conn)?,
        buffer: settings::event_buffer_minutes(conn)?,
        max_chunks: settings::max_chunks(conn)?,
        split_over: settings::split_over_minutes(conn)?,
        max_block: settings::max_block_minutes(conn)?,
    })
}

/// The planner window for `day`: its weekday's hours (or the default
/// window), or an empty window if that weekday is off.
fn day_window(conn: &Connection, day: NaiveDate) -> Result<DayWindow> {
    Ok(settings::work_day_for(conn, day)?.window(day))
}

/// Every calendar event as a busy interval across `today ..= last_day`:
/// recurring ones expanded, and expanded from the day *before* `today`,
/// so an event that started last night and runs into this morning still
/// blocks it. Shared by [`plan_blocks`] and [`day_capacities`] so the two
/// can never disagree about what is busy.
fn event_busy_intervals(conn: &Connection, today: NaiveDate, last_day: NaiveDate) -> Result<Vec<Interval>> {
    let all_events = events::list_all_events(conn)?;
    Ok(occurrence::occurrences_in_window(conn, &all_events, today - Duration::days(1), last_day)
        .into_values()
        .flatten()
        .map(|occ| Interval::new(occ.event.start_time.naive_local(), occ.event.end_time.naive_local()))
        .collect())
}

/// Free minutes per day across the horizon — what Auto-assign's
/// per-day budget is capped by (D2). A day's capacity is its working
/// window (for today, from `now` rounded up to the grid) minus calendar
/// events and the blocks of completed tasks, counted in gaps the planner
/// could actually use (snapped to the grid, shorter than the minimum
/// block dropped).
///
/// **Pinned blocks are deliberately not subtracted.** Auto-assign's
/// `load` already counts every manually-assigned task's full estimate,
/// and a pinned block belongs to exactly such a task; subtracting its
/// time here as well would count it twice.
pub fn day_capacities(conn: &Connection, today: NaiveDate, now: NaiveDateTime) -> Result<HashMap<NaiveDate, i64>> {
    let config = block_config(conn)?;
    let horizon_days = settings::auto_assign_horizon_days(conn)?;
    let last_day = today + Duration::days(horizon_days - 1);

    let mut busy = event_busy_intervals(conn, today, last_day)?;
    let completed_ids: HashSet<i64> =
        tasks::list_tasks(conn)?.into_iter().filter(|t| t.completed).map(|t| t.id).collect();
    busy.extend(
        task_blocks::list_blocks_between(conn, today, last_day)?
            .iter()
            .filter(|b| b.is_settled() || (!b.locked && completed_ids.contains(&b.task_id)))
            .map(|b| b.interval()),
    );

    Ok((0..horizon_days)
        .map(|i| today + Duration::days(i))
        .map(|day| {
            // A day off has an empty window, so its capacity is 0 and
            // Auto-assign gives it no work.
            let window = day_window(conn, day)?;
            let open = Interval::new(time_blocking::earliest_start(now, &window, &config), window.end);
            Ok((day, time_blocking::day_capacity(&open, &busy, &config)))
        })
        .collect::<Result<HashMap<_, _>>>()?)
}

/// Whether a task is the kind that gets a time block at all: not
/// finished, not tied to a revision session (the session is its time),
/// not a legacy topic-planning template, and not a container (a parent
/// with incomplete subtasks — the subtasks carry the work). One
/// definition, used by [`plan_blocks`] and by the UI's "unscheduled"
/// tag, so what the planner skips and what the screen calls
/// "unscheduled" can't drift apart.
pub fn is_blockable(task: &Task, containers: &HashSet<i64>) -> bool {
    !task.completed && task.event_id.is_none() && !task.plan_topics_ahead && !containers.contains(&task.id)
}

/// Removes every auto (unpinned) block from `from_day` on — the
/// "Clear auto blocks" command. Pinned blocks stay (they are the
/// person's), and so do finished tasks' blocks (history of the plan).
/// Returns how many blocks went. Tasks keep their assigned days.
pub fn clear_auto_blocks(conn: &Connection, from_day: NaiveDate) -> Result<usize> {
    let completed: HashSet<i64> =
        tasks::list_tasks(conn)?.into_iter().filter(|t| t.completed).map(|t| t.id).collect();
    let last_day = from_day + Duration::days(3650);
    let before = task_blocks::list_blocks_between(conn, from_day, last_day)?;
    let days: Vec<NaiveDate> = {
        let mut days: Vec<NaiveDate> = before.iter().map(|b| b.day()).collect();
        days.sort();
        days.dedup();
        days
    };
    task_blocks::replace_auto_blocks_for_days_keeping(conn, &days, &[], &completed)?;
    let after = task_blocks::list_blocks_between(conn, from_day, last_day)?.len();
    Ok(before.len() - after)
}

/// What the "Plan Times" palette command does, as a one-line message
/// for the To Do view. Declines (with the reason) while time blocking
/// is off rather than silently planning for someone who didn't opt in.
pub fn plan_times_command(conn: &Connection, today: NaiveDate, now: NaiveDateTime) -> String {
    match settings::time_blocking_enabled(conn) {
        Ok(true) => match plan_blocks(conn, today, now) {
            Ok(report) => report.summary(),
            Err(err) => format!("Plan times failed: {err}"),
        },
        Ok(false) => "Turn on time blocking in Settings first.".to_owned(),
        Err(err) => format!("Plan times failed: {err}"),
    }
}

/// What the "Clear Auto Blocks" palette command does, as a one-line
/// message. Works whether or not time blocking is on (clearing is
/// always safe: pinned and finished blocks stay).
pub fn clear_auto_blocks_command(conn: &Connection, today: NaiveDate) -> String {
    match clear_auto_blocks(conn, today) {
        Ok(1) => "Cleared 1 auto block. Pinned blocks were kept.".to_owned(),
        Ok(n) => format!("Cleared {n} auto blocks. Pinned blocks were kept."),
        Err(err) => format!("Clear failed: {err}"),
    }
}

/// The automatic re-plan (Q4): plans times again, but only when time
/// blocking **and** its automatic re-plan are both switched on. Returns
/// `None` when it did nothing. Called at launch and when the day changes
/// (from the app's daily cleanup), so a plan made yesterday doesn't sit
/// in the past.
///
/// It re-plans *times* only; it does not run Auto-assign, so it never
/// moves a task to another day on its own. Pinned blocks are left where
/// the person put them, like any run.
pub fn replan_if_enabled(conn: &Connection, today: NaiveDate, now: NaiveDateTime) -> Result<Option<BlockReport>> {
    if settings::time_blocking_enabled(conn)? && settings::auto_replan_blocks(conn)? {
        plan_blocks(conn, today, now).map(Some)
    } else {
        Ok(None)
    }
}

/// Plans times for **one** task, on or after `not_before`, without
/// touching anything else: every other block (planned, pinned, in
/// progress or done) is treated as busy time and left exactly as it is.
/// Used when a task has unfinished work to carry forward — re-planning
/// the whole horizon for that would shuffle blocks nobody asked to move.
///
/// The task's own stale *planned* blocks are dropped first (they are being
/// replaced); pinned ones and one being worked count toward its estimate,
/// as in [`plan_blocks`]. Returns how many blocks were placed — `0` when
/// the task isn't blockable, has nothing left to place, or doesn't fit
/// before its deadline / the end of the horizon (in which case it simply
/// stays unplanned, and shows as "unscheduled").
pub fn plan_task_blocks(
    conn: &Connection,
    today: NaiveDate,
    now: NaiveDateTime,
    task_id: i64,
    not_before: NaiveDate,
) -> Result<usize> {
    let config = block_config(conn)?;
    let horizon_days = settings::auto_assign_horizon_days(conn)?;
    let default_minutes = settings::auto_assign_default_task_minutes(conn)?;
    let last_day = today + Duration::days(horizon_days - 1);
    let first_day = not_before.max(today);

    // Its old plans go whether or not there is room for new ones.
    conn.execute(
        "DELETE FROM task_blocks WHERE task_id = ?1 AND status = ?2 AND locked = 0",
        rusqlite::params![task_id, crate::tasks::BlockStatus::Planned.to_db()],
    )?;
    if first_day > last_day {
        return Ok(0);
    }

    let all_tasks = tasks::list_tasks(conn)?;
    let containers = container_ids(&all_tasks);
    let Some(task) = all_tasks.iter().find(|t| t.id == task_id) else { return Ok(0) };
    if !is_blockable(task, &containers) {
        return Ok(0);
    }
    let due_by_id: HashMap<i64, NaiveDate> =
        all_tasks.iter().filter_map(|t| parse_day(t.due_date.as_deref()).map(|d| (t.id, d))).collect();

    let existing = task_blocks::list_blocks_between(conn, today, last_day)?;
    let mut busy = event_busy_intervals(conn, today, last_day)?;
    busy.extend(existing.iter().map(|b| b.interval()));

    let mine = existing.iter().filter(|b| {
        b.task_id == task_id
            && (b.status == crate::tasks::BlockStatus::InProgress || (b.locked && !b.is_settled()))
    });
    let (covered, pieces) = mine.fold((0, 0), |(m, n), b| (m + b.duration().num_minutes().max(0), n + 1));
    let remaining = task_minutes(task, default_minutes) - covered;
    if remaining <= 0 {
        return Ok(0);
    }

    let assigned = parse_day(task.assigned_date.as_deref()).map_or(first_day, |d| d.max(first_day));
    let horizon_task = HorizonTask {
        id: task_id,
        minutes: remaining,
        assigned,
        due: effective_deadline(task, &due_by_id),
        position: task.position,
        locked_chunks: pieces,
    };
    let windows: Vec<DayWindow> = (0..=(last_day - first_day).num_days())
        .map(|i| day_window(conn, first_day + Duration::days(i)))
        .collect::<Result<_>>()?;
    let placed = time_blocking::plan_horizon(&windows, &busy, &[horizon_task], &config, now);
    task_blocks::insert_auto_blocks(conn, &placed.blocks)?;
    Ok(placed.blocks.len())
}

/// Plans times for every task assigned to a day in
/// `today ..= today + horizon - 1`, as of `now`, and writes the result.
///
/// Auto blocks on those days are replaced wholesale, so running it
/// twice gives the same plan (and picks up new events, finished tasks
/// and changed settings). Does not check `time_blocking_enabled` — that
/// gates whether the UI offers the command, not what the command does.
pub fn plan_blocks(conn: &Connection, today: NaiveDate, now: NaiveDateTime) -> Result<BlockReport> {
    let config = block_config(conn)?;
    let horizon_days = settings::auto_assign_horizon_days(conn)?;
    let default_minutes = settings::auto_assign_default_task_minutes(conn)?;

    let days: Vec<NaiveDate> = (0..horizon_days).map(|i| today + Duration::days(i)).collect();
    let last_day = *days.last().expect("horizon is at least one day");
    let day_set: HashSet<NaiveDate> = days.iter().copied().collect();

    // --- Busy time: events ------------------------------------------------
    let event_busy = event_busy_intervals(conn, today, last_day)?;

    // --- Busy time: blocks we must not move ---------------------------------
    let all_tasks = tasks::list_tasks(conn)?;
    let completed_ids: HashSet<i64> = all_tasks.iter().filter(|t| t.completed).map(|t| t.id).collect();

    let existing = task_blocks::list_blocks_between(conn, today, last_day)?;
    // Pinned plans, which are the person's and stay put...
    let locked: Vec<_> = existing.iter().filter(|b| b.locked && !b.is_settled()).collect();
    // ...and history, which is kept and is busy time too: blocks being worked
    // or already worked, and the blocks of finished tasks.
    let history: Vec<_> = existing
        .iter()
        .filter(|b| b.is_settled() || (!b.locked && completed_ids.contains(&b.task_id)))
        .collect();

    let mut busy: Vec<Interval> = event_busy.clone();
    busy.extend(locked.iter().map(|b| b.interval()));
    busy.extend(history.iter().map(|b| b.interval()));

    let conflicts = time_blocking::find_conflicts(
        &locked.iter().map(|b| (b.id, b.interval())).collect::<Vec<_>>(),
        &event_busy,
    );

    // --- Tasks to place, grouped by day ----------------------------------------
    let containers = container_ids(&all_tasks);
    let due_by_id: HashMap<i64, NaiveDate> =
        all_tasks.iter().filter_map(|t| parse_day(t.due_date.as_deref()).map(|d| (t.id, d))).collect();
    // Minutes and pieces already accounted for per task: pieces the person
    // pinned, and a block a timer/stopwatch is running against right now
    // (that *is* the planned work, so it must not be planned a second time).
    // Worked (done) blocks are deliberately not counted: a task's estimate
    // is what is still to do, and finishing a session reduces it (see
    // `carry_over`), so counting them here would subtract the time twice.
    let mut pinned: HashMap<i64, (i64, usize)> = HashMap::new();
    for block in existing.iter().filter(|b| {
        b.status == crate::tasks::BlockStatus::InProgress || (b.locked && !b.is_settled())
    }) {
        let entry = pinned.entry(block.task_id).or_default();
        entry.0 += block.duration().num_minutes().max(0);
        entry.1 += 1;
    }

    let mut to_place: Vec<HorizonTask> = Vec::new();
    for task in &all_tasks {
        if !is_blockable(task, &containers) {
            continue;
        }
        let Some(assigned) = parse_day(task.assigned_date.as_deref()) else { continue };
        if !day_set.contains(&assigned) {
            continue;
        }
        let (pinned_minutes, pinned_pieces) = pinned.get(&task.id).copied().unwrap_or((0, 0));
        let remaining = task_minutes(task, default_minutes) - pinned_minutes;
        if remaining <= 0 {
            continue; // the pinned pieces already cover it
        }
        to_place.push(HorizonTask {
            id: task.id,
            minutes: remaining,
            assigned,
            due: effective_deadline(task, &due_by_id),
            position: task.position,
            locked_chunks: pinned_pieces,
        });
    }

    // --- Place across the horizon ------------------------------------------------------
    let windows: Vec<DayWindow> = days.iter().map(|day| day_window(conn, *day)).collect::<Result<_>>()?;
    let placed = time_blocking::plan_horizon(&windows, &busy, &to_place, &config, now);
    let new_blocks = placed.blocks;
    let unplaced: Vec<Unplaced> =
        placed.unplaced.into_iter().map(|(task_id, day)| Unplaced { task_id, day }).collect();

    // --- Write ----------------------------------------------------------------------
    task_blocks::replace_auto_blocks_for_days_keeping(conn, &days, &new_blocks, &completed_ids)?;

    Ok(BlockReport {
        placed_blocks: new_blocks.len(),
        tasks_placed: new_blocks.iter().map(|b| b.task_id).collect::<HashSet<_>>().len(),
        unplaced,
        conflicts,
        horizon_days,
    })
}

/// Minutes of a task already spoken for by its pinned blocks and one being
/// worked — what the planner subtracts from the estimate before placing the
/// rest (done blocks are history, and unpinned ones are being replaced).
fn covered_minutes(conn: &Connection, task_id: i64) -> Result<i64> {
    Ok(task_blocks::list_blocks_for_task(conn, task_id)?
        .iter()
        .filter(|b| b.status == crate::tasks::BlockStatus::InProgress || (b.locked && !b.is_settled()))
        .map(|b| b.duration().num_minutes().max(0))
        .sum())
}

/// How much of a task's estimate is not yet covered by pinned or in-progress
/// blocks — what a re-plan of that one task would still have to place. `0`
/// for a task that doesn't exist.
pub fn minutes_still_to_place(conn: &Connection, task_id: i64) -> Result<i64> {
    let default_minutes = settings::auto_assign_default_task_minutes(conn)?;
    let all_tasks = tasks::list_tasks(conn)?;
    let Some(task) = all_tasks.iter().find(|t| t.id == task_id) else { return Ok(0) };
    Ok((task_minutes(task, default_minutes) - covered_minutes(conn, task_id)?).max(0))
}

/// What dropping a task on the Day timeline would do. Read-only: the live
/// preview asks this every frame and [`drop_task`] asks it once more on
/// release, so what is shown is what is saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPlan {
    /// The block it would get (possibly shorter than the task needs).
    Place(DropPlacement),
    /// No free time near enough to the drop point.
    NoRoom,
    /// The task takes no block (finished, a container, tied to a session,
    /// not on this day) or its pinned pieces already cover its estimate.
    NothingToPlace,
}

/// Plans a task dropped onto `day`'s timeline at `drop_minutes` after
/// midnight, inside `bounds` (the working window, or the whole day when
/// time blocking is off). The time it needs is its estimate (or the
/// default length) less what is already pinned or being worked, like
/// [`plan_blocks`]; its own *unpinned* blocks don't count as busy, since
/// the drop replaces them. Every event and every other block does.
pub fn plan_drop(
    conn: &Connection,
    task_id: i64,
    day: NaiveDate,
    drop_minutes: i64,
    bounds: (i64, i64),
) -> Result<DropPlan> {
    let config = block_config(conn)?;
    let default_minutes = settings::auto_assign_default_task_minutes(conn)?;
    let all_tasks = tasks::list_tasks(conn)?;
    let containers = container_ids(&all_tasks);
    let Some(task) = all_tasks.iter().find(|t| t.id == task_id) else { return Ok(DropPlan::NothingToPlace) };
    if !is_blockable(task, &containers) || parse_day(task.assigned_date.as_deref()) != Some(day) {
        return Ok(DropPlan::NothingToPlace);
    }

    let planned = crate::tasks::BlockStatus::Planned;
    let wanted = task_minutes(task, default_minutes) - covered_minutes(conn, task_id)?;
    if wanted <= 0 {
        return Ok(DropPlan::NothingToPlace);
    }

    let mut busy = event_busy_intervals(conn, day, day)?;
    busy.extend(
        task_blocks::list_blocks_between(conn, day, day)?
            .iter()
            .filter(|b| !(b.task_id == task_id && b.status == planned && !b.locked))
            .map(|b| b.interval()),
    );
    let window = DayWindow::from_minutes(day, bounds.0, bounds.1).interval();
    let drop_at = day.and_time(NaiveTime::MIN) + Duration::minutes(drop_minutes);
    Ok(match time_blocking::place_dropped(drop_at, wanted, &window, &busy, &config) {
        Some(placement) => DropPlan::Place(placement),
        None => DropPlan::NoRoom,
    })
}

/// What [`drop_task`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropOutcome {
    /// A pinned block was written. `left_over` minutes didn't fit and
    /// were assumed unfinished; `replanned` is how many blocks the planner
    /// then found for them (`0`: none, or time blocking is off).
    Placed { placement: DropPlacement, left_over: i64, replanned: usize },
    NoRoom,
    NothingToPlace,
}

impl DropOutcome {
    /// One line for the Day view.
    pub fn summary(&self, title: &str) -> String {
        match self {
            DropOutcome::Placed { placement, left_over, replanned } => {
                let times = format!("{}–{}", placement.start.format("%H:%M"), placement.end.format("%H:%M"));
                if *left_over <= 0 {
                    format!("Placed “{title}” at {times}.")
                } else if *replanned > 0 {
                    format!(
                        "Only {} of {} min of “{title}” fit at {times}; the other {left_over} min is planned in another slot.",
                        placement.minutes(),
                        placement.wanted
                    )
                } else {
                    format!(
                        "Only {} of {} min of “{title}” fit at {times}; the other {left_over} min has no slot yet and stays unscheduled.",
                        placement.minutes(),
                        placement.wanted
                    )
                }
            }
            DropOutcome::NoRoom => format!("No free time there for “{title}”."),
            DropOutcome::NothingToPlace => format!("“{title}” has nothing left to place (its time is already pinned)."),
        }
    }
}

/// Saves a task dropped onto the timeline (see [`plan_drop`]): its own
/// unpinned blocks are replaced by one **pinned** block where it landed,
/// and the task becomes the person's (as with any hand edit). If the
/// space was too short the task is assumed not to be finished there: its
/// estimate is left alone, and when time blocking is on the rest is
/// planned into other free time (this day first, then onward) without
/// touching any other block. Nothing is written unless there is a block
/// to place.
pub fn drop_task(
    conn: &Connection,
    task_id: i64,
    day: NaiveDate,
    drop_minutes: i64,
    bounds: (i64, i64),
    today: NaiveDate,
    now: NaiveDateTime,
) -> Result<DropOutcome> {
    let placement = match plan_drop(conn, task_id, day, drop_minutes, bounds)? {
        DropPlan::Place(placement) => placement,
        DropPlan::NoRoom => return Ok(DropOutcome::NoRoom),
        DropPlan::NothingToPlace => return Ok(DropOutcome::NothingToPlace),
    };

    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM task_blocks WHERE task_id = ?1 AND status = ?2 AND locked = 0",
        rusqlite::params![task_id, crate::tasks::BlockStatus::Planned.to_db()],
    )?;
    let pieces = task_blocks::list_blocks_for_task(&tx, task_id)?.len() as i64;
    task_blocks::insert_locked_block(
        &tx,
        task_id,
        time_blocking::naive_to_local(placement.start),
        time_blocking::naive_to_local(placement.end),
        pieces,
    )?;
    tx.commit()?;

    let left_over = (placement.wanted - placement.minutes()).max(0);
    let replanned = if left_over > 0 && settings::time_blocking_enabled(conn)? {
        plan_task_blocks(conn, today, now, task_id, day)?
    } else {
        0
    };
    Ok(DropOutcome::Placed { placement, left_over, replanned })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::events::{Frequency, RecurrenceRule};
    use chrono::{DateTime, Local, NaiveTime, TimeZone};
    use rusqlite::params;

    /// "Today" for every test: Wednesday 14 October 2026.
    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 14).unwrap()
    }

    fn day(offset: i64) -> NaiveDate {
        today() + Duration::days(offset)
    }

    /// The evening before: nothing is "in the past" on any planned day.
    fn early() -> NaiveDateTime {
        today().pred_opt().unwrap().and_hms_opt(20, 0, 0).unwrap()
    }

    fn at(offset: i64, h: u32, m: u32) -> NaiveDateTime {
        day(offset).and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap())
    }

    fn local(offset: i64, h: u32, m: u32) -> DateTime<Local> {
        Local.from_local_datetime(&at(offset, h, m)).earliest().unwrap()
    }

    fn db() -> Database {
        let db = Database::open_in_memory().expect("open in-memory db");
        // Most of these tests are about events, pinning and capacity, with each task one
        // block; threshold splitting (default 30 min) has its own tests further down.
        settings::set_split_over_minutes(&db.conn, 100_000).expect("no threshold splitting");
        db
    }

    /// A task assigned to `offset` days from today with an estimate.
    fn task(db: &Database, title: &str, offset: i64, minutes: i64) -> i64 {
        let id = tasks::create_task(&db.conn, title, None, None, None).expect("create task").id;
        tasks::set_assigned_date(&db.conn, id, Some(&day(offset).format("%Y-%m-%d").to_string())).expect("assign");
        db.conn.execute("UPDATE tasks SET estimated_minutes = ?1 WHERE id = ?2", params![minutes, id]).expect("estimate");
        id
    }

    fn event(db: &Database, offset: i64, h1: u32, m1: u32, h2: u32, m2: u32) -> i64 {
        events::create_event(&db.conn, "Event", "", local(offset, h1, m1), local(offset, h2, m2), None, None)
            .expect("create event")
            .id
    }

    fn blocks_of(db: &Database, task_id: i64) -> Vec<(NaiveDateTime, NaiveDateTime)> {
        task_blocks::list_blocks_for_task(&db.conn, task_id)
            .expect("list")
            .iter()
            .map(|b| (b.start_time.naive_local(), b.end_time.naive_local()))
            .collect()
    }

    fn plan(db: &Database) -> BlockReport {
        plan_blocks(&db.conn, today(), early()).expect("plan_blocks")
    }

    // --- placement ------------------------------------------------------

    #[test]
    fn a_task_is_placed_at_the_start_of_the_working_window_on_its_day() {
        let db = db();
        let t = task(&db, "Essay", 1, 60);

        let report = plan(&db);

        assert_eq!(blocks_of(&db, t), vec![(at(1, 9, 0), at(1, 10, 0))]);
        assert_eq!(report.placed_blocks, 1);
        assert_eq!(report.tasks_placed, 1);
        assert!(report.unplaced.is_empty());
    }

    #[test]
    fn tasks_on_a_day_are_placed_around_its_events() {
        let db = db();
        event(&db, 1, 9, 0, 11, 0);
        let t = task(&db, "Essay", 1, 60);

        plan(&db);

        assert_eq!(blocks_of(&db, t), vec![(at(1, 11, 0), at(1, 12, 0))]);
    }

    #[test]
    fn a_task_with_no_estimate_uses_the_default_task_length() {
        let db = db();
        let t = tasks::create_task(&db.conn, "No estimate", None, None, None).expect("create").id;
        tasks::set_assigned_date(&db.conn, t, Some(&day(1).format("%Y-%m-%d").to_string())).expect("assign");

        plan(&db);

        // The Auto-assign default of 30 minutes.
        assert_eq!(blocks_of(&db, t), vec![(at(1, 9, 0), at(1, 9, 30))]);
    }

    #[test]
    fn a_recurring_event_blocks_every_day_it_occurs_on() {
        let db = db();
        let rule = queries_rule(&db);
        events::create_event(&db.conn, "Daily", "", local(0, 9, 0), local(0, 10, 0), None, Some(rule)).expect("event");
        let tomorrow = task(&db, "Tomorrow", 1, 60);
        let later = task(&db, "Later", 3, 60);

        plan(&db);

        assert_eq!(blocks_of(&db, tomorrow), vec![(at(1, 10, 0), at(1, 11, 0))], "expanded, not just the anchor day");
        assert_eq!(blocks_of(&db, later), vec![(at(3, 10, 0), at(3, 11, 0))]);
    }

    fn queries_rule(db: &Database) -> i64 {
        crate::database::queries::recurrence::create_rule(
            &db.conn,
            &RecurrenceRule {
                id: None,
                frequency: Frequency::Daily,
                interval: 1,
                weekdays: Vec::new(),
                end_date: None,
                occurrence_limit: None,
            },
        )
        .expect("create rule")
    }

    #[test]
    fn an_event_spilling_past_midnight_blocks_the_start_of_the_next_morning() {
        let db = db();
        // 22:00 yesterday to 09:30 today.
        events::create_event(&db.conn, "Night shift", "", local(-1, 22, 0), local(0, 9, 30), None, None).expect("event");
        let t = task(&db, "Essay", 0, 60);

        plan_blocks(&db.conn, today(), at(-1, 12, 0)).expect("plan");

        assert_eq!(blocks_of(&db, t), vec![(at(0, 9, 30), at(0, 10, 30))]);
    }

    #[test]
    fn today_is_never_planned_in_the_past() {
        let db = db();
        let t = task(&db, "Essay", 0, 60);

        plan_blocks(&db.conn, today(), at(0, 13, 1)).expect("plan");

        assert_eq!(blocks_of(&db, t), vec![(at(0, 13, 15), at(0, 14, 15))]);
    }

    #[test]
    fn tasks_are_ordered_by_deadline_within_a_day() {
        let db = db();
        let relaxed = task(&db, "Relaxed", 1, 60);
        let urgent = task(&db, "Urgent", 1, 60);
        db.conn
            .execute("UPDATE tasks SET due_date = ?1 WHERE id = ?2", params![day(2).format("%Y-%m-%d").to_string(), urgent])
            .expect("due");

        plan(&db);

        assert_eq!(blocks_of(&db, urgent), vec![(at(1, 9, 0), at(1, 10, 0))]);
        assert_eq!(blocks_of(&db, relaxed), vec![(at(1, 10, 0), at(1, 11, 0))]);
    }

    // --- who is (not) placed ------------------------------------------------

    #[test]
    fn unassigned_completed_container_and_session_linked_tasks_get_no_block() {
        let db = db();
        let unassigned = tasks::create_task(&db.conn, "Unassigned", None, None, None).expect("create").id;
        let completed = task(&db, "Completed", 1, 30);
        tasks::set_completed(&db.conn, completed, true).expect("complete");

        let parent = task(&db, "Parent", 1, 30);
        let child = task(&db, "Child", 1, 30);
        tasks::update_task(&db.conn, child, "Child", "", None, None, None, false, Some(parent), None).expect("attach");

        let session = event(&db, 5, 15, 0, 16, 0);
        let linked = task(&db, "Linked", 1, 30);
        tasks::set_event(&db.conn, linked, Some(session)).expect("link");

        plan(&db);

        assert!(blocks_of(&db, unassigned).is_empty());
        assert!(blocks_of(&db, completed).is_empty());
        assert!(blocks_of(&db, parent).is_empty(), "a container's subtasks carry the work");
        assert!(blocks_of(&db, linked).is_empty(), "the session already covers its time");
        assert_eq!(blocks_of(&db, child).len(), 1);
    }

    #[test]
    fn a_task_assigned_beyond_the_horizon_is_left_alone() {
        let db = db();
        let t = task(&db, "Far away", 40, 60);

        let report = plan(&db);

        assert!(blocks_of(&db, t).is_empty());
        assert_eq!(report.placed_blocks, 0);
    }

    // --- not fitting ------------------------------------------------------------

    #[test]
    fn with_splitting_off_a_task_that_does_not_fit_its_day_is_reported_not_moved() {
        let db = db();
        settings::set_max_chunks(&db.conn, 1).expect("never split");
        event(&db, 1, 9, 0, 21, 0); // the whole working window
        let t = task(&db, "Too late", 1, 60);

        let report = plan(&db);

        assert!(blocks_of(&db, t).is_empty());
        assert_eq!(report.unplaced, vec![Unplaced { task_id: t, day: day(1) }]);
        let reloaded = tasks::list_tasks(&db.conn).expect("list").into_iter().find(|task| task.id == t).expect("exists");
        assert_eq!(
            reloaded.assigned_date.as_deref(),
            Some(day(1).format("%Y-%m-%d").to_string().as_str()),
            "the day is Stage A's call"
        );
    }


    // --- splitting, across gaps and across days -----------------------------

    #[test]
    fn a_task_too_big_for_its_gaps_is_split_and_the_rest_carries_to_the_next_day() {
        let db = db();
        // Day 1 has only a 30-minute and a 60-minute gap.
        event(&db, 1, 9, 30, 20, 0);
        let t = task(&db, "Maths assignment", 1, 120);

        let report = plan(&db);

        assert!(report.unplaced.is_empty());
        assert_eq!(
            blocks_of(&db, t),
            vec![(at(1, 9, 0), at(1, 9, 30)), (at(1, 20, 0), at(1, 21, 0)), (at(2, 9, 0), at(2, 9, 30))]
        );
        assert_eq!(report.placed_blocks, 3);
        assert_eq!(report.tasks_placed, 1, "three pieces, one task");
    }

    #[test]
    fn a_split_task_keeps_its_assigned_day() {
        // Stage A owns the day; the pieces just run on from it.
        let db = db();
        event(&db, 1, 9, 30, 20, 0);
        let t = task(&db, "Maths assignment", 1, 120);

        plan(&db);

        let reloaded = tasks::list_tasks(&db.conn).expect("list").into_iter().find(|x| x.id == t).expect("exists");
        assert_eq!(reloaded.assigned_date.as_deref(), Some(day(1).format("%Y-%m-%d").to_string().as_str()));
    }

    #[test]
    fn carrying_stops_at_the_deadline_and_no_piece_is_kept() {
        let db = db();
        event(&db, 1, 9, 30, 20, 0);
        event(&db, 2, 9, 0, 21, 0);
        let t = task(&db, "Maths assignment", 1, 120);
        db.conn
            .execute("UPDATE tasks SET due_date = ?1 WHERE id = ?2", params![day(2).format("%Y-%m-%d").to_string(), t])
            .expect("due");

        let report = plan(&db);

        assert!(blocks_of(&db, t).is_empty(), "never a half-planned task");
        assert_eq!(report.unplaced, vec![Unplaced { task_id: t, day: day(1) }]);
    }

    #[test]
    fn a_pinned_piece_counts_toward_the_task_and_only_the_remainder_is_planned() {
        let db = db();
        let t = task(&db, "Maths assignment", 1, 120);
        task_blocks::insert_locked_block(&db.conn, t, local(1, 15, 0), local(1, 16, 0), 0).expect("pin one hour");

        let report = plan(&db);

        // 120 minutes total, 60 pinned: one new 60-minute piece.
        let mut blocks = blocks_of(&db, t);
        blocks.sort();
        assert_eq!(blocks, vec![(at(1, 9, 0), at(1, 10, 0)), (at(1, 15, 0), at(1, 16, 0))]);
        assert_eq!(report.placed_blocks, 1, "only the new piece is counted");
    }

    #[test]
    fn a_task_whose_pinned_pieces_cover_its_estimate_gets_nothing_more() {
        let db = db();
        let t = task(&db, "Pinned", 1, 60);
        task_blocks::insert_locked_block(&db.conn, t, local(1, 15, 0), local(1, 16, 0), 0).expect("pin");

        let report = plan(&db);

        assert_eq!(blocks_of(&db, t), vec![(at(1, 15, 0), at(1, 16, 0))]);
        assert_eq!(report.placed_blocks, 0);
    }

    #[test]
    fn planning_a_split_task_twice_gives_the_same_pieces() {
        let db = db();
        event(&db, 1, 9, 30, 20, 0);
        let t = task(&db, "Maths assignment", 1, 120);

        plan(&db);
        let first = blocks_of(&db, t);
        plan(&db);

        assert_eq!(blocks_of(&db, t), first);
        assert_eq!(task_blocks::list_blocks_between(&db.conn, day(0), day(30)).expect("list").len(), 3);
    }

    // --- re-planning -------------------------------------------------------------

    #[test]
    fn planning_twice_gives_the_same_plan() {
        let db = db();
        event(&db, 1, 10, 0, 11, 0);
        let a = task(&db, "A", 1, 60);
        let b = task(&db, "B", 1, 90);
        let c = task(&db, "C", 2, 45);

        plan(&db);
        let first: Vec<_> = [a, b, c].iter().map(|id| blocks_of(&db, *id)).collect();
        let second_report = plan(&db);
        let second: Vec<_> = [a, b, c].iter().map(|id| blocks_of(&db, *id)).collect();

        assert_eq!(first, second);
        assert_eq!(second_report.placed_blocks, 3);
        let total: usize = task_blocks::list_blocks_between(&db.conn, day(0), day(30)).expect("list").len();
        assert_eq!(total, 3, "no duplicates left behind");
    }

    #[test]
    fn a_new_event_is_picked_up_on_the_next_run() {
        let db = db();
        let t = task(&db, "Essay", 1, 60);
        plan(&db);
        assert_eq!(blocks_of(&db, t), vec![(at(1, 9, 0), at(1, 10, 0))]);

        event(&db, 1, 9, 0, 10, 30);
        plan(&db);

        assert_eq!(blocks_of(&db, t), vec![(at(1, 10, 30), at(1, 11, 30))]);
    }

    #[test]
    fn changed_settings_apply_on_the_next_run() {
        let db = db();
        let t = task(&db, "Essay", 1, 60);
        plan(&db);

        settings::set_work_window_minutes(&db.conn, 13 * 60, 18 * 60).expect("window");
        plan(&db);

        assert_eq!(blocks_of(&db, t), vec![(at(1, 13, 0), at(1, 14, 0))]);
    }

    #[test]
    fn the_event_buffer_setting_keeps_a_gap_around_events() {
        let db = db();
        settings::set_event_buffer_minutes(&db.conn, 30).expect("buffer");
        event(&db, 1, 9, 0, 10, 0);
        let t = task(&db, "Essay", 1, 60);

        plan(&db);

        assert_eq!(blocks_of(&db, t), vec![(at(1, 10, 30), at(1, 11, 30))]);
    }

    // --- locked blocks -------------------------------------------------------------

    #[test]
    fn a_locked_block_is_kept_and_other_tasks_plan_around_it() {
        let db = db();
        let pinned = task(&db, "Pinned", 1, 60);
        task_blocks::insert_locked_block(&db.conn, pinned, local(1, 9, 0), local(1, 10, 0), 0).expect("lock");
        let other = task(&db, "Other", 1, 60);

        plan(&db);

        assert_eq!(blocks_of(&db, pinned), vec![(at(1, 9, 0), at(1, 10, 0))], "kept, and not duplicated");
        assert_eq!(blocks_of(&db, other), vec![(at(1, 10, 0), at(1, 11, 0))]);
    }

    #[test]
    fn a_locked_block_that_an_event_landed_on_is_flagged_not_moved() {
        let db = db();
        let pinned = task(&db, "Pinned", 1, 60);
        let block = task_blocks::insert_locked_block(&db.conn, pinned, local(1, 14, 0), local(1, 15, 0), 0).expect("lock");
        event(&db, 1, 14, 30, 15, 30);

        let report = plan(&db);

        assert_eq!(report.conflicts, vec![block.id]);
        assert_eq!(blocks_of(&db, pinned), vec![(at(1, 14, 0), at(1, 15, 0))], "never moved");
        assert!(report.summary().contains("clash"));
    }

    // --- completed tasks --------------------------------------------------------------

    #[test]
    fn a_completed_tasks_block_survives_a_re_plan_and_is_not_double_booked() {
        let db = db();
        let done = task(&db, "Done", 1, 60);
        plan(&db);
        assert_eq!(blocks_of(&db, done), vec![(at(1, 9, 0), at(1, 10, 0))]);
        tasks::set_completed(&db.conn, done, true).expect("complete");
        let next = task(&db, "Next", 1, 60);

        plan(&db);

        assert_eq!(blocks_of(&db, done), vec![(at(1, 9, 0), at(1, 10, 0))], "history is kept");
        assert_eq!(blocks_of(&db, next), vec![(at(1, 10, 0), at(1, 11, 0))], "and its time is not reused");
    }

    // --- scope ---------------------------------------------------------------------------

    #[test]
    fn blocks_outside_the_horizon_are_not_touched() {
        let db = db();
        let far = task(&db, "Far", 40, 60);
        let kept = task_blocks::insert_locked_block(&db.conn, far, local(40, 9, 0), local(40, 10, 0), 0).expect("block");
        // An *unlocked* block beyond the horizon, as an earlier, wider run might have left.
        db.conn
            .execute(
                "INSERT INTO task_blocks (task_id, start_time, end_time, locked, chunk_index) VALUES (?1, ?2, ?3, 0, 1)",
                params![far, local(41, 9, 0).to_rfc3339(), local(41, 10, 0).to_rfc3339()],
            )
            .expect("insert");

        plan(&db);

        assert_eq!(task_blocks::list_blocks_for_task(&db.conn, far).expect("list").len(), 2);
        assert!(task_blocks::list_blocks_for_task(&db.conn, far).expect("list").iter().any(|b| b.id == kept.id));
    }

    #[test]
    fn an_empty_database_plans_nothing_and_says_so() {
        let db = db();
        let report = plan(&db);
        assert_eq!(report.placed_blocks, 0);
        assert_eq!(report.horizon_days, 14);
        assert_eq!(report.summary(), "Placed 0 blocks for 0 tasks.");
    }

    // --- summary ------------------------------------------------------------------------------

    #[test]
    fn the_summary_reads_naturally_for_each_combination() {
        let base = BlockReport { placed_blocks: 1, tasks_placed: 1, horizon_days: 14, ..Default::default() };
        assert_eq!(base.summary(), "Placed 1 block for 1 task.");

        let more = BlockReport { placed_blocks: 5, tasks_placed: 4, horizon_days: 14, ..Default::default() };
        assert_eq!(more.summary(), "Placed 5 blocks for 4 tasks.");

        let misses = BlockReport {
            unplaced: vec![Unplaced { task_id: 1, day: today() }, Unplaced { task_id: 2, day: today() }],
            ..more.clone()
        };
        assert_eq!(misses.summary(), "Placed 5 blocks for 4 tasks. 2 didn't fit on their day.");

        let clashes = BlockReport { conflicts: vec![7], ..base.clone() };
        assert_eq!(clashes.summary(), "Placed 1 block for 1 task. 1 locked block clashes with an event.");

        let many_clashes = BlockReport { conflicts: vec![7, 8], ..base };
        assert_eq!(many_clashes.summary(), "Placed 1 block for 1 task. 2 locked blocks clash with an event.");
    }

    // --- day_capacities (feeds Auto-assign, D2) --------------------------------------

    fn capacities(db: &Database, now: NaiveDateTime) -> HashMap<NaiveDate, i64> {
        day_capacities(&db.conn, today(), now).expect("day_capacities")
    }

    #[test]
    fn an_empty_day_has_the_whole_working_window_free_and_the_horizon_is_covered() {
        let db = db();
        let cap = capacities(&db, early());
        assert_eq!(cap.len(), 14);
        assert_eq!(cap[&day(1)], 12 * 60);
        assert!(!cap.contains_key(&day(14)), "one past the horizon");
    }

    #[test]
    fn events_reduce_a_days_capacity_and_a_booked_day_has_none() {
        let db = db();
        event(&db, 1, 10, 0, 12, 0);
        event(&db, 2, 9, 0, 21, 0);
        let cap = capacities(&db, early());
        assert_eq!(cap[&day(1)], 10 * 60);
        assert_eq!(cap[&day(2)], 0);
        assert_eq!(cap[&day(3)], 12 * 60);
    }

    #[test]
    fn todays_capacity_only_counts_the_time_still_ahead() {
        let db = db();
        let cap = capacities(&db, at(0, 15, 1));
        assert_eq!(cap[&day(0)], 5 * 60 + 45, "from 15:15 to 21:00");
        assert_eq!(cap[&day(1)], 12 * 60);
    }

    #[test]
    fn a_completed_tasks_block_takes_capacity_but_a_pinned_one_does_not() {
        let db = db();
        let done = task(&db, "Done", 1, 60);
        let pinned = task(&db, "Pinned", 1, 60);
        task_blocks::insert_locked_block(&db.conn, pinned, local(1, 12, 0), local(1, 13, 0), 0).expect("pin");
        plan(&db);
        tasks::set_completed(&db.conn, done, true).expect("complete");

        let cap = capacities(&db, early());

        // The finished task's hour is gone; the pinned task's hour still counts as free
        // because Auto-assign already counts that task's minutes as load.
        assert_eq!(cap[&day(1)], 11 * 60);
    }

    #[test]
    fn capacity_follows_the_working_window_setting() {
        let db = db();
        settings::set_work_window_minutes(&db.conn, 9 * 60, 12 * 60).expect("window");
        assert_eq!(capacities(&db, early())[&day(1)], 3 * 60);
    }

    // --- automatic re-plan (Q4) ------------------------------------------------------------

    #[test]
    fn the_automatic_replan_does_nothing_unless_both_switches_are_on() {
        let db = db();
        let t = task(&db, "Essay", 1, 60);

        // Neither on.
        assert_eq!(replan_if_enabled(&db.conn, today(), early()).expect("replan"), None);
        // Time blocking on, auto re-plan off.
        settings::set_time_blocking_enabled(&db.conn, true).expect("enable");
        assert_eq!(replan_if_enabled(&db.conn, today(), early()).expect("replan"), None);
        // Auto re-plan on, time blocking off.
        settings::set_time_blocking_enabled(&db.conn, false).expect("disable");
        settings::set_auto_replan_blocks(&db.conn, true).expect("auto");
        assert_eq!(replan_if_enabled(&db.conn, today(), early()).expect("replan"), None);

        assert!(blocks_of(&db, t).is_empty(), "no block was written by any of those");
    }

    #[test]
    fn the_automatic_replan_plans_times_when_both_are_on() {
        let db = db();
        settings::set_time_blocking_enabled(&db.conn, true).expect("enable");
        settings::set_auto_replan_blocks(&db.conn, true).expect("auto");
        let t = task(&db, "Essay", 1, 60);

        let report = replan_if_enabled(&db.conn, today(), early()).expect("replan").expect("it ran");

        assert_eq!(report.placed_blocks, 1);
        assert_eq!(blocks_of(&db, t), vec![(at(1, 9, 0), at(1, 10, 0))]);
    }

    #[test]
    fn the_automatic_replan_never_moves_a_task_to_another_day_or_a_pinned_block() {
        let db = db();
        settings::set_time_blocking_enabled(&db.conn, true).expect("enable");
        settings::set_auto_replan_blocks(&db.conn, true).expect("auto");
        let t = task(&db, "Essay", 1, 60);
        task_blocks::insert_locked_block(&db.conn, t, local(1, 15, 0), local(1, 16, 0), 0).expect("pin");
        let day_before = tasks::list_tasks(&db.conn).unwrap().into_iter().find(|x| x.id == t).unwrap().assigned_date;

        replan_if_enabled(&db.conn, today(), early()).expect("replan");

        assert_eq!(blocks_of(&db, t), vec![(at(1, 15, 0), at(1, 16, 0))]);
        let day_after = tasks::list_tasks(&db.conn).unwrap().into_iter().find(|x| x.id == t).unwrap().assigned_date;
        assert_eq!(day_before, day_after);
    }

    // --- is_blockable / clear_auto_blocks (Phase 4) -----------------------------------------

    fn find(db: &Database, id: i64) -> Task {
        tasks::list_tasks(&db.conn).expect("list").into_iter().find(|t| t.id == id).expect("exists")
    }

    #[test]
    fn is_blockable_excludes_exactly_what_the_planner_skips() {
        let db = db();
        let plain = task(&db, "Plain", 1, 30);
        let completed = task(&db, "Completed", 1, 30);
        tasks::set_completed(&db.conn, completed, true).expect("complete");
        let parent = task(&db, "Parent", 1, 30);
        let child = task(&db, "Child", 1, 30);
        tasks::update_task(&db.conn, child, "Child", "", None, None, None, false, Some(parent), None).expect("attach");
        let session = event(&db, 5, 15, 0, 16, 0);
        let linked = task(&db, "Linked", 1, 30);
        tasks::set_event(&db.conn, linked, Some(session)).expect("link");

        let all = tasks::list_tasks(&db.conn).expect("list");
        let containers = container_ids(&all);

        assert!(is_blockable(&find(&db, plain), &containers));
        assert!(is_blockable(&find(&db, child), &containers));
        assert!(!is_blockable(&find(&db, completed), &containers));
        assert!(!is_blockable(&find(&db, parent), &containers));
        assert!(!is_blockable(&find(&db, linked), &containers));

        // And the planner agrees: it blocks exactly the blockable ones.
        plan(&db);
        for (id, expected) in [(plain, true), (child, true), (completed, false), (parent, false), (linked, false)] {
            assert_eq!(!blocks_of(&db, id).is_empty(), expected, "task {id}");
        }
    }

    #[test]
    fn clearing_auto_blocks_keeps_pinned_and_finished_blocks_and_task_days() {
        let db = db();
        let auto = task(&db, "Auto", 1, 60);
        let pinned = task(&db, "Pinned", 1, 60);
        let done = task(&db, "Done", 2, 60);
        task_blocks::insert_locked_block(&db.conn, pinned, local(1, 15, 0), local(1, 16, 0), 0).expect("pin");
        plan(&db);
        tasks::set_completed(&db.conn, done, true).expect("complete");
        assert_eq!(blocks_of(&db, auto).len(), 1);

        let removed = clear_auto_blocks(&db.conn, today()).expect("clear");

        assert_eq!(removed, 1, "only the one auto block");
        assert!(blocks_of(&db, auto).is_empty());
        assert_eq!(blocks_of(&db, pinned).len(), 1);
        assert_eq!(blocks_of(&db, done).len(), 1, "history is kept");
        assert!(find(&db, auto).assigned_date.is_some(), "the task keeps its day");
    }

    #[test]
    fn clearing_auto_blocks_does_not_touch_earlier_days() {
        let db = db();
        let t = task(&db, "Earlier", -1, 60);
        let block = task_blocks::insert_locked_block(&db.conn, t, local(-1, 9, 0), local(-1, 10, 0), 0).expect("pin");
        task_blocks::set_block_locked(&db.conn, block.id, false).expect("make it an auto block");

        clear_auto_blocks(&db.conn, today()).expect("clear");

        assert_eq!(blocks_of(&db, t).len(), 1, "yesterday is before `from_day`");
    }

    #[test]
    fn clearing_with_nothing_to_clear_is_a_no_op() {
        let db = db();
        assert_eq!(clear_auto_blocks(&db.conn, today()).expect("clear"), 0);
    }

    #[test]
    fn the_plan_times_command_declines_while_time_blocking_is_off() {
        let db = db();
        let t = task(&db, "Essay", 1, 60);

        let message = plan_times_command(&db.conn, today(), early());

        assert_eq!(message, "Turn on time blocking in Settings first.");
        assert!(blocks_of(&db, t).is_empty());
    }

    #[test]
    fn the_plan_times_command_plans_and_summarises_when_on() {
        let db = db();
        settings::set_time_blocking_enabled(&db.conn, true).expect("enable");
        let t = task(&db, "Essay", 1, 60);

        let message = plan_times_command(&db.conn, today(), early());

        assert_eq!(message, "Placed 1 block for 1 task.");
        assert_eq!(blocks_of(&db, t).len(), 1);
    }

    #[test]
    fn the_clear_command_reports_how_many_it_cleared() {
        let db = db();
        settings::set_time_blocking_enabled(&db.conn, true).expect("enable");
        task(&db, "A", 1, 60);
        task(&db, "B", 2, 60);
        plan(&db);

        assert_eq!(clear_auto_blocks_command(&db.conn, today()), "Cleared 2 auto blocks. Pinned blocks were kept.");
        assert_eq!(clear_auto_blocks_command(&db.conn, today()), "Cleared 0 auto blocks. Pinned blocks were kept.");
    }

    #[test]
    fn the_clear_command_uses_the_singular_for_one() {
        let db = db();
        settings::set_time_blocking_enabled(&db.conn, true).expect("enable");
        task(&db, "A", 1, 60);
        plan(&db);
        assert_eq!(clear_auto_blocks_command(&db.conn, today()), "Cleared 1 auto block. Pinned blocks were kept.");
    }

    // --- per-weekday hours (Q1) -----------------------------------------------------------
    // The tests' "today" is Wednesday 14 Oct 2026, so day(2) is Friday, day(4) Sunday, day(5) Monday.

    use crate::tasks::time_blocking::WorkDay;
    use chrono::Weekday;

    #[test]
    fn nothing_is_planned_on_a_day_off_and_an_unsplittable_task_there_is_reported() {
        let db = db();
        settings::set_weekday_override(&db.conn, Weekday::Sun, Some(WorkDay::Off)).expect("sunday off");
        settings::set_max_chunks(&db.conn, 1).expect("never split");
        let t = task(&db, "Sunday task", 4, 60);

        let report = plan(&db);

        assert!(blocks_of(&db, t).is_empty());
        assert_eq!(report.unplaced, vec![Unplaced { task_id: t, day: day(4) }]);
    }

    #[test]
    fn with_splitting_on_a_task_assigned_to_a_day_off_moves_on_to_the_next_working_day() {
        let db = db();
        settings::set_weekday_override(&db.conn, Weekday::Sun, Some(WorkDay::Off)).expect("sunday off");
        let t = task(&db, "Sunday task", 4, 60);

        plan(&db);

        assert_eq!(blocks_of(&db, t), vec![(at(5, 9, 0), at(5, 10, 0))], "Monday, not Sunday");
    }

    #[test]
    fn a_short_day_holds_less_and_the_rest_carries_on() {
        let db = db();
        // Friday is only 09:00-12:00.
        settings::set_weekday_override(&db.conn, Weekday::Fri, Some(WorkDay::Hours(9 * 60, 12 * 60))).expect("short friday");
        let first = task(&db, "First", 2, 120);
        let second = task(&db, "Second", 2, 120);

        plan(&db);

        assert_eq!(blocks_of(&db, first), vec![(at(2, 9, 0), at(2, 11, 0))]);
        let mut second_blocks = blocks_of(&db, second);
        second_blocks.sort();
        assert_eq!(second_blocks[0], (at(2, 11, 0), at(2, 12, 0)), "the last hour of Friday");
        assert_eq!(second_blocks[1].0.date(), day(3).and_hms_opt(0, 0, 0).unwrap().date(), "the rest on Saturday");
        for (_, end) in blocks_of(&db, first).iter().chain(second_blocks.iter()).filter(|(s, _)| s.date() == day(2)) {
            assert!(*end <= at(2, 12, 0), "nothing after Friday's closing time");
        }
    }

    #[test]
    fn capacity_is_zero_on_a_day_off_and_follows_a_days_own_hours() {
        let db = db();
        settings::set_weekday_override(&db.conn, Weekday::Sun, Some(WorkDay::Off)).expect("off");
        settings::set_weekday_override(&db.conn, Weekday::Fri, Some(WorkDay::Hours(9 * 60, 12 * 60))).expect("short");

        let cap = capacities(&db, early());

        assert_eq!(cap[&day(4)], 0, "Sunday");
        assert_eq!(cap[&day(2)], 3 * 60, "Friday");
        assert_eq!(cap[&day(1)], 12 * 60, "an ordinary day");
    }

    #[test]
    fn the_default_window_still_applies_to_days_without_an_override() {
        let db = db();
        settings::set_weekday_override(&db.conn, Weekday::Sun, Some(WorkDay::Off)).expect("off");
        settings::set_work_window_minutes(&db.conn, 13 * 60, 18 * 60).expect("afternoons");
        let t = task(&db, "Essay", 1, 60);

        plan(&db);

        assert_eq!(blocks_of(&db, t), vec![(at(1, 13, 0), at(1, 14, 0))]);
    }

    // --- settled blocks (in progress / done) ---------------------------------------------

    use crate::tasks::BlockStatus;

    fn settled_block(db: &Database, task_id: i64, start: (i64, u32, u32), end: (i64, u32, u32), status: BlockStatus) -> i64 {
        let block = task_blocks::insert_locked_block(
            &db.conn,
            task_id,
            local(start.0, start.1, start.2),
            local(end.0, end.1, end.2),
            0,
        )
        .expect("insert");
        task_blocks::set_block_locked(&db.conn, block.id, false).expect("unpin");
        task_blocks::set_block_status(&db.conn, block.id, status).expect("status");
        block.id
    }

    #[test]
    fn a_done_block_survives_a_re_plan_and_its_time_is_not_reused() {
        let db = db();
        let worked = task(&db, "Worked", 1, 60);
        let done = settled_block(&db, worked, (1, 9, 0), (1, 10, 0), BlockStatus::Done);
        let other = task(&db, "Other", 1, 60);
        // Finished and unrelated to the worked task: nothing here should land on 09:00-10:00.
        let _ = other;

        plan(&db);

        let kept = task_blocks::list_blocks_for_task(&db.conn, worked).expect("blocks");
        assert!(kept.iter().any(|b| b.id == done), "the record of work is kept");
        for task_id in [worked, other] {
            for (start, _) in blocks_of(&db, task_id).iter().filter(|(_, e)| *e != at(1, 10, 0)) {
                assert!(*start >= at(1, 10, 0), "nothing planned on top of worked time: {start}");
            }
        }
    }

    #[test]
    fn an_in_progress_block_is_kept_and_busy_too() {
        let db = db();
        let t = task(&db, "Running", 1, 60);
        let running = settled_block(&db, t, (1, 9, 0), (1, 10, 0), BlockStatus::InProgress);
        let other = task(&db, "Other", 1, 60);

        plan(&db);

        assert!(task_blocks::list_blocks_for_task(&db.conn, t).unwrap().iter().any(|b| b.id == running));
        assert_eq!(blocks_of(&db, other), vec![(at(1, 10, 0), at(1, 11, 0))]);
    }

    #[test]
    fn a_done_block_does_not_count_as_a_pinned_piece_nor_a_conflict() {
        let db = db();
        let t = task(&db, "Worked", 1, 60);
        settled_block(&db, t, (1, 9, 0), (1, 10, 0), BlockStatus::Done);
        event(&db, 1, 9, 0, 10, 0); // the synthesized session event sits on the same time

        let report = plan(&db);

        assert!(report.conflicts.is_empty(), "history never 'clashes'");
        // The task still needs its full estimate planned: worked time is not a pinned piece.
        assert_eq!(blocks_of(&db, t).len(), 2, "the done block plus a new planned one");
    }

    #[test]
    fn done_blocks_take_capacity_like_any_history() {
        let db = db();
        let t = task(&db, "Worked", 1, 60);
        settled_block(&db, t, (1, 9, 0), (1, 10, 0), BlockStatus::Done);
        assert_eq!(capacities(&db, early())[&day(1)], 11 * 60);
    }

    // --- drop_task / plan_drop ------------------------------------------

    const BOUNDS: (i64, i64) = (9 * 60, 21 * 60);

    fn drop_at(db: &Database, t: i64, offset: i64, h: i64, m: i64) -> DropOutcome {
        drop_task(&db.conn, t, day(offset), h * 60 + m, BOUNDS, today(), early()).expect("drop_task")
    }

    fn enable_blocking(db: &Database) {
        settings::set_time_blocking_enabled(&db.conn, true).expect("enable");
    }

    #[test]
    fn a_dropped_task_with_room_gets_one_pinned_block_of_its_full_length() {
        let db = db();
        let t = task(&db, "Essay", 1, 60);

        let outcome = drop_at(&db, t, 1, 14, 0);

        assert!(matches!(outcome, DropOutcome::Placed { left_over: 0, replanned: 0, .. }));
        assert_eq!(blocks_of(&db, t), vec![(at(1, 14, 0), at(1, 15, 0))]);
        let block = &task_blocks::list_blocks_for_task(&db.conn, t).unwrap()[0];
        assert!(block.locked, "a hand-placed block is pinned");
    }

    #[test]
    fn a_drop_that_runs_into_an_event_takes_the_space_and_plans_the_rest_elsewhere() {
        let db = db();
        enable_blocking(&db);
        event(&db, 1, 15, 0, 16, 0);
        let t = task(&db, "Revision", 1, 120);

        let outcome = drop_at(&db, t, 1, 14, 0);

        match outcome {
            DropOutcome::Placed { placement, left_over, replanned } => {
                assert_eq!(placement.minutes(), 60, "only the hour before the event was free there");
                assert_eq!(left_over, 60);
                assert_eq!(replanned, 1);
            }
            other => panic!("expected a placement, got {other:?}"),
        }
        let mut blocks = blocks_of(&db, t);
        blocks.sort();
        assert_eq!(blocks, vec![(at(1, 9, 0), at(1, 10, 0)), (at(1, 14, 0), at(1, 15, 0))]);
        // The estimate is left alone: the task is simply not finished in this slot.
        let task_row = tasks::list_tasks(&db.conn).unwrap().into_iter().find(|x| x.id == t).unwrap();
        assert_eq!(task_row.estimated_minutes, Some(120));
    }

    #[test]
    fn with_time_blocking_off_the_unfitted_rest_is_left_unscheduled() {
        let db = db();
        event(&db, 1, 15, 0, 16, 0);
        let t = task(&db, "Revision", 1, 120);

        let outcome = drop_at(&db, t, 1, 14, 0);

        assert!(matches!(outcome, DropOutcome::Placed { left_over: 60, replanned: 0, .. }));
        assert_eq!(blocks_of(&db, t), vec![(at(1, 14, 0), at(1, 15, 0))]);
    }

    #[test]
    fn dropping_a_planned_task_moves_it_instead_of_adding_a_second_block() {
        let db = db();
        let t = task(&db, "Reading", 1, 60);
        plan(&db);
        assert_eq!(blocks_of(&db, t), vec![(at(1, 9, 0), at(1, 10, 0))]);

        drop_at(&db, t, 1, 14, 0);

        assert_eq!(blocks_of(&db, t), vec![(at(1, 14, 0), at(1, 15, 0))]);
    }

    #[test]
    fn a_drop_never_lands_on_another_tasks_block() {
        let db = db();
        let other = task(&db, "Other", 1, 60);
        task_blocks::insert_locked_block(&db.conn, other, local(1, 14, 0), local(1, 15, 0), 0).unwrap();
        let t = task(&db, "Mine", 1, 60);

        drop_at(&db, t, 1, 14, 15);

        let mine = blocks_of(&db, t);
        assert_eq!(mine.len(), 1);
        assert!(mine[0].1 <= at(1, 14, 0) || mine[0].0 >= at(1, 15, 0), "clear of the other block: {mine:?}");
    }

    #[test]
    fn a_drop_with_no_room_changes_nothing_and_keeps_the_existing_plan() {
        let db = db();
        let t = task(&db, "Reading", 1, 60);
        plan(&db);
        event(&db, 1, 9, 0, 21, 0);

        let outcome = drop_at(&db, t, 1, 14, 0);

        assert_eq!(outcome, DropOutcome::NoRoom);
        assert_eq!(blocks_of(&db, t), vec![(at(1, 9, 0), at(1, 10, 0))], "the earlier plan survives");
    }

    #[test]
    fn a_task_whose_pinned_time_already_covers_its_estimate_is_not_placed_again() {
        let db = db();
        let t = task(&db, "Pinned", 1, 60);
        task_blocks::insert_locked_block(&db.conn, t, local(1, 15, 0), local(1, 16, 0), 0).unwrap();

        assert_eq!(drop_at(&db, t, 1, 11, 0), DropOutcome::NothingToPlace);
        assert_eq!(blocks_of(&db, t), vec![(at(1, 15, 0), at(1, 16, 0))]);
    }

    #[test]
    fn a_task_assigned_to_another_day_or_finished_is_not_placed() {
        let db = db();
        let t = task(&db, "Elsewhere", 2, 60);
        assert_eq!(drop_at(&db, t, 1, 11, 0), DropOutcome::NothingToPlace);

        let done = task(&db, "Done", 1, 60);
        tasks::set_completed(&db.conn, done, true).unwrap();
        assert_eq!(drop_at(&db, done, 1, 11, 0), DropOutcome::NothingToPlace);
        assert!(blocks_of(&db, t).is_empty() && blocks_of(&db, done).is_empty());
    }

    #[test]
    fn the_preview_plan_matches_what_the_drop_then_saves() {
        let db = db();
        event(&db, 1, 15, 0, 16, 0);
        let t = task(&db, "Revision", 1, 120);

        let planned = plan_drop(&db.conn, t, day(1), 14 * 60, BOUNDS).unwrap();
        let outcome = drop_at(&db, t, 1, 14, 0);

        let (DropPlan::Place(p), DropOutcome::Placed { placement, .. }) = (planned, outcome) else {
            panic!("both should place");
        };
        assert_eq!(p, placement);
    }

    // --- resizing a block keeps the estimate and plans the rest later ------

    #[test]
    fn shrinking_a_block_plans_the_rest_straight_after_it_and_keeps_the_estimate() {
        let db = db();
        enable_blocking(&db);
        let t = task(&db, "Essay", 1, 60);
        plan(&db);
        let id = task_blocks::list_blocks_for_task(&db.conn, t).unwrap()[0].id;

        let outcome = task_blocks::resize_block(&db.conn, id, local(1, 9, 0), local(1, 9, 30), today(), early()).unwrap();

        assert_eq!((outcome.remaining, outcome.replanned), (30, 1));
        let mut blocks = blocks_of(&db, t);
        blocks.sort();
        assert_eq!(blocks, vec![(at(1, 9, 0), at(1, 9, 30)), (at(1, 9, 30), at(1, 10, 0))]);
        let estimate = tasks::list_tasks(&db.conn).unwrap().into_iter().find(|x| x.id == t).unwrap().estimated_minutes;
        assert_eq!(estimate, Some(60), "the prediction stands");
    }

    #[test]
    fn when_the_day_has_no_room_after_a_shrunk_block_the_rest_goes_to_a_later_day() {
        let db = db();
        enable_blocking(&db);
        let t = task(&db, "Essay", 1, 60);
        plan(&db);
        let id = task_blocks::list_blocks_for_task(&db.conn, t).unwrap()[0].id;
        event(&db, 1, 9, 30, 21, 0);

        let outcome = task_blocks::resize_block(&db.conn, id, local(1, 9, 0), local(1, 9, 30), today(), early()).unwrap();

        assert_eq!(outcome.replanned, 1);
        let mut blocks = blocks_of(&db, t);
        blocks.sort();
        assert_eq!(blocks, vec![(at(1, 9, 0), at(1, 9, 30)), (at(2, 9, 0), at(2, 9, 30))]);
    }

    #[test]
    fn the_rest_is_never_planned_before_the_resized_block() {
        let db = db();
        enable_blocking(&db);
        let t = task(&db, "Essay", 1, 60);
        // The block sits in the afternoon; free time before it must not be used for the rest.
        task_blocks::insert_locked_block(&db.conn, t, local(1, 15, 0), local(1, 16, 0), 0).unwrap();
        let id = task_blocks::list_blocks_for_task(&db.conn, t).unwrap()[0].id;

        task_blocks::resize_block(&db.conn, id, local(1, 15, 0), local(1, 15, 30), today(), early()).unwrap();

        let others: Vec<_> = blocks_of(&db, t).into_iter().filter(|b| b.0 != at(1, 15, 0)).collect();
        assert_eq!(others, vec![(at(1, 15, 30), at(1, 16, 0))], "after the block, not at 9:00");
    }

    #[test]
    fn growing_a_block_past_the_estimate_drops_the_pieces_the_task_no_longer_needs() {
        let db = db();
        enable_blocking(&db);
        settings::set_split_over_minutes(&db.conn, 30).unwrap();
        let t = task(&db, "Essay", 1, 90);
        plan(&db);
        let mut pieces = task_blocks::list_blocks_for_task(&db.conn, t).unwrap();
        pieces.sort_by_key(|b| b.start_time);
        assert_eq!(pieces.len(), 2, "90 minutes is over the threshold: two pieces");

        let outcome = task_blocks::resize_block(&db.conn, pieces[0].id, local(1, 9, 0), local(1, 10, 30), today(), early()).unwrap();

        assert_eq!((outcome.remaining, outcome.replanned), (0, 0));
        assert_eq!(blocks_of(&db, t), vec![(at(1, 9, 0), at(1, 10, 30))]);
    }

    // --- tasks over the threshold are planned as several blocks ------------

    #[test]
    fn by_default_a_task_over_thirty_minutes_is_planned_as_several_blocks() {
        let db = Database::open_in_memory().unwrap();
        enable_blocking(&db);
        let t = task(&db, "Essay", 1, 90);
        plan(&db);
        let mut blocks = blocks_of(&db, t);
        blocks.sort();
        assert_eq!(blocks, vec![(at(1, 9, 0), at(1, 9, 45)), (at(1, 10, 0), at(1, 10, 45))]);
    }

    #[test]
    fn a_task_of_thirty_minutes_or_less_is_still_one_block() {
        let db = Database::open_in_memory().unwrap();
        enable_blocking(&db);
        let t = task(&db, "Quick", 1, 30);
        plan(&db);
        assert_eq!(blocks_of(&db, t), vec![(at(1, 9, 0), at(1, 9, 30))]);
    }

    #[test]
    fn the_split_threshold_and_block_lengths_are_settings() {
        let db = Database::open_in_memory().unwrap();
        enable_blocking(&db);
        settings::set_split_over_minutes(&db.conn, 120).unwrap();
        let whole = task(&db, "Whole", 1, 90);
        plan(&db);
        assert_eq!(blocks_of(&db, whole), vec![(at(1, 9, 0), at(1, 10, 30))], "not over 120, so one block");

        settings::set_split_over_minutes(&db.conn, 30).unwrap();
        settings::set_max_block_minutes(&db.conn, 30).unwrap();
        plan(&db);
        assert_eq!(blocks_of(&db, whole).len(), 3, "now cut into 30-minute blocks");
    }
}

