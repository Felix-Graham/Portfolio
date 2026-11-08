//! Storage for time blocks (`task_blocks`, migration v27). See
//! `docs/planning/time-blocking.md` §5.2–5.3.
//!
//! Single-purpose functions, in the style of the rest of `queries`.
//! There is no `ON DELETE CASCADE`, and although the bundled SQLite
//! enforces `REFERENCES` by default, enforcement only *rejects* a bad
//! delete — it never cleans up. Anything that deletes or un-days a
//! task must call [`delete_blocks_for_task`] **before** removing the
//! task (the wiring lives in `queries::tasks`, one test per path).
//! [`delete_orphan_blocks`] is the safety net the daily cleanup runs
//! for a build or database where foreign keys are off.

use std::collections::HashSet;

use chrono::{DateTime, Local, NaiveDate};
use rusqlite::{params, Connection, Result};

use crate::tasks::time_blocking::{naive_to_local, NewBlock};
use crate::tasks::{BlockStatus, TimeBlock};

fn row_to_block(row: &rusqlite::Row) -> rusqlite::Result<TimeBlock> {
    let start_raw: String = row.get(2)?;
    let end_raw: String = row.get(3)?;

    // Stored via `to_rfc3339()`, so this should always parse; a
    // corrupt row falls back to "now" rather than panicking the UI
    // (same policy as `events`).
    let start_time =
        DateTime::parse_from_rfc3339(&start_raw).map(|dt| dt.with_timezone(&Local)).unwrap_or_else(|_| Local::now());
    let end_time =
        DateTime::parse_from_rfc3339(&end_raw).map(|dt| dt.with_timezone(&Local)).unwrap_or_else(|_| Local::now());

    Ok(TimeBlock {
        id: row.get(0)?,
        task_id: row.get(1)?,
        start_time,
        end_time,
        locked: row.get::<_, i64>(4)? != 0,
        chunk_index: row.get(5)?,
        status: BlockStatus::from_db(row.get(6)?),
    })
}

const SELECT_COLUMNS: &str = "id, task_id, start_time, end_time, locked, chunk_index, status";

/// One block by id.
pub fn find_block(conn: &Connection, id: i64) -> Result<Option<TimeBlock>> {
    let mut stmt = conn.prepare(&format!("SELECT {SELECT_COLUMNS} FROM task_blocks WHERE id = ?1"))?;
    let mut rows = stmt.query_map(params![id], row_to_block)?;
    rows.next().transpose()
}

/// Every block, chronologically. The table only ever holds today and
/// later (past days are swept by the daily cleanup), so it stays tiny;
/// filtering by day is done in Rust, like `events`, because RFC3339
/// strings with offsets don't compare reliably in SQL.
fn list_all_blocks(conn: &Connection) -> Result<Vec<TimeBlock>> {
    let mut stmt = conn.prepare(&format!("SELECT {SELECT_COLUMNS} FROM task_blocks"))?;
    let mut blocks: Vec<TimeBlock> = stmt.query_map([], row_to_block)?.collect::<Result<_>>()?;
    blocks.sort_by_key(|b| (b.start_time, b.id));
    Ok(blocks)
}

/// Blocks whose start day is in `[start_day, end_day]` (inclusive),
/// chronologically.
pub fn list_blocks_between(conn: &Connection, start_day: NaiveDate, end_day: NaiveDate) -> Result<Vec<TimeBlock>> {
    Ok(list_all_blocks(conn)?.into_iter().filter(|b| b.day() >= start_day && b.day() <= end_day).collect())
}

/// A task's blocks in chunk order.
pub fn list_blocks_for_task(conn: &Connection, task_id: i64) -> Result<Vec<TimeBlock>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLUMNS} FROM task_blocks WHERE task_id = ?1 ORDER BY chunk_index ASC, id ASC"
    ))?;
    let blocks = stmt.query_map(params![task_id], row_to_block)?.collect::<Result<_>>()?;
    Ok(blocks)
}

/// Replaces the **auto** (unlocked) blocks on `days` with `new_blocks`,
/// in one transaction: either the day is re-planned or it is left as
/// it was. Locked blocks, and every block on other days, are
/// untouched. A new block is filed under its own start day, so pass
/// blocks only for days in `days`.
pub fn replace_auto_blocks_for_days(conn: &Connection, days: &[NaiveDate], new_blocks: &[NewBlock]) -> Result<()> {
    replace_auto_blocks_for_days_keeping(conn, days, new_blocks, &HashSet::new())
}

/// [`replace_auto_blocks_for_days`], but blocks belonging to any task
/// in `keep_tasks` survive too. The planner passes the completed
/// tasks: a finished task's block is history of the plan (drawn as
/// done), so re-planning the day must not erase it.
pub fn replace_auto_blocks_for_days_keeping(
    conn: &Connection,
    days: &[NaiveDate],
    new_blocks: &[NewBlock],
    keep_tasks: &HashSet<i64>,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    let stale: Vec<i64> = list_all_blocks(&tx)?
        .into_iter()
        .filter(|b| {
            // Only planned, unpinned blocks are the planner's to replace; one in
            // progress or done is history of what actually happened.
            !b.locked && !b.is_settled() && days.contains(&b.day()) && !keep_tasks.contains(&b.task_id)
        })
        .map(|b| b.id)
        .collect();
    for id in stale {
        tx.execute("DELETE FROM task_blocks WHERE id = ?1", params![id])?;
    }

    for block in new_blocks {
        tx.execute(
            "INSERT INTO task_blocks (task_id, start_time, end_time, locked, chunk_index) \
             VALUES (?1, ?2, ?3, 0, ?4)",
            params![
                block.task_id,
                naive_to_local(block.start).to_rfc3339(),
                naive_to_local(block.end).to_rfc3339(),
                block.chunk_index as i64
            ],
        )?;
    }

    tx.commit()
}

/// D6 of the plan: a block the person pinned, moved or resized is
/// *theirs*, so its task stops being "placed by the planner"
/// (`auto_assigned = 0`, `assigned_date` untouched). Without this the
/// next Auto-assign could move the task to another day — and a task
/// changing day deletes its blocks, pinned ones included, which would
/// silently throw the person's choice away.
fn make_task_manual(conn: &Connection, task_id: i64) -> Result<()> {
    conn.execute("UPDATE tasks SET auto_assigned = 0 WHERE id = ?1", params![task_id])?;
    Ok(())
}

fn task_of_block(conn: &Connection, block_id: i64) -> Result<Option<i64>> {
    use rusqlite::OptionalExtension;
    conn.query_row("SELECT task_id FROM task_blocks WHERE id = ?1", params![block_id], |row| row.get(0)).optional()
}

/// A block the person placed by hand (`locked = 1`).
pub fn insert_locked_block(
    conn: &Connection,
    task_id: i64,
    start_time: DateTime<Local>,
    end_time: DateTime<Local>,
    chunk_index: i64,
) -> Result<TimeBlock> {
    conn.execute(
        "INSERT INTO task_blocks (task_id, start_time, end_time, locked, chunk_index) VALUES (?1, ?2, ?3, 1, ?4)",
        params![task_id, start_time.to_rfc3339(), end_time.to_rfc3339(), chunk_index],
    )?;
    let id = conn.last_insert_rowid();
    make_task_manual(conn, task_id)?;
    Ok(find_block(conn, id)?.expect("row just inserted"))
}

/// Moves or resizes a block. Any such edit is the person's, so it
/// **locks** the block: re-planning must not undo it (D6).
pub fn update_block_times(
    conn: &Connection,
    id: i64,
    start_time: DateTime<Local>,
    end_time: DateTime<Local>,
) -> Result<()> {
    conn.execute(
        "UPDATE task_blocks SET start_time = ?1, end_time = ?2, locked = 1 WHERE id = ?3",
        params![start_time.to_rfc3339(), end_time.to_rfc3339(), id],
    )?;
    if let Some(task_id) = task_of_block(conn, id)? {
        make_task_manual(conn, task_id)?;
    }
    Ok(())
}

/// Pins or unpins a block without moving it. Pinned (`locked`) blocks
/// are the person's: re-planning leaves them in place and plans around
/// them. Unpinning hands the block back to the planner, which may move
/// or drop it on the next run.
pub fn set_block_locked(conn: &Connection, id: i64, locked: bool) -> Result<()> {
    conn.execute("UPDATE task_blocks SET locked = ?1 WHERE id = ?2", params![locked as i64, id])?;
    // Pinning takes the task out of the planner's hands. Unpinning does
    // not hand it back: the person may still want the day they chose
    // ("Reset to auto" in the plan would need to remember who set the flag).
    if locked {
        if let Some(task_id) = task_of_block(conn, id)? {
            make_task_manual(conn, task_id)?;
        }
    }
    Ok(())
}

/// What [`resize_block`] left to do about the rest of the task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeOutcome {
    /// Minutes of the task's estimate not covered by pinned or in-progress
    /// blocks after the resize (`0` when the block now covers all of it).
    pub remaining: i64,
    /// How many blocks the planner then made for that remainder, later the
    /// same day or on a later one (`0`: none needed, none found, or time
    /// blocking is off).
    pub replanned: usize,
}

/// Resizes a block, pins it, and **leaves the task's estimate alone**.
///
/// A task's estimate is a prediction of the time it takes, and one block
/// being made shorter doesn't make the task shorter: the minutes cut off are
/// still to be done. So they are planned again, as new unpinned blocks
/// *after the end of this one* — later the same day, or on a later day. A
/// block made longer covers more of the estimate and nothing is added (any
/// unpinned pieces the task no longer needs are dropped). With time blocking
/// off nothing is planned: the person is placing blocks by hand.
///
/// (This replaces the earlier rule that a resize rewrote the estimate to the
/// total of the task's blocks.)
pub fn resize_block(
    conn: &Connection,
    id: i64,
    start_time: DateTime<Local>,
    end_time: DateTime<Local>,
    today: NaiveDate,
    now: chrono::NaiveDateTime,
) -> Result<ResizeOutcome> {
    update_block_times(conn, id, start_time, end_time)?;
    let task_id: i64 = conn.query_row("SELECT task_id FROM task_blocks WHERE id = ?1", params![id], |row| row.get(0))?;

    let remaining = super::time_blocking::minutes_still_to_place(conn, task_id)?;
    let replanned = if crate::database::queries::settings::time_blocking_enabled(conn)? {
        // Not before the end of this block, so the rest lands after it.
        let after = now.max(end_time.naive_local());
        super::time_blocking::plan_task_blocks(conn, today, after, task_id, start_time.date_naive())?
    } else {
        0
    };
    Ok(ResizeOutcome { remaining, replanned })
}

/// Feature request: cuts a planned block in two, at its middle (snapped
/// down to `granularity` minutes), so the halves can be moved and resized
/// separately. Both halves are pinned — they are now the person's, and a
/// re-plan that rebuilt them from the estimate would just put the block
/// back — and the task's estimate is untouched (the two cover what one did).
/// Returns the id of the new second half, or `None` for a block that can't
/// be split: one being worked or already worked, one that doesn't exist, or
/// one too short to give each half at least one grid step.
pub fn split_block(conn: &Connection, id: i64, granularity: i64) -> Result<Option<i64>> {
    let Some(block) = find_block(conn, id)? else { return Ok(None) };
    if block.status != BlockStatus::Planned {
        return Ok(None);
    }
    let grid = chrono::Duration::minutes(granularity.max(1));
    let (start, end) = (block.start_time.naive_local(), block.end_time.naive_local());
    let mid = crate::tasks::time_blocking::snap_down(start + (end - start) / 2, granularity.max(1));
    if mid - start < grid || end - mid < grid {
        return Ok(None);
    }

    let tx = conn.unchecked_transaction()?;
    let pieces = list_blocks_for_task(&tx, block.task_id)?.len() as i64;
    tx.execute(
        "UPDATE task_blocks SET end_time = ?1, locked = 1 WHERE id = ?2",
        params![naive_to_local(mid).to_rfc3339(), id],
    )?;
    let second = insert_locked_block(&tx, block.task_id, naive_to_local(mid), block.end_time, pieces)?.id;
    tx.commit()?;
    Ok(Some(second))
}

/// Sets a block's status without touching its times or pinning it.
pub fn set_block_status(conn: &Connection, id: i64, status: BlockStatus) -> Result<()> {
    conn.execute("UPDATE task_blocks SET status = ?1 WHERE id = ?2", params![status.to_db(), id])?;
    Ok(())
}

/// Records what actually happened to a block: its times become the real
/// span and its status the given one (in practice `Done`). Unlike
/// [`update_block_times`] this is the *app's* observation, not the
/// person's edit, so it neither pins the block nor takes the task out of
/// the planner's hands.
pub fn settle_block(
    conn: &Connection,
    id: i64,
    start_time: DateTime<Local>,
    end_time: DateTime<Local>,
    status: BlockStatus,
) -> Result<()> {
    conn.execute(
        "UPDATE task_blocks SET start_time = ?1, end_time = ?2, status = ?3 WHERE id = ?4",
        params![start_time.to_rfc3339(), end_time.to_rfc3339(), status.to_db(), id],
    )?;
    Ok(())
}

/// Adds auto (unpinned, planned) blocks without removing anything — for
/// placing one task's follow-up block without re-planning every day.
pub fn insert_auto_blocks(conn: &Connection, new_blocks: &[NewBlock]) -> Result<()> {
    for block in new_blocks {
        conn.execute(
            "INSERT INTO task_blocks (task_id, start_time, end_time, locked, chunk_index) VALUES (?1, ?2, ?3, 0, ?4)",
            params![
                block.task_id,
                naive_to_local(block.start).to_rfc3339(),
                naive_to_local(block.end).to_rfc3339(),
                block.chunk_index as i64
            ],
        )?;
    }
    Ok(())
}

pub fn delete_block(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM task_blocks WHERE id = ?1", params![id])?;
    Ok(())
}

/// Removes a task's blocks that are still *plans* (planned, pinned or
/// not), keeping those in progress or done — the record of work that
/// really happened. Used when a task changes day or resets, where its
/// future plans stop meaning anything but its history does not.
pub fn delete_unfinished_blocks_for_task(conn: &Connection, task_id: i64) -> Result<usize> {
    conn.execute(
        "DELETE FROM task_blocks WHERE task_id = ?1 AND status = ?2",
        params![task_id, BlockStatus::Planned.to_db()],
    )
}

/// Removes every block (locked or not) of one task. Returns how many.
/// The cascade every task-removal and un-dating path calls.
pub fn delete_blocks_for_task(conn: &Connection, task_id: i64) -> Result<usize> {
    conn.execute("DELETE FROM task_blocks WHERE task_id = ?1", params![task_id])
}

/// Removes blocks that start before `today` — plans for days that are
/// over. Returns how many. Part of the daily cleanup.
pub fn delete_blocks_before(conn: &Connection, today: NaiveDate) -> Result<usize> {
    let stale: Vec<i64> = list_all_blocks(conn)?.into_iter().filter(|b| b.day() < today).map(|b| b.id).collect();
    for id in &stale {
        delete_block(conn, *id)?;
    }
    Ok(stale.len())
}

/// Safety net: removes blocks whose task no longer exists. With
/// foreign keys enforced this can only happen on a database written
/// while they were off (or hand-edited); it costs one statement.
/// Returns how many. Part of the daily cleanup.
pub fn delete_orphan_blocks(conn: &Connection) -> Result<usize> {
    conn.execute("DELETE FROM task_blocks WHERE task_id NOT IN (SELECT id FROM tasks)", [])
}

/// The block part of the daily cleanup: drops plans for days that are
/// over, then any block whose task no longer exists. Returns how many
/// rows went in total.
pub fn sweep_stale_blocks(conn: &Connection, today: NaiveDate) -> Result<usize> {
    Ok(delete_blocks_before(conn, today)? + delete_orphan_blocks(conn)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::queries::tasks;
    use crate::database::Database;
    use chrono::TimeZone;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    fn at(d: u32, h: u32, m: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, d, h, m, 0).unwrap()
    }

    fn new_block(task_id: i64, d: u32, h1: u32, h2: u32, chunk: usize) -> NewBlock {
        NewBlock {
            task_id,
            start: day(d).and_hms_opt(h1, 0, 0).unwrap(),
            end: day(d).and_hms_opt(h2, 0, 0).unwrap(),
            chunk_index: chunk,
        }
    }

    /// Inserts a block for a task id that doesn't exist. The bundled
    /// SQLite enforces foreign keys, so enforcement is switched off for
    /// just this insert to simulate a database written without it.
    fn insert_orphan(db: &Database, task_id: i64, d: u32) {
        db.conn.execute_batch("PRAGMA foreign_keys = OFF").expect("fk off");
        insert_locked_block(&db.conn, task_id, at(d, 10, 0), at(d, 11, 0), 0).expect("insert orphan");
        db.conn.execute_batch("PRAGMA foreign_keys = ON").expect("fk on");
    }

    fn task(db: &Database, title: &str) -> i64 {
        tasks::create_task(&db.conn, title, None, None, None).expect("create task").id
    }

    #[test]
    fn a_new_database_has_no_blocks() {
        let db = Database::open_in_memory().expect("open in-memory db");
        assert!(list_blocks_between(&db.conn, day(1), day(31)).expect("list").is_empty());
    }

    #[test]
    fn replace_inserts_blocks_that_read_back_unlocked_with_their_times() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[new_block(t, 14, 9, 10, 0)]).expect("replace");

        let blocks = list_blocks_for_task(&db.conn, t).expect("list");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].start_time, at(14, 9, 0));
        assert_eq!(blocks[0].end_time, at(14, 10, 0));
        assert!(!blocks[0].locked);
        assert_eq!(blocks[0].chunk_index, 0);
    }

    #[test]
    fn replace_removes_the_old_auto_blocks_on_those_days_only() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        replace_auto_blocks_for_days(&db.conn, &[day(14), day(15)], &[new_block(t, 14, 9, 10, 0), new_block(t, 15, 9, 10, 1)])
            .expect("first plan");

        // Re-plan only the 14th.
        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[new_block(t, 14, 13, 14, 0)]).expect("re-plan");

        let blocks = list_blocks_between(&db.conn, day(14), day(15)).expect("list");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].start_time, at(14, 13, 0), "the 14th was replaced");
        assert_eq!(blocks[1].start_time, at(15, 9, 0), "the 15th was left alone");
    }

    #[test]
    fn replace_never_touches_locked_blocks() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        let locked = insert_locked_block(&db.conn, t, at(14, 11, 0), at(14, 12, 0), 0).expect("insert locked");
        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[new_block(t, 14, 9, 10, 0)]).expect("plan");
        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[]).expect("clear auto");

        let blocks = list_blocks_between(&db.conn, day(14), day(14)).expect("list");
        assert_eq!(blocks.len(), 1, "only the locked block survives re-planning");
        assert_eq!(blocks[0].id, locked.id);
        assert!(blocks[0].locked);
    }

    #[test]
    fn update_block_times_moves_the_block_and_locks_it() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[new_block(t, 14, 9, 10, 0)]).expect("plan");
        let id = list_blocks_for_task(&db.conn, t).expect("list")[0].id;

        update_block_times(&db.conn, id, at(14, 15, 0), at(14, 16, 30)).expect("move");

        let moved = list_blocks_for_task(&db.conn, t).expect("list").remove(0);
        assert_eq!(moved.start_time, at(14, 15, 0));
        assert_eq!(moved.end_time, at(14, 16, 30));
        assert!(moved.locked, "a user edit locks the block");
    }

    #[test]
    fn list_blocks_between_is_inclusive_chronological_and_filters_by_start_day() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        insert_locked_block(&db.conn, t, at(16, 9, 0), at(16, 10, 0), 0).expect("insert");
        insert_locked_block(&db.conn, t, at(14, 15, 0), at(14, 16, 0), 0).expect("insert");
        insert_locked_block(&db.conn, t, at(14, 9, 0), at(14, 10, 0), 0).expect("insert");
        insert_locked_block(&db.conn, t, at(20, 9, 0), at(20, 10, 0), 0).expect("insert");

        let starts: Vec<_> =
            list_blocks_between(&db.conn, day(14), day(16)).expect("list").iter().map(|b| b.start_time).collect();
        assert_eq!(starts, vec![at(14, 9, 0), at(14, 15, 0), at(16, 9, 0)]);
    }

    #[test]
    fn list_blocks_for_task_returns_chunks_in_order_and_only_that_task() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let a = task(&db, "A");
        let b = task(&db, "B");
        insert_locked_block(&db.conn, a, at(14, 13, 0), at(14, 14, 0), 1).expect("insert");
        insert_locked_block(&db.conn, a, at(14, 9, 0), at(14, 10, 0), 0).expect("insert");
        insert_locked_block(&db.conn, b, at(14, 11, 0), at(14, 12, 0), 0).expect("insert");

        let chunks: Vec<i64> = list_blocks_for_task(&db.conn, a).expect("list").iter().map(|x| x.chunk_index).collect();
        assert_eq!(chunks, vec![0, 1]);
    }

    #[test]
    fn delete_block_and_delete_blocks_for_task_remove_the_right_rows() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let a = task(&db, "A");
        let b = task(&db, "B");
        let first = insert_locked_block(&db.conn, a, at(14, 9, 0), at(14, 10, 0), 0).expect("insert");
        insert_locked_block(&db.conn, a, at(14, 10, 0), at(14, 11, 0), 1).expect("insert");
        insert_locked_block(&db.conn, b, at(14, 11, 0), at(14, 12, 0), 0).expect("insert");

        delete_block(&db.conn, first.id).expect("delete one");
        assert_eq!(list_blocks_for_task(&db.conn, a).expect("list").len(), 1);

        assert_eq!(delete_blocks_for_task(&db.conn, a).expect("delete task blocks"), 1);
        assert!(list_blocks_for_task(&db.conn, a).expect("list").is_empty());
        assert_eq!(list_blocks_for_task(&db.conn, b).expect("list").len(), 1, "another task's block is untouched");
    }

    #[test]
    fn delete_blocks_before_removes_only_days_that_are_over() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        insert_locked_block(&db.conn, t, at(13, 9, 0), at(13, 10, 0), 0).expect("yesterday");
        insert_locked_block(&db.conn, t, at(14, 9, 0), at(14, 10, 0), 0).expect("today");
        insert_locked_block(&db.conn, t, at(15, 9, 0), at(15, 10, 0), 0).expect("tomorrow");

        assert_eq!(delete_blocks_before(&db.conn, day(14)).expect("sweep"), 1);
        let days: Vec<NaiveDate> = list_blocks_between(&db.conn, day(1), day(31)).expect("list").iter().map(|b| b.day()).collect();
        assert_eq!(days, vec![day(14), day(15)]);
    }

    #[test]
    fn delete_orphan_blocks_removes_blocks_of_missing_tasks_only() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let kept = task(&db, "Kept");
        insert_locked_block(&db.conn, kept, at(14, 9, 0), at(14, 10, 0), 0).expect("insert");
        // A block pointing at a task id that was never created.
        insert_orphan(&db, 9999, 14);

        assert_eq!(delete_orphan_blocks(&db.conn).expect("sweep"), 1);
        let left = list_blocks_between(&db.conn, day(14), day(14)).expect("list");
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].task_id, kept);
    }

    #[test]
    fn a_task_cannot_be_deleted_while_blocks_still_point_at_it() {
        // Documents the enforced-foreign-key behaviour the cascades rely on:
        // deleting the task row directly (skipping the cascade) is rejected,
        // while `tasks::delete_task` — blocks first — succeeds.
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        insert_locked_block(&db.conn, t, at(14, 9, 0), at(14, 10, 0), 0).expect("insert");

        assert!(db.conn.execute("DELETE FROM tasks WHERE id = ?1", params![t]).is_err());
        tasks::delete_task(&db.conn, t).expect("delete_task cascades first");
        assert!(list_blocks_for_task(&db.conn, t).expect("list").is_empty());
    }

    #[test]
    fn the_migration_creates_the_task_blocks_table_with_the_expected_columns() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let columns: Vec<String> = db
            .conn
            .prepare("PRAGMA table_info(task_blocks)")
            .expect("prepare")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query")
            .collect::<Result<_>>()
            .expect("collect");
        assert_eq!(columns, vec!["id", "task_id", "start_time", "end_time", "locked", "chunk_index", "status"]);
    }

    #[test]
    fn sweep_stale_blocks_removes_past_days_and_orphans_in_one_call() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        insert_locked_block(&db.conn, t, at(13, 9, 0), at(13, 10, 0), 0).expect("past");
        insert_orphan(&db, 9999, 14);
        let keeper = insert_locked_block(&db.conn, t, at(14, 11, 0), at(14, 12, 0), 0).expect("keeper");

        assert_eq!(sweep_stale_blocks(&db.conn, day(14)).expect("sweep"), 2);
        let left = list_blocks_between(&db.conn, day(1), day(31)).expect("list");
        assert_eq!(left.iter().map(|b| b.id).collect::<Vec<_>>(), vec![keeper.id]);
    }

    #[test]
    fn replace_keeping_spares_the_blocks_of_the_kept_tasks() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let done = task(&db, "Done");
        let open = task(&db, "Open");
        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[new_block(done, 14, 9, 10, 0), new_block(open, 14, 10, 11, 0)])
            .expect("plan");

        let keep: HashSet<i64> = [done].into_iter().collect();
        replace_auto_blocks_for_days_keeping(&db.conn, &[day(14)], &[], &keep).expect("re-plan");

        assert_eq!(list_blocks_for_task(&db.conn, done).expect("list").len(), 1, "history is kept");
        assert!(list_blocks_for_task(&db.conn, open).expect("list").is_empty(), "the rest is replaced");
    }

    #[test]
    fn set_block_locked_pins_and_unpins_without_moving() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[new_block(t, 14, 9, 10, 0)]).expect("plan");
        let id = list_blocks_for_task(&db.conn, t).expect("list")[0].id;

        set_block_locked(&db.conn, id, true).expect("pin");
        let pinned = list_blocks_for_task(&db.conn, t).expect("list").remove(0);
        assert!(pinned.locked);
        assert_eq!(pinned.start_time, at(14, 9, 0), "pinning does not move it");

        set_block_locked(&db.conn, id, false).expect("unpin");
        assert!(!list_blocks_for_task(&db.conn, t).expect("list")[0].locked);
    }

    #[test]
    fn an_unpinned_block_is_replanned_again() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        let block = insert_locked_block(&db.conn, t, at(14, 15, 0), at(14, 16, 0), 0).expect("pin");
        set_block_locked(&db.conn, block.id, false).expect("unpin");

        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[]).expect("re-plan clears auto blocks");

        assert!(list_blocks_for_task(&db.conn, t).expect("list").is_empty());
    }

    fn with_estimate(db: &Database, t: i64, minutes: i64) {
        db.conn.execute("UPDATE tasks SET estimated_minutes = ?1 WHERE id = ?2", params![minutes, t]).expect("estimate");
    }

    fn estimate_of(db: &Database, t: i64) -> Option<i64> {
        db.conn.query_row("SELECT estimated_minutes FROM tasks WHERE id = ?1", params![t], |row| row.get(0)).expect("read estimate")
    }

    /// A resize as the Day view makes it: today is the 1st, and it is midnight.
    fn resize(db: &Database, id: i64, start: DateTime<Local>, end: DateTime<Local>) -> ResizeOutcome {
        resize_block(&db.conn, id, start, end, day(1), day(1).and_hms_opt(0, 0, 0).unwrap()).expect("resize")
    }

    #[test]
    fn resizing_a_block_locks_it_but_leaves_the_tasks_estimate_alone() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        with_estimate(&db, t, 60);
        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[new_block(t, 14, 9, 10, 0)]).expect("plan");
        let id = list_blocks_for_task(&db.conn, t).expect("list")[0].id;

        let outcome = resize(&db, id, at(14, 9, 0), at(14, 11, 30));

        let resized = list_blocks_for_task(&db.conn, t).expect("list").remove(0);
        assert!(resized.locked);
        assert_eq!(resized.end_time, at(14, 11, 30));
        assert_eq!(estimate_of(&db, t), Some(60), "a longer block is not a longer task");
        assert_eq!(outcome, ResizeOutcome { remaining: 0, replanned: 0 }, "150 min covers the 60");
    }

    #[test]
    fn shrinking_a_block_leaves_the_estimate_and_reports_what_is_still_to_place() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        with_estimate(&db, t, 60);
        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[new_block(t, 14, 9, 10, 0)]).expect("plan");
        let id = list_blocks_for_task(&db.conn, t).expect("list")[0].id;

        let outcome = resize(&db, id, at(14, 9, 0), at(14, 9, 30));

        assert_eq!(estimate_of(&db, t), Some(60), "still an hour of work");
        assert_eq!(outcome.remaining, 30, "the other half hour is still to do");
        assert_eq!(outcome.replanned, 0, "time blocking is off, so nothing is planned for it");
    }

    #[test]
    fn resizing_one_piece_of_a_split_task_counts_only_pinned_pieces_toward_the_estimate() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Maths assignment");
        with_estimate(&db, t, 120);
        replace_auto_blocks_for_days(
            &db.conn,
            &[day(14), day(15)],
            &[new_block(t, 14, 9, 10, 0), new_block(t, 15, 9, 10, 1)],
        )
        .expect("plan two pieces");
        let first = list_blocks_for_task(&db.conn, t).expect("list")[0].id;

        // Stretch the first piece from 60 to 90 minutes; the other is still a plan.
        let outcome = resize(&db, first, at(14, 9, 0), at(14, 10, 30));

        assert_eq!(estimate_of(&db, t), Some(120));
        assert_eq!(outcome.remaining, 30, "90 pinned of 120");
    }

    #[test]
    fn a_resize_never_changes_the_estimate_even_when_the_block_collapses() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        with_estimate(&db, t, 60);
        let block = insert_locked_block(&db.conn, t, at(14, 9, 0), at(14, 10, 0), 0).expect("insert");

        resize(&db, block.id, at(14, 9, 0), at(14, 9, 0));

        assert_eq!(estimate_of(&db, t), Some(60));
    }

    #[test]
    fn resizing_a_block_does_not_touch_another_tasks_estimate() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let a = task(&db, "A");
        let b = task(&db, "B");
        with_estimate(&db, b, 45);
        let block = insert_locked_block(&db.conn, a, at(14, 9, 0), at(14, 10, 0), 0).expect("insert");

        resize(&db, block.id, at(14, 9, 0), at(14, 10, 30));

        assert_eq!(estimate_of(&db, b), Some(45));
    }

    // --- split_block -------------------------------------------------------

    #[test]
    fn splitting_a_block_makes_two_pinned_halves_that_cover_the_same_time() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        with_estimate(&db, t, 60);
        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[new_block(t, 14, 9, 10, 0)]).expect("plan");
        let id = list_blocks_for_task(&db.conn, t).expect("list")[0].id;

        let second = split_block(&db.conn, id, 15).expect("split").expect("it can be split");

        let blocks = list_blocks_for_task(&db.conn, t).expect("list");
        assert_eq!(blocks.len(), 2);
        let first = blocks.iter().find(|b| b.id == id).unwrap();
        let other = blocks.iter().find(|b| b.id == second).unwrap();
        assert_eq!((first.start_time, first.end_time), (at(14, 9, 0), at(14, 9, 30)));
        assert_eq!((other.start_time, other.end_time), (at(14, 9, 30), at(14, 10, 0)));
        assert!(first.locked && other.locked, "both halves are the person's now");
        assert_eq!(estimate_of(&db, t), Some(60), "the two cover what one did");
    }

    #[test]
    fn the_split_point_snaps_to_the_grid() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        let block = insert_locked_block(&db.conn, t, at(14, 9, 0), at(14, 10, 15), 0).expect("insert");

        split_block(&db.conn, block.id, 15).unwrap().unwrap();

        let mut blocks = list_blocks_for_task(&db.conn, t).unwrap();
        blocks.sort_by_key(|b| b.start_time);
        assert_eq!(blocks[0].end_time, at(14, 9, 30), "37.5 minutes in, snapped down to the quarter hour");
        assert_eq!(blocks[1].start_time, at(14, 9, 30));
    }

    #[test]
    fn a_block_too_short_to_give_each_half_a_grid_step_is_not_split() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        let block = insert_locked_block(&db.conn, t, at(14, 9, 0), at(14, 9, 15), 0).expect("insert");
        assert_eq!(split_block(&db.conn, block.id, 15).unwrap(), None);
        assert_eq!(list_blocks_for_task(&db.conn, t).unwrap().len(), 1);
    }

    #[test]
    fn a_block_being_worked_or_worked_already_cannot_be_split() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        let block = insert_locked_block(&db.conn, t, at(14, 9, 0), at(14, 10, 0), 0).expect("insert");
        set_block_status(&db.conn, block.id, BlockStatus::Done).unwrap();
        assert_eq!(split_block(&db.conn, block.id, 15).unwrap(), None);
        assert_eq!(split_block(&db.conn, 9999, 15).unwrap(), None, "and a missing one is ignored");
    }

    #[test]
    fn the_halves_of_a_split_block_can_be_moved_separately() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        let block = insert_locked_block(&db.conn, t, at(14, 9, 0), at(14, 10, 0), 0).expect("insert");
        let second = split_block(&db.conn, block.id, 15).unwrap().unwrap();

        update_block_times(&db.conn, second, at(14, 15, 0), at(14, 15, 30)).expect("move the second half");

        let first = find_block(&db.conn, block.id).unwrap().unwrap();
        assert_eq!((first.start_time, first.end_time), (at(14, 9, 0), at(14, 9, 30)), "the first stays put");
    }

    // --- D6: pinning makes the task manual -----------------------------------

    fn auto_assigned(db: &Database, task_id: i64) -> bool {
        tasks::list_tasks(&db.conn).expect("list").into_iter().find(|t| t.id == task_id).expect("exists").auto_assigned
    }

    fn planner_placed_task(db: &Database, title: &str) -> i64 {
        let id = task(db, title);
        tasks::set_auto_assignment(&db.conn, id, Some("2026-10-14")).expect("auto assign");
        assert!(auto_assigned(db, id));
        id
    }

    #[test]
    fn pinning_a_block_makes_its_task_manual_without_changing_its_day() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = planner_placed_task(&db, "Essay");
        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[new_block(t, 14, 9, 10, 0)]).expect("plan");
        let id = list_blocks_for_task(&db.conn, t).expect("list")[0].id;

        set_block_locked(&db.conn, id, true).expect("pin");

        assert!(!auto_assigned(&db, t));
        let still = tasks::list_tasks(&db.conn).expect("list").into_iter().find(|x| x.id == t).expect("exists");
        assert_eq!(still.assigned_date.as_deref(), Some("2026-10-14"), "the day is kept");
    }

    #[test]
    fn moving_or_resizing_a_block_makes_its_task_manual() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let moved = planner_placed_task(&db, "Moved");
        let resized = planner_placed_task(&db, "Resized");
        replace_auto_blocks_for_days(
            &db.conn,
            &[day(14)],
            &[new_block(moved, 14, 9, 10, 0), new_block(resized, 14, 10, 11, 0)],
        )
        .expect("plan");
        let moved_id = list_blocks_for_task(&db.conn, moved).expect("list")[0].id;
        let resized_id = list_blocks_for_task(&db.conn, resized).expect("list")[0].id;

        update_block_times(&db.conn, moved_id, at(14, 13, 0), at(14, 14, 0)).expect("move");
        resize(&db, resized_id, at(14, 10, 0), at(14, 12, 0));

        assert!(!auto_assigned(&db, moved));
        assert!(!auto_assigned(&db, resized));
    }

    #[test]
    fn inserting_a_hand_placed_block_makes_its_task_manual() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = planner_placed_task(&db, "Essay");

        insert_locked_block(&db.conn, t, at(14, 15, 0), at(14, 16, 0), 0).expect("place by hand");

        assert!(!auto_assigned(&db, t));
    }

    #[test]
    fn unpinning_does_not_hand_the_task_back_to_the_planner() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = planner_placed_task(&db, "Essay");
        let block = insert_locked_block(&db.conn, t, at(14, 15, 0), at(14, 16, 0), 0).expect("pin");

        set_block_locked(&db.conn, block.id, false).expect("unpin");

        assert!(!auto_assigned(&db, t), "the person's chosen day stands");
    }

    #[test]
    fn editing_one_tasks_block_leaves_other_planner_tasks_alone() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let edited = planner_placed_task(&db, "Edited");
        let bystander = planner_placed_task(&db, "Bystander");
        let block = insert_locked_block(&db.conn, edited, at(14, 15, 0), at(14, 16, 0), 0).expect("pin");
        update_block_times(&db.conn, block.id, at(14, 16, 0), at(14, 17, 0)).expect("move");

        assert!(auto_assigned(&db, bystander));
    }

    // --- status (planned / in progress / done) ----------------------------------------------

    #[test]
    fn a_new_block_is_planned_and_a_status_change_does_not_move_or_pin_it() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[new_block(t, 14, 9, 10, 0)]).expect("plan");
        let id = list_blocks_for_task(&db.conn, t).expect("list")[0].id;
        assert_eq!(list_blocks_for_task(&db.conn, t).unwrap()[0].status, BlockStatus::Planned);

        set_block_status(&db.conn, id, BlockStatus::InProgress).expect("status");

        let after = list_blocks_for_task(&db.conn, t).unwrap().remove(0);
        assert_eq!(after.status, BlockStatus::InProgress);
        assert_eq!(after.start_time, at(14, 9, 0));
        assert!(!after.locked);
    }

    #[test]
    fn settling_a_block_records_the_real_span_without_pinning_or_making_the_task_manual() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        tasks::set_auto_assignment(&db.conn, t, Some("2026-10-14")).expect("auto assign");
        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[new_block(t, 14, 9, 10, 0)]).expect("plan");
        let id = list_blocks_for_task(&db.conn, t).unwrap()[0].id;

        settle_block(&db.conn, id, at(14, 9, 10), at(14, 10, 25), BlockStatus::Done).expect("settle");

        let done = list_blocks_for_task(&db.conn, t).unwrap().remove(0);
        assert_eq!((done.start_time, done.end_time), (at(14, 9, 10), at(14, 10, 25)));
        assert_eq!(done.status, BlockStatus::Done);
        assert!(!done.locked, "an observation, not an edit");
        assert!(auto_assigned(&db, t), "the planner still owns the task");
    }

    #[test]
    fn re_planning_replaces_planned_blocks_but_never_in_progress_or_done_ones() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        replace_auto_blocks_for_days(
            &db.conn,
            &[day(14)],
            &[new_block(t, 14, 9, 10, 0), new_block(t, 14, 10, 11, 1), new_block(t, 14, 11, 12, 2)],
        )
        .expect("plan");
        let ids: Vec<i64> = list_blocks_for_task(&db.conn, t).unwrap().iter().map(|b| b.id).collect();
        set_block_status(&db.conn, ids[0], BlockStatus::Done).unwrap();
        set_block_status(&db.conn, ids[1], BlockStatus::InProgress).unwrap();

        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[]).expect("re-plan");

        let left: Vec<i64> = list_blocks_for_task(&db.conn, t).unwrap().iter().map(|b| b.id).collect();
        assert_eq!(left, vec![ids[0], ids[1]], "only the planned one went");
    }

    #[test]
    fn a_day_change_removes_plans_but_keeps_the_record_of_work_done() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        tasks::set_assigned_date(&db.conn, t, Some("2026-10-14")).expect("assign");
        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[new_block(t, 14, 9, 10, 0), new_block(t, 14, 10, 11, 1)])
            .expect("plan");
        let done = list_blocks_for_task(&db.conn, t).unwrap()[0].id;
        set_block_status(&db.conn, done, BlockStatus::Done).unwrap();

        tasks::set_assigned_date(&db.conn, t, Some("2026-10-15")).expect("move to another day");

        let left: Vec<i64> = list_blocks_for_task(&db.conn, t).unwrap().iter().map(|b| b.id).collect();
        assert_eq!(left, vec![done]);
    }

    #[test]
    fn insert_auto_blocks_adds_without_removing_anything() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        replace_auto_blocks_for_days(&db.conn, &[day(14)], &[new_block(t, 14, 9, 10, 0)]).expect("plan");

        insert_auto_blocks(&db.conn, &[new_block(t, 15, 9, 10, 1)]).expect("insert");

        assert_eq!(list_blocks_for_task(&db.conn, t).unwrap().len(), 2);
    }

    #[test]
    fn a_done_block_does_not_count_toward_what_a_resize_covers() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let t = task(&db, "Essay");
        with_estimate(&db, t, 60);
        replace_auto_blocks_for_days(&db.conn, &[day(14), day(15)], &[new_block(t, 14, 9, 10, 0), new_block(t, 15, 9, 10, 1)])
            .expect("plan");
        let blocks = list_blocks_for_task(&db.conn, t).unwrap();
        set_block_status(&db.conn, blocks[0].id, BlockStatus::Done).unwrap();

        // Stretch the planned piece to 90 minutes: the worked hour is history, not coverage.
        let outcome = resize(&db, blocks[1].id, at(15, 9, 0), at(15, 10, 30));

        assert_eq!(estimate_of(&db, t), Some(60));
        assert_eq!(outcome.remaining, 0, "90 pinned minutes cover the 60");
    }
}
