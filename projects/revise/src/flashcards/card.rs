/// A single question/answer pair belonging to a `Deck`. `position`
/// preserves import order, which doubles as study order — the same
/// order the source file listed its vocab pairs in.
#[derive(Debug, Clone)]
pub struct Flashcard {
    pub id: i64,
    pub deck_id: i64,
    pub question: String,
    pub answer: String,
    pub position: i64,
}
