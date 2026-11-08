use chrono::Local;

use crate::natural_date::parse_natural_date;

/// Normalizes a free-typed due-date string. Recognizes everything
/// `natural_date::parse_natural_date` does — `"today"`/`"now"`,
/// `"tomorrow"`/`"yesterday"`, weekday names, `"next <weekday>"`, and
/// literal `YYYY-MM-DD` — resolving any of those to a real date.
///
/// Anything else passes through unchanged as free text: due dates
/// have never been strictly validated here (Sprint 2 never enforced
/// `YYYY-MM-DD`), and adding natural-language shortcuts doesn't start
/// enforcing it — someone typing "before the exam" should still be
/// able to. Empty input (after trimming) means "no due date".
pub fn normalize_due_date(raw: &str) -> Option<String> {
    match parse_natural_date(raw, Local::now().date_naive()) {
        Ok(Some(date)) => Some(date.format("%Y-%m-%d").to_string()),
        Ok(None) => None,
        Err(_) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_owned())
            }
        }
    }
}

pub fn today_string() -> String {
    Local::now().date_naive().format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_no_due_date() {
        assert_eq!(normalize_due_date(""), None);
        assert_eq!(normalize_due_date("   "), None);
    }

    #[test]
    fn today_and_now_resolve_to_todays_date_case_insensitively() {
        let expected = today_string();
        assert_eq!(normalize_due_date("today"), Some(expected.clone()));
        assert_eq!(normalize_due_date("Today"), Some(expected.clone()));
        assert_eq!(normalize_due_date("NOW"), Some(expected.clone()));
        assert_eq!(normalize_due_date("  now  "), Some(expected));
    }

    #[test]
    fn tomorrow_and_weekday_names_resolve_to_real_dates() {
        use chrono::Duration;
        let today = Local::now().date_naive();
        let tomorrow = (today + Duration::days(1)).format("%Y-%m-%d").to_string();
        assert_eq!(normalize_due_date("tomorrow"), Some(tomorrow));
        // "next monday" always resolves to a real YYYY-MM-DD, whatever today is.
        let result = normalize_due_date("next monday").expect("should resolve");
        assert!(chrono::NaiveDate::parse_from_str(&result, "%Y-%m-%d").is_ok());
    }

    #[test]
    fn literal_iso_date_passes_through_as_the_same_date() {
        assert_eq!(normalize_due_date("2026-09-01"), Some("2026-09-01".to_owned()));
    }

    #[test]
    fn unrecognized_free_text_still_passes_through_unchanged() {
        assert_eq!(normalize_due_date("before the exam"), Some("before the exam".to_owned()));
    }
}
