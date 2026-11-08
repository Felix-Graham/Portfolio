use chrono::{DateTime, Duration, Local, NaiveDate};
use rusqlite::{params, Connection, Result};

use crate::tasks::Task;

fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        completed: row.get::<_, i64>(3)? != 0,
        created_at: row.get(4)?,
        due_date: row.get(5)?,
        position: row.get(6)?,
        event_id: row.get(7)?,
        completed_at: row.get(8)?,
        subject_id: row.get(9)?,
        assigned_date: row.get(10)?,
        repeat_weekly: row.get::<_, i64>(11)? != 0,
        parent_task_id: row.get(12)?,
        estimated_minutes: row.get(13)?,
        repeat_through_list: row.get::<_, i64>(14)? != 0,
        repeat_list_base_title: row.get(15)?,
        repeat_list_topics: row.get(16)?,
        repeat_list_mode: row.get(17)?,
        repeat_list_state: row.get(18)?,
        repeat_interval_days: row.get(19)?,
        plan_topics_ahead: row.get::<_, i64>(20)? != 0,
        generated_from: row.get(21)?,
        auto_assigned: row.get::<_, i64>(22)? != 0,
    })
}

const SELECT_COLUMNS: &str = "id, title, description, completed, created_at, due_date, position, event_id, \
                               completed_at, subject_id, assigned_date, repeat_weekly, parent_task_id, \
                               estimated_minutes, repeat_through_list, repeat_list_base_title, \
                               repeat_list_topics, repeat_list_mode, repeat_list_state, repeat_interval_days, \
                               plan_topics_ahead, generated_from, auto_assigned";

pub fn list_tasks(conn: &Connection) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLUMNS} FROM tasks ORDER BY position ASC, id ASC"
    ))?;
    let tasks = stmt.query_map([], row_to_task)?.collect::<Result<Vec<_>>>()?;
    Ok(tasks)
}

// Used by this module's own tests, and now by `set_repeat_list`
// (needs the current row to decide whether a config change is a
// "fresh start" or not).
fn find_task(conn: &Connection, id: i64) -> Result<Option<Task>> {
    conn.query_row(&format!("SELECT {SELECT_COLUMNS} FROM tasks WHERE id = ?1"), params![id], row_to_task).map(Some).or_else(
        |err| if matches!(err, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(err) },
    )
}

/// Tasks attached to a specific revision session, in the same manual
/// order as the To Do list. Used by the Calendar's event blocks and
/// the event edit modal (Sprint 4).
pub fn list_tasks_for_event(conn: &Connection, event_id: i64) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLUMNS} FROM tasks WHERE event_id = ?1 ORDER BY position ASC, id ASC"
    ))?;
    let tasks = stmt.query_map(params![event_id], row_to_task)?.collect::<Result<Vec<_>>>()?;
    Ok(tasks)
}

/// Feature request: "Sub Tasks" — every child of `parent_id`, in
/// manual order.
pub fn list_children(conn: &Connection, parent_id: i64) -> Result<Vec<Task>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLUMNS} FROM tasks WHERE parent_task_id = ?1 ORDER BY position ASC, id ASC"
    ))?;
    let tasks = stmt.query_map(params![parent_id], row_to_task)?.collect::<Result<Vec<_>>>()?;
    Ok(tasks)
}

pub fn create_task(
    conn: &Connection,
    title: &str,
    due_date: Option<&str>,
    event_id: Option<i64>,
    subject_id: Option<i64>,
) -> Result<Task> {
    let created_at = chrono::Local::now().to_rfc3339();
    let next_position: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM tasks",
        [],
        |row| row.get(0),
    )?;

    conn.execute(
        "INSERT INTO tasks (title, description, completed, created_at, due_date, position, event_id, subject_id)
         VALUES (?1, '', 0, ?2, ?3, ?4, ?5, ?6)",
        params![title, created_at, due_date, next_position, event_id, subject_id],
    )?;

    Ok(Task {
        id: conn.last_insert_rowid(),
        title: title.to_owned(),
        description: String::new(),
        completed: false,
        created_at,
        due_date: due_date.map(str::to_owned),
        position: next_position,
        event_id,
        completed_at: None,
        subject_id,
        assigned_date: None,
        repeat_weekly: false,
        parent_task_id: None,
        estimated_minutes: None,
        repeat_through_list: false,
        repeat_list_base_title: None,
        repeat_list_topics: None,
        repeat_list_mode: None,
        repeat_list_state: None,
        // Matches the column's own `DEFAULT 7` (v23) — this task
        // doesn't repeat yet anyway (`repeat_weekly: false` above), so
        // the value is inert until someone actually turns repeating on
        // and picks a cadence via `set_repeat_interval_days`.
        repeat_interval_days: 7,
        plan_topics_ahead: false,
        generated_from: None,
        auto_assigned: false,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn update_task(
    conn: &Connection,
    id: i64,
    title: &str,
    description: &str,
    due_date: Option<&str>,
    event_id: Option<i64>,
    subject_id: Option<i64>,
    repeat_weekly: bool,
    parent_task_id: Option<i64>,
    estimated_minutes: Option<i64>,
) -> Result<()> {
    // Feature request: task "bars" — a changed estimate re-bases the bar's budget
    // (see `task_time::rebase_budget`); an edit that leaves it alone doesn't.
    let previous_estimate: Option<Option<i64>> = conn
        .query_row("SELECT estimated_minutes FROM tasks WHERE id = ?1", params![id], |r| r.get(0))
        .ok();
    conn.execute(
        "UPDATE tasks SET title = ?1, description = ?2, due_date = ?3, event_id = ?4, subject_id = ?5, \
         repeat_weekly = ?6, parent_task_id = ?7, estimated_minutes = ?8 WHERE id = ?9",
        params![
            title,
            description,
            due_date,
            event_id,
            subject_id,
            repeat_weekly,
            parent_task_id,
            estimated_minutes,
            id
        ],
    )?;
    if previous_estimate.is_some_and(|before| before != estimated_minutes) {
        super::task_time::rebase_budget(conn, id, estimated_minutes)?;
    }
    Ok(())
}

/// Sets how many days a `repeat_weekly` task's cadence actually is —
/// `1` for daily, `7` for weekly (feature request: "Tasks Repeating By
/// Day"). A fast-path setter of its own rather than a parameter on
/// `update_task` above — same precedent as `set_repeat_list` right
/// below and `subjects::set_color`: `update_task` already has close to
/// a dozen call sites across this file's own tests plus
/// `stats.rs`/`database::mod`, all of which stay completely unaffected
/// this way, exactly like adding "Tasks Repeating Through A List"
/// didn't touch `update_task` either.
pub fn set_repeat_interval_days(conn: &Connection, id: i64, days: i64) -> Result<()> {
    conn.execute("UPDATE tasks SET repeat_interval_days = ?1 WHERE id = ?2", params![days, id])?;
    Ok(())
}

/// True if `parent_id` has at least one child that isn't complete yet
/// (feature request: "Sub Tasks").
fn has_incomplete_children(conn: &Connection, parent_id: i64) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE parent_task_id = ?1 AND completed = 0",
        params![parent_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Marks a task complete or incomplete. Completing stamps
/// `completed_at` with the current time; un-completing clears it back
/// to `NULL`, so the Dashboard streak (Sprint 5) only ever reflects
/// tasks that are *currently* done.
///
/// One behavior layered on top for a feature request, living here
/// rather than in the UI, since every completion-toggle call site in
/// the app (Dashboard, To Do, Calendar, Revision Session overlay)
/// goes through this one function:
/// - "Sub Tasks": a task with any incomplete children can't be marked
///   complete — this call becomes a no-op rather than an error, since
///   every call site already just fires-and-forgets it; the UI
///   surfaces *why* separately (an "x/y subtasks" indicator).
///
/// "Repeating Tasks" completion used to spawn a brand new row here,
/// immediately — since fixed (see `reset_due_repeating_tasks`): a
/// completed repeating task now just stays completed, same as any
/// other task, until its own next due date actually arrives.
pub fn set_completed(conn: &Connection, id: i64, completed: bool) -> Result<()> {
    if completed {
        if has_incomplete_children(conn, id)? {
            return Ok(());
        }
        let now = chrono::Local::now().to_rfc3339();
        conn.execute("UPDATE tasks SET completed = 1, completed_at = ?1 WHERE id = ?2", params![now, id])?;
        // Time-blocking: ticking a task off brings its blocks into line with the
        // time actually spent (a block under way ends now, one still ahead goes).
        crate::database::queries::block_sessions::on_task_completed(conn, id, chrono::Local::now())?;
    } else {
        conn.execute("UPDATE tasks SET completed = 0, completed_at = NULL WHERE id = ?1", params![id])?;
    }
    Ok(())
}

/// How done a repeating task is, for the reset pass.
enum Completion {
    /// Not ticked.
    Open,
    /// Ticked, on this day (`None` if the timestamp is missing or
    /// unreadable, which counts as "long ago").
    DoneOn(Option<NaiveDate>),
}

/// The day a task was ticked, as a local date.
fn completed_day(task: &Task) -> Option<NaiveDate> {
    let completed_at = task.completed_at.as_deref()?;
    DateTime::parse_from_rfc3339(completed_at).ok().map(|dt| dt.with_timezone(&Local).date_naive())
}

/// A task counts as done if it is ticked — or if it is a parent whose
/// sections are all ticked. The second case keeps a repeating parent in
/// step with its sections: it can only be ticked once every section is,
/// but forgetting to tick it must not leave it stuck on last week's
/// date while its sections have all moved on.
fn completion(all: &[Task], task: &Task) -> Completion {
    if task.completed {
        return Completion::DoneOn(completed_day(task));
    }
    let children: Vec<&Task> = all.iter().filter(|t| t.parent_task_id == Some(task.id)).collect();
    if children.is_empty() || !children.iter().all(|c| c.completed) {
        return Completion::Open;
    }
    // The latest day any section was ticked; unreadable counts as old.
    let latest = children.iter().map(|c| completed_day(c)).collect::<Option<Vec<_>>>().and_then(|days| days.into_iter().max());
    Completion::DoneOn(latest)
}

/// Bug fix: "repeating events reoccur after marked complete." A
/// completed `repeat_weekly` task used to spawn a brand new,
/// not-yet-due row the instant it was completed
/// (`spawn_next_repeat_if_needed`, since removed) — so the *next*
/// week's occurrence was already sitting in the list well before it
/// was actually due. Now nothing happens at completion time; instead,
/// this runs once a day (see `AppState::run_cleanup`) and, for every
/// completed repeating task whose next due date (`due_date + 7 days`)
/// has arrived, resets that same row in place — uncompleting it and
/// advancing `due_date` — rather than creating a new one. A weekly
/// task is therefore always exactly one row, reused forever, that
/// simply goes quiet after completion and reappears exactly on its
/// next due date.
///
/// If more than one cycle has passed since it was last reset (the app
/// wasn't opened for a week or more), `due_date` is advanced in
/// 7-day steps until it lands on the most recent scheduled occurrence
/// that isn't in the future, rather than surfacing an occurrence
/// that's weeks stale.
///
/// Bug fix: "the event repeats but its deadline does not change ... a
/// task which is due every Monday is always late the next Mondays."
/// This pass used to skip any task that wasn't completed, so a
/// repeating task that was missed once kept its old `due_date`
/// forever. It now rolls an *incomplete* repeating task forward
/// exactly the same way as a completed one.
///
/// Bug fix: "the dates for repeating tasks do not change after a due
/// date has been reached", then "on Monday things SHOULD move unless
/// they were unticked at the end of Sunday. This goes for all
/// repeating tasks." The reset used to wait until the *next*
/// occurrence had arrived (`due_date` + one interval), so a task that
/// was done kept last Monday's date and tick for a further week. It
/// now depends on whether the task was ticked on an *earlier day*:
///
/// - ticked before today and due today or earlier → it resets to
///   unticked and moves to the first scheduled date after the one that
///   just ended that isn't in the past (a week on for a weekly task on
///   its due day, the next day for a daily one, just the next upcoming
///   date after a gap of several cycles);
/// - ticked today → untouched until tomorrow, so restarting the app
///   (this runs at launch) never un-ticks the morning's work;
/// - ticked ahead of its due date → untouched until it is due;
/// - not ticked → it does not move on its due day, since it didn't get
///   done; the missed occurrence is only dropped once the next one
///   arrives (see the "always late the next Mondays" fix above), so it
///   is never late for ever.
///
/// A parent whose sections are all ticked counts as ticked even if the
/// parent itself was never ticked, so it moves with its sections.
///
/// Either way, the reset also clears the task's `assigned_date` (and
/// `auto_assigned`): an assignment picked for last occurrence would
/// otherwise keep the new one off its own due day's lists.
///
/// Feature request: "Tasks Repeating Through A List" — a
/// `repeat_through_list` task also gets its `title` and
/// `repeat_list_state` advanced to the next topic in the same pass,
/// via `tasks::repeat_list::next_topic`, so a fresh topic appears at
/// exactly the same moment the task itself resets — never a week
/// where the task reappears but still shows last week's topic.
pub fn reset_due_repeating_tasks(conn: &Connection, today: NaiveDate) -> Result<usize> {
    let tasks = list_tasks(conn)?;
    let mut reset_count = 0;

    for task in &tasks {
        // Feature request: "Auto Assigning" — a template doesn't cycle
        // through its topics itself any more; its per-day tasks do.
        // Completed or not — see the bug-fix note above.
        if !task.repeat_weekly || task.plan_topics_ahead {
            continue;
        }
        let Some(due_date) = task.due_date.as_deref().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()) else {
            continue;
        };

        // Feature request: "Tasks Repeating By Day" — `interval_days`
        // is `1` for a daily task, `7` for weekly (the pre-existing,
        // only behavior before this request). `.max(1)` guards the
        // catch-up loops below against ever looping forever on a
        // corrupt/hand-edited `0` or negative value.
        let interval_days = Duration::days(task.repeat_interval_days.max(1));

        // Bug fix: "on Monday things SHOULD move unless they were
        // unticked at the end of Sunday. This goes for all repeating
        // tasks." Whether a task moves on its due day depends on
        // whether it was *done before today*:
        //
        // - ticked on an earlier day: its occurrence is finished, so
        //   as soon as it is due (the due day itself, or any day
        //   after) it resets to unticked and moves on to the next
        //   occurrence;
        // - ticked today: left exactly as it is until tomorrow, so
        //   restarting the app (this runs at launch) never un-ticks
        //   work you did this morning;
        // - not ticked: it did not get done, so it does not move on
        //   its due day — it stays as the outstanding task it is —
        //   and only when the next occurrence arrives is the missed
        //   one dropped and the task re-dated (the earlier "always
        //   late the next Mondays" fix).
        let next_due = match completion(&tasks, task) {
            Completion::DoneOn(Some(day)) if day >= today => continue,
            Completion::DoneOn(_) => {
                if due_date > today {
                    // Ticked early; not due yet. It resets when it is.
                    continue;
                }
                // The first scheduled date after the one just ended
                // that isn't in the past: next Monday for a weekly
                // task on its due day, tomorrow for a daily one, and
                // — after a gap of several cycles — just the next
                // upcoming date, never a stale or a skipped one.
                let mut next_due = due_date + interval_days;
                while next_due < today {
                    next_due += interval_days;
                }
                next_due
            }
            Completion::Open => {
                let mut next_due = due_date + interval_days;
                if next_due > today {
                    continue;
                }
                while next_due + interval_days <= today {
                    next_due += interval_days;
                }
                next_due
            }
        };
        let next_due_str = next_due.format("%Y-%m-%d").to_string();
        // How many occurrences this reset moves forward — 1 normally,
        // more if the app wasn't opened for a few cycles.
        let steps = (next_due - due_date).num_days() / interval_days.num_days();

        let topics = crate::tasks::repeat_list::parse_topics(task.repeat_list_topics.as_deref().unwrap_or(""));
        let advanced_topic = if task.repeat_through_list && !topics.is_empty() {
            let mode =
                task.repeat_list_mode.as_deref().and_then(crate::tasks::repeat_list::RepeatListMode::parse).unwrap_or_default();
            let seed =
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
            // Feature request: "tasks which loop through a list should
            // only show the parent task, renamed daily to that day's
            // topic". Advancing by `steps`, not once, keeps the title on
            // the topic that belongs to the day it now falls on even
            // after skipped days.
            crate::tasks::repeat_list::advance_topic(
                mode,
                &topics,
                task.repeat_list_state.as_deref().unwrap_or(""),
                steps,
                seed,
            )
        } else {
            None
        };

        match advanced_topic {
            Some((topic, new_state)) => {
                let base = task.repeat_list_base_title.as_deref().unwrap_or(&task.title);
                conn.execute(
                    "UPDATE tasks SET completed = 0, completed_at = NULL, due_date = ?1, title = ?2, \
                     repeat_list_state = ?3, assigned_date = NULL, auto_assigned = 0 WHERE id = ?4",
                    params![next_due_str, format!("{base}: {topic}"), new_state, task.id],
                )?;
            }
            None => {
                conn.execute(
                    "UPDATE tasks SET completed = 0, completed_at = NULL, due_date = ?1, \
                     assigned_date = NULL, auto_assigned = 0 WHERE id = ?2",
                    params![next_due_str, task.id],
                )?;
            }
        }
        // Feature request: "Time-blocking" — the reset just cleared the
        // task's `assigned_date`, so its blocks (a time on that day)
        // are stale; a weekly task is re-planned and re-blocked each cycle.
        crate::database::queries::task_blocks::delete_unfinished_blocks_for_task(conn, task.id)?;
        reset_count += 1;
    }

    Ok(reset_count)
}

/// Feature request: "Tasks which repeat and loop through a list of
/// topics should only show the parent task (the task which currently
/// spawns each topic task). The parent task should be renamed daily to
/// the corresponding topic for that day. This will remove clutter in
/// the task section."
///
/// "Plan each day's topic ahead" used to turn a topic-list task into a
/// hidden template and fill the To Do view and Calendar with one
/// generated task per day. That is gone: a topic-list task is always a
/// single task whose title is re-dated and renamed by
/// `reset_due_repeating_tasks`. This converts anything saved under the
/// old scheme — it clears the template flag, deletes the generated
/// tasks that hadn't been done (finished ones are cleaned up like any
/// other completed task), and restarts the list so the title shows a
/// topic again; the reset pass that follows moves it to today's. Safe
/// to run on every cleanup: it does nothing once no templates remain.
/// Returns how many templates were converted.
pub fn retire_topic_planning(conn: &Connection) -> Result<usize> {
    let tasks = list_tasks(conn)?;
    let mut converted = 0;
    for task in tasks.iter().filter(|t| t.plan_topics_ahead) {
        // Feature request: "Time-blocking" — blocks first: with foreign
        // keys enforced (see migration v27) a task can't be deleted
        // while a block still points at it.
        conn.execute(
            "DELETE FROM task_blocks WHERE task_id IN \
             (SELECT id FROM tasks WHERE generated_from = ?1 AND completed = 0)",
            params![task.id],
        )?;
        conn.execute(
            "DELETE FROM task_time_log WHERE task_id IN \
             (SELECT id FROM tasks WHERE generated_from = ?1 AND completed = 0)",
            params![task.id],
        )?;
        conn.execute("DELETE FROM tasks WHERE generated_from = ?1 AND completed = 0", params![task.id])?;
        conn.execute("UPDATE tasks SET plan_topics_ahead = 0, plan_signature = NULL WHERE id = ?1", params![task.id])?;

        let topics_text = task.repeat_list_topics.as_deref().unwrap_or("");
        let topics = crate::tasks::repeat_list::parse_topics(topics_text);
        if task.repeat_through_list && !topics.is_empty() {
            let mode =
                task.repeat_list_mode.as_deref().and_then(crate::tasks::repeat_list::RepeatListMode::parse).unwrap_or_default();
            let base = task.repeat_list_base_title.as_deref().unwrap_or(&task.title);
            let seed =
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
            if let Some((topic, state)) = crate::tasks::repeat_list::next_topic(mode, &topics, "", seed) {
                conn.execute(
                    "UPDATE tasks SET repeat_list_state = ?1, title = ?2 WHERE id = ?3",
                    params![state, format!("{base}: {topic}"), task.id],
                )?;
            }
        }
        converted += 1;
    }
    Ok(converted)
}

/// Saves a task's "Tasks Repeating Through A List" configuration.
/// `enabled = false` just turns it off (the rest of the config is
/// left in place, same as unchecking "This is a flashcard set" on a
/// note doesn't erase its content — turning it back on later picks up
/// where it left off rather than starting over).
///
/// When enabling it, or leaving it enabled: if this is a genuinely
/// *fresh* start — it wasn't already on, or `base_title`/`topics_text`
/// changed from what's stored — the very first topic is picked
/// immediately (via `tasks::repeat_list::next_topic` against empty
/// state) and `title` updated to show it right away, matching the
/// request's own example ("Week 1: French Vocab Task Topic {1}" shows
/// Topic 1 from the start, not a bare title waiting for its first
/// reset). If nothing meaningful changed — the person just resaved
/// the task for some unrelated edit — the current topic and its
/// position in the cycle are left alone; only the actual reset pass
/// (`reset_due_repeating_tasks`) advances them further after that.
pub fn set_repeat_list(
    conn: &Connection,
    id: i64,
    enabled: bool,
    base_title: &str,
    topics_text: &str,
    mode: crate::tasks::repeat_list::RepeatListMode,
) -> Result<()> {
    if !enabled {
        conn.execute("UPDATE tasks SET repeat_through_list = 0 WHERE id = ?1", params![id])?;
        return Ok(());
    }

    let existing = find_task(conn, id)?;
    let needs_fresh_start = match &existing {
        Some(t) => {
            !t.repeat_through_list
                || t.repeat_list_topics.as_deref() != Some(topics_text)
                || t.repeat_list_base_title.as_deref() != Some(base_title)
        }
        None => true,
    };

    let topics = crate::tasks::repeat_list::parse_topics(topics_text);
    if needs_fresh_start && !topics.is_empty() {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        if let Some((topic, state)) = crate::tasks::repeat_list::next_topic(mode, &topics, "", seed) {
            conn.execute(
                "UPDATE tasks SET repeat_through_list = 1, repeat_list_base_title = ?1, repeat_list_topics = ?2, \
                 repeat_list_mode = ?3, repeat_list_state = ?4, title = ?5 WHERE id = ?6",
                params![base_title, topics_text, mode.as_str(), state, format!("{base_title}: {topic}"), id],
            )?;
            return Ok(());
        }
    }

    conn.execute(
        "UPDATE tasks SET repeat_through_list = 1, repeat_list_base_title = ?1, repeat_list_topics = ?2, \
         repeat_list_mode = ?3 WHERE id = ?4",
        params![base_title, topics_text, mode.as_str(), id],
    )?;
    Ok(())
}

/// Attaches or detaches a task from a revision session without
/// touching anything else about it — used for quick "add existing
/// task to this session" style actions.
pub fn set_event(conn: &Connection, id: i64, event_id: Option<i64>) -> Result<()> {
    conn.execute("UPDATE tasks SET event_id = ?1 WHERE id = ?2", params![event_id, id])?;
    Ok(())
}

/// Feature request: "Time-blocking" — a block is a time *on the
/// task's assigned day*, so when that day changes (or is cleared) the
/// task's blocks no longer mean anything and are deleted; the next
/// "Plan times" run places the task afresh. Re-saving the same day
/// leaves them alone. Shared by the manual and the planner's setter so
/// the rule can't drift between them.
fn drop_blocks_if_day_changes(conn: &Connection, id: i64, new_date: Option<&str>) -> Result<()> {
    use rusqlite::OptionalExtension;
    let current: Option<String> = conn
        .query_row("SELECT assigned_date FROM tasks WHERE id = ?1", params![id], |row| row.get(0))
        .optional()?
        .flatten();
    if current.as_deref() != new_date {
        // Plans go; blocks in progress or done are history and stay.
        crate::database::queries::task_blocks::delete_unfinished_blocks_for_task(conn, id)?;
    }
    Ok(())
}

/// Feature request: "Task Assignment" — plans (or un-plans, with
/// `None`) a task for a specific day, independent of `due_date`.
/// Kept as its own fast-path call, not folded into `update_task`,
/// since assigning a task to a day is a one-click action (from the
/// Dashboard's Today's Tasks or the Calendar Day view) rather than
/// something done from the full edit modal.
///
/// Feature request: "Auto Assigning" — this is the *manual* path, so
/// it also clears `auto_assigned`: a day a person picked (or removed)
/// is theirs, and a later Auto-assign run must not move it. The
/// planner itself uses `set_auto_assignment` instead.
pub fn set_assigned_date(conn: &Connection, id: i64, assigned_date: Option<&str>) -> Result<()> {
    drop_blocks_if_day_changes(conn, id, assigned_date)?;
    conn.execute(
        "UPDATE tasks SET assigned_date = ?1, auto_assigned = 0 WHERE id = ?2",
        params![assigned_date, id],
    )?;
    Ok(())
}

/// Feature request: "Auto Assigning" — the planner's own way of
/// assigning a task to a day (or clearing it, with `None`), marking
/// it `auto_assigned` (or not, when cleared) so a later run may
/// re-plan it. Separate from `set_assigned_date` on purpose; see there.
pub fn set_auto_assignment(conn: &Connection, id: i64, assigned_date: Option<&str>) -> Result<()> {
    drop_blocks_if_day_changes(conn, id, assigned_date)?;
    conn.execute(
        "UPDATE tasks SET assigned_date = ?1, auto_assigned = ?2 WHERE id = ?3",
        params![assigned_date, assigned_date.is_some() as i64, id],
    )?;
    Ok(())
}

/// Persists just a repeat-list's cycle state — used by Auto-assign
/// after it draws topics for the stateful (shuffle / spaced) modes.
pub fn set_repeat_list_state(conn: &Connection, id: i64, state: &str) -> Result<()> {
    conn.execute("UPDATE tasks SET repeat_list_state = ?1 WHERE id = ?2", params![state, id])?;
    Ok(())
}

pub fn delete_task(conn: &Connection, id: i64) -> Result<()> {
    // Feature request: "Time-blocking" — foreign keys are off, so the
    // task's time blocks (and its generated tasks' blocks, below) go
    // by hand, before the rows they point at.
    conn.execute(
        "DELETE FROM task_blocks WHERE task_id = ?1 \
         OR task_id IN (SELECT id FROM tasks WHERE generated_from = ?1)",
        params![id],
    )?;
    // Feature request: task "bars" — the time logged against it (and against its generated tasks) goes too.
    conn.execute(
        "DELETE FROM task_time_log WHERE task_id = ?1 OR task_id IN (SELECT id FROM tasks WHERE generated_from = ?1)",
        params![id],
    )?;
    conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
    // Feature request: "Auto Assigning" — foreign keys are off, so a
    // template's generated per-day tasks are removed by hand along
    // with it (a no-op for any ordinary task).
    conn.execute("DELETE FROM tasks WHERE generated_from = ?1", params![id])?;
    Ok(())
}

/// Swaps the `position` of two tasks, used for the To Do view's
/// move-up / move-down ordering controls.
pub fn swap_positions(conn: &Connection, id_a: i64, position_a: i64, id_b: i64, position_b: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("UPDATE tasks SET position = ?1 WHERE id = ?2", params![position_b, id_a])?;
    tx.execute("UPDATE tasks SET position = ?1 WHERE id = ?2", params![position_a, id_b])?;
    tx.commit()
}

/// Every distinct local calendar date on which a task was completed.
/// Feeds `tasks::streak::current_streak` (Sprint 5's "basic streak").
/// Every distinct local calendar date on which a task was completed —
/// unioning still-live tasks' `completed_at` with the durable
/// `completion_log` (see migration v8). The log exists specifically
/// so this stays accurate after `delete_past_completed_tasks` removes
/// the underlying task rows; without it, the streak would quietly
/// lose history every time a completed task got cleaned up.
pub fn list_completed_dates(conn: &Connection) -> Result<Vec<NaiveDate>> {
    let mut stmt = conn.prepare("SELECT completed_at FROM tasks WHERE completed_at IS NOT NULL")?;
    let raw = stmt.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>>>()?;

    let mut dates: Vec<NaiveDate> = raw
        .iter()
        .filter_map(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Local).date_naive())
        .collect();

    dates.extend(list_completion_log_dates(conn)?);
    Ok(dates)
}

/// Appends a date to the durable completion log. Called by
/// `delete_past_completed_tasks` right before removing a completed
/// task, so its contribution to the streak survives the deletion.
pub fn log_completion(conn: &Connection, date: NaiveDate) -> Result<()> {
    conn.execute("INSERT INTO completion_log (date) VALUES (?1)", params![date.format("%Y-%m-%d").to_string()])?;
    Ok(())
}

fn list_completion_log_dates(conn: &Connection) -> Result<Vec<NaiveDate>> {
    let mut stmt = conn.prepare("SELECT date FROM completion_log")?;
    let dates = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .filter_map(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
        .collect();
    Ok(dates)
}

/// Deletes completed tasks whose completion date is before `today` —
/// "to do items should be deleted at the end of the day they were
/// fulfilled" (feedback pass). Incomplete tasks are never touched,
/// however old ("old ones not fulfilled carried forwards"). Each
/// task's completion date is logged durably first, so the Dashboard
/// streak keeps remembering it after the row is gone. Returns how
/// many were deleted, mainly for tests.
/// Deletes completed tasks whose completion date is in the past,
/// logging each one's completion date first so it still counts toward
/// the Dashboard streak after the row is gone. `repeat_weekly` tasks
/// are exempt regardless of age — bug fix: "repeating events reoccur
/// after marked complete" made them perpetual, reused rows
/// (`reset_due_repeating_tasks` uncompletes and re-dates the same row
/// in place rather than this function ever removing it).
pub fn delete_past_completed_tasks(conn: &Connection, today: NaiveDate) -> Result<usize> {
    let tasks = list_tasks(conn)?;
    let mut deleted = 0;
    for task in tasks {
        if !task.completed || task.repeat_weekly {
            continue;
        }
        let Some(completed_at) = &task.completed_at else { continue };
        let Ok(completed_date) = DateTime::parse_from_rfc3339(completed_at).map(|dt| dt.with_timezone(&Local).date_naive())
        else {
            continue;
        };
        if completed_date < today {
            log_completion(conn, completed_date)?;
            delete_task(conn, task.id)?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    #[test]
    fn completing_a_task_with_incomplete_children_is_a_no_op() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let parent = create_task(&db.conn, "Parent", None, None, None).expect("create parent");
        let child = create_task(&db.conn, "Child", None, None, None).expect("create child");
        update_task(&db.conn, child.id, "Child", "", None, None, None, false, Some(parent.id), None)
            .expect("attach child");

        set_completed(&db.conn, parent.id, true).expect("attempt complete");
        let reloaded = find_task(&db.conn, parent.id).expect("find").expect("exists");
        assert!(!reloaded.completed, "parent must not complete while a child is incomplete");
    }

    #[test]
    fn completing_a_task_with_all_children_complete_succeeds() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let parent = create_task(&db.conn, "Parent", None, None, None).expect("create parent");
        let child = create_task(&db.conn, "Child", None, None, None).expect("create child");
        update_task(&db.conn, child.id, "Child", "", None, None, None, false, Some(parent.id), None)
            .expect("attach child");

        set_completed(&db.conn, child.id, true).expect("complete child");
        set_completed(&db.conn, parent.id, true).expect("complete parent");

        let reloaded = find_task(&db.conn, parent.id).expect("find").expect("exists");
        assert!(reloaded.completed);
    }

    #[test]
    fn completing_a_repeating_task_does_not_spawn_a_new_row_or_change_its_due_date() {
        // Bug fix: "repeating events reoccur after marked complete" —
        // completion alone must never create a second row, or move
        // the due date, ahead of time.
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "Water the plants", Some("2026-09-01"), None, None).expect("create");
        update_task(&db.conn, task.id, "Water the plants", "", Some("2026-09-01"), None, None, true, None, None)
            .expect("mark repeating");

        set_completed(&db.conn, task.id, true).expect("complete");

        let all = list_tasks(&db.conn).expect("list");
        assert_eq!(all.len(), 1, "completing a repeating task must not spawn a new row");
        assert!(all[0].completed);
        assert_eq!(all[0].due_date.as_deref(), Some("2026-09-01"), "due date must not move until actually reset");
    }

    #[test]
    fn completing_a_non_repeating_task_spawns_nothing() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "One-off task", None, None, None).expect("create");
        set_completed(&db.conn, task.id, true).expect("complete");
        assert_eq!(list_tasks(&db.conn).expect("list").len(), 1);
    }

    /// Ticks a task as if on `day` (`YYYY-MM-DD`): `set_completed`
    /// stamps the real clock, and the reset pass now depends on *which
    /// day* a task was ticked.
    fn complete_on(db: &Database, id: i64, day: &str) {
        set_completed(&db.conn, id, true).expect("complete");
        db.conn
            .execute("UPDATE tasks SET completed_at = ?1 WHERE id = ?2", params![format!("{day}T12:00:00+00:00"), id])
            .expect("backdate completion");
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn due_of(db: &Database, id: i64) -> Option<String> {
        find_task(&db.conn, id).expect("find").expect("exists").due_date
    }

    #[test]
    fn a_task_ticked_on_an_earlier_day_resets_on_its_due_day() {
        // "On Monday things SHOULD move unless they were unticked at
        // the end of Sunday." Due Monday 2026-09-07, ticked Sunday.
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "Water the plants", Some("2026-09-07"), None, None).expect("create");
        update_task(&db.conn, task.id, "Water the plants", "", Some("2026-09-07"), None, None, true, None, None)
            .expect("mark repeating");
        complete_on(&db, task.id, "2026-09-06");

        let reset_count = reset_due_repeating_tasks(&db.conn, day(2026, 9, 7)).expect("reset pass");

        assert_eq!(reset_count, 1);
        let reloaded = find_task(&db.conn, task.id).expect("find").expect("exists");
        assert!(!reloaded.completed, "unticked again on its due day");
        assert!(reloaded.completed_at.is_none());
        assert_eq!(reloaded.due_date.as_deref(), Some("2026-09-14"), "moved on by exactly one week");
        assert!(reloaded.repeat_weekly, "still repeats");
        // Running it again the same day changes nothing.
        assert_eq!(reset_due_repeating_tasks(&db.conn, day(2026, 9, 7)).expect("again"), 0);
    }

    #[test]
    fn a_task_ticked_today_is_left_alone_until_tomorrow() {
        // The pass runs at every launch: restarting the app on Monday
        // afternoon must not un-tick what was done on Monday morning.
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "Water the plants", Some("2026-09-07"), None, None).expect("create");
        update_task(&db.conn, task.id, "Water the plants", "", Some("2026-09-07"), None, None, true, None, None)
            .expect("mark repeating");
        complete_on(&db, task.id, "2026-09-07");

        assert_eq!(reset_due_repeating_tasks(&db.conn, day(2026, 9, 7)).expect("monday"), 0);
        let still = find_task(&db.conn, task.id).expect("find").expect("exists");
        assert!(still.completed);
        assert_eq!(still.due_date.as_deref(), Some("2026-09-07"));

        // Tuesday it was ticked on an earlier day: it moves on.
        assert_eq!(reset_due_repeating_tasks(&db.conn, day(2026, 9, 8)).expect("tuesday"), 1);
        assert_eq!(due_of(&db, task.id).as_deref(), Some("2026-09-14"));
    }

    #[test]
    fn a_task_unticked_at_the_end_of_sunday_does_not_move_on_monday() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let id = weekly_task(&db, "Weekly review", "2026-09-07");

        // Monday and the days after: it didn't get done, so it stays
        // as the outstanding task it is.
        for d in 7..=13 {
            assert_eq!(reset_due_repeating_tasks(&db.conn, day(2026, 9, d)).expect("reset pass"), 0, "09-{d}");
            assert_eq!(due_of(&db, id).as_deref(), Some("2026-09-07"), "09-{d}");
        }
        // Only when the next occurrence arrives is the missed one
        // dropped (so it isn't late for ever).
        assert_eq!(reset_due_repeating_tasks(&db.conn, day(2026, 9, 14)).expect("next monday"), 1);
        assert_eq!(due_of(&db, id).as_deref(), Some("2026-09-14"));
    }

    #[test]
    fn a_task_ticked_before_it_is_due_waits_for_its_due_day() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let id = weekly_task(&db, "Weekly review", "2026-09-14");
        complete_on(&db, id, "2026-09-09"); // done on Wednesday, due Monday

        assert_eq!(reset_due_repeating_tasks(&db.conn, day(2026, 9, 10)).expect("thursday"), 0);
        assert!(find_task(&db.conn, id).expect("find").expect("exists").completed, "stays ticked until due");
        assert_eq!(reset_due_repeating_tasks(&db.conn, day(2026, 9, 13)).expect("sunday"), 0);

        assert_eq!(reset_due_repeating_tasks(&db.conn, day(2026, 9, 14)).expect("monday"), 1);
        assert_eq!(due_of(&db, id).as_deref(), Some("2026-09-21"));
    }

    #[test]
    fn a_daily_task_ticked_yesterday_comes_back_today() {
        // The same rule for all repeating tasks, daily included.
        let db = Database::open_in_memory().expect("open in-memory db");
        let id = weekly_task(&db, "Take vitamins", "2026-09-01");
        set_repeat_interval_days(&db.conn, id, 1).expect("mark daily");
        complete_on(&db, id, "2026-09-01");

        // Ticked today: stays ticked.
        assert_eq!(reset_due_repeating_tasks(&db.conn, day(2026, 9, 1)).expect("same day"), 0);
        // The next day: back, due that day.
        assert_eq!(reset_due_repeating_tasks(&db.conn, day(2026, 9, 2)).expect("next day"), 1);
        let reloaded = find_task(&db.conn, id).expect("find").expect("exists");
        assert!(!reloaded.completed);
        assert_eq!(reloaded.due_date.as_deref(), Some("2026-09-02"));
        assert_eq!(reloaded.repeat_interval_days, 1, "still daily");
    }

    #[test]
    fn new_repeating_task_defaults_to_a_weekly_interval() {
        // Every task predating "Tasks Repeating By Day" behaved as
        // weekly — the column's own `DEFAULT 7` (migration v23) is
        // what keeps that true without needing to touch existing rows.
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "Water the plants", Some("2026-09-01"), None, None).expect("create");
        assert_eq!(task.repeat_interval_days, 7);
    }

    #[test]
    fn set_repeat_interval_days_persists_and_is_read_back() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "Take vitamins", Some("2026-09-01"), None, None).expect("create");
        set_repeat_interval_days(&db.conn, task.id, 1).expect("set interval");

        let reloaded = find_task(&db.conn, task.id).expect("find").expect("exists");
        assert_eq!(reloaded.repeat_interval_days, 1);
    }

    #[test]
    fn a_stale_daily_task_catches_up_one_day_at_a_time_not_one_week_at_a_time() {
        // The multi-cycle catch-up loop (the app wasn't opened for a
        // while) must use the task's own interval, not a hardcoded
        // week, or a daily task left alone for 3 days would jump to
        // some date a week+ in the future instead of landing on the
        // most recent day that isn't in the future.
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "Take vitamins", Some("2026-09-01"), None, None).expect("create");
        update_task(&db.conn, task.id, "Take vitamins", "", Some("2026-09-01"), None, None, true, None, None)
            .expect("mark repeating");
        set_repeat_interval_days(&db.conn, task.id, 1).expect("mark daily");
        complete_on(&db, task.id, "2026-09-01");

        reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 4).unwrap()).expect("reset pass");

        let reloaded = find_task(&db.conn, task.id).expect("find").expect("exists");
        assert_eq!(reloaded.due_date.as_deref(), Some("2026-09-04"), "lands on today, not a week past it");
    }

    #[test]
    fn set_repeat_list_picks_the_first_topic_immediately() {
        // Feature request: "Tasks Repeating Through A List" — the
        // very first save already shows Topic 1, per the request's
        // own example ("Week 1: ... Topic {1}"), not a bare title
        // waiting for the first reset.
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "French Vocab", Some("2026-09-01"), None, None).expect("create");
        update_task(&db.conn, task.id, "French Vocab", "", Some("2026-09-01"), None, None, true, None, None)
            .expect("mark repeating");

        set_repeat_list(
            &db.conn,
            task.id,
            true,
            "French Vocab",
            "Topic 1\nTopic 2\nTopic 3",
            crate::tasks::repeat_list::RepeatListMode::Sequential,
        )
        .expect("set repeat list");

        let reloaded = find_task(&db.conn, task.id).expect("find").expect("exists");
        assert_eq!(reloaded.title, "French Vocab: Topic 1");
        assert!(reloaded.repeat_through_list);
        assert_eq!(reloaded.repeat_list_base_title.as_deref(), Some("French Vocab"));
        assert_eq!(reloaded.repeat_list_state.as_deref(), Some("1"));
    }

    #[test]
    fn set_repeat_list_resaving_unchanged_config_does_not_disturb_the_current_topic() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "French Vocab", Some("2026-09-01"), None, None).expect("create");
        update_task(&db.conn, task.id, "French Vocab", "", Some("2026-09-01"), None, None, true, None, None)
            .expect("mark repeating");
        set_repeat_list(
            &db.conn,
            task.id,
            true,
            "French Vocab",
            "Topic 1\nTopic 2",
            crate::tasks::repeat_list::RepeatListMode::Sequential,
        )
        .expect("first save");

        // Simulate having already advanced once (as reset_due_repeating_tasks would).
        db.conn.execute("UPDATE tasks SET repeat_list_state = '1', title = 'French Vocab: Topic 2' WHERE id = ?1", params![task.id]).expect("simulate advance");

        // Re-saving the exact same config (e.g. editing an unrelated
        // field) must not reset back to Topic 1.
        set_repeat_list(
            &db.conn,
            task.id,
            true,
            "French Vocab",
            "Topic 1\nTopic 2",
            crate::tasks::repeat_list::RepeatListMode::Sequential,
        )
        .expect("resave");

        let reloaded = find_task(&db.conn, task.id).expect("find").expect("exists");
        assert_eq!(reloaded.title, "French Vocab: Topic 2", "unrelated resave must not disturb the current topic");
    }

    #[test]
    fn set_repeat_list_changing_the_topics_starts_over() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "French Vocab", Some("2026-09-01"), None, None).expect("create");
        update_task(&db.conn, task.id, "French Vocab", "", Some("2026-09-01"), None, None, true, None, None)
            .expect("mark repeating");
        set_repeat_list(
            &db.conn,
            task.id,
            true,
            "French Vocab",
            "Topic 1\nTopic 2",
            crate::tasks::repeat_list::RepeatListMode::Sequential,
        )
        .expect("first save");
        db.conn.execute("UPDATE tasks SET repeat_list_state = '1', title = 'French Vocab: Topic 2' WHERE id = ?1", params![task.id]).expect("simulate advance");

        set_repeat_list(
            &db.conn,
            task.id,
            true,
            "French Vocab",
            "New Topic A\nNew Topic B",
            crate::tasks::repeat_list::RepeatListMode::Sequential,
        )
        .expect("changed topics");

        let reloaded = find_task(&db.conn, task.id).expect("find").expect("exists");
        assert_eq!(reloaded.title, "French Vocab: New Topic A", "a changed topic list starts over at the top");
    }

    #[test]
    fn set_repeat_list_disabled_leaves_stored_config_in_place() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "French Vocab", Some("2026-09-01"), None, None).expect("create");
        set_repeat_list(
            &db.conn,
            task.id,
            true,
            "French Vocab",
            "Topic 1\nTopic 2",
            crate::tasks::repeat_list::RepeatListMode::Sequential,
        )
        .expect("enable");

        set_repeat_list(&db.conn, task.id, false, "French Vocab", "Topic 1\nTopic 2", crate::tasks::repeat_list::RepeatListMode::Sequential)
            .expect("disable");

        let reloaded = find_task(&db.conn, task.id).expect("find").expect("exists");
        assert!(!reloaded.repeat_through_list);
        assert_eq!(reloaded.repeat_list_topics.as_deref(), Some("Topic 1\nTopic 2"), "disabling keeps the config for later");
    }

    #[test]
    fn resetting_a_repeat_list_task_advances_both_the_title_and_the_due_date() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "French Vocab", Some("2026-09-01"), None, None).expect("create");
        update_task(&db.conn, task.id, "French Vocab", "", Some("2026-09-01"), None, None, true, None, None)
            .expect("mark repeating");
        set_repeat_list(
            &db.conn,
            task.id,
            true,
            "French Vocab",
            "Topic 1\nTopic 2\nTopic 3",
            crate::tasks::repeat_list::RepeatListMode::Sequential,
        )
        .expect("set repeat list");
        complete_on(&db, task.id, "2026-09-01");

        let reset_count = reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 8).unwrap()).expect("reset");

        assert_eq!(reset_count, 1);
        let reloaded = find_task(&db.conn, task.id).expect("find").expect("exists");
        assert!(!reloaded.completed);
        assert_eq!(reloaded.due_date.as_deref(), Some("2026-09-08"));
        assert_eq!(reloaded.title, "French Vocab: Topic 2", "advances to the next topic in the same pass");
    }

    #[test]
    fn resetting_a_repeat_weekly_task_without_repeat_list_leaves_its_title_alone() {
        // A plain repeating task (no list configured) must be
        // completely unaffected by any of this — its title never
        // changes, matching the pre-existing behavior this feature is
        // additive to.
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "Water the plants", Some("2026-09-01"), None, None).expect("create");
        update_task(&db.conn, task.id, "Water the plants", "", Some("2026-09-01"), None, None, true, None, None)
            .expect("mark repeating");
        complete_on(&db, task.id, "2026-09-01");

        reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 8).unwrap()).expect("reset");

        let reloaded = find_task(&db.conn, task.id).expect("find").expect("exists");
        assert_eq!(reloaded.title, "Water the plants");
    }

    #[test]
    fn a_completed_repeating_task_left_unopened_for_several_cycles_lands_on_the_next_occurrence() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "Water the plants", Some("2026-09-01"), None, None).expect("create");
        update_task(&db.conn, task.id, "Water the plants", "", Some("2026-09-01"), None, None, true, None, None)
            .expect("mark repeating");
        complete_on(&db, task.id, "2026-09-01");

        // Three weeks later — 09-08, 09-15, and 09-22 have all come
        // and gone since the app was last opened.
        let reset_count = reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 22).unwrap())
            .expect("reset pass");

        assert_eq!(reset_count, 1, "a multi-week gap is still exactly one reset, not one per missed week");
        let reloaded = find_task(&db.conn, task.id).expect("find").expect("exists");
        assert!(!reloaded.completed);
        assert_eq!(reloaded.due_date.as_deref(), Some("2026-09-22"), "09-22 is itself a scheduled date, so it is due today");
    }

    #[test]
    fn reset_due_repeating_tasks_ignores_non_repeating_tasks() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let far_future = NaiveDate::from_ymd_opt(2027, 1, 1).unwrap();

        let completed_one_off = create_task(&db.conn, "One-off", Some("2026-09-01"), None, None).expect("create");
        set_completed(&db.conn, completed_one_off.id, true).expect("complete");
        let overdue_one_off = create_task(&db.conn, "Late one-off", Some("2026-09-01"), None, None).expect("create");

        let reset_count = reset_due_repeating_tasks(&db.conn, far_future).expect("reset pass");

        assert_eq!(reset_count, 0);
        assert!(find_task(&db.conn, completed_one_off.id).expect("find").expect("exists").completed);
        let late = find_task(&db.conn, overdue_one_off.id).expect("find").expect("exists");
        assert_eq!(late.due_date.as_deref(), Some("2026-09-01"), "a non-repeating task's deadline never rolls");
    }

    fn weekly_task(db: &Database, title: &str, due: &str) -> i64 {
        let task = create_task(&db.conn, title, Some(due), None, None).expect("create");
        update_task(&db.conn, task.id, title, "", Some(due), None, None, true, None, None).expect("mark repeating");
        task.id
    }

    #[test]
    fn an_incomplete_weekly_task_rolls_to_the_next_occurrence_instead_of_staying_late() {
        // The reported bug: due Monday 2026-09-07, never completed.
        let db = Database::open_in_memory().expect("open in-memory db");
        let id = weekly_task(&db, "Weekly review", "2026-09-07");

        // The next Monday, 09-14, it is due today — not a week late.
        let reset_count =
            reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 14).unwrap()).expect("reset pass");

        assert_eq!(reset_count, 1);
        let reloaded = find_task(&db.conn, id).expect("find").expect("exists");
        assert_eq!(reloaded.due_date.as_deref(), Some("2026-09-14"));
        assert!(!reloaded.completed, "a missed occurrence is dropped, not marked done");
    }

    #[test]
    fn an_incomplete_weekly_task_not_yet_due_is_untouched() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let id = weekly_task(&db, "Weekly review", "2026-09-14");
        let reset_count =
            reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 10).unwrap()).expect("reset pass");
        assert_eq!(reset_count, 0);
        assert_eq!(find_task(&db.conn, id).expect("find").expect("exists").due_date.as_deref(), Some("2026-09-14"));
    }

    #[test]
    fn an_incomplete_weekly_task_missed_for_several_weeks_lands_on_the_most_recent_occurrence() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let id = weekly_task(&db, "Weekly review", "2026-09-01");

        reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 24).unwrap()).expect("reset pass");

        // 09-08, 09-15 and 09-22 have all arrived; 09-22 is the latest.
        assert_eq!(find_task(&db.conn, id).expect("find").expect("exists").due_date.as_deref(), Some("2026-09-22"));
    }

    #[test]
    fn an_incomplete_daily_task_is_never_late_for_more_than_a_day() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let id = weekly_task(&db, "Daily", "2026-09-01");
        set_repeat_interval_days(&db.conn, id, 1).expect("daily");

        reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 2).unwrap()).expect("reset pass");
        assert_eq!(find_task(&db.conn, id).expect("find").expect("exists").due_date.as_deref(), Some("2026-09-02"));
    }

    #[test]
    fn rolling_an_incomplete_list_task_forward_advances_its_topic() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let id = weekly_task(&db, "Vocab", "2026-09-07");
        set_repeat_list(&db.conn, id, true, "Vocab", "A\nB\nC", crate::tasks::repeat_list::RepeatListMode::Sequential)
            .expect("list");
        let before = find_task(&db.conn, id).expect("find").expect("exists").title;
        assert_eq!(before, "Vocab: A");

        reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 14).unwrap()).expect("reset pass");

        let after = find_task(&db.conn, id).expect("find").expect("exists");
        assert_eq!(after.title, "Vocab: B", "a new occurrence gets the next topic, even if the last was missed");
        assert_eq!(after.due_date.as_deref(), Some("2026-09-14"));
    }

    #[test]
    fn a_reset_clears_a_stale_assigned_date_whether_or_not_the_task_was_completed() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let incomplete = weekly_task(&db, "Incomplete", "2026-09-07");
        set_assigned_date(&db.conn, incomplete, Some("2026-09-09")).expect("assign");
        let completed = weekly_task(&db, "Completed", "2026-09-07");
        set_auto_assignment(&db.conn, completed, Some("2026-09-09")).expect("auto assign");
        complete_on(&db, completed, "2026-09-09");

        reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 14).unwrap()).expect("reset pass");

        for id in [incomplete, completed] {
            let task = find_task(&db.conn, id).expect("find").expect("exists");
            assert_eq!(task.assigned_date, None);
            assert!(!task.auto_assigned);
        }
    }

    #[test]
    fn a_weekly_parent_with_weekly_subtasks_resets_as_a_unit_each_monday() {
        // A "multipart task due every Monday": a weekly parent with
        // three weekly sections, all due the same Monday (2026-09-07).
        let db = Database::open_in_memory().expect("open in-memory db");
        let parent = weekly_task(&db, "Weekly review", "2026-09-07");
        let sections: Vec<i64> = (1..=3)
            .map(|n| {
                let id = weekly_task(&db, &format!("Part {n}"), "2026-09-07");
                update_task(&db.conn, id, &format!("Part {n}"), "", Some("2026-09-07"), None, None, true, Some(parent), None)
                    .expect("attach as subtask");
                id
            })
            .collect();

        // The parent can't be completed until every section is.
        set_completed(&db.conn, parent, true).expect("attempt");
        assert!(!find_task(&db.conn, parent).expect("find").expect("exists").completed);
        for id in &sections {
            complete_on(&db, *id, "2026-09-06");
        }
        complete_on(&db, parent, "2026-09-06");
        assert!(find_task(&db.conn, parent).expect("find").expect("exists").completed);

        // Cleanup must not delete the completed sections. (`completed_at`
        // is the real "now", so a far-future `today` stands in for
        // "the day after".)
        let deleted = delete_past_completed_tasks(&db.conn, NaiveDate::from_ymd_opt(2099, 1, 1).unwrap()).expect("cleanup");
        assert_eq!(deleted, 0, "repeating parents and sections are exempt from cleanup");

        // ...and the next Monday everything resets together.
        reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 14).unwrap()).expect("reset pass");
        for id in std::iter::once(parent).chain(sections) {
            let task = find_task(&db.conn, id).expect("find").expect("exists");
            assert!(!task.completed);
            assert_eq!(task.due_date.as_deref(), Some("2026-09-14"));
        }
        assert_eq!(list_children(&db.conn, parent).expect("children").len(), 3);
    }

    fn maths_assignment(db: &Database) -> (i64, Vec<i64>) {
        // The "Maths Assignment" shape: due every Monday (2026-09-07),
        // with Sections A-C that repeat with it.
        let parent = weekly_task(db, "Maths Assignment", "2026-09-07");
        let sections = ["Section A", "Section B", "Section C"]
            .iter()
            .map(|title| {
                let id = weekly_task(db, title, "2026-09-07");
                update_task(&db.conn, id, title, "", Some("2026-09-07"), None, None, true, Some(parent), None)
                    .expect("attach as subtask");
                id
            })
            .collect();
        (parent, sections)
    }

    #[test]
    fn a_weekly_parent_and_all_its_sections_reset_on_monday_when_all_were_ticked_by_sunday() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let (parent, sections) = maths_assignment(&db);
        for id in &sections {
            complete_on(&db, *id, "2026-09-06");
        }
        // The parent itself was never ticked: it still moves with its
        // sections rather than being stranded on last week's date.
        assert!(!find_task(&db.conn, parent).expect("find").expect("exists").completed);

        let reset_count = reset_due_repeating_tasks(&db.conn, day(2026, 9, 7)).expect("monday");

        assert_eq!(reset_count, 4);
        for id in std::iter::once(parent).chain(sections) {
            let task = find_task(&db.conn, id).expect("find").expect("exists");
            assert_eq!(task.due_date.as_deref(), Some("2026-09-14"));
            assert!(!task.completed);
        }
    }

    #[test]
    fn on_monday_only_the_sections_that_were_ticked_by_sunday_move() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let (parent, sections) = maths_assignment(&db);
        complete_on(&db, sections[0], "2026-09-05");
        complete_on(&db, sections[1], "2026-09-06");
        // Section C was not done, so neither is the assignment.

        let reset_count = reset_due_repeating_tasks(&db.conn, day(2026, 9, 7)).expect("monday");

        assert_eq!(reset_count, 2, "just A and B");
        assert_eq!(due_of(&db, sections[0]).as_deref(), Some("2026-09-14"));
        assert_eq!(due_of(&db, sections[1]).as_deref(), Some("2026-09-14"));
        assert_eq!(due_of(&db, sections[2]).as_deref(), Some("2026-09-07"), "unticked: stays");
        assert_eq!(due_of(&db, parent).as_deref(), Some("2026-09-07"), "not finished: stays");
    }

    /// A daily task looping through `topics`, due 2026-09-01 and showing
    /// the first topic.
    fn daily_topic_task(db: &Database, topics: &str) -> i64 {
        let id = weekly_task(db, "Vocab", "2026-09-01");
        set_repeat_interval_days(&db.conn, id, 1).expect("daily");
        set_repeat_list(&db.conn, id, true, "Vocab", topics, crate::tasks::repeat_list::RepeatListMode::Sequential)
            .expect("list");
        id
    }

    fn title_of(db: &Database, id: i64) -> String {
        find_task(&db.conn, id).expect("find").expect("exists").title
    }

    #[test]
    fn a_daily_topic_task_is_one_task_renamed_to_each_days_topic() {
        // "Tasks which repeat and loop through a list of topics should
        // only show the parent task... renamed daily to the
        // corresponding topic for that day."
        let db = Database::open_in_memory().expect("open in-memory db");
        let id = daily_topic_task(&db, "A\nB\nC");
        assert_eq!(title_of(&db, id), "Vocab: A");

        for (d, topic) in [(2, "B"), (3, "C"), (4, "A"), (5, "B")] {
            reset_due_repeating_tasks(&db.conn, day(2026, 9, d)).expect("reset pass");
            assert_eq!(title_of(&db, id), format!("Vocab: {topic}"), "09-0{d}");
            assert_eq!(due_of(&db, id), Some(format!("2026-09-0{d}")));
            assert_eq!(list_tasks(&db.conn).expect("list").len(), 1, "never more than the one task");
        }
    }

    #[test]
    fn skipped_days_land_on_the_topic_that_belongs_to_today() {
        // The app wasn't opened on 09-02 and 09-03: on 09-04 the title
        // must show 09-04's topic (A, the fourth in a 3-topic cycle),
        // not merely the one after the last it showed (B).
        let db = Database::open_in_memory().expect("open in-memory db");
        let id = daily_topic_task(&db, "A\nB\nC");
        reset_due_repeating_tasks(&db.conn, day(2026, 9, 4)).expect("reset pass");
        assert_eq!(title_of(&db, id), "Vocab: A");
        assert_eq!(due_of(&db, id).as_deref(), Some("2026-09-04"));
    }

    #[test]
    fn a_ticked_daily_topic_task_comes_back_next_day_with_the_next_topic() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let id = daily_topic_task(&db, "A\nB\nC");
        complete_on(&db, id, "2026-09-01");

        // Ticked today: still showing today's topic, ticked.
        reset_due_repeating_tasks(&db.conn, day(2026, 9, 1)).expect("same day");
        assert_eq!(title_of(&db, id), "Vocab: A");
        assert!(find_task(&db.conn, id).expect("find").expect("exists").completed);

        reset_due_repeating_tasks(&db.conn, day(2026, 9, 2)).expect("next day");
        assert_eq!(title_of(&db, id), "Vocab: B");
        assert!(!find_task(&db.conn, id).expect("find").expect("exists").completed);
    }

    #[test]
    fn a_weekly_topic_task_advances_once_per_week_and_over_gaps_by_whole_weeks() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let id = weekly_task(&db, "Essay", "2026-09-07");
        set_repeat_list(&db.conn, id, true, "Essay", "W1\nW2\nW3\nW4", crate::tasks::repeat_list::RepeatListMode::Sequential)
            .expect("list");
        assert_eq!(title_of(&db, id), "Essay: W1");

        // Three weeks unopened: the topic for the third week on.
        reset_due_repeating_tasks(&db.conn, day(2026, 9, 28)).expect("reset pass");
        assert_eq!(due_of(&db, id).as_deref(), Some("2026-09-28"));
        assert_eq!(title_of(&db, id), "Essay: W4");
    }

    /// Builds what the retired "plan each day's topic ahead" feature
    /// left behind: a flagged template plus generated per-day tasks
    /// (one finished, two not).
    fn legacy_template(db: &Database) -> (i64, Vec<i64>) {
        let template = daily_topic_task(db, "A\nB\nC");
        db.conn
            .execute("UPDATE tasks SET plan_topics_ahead = 1, plan_signature = 'sig', title = 'Vocab' WHERE id = ?1", params![template])
            .expect("flag as template");
        let generated: Vec<i64> = ["Vocab: A", "Vocab: B", "Vocab: C"]
            .iter()
            .map(|title| {
                let id = create_task(&db.conn, title, Some("2026-09-02"), None, None).expect("create generated").id;
                db.conn.execute("UPDATE tasks SET generated_from = ?1 WHERE id = ?2", params![template, id]).expect("link");
                id
            })
            .collect();
        complete_on(db, generated[0], "2026-09-01");
        (template, generated)
    }

    #[test]
    fn retiring_topic_planning_leaves_one_ordinary_topic_task() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let (template, generated) = legacy_template(&db);

        assert_eq!(retire_topic_planning(&db.conn).expect("retire"), 1);

        let task = find_task(&db.conn, template).expect("find").expect("exists");
        assert!(!task.plan_topics_ahead, "no longer a template");
        assert_eq!(task.title, "Vocab: A", "shows a topic again");
        assert!(find_task(&db.conn, generated[1]).expect("find").is_none(), "unfinished per-day tasks are gone");
        assert!(find_task(&db.conn, generated[2]).expect("find").is_none());
        assert!(find_task(&db.conn, generated[0]).expect("find").is_some(), "finished work is kept");

        // Idempotent: nothing left to convert.
        assert_eq!(retire_topic_planning(&db.conn).expect("again"), 0);

        // And the converted task now renames itself like any other.
        reset_due_repeating_tasks(&db.conn, day(2026, 9, 3)).expect("reset pass");
        assert_eq!(title_of(&db, template), "Vocab: C", "two days on from the first topic");
    }

    #[test]
    fn retiring_topic_planning_deletes_the_blocks_of_the_per_day_tasks_it_removes() {
        // `generated[1]` is deleted by the retirement; with foreign keys
        // enforced the delete would be rejected (and silently skipped by the
        // cleanup) if its block were left behind. `generated[0]` is finished
        // work that is kept, so its block stays too.
        let db = Database::open_in_memory().expect("open in-memory db");
        let (_template, generated) = legacy_template(&db);
        add_block(&db, generated[1]);
        add_block(&db, generated[0]);

        assert_eq!(retire_topic_planning(&db.conn).expect("retire"), 1);

        assert!(find_task(&db.conn, generated[1]).expect("find").is_none(), "the task really was deleted");
        assert_eq!(block_count(&db, generated[1]), 0);
        assert_eq!(block_count(&db, generated[0]), 1, "a kept task keeps its block");
    }

    #[test]
    fn a_non_repeating_subtask_of_a_weekly_parent_is_deleted_once_completed_and_never_resets() {
        // Why each section must itself repeat: a plain subtask that is
        // completed is cleaned up the next day, taking the section
        // with it.
        let db = Database::open_in_memory().expect("open in-memory db");
        let parent = weekly_task(&db, "Weekly review", "2026-09-07");
        let section = create_task(&db.conn, "Part 1", Some("2026-09-07"), None, None).expect("create");
        update_task(&db.conn, section.id, "Part 1", "", Some("2026-09-07"), None, None, false, Some(parent), None)
            .expect("attach");
        set_completed(&db.conn, section.id, true).expect("complete");

        delete_past_completed_tasks(&db.conn, NaiveDate::from_ymd_opt(2099, 1, 1).unwrap()).expect("cleanup");
        assert!(find_task(&db.conn, section.id).expect("find").is_none());
        assert!(list_children(&db.conn, parent).expect("children").is_empty());
    }

    #[test]
    fn list_children_returns_only_that_parents_children() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let parent = create_task(&db.conn, "Parent", None, None, None).expect("create parent");
        let child_a = create_task(&db.conn, "Child A", None, None, None).expect("create a");
        let _unrelated = create_task(&db.conn, "Unrelated", None, None, None).expect("create unrelated");
        update_task(&db.conn, child_a.id, "Child A", "", None, None, None, false, Some(parent.id), None)
            .expect("attach a");

        let children = list_children(&db.conn, parent.id).expect("list children");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, child_a.id);
    }

    // --- Time-blocking cascades (docs/planning/time-blocking.md §5.3) ---------
    //
    // Foreign keys are off, so each path that removes a task or clears
    // its day must delete the task's blocks by hand. One test per path.

    fn add_block(db: &Database, task_id: i64) {
        let start = Local::now();
        crate::database::queries::task_blocks::insert_locked_block(
            &db.conn,
            task_id,
            start,
            start + Duration::hours(1),
            0,
        )
        .expect("insert block");
    }

    fn block_count(db: &Database, task_id: i64) -> usize {
        crate::database::queries::task_blocks::list_blocks_for_task(&db.conn, task_id).expect("list blocks").len()
    }

    #[test]
    fn deleting_a_task_deletes_its_blocks_and_only_its_blocks() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let doomed = create_task(&db.conn, "Doomed", None, None, None).expect("create").id;
        let kept = create_task(&db.conn, "Kept", None, None, None).expect("create").id;
        add_block(&db, doomed);
        add_block(&db, kept);

        delete_task(&db.conn, doomed).expect("delete");

        assert_eq!(block_count(&db, doomed), 0);
        assert_eq!(block_count(&db, kept), 1);
    }

    #[test]
    fn deleting_a_task_also_deletes_the_blocks_of_tasks_generated_from_it() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let template = create_task(&db.conn, "Template", None, None, None).expect("create").id;
        let generated = create_task(&db.conn, "Generated", None, None, None).expect("create").id;
        db.conn.execute("UPDATE tasks SET generated_from = ?1 WHERE id = ?2", params![template, generated]).expect("link");
        add_block(&db, generated);

        delete_task(&db.conn, template).expect("delete");

        assert_eq!(block_count(&db, generated), 0);
    }

    #[test]
    fn cleaning_up_a_past_completed_task_deletes_its_blocks() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let done = create_task(&db.conn, "Done", None, None, None).expect("create").id;
        add_block(&db, done);
        complete_on(&db, done, "2026-09-01");

        delete_past_completed_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 2).unwrap()).expect("cleanup");

        assert_eq!(block_count(&db, done), 0);
    }

    #[test]
    fn completing_a_task_during_its_block_keeps_the_block_cut_off_at_that_moment() {
        // The block is history of the slot, not removed — but it now records the time
        // actually spent (it used to be left untouched; see `block_sessions`).
        let db = Database::open_in_memory().expect("open in-memory db");
        let done = create_task(&db.conn, "Done", None, None, None).expect("create").id;
        let started = Local::now() - Duration::minutes(30);
        crate::database::queries::task_blocks::insert_locked_block(
            &db.conn,
            done,
            started,
            started + Duration::hours(1),
            0,
        )
        .expect("insert block");

        set_completed(&db.conn, done, true).expect("complete");

        let blocks = crate::database::queries::task_blocks::list_blocks_for_task(&db.conn, done).expect("list");
        assert_eq!(blocks.len(), 1, "kept until cleanup");
        assert!(blocks[0].end_time < started + Duration::hours(1), "cut off when it was ticked, not left at its planned end");
    }

    #[test]
    fn completing_a_task_before_its_block_has_started_removes_the_block() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let done = create_task(&db.conn, "Done early", None, None, None).expect("create").id;
        let later = Local::now() + Duration::hours(3);
        crate::database::queries::task_blocks::insert_locked_block(&db.conn, done, later, later + Duration::hours(1), 0)
            .expect("insert block");

        set_completed(&db.conn, done, true).expect("complete");

        assert_eq!(block_count(&db, done), 0, "no time was spent on it");
    }

    #[test]
    fn a_repeating_tasks_reset_deletes_its_blocks() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let weekly = weekly_task(&db, "Weekly", "2026-09-07");
        set_assigned_date(&db.conn, weekly, Some("2026-09-09")).expect("assign");
        add_block(&db, weekly);

        reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 14).unwrap()).expect("reset pass");

        assert_eq!(block_count(&db, weekly), 0, "the day it was blocked on is gone, so are the blocks");
    }

    #[test]
    fn a_repeating_task_that_is_not_reset_keeps_its_blocks() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let weekly = weekly_task(&db, "Weekly", "2026-09-14");
        set_assigned_date(&db.conn, weekly, Some("2026-09-10")).expect("assign");
        add_block(&db, weekly);

        // Not due yet: nothing is reset, so nothing is cleared.
        reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 9).unwrap()).expect("reset pass");

        assert_eq!(block_count(&db, weekly), 1);
    }

    #[test]
    fn unassigning_a_task_deletes_its_blocks() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = create_task(&db.conn, "Task", None, None, None).expect("create").id;
        set_assigned_date(&db.conn, t, Some("2026-09-09")).expect("assign");
        add_block(&db, t);

        set_assigned_date(&db.conn, t, None).expect("unassign");

        assert_eq!(block_count(&db, t), 0);
    }

    #[test]
    fn moving_a_task_to_another_day_deletes_its_blocks_but_re_saving_the_same_day_keeps_them() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = create_task(&db.conn, "Task", None, None, None).expect("create").id;
        set_assigned_date(&db.conn, t, Some("2026-09-09")).expect("assign");
        add_block(&db, t);

        set_assigned_date(&db.conn, t, Some("2026-09-09")).expect("same day");
        assert_eq!(block_count(&db, t), 1, "same day: the time on it is still valid");

        set_assigned_date(&db.conn, t, Some("2026-09-10")).expect("another day");
        assert_eq!(block_count(&db, t), 0, "a time on the old day means nothing on the new one");
    }

    #[test]
    fn the_planners_own_re_assignment_follows_the_same_rule() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = create_task(&db.conn, "Task", None, None, None).expect("create").id;
        set_auto_assignment(&db.conn, t, Some("2026-09-09")).expect("auto assign");
        add_block(&db, t);

        set_auto_assignment(&db.conn, t, Some("2026-09-09")).expect("same day");
        assert_eq!(block_count(&db, t), 1);

        set_auto_assignment(&db.conn, t, Some("2026-09-12")).expect("moved");
        assert_eq!(block_count(&db, t), 0);

        add_block(&db, t);
        set_auto_assignment(&db.conn, t, None).expect("cleared");
        assert_eq!(block_count(&db, t), 0);
    }

    #[test]
    fn assigning_a_day_to_a_task_that_had_none_deletes_nothing_it_should_keep() {
        // `None -> Some`: no blocks can exist for an undated task, but a stray one is dropped.
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = create_task(&db.conn, "Task", None, None, None).expect("create").id;
        let other = create_task(&db.conn, "Other", None, None, None).expect("create").id;
        add_block(&db, t);
        add_block(&db, other);

        set_assigned_date(&db.conn, t, Some("2026-09-09")).expect("assign");

        assert_eq!(block_count(&db, t), 0);
        assert_eq!(block_count(&db, other), 1, "another task's blocks are never touched");
    }
}
