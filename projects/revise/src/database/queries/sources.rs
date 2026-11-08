use chrono::Local;
use rusqlite::{params, Connection, OptionalExtension, Result};

use crate::sources::Source;

fn row_to_source(row: &rusqlite::Row) -> rusqlite::Result<Source> {
    Ok(Source { id: row.get(0)?, name: row.get(1)?, created_at: row.get(2)? })
}

const SELECT_COLUMNS: &str = "id, name, created_at";

/// All registered sources, oldest first — matches "Manage Sources"'
/// own listing order (a person's most-recently-registered tool ending
/// up at the bottom of a short list is easier to spot than one jumping
/// around alphabetically as new ones are added).
pub fn list_all_sources(conn: &Connection) -> Result<Vec<Source>> {
    let mut stmt = conn.prepare(&format!("SELECT {SELECT_COLUMNS} FROM sources ORDER BY id ASC"))?;
    let sources = stmt.query_map([], row_to_source)?.collect::<Result<Vec<_>>>()?;
    Ok(sources)
}

pub fn find_source(conn: &Connection, id: i64) -> Result<Option<Source>> {
    conn.query_row(&format!("SELECT {SELECT_COLUMNS} FROM sources WHERE id = ?1"), params![id], row_to_source)
        .optional()
}

/// Case-insensitive — `revise-cli register "French Quiz"` followed by
/// `revise-cli log --source "french quiz" ...` should resolve, since a
/// person typing a `--source` flag by hand is far more likely to slip
/// on case than to have genuinely meant a *different* tool that only
/// differs by it.
pub fn find_source_by_name(conn: &Connection, name: &str) -> Result<Option<Source>> {
    conn.query_row(
        &format!("SELECT {SELECT_COLUMNS} FROM sources WHERE LOWER(name) = LOWER(?1)"),
        params![name],
        row_to_source,
    )
    .optional()
}

pub fn create_source(conn: &Connection, name: &str) -> Result<Source> {
    let created_at = Local::now().to_rfc3339();
    conn.execute("INSERT INTO sources (name, created_at) VALUES (?1, ?2)", params![name, created_at])?;
    Ok(Source { id: conn.last_insert_rowid(), name: name.to_owned(), created_at })
}

pub fn rename_source(conn: &Connection, id: i64, new_name: &str) -> Result<()> {
    conn.execute("UPDATE sources SET name = ?1 WHERE id = ?2", params![new_name, id])?;
    Ok(())
}

/// Deletes a source and every contribution it ever logged. Not a
/// `FOREIGN KEY ... ON DELETE CASCADE` — this app never turns on
/// `PRAGMA foreign_keys` (its `REFERENCES` clauses elsewhere are
/// documentation, not enforcement; see the migration-fixture bug
/// fixed in the seventh `issues.md` pass for exactly what trusting
/// that not to matter can cost), so cascading has to be done
/// explicitly, deleting the child rows first.
pub fn delete_source(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM external_contributions WHERE source_id = ?1", params![id])?;
    conn.execute("DELETE FROM sources WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    #[test]
    fn create_source_can_be_found_by_id_and_by_name() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let source = create_source(&db.conn, "French Quiz").expect("create");

        assert_eq!(find_source(&db.conn, source.id).expect("find by id"), Some(source.clone()));
        assert_eq!(find_source_by_name(&db.conn, "French Quiz").expect("find by name"), Some(source));
    }

    #[test]
    fn find_source_by_name_is_case_insensitive() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let source = create_source(&db.conn, "French Quiz").expect("create");

        assert_eq!(find_source_by_name(&db.conn, "french quiz").expect("find"), Some(source));
    }

    #[test]
    fn find_source_by_name_returns_none_when_unregistered() {
        let db = Database::open_in_memory().expect("open in-memory db");
        assert_eq!(find_source_by_name(&db.conn, "Nonexistent").expect("find"), None);
    }

    #[test]
    fn source_names_are_unique() {
        let db = Database::open_in_memory().expect("open in-memory db");
        create_source(&db.conn, "French Quiz").expect("create");
        assert!(create_source(&db.conn, "French Quiz").is_err());
    }

    #[test]
    fn list_all_sources_is_ordered_oldest_first() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let first = create_source(&db.conn, "First").expect("create");
        let second = create_source(&db.conn, "Second").expect("create");

        let all = list_all_sources(&db.conn).expect("list");
        assert_eq!(all.iter().map(|s| s.id).collect::<Vec<_>>(), vec![first.id, second.id]);
    }

    #[test]
    fn rename_source_persists() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let source = create_source(&db.conn, "French Quiz").expect("create");
        rename_source(&db.conn, source.id, "French Vocab Quiz").expect("rename");

        assert_eq!(find_source(&db.conn, source.id).expect("find").map(|s| s.name), Some("French Vocab Quiz".to_owned()));
    }

    #[test]
    fn delete_source_also_deletes_its_contributions() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let subject = crate::database::queries::subjects::create_subject(&db.conn, "French").expect("create subject");
        let source = create_source(&db.conn, "French Quiz").expect("create source");
        crate::database::queries::contributions::create_contribution(
            &db.conn,
            source.id,
            subject.id,
            chrono::NaiveDate::from_ymd_opt(2026, 9, 27).unwrap(),
            25,
            None,
        )
        .expect("create contribution");

        delete_source(&db.conn, source.id).expect("delete");

        assert_eq!(find_source(&db.conn, source.id).expect("find"), None);
        assert!(crate::database::queries::contributions::list_all_contributions(&db.conn).expect("list").is_empty());
    }
}
