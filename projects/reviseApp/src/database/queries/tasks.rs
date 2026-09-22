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
    })
}

const SELECT_COLUMNS: &str = "id, title, description, completed, created_at, due_date, position, event_id, \
                               completed_at, subject_id, assigned_date, repeat_weekly, parent_task_id, \
                               estimated_minutes, repeat_through_list, repeat_list_base_title, \
                               repeat_list_topics, repeat_list_mode, repeat_list_state";

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
    } else {
        conn.execute("UPDATE tasks SET completed = 0, completed_at = NULL WHERE id = ?1", params![id])?;
    }
    Ok(())
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
/// Feature request: "Tasks Repeating Through A List" — a
/// `repeat_through_list` task also gets its `title` and
/// `repeat_list_state` advanced to the next topic in the same pass,
/// via `tasks::repeat_list::next_topic`, so a fresh topic appears at
/// exactly the same moment the task itself resets — never a week
/// where the task reappears but still shows last week's topic.
pub fn reset_due_repeating_tasks(conn: &Connection, today: NaiveDate) -> Result<usize> {
    let tasks = list_tasks(conn)?;
    let mut reset_count = 0;

    for task in tasks {
        if !task.repeat_weekly || !task.completed {
            continue;
        }
        let Some(due_date) = task.due_date.as_deref().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()) else {
            continue;
        };

        let mut next_due = due_date + Duration::days(7);
        if next_due > today {
            continue;
        }
        while next_due + Duration::days(7) <= today {
            next_due += Duration::days(7);
        }
        let next_due_str = next_due.format("%Y-%m-%d").to_string();

        let topics = crate::tasks::repeat_list::parse_topics(task.repeat_list_topics.as_deref().unwrap_or(""));
        let advanced_topic = if task.repeat_through_list && !topics.is_empty() {
            let mode =
                task.repeat_list_mode.as_deref().and_then(crate::tasks::repeat_list::RepeatListMode::parse).unwrap_or_default();
            let seed =
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
            crate::tasks::repeat_list::next_topic(mode, &topics, task.repeat_list_state.as_deref().unwrap_or(""), seed)
        } else {
            None
        };

        match advanced_topic {
            Some((topic, new_state)) => {
                let base = task.repeat_list_base_title.as_deref().unwrap_or(&task.title);
                conn.execute(
                    "UPDATE tasks SET completed = 0, completed_at = NULL, due_date = ?1, title = ?2, \
                     repeat_list_state = ?3 WHERE id = ?4",
                    params![next_due_str, format!("{base}: {topic}"), new_state, task.id],
                )?;
            }
            None => {
                conn.execute(
                    "UPDATE tasks SET completed = 0, completed_at = NULL, due_date = ?1 WHERE id = ?2",
                    params![next_due_str, task.id],
                )?;
            }
        }
        reset_count += 1;
    }

    Ok(reset_count)
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

/// Feature request: "Task Assignment" — plans (or un-plans, with
/// `None`) a task for a specific day, independent of `due_date`.
/// Kept as its own fast-path call, not folded into `update_task`,
/// since assigning a task to a day is a one-click action (from the
/// Dashboard's Today's Tasks or the Calendar Day view) rather than
/// something done from the full edit modal.
pub fn set_assigned_date(conn: &Connection, id: i64, assigned_date: Option<&str>) -> Result<()> {
    conn.execute("UPDATE tasks SET assigned_date = ?1 WHERE id = ?2", params![assigned_date, id])?;
    Ok(())
}

pub fn delete_task(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
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

    #[test]
    fn a_completed_repeating_task_is_not_reset_before_its_next_due_date() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "Water the plants", Some("2026-09-01"), None, None).expect("create");
        update_task(&db.conn, task.id, "Water the plants", "", Some("2026-09-01"), None, None, true, None, None)
            .expect("mark repeating");
        set_completed(&db.conn, task.id, true).expect("complete");

        // Still the same week — next due date (2026-09-08) hasn't
        // arrived yet.
        let reset_count = reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 4).unwrap())
            .expect("reset pass");

        assert_eq!(reset_count, 0);
        let reloaded = find_task(&db.conn, task.id).expect("find").expect("exists");
        assert!(reloaded.completed, "must stay completed until its own next due date");
        assert_eq!(reloaded.due_date.as_deref(), Some("2026-09-01"));
    }

    #[test]
    fn a_completed_repeating_task_resets_exactly_on_its_next_due_date() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "Water the plants", Some("2026-09-01"), None, None).expect("create");
        update_task(&db.conn, task.id, "Water the plants", "", Some("2026-09-01"), None, None, true, None, None)
            .expect("mark repeating");
        set_completed(&db.conn, task.id, true).expect("complete");

        let reset_count = reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 8).unwrap())
            .expect("reset pass");

        assert_eq!(reset_count, 1);
        let reloaded = find_task(&db.conn, task.id).expect("find").expect("exists");
        assert!(!reloaded.completed, "must reset back to incomplete on its next due date");
        assert!(reloaded.completed_at.is_none());
        assert_eq!(reloaded.due_date.as_deref(), Some("2026-09-08"), "due date advances by exactly one week");
        assert!(reloaded.repeat_weekly, "still repeats");
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
        set_completed(&db.conn, task.id, true).expect("complete week 1");

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
        set_completed(&db.conn, task.id, true).expect("complete");

        reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 8).unwrap()).expect("reset");

        let reloaded = find_task(&db.conn, task.id).expect("find").expect("exists");
        assert_eq!(reloaded.title, "Water the plants");
    }

    #[test]
    fn a_completed_repeating_task_left_unopened_for_several_cycles_lands_on_the_most_recent_occurrence() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = create_task(&db.conn, "Water the plants", Some("2026-09-01"), None, None).expect("create");
        update_task(&db.conn, task.id, "Water the plants", "", Some("2026-09-01"), None, None, true, None, None)
            .expect("mark repeating");
        set_completed(&db.conn, task.id, true).expect("complete");

        // Three weeks later — 09-08, 09-15, and 09-22 have all come
        // and gone since the app was last opened.
        let reset_count = reset_due_repeating_tasks(&db.conn, NaiveDate::from_ymd_opt(2026, 9, 22).unwrap())
            .expect("reset pass");

        assert_eq!(reset_count, 1, "a multi-week gap is still exactly one reset, not one per missed week");
        let reloaded = find_task(&db.conn, task.id).expect("find").expect("exists");
        assert!(!reloaded.completed);
        assert_eq!(reloaded.due_date.as_deref(), Some("2026-09-22"), "lands on the most recent occurrence, not the stalest one");
    }

    #[test]
    fn reset_due_repeating_tasks_ignores_incomplete_and_non_repeating_tasks() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let far_future = NaiveDate::from_ymd_opt(2027, 1, 1).unwrap();

        let incomplete_repeating = create_task(&db.conn, "Not done yet", Some("2026-09-01"), None, None).expect("create");
        update_task(
            &db.conn,
            incomplete_repeating.id,
            "Not done yet",
            "",
            Some("2026-09-01"),
            None,
            None,
            true,
            None,
            None,
        )
        .expect("mark repeating");

        let completed_one_off = create_task(&db.conn, "One-off", Some("2026-09-01"), None, None).expect("create");
        set_completed(&db.conn, completed_one_off.id, true).expect("complete");

        let reset_count = reset_due_repeating_tasks(&db.conn, far_future).expect("reset pass");

        assert_eq!(reset_count, 0);
        assert!(!find_task(&db.conn, incomplete_repeating.id).expect("find").expect("exists").completed);
        assert!(find_task(&db.conn, completed_one_off.id).expect("find").expect("exists").completed);
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
}
