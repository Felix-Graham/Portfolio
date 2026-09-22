use chrono::{Datelike, NaiveDate};

use super::recurrence::{Frequency, RecurrenceRule};

/// Safety cap on how many candidate dates a single call will walk
/// through, regardless of frequency/interval. Prevents a pathological
/// rule (e.g. daily, no end date, someone querying a range decades
/// out) from hanging the UI thread. Comfortably larger than any
/// realistic revision-planning horizon.
const MAX_CANDIDATES: usize = 5000;

/// All occurrence dates for `rule`, anchored at `anchor_date`, that
/// fall within `[range_start, range_end]` (inclusive), respecting
/// `end_date` and `occurrence_limit`.
///
/// `occurrence_limit` counts the *whole series* from the anchor
/// forward, not just what's visible in the range — the same
/// occurrence is always occurrence #12 regardless of which window
/// you're viewing it through, matching how `RRULE;COUNT=` behaves.
pub fn occurrences_in_range(
    anchor_date: NaiveDate,
    rule: &RecurrenceRule,
    range_start: NaiveDate,
    range_end: NaiveDate,
) -> Vec<NaiveDate> {
    if range_end < anchor_date {
        return Vec::new();
    }

    let mut results = Vec::new();
    let mut index: u32 = 0;

    for candidate in candidate_dates(anchor_date, rule, range_end) {
        if let Some(end) = rule.end_date {
            if candidate > end {
                break;
            }
        }
        if let Some(limit) = rule.occurrence_limit {
            if index >= limit {
                break;
            }
        }
        if candidate > range_end {
            break;
        }

        index += 1;
        if candidate >= range_start {
            results.push(candidate);
        }
    }

    results
}

/// True if `day` is a valid, non-excepted occurrence of `rule`.
/// Used directly by `ui::calendar::occurrences_in_window` and by
/// `notifications::todays_event_starts`, which only needs a
/// single-day check rather than a whole window.
pub fn occurs_on(anchor_date: NaiveDate, rule: &RecurrenceRule, day: NaiveDate) -> bool {
    occurrences_in_range(anchor_date, rule, day, day).contains(&day)
}

/// Every *structurally* matching date from the anchor forward, up to
/// (and not much past) `upper_bound`, ignoring `end_date` /
/// `occurrence_limit` (the caller applies those against this list).
///
/// Bounding generation by `upper_bound` — which is always the
/// caller's query range end, `today` in the current UI — keeps this
/// cheap even though it reruns every frame: a daily rule anchored a
/// year ago still only walks forward to "today", not through
/// `MAX_CANDIDATES` entries. `MAX_CANDIDATES` remains as a hard
/// safety cap for the pathological case (rule with no end date and no
/// occurrence limit, queried against a far-future range).
fn candidate_dates(anchor_date: NaiveDate, rule: &RecurrenceRule, upper_bound: NaiveDate) -> Vec<NaiveDate> {
    let interval = rule.interval.max(1) as i64;

    match rule.frequency {
        Frequency::Daily => {
            let mut dates = Vec::new();
            let mut candidate = anchor_date;
            let mut steps = 0usize;
            while candidate <= upper_bound && steps < MAX_CANDIDATES {
                dates.push(candidate);
                candidate += chrono::Duration::days(interval);
                steps += 1;
            }
            dates
        }

        Frequency::Weekly => {
            let weekdays = if rule.weekdays.is_empty() {
                vec![anchor_date.weekday()]
            } else {
                let mut days = rule.weekdays.clone();
                days.sort_by_key(|d| d.num_days_from_monday());
                days
            };

            let anchor_week_monday =
                anchor_date - chrono::Duration::days(anchor_date.weekday().num_days_from_monday() as i64);

            let mut dates = Vec::new();
            let mut week_monday = anchor_week_monday;
            let mut steps = 0usize;
            while week_monday <= upper_bound && steps < MAX_CANDIDATES {
                for wd in &weekdays {
                    let date = week_monday + chrono::Duration::days(wd.num_days_from_monday() as i64);
                    if date >= anchor_date {
                        dates.push(date);
                    }
                }
                week_monday += chrono::Duration::weeks(interval);
                steps += 1;
            }
            dates.sort();
            dates
        }

        Frequency::Monthly => {
            let mut dates = Vec::new();
            let mut k: i64 = 0;
            loop {
                if k as usize >= MAX_CANDIDATES {
                    break;
                }
                let total_months = anchor_date.month0() as i64 + k * interval;
                let year = anchor_date.year() + total_months.div_euclid(12) as i32;
                let month = total_months.rem_euclid(12) as u32 + 1;

                // Bound the loop using the 1st of the target month —
                // valid regardless of whether `anchor_date`'s
                // day-of-month exists there — so we still terminate
                // even during a long run of invalid months.
                let first_of_month = NaiveDate::from_ymd_opt(year, month, 1).expect("valid y/m/1 date");
                if first_of_month > upper_bound {
                    break;
                }

                // `from_ymd_opt` returns `None` rather than clamping
                // when the day doesn't exist in this month (e.g. the
                // 31st in February) — exactly the "skip" semantics we
                // want, unlike `checked_add_months`.
                if let Some(candidate) = NaiveDate::from_ymd_opt(year, month, anchor_date.day()) {
                    if candidate <= upper_bound {
                        dates.push(candidate);
                    }
                }

                k += 1;
            }
            dates
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Weekday;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn rule(frequency: Frequency, interval: u32) -> RecurrenceRule {
        RecurrenceRule {
            id: None,
            frequency,
            interval,
            weekdays: Vec::new(),
            end_date: None,
            occurrence_limit: None,
        }
    }

    #[test]
    fn daily_every_day() {
        let anchor = date(2026, 8, 1);
        let r = rule(Frequency::Daily, 1);
        let occurrences = occurrences_in_range(anchor, &r, date(2026, 8, 1), date(2026, 8, 5));
        assert_eq!(
            occurrences,
            vec![date(2026, 8, 1), date(2026, 8, 2), date(2026, 8, 3), date(2026, 8, 4), date(2026, 8, 5)]
        );
    }

    #[test]
    fn every_n_days() {
        let anchor = date(2026, 8, 1);
        let r = rule(Frequency::Daily, 3);
        let occurrences = occurrences_in_range(anchor, &r, date(2026, 8, 1), date(2026, 8, 10));
        assert_eq!(occurrences, vec![date(2026, 8, 1), date(2026, 8, 4), date(2026, 8, 7), date(2026, 8, 10)]);
    }

    #[test]
    fn weekly_same_weekday_as_anchor() {
        // 2026-08-01 is a Saturday.
        let anchor = date(2026, 8, 1);
        let r = rule(Frequency::Weekly, 1);
        let occurrences = occurrences_in_range(anchor, &r, date(2026, 8, 1), date(2026, 8, 22));
        assert_eq!(occurrences, vec![date(2026, 8, 1), date(2026, 8, 8), date(2026, 8, 15), date(2026, 8, 22)]);
    }

    #[test]
    fn every_n_weeks() {
        let anchor = date(2026, 8, 1); // Saturday
        let mut r = rule(Frequency::Weekly, 2);
        r.interval = 2;
        let occurrences = occurrences_in_range(anchor, &r, date(2026, 8, 1), date(2026, 9, 1));
        assert_eq!(occurrences, vec![date(2026, 8, 1), date(2026, 8, 15), date(2026, 8, 29)]);
    }

    #[test]
    fn selected_weekdays() {
        // Anchor on a Monday; select Tue + Thu per the plan's example.
        let anchor = date(2026, 8, 3); // Monday
        let mut r = rule(Frequency::Weekly, 1);
        r.weekdays = vec![Weekday::Tue, Weekday::Thu];
        let occurrences = occurrences_in_range(anchor, &r, date(2026, 8, 3), date(2026, 8, 16));
        // Monday itself isn't selected, so the first occurrence is
        // that week's Tuesday, not the anchor date.
        assert_eq!(
            occurrences,
            vec![date(2026, 8, 4), date(2026, 8, 6), date(2026, 8, 11), date(2026, 8, 13)]
        );
    }

    #[test]
    fn monthly_same_day_of_month() {
        let anchor = date(2026, 1, 15);
        let r = rule(Frequency::Monthly, 1);
        let occurrences = occurrences_in_range(anchor, &r, date(2026, 1, 1), date(2026, 4, 30));
        assert_eq!(occurrences, vec![date(2026, 1, 15), date(2026, 2, 15), date(2026, 3, 15), date(2026, 4, 15)]);
    }

    #[test]
    fn monthly_skips_months_without_that_day_of_month() {
        // 31st doesn't exist in February or April.
        let anchor = date(2026, 1, 31);
        let r = rule(Frequency::Monthly, 1);
        let occurrences = occurrences_in_range(anchor, &r, date(2026, 1, 1), date(2026, 5, 31));
        assert_eq!(occurrences, vec![date(2026, 1, 31), date(2026, 3, 31), date(2026, 5, 31)]);
    }

    #[test]
    fn respects_end_date() {
        let anchor = date(2026, 8, 1);
        let mut r = rule(Frequency::Daily, 1);
        r.end_date = Some(date(2026, 8, 3));
        let occurrences = occurrences_in_range(anchor, &r, date(2026, 8, 1), date(2026, 8, 10));
        assert_eq!(occurrences, vec![date(2026, 8, 1), date(2026, 8, 2), date(2026, 8, 3)]);
    }

    #[test]
    fn respects_occurrence_limit() {
        let anchor = date(2026, 8, 1);
        let mut r = rule(Frequency::Daily, 1);
        r.occurrence_limit = Some(3);
        let occurrences = occurrences_in_range(anchor, &r, date(2026, 8, 1), date(2026, 8, 30));
        assert_eq!(occurrences, vec![date(2026, 8, 1), date(2026, 8, 2), date(2026, 8, 3)]);
    }

    #[test]
    fn occurrence_limit_counts_from_anchor_not_from_the_query_window() {
        // Querying a window that starts after the series' first few
        // occurrences must not "reset" the count.
        let anchor = date(2026, 8, 1);
        let mut r = rule(Frequency::Daily, 1);
        r.occurrence_limit = Some(3); // Aug 1, 2, 3 only, ever.
        let occurrences = occurrences_in_range(anchor, &r, date(2026, 8, 2), date(2026, 8, 10));
        assert_eq!(occurrences, vec![date(2026, 8, 2), date(2026, 8, 3)]);
    }

    #[test]
    fn never_occurs_before_the_anchor_date() {
        let anchor = date(2026, 8, 10);
        let r = rule(Frequency::Daily, 1);
        let occurrences = occurrences_in_range(anchor, &r, date(2026, 8, 1), date(2026, 8, 9));
        assert!(occurrences.is_empty());
    }

    #[test]
    fn occurs_on_matches_a_single_day_correctly() {
        let anchor = date(2026, 8, 1); // Saturday
        let r = rule(Frequency::Weekly, 1);
        assert!(occurs_on(anchor, &r, date(2026, 8, 8)));
        assert!(!occurs_on(anchor, &r, date(2026, 8, 9)));
    }
}
