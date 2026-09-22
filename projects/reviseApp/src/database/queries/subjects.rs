use chrono::Local;
use rusqlite::{params, Connection, Result};

use crate::subjects::Subject;

fn row_to_subject(row: &rusqlite::Row) -> rusqlite::Result<Subject> {
    Ok(Subject {
        id: row.get(0)?,
        name: row.get(1)?,
        archived: row.get::<_, i64>(2)? != 0,
        created_at: row.get(3)?,
        parent_subject_id: row.get(4)?,
    })
}

const SELECT_COLUMNS: &str = "id, name, archived, created_at, parent_subject_id";

/// All subjects, active and archived, alphabetical — the list used
/// wherever a picker needs to show (and correctly label) an archived
/// subject that's still attached to older data.
pub fn list_all_subjects(conn: &Connection) -> Result<Vec<Subject>> {
    let mut stmt = conn.prepare(&format!("SELECT {SELECT_COLUMNS} FROM subjects ORDER BY name ASC"))?;
    let subjects = stmt.query_map([], row_to_subject)?.collect::<Result<Vec<_>>>()?;
    Ok(subjects)
}

/// Only non-archived subjects — used for "assign a subject to new
/// work" pickers, so archived ones can't be picked for anything new
/// while still showing up on whatever they're already attached to.
pub fn list_active_subjects(conn: &Connection) -> Result<Vec<Subject>> {
    let mut stmt =
        conn.prepare(&format!("SELECT {SELECT_COLUMNS} FROM subjects WHERE archived = 0 ORDER BY name ASC"))?;
    let subjects = stmt.query_map([], row_to_subject)?.collect::<Result<Vec<_>>>()?;
    Ok(subjects)
}

pub fn create_subject(conn: &Connection, name: &str) -> Result<Subject> {
    let created_at = Local::now().to_rfc3339();
    conn.execute(
        "INSERT INTO subjects (name, archived, created_at) VALUES (?1, 0, ?2)",
        params![name, created_at],
    )?;
    Ok(Subject { id: conn.last_insert_rowid(), name: name.to_owned(), archived: false, created_at, parent_subject_id: None })
}

/// Creates a subtopic under `parent_id` (feature request: "Subject
/// Sub Topics") — kept separate from `create_subject` rather than
/// adding a parameter to it, since every other caller only ever
/// creates top-level subjects.
pub fn create_subtopic(conn: &Connection, parent_id: i64, name: &str) -> Result<Subject> {
    let created_at = Local::now().to_rfc3339();
    conn.execute(
        "INSERT INTO subjects (name, archived, created_at, parent_subject_id) VALUES (?1, 0, ?2, ?3)",
        params![name, created_at, parent_id],
    )?;
    Ok(Subject {
        id: conn.last_insert_rowid(),
        name: name.to_owned(),
        archived: false,
        created_at,
        parent_subject_id: Some(parent_id),
    })
}

pub fn rename_subject(conn: &Connection, id: i64, new_name: &str) -> Result<()> {
    conn.execute("UPDATE subjects SET name = ?1 WHERE id = ?2", params![new_name, id])?;
    Ok(())
}

pub fn set_archived(conn: &Connection, id: i64, archived: bool) -> Result<()> {
    conn.execute("UPDATE subjects SET archived = ?1 WHERE id = ?2", params![archived as i64, id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    #[test]
    fn create_subject_defaults_to_top_level() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let subject = create_subject(&db.conn, "Physics").expect("create");
        assert_eq!(subject.parent_subject_id, None);
    }

    #[test]
    fn create_subtopic_is_nested_under_its_parent() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let physics = create_subject(&db.conn, "Physics").expect("create parent");
        let mechanics = create_subtopic(&db.conn, physics.id, "Mechanics").expect("create subtopic");

        assert_eq!(mechanics.parent_subject_id, Some(physics.id));
        let all = list_all_subjects(&db.conn).expect("list");
        assert_eq!(all.iter().find(|s| s.id == mechanics.id).and_then(|s| s.parent_subject_id), Some(physics.id));
    }

    #[test]
    fn subtopics_show_up_in_the_same_lists_as_any_other_subject() {
        // Feature request's own wording: subtopics should be
        // "referenced as their own subjects" — this is what makes
        // that true everywhere else in the app with no further
        // changes: every picker just lists this table.
        let db = Database::open_in_memory().expect("open in-memory db");
        let physics = create_subject(&db.conn, "Physics").expect("create parent");
        create_subtopic(&db.conn, physics.id, "Mechanics").expect("create subtopic");

        assert_eq!(list_all_subjects(&db.conn).expect("list all").len(), 2);
        assert_eq!(list_active_subjects(&db.conn).expect("list active").len(), 2);
    }
}
