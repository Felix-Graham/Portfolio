use chrono::{Datelike, Duration, NaiveDate, Weekday};

/// Parses a date written as `"today"`/`"now"`, `"tomorrow"`,
/// `"yesterday"`, a bare weekday name, `"next <weekday>"`, or a
/// literal `YYYY-MM-DD`. Case-insensitive, whitespace-trimmed. Empty
/// input means "no date" (`Ok(None)`), distinct from unparseable
/// input (`Err`).
///
/// A bare weekday name ("tuesday") resolves to the closest occurrence
/// on or after `today` — if today already is that weekday, it means
/// today, matching how people actually use it ("I'll do it Tuesday").
/// `"next tuesday"` is always strictly after today, even if today
/// happens to be Tuesday — the word "next" is doing real work there.
///
/// Pure and independent of the database/UI, shared by
/// `tasks::due_date` (which falls back to treating unparseable input
/// as free text — due dates have never been strictly validated) and
/// the Calendar's event date field (which surfaces the `Err` message
/// directly, since an event's date is load-bearing for scheduling).
pub fn parse_natural_date(input: &str, today: NaiveDate) -> Result<Option<NaiveDate>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let lower = trimmed.to_lowercase();

    match lower.as_str() {
        "today" | "now" => return Ok(Some(today)),
        "tomorrow" => return Ok(Some(today + Duration::days(1))),
        "yesterday" => return Ok(Some(today - Duration::days(1))),
        _ => {}
    }

    if let Some(name) = lower.strip_prefix("next ") {
        if let Some(weekday) = parse_weekday(name) {
            return Ok(Some(next_occurrence_of(today, weekday, true)));
        }
    }

    if let Some(weekday) = parse_weekday(&lower) {
        return Ok(Some(next_occurrence_of(today, weekday, false)));
    }

    NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| "Try a date like 2026-08-25, 'today', 'tomorrow', or 'next tuesday'.".to_owned())
}

fn parse_weekday(s: &str) -> Option<Weekday> {
    match s {
        "monday" => Some(Weekday::Mon),
        "tuesday" => Some(Weekday::Tue),
        "wednesday" => Some(Weekday::Wed),
        "thursday" => Some(Weekday::Thu),
        "friday" => Some(Weekday::Fri),
        "saturday" => Some(Weekday::Sat),
        "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

/// The next date matching `weekday`. If `strictly_after` is `false`
/// and `today` already falls on `weekday`, returns `today` itself;
/// if `true`, always returns a date after `today`.
fn next_occurrence_of(today: NaiveDate, weekday: Weekday, strictly_after: bool) -> NaiveDate {
    let mut candidate = if strictly_after { today + Duration::days(1) } else { today };
    while candidate.weekday() != weekday {
        candidate += Duration::days(1);
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn empty_input_means_no_date() {
        assert_eq!(parse_natural_date("", date(2026, 8, 25)), Ok(None));
        assert_eq!(parse_natural_date("   ", date(2026, 8, 25)), Ok(None));
    }

    #[test]
    fn today_and_now() {
        let today = date(2026, 8, 25);
        assert_eq!(parse_natural_date("today", today), Ok(Some(today)));
        assert_eq!(parse_natural_date("Today", today), Ok(Some(today)));
        assert_eq!(parse_natural_date("NOW", today), Ok(Some(today)));
    }

    #[test]
    fn tomorrow_and_yesterday() {
        let today = date(2026, 8, 25);
        assert_eq!(parse_natural_date("tomorrow", today), Ok(Some(date(2026, 8, 26))));
        assert_eq!(parse_natural_date("Tomorrow", today), Ok(Some(date(2026, 8, 26))));
        assert_eq!(parse_natural_date("yesterday", today), Ok(Some(date(2026, 8, 24))));
    }

    #[test]
    fn bare_weekday_on_that_day_means_today() {
        // 2026-08-25 is a Tuesday.
        let today = date(2026, 8, 25);
        assert_eq!(parse_natural_date("tuesday", today), Ok(Some(today)));
    }

    #[test]
    fn bare_weekday_not_today_means_the_closest_upcoming_one() {
        let today = date(2026, 8, 25); // Tuesday
        assert_eq!(parse_natural_date("thursday", today), Ok(Some(date(2026, 8, 27))));
        // Wraps to next week if the day already passed this week.
        assert_eq!(parse_natural_date("monday", today), Ok(Some(date(2026, 8, 31))));
    }

    #[test]
    fn next_weekday_is_always_strictly_after_today_even_if_today_matches() {
        let today = date(2026, 8, 25); // Tuesday
        assert_eq!(parse_natural_date("next tuesday", today), Ok(Some(date(2026, 9, 1))));
        assert_eq!(parse_natural_date("Next Thursday", today), Ok(Some(date(2026, 8, 27))));
    }

    #[test]
    fn literal_iso_date_still_works() {
        assert_eq!(parse_natural_date("2026-12-31", date(2026, 8, 25)), Ok(Some(date(2026, 12, 31))));
    }

    #[test]
    fn unrecognized_text_is_an_error() {
        assert!(parse_natural_date("whenever", date(2026, 8, 25)).is_err());
        assert!(parse_natural_date("Fredsday", date(2026, 8, 25)).is_err());
    }
}
