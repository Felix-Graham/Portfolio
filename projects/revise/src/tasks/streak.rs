use std::collections::BTreeSet;

use chrono::{Duration, NaiveDate};

/// Counts consecutive days, ending today (or ending yesterday if
/// nothing has been completed yet today — so the streak doesn't drop
/// to zero first thing in the morning), on which at least one task
/// was completed.
///
/// This is deliberately a pure function over dates rather than
/// something that touches SQLite directly, so the day-boundary logic
/// can be unit-tested without a database.
pub fn current_streak(completed_dates: &[NaiveDate], today: NaiveDate) -> u32 {
    let days: BTreeSet<NaiveDate> = completed_dates.iter().copied().collect();
    if days.is_empty() {
        return 0;
    }

    let mut cursor = if days.contains(&today) {
        today
    } else if days.contains(&(today - Duration::days(1))) {
        today - Duration::days(1)
    } else {
        return 0;
    };

    let mut streak = 0u32;
    while days.contains(&cursor) {
        streak += 1;
        cursor -= Duration::days(1);
    }
    streak
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn no_completions_is_zero() {
        assert_eq!(current_streak(&[], date(2026, 8, 25)), 0);
    }

    #[test]
    fn single_day_today_is_one() {
        let today = date(2026, 8, 25);
        assert_eq!(current_streak(&[today], today), 1);
    }

    #[test]
    fn three_consecutive_days_ending_today() {
        let today = date(2026, 8, 25);
        let dates = [today, today - Duration::days(1), today - Duration::days(2)];
        assert_eq!(current_streak(&dates, today), 3);
    }

    #[test]
    fn nothing_completed_today_yet_still_counts_yesterdays_streak() {
        let today = date(2026, 8, 25);
        let yesterday = today - Duration::days(1);
        let day_before = today - Duration::days(2);
        assert_eq!(current_streak(&[yesterday, day_before], today), 2);
    }

    #[test]
    fn a_gap_breaks_the_streak() {
        let today = date(2026, 8, 25);
        let three_days_ago = today - Duration::days(3);
        // Nothing yesterday or today — the streak is over, regardless
        // of older history.
        assert_eq!(current_streak(&[three_days_ago], today), 0);
    }

    #[test]
    fn duplicate_dates_do_not_inflate_the_streak() {
        let today = date(2026, 8, 25);
        assert_eq!(current_streak(&[today, today, today], today), 1);
    }
}
