use std::fmt;

/// Parses the plain-text flashcard format the uploaded Python vocab
/// tool used for its deck files: a title on the first line, then
/// question/answer pairs, one per line, alternating (question,
/// answer, question, answer, ...). Blank lines are ignored wherever
/// they appear — including the blank line the original files put
/// between the title and the first pair — rather than being treated
/// as meaningful separators, since hand-edited `.txt` files aren't
/// perfectly consistent about them.
///
/// Returns the parsed title and pairs, or an `ImportError` if the
/// file has no title line at all, or no complete pair after it. A
/// trailing, unpaired question (odd line count after the title) is
/// silently dropped rather than erroring — the original tool was just
/// as permissive about it.
pub fn parse_deck_text(content: &str) -> Result<(String, Vec<(String, String)>), ImportError> {
    let mut lines = content.lines().map(str::trim).filter(|line| !line.is_empty());

    let title = lines.next().ok_or(ImportError::Empty)?.to_owned();

    let rest: Vec<&str> = lines.collect();
    let pairs: Vec<(String, String)> =
        rest.chunks_exact(2).map(|pair| (pair[0].to_owned(), pair[1].to_owned())).collect();

    if pairs.is_empty() {
        return Err(ImportError::NoPairs);
    }

    Ok((title, pairs))
}

/// Parses question/answer pairs with no title line — the format a
/// flashcard-set Note's `content` uses (feature request: "Built-In
/// Flashcard Editor"). Unlike an imported `.txt` file, a Note already
/// has its own `title` column, so repeating a title inside `content`
/// would be redundant; this is `parse_deck_text` minus that first
/// line. Never errors: an empty or odd-length input just yields fewer
/// (possibly zero) pairs, since this runs live against a note that
/// might still be mid-edit, not a one-shot file import.
pub fn parse_pairs(content: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = content.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    lines.chunks_exact(2).map(|pair| (pair[0].to_owned(), pair[1].to_owned())).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// The file had no non-blank lines at all.
    Empty,
    /// There was a title, but no complete question/answer pair after it.
    NoPairs,
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Empty => write!(f, "The file is empty."),
            ImportError::NoPairs => write!(f, "No question/answer pairs found after the title."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_title_and_pairs() {
        let text = "French Vocab\n\nle chat\nthe cat\nle chien\nthe dog\n";
        let (title, pairs) = parse_deck_text(text).expect("parse");
        assert_eq!(title, "French Vocab");
        assert_eq!(
            pairs,
            vec![("le chat".to_owned(), "the cat".to_owned()), ("le chien".to_owned(), "the dog".to_owned())]
        );
    }

    #[test]
    fn tolerates_extra_blank_lines_between_pairs() {
        let text = "Title\n\n\nq1\na1\n\nq2\na2\n\n\n";
        let (title, pairs) = parse_deck_text(text).expect("parse");
        assert_eq!(title, "Title");
        assert_eq!(pairs.len(), 2);
    }

    #[test]
    fn drops_a_trailing_unpaired_question() {
        let text = "Title\n\nq1\na1\nq2";
        let (_, pairs) = parse_deck_text(text).expect("parse");
        assert_eq!(pairs, vec![("q1".to_owned(), "a1".to_owned())]);
    }

    #[test]
    fn empty_content_is_an_error() {
        assert_eq!(parse_deck_text(""), Err(ImportError::Empty));
        assert_eq!(parse_deck_text("   \n  \n"), Err(ImportError::Empty));
    }

    #[test]
    fn a_title_with_no_pairs_is_an_error() {
        assert_eq!(parse_deck_text("Just a title"), Err(ImportError::NoPairs));
    }

    #[test]
    fn trims_surrounding_whitespace_on_every_line() {
        let text = "  Title  \n\n  q1  \n  a1  \n";
        let (title, pairs) = parse_deck_text(text).expect("parse");
        assert_eq!(title, "Title");
        assert_eq!(pairs, vec![("q1".to_owned(), "a1".to_owned())]);
    }

    #[test]
    fn parse_pairs_has_no_title_line_and_never_errors() {
        assert_eq!(parse_pairs("le chat\nthe cat\nle chien\nthe dog"), vec![
            ("le chat".to_owned(), "the cat".to_owned()),
            ("le chien".to_owned(), "the dog".to_owned()),
        ]);
        assert_eq!(parse_pairs(""), Vec::<(String, String)>::new());
        assert_eq!(parse_pairs("just one line"), Vec::<(String, String)>::new());
    }
}
