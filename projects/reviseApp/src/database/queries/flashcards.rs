use chrono::{Local, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension, Result, Row};

use crate::flashcards::{Deck, Flashcard};

fn row_to_deck(row: &Row) -> rusqlite::Result<Deck> {
    Ok(Deck { id: row.get(0)?, title: row.get(1)?, source_name: row.get(2)?, created_at: row.get(3)? })
}

fn row_to_card(row: &Row) -> rusqlite::Result<Flashcard> {
    Ok(Flashcard { id: row.get(0)?, deck_id: row.get(1)?, question: row.get(2)?, answer: row.get(3)?, position: row.get(4)? })
}

/// Imports a whole deck — the deck row plus every card, in parsed
/// order — in one transaction, so a half-imported deck (a deck row
/// with no cards, or cards with no deck) can never happen if
/// something fails partway through.
pub fn import_deck(conn: &Connection, title: &str, source_name: Option<&str>, pairs: &[(String, String)]) -> Result<Deck> {
    let now = Local::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;

    tx.execute(
        "INSERT INTO flashcard_decks (title, source_name, created_at) VALUES (?1, ?2, ?3)",
        params![title, source_name, now],
    )?;
    let deck_id = tx.last_insert_rowid();

    for (position, (question, answer)) in pairs.iter().enumerate() {
        tx.execute(
            "INSERT INTO flashcards (deck_id, question, answer, position) VALUES (?1, ?2, ?3, ?4)",
            params![deck_id, question, answer, position as i64],
        )?;
    }

    tx.commit()?;

    Ok(Deck { id: deck_id, title: title.to_owned(), source_name: source_name.map(str::to_owned), created_at: now })
}

/// All decks, most recently imported first.
pub fn list_all_decks(conn: &Connection) -> Result<Vec<Deck>> {
    let mut stmt = conn.prepare("SELECT id, title, source_name, created_at FROM flashcard_decks ORDER BY created_at DESC")?;
    let decks = stmt.query_map([], row_to_deck)?.collect::<Result<Vec<_>>>()?;
    Ok(decks)
}

/// Every card in a deck, in import order — the natural order to study
/// them in, and what "Shuffle" reorders away from.
pub fn list_cards_for_deck(conn: &Connection, deck_id: i64) -> Result<Vec<Flashcard>> {
    let mut stmt =
        conn.prepare("SELECT id, deck_id, question, answer, position FROM flashcards WHERE deck_id = ?1 ORDER BY position ASC")?;
    let cards = stmt.query_map(params![deck_id], row_to_card)?.collect::<Result<Vec<_>>>()?;
    Ok(cards)
}

/// A single deck by id — used to resolve a palette-picked deck
/// reference (`ui::flashcards::start_study_session`) without pulling
/// every deck just to find the one that was picked.
pub fn get_deck(conn: &Connection, id: i64) -> Result<Option<Deck>> {
    conn.query_row(
        "SELECT id, title, source_name, created_at FROM flashcard_decks WHERE id = ?1",
        params![id],
        row_to_deck,
    )
    .optional()
}

/// How many cards a deck has — used in the deck list without pulling
/// every card's full question/answer text just to show a count.
pub fn count_cards(conn: &Connection, deck_id: i64) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM flashcards WHERE deck_id = ?1", params![deck_id], |row| row.get(0))
}

/// Deletes a deck and every one of its cards. Unlike a Notes folder
/// (whose notes stand on their own without it — see
/// `folders::delete_folder`), a flashcard has no meaning detached
/// from its deck, so this is a real cascade rather than an
/// "unlink, then delete."
pub fn delete_deck(conn: &Connection, deck_id: i64) -> Result<()> {
    conn.execute("DELETE FROM flashcards WHERE deck_id = ?1", params![deck_id])?;
    conn.execute("DELETE FROM flashcard_decks WHERE id = ?1", params![deck_id])?;
    Ok(())
}

/// Records one card's answer being revealed during study — feeds the
/// Dashboard's "Flashcards seen" stat. Append-only, same shape and
/// purpose as `pomodoro::log_session`: the durable source of truth,
/// independent of the deck or card it came from still existing.
pub fn log_seen(conn: &Connection, date: NaiveDate) -> Result<()> {
    conn.execute("INSERT INTO flashcards_seen_log (date) VALUES (?1)", params![date.format("%Y-%m-%d").to_string()])?;
    Ok(())
}

/// Total flashcards seen, all time.
pub fn total_seen(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM flashcards_seen_log", [], |row| row.get(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    fn sample_pairs() -> Vec<(String, String)> {
        vec![("le chat".to_owned(), "the cat".to_owned()), ("le chien".to_owned(), "the dog".to_owned())]
    }

    #[test]
    fn import_deck_creates_deck_and_cards_in_order() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let deck = import_deck(&db.conn, "French Vocab", Some("french.txt"), &sample_pairs()).expect("import");

        assert_eq!(deck.title, "French Vocab");
        assert_eq!(deck.source_name.as_deref(), Some("french.txt"));

        let cards = list_cards_for_deck(&db.conn, deck.id).expect("list cards");
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].question, "le chat");
        assert_eq!(cards[1].answer, "the dog");
        assert_eq!(count_cards(&db.conn, deck.id).expect("count"), 2);
    }

    #[test]
    fn list_all_decks_returns_newest_first() {
        let db = Database::open_in_memory().expect("open in-memory db");
        import_deck(&db.conn, "First", None, &sample_pairs()).expect("import 1");
        import_deck(&db.conn, "Second", None, &sample_pairs()).expect("import 2");

        let decks = list_all_decks(&db.conn).expect("list");
        assert_eq!(decks.len(), 2);
        assert_eq!(decks[0].title, "Second");
    }

    #[test]
    fn delete_deck_removes_its_cards_too() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let deck = import_deck(&db.conn, "Temp", None, &sample_pairs()).expect("import");

        delete_deck(&db.conn, deck.id).expect("delete");

        assert!(list_all_decks(&db.conn).expect("list").is_empty());
        assert!(list_cards_for_deck(&db.conn, deck.id).expect("list cards").is_empty());
    }

    #[test]
    fn log_seen_and_total_seen_roundtrip() {
        let db = Database::open_in_memory().expect("open in-memory db");
        assert_eq!(total_seen(&db.conn).expect("total"), 0);

        log_seen(&db.conn, Local::now().date_naive()).expect("log 1");
        log_seen(&db.conn, Local::now().date_naive()).expect("log 2");

        assert_eq!(total_seen(&db.conn).expect("total"), 2);
    }

    #[test]
    fn get_deck_finds_it_by_id_and_none_for_a_missing_one() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let deck = import_deck(&db.conn, "French Vocab", None, &sample_pairs()).expect("import");

        let found = get_deck(&db.conn, deck.id).expect("get").expect("should exist");
        assert_eq!(found.title, "French Vocab");
        assert!(get_deck(&db.conn, deck.id + 999).expect("get missing").is_none());
    }

    #[test]
    fn decks_and_cards_survive_reopening_the_same_file() {
        let dir = std::env::temp_dir().join(format!("revise-flashcards-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("revise.db");

        {
            let db = Database::open_at(db_path.clone()).expect("open db (first run)");
            import_deck(&db.conn, "Persisted", None, &sample_pairs()).expect("import");
        }
        {
            let db = Database::open_at(db_path.clone()).expect("open db (second run)");
            let decks = list_all_decks(&db.conn).expect("list");
            assert_eq!(decks.len(), 1);
            assert_eq!(decks[0].title, "Persisted");
            assert_eq!(count_cards(&db.conn, decks[0].id).expect("count"), 2);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
