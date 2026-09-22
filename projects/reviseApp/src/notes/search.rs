use super::Note;

/// Case-insensitive substring search over a note's title and content.
///
/// Scoped to title/content only as of Sprint 9: subject is now a
/// structured `subject_id` rather than free text, and narrowing by
/// subject is handled by an explicit filter control instead of
/// hoping a subject's name happens to appear in a text search.
///
/// Pure and independent of the database/UI so the matching behavior
/// is unit-tested directly, matching the pattern used for
/// `app::palette::filter_commands`.
pub fn search_notes<'a>(notes: &'a [Note], query: &str) -> Vec<&'a Note> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return notes.iter().collect();
    }

    notes
        .iter()
        .filter(|note| note.title.to_lowercase().contains(&needle) || note.content.to_lowercase().contains(&needle))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(title: &str, content: &str) -> Note {
        Note {
            id: 1,
            title: title.to_owned(),
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
    fn empty_query_returns_everything() {
        let notes = vec![note("A", ""), note("B", "")];
        assert_eq!(search_notes(&notes, "").len(), 2);
    }

    #[test]
    fn matches_title_case_insensitively() {
        let notes = vec![note("Electromagnetism", ""), note("Thermodynamics", "")];
        let results = search_notes(&notes, "ELECTRO");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Electromagnetism");
    }

    #[test]
    fn matches_content() {
        let notes = vec![note("Notes", "right-hand rule")];
        assert_eq!(search_notes(&notes, "hand rule").len(), 1);
    }

    #[test]
    fn no_match_returns_empty() {
        let notes = vec![note("A", "")];
        assert!(search_notes(&notes, "xyz").is_empty());
    }
}
