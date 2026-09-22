use chrono::Local;
use rusqlite::{params, Connection, OptionalExtension, Result};

use crate::folders::Folder;

/// The reserved name of the single top-level folder flashcard-set
/// notes' subject subfolders live under (feature request: "Built-In
/// Flashcard Editor"). Matched case-sensitively and only among
/// top-level folders — a person could rename it, same caveat as any
/// other user-editable folder name; nothing re-creates a second one
/// automatically unless a lookup genuinely finds none.
pub const FLASHCARDS_ROOT_NAME: &str = "Flashcards";

fn row_to_folder(row: &rusqlite::Row) -> rusqlite::Result<Folder> {
    Ok(Folder { id: row.get(0)?, name: row.get(1)?, color: row.get(2)?, parent_id: row.get(3)?, created_at: row.get(4)? })
}

const SELECT_COLUMNS: &str = "id, name, color, parent_id, created_at";

/// All folders, alphabetical, top-level and subfolders alike — the
/// only listing folders need, unlike subjects there's no archived/
/// active distinction requested here. Callers that only want
/// top-level folders (the Notes tab row) or one folder's subfolders
/// (a flashcard-set note's subject picker) filter this in memory,
/// same as how the subject filter beside it already works — this
/// app's folder count doesn't justify two more queries.
pub fn list_all_folders(conn: &Connection) -> Result<Vec<Folder>> {
    let mut stmt = conn.prepare(&format!("SELECT {SELECT_COLUMNS} FROM folders ORDER BY name ASC"))?;
    let folders = stmt.query_map([], row_to_folder)?.collect::<Result<Vec<_>>>()?;
    Ok(folders)
}

pub fn create_folder(conn: &Connection, name: &str, color: &str) -> Result<Folder> {
    let created_at = Local::now().to_rfc3339();
    conn.execute(
        "INSERT INTO folders (name, color, created_at) VALUES (?1, ?2, ?3)",
        params![name, color, created_at],
    )?;
    Ok(Folder { id: conn.last_insert_rowid(), name: name.to_owned(), color: color.to_owned(), parent_id: None, created_at })
}

/// Creates a subfolder under `parent_id` — used for a flashcard-set
/// note's subject/sub-topic subfolders. Kept separate from
/// `create_folder` (rather than adding a parameter to it) since every
/// other caller only ever creates top-level folders.
pub fn create_subfolder(conn: &Connection, name: &str, color: &str, parent_id: i64) -> Result<Folder> {
    let created_at = Local::now().to_rfc3339();
    conn.execute(
        "INSERT INTO folders (name, color, parent_id, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![name, color, parent_id, created_at],
    )?;
    Ok(Folder {
        id: conn.last_insert_rowid(),
        name: name.to_owned(),
        color: color.to_owned(),
        parent_id: Some(parent_id),
        created_at,
    })
}

/// Finds the single top-level "Flashcards" folder every flashcard-set
/// note's subject subfolder lives under, creating it if this is the
/// first one anyone's needed. Per the request's own wording ("stored
/// in a new folder called flashcards, with subfolders for each
/// subject/sub-topic") — this is that one folder, created lazily
/// rather than unconditionally at startup, since plenty of installs
/// will never use flashcard-set notes at all.
pub fn find_or_create_flashcards_root(conn: &Connection) -> Result<Folder> {
    let existing = conn
        .query_row(
            &format!("SELECT {SELECT_COLUMNS} FROM folders WHERE parent_id IS NULL AND name = ?1 LIMIT 1"),
            params![FLASHCARDS_ROOT_NAME],
            row_to_folder,
        )
        .optional()?;

    match existing {
        Some(folder) => Ok(folder),
        // Gray, matching the rest of the app's deliberately muted
        // folder-colour palette (`ui::notes::FOLDER_SWATCHES`).
        None => create_folder(conn, FLASHCARDS_ROOT_NAME, "#5A5A5A"),
    }
}

pub fn rename_folder(conn: &Connection, id: i64, new_name: &str) -> Result<()> {
    conn.execute("UPDATE folders SET name = ?1 WHERE id = ?2", params![new_name, id])?;
    Ok(())
}

pub fn recolor_folder(conn: &Connection, id: i64, new_color: &str) -> Result<()> {
    conn.execute("UPDATE folders SET color = ?1 WHERE id = ?2", params![new_color, id])?;
    Ok(())
}

/// Deletes the folder and detaches (rather than deletes) any notes in
/// it — a folder is meant to be an organizing label, not a
/// container whose removal should take its contents down with it.
/// Any subfolders are promoted to top-level rather than deleted too,
/// for the same reason.
pub fn delete_folder(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("UPDATE notes SET folder_id = NULL WHERE folder_id = ?1", params![id])?;
    conn.execute("UPDATE folders SET parent_id = NULL WHERE parent_id = ?1", params![id])?;
    conn.execute("DELETE FROM folders WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    #[test]
    fn create_then_list_returns_the_new_folder() {
        let db = Database::open_in_memory().expect("open in-memory db");
        create_folder(&db.conn, "Physics", "#5A5A5A").expect("create");
        let folders = list_all_folders(&db.conn).expect("list");
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Physics");
        assert_eq!(folders[0].color, "#5A5A5A");
    }

    #[test]
    fn rename_and_recolor_update_in_place() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let folder = create_folder(&db.conn, "Physics", "#5A5A5A").expect("create");
        rename_folder(&db.conn, folder.id, "Physics 101").expect("rename");
        recolor_folder(&db.conn, folder.id, "#6E7681").expect("recolor");
        let folders = list_all_folders(&db.conn).expect("list");
        assert_eq!(folders[0].name, "Physics 101");
        assert_eq!(folders[0].color, "#6E7681");
    }

    #[test]
    fn deleting_a_folder_detaches_its_notes_instead_of_deleting_them() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let folder = create_folder(&db.conn, "Physics", "#5A5A5A").expect("create");
        let note = crate::database::queries::notes::create_note(&db.conn, "Kinematics", "", None, None, None)
            .expect("create note");
        crate::database::queries::notes::set_folder(&db.conn, note.id, Some(folder.id)).expect("set folder");

        delete_folder(&db.conn, folder.id).expect("delete folder");

        let folders = list_all_folders(&db.conn).expect("list folders");
        assert!(folders.is_empty());
        let notes = crate::database::queries::notes::list_all_notes(&db.conn).expect("list notes");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].folder_id, None);
    }

    #[test]
    fn create_folder_defaults_to_top_level() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let folder = create_folder(&db.conn, "Physics", "#5A5A5A").expect("create");
        assert_eq!(folder.parent_id, None);
    }

    #[test]
    fn create_subfolder_is_nested_under_its_parent() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let root = create_folder(&db.conn, "Flashcards", "#5A5A5A").expect("create root");
        let child = create_subfolder(&db.conn, "French", "#5A5A5A", root.id).expect("create subfolder");

        assert_eq!(child.parent_id, Some(root.id));
        let folders = list_all_folders(&db.conn).expect("list");
        assert_eq!(folders.iter().find(|f| f.id == child.id).and_then(|f| f.parent_id), Some(root.id));
    }

    #[test]
    fn find_or_create_flashcards_root_creates_it_once_then_reuses_it() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let first = find_or_create_flashcards_root(&db.conn).expect("first call");
        let second = find_or_create_flashcards_root(&db.conn).expect("second call");

        assert_eq!(first.id, second.id);
        assert_eq!(first.name, FLASHCARDS_ROOT_NAME);
        assert_eq!(first.parent_id, None);
        assert_eq!(list_all_folders(&db.conn).expect("list").len(), 1);
    }

    #[test]
    fn deleting_a_parent_folder_promotes_its_subfolders_to_top_level() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let root = create_folder(&db.conn, "Flashcards", "#5A5A5A").expect("create root");
        let child = create_subfolder(&db.conn, "French", "#5A5A5A", root.id).expect("create subfolder");

        delete_folder(&db.conn, root.id).expect("delete root");

        let folders = list_all_folders(&db.conn).expect("list");
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].id, child.id);
        assert_eq!(folders[0].parent_id, None);
    }
}
