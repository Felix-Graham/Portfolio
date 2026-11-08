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
        movable: row.get::<_, i64>(9)? != 0,
    })
}

const SELECT_COLUMNS: &str =
    "id, title, description, start_time, end_time, subject_id, recurrence_id, exec_command, track_session, movable";

/// All events, ordered chronologically. Filtering to a specific day is
/// done in Rust (see `crate::ui::calendar::events_on_day`) rather than
/// in SQL, since RFC3339 strings with timezone offsets don't reliably
/// compare against SQLite's `date()` function.
pub fn list_all_events(conn: &Connection) -> Result<Vec<Event>> {
    let mut stmt = conn.prepare(&format!("SELECT {SELECT_COLUMNS} FROM events ORDER BY start_time ASC"))?;
    let events = stmt.query_map([], row_to_event)?.collect::<Result<Vec<_>>>()?;
    Ok(events)
}

/// The events the person can see: [`list_all_events`] without the hidden
/// ones. Hidden events (see [`set_event_hidden`]) still exist and still
/// count — the stats and the planner read [`list_all_events`] — they are
/// only kept off the calendar and out of the pickers. Every screen that
/// lists events for the person to look at or choose from uses this.
pub fn list_visible_events(conn: &Connection) -> Result<Vec<Event>> {
    let mut stmt =
        conn.prepare(&format!("SELECT {SELECT_COLUMNS} FROM events WHERE hidden = 0 ORDER BY start_time ASC"))?;
    let events = stmt.query_map([], row_to_event)?.collect::<Result<Vec<_>>>()?;
    Ok(events)
}

/// Hides (or reveals) an event: it stays in the database and in the stats
/// but is left off the calendar and out of the pickers.
pub fn set_event_hidden(conn: &Connection, id: i64, hidden: bool) -> Result<()> {
    conn.execute("UPDATE events SET hidden = ?1 WHERE id = ?2", params![hidden as i64, id])?;
    Ok(())
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
        movable: false,
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

/// Sets whether the event can be moved and resized in the Day view like a
/// task's block (feature request: "Movable events") — a separate call for
/// the same reason as [`set_track_session`].
pub fn set_event_movable(conn: &Connection, id: i64, movable: bool) -> Result<()> {
    conn.execute("UPDATE events SET movable = ?1 WHERE id = ?2", params![movable, id])?;
    Ok(())
}

/// Moves or resizes an event: changes only its start and end. What the Day
/// view saves when a movable event is dragged.
pub fn update_event_times(
    conn: &Connection,
    id: i64,
    start_time: DateTime<Local>,
    end_time: DateTime<Local>,
) -> Result<()> {
    conn.execute(
        "UPDATE events SET start_time = ?1, end_time = ?2 WHERE id = ?3",
        params![start_time.to_rfc3339(), end_time.to_rfc3339(), id],
    )?;
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

#[cfg(test)]
mod hidden_tests {
    use super::*;
    use crate::database::Database;
    use chrono::TimeZone;

    fn make(db: &Database, title: &str, hour: u32) -> i64 {
        let day = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        let at = |h| Local.from_local_datetime(&day.and_hms_opt(h, 0, 0).unwrap()).earliest().unwrap();
        create_event(&db.conn, title, "", at(hour), at(hour + 1), None, None).expect("create").id
    }

    #[test]
    fn a_new_event_is_visible() {
        let db = Database::open_in_memory().expect("open in-memory db");
        make(&db, "Lecture", 9);
        assert_eq!(list_visible_events(&db.conn).unwrap().len(), 1);
    }

    #[test]
    fn a_hidden_event_is_left_out_of_the_visible_list_but_kept_in_the_full_one() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let shown = make(&db, "Lecture", 9);
        let hidden = make(&db, "Timer focus session", 11);

        set_event_hidden(&db.conn, hidden, true).unwrap();

        let visible: Vec<i64> = list_visible_events(&db.conn).unwrap().iter().map(|e| e.id).collect();
        let all: Vec<i64> = list_all_events(&db.conn).unwrap().iter().map(|e| e.id).collect();
        assert_eq!(visible, vec![shown]);
        assert_eq!(all, vec![shown, hidden], "still there, so the stats and the planner can count it");
    }

    #[test]
    fn a_hidden_event_can_be_revealed_again() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let id = make(&db, "Lecture", 9);
        set_event_hidden(&db.conn, id, true).unwrap();
        set_event_hidden(&db.conn, id, false).unwrap();
        assert_eq!(list_visible_events(&db.conn).unwrap().len(), 1);
    }

    #[test]
    fn visible_events_stay_in_chronological_order() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let late = make(&db, "Late", 15);
        let early = make(&db, "Early", 8);
        let hidden = make(&db, "Hidden", 10);
        set_event_hidden(&db.conn, hidden, true).unwrap();

        let ids: Vec<i64> = list_visible_events(&db.conn).unwrap().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![early, late]);
    }
}

#[cfg(test)]
mod movable_tests {
    use super::*;
    use crate::database::Database;
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, 14, h, m, 0).unwrap()
    }

    fn make(db: &Database) -> i64 {
        create_event(&db.conn, "Lecture", "", at(10, 0), at(11, 0), None, None).unwrap().id
    }

    fn load(db: &Database, id: i64) -> Event {
        list_all_events(&db.conn).unwrap().into_iter().find(|e| e.id == id).unwrap()
    }

    #[test]
    fn a_new_event_is_not_movable_and_can_be_made_so_and_back() {
        let db = Database::open_in_memory().unwrap();
        let id = make(&db);
        assert!(!load(&db, id).movable);

        set_event_movable(&db.conn, id, true).unwrap();
        assert!(load(&db, id).movable);

        set_event_movable(&db.conn, id, false).unwrap();
        assert!(!load(&db, id).movable);
    }

    #[test]
    fn moving_an_event_changes_only_its_times() {
        let db = Database::open_in_memory().unwrap();
        let id = make(&db);
        set_event_movable(&db.conn, id, true).unwrap();

        update_event_times(&db.conn, id, at(14, 0), at(15, 30)).unwrap();

        let e = load(&db, id);
        assert_eq!((e.start_time, e.end_time), (at(14, 0), at(15, 30)));
        assert_eq!(e.title, "Lecture");
        assert!(e.movable, "still movable");
    }

    #[test]
    fn saving_an_edit_does_not_clear_the_movable_flag() {
        let db = Database::open_in_memory().unwrap();
        let id = make(&db);
        set_event_movable(&db.conn, id, true).unwrap();
        update_event(&db.conn, id, "Renamed", "", at(10, 0), at(11, 0), None, None).unwrap();
        assert!(load(&db, id).movable);
    }
}

