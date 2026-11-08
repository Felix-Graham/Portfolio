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

/// Where a logged focus session came from. Feature request: "Focus
/// time should accept data from the stopwatch not just pomodoro" —
/// the log (`pomodoro_sessions`, kept under its original name) now
/// takes the stopwatch and the new countdown timer too, tagged with
/// their source so "Pomodoros completed" can still count only real
/// Pomodoros while "Focus time" sums everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusSource {
    Pomodoro,
    Stopwatch,
    Timer,
}

impl FocusSource {
    pub fn as_str(self) -> &'static str {
        match self {
            FocusSource::Pomodoro => "pomodoro",
            FocusSource::Stopwatch => "stopwatch",
            FocusSource::Timer => "timer",
        }
    }
}

/// Records a completed focus block from any source. A block of zero
/// (or negative) minutes — e.g. a stopwatch stopped after a few
/// seconds — is not worth a row and is skipped, so it can't inflate a
/// count. `log_session` is the Pomodoro shorthand for this.
pub fn log_focus(conn: &Connection, date: NaiveDate, minutes: i64, source: FocusSource) -> Result<()> {
    if minutes < 1 {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO pomodoro_sessions (date, minutes, source) VALUES (?1, ?2, ?3)",
        params![date.format("%Y-%m-%d").to_string(), minutes, source.as_str()],
    )?;
    Ok(())
}

/// Records a finished timer or stopwatch run — the one place the two do
/// it. The minutes always count toward Focus time ([`log_focus`]; under a
/// minute is skipped there). If the run has a subject it also becomes a
/// one-off Calendar event with a completed session-log entry, which is the
/// pipeline `stats::minutes_per_subject` already sums, so subject time has
/// a single source rather than a second one.
///
/// `logged_minutes` is what Focus time gets; `event_minutes` is the length
/// of the event (the stopwatch gives a sub-minute run a one-minute event so
/// it still has a span). A run linked to a planned time block passes that
/// block's task's subject here, which is how time in a block is added to
/// the stats.
///
/// `hidden` keeps that event off the calendar (the stats still count it): a
/// run started against a planned time block passes `true`, because the block
/// already shows the time and a second card for the same minutes would only
/// clutter the day. Both writes are attempted; the first error, if any, is
/// returned.
#[allow(clippy::too_many_arguments)]
pub fn record_focus_run(
    conn: &Connection,
    source: FocusSource,
    title: &str,
    subject_id: Option<i64>,
    logged_minutes: i64,
    event_minutes: i64,
    end: chrono::DateTime<chrono::Local>,
    hidden: bool,
) -> Result<()> {
    let focus = log_focus(conn, end.date_naive(), logged_minutes, source);

    let mut subject_result = Ok(());
    if let Some(subject_id) = subject_id {
        let start = end - chrono::Duration::minutes(event_minutes);
        subject_result = super::events::create_event(conn, title, "", start, end, Some(subject_id), None).and_then(
            |event| {
                if hidden {
                    super::events::set_event_hidden(conn, event.id, true)?;
                }
                super::session_log::log_session(
                    conn,
                    event.id,
                    event.day(),
                    crate::events::SessionStatus::Completed,
                    None,
                )
                .map(|_| ())
            },
        );
    }
    focus.and(subject_result)
}

/// Total completed Pomodoro count, all time — Pomodoros only, not
/// stopwatch or timer sessions (those add to focus *time*, but they
/// aren't Pomodoros).
pub fn count_sessions(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM pomodoro_sessions WHERE source = 'pomodoro'", [], |row| row.get(0))
}

/// Total focus minutes logged, all time, from every source (Pomodoro,
/// stopwatch and timer).
pub fn total_minutes(conn: &Connection) -> Result<i64> {
    let total: Option<i64> = conn.query_row("SELECT SUM(minutes) FROM pomodoro_sessions", [], |row| row.get(0))?;
    Ok(total.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()
    }

    #[test]
    fn focus_time_sums_every_source_but_pomodoros_completed_counts_only_pomodoros() {
        let db = Database::open_in_memory().expect("open in-memory db");
        log_session(&db.conn, day(), 25).expect("pomodoro");
        log_focus(&db.conn, day(), 40, FocusSource::Stopwatch).expect("stopwatch");
        log_focus(&db.conn, day(), 10, FocusSource::Timer).expect("timer");

        assert_eq!(total_minutes(&db.conn).expect("total"), 75);
        assert_eq!(count_sessions(&db.conn).expect("count"), 1);
    }

    #[test]
    fn a_zero_minute_block_is_not_logged() {
        let db = Database::open_in_memory().expect("open in-memory db");
        log_focus(&db.conn, day(), 0, FocusSource::Stopwatch).expect("zero");
        log_focus(&db.conn, day(), -5, FocusSource::Timer).expect("negative");
        assert_eq!(total_minutes(&db.conn).expect("total"), 0);
        let rows: i64 = db.conn.query_row("SELECT COUNT(*) FROM pomodoro_sessions", [], |r| r.get(0)).expect("rows");
        assert_eq!(rows, 0);
    }

    #[test]
    fn rows_logged_before_the_source_column_existed_count_as_pomodoros() {
        // A pre-v26 row has no source; the column default must make it
        // a Pomodoro so existing history is unchanged.
        let db = Database::open_in_memory().expect("open in-memory db");
        db.conn
            .execute("INSERT INTO pomodoro_sessions (date, minutes) VALUES ('2026-01-01', 25)", [])
            .expect("legacy-shaped insert");
        assert_eq!(count_sessions(&db.conn).expect("count"), 1);
        assert_eq!(total_minutes(&db.conn).expect("total"), 25);
    }
}
