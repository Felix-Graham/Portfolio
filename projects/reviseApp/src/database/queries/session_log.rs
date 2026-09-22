use chrono::NaiveDate;
use rusqlite::{params, Connection, OptionalExtension, Result};

use crate::events::{SessionLogEntry, SessionStatus};

fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<SessionLogEntry> {
    let status_raw: String = row.get(3)?;
    let date_raw: String = row.get(2)?;
    Ok(SessionLogEntry {
        id: row.get(0)?,
        event_id: row.get(1)?,
        occurrence_date: NaiveDate::parse_from_str(&date_raw, "%Y-%m-%d").unwrap_or_default(),
        status: SessionStatus::parse(&status_raw).unwrap_or(SessionStatus::Completed),
        actual_minutes: row.get(4)?,
    })
}

const SELECT_COLUMNS: &str = "id, event_id, occurrence_date, status, actual_minutes";

/// Logs (or overwrites) the outcome of one occurrence of an event.
/// Keyed by `(event_id, occurrence_date)` — logging one occurrence
/// never touches any other, or the event/recurrence rule itself.
pub fn log_session(
    conn: &Connection,
    event_id: i64,
    date: NaiveDate,
    status: SessionStatus,
    actual_minutes: Option<i64>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO session_log (event_id, occurrence_date, status, actual_minutes)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(event_id, occurrence_date) DO UPDATE SET status = excluded.status, actual_minutes = excluded.actual_minutes",
        params![event_id, date.format("%Y-%m-%d").to_string(), status.as_str(), actual_minutes],
    )?;
    Ok(())
}

/// Reverts an occurrence back to the default (assume it happened as
/// scheduled) by removing its log entry.
pub fn clear_log(conn: &Connection, event_id: i64, date: NaiveDate) -> Result<()> {
    conn.execute(
        "DELETE FROM session_log WHERE event_id = ?1 AND occurrence_date = ?2",
        params![event_id, date.format("%Y-%m-%d").to_string()],
    )?;
    Ok(())
}

pub fn get_log(conn: &Connection, event_id: i64, date: NaiveDate) -> Result<Option<SessionLogEntry>> {
    conn.query_row(
        &format!("SELECT {SELECT_COLUMNS} FROM session_log WHERE event_id = ?1 AND occurrence_date = ?2"),
        params![event_id, date.format("%Y-%m-%d").to_string()],
        row_to_entry,
    )
    .optional()
}

/// All log entries — used to compute stats (e.g. "hours per subject")
/// across every logged occurrence in one query rather than one lookup
/// per occurrence.
pub fn list_all_logs(conn: &Connection) -> Result<Vec<SessionLogEntry>> {
    let mut stmt = conn.prepare(&format!("SELECT {SELECT_COLUMNS} FROM session_log"))?;
    let entries = stmt.query_map([], row_to_entry)?.collect::<Result<Vec<_>>>()?;
    Ok(entries)
}
