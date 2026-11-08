use chrono::{Local, NaiveDate};
use rusqlite::{params, Connection, Result};

use crate::sources::ExternalContribution;

fn row_to_contribution(row: &rusqlite::Row) -> rusqlite::Result<ExternalContribution> {
    let occurred_on_raw: String = row.get(3)?;
    Ok(ExternalContribution {
        id: row.get(0)?,
        source_id: row.get(1)?,
        subject_id: row.get(2)?,
        occurred_on: NaiveDate::parse_from_str(&occurred_on_raw, "%Y-%m-%d").unwrap_or_default(),
        minutes: row.get(4)?,
        note: row.get(5)?,
        created_at: row.get(6)?,
    })
}

const SELECT_COLUMNS: &str = "id, source_id, subject_id, occurred_on, minutes, note, created_at";

/// Every contribution ever logged, across every source — used by
/// `stats::minutes_per_subject_in_range` to fold them into a
/// subject's total, the same way it already reads
/// `queries::tasks::list_tasks` in full and filters in Rust rather
/// than adding a `WHERE` clause per call site.
pub fn list_all_contributions(conn: &Connection) -> Result<Vec<ExternalContribution>> {
    let mut stmt = conn.prepare(&format!("SELECT {SELECT_COLUMNS} FROM external_contributions"))?;
    let contributions = stmt.query_map([], row_to_contribution)?.collect::<Result<Vec<_>>>()?;
    Ok(contributions)
}

/// A single source's own history — "Manage Sources" shows each
/// source's all-time total and lets a contribution be deleted
/// individually (e.g. a mis-logged amount) without needing to see
/// every other source's entries at the same time.
pub fn list_contributions_for_source(conn: &Connection, source_id: i64) -> Result<Vec<ExternalContribution>> {
    let mut stmt =
        conn.prepare(&format!("SELECT {SELECT_COLUMNS} FROM external_contributions WHERE source_id = ?1"))?;
    let contributions = stmt.query_map(params![source_id], row_to_contribution)?.collect::<Result<Vec<_>>>()?;
    Ok(contributions)
}

/// Logs one block of time (direct request: "It would add hours/
/// contribute to statistics"). `occurred_on` is the caller's own
/// choice, not always "today" — `revise-cli log` defaults to today but
/// accepts `--date` for logging something after the fact, the same
/// flexibility `session_log`'s own date parameter already has.
pub fn create_contribution(
    conn: &Connection,
    source_id: i64,
    subject_id: i64,
    occurred_on: NaiveDate,
    minutes: i64,
    note: Option<&str>,
) -> Result<ExternalContribution> {
    let created_at = Local::now().to_rfc3339();
    conn.execute(
        "INSERT INTO external_contributions (source_id, subject_id, occurred_on, minutes, note, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![source_id, subject_id, occurred_on.format("%Y-%m-%d").to_string(), minutes, note, created_at],
    )?;
    Ok(ExternalContribution {
        id: conn.last_insert_rowid(),
        source_id,
        subject_id,
        occurred_on,
        minutes,
        note: note.map(str::to_owned),
        created_at,
    })
}

pub fn delete_contribution(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM external_contributions WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::queries::{sources, subjects};
    use crate::database::Database;

    fn setup(db: &Database) -> (i64, i64) {
        let subject = subjects::create_subject(&db.conn, "French").expect("create subject");
        let source = sources::create_source(&db.conn, "French Quiz").expect("create source");
        (source.id, subject.id)
    }

    #[test]
    fn create_contribution_round_trips_every_field() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let (source_id, subject_id) = setup(&db);
        let date = NaiveDate::from_ymd_opt(2026, 9, 27).unwrap();

        let contribution =
            create_contribution(&db.conn, source_id, subject_id, date, 25, Some("Chapter 4 quiz")).expect("create");

        assert_eq!(contribution.source_id, source_id);
        assert_eq!(contribution.subject_id, subject_id);
        assert_eq!(contribution.occurred_on, date);
        assert_eq!(contribution.minutes, 25);
        assert_eq!(contribution.note.as_deref(), Some("Chapter 4 quiz"));

        let all = list_all_contributions(&db.conn).expect("list");
        assert_eq!(all, vec![contribution]);
    }

    #[test]
    fn create_contribution_accepts_no_note() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let (source_id, subject_id) = setup(&db);

        let contribution = create_contribution(
            &db.conn,
            source_id,
            subject_id,
            NaiveDate::from_ymd_opt(2026, 9, 27).unwrap(),
            10,
            None,
        )
        .expect("create");

        assert_eq!(contribution.note, None);
    }

    #[test]
    fn list_contributions_for_source_only_returns_that_sources_own_entries() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let (source_id, subject_id) = setup(&db);
        let other_source = sources::create_source(&db.conn, "Anki").expect("create other source");
        let date = NaiveDate::from_ymd_opt(2026, 9, 27).unwrap();

        create_contribution(&db.conn, source_id, subject_id, date, 25, None).expect("create");
        create_contribution(&db.conn, other_source.id, subject_id, date, 15, None).expect("create");

        let for_source = list_contributions_for_source(&db.conn, source_id).expect("list");
        assert_eq!(for_source.len(), 1);
        assert_eq!(for_source[0].source_id, source_id);
    }

    #[test]
    fn delete_contribution_removes_only_that_entry() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let (source_id, subject_id) = setup(&db);
        let date = NaiveDate::from_ymd_opt(2026, 9, 27).unwrap();

        let first = create_contribution(&db.conn, source_id, subject_id, date, 25, None).expect("create");
        let second = create_contribution(&db.conn, source_id, subject_id, date, 15, None).expect("create");

        delete_contribution(&db.conn, first.id).expect("delete");

        let remaining = list_all_contributions(&db.conn).expect("list");
        assert_eq!(remaining, vec![second]);
    }
}
