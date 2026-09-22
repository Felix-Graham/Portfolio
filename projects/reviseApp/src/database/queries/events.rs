use chrono::{DateTime, Local, NaiveDate};
use rusqlite::{params, Connection, Result};

use crate::events::Event;

fn row_to_event(row: &rusqlite::Row) -> rusqlite::Result<Event> {
    let start_raw: String = row.get(3)?;
    let end_raw: String = row.get(4)?;

    // Stored via `to_rfc3339()`, so this should always parse; a
    // corrupt row falls back to "now" rather than panicking the UI.
    let start_time = DateTime::parse_from_rfc3339(&start_raw)
        .map(|dt| dt.with_timezone(&Local))
        .unwrap_or_else(|_| Local::now());
    let end_time = DateTime::parse_from_rfc3339(&end_raw)
        .map(|dt| dt.with_timezone(&Local))
        .unwrap_or_else(|_| Local::now());

    Ok(Event {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        start_time,
        end_time,
        subject_id: row.get(5)?,
        recurrence_id: row.get(6)?,
        exec_command: row.get(7)?,
        track_session: row.get::<_, i64>(8)? != 0,
    })
}

const SELECT_COLUMNS: &str =
    "id, title, description, start_time, end_time, subject_id, recurrence_id, exec_command, track_session";

/// All events, ordered chronologically. Filtering to a specific day is
/// done in Rust (see `crate::ui::calendar::events_on_day`) rather than
/// in SQL, since RFC3339 strings with timezone offsets don't reliably
/// compare against SQLite's `date()` function.
pub fn list_all_events(conn: &Connection) -> Result<Vec<Event>> {
    let mut stmt = conn.prepare(&format!("SELECT {SELECT_COLUMNS} FROM events ORDER BY start_time ASC"))?;
    let events = stmt.query_map([], row_to_event)?.collect::<Result<Vec<_>>>()?;
    Ok(events)
}

#[allow(clippy::too_many_arguments)]
pub fn create_event(
    conn: &Connection,
    title: &str,
    description: &str,
    start_time: DateTime<Local>,
    end_time: DateTime<Local>,
    subject_id: Option<i64>,
    recurrence_id: Option<i64>,
) -> Result<Event> {
    conn.execute(
        "INSERT INTO events (title, description, start_time, end_time, subject_id, recurrence_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![title, description, start_time.to_rfc3339(), end_time.to_rfc3339(), subject_id, recurrence_id],
    )?;

    Ok(Event {
        id: conn.last_insert_rowid(),
        title: title.to_owned(),
        description: description.to_owned(),
        start_time,
        end_time,
        subject_id,
        recurrence_id,
        exec_command: None,
        track_session: true,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn update_event(
    conn: &Connection,
    id: i64,
    title: &str,
    description: &str,
    start_time: DateTime<Local>,
    end_time: DateTime<Local>,
    subject_id: Option<i64>,
    recurrence_id: Option<i64>,
) -> Result<()> {
    conn.execute(
        "UPDATE events SET title = ?1, description = ?2, start_time = ?3, end_time = ?4, subject_id = ?5, recurrence_id = ?6
         WHERE id = ?7",
        params![title, description, start_time.to_rfc3339(), end_time.to_rfc3339(), subject_id, recurrence_id, id],
    )?;
    Ok(())
}

/// Sets (or clears, with `None`) the event's startup command, without
/// touching anything else about it — kept separate from
/// `create_event`/`update_event` so those two functions' existing
/// signatures (and their many call sites, including tests) don't need
/// to change for a feature-request field most events will never use.
pub fn set_exec_command(conn: &Connection, id: i64, exec_command: Option<&str>) -> Result<()> {
    conn.execute("UPDATE events SET exec_command = ?1 WHERE id = ?2", params![exec_command, id])?;
    Ok(())
}

/// Sets whether this event's occurrences show session-logging
/// controls (feature request: "Event Logging") — a fast-path call
/// for the same reason `set_exec_command` is: a field most events
/// keep at its default, set once at creation, without needing to grow
/// `create_event`/`update_event`'s own signatures.
pub fn set_track_session(conn: &Connection, id: i64, track_session: bool) -> Result<()> {
    conn.execute("UPDATE events SET track_session = ?1 WHERE id = ?2", params![track_session, id])?;
    Ok(())
}

/// Deletes an event. Any tasks attached to it are unlinked, not
/// deleted — Sprint 4's acceptance criterion is explicit that tasks
/// can exist independently of events. Its recurrence rule (if any)
/// and any occurrence exceptions are cleaned up too, since nothing
/// else can reference them once the event is gone.
pub fn delete_event(conn: &Connection, id: i64) -> Result<()> {
    let recurrence_id: Option<i64> =
        conn.query_row("SELECT recurrence_id FROM events WHERE id = ?1", params![id], |row| row.get(0)).ok();

    let tx = conn.unchecked_transaction()?;
    tx.execute("UPDATE tasks SET event_id = NULL WHERE event_id = ?1", params![id])?;
    tx.execute("DELETE FROM event_exceptions WHERE event_id = ?1", params![id])?;
    tx.execute("DELETE FROM events WHERE id = ?1", params![id])?;
    if let Some(rule_id) = recurrence_id {
        tx.execute("DELETE FROM recurrence_rules WHERE id = ?1", params![rule_id])?;
    }
    tx.commit()
}

/// Deletes non-recurring events whose day is before `today` — "for
/// calendar events this should be at the end of the day which they
/// were a part of" (feedback pass). Deliberately scoped to
/// non-recurring events only: a recurring event's own row is its
/// *first* occurrence, not "an event from one particular day" — a
/// weekly session that started months ago is still very much a live
/// series, and deleting its row would destroy every future
/// occurrence along with it, not just clean up a stale past one.
/// Returns how many were deleted, mainly for tests.
pub fn delete_past_events(conn: &Connection, today: NaiveDate) -> Result<usize> {
    let events = list_all_events(conn)?;
    let mut deleted = 0;
    for event in events {
        if event.recurrence_id.is_none() && event.day() < today {
            delete_event(conn, event.id)?;
            deleted += 1;
        }
    }
    Ok(deleted)
}
