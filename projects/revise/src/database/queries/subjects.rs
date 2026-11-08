use chrono::Local;
use rusqlite::{params, Connection, OptionalExtension, Result};

use crate::subjects::Subject;

fn row_to_subject(row: &rusqlite::Row) -> rusqlite::Result<Subject> {
    Ok(Subject {
        id: row.get(0)?,
        name: row.get(1)?,
        archived: row.get::<_, i64>(2)? != 0,
        created_at: row.get(3)?,
        parent_subject_id: row.get(4)?,
        color: row.get(5)?,
    })
}

const SELECT_COLUMNS: &str = "id, name, archived, created_at, parent_subject_id, color";

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

/// Case-insensitive, same reasoning as
/// `queries::sources::find_source_by_name` — used by `revise-cli log
/// --subject <name>` (direct request: "an API"), where a person typing
/// a flag by hand is far more likely to slip on case than to have
/// genuinely meant a different subject that only differs by it.
pub fn find_subject_by_name(conn: &Connection, name: &str) -> Result<Option<Subject>> {
    conn.query_row(
        &format!("SELECT {SELECT_COLUMNS} FROM subjects WHERE LOWER(name) = LOWER(?1)"),
        params![name],
        row_to_subject,
    )
    .optional()
}

pub fn create_subject(conn: &Connection, name: &str) -> Result<Subject> {
    let created_at = Local::now().to_rfc3339();
    conn.execute(
        "INSERT INTO subjects (name, archived, created_at) VALUES (?1, 0, ?2)",
        params![name, created_at],
    )?;
    Ok(Subject {
        id: conn.last_insert_rowid(),
        name: name.to_owned(),
        archived: false,
        created_at,
        parent_subject_id: None,
        color: None,
    })
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
        color: None,
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

/// Sets a subject's own explicit color (feature request: "Colour" —
/// "the user should be able to pick the colour of the subject"), as a
/// `"#rrggbb"` string (see `theme::color_to_hex`). A fast-path setter
/// of its own, same precedent as `set_archived` right above, rather
/// than a parameter on `create_subject`/`create_subtopic` — most
/// subjects never have this touched, and every existing caller of
/// those two functions only ever creates one without a color chosen
/// yet.
pub fn set_color(conn: &Connection, id: i64, color: &str) -> Result<()> {
    conn.execute("UPDATE subjects SET color = ?1 WHERE id = ?2", params![color, id])?;
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
    fn find_subject_by_name_is_case_insensitive() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let subject = create_subject(&db.conn, "French").expect("create");
        assert_eq!(find_subject_by_name(&db.conn, "french").expect("find"), Some(subject));
    }

    #[test]
    fn find_subject_by_name_returns_none_when_it_does_not_exist() {
        let db = Database::open_in_memory().expect("open in-memory db");
        assert_eq!(find_subject_by_name(&db.conn, "Nonexistent").expect("find"), None);
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
    fn new_subject_has_no_explicit_color_by_default() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let subject = create_subject(&db.conn, "Physics").expect("create");
        assert_eq!(subject.color, None);
    }

    #[test]
    fn set_color_persists_and_is_read_back() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let subject = create_subject(&db.conn, "Physics").expect("create");
        set_color(&db.conn, subject.id, "#a1b2c3").expect("set color");

        let reloaded = list_all_subjects(&db.conn).expect("list");
        let physics = reloaded.iter().find(|s| s.id == subject.id).expect("subject still present");
        assert_eq!(physics.color.as_deref(), Some("#a1b2c3"));
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
