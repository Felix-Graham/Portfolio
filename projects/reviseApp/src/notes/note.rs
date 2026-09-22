/// A lightweight revision note (Section 15's data model). Deliberately
/// plain text/simple Markdown, not a full note-taking format — the
/// plan is explicit this shouldn't grow into one.
#[derive(Debug, Clone)]
pub struct Note {
    pub id: i64,
    pub title: String,
    pub content: String,
    /// The revision subject this note belongs to, if any (Sprint 9 —
    /// replaces the free-text label used through Sprint 8).
    pub subject_id: Option<i64>,
    /// The revision session this note was taken during, if any.
    pub event_id: Option<i64>,
    /// The folder this note is organized under, if any (feature
    /// request: "Notes Folders"). `None` means "not in a folder" —
    /// notes standing alone was already true before folders existed,
    /// and stays the default.
    pub folder_id: Option<i64>,
    /// Whether this note is a flashcard set (feature request:
    /// "Built-In Flashcard Editor") — its `content` is read as
    /// question/answer pairs (`flashcards::import::parse_pairs`)
    /// rather than freeform text, and it's studyable from the
    /// Flashcards view alongside imported decks.
    pub is_flashcard_set: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl Note {
    /// A short, single-line preview of the content for list views —
    /// first non-empty line, truncated, so a multi-paragraph note
    /// doesn't blow out a list row.
    pub fn preview(&self, max_len: usize) -> String {
        let first_line = self.content.lines().find(|line| !line.trim().is_empty()).unwrap_or("").trim();

        if first_line.chars().count() <= max_len {
            first_line.to_owned()
        } else {
            let truncated: String = first_line.chars().take(max_len).collect();
            format!("{truncated}…")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_with_content(content: &str) -> Note {
        Note {
            id: 1,
            title: "Test".to_owned(),
            content: content.to_owned(),
            subject_id: None,
            event_id: None,
            folder_id: None,
            is_flashcard_set: false,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn preview_uses_first_non_empty_line() {
        let note = note_with_content("\n\n  Magnetic field strength\nB = mu0 I / 2 pi r");
        assert_eq!(note.preview(80), "Magnetic field strength");
    }

    #[test]
    fn preview_truncates_long_lines() {
        let note = note_with_content("a".repeat(200).as_str());
        let preview = note.preview(50);
        assert_eq!(preview.chars().count(), 51); // 50 chars + ellipsis
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn preview_of_empty_content_is_empty() {
        let note = note_with_content("");
        assert_eq!(note.preview(50), "");
    }
}
