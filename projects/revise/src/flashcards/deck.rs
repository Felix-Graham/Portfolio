/// A saved flashcard deck, imported wholesale from a `.txt` file (see
/// `flashcards::import::parse_deck_text`) and kept in the database so
/// it can be studied again without re-uploading the file each time.
/// The app has no flashcard editor of its own — import is the only
/// way a deck's cards are ever created, per the feature request's own
/// scope.
#[derive(Debug, Clone)]
pub struct Deck {
    pub id: i64,
    pub title: String,
    /// The original filename it was imported from, if known — shown
    /// in the deck list so a re-import is recognisable. Purely
    /// informational: nothing re-reads the file after import, and a
    /// deck imported from pasted content with no path has `None`.
    pub source_name: Option<String>,
    pub created_at: String,
}
