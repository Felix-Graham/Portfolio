use chrono::NaiveDate;
use rusqlite::{params, Connection, Result};

/// Records one completed focus (work) session — never break phases.
/// Append-only, in the same spirit as `tasks::completion_log`: the
/// durable source of truth for "focus time" and "Pomodoros completed"
/// stats, independent of anything else's lifecycle.
pub fn log_session(conn: &Connection, date: NaiveDate, minutes: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO pomodoro_sessions (date, minutes) VALUES (?1, ?2)",
        params![date.format("%Y-%m-%d").to_string(), minutes],
    )?;
    Ok(())
}

/// Total completed Pomodoro count, all time.
pub fn count_sessions(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM pomodoro_sessions", [], |row| row.get(0))
}

/// Total focus minutes logged, all time.
pub fn total_minutes(conn: &Connection) -> Result<i64> {
    let total: Option<i64> = conn.query_row("SELECT SUM(minutes) FROM pomodoro_sessions", [], |row| row.get(0))?;
    Ok(total.unwrap_or(0))
}
