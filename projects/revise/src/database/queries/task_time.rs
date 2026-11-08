//! Feature request: task "bars" — time spent per task.
//!
//! A stopwatch, timer or Pomodoro started with a `task` picked logs the
//! minutes it ran against that task ([`log_task_time`], one row per run).
//! The bar beside a task's name is those minutes against the task's
//! *budget* ([`progress_for`]).
//!
//! **Why a budget of its own.** `tasks.estimated_minutes` means "what is
//! still to do": when a planned block is worked, `block_sessions::carry_over`
//! lowers it by the time spent. A bar measured against it would race ahead
//! as the work went on. So the first time time is logged (or carried over)
//! the estimate is copied to `time_budget_minutes`, and that copy is what
//! the bar measures against from then on. A task nobody has worked on has
//! no copy yet and simply follows its estimate (or the planner's default
//! length when it has none). If the person later edits the estimate, the
//! budget becomes what is already spent plus the new estimate — "this much
//! more" — see [`rebase_budget`].
//!
//! A task with subtasks shows its subtasks' time and budgets together.

use std::collections::HashMap;

use chrono::NaiveDate;
use rusqlite::{params, Connection, Result};

use super::pomodoro::FocusSource;
use super::{settings, tasks};
use crate::tasks::time_bar::TaskProgress;
use crate::tasks::Task;

/// Deepest chain of subtasks the roll-up follows (guards a corrupt cycle).
const MAX_DEPTH: usize = 8;

/// Copies the task's current estimate (or the default length) to its budget
/// if it has none yet. Idempotent: an existing budget is never touched.
pub fn ensure_budget(conn: &Connection, task_id: i64) -> Result<()> {
    let default = settings::auto_assign_default_task_minutes(conn)?;
    conn.execute(
        "UPDATE tasks SET time_budget_minutes = COALESCE(time_budget_minutes, \
         CASE WHEN estimated_minutes > 0 THEN estimated_minutes ELSE ?1 END) WHERE id = ?2",
        params![default, task_id],
    )?;
    Ok(())
}

/// Logs a finished run against a task. Returns whether a row was written:
/// not for under a minute (like Focus time) and not for a task that no
/// longer exists (it may have been deleted while the run was going).
pub fn log_task_time(
    conn: &Connection,
    task_id: i64,
    date: NaiveDate,
    minutes: i64,
    source: FocusSource,
) -> Result<bool> {
    if minutes < 1 {
        return Ok(false);
    }
    let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1)", params![task_id], |r| r.get(0))?;
    if !exists {
        return Ok(false);
    }
    ensure_budget(conn, task_id)?;
    conn.execute(
        "INSERT INTO task_time_log (task_id, date, minutes, source) VALUES (?1, ?2, ?3, ?4)",
        params![task_id, date.format("%Y-%m-%d").to_string(), minutes, source.as_str()],
    )?;
    Ok(true)
}

/// Minutes logged against one task (its own, not its subtasks').
pub fn spent_minutes(conn: &Connection, task_id: i64) -> Result<i64> {
    let total: Option<i64> =
        conn.query_row("SELECT SUM(minutes) FROM task_time_log WHERE task_id = ?1", params![task_id], |r| r.get(0))?;
    Ok(total.unwrap_or(0))
}

/// Removes a task's log rows (its delete paths call this before the task
/// row goes). Returns how many.
pub fn delete_for_task(conn: &Connection, task_id: i64) -> Result<usize> {
    conn.execute("DELETE FROM task_time_log WHERE task_id = ?1", params![task_id])
}

/// Called when the person edits a task's estimate: for a task that has a
/// budget, the new budget is the time already spent plus the new estimate.
/// A task with no budget follows its estimate and needs nothing.
pub fn rebase_budget(conn: &Connection, task_id: i64, new_estimate: Option<i64>) -> Result<()> {
    let has_budget: bool = conn.query_row(
        "SELECT time_budget_minutes IS NOT NULL FROM tasks WHERE id = ?1",
        params![task_id],
        |r| r.get(0),
    )?;
    if !has_budget {
        return Ok(());
    }
    let default = settings::auto_assign_default_task_minutes(conn)?;
    let estimate = new_estimate.filter(|m| *m > 0).unwrap_or(default);
    let budget = spent_minutes(conn, task_id)? + estimate;
    conn.execute("UPDATE tasks SET time_budget_minutes = ?1 WHERE id = ?2", params![budget, task_id])?;
    Ok(())
}

/// The subject of a task, for a run that has none of its own.
pub fn task_subject(conn: &Connection, task_id: i64) -> Result<Option<i64>> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row("SELECT subject_id FROM tasks WHERE id = ?1", params![task_id], |r| r.get::<_, Option<i64>>(0))
        .optional()?
        .flatten())
}

fn spent_by_task(conn: &Connection) -> Result<HashMap<i64, i64>> {
    let mut stmt = conn.prepare("SELECT task_id, SUM(minutes) FROM task_time_log GROUP BY task_id")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    rows.collect()
}

fn budgets(conn: &Connection) -> Result<HashMap<i64, i64>> {
    let mut stmt = conn.prepare("SELECT id, time_budget_minutes FROM tasks WHERE time_budget_minutes IS NOT NULL")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    rows.collect()
}

/// The bar for every task in `all_tasks`. A task with subtasks gets its
/// subtasks' progress added up (plus any time logged to the parent itself,
/// against the subtasks' budgets); any other task gets its own.
pub fn progress_for(conn: &Connection, all_tasks: &[Task]) -> Result<HashMap<i64, TaskProgress>> {
    let default = settings::auto_assign_default_task_minutes(conn)?;
    let spent = spent_by_task(conn)?;
    let budgets = budgets(conn)?;

    let own = |t: &Task| {
        let budget = budgets.get(&t.id).copied().unwrap_or_else(|| super::auto_assign::task_minutes(t, default));
        TaskProgress::new(spent.get(&t.id).copied().unwrap_or(0), budget)
    };
    let mut children: HashMap<i64, Vec<&Task>> = HashMap::new();
    for t in all_tasks {
        if let Some(parent) = t.parent_task_id {
            children.entry(parent).or_default().push(t);
        }
    }

    fn resolve(
        task: &Task,
        children: &HashMap<i64, Vec<&Task>>,
        own: &dyn Fn(&Task) -> TaskProgress,
        depth: usize,
    ) -> TaskProgress {
        let mine = own(task);
        let kids = match children.get(&task.id) {
            Some(kids) if depth < MAX_DEPTH => kids,
            _ => return mine,
        };
        let together = TaskProgress::combine(kids.iter().map(|k| resolve(k, children, own, depth + 1)));
        TaskProgress::new(together.spent + mine.spent, together.budget)
    }

    Ok(all_tasks.iter().map(|t| (t.id, resolve(t, &children, &own, 0))).collect())
}

/// Convenience for a caller that has no task list yet.
pub fn progress_for_all(conn: &Connection) -> Result<HashMap<i64, TaskProgress>> {
    let all = tasks::list_tasks(conn)?;
    progress_for(conn, &all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use rusqlite::params;

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 14).unwrap()
    }

    fn task(db: &Database, title: &str, minutes: Option<i64>) -> i64 {
        let id = tasks::create_task(&db.conn, title, None, None, None).expect("task").id;
        db.conn.execute("UPDATE tasks SET estimated_minutes = ?1 WHERE id = ?2", params![minutes, id]).expect("estimate");
        id
    }

    fn progress(db: &Database, id: i64) -> TaskProgress {
        progress_for_all(&db.conn).expect("progress")[&id]
    }

    #[test]
    fn a_task_nobody_has_worked_on_is_empty_against_its_estimate_or_the_default() {
        let db = Database::open_in_memory().unwrap();
        let with = task(&db, "Estimated", Some(45));
        let without = task(&db, "Unestimated", None);
        assert_eq!(progress(&db, with), TaskProgress::new(0, 45));
        assert_eq!(progress(&db, without), TaskProgress::new(0, crate::tasks::auto_assign::DEFAULT_TASK_MINUTES));
    }

    #[test]
    fn logged_runs_add_up_and_each_source_is_recorded() {
        let db = Database::open_in_memory().unwrap();
        let t = task(&db, "Essay", Some(60));
        assert!(log_task_time(&db.conn, t, day(), 20, FocusSource::Stopwatch).unwrap());
        assert!(log_task_time(&db.conn, t, day(), 25, FocusSource::Pomodoro).unwrap());
        assert!(log_task_time(&db.conn, t, day(), 5, FocusSource::Timer).unwrap());
        assert_eq!(spent_minutes(&db.conn, t).unwrap(), 50);
        assert_eq!(progress(&db, t), TaskProgress::new(50, 60));
    }

    #[test]
    fn under_a_minute_and_a_missing_task_log_nothing() {
        let db = Database::open_in_memory().unwrap();
        let t = task(&db, "Essay", Some(60));
        assert!(!log_task_time(&db.conn, t, day(), 0, FocusSource::Timer).unwrap());
        assert!(!log_task_time(&db.conn, 9999, day(), 30, FocusSource::Timer).unwrap());
        assert_eq!(spent_minutes(&db.conn, t).unwrap(), 0);
    }

    #[test]
    fn the_budget_is_fixed_when_work_starts_so_a_shrinking_estimate_does_not_move_the_bar() {
        let db = Database::open_in_memory().unwrap();
        let t = task(&db, "Essay", Some(60));
        log_task_time(&db.conn, t, day(), 30, FocusSource::Stopwatch).unwrap();
        // What the planner does when a block is worked: the estimate falls to what is left.
        db.conn.execute("UPDATE tasks SET estimated_minutes = 30 WHERE id = ?1", params![t]).unwrap();

        assert_eq!(progress(&db, t), TaskProgress::new(30, 60), "half way, not 100%");
    }

    #[test]
    fn working_past_the_budget_overfills_the_bar() {
        let db = Database::open_in_memory().unwrap();
        let t = task(&db, "Essay", Some(60));
        log_task_time(&db.conn, t, day(), 90, FocusSource::Stopwatch).unwrap();
        let p = progress(&db, t);
        assert!(p.is_over());
        assert_eq!(p.describe(), "90 of 60 min (150%) — 30 min over");
    }

    #[test]
    fn editing_the_estimate_of_a_worked_task_means_this_much_more() {
        let db = Database::open_in_memory().unwrap();
        let t = task(&db, "Essay", Some(60));
        log_task_time(&db.conn, t, day(), 90, FocusSource::Stopwatch).unwrap();

        rebase_budget(&db.conn, t, Some(30)).unwrap();

        assert_eq!(progress(&db, t), TaskProgress::new(90, 120));
    }

    #[test]
    fn editing_the_estimate_of_an_untouched_task_just_changes_the_bars_budget() {
        let db = Database::open_in_memory().unwrap();
        let t = task(&db, "Essay", Some(60));
        rebase_budget(&db.conn, t, Some(90)).unwrap();
        db.conn.execute("UPDATE tasks SET estimated_minutes = 90 WHERE id = ?1", params![t]).unwrap();
        assert_eq!(progress(&db, t), TaskProgress::new(0, 90));
    }

    #[test]
    fn a_parent_shows_its_subtasks_time_and_budgets_together() {
        let db = Database::open_in_memory().unwrap();
        let parent = task(&db, "Project", None);
        let a = task(&db, "Part A", Some(60));
        let b = task(&db, "Part B", Some(30));
        db.conn.execute("UPDATE tasks SET parent_task_id = ?1 WHERE id IN (?2, ?3)", params![parent, a, b]).unwrap();
        log_task_time(&db.conn, a, day(), 30, FocusSource::Timer).unwrap();
        log_task_time(&db.conn, b, day(), 30, FocusSource::Timer).unwrap();

        assert_eq!(progress(&db, parent), TaskProgress::new(60, 90));
        assert_eq!(progress(&db, a), TaskProgress::new(30, 60));
    }

    #[test]
    fn deleting_a_task_removes_its_log() {
        let db = Database::open_in_memory().unwrap();
        let t = task(&db, "Essay", Some(60));
        log_task_time(&db.conn, t, day(), 30, FocusSource::Timer).unwrap();
        tasks::delete_task(&db.conn, t).unwrap();
        let rows: i64 = db.conn.query_row("SELECT COUNT(*) FROM task_time_log", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn the_subject_of_a_task_is_available_for_attribution() {
        let db = Database::open_in_memory().unwrap();
        let t = task(&db, "Essay", Some(60));
        assert_eq!(task_subject(&db.conn, t).unwrap(), None);
        assert_eq!(task_subject(&db.conn, 9999).unwrap(), None);
    }

    #[test]
    fn saving_a_task_with_a_new_estimate_rebases_the_budget_but_other_edits_do_not() {
        let db = Database::open_in_memory().unwrap();
        let t = task(&db, "Essay", Some(60));
        log_task_time(&db.conn, t, day(), 40, FocusSource::Timer).unwrap();
        let save = |title: &str, estimate: Option<i64>| {
            tasks::update_task(&db.conn, t, title, "", None, None, None, false, None, estimate).unwrap();
        };

        save("Essay (renamed)", Some(60));
        assert_eq!(progress(&db, t), TaskProgress::new(40, 60), "same estimate: budget untouched");

        save("Essay (renamed)", Some(30));
        assert_eq!(progress(&db, t), TaskProgress::new(40, 70), "40 spent + 30 more");
    }
}

