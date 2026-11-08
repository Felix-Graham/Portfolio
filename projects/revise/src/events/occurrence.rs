use std::collections::HashMap;

use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;

use super::event::Event;
use super::recurrence::{Frequency, RecurrenceRule};
use crate::database::queries;

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

/// One event as it appears on a specific day — either the literal
/// stored row (`is_anchor: true`) or a virtual occurrence synthesized
/// from a recurrence rule, with `start_time`/`end_time` shifted to that
/// day but otherwise identical. Nothing about a virtual occurrence is
/// persisted; it is recomputed on demand, per the plan's "don't
/// duplicate the underlying event definition" guidance.
///
/// UI-free on purpose: the Calendar views and the time-block planner
/// both consume it.
#[derive(Clone, Debug)]
pub struct Occurrence {
    pub event: Event,
    pub is_anchor: bool,
    pub rule_label: Option<String>,
}

/// Every occurrence — literal or generated from a recurrence rule —
/// across all `events` that falls within `[range_start, range_end]`
/// (inclusive), keyed by the date it appears on and sorted by start
/// time within each day. The single-day case (`occurrences_on_day`)
/// and the multi-day case (Week/Month views, the block planner) share
/// this one implementation.
///
/// An occurrence belongs to the day it **starts** (`Event::day`); one
/// that runs past midnight keeps its real `end_time`, so callers that
/// care about the overflow on the following day (the block planner)
/// can derive it from the occurrence itself.
pub fn occurrences_in_window(
    conn: &Connection,
    events: &[Event],
    range_start: NaiveDate,
    range_end: NaiveDate,
) -> HashMap<NaiveDate, Vec<Occurrence>> {
    let mut by_day: HashMap<NaiveDate, Vec<Occurrence>> = HashMap::new();

    for event in events {
        let anchor_date = event.day();

        match event.recurrence_id {
            None => {
                if anchor_date >= range_start && anchor_date <= range_end {
                    by_day.entry(anchor_date).or_default().push(Occurrence {
                        event: event.clone(),
                        is_anchor: true,
                        rule_label: None,
                    });
                }
            }
            Some(rule_id) => {
                let Ok(Some(rule)) = queries::recurrence::get_rule(conn, rule_id) else { continue };
                let dates = occurrences_in_range(anchor_date, &rule, range_start, range_end);
                if dates.is_empty() {
                    continue;
                }
                let exceptions = queries::recurrence::list_exceptions(conn, event.id).unwrap_or_default();
                let rule_label = rule.describe();

                for date in dates {
                    if exceptions.contains(&date) {
                        continue;
                    }
                    let occ = if date == anchor_date {
                        Occurrence { event: event.clone(), is_anchor: true, rule_label: Some(rule_label.clone()) }
                    } else {
                        let delta = date - anchor_date;
                        let mut shifted = event.clone();
                        shifted.start_time = event.start_time + delta;
                        shifted.end_time = event.end_time + delta;
                        Occurrence { event: shifted, is_anchor: false, rule_label: Some(rule_label.clone()) }
                    };
                    by_day.entry(date).or_default().push(occ);
                }
            }
        }
    }

    for occurrences in by_day.values_mut() {
        occurrences.sort_by_key(|occ| occ.event.start_time);
    }
    by_day
}

/// Every occurrence that falls on `day`, sorted by start time.
pub fn occurrences_on_day(conn: &Connection, events: &[Event], day: NaiveDate) -> Vec<Occurrence> {
    occurrences_in_window(conn, events, day, day).remove(&day).unwrap_or_default()
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

    // --- occurrences_in_window / occurrences_on_day (moved from ui/calendar.rs) ---

    use crate::database::Database;
    use chrono::{Local, TimeZone};

    fn daily_event(db: &Database, title: &str, start_h: u32, end_h: u32, end_day_offset: u32) -> Event {
        let rule_id = queries::recurrence::create_rule(&db.conn, &rule(Frequency::Daily, 1)).expect("create rule");
        let start = Local.with_ymd_and_hms(2026, 8, 24, start_h, 0, 0).unwrap();
        let end = Local.with_ymd_and_hms(2026, 8, 24 + end_day_offset, end_h, 0, 0).unwrap();
        queries::events::create_event(&db.conn, title, "", start, end, None, Some(rule_id)).expect("create event")
    }

    #[test]
    fn window_expands_a_recurring_event_and_marks_only_the_first_as_anchor() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let event = daily_event(&db, "Daily review", 9, 10, 0);

        let by_day = occurrences_in_window(&db.conn, &[event], date(2026, 8, 24), date(2026, 8, 26));
        assert_eq!(by_day.len(), 3);
        assert!(by_day[&date(2026, 8, 24)][0].is_anchor);
        assert!(!by_day[&date(2026, 8, 25)][0].is_anchor);
        assert_eq!(by_day[&date(2026, 8, 25)][0].event.start_time.date_naive(), date(2026, 8, 25));
        assert!(by_day[&date(2026, 8, 25)][0].rule_label.is_some());
    }

    #[test]
    fn window_skips_dates_with_an_exception() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let event = daily_event(&db, "Daily review", 9, 10, 0);
        queries::recurrence::add_exception(&db.conn, event.id, date(2026, 8, 25)).expect("add exception");

        let by_day = occurrences_in_window(&db.conn, &[event], date(2026, 8, 24), date(2026, 8, 26));
        assert!(by_day.contains_key(&date(2026, 8, 24)));
        assert!(!by_day.contains_key(&date(2026, 8, 25)));
        assert!(by_day.contains_key(&date(2026, 8, 26)));
    }

    #[test]
    fn window_returns_a_non_recurring_event_only_when_its_day_is_in_range() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let start = Local.with_ymd_and_hms(2026, 8, 24, 9, 0, 0).unwrap();
        let end = Local.with_ymd_and_hms(2026, 8, 24, 10, 0, 0).unwrap();
        let event = queries::events::create_event(&db.conn, "One-off", "", start, end, None, None).expect("create event");

        let inside = occurrences_in_window(&db.conn, &[event.clone()], date(2026, 8, 24), date(2026, 8, 24));
        assert_eq!(inside[&date(2026, 8, 24)].len(), 1);
        assert!(inside[&date(2026, 8, 24)][0].rule_label.is_none());

        let outside = occurrences_in_window(&db.conn, &[event], date(2026, 8, 25), date(2026, 8, 31));
        assert!(outside.is_empty());
    }

    #[test]
    fn window_sorts_each_day_by_start_time() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let late = {
            let s = Local.with_ymd_and_hms(2026, 8, 24, 15, 0, 0).unwrap();
            let e = Local.with_ymd_and_hms(2026, 8, 24, 16, 0, 0).unwrap();
            queries::events::create_event(&db.conn, "Late", "", s, e, None, None).unwrap()
        };
        let early = {
            let s = Local.with_ymd_and_hms(2026, 8, 24, 8, 0, 0).unwrap();
            let e = Local.with_ymd_and_hms(2026, 8, 24, 9, 0, 0).unwrap();
            queries::events::create_event(&db.conn, "Early", "", s, e, None, None).unwrap()
        };

        let day = occurrences_on_day(&db.conn, &[late, early], date(2026, 8, 24));
        let titles: Vec<&str> = day.iter().map(|o| o.event.title.as_str()).collect();
        assert_eq!(titles, vec!["Early", "Late"]);
    }

    #[test]
    fn midnight_crossing_occurrence_belongs_to_its_start_day_and_keeps_its_real_end() {
        let db = Database::open_in_memory().expect("open in-memory db");
        // 23:00 -> 01:00 next day, repeating daily.
        let event = daily_event(&db, "Late shift", 23, 1, 1);

        let by_day = occurrences_in_window(&db.conn, &[event], date(2026, 8, 24), date(2026, 8, 26));
        // Listed under the day it starts, never duplicated onto the next.
        assert_eq!(by_day.len(), 3);
        let second = &by_day[&date(2026, 8, 25)][0].event;
        assert_eq!(second.start_time.date_naive(), date(2026, 8, 25));
        // The overflow past midnight survives on the shifted occurrence,
        // so the block planner can mark 00:00-01:00 busy on the next day.
        assert_eq!(second.end_time.date_naive(), date(2026, 8, 26));
        assert_eq!(second.end_time - second.start_time, chrono::Duration::hours(2));
    }

    #[test]
    fn on_day_is_empty_for_a_day_with_nothing() {
        let db = Database::open_in_memory().expect("open in-memory db");
        assert!(occurrences_on_day(&db.conn, &[], date(2026, 8, 24)).is_empty());
    }
}
