use chrono::Local;
use rusqlite::{params, Connection, OptionalExtension, Result};

use crate::notes::Note;

fn row_to_note(row: &rusqlite::Row) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        subject_id: row.get(3)?,
        event_id: row.get(4)?,
        folder_id: row.get(5)?,
        is_flashcard_set: row.get::<_, i64>(6)? != 0,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

const SELECT_COLUMNS: &str = "id, title, content, subject_id, event_id, folder_id, is_flashcard_set, created_at, updated_at";

/// All notes, most recently updated first — the natural default for
/// a notes list (unlike tasks/events, notes have no other inherent
/// order).
pub fn list_all_notes(conn: &Connection) -> Result<Vec<Note>> {
    let mut stmt = conn.prepare(&format!("SELECT {SELECT_COLUMNS} FROM notes ORDER BY updated_at DESC"))?;
    let notes = stmt.query_map([], row_to_note)?.collect::<Result<Vec<_>>>()?;
    Ok(notes)
}

/// A single note by id — used to resolve a flashcard-set note into a
/// study session (see `ui::flashcards::start_study_session`), where
/// only the one picked note is needed rather than the whole list.
pub fn get_note(conn: &Connection, id: i64) -> Result<Option<Note>> {
    conn.query_row(&format!("SELECT {SELECT_COLUMNS} FROM notes WHERE id = ?1"), params![id], row_to_note).optional()
}

/// Every note flagged as a flashcard set (feature request: "Built-In
/// Flashcard Editor") — the second source (alongside imported decks)
/// `ui::flashcards::available_study_sets` merges into one studyable
/// list.
pub fn list_flashcard_set_notes(conn: &Connection) -> Result<Vec<Note>> {
    let mut stmt =
        conn.prepare(&format!("SELECT {SELECT_COLUMNS} FROM notes WHERE is_flashcard_set = 1 ORDER BY updated_at DESC"))?;
    let notes = stmt.query_map([], row_to_note)?.collect::<Result<Vec<_>>>()?;
    Ok(notes)
}

/// Notes attached to a specific revision session — "opening a
/// revision session can show related notes" (Sprint 8's acceptance
/// criterion).
pub fn list_notes_for_event(conn: &Connection, event_id: i64) -> Result<Vec<Note>> {
    let mut stmt =
        conn.prepare(&format!("SELECT {SELECT_COLUMNS} FROM notes WHERE event_id = ?1 ORDER BY updated_at DESC"))?;
    let notes = stmt.query_map(params![event_id], row_to_note)?.collect::<Result<Vec<_>>>()?;
    Ok(notes)
}

/// Notes in a specific folder (feature request: "Notes Folders") —
/// used for a database-level round-trip test; the Notes list itself
/// filters `list_all_notes` in memory instead, consistent with how
/// the subject filter beside it already works.
#[cfg(test)]
pub fn list_notes_in_folder(conn: &Connection, folder_id: i64) -> Result<Vec<Note>> {
    let mut stmt =
        conn.prepare(&format!("SELECT {SELECT_COLUMNS} FROM notes WHERE folder_id = ?1 ORDER BY updated_at DESC"))?;
    let notes = stmt.query_map(params![folder_id], row_to_note)?.collect::<Result<Vec<_>>>()?;
    Ok(notes)
}

pub fn create_note(
    conn: &Connection,
    title: &str,
    content: &str,
    subject_id: Option<i64>,
    event_id: Option<i64>,
    folder_id: Option<i64>,
) -> Result<Note> {
    let now = Local::now().to_rfc3339();
    conn.execute(
        "INSERT INTO notes (title, content, subject_id, event_id, folder_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![title, content, subject_id, event_id, folder_id, now],
    )?;

    Ok(Note {
        id: conn.last_insert_rowid(),
        title: title.to_owned(),
        content: content.to_owned(),
        subject_id,
        event_id,
        folder_id,
        is_flashcard_set: false,
        created_at: now.clone(),
        updated_at: now,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn update_note(
    conn: &Connection,
    id: i64,
    title: &str,
    content: &str,
    subject_id: Option<i64>,
    event_id: Option<i64>,
    folder_id: Option<i64>,
) -> Result<()> {
    let now = Local::now().to_rfc3339();
    conn.execute(
        "UPDATE notes SET title = ?1, content = ?2, subject_id = ?3, event_id = ?4, folder_id = ?5, updated_at = ?6
         WHERE id = ?7",
        params![title, content, subject_id, event_id, folder_id, now, id],
    )?;
    Ok(())
}

/// Moves a note into (`Some`) or out of (`None`) a folder without
/// touching anything else about it — the fast path the Notes list's
/// folder picker uses, as distinct from a full `update_note` edit.
pub fn set_folder(conn: &Connection, id: i64, folder_id: Option<i64>) -> Result<()> {
    conn.execute("UPDATE notes SET folder_id = ?1 WHERE id = ?2", params![folder_id, id])?;
    Ok(())
}

/// Flags or unflags a note as a flashcard set (feature request:
/// "Built-In Flashcard Editor") without touching anything else about
/// it — a fast-path setter (per this project's usual convention for a
/// field most rows never use) rather than a new parameter on
/// `create_note`/`update_note`, which would ripple through their
/// other call sites (the Calendar's auto-note-from-event, in
/// particular) that have no reason to know about it.
pub fn set_is_flashcard_set(conn: &Connection, id: i64, is_flashcard_set: bool) -> Result<()> {
    conn.execute("UPDATE notes SET is_flashcard_set = ?1 WHERE id = ?2", params![is_flashcard_set as i64, id])?;
    Ok(())
}

pub fn delete_note(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM notes WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    #[test]
    fn list_notes_in_folder_only_returns_that_folders_notes() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let folder = queries_folders_create(&db.conn, "Physics", "#5A5A5A");
        create_note(&db.conn, "In folder", "", None, None, Some(folder)).expect("create");
        create_note(&db.conn, "Unfiled", "", None, None, None).expect("create");

        let notes = list_notes_in_folder(&db.conn, folder).expect("list");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].title, "In folder");
    }

    fn queries_folders_create(conn: &Connection, name: &str, color: &str) -> i64 {
        crate::database::queries::folders::create_folder(conn, name, color).expect("create folder").id
    }

    #[test]
    fn get_note_finds_it_by_id_and_none_for_a_missing_one() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let note = create_note(&db.conn, "Kinematics", "v = u + at", None, None, None).expect("create");

        let found = get_note(&db.conn, note.id).expect("get").expect("should exist");
        assert_eq!(found.title, "Kinematics");
        assert!(get_note(&db.conn, note.id + 999).expect("get missing").is_none());
    }

    #[test]
    fn new_notes_are_not_flashcard_sets_by_default() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let note = create_note(&db.conn, "Plain note", "", None, None, None).expect("create");
        assert!(!note.is_flashcard_set);
        assert!(list_flashcard_set_notes(&db.conn).expect("list").is_empty());
    }

    #[test]
    fn set_is_flashcard_set_flags_it_and_it_shows_up_in_the_list() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let note = create_note(&db.conn, "French Vocab", "le chat\nthe cat", None, None, None).expect("create");

        set_is_flashcard_set(&db.conn, note.id, true).expect("flag as flashcard set");

        let sets = list_flashcard_set_notes(&db.conn).expect("list");
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].id, note.id);
        assert!(get_note(&db.conn, note.id).expect("get").expect("exists").is_flashcard_set);

        set_is_flashcard_set(&db.conn, note.id, false).expect("unflag");
        assert!(list_flashcard_set_notes(&db.conn).expect("list").is_empty());
    }
}
