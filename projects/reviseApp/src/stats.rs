use std::collections::HashMap;

use chrono::{Datelike, Duration, Local, NaiveDate};

use crate::database::{queries, Database};
use crate::events::{effective_minutes, occurrence};

/// Which time window "Hours Per Subject" is currently showing
/// (feature request: "Improved Hours Per Subject View"). `Week` is
/// the default — matches how someone actually plans their revision
/// day to day, with `Month`/`Year` available for a longer-range view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HoursPeriod {
    #[default]
    Week,
    Month,
    Year,
}

impl HoursPeriod {
    pub fn label(self) -> &'static str {
        match self {
            HoursPeriod::Week => "This Week",
            HoursPeriod::Month => "This Month",
            HoursPeriod::Year => "This Year",
        }
    }
}

/// The inclusive `[start, end]` date range `period` covers, anchored
/// on `today`. Pulled out as its own pure function so the boundary
/// math (month/year-end handling in particular) is unit-tested
/// directly rather than only visible through the chart it feeds.
pub fn period_bounds(period: HoursPeriod, today: NaiveDate) -> (NaiveDate, NaiveDate) {
    match period {
        HoursPeriod::Week => {
            let start = today - Duration::days(today.weekday().num_days_from_monday() as i64);
            (start, start + Duration::days(6))
        }
        HoursPeriod::Month => {
            let start = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
            let end = if today.month() == 12 {
                NaiveDate::from_ymd_opt(today.year() + 1, 1, 1).unwrap()
            } else {
                NaiveDate::from_ymd_opt(today.year(), today.month() + 1, 1).unwrap()
            } - Duration::days(1);
            (start, end)
        }
        HoursPeriod::Year => (
            NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(today.year(), 12, 31).unwrap(),
        ),
    }
}

/// Total effective minutes per `subject_id`, across every occurrence
/// of every event with a subject that falls within `[range_start,
/// range_end]` — "Hours Per Subject" for the Dashboard stat tracker.
/// Occurrences after today don't count yet (the range is clamped to
/// today, same as the unbounded version this replaced always did); a
/// logged outcome (`session_log`) overrides the default assumption
/// that a session happened exactly as scheduled.
pub fn minutes_per_subject_in_range(db: &Database, range_start: NaiveDate, range_end: NaiveDate) -> HashMap<i64, i64> {
    let conn = &db.conn;
    let today = Local::now().date_naive();
    let range_end = range_end.min(today);
    let events = queries::events::list_all_events(conn).unwrap_or_default();
    let logs = queries::session_log::list_all_logs(conn).unwrap_or_default();

    let mut totals: HashMap<i64, i64> = HashMap::new();
    if range_start > range_end {
        return totals;
    }

    for event in &events {
        let Some(subject_id) = event.subject_id else { continue };
        let scheduled_minutes = event.duration().num_minutes().max(0);
        let anchor = event.day();

        match event.recurrence_id {
            None => {
                if anchor >= range_start && anchor <= range_end {
                    let log = logs.iter().find(|l| l.event_id == event.id && l.occurrence_date == anchor);
                    *totals.entry(subject_id).or_insert(0) += effective_minutes(scheduled_minutes, log);
                }
            }
            Some(rule_id) => {
                let Ok(Some(rule)) = queries::recurrence::get_rule(conn, rule_id) else { continue };
                let exceptions = queries::recurrence::list_exceptions(conn, event.id).unwrap_or_default();
                for date in occurrence::occurrences_in_range(anchor, &rule, range_start, range_end) {
                    if exceptions.contains(&date) {
                        continue;
                    }
                    let log = logs.iter().find(|l| l.event_id == event.id && l.occurrence_date == date);
                    *totals.entry(subject_id).or_insert(0) += effective_minutes(scheduled_minutes, log);
                }
            }
        }
    }

    // Feature request: "Task Timings" — a standalone task's estimated
    // time counts toward its subject's total too, once completed, but
    // *only* if it isn't linked to an event: a task attached to a
    // revision session is already covered by that session's own
    // logged/scheduled time above, and adding the estimate on top
    // would double-count the same work. Attributed to whichever date
    // it was actually completed on, for range filtering — matching
    // how a session's minutes are anchored to its occurrence date.
    let tasks = queries::tasks::list_tasks(conn).unwrap_or_default();
    for task in &tasks {
        let (Some(subject_id), Some(minutes)) = (task.subject_id, task.estimated_minutes) else { continue };
        if !task.completed || task.event_id.is_some() {
            continue;
        }
        let Some(completed_date) = task
            .completed_at
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Local).date_naive())
        else {
            continue;
        };
        if completed_date >= range_start && completed_date <= range_end {
            *totals.entry(subject_id).or_insert(0) += minutes;
        }
    }

    totals
}

/// Same totals as `minutes_per_subject_in_range`, but with every
/// subtopic's minutes folded into its parent subject, and the
/// subtopic itself dropped from the result — feature request:
/// "Subject Sub Topics" ("the time spent on them added to the time
/// spent for the parent subject"). This is what every chart that
/// aggregates "hours by subject" should call instead of the raw
/// function, so a subtopic's time is reflected exactly once, in its
/// parent's total, rather than counted twice if the subtopic also
/// showed up as its own separate slice/line/bar right next to it. A
/// top-level subject's own key is present only if it (or one of its
/// subtopics) actually has logged time — same "don't show empty
/// entries" behavior the raw function already has.
pub fn minutes_per_subject_rolled_up(db: &Database, range_start: NaiveDate, range_end: NaiveDate) -> HashMap<i64, i64> {
    let raw = minutes_per_subject_in_range(db, range_start, range_end);
    let subjects = queries::subjects::list_all_subjects(&db.conn).unwrap_or_default();

    let mut rolled_up: HashMap<i64, i64> = HashMap::new();
    for subject in subjects.iter().filter(|s| s.parent_subject_id.is_none()) {
        let own = raw.get(&subject.id).copied().unwrap_or(0);
        let children_total: i64 = subjects
            .iter()
            .filter(|child| child.parent_subject_id == Some(subject.id))
            .map(|child| raw.get(&child.id).copied().unwrap_or(0))
            .sum();
        let total = own + children_total;
        if total > 0 {
            rolled_up.insert(subject.id, total);
        }
    }
    rolled_up
}
/// standalone task/event — see below), for each of the last
/// `num_weeks` weeks including the current one, oldest first —
/// feature request: "Total time by Week". Each week's total is
/// exactly what `minutes_per_subject_in_range` would sum to across
/// all subjects for that week's `[Monday, Sunday]` range — "the sum
/// of all hours, through tasks, events etc," per the request's own
/// wording — computed by reusing that same function's per-subject
/// breakdown and summing it, rather than a second, separate
/// aggregation query that could drift out of sync with it.
pub fn total_minutes_by_week(db: &Database, num_weeks: usize) -> Vec<(NaiveDate, i64)> {
    let today = Local::now().date_naive();
    let num_weeks = num_weeks.max(1);

    (0..num_weeks)
        .rev()
        .map(|weeks_ago| {
            let anchor = today - Duration::weeks(weeks_ago as i64);
            let (start, end) = period_bounds(HoursPeriod::Week, anchor);
            let total: i64 = minutes_per_subject_in_range(db, start, end).values().sum();
            (start, total)
        })
        .collect()
}

/// Per-subject minutes for each of the last `num_weeks` weeks
/// including the current one, oldest first — feature request:
/// "Improved Line Graph" (show a line per subject on the "Total time
/// by Week" chart, instead of one combined line). Each week's map is
/// exactly what `total_minutes_by_week` would have summed for that
/// week (both roll up subtopics into their parent, via
/// `minutes_per_subject_rolled_up` — feature request: "Subject Sub
/// Topics" — so the two can never drift apart on what counts toward
/// a subject's line, any more than they already couldn't on what
/// counts as a "week").
pub fn total_minutes_by_week_per_subject(db: &Database, num_weeks: usize) -> Vec<(NaiveDate, HashMap<i64, i64>)> {
    let today = Local::now().date_naive();
    let num_weeks = num_weeks.max(1);

    (0..num_weeks)
        .rev()
        .map(|weeks_ago| {
            let anchor = today - Duration::weeks(weeks_ago as i64);
            let (start, end) = period_bounds(HoursPeriod::Week, anchor);
            (start, minutes_per_subject_rolled_up(db, start, end))
        })
        .collect()
}

/// A human-readable comparison of the most recent two entries from
/// `total_minutes_by_week` — feature request: "up 14% from last
/// week." `None` if there isn't at least two weeks of data to compare
/// (nothing to compare the very first tracked week against).
pub fn week_over_week_change(weeks: &[(NaiveDate, i64)]) -> Option<String> {
    let &[.., (_, previous), (_, current)] = weeks else { return None };

    if previous == 0 {
        return if current == 0 {
            None
        } else {
            Some("no time logged last week to compare against".to_owned())
        };
    }

    let percent = ((current - previous) as f64 / previous as f64 * 100.0).round() as i64;
    Some(match percent.cmp(&0) {
        std::cmp::Ordering::Greater => format!("up {percent}% from last week"),
        std::cmp::Ordering::Less => format!("down {}% from last week", percent.abs()),
        std::cmp::Ordering::Equal => "unchanged from last week".to_owned(),
    })
}

/// How many tasks were completed on each day — the data behind the
/// "Tasks Done By Day" GitHub-style heatmap. Built directly on
/// `tasks::list_completed_dates`, which already unions still-live
/// tasks' `completed_at` with the durable `completion_log` (Sprint 10
/// feedback pass), so a task getting auto-cleaned-up after its
/// fulfilled day doesn't drop it from the heatmap either.
pub fn tasks_done_by_day(db: &Database) -> HashMap<NaiveDate, usize> {
    let dates = queries::tasks::list_completed_dates(&db.conn).unwrap_or_default();
    let mut counts = HashMap::new();
    for date in dates {
        *counts.entry(date).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::queries::{events, session_log};
    use crate::database::Database;
    use crate::events::SessionStatus;
    use chrono::Duration;

    #[test]
    fn minutes_per_subject_counts_scheduled_duration_by_default() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let subject = queries::subjects::create_subject(&db.conn, "Physics").expect("subject");
        let start = Local::now() - Duration::days(1);
        let end = start + Duration::hours(2);
        events::create_event(&db.conn, "Revision", "", start, end, Some(subject.id), None).expect("event");

        let today = Local::now().date_naive();
        let range = (today - Duration::days(30), today);
        let totals = minutes_per_subject_in_range(&db, range.0, range.1);
        assert_eq!(totals.get(&subject.id), Some(&120));
    }

    #[test]
    fn minutes_per_subject_excludes_future_occurrences() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let subject = queries::subjects::create_subject(&db.conn, "Physics").expect("subject");
        let start = Local::now() + Duration::days(3);
        let end = start + Duration::hours(1);
        events::create_event(&db.conn, "Future session", "", start, end, Some(subject.id), None).expect("event");

        let today = Local::now().date_naive();
        let range = (today - Duration::days(30), today);
        let totals = minutes_per_subject_in_range(&db, range.0, range.1);
        assert_eq!(totals.get(&subject.id), None);
    }

    #[test]
    fn minutes_per_subject_respects_a_denied_log_entry() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let subject = queries::subjects::create_subject(&db.conn, "Physics").expect("subject");
        let start = Local::now() - Duration::days(1);
        let end = start + Duration::hours(2);
        let event =
            events::create_event(&db.conn, "Revision", "", start, end, Some(subject.id), None).expect("event");
        session_log::log_session(&db.conn, event.id, event.day(), SessionStatus::Denied, None).expect("log");

        let today = Local::now().date_naive();
        let range = (today - Duration::days(30), today);
        let totals = minutes_per_subject_in_range(&db, range.0, range.1);
        assert_eq!(
            totals.get(&subject.id),
            Some(&0),
            "a denied session contributes zero minutes but the subject still appears, at 0h"
        );
    }

    #[test]
    fn minutes_per_subject_respects_an_adjusted_log_entry() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let subject = queries::subjects::create_subject(&db.conn, "Physics").expect("subject");
        let start = Local::now() - Duration::days(1);
        let end = start + Duration::hours(1);
        let event =
            events::create_event(&db.conn, "Revision", "", start, end, Some(subject.id), None).expect("event");
        session_log::log_session(&db.conn, event.id, event.day(), SessionStatus::Adjusted, Some(150)).expect("log");

        let today = Local::now().date_naive();
        let range = (today - Duration::days(30), today);
        let totals = minutes_per_subject_in_range(&db, range.0, range.1);
        assert_eq!(totals.get(&subject.id), Some(&150));
    }

    #[test]
    fn minutes_per_subject_counts_a_subject_linked_pomodoro_session() {
        // Mirrors exactly what `AppState::tick_pomodoro` does when a
        // subject is attached to a completed Work phase: create a
        // one-off event for the block, then log it Completed — the
        // "Pomodoro Subject Links" feature deliberately reuses this
        // pipeline rather than adding a second way to accumulate
        // subject minutes, so this test is really confirming that
        // reuse actually works end to end, not just each half of it
        // in isolation.
        let db = Database::open_in_memory().expect("open in-memory db");
        let subject = queries::subjects::create_subject(&db.conn, "Chemistry").expect("subject");
        let end = Local::now();
        let start = end - Duration::minutes(25);
        let event = events::create_event(&db.conn, "Pomodoro focus session", "", start, end, Some(subject.id), None)
            .expect("event");
        session_log::log_session(&db.conn, event.id, event.day(), SessionStatus::Completed, None).expect("log");

        let today = Local::now().date_naive();
        let range = (today - Duration::days(30), today);
        let totals = minutes_per_subject_in_range(&db, range.0, range.1);
        assert_eq!(totals.get(&subject.id), Some(&25));
    }

    #[test]
    fn tasks_done_by_day_counts_multiple_tasks_on_the_same_day() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let a = queries::tasks::create_task(&db.conn, "A", None, None, None).expect("a");
        let b = queries::tasks::create_task(&db.conn, "B", None, None, None).expect("b");
        queries::tasks::set_completed(&db.conn, a.id, true).expect("complete a");
        queries::tasks::set_completed(&db.conn, b.id, true).expect("complete b");

        let counts = tasks_done_by_day(&db);
        let today = Local::now().date_naive();
        assert_eq!(counts.get(&today), Some(&2));
    }

    #[test]
    fn period_bounds_week_is_monday_to_sunday() {
        // 2026-08-27 is a Thursday.
        let today = NaiveDate::from_ymd_opt(2026, 8, 27).unwrap();
        let (start, end) = period_bounds(HoursPeriod::Week, today);
        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 8, 24).unwrap());
        assert_eq!(end, NaiveDate::from_ymd_opt(2026, 8, 30).unwrap());
    }

    #[test]
    fn period_bounds_month_spans_the_whole_month() {
        let today = NaiveDate::from_ymd_opt(2026, 2, 15).unwrap();
        let (start, end) = period_bounds(HoursPeriod::Month, today);
        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 2, 1).unwrap());
        assert_eq!(end, NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());
    }

    #[test]
    fn period_bounds_month_handles_december_correctly() {
        // The year-rollover edge case in the month-end calculation.
        let today = NaiveDate::from_ymd_opt(2026, 12, 10).unwrap();
        let (start, end) = period_bounds(HoursPeriod::Month, today);
        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 12, 1).unwrap());
        assert_eq!(end, NaiveDate::from_ymd_opt(2026, 12, 31).unwrap());
    }

    #[test]
    fn period_bounds_year_is_jan_1_to_dec_31() {
        let today = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let (start, end) = period_bounds(HoursPeriod::Year, today);
        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        assert_eq!(end, NaiveDate::from_ymd_opt(2026, 12, 31).unwrap());
    }

    #[test]
    fn minutes_per_subject_in_range_excludes_occurrences_outside_the_window() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let subject = queries::subjects::create_subject(&db.conn, "Physics").expect("subject");
        // 10 days ago — outside a "this week" range.
        let start = Local::now() - Duration::days(10);
        let end = start + Duration::hours(1);
        events::create_event(&db.conn, "Old session", "", start, end, Some(subject.id), None).expect("event");

        let today = Local::now().date_naive();
        let (week_start, week_end) = period_bounds(HoursPeriod::Week, today);
        let totals = minutes_per_subject_in_range(&db, week_start, week_end);
        assert_eq!(totals.get(&subject.id), None);
    }

    #[test]
    fn minutes_per_subject_in_range_counts_a_completed_standalone_tasks_estimate() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let subject = queries::subjects::create_subject(&db.conn, "Chemistry").expect("subject");
        let task = queries::tasks::create_task(&db.conn, "Read chapter 2", None, None, Some(subject.id)).expect("task");
        queries::tasks::update_task(&db.conn, task.id, "Read chapter 2", "", None, None, Some(subject.id), false, None, Some(45))
            .expect("set estimate");
        queries::tasks::set_completed(&db.conn, task.id, true).expect("complete");

        let today = Local::now().date_naive();
        let (week_start, week_end) = period_bounds(HoursPeriod::Week, today);
        let totals = minutes_per_subject_in_range(&db, week_start, week_end);
        assert_eq!(totals.get(&subject.id), Some(&45));
    }

    #[test]
    fn minutes_per_subject_in_range_ignores_estimate_for_a_task_linked_to_an_event() {
        // Avoids double-counting: the linked event's own scheduled/
        // logged time already covers this work.
        let db = Database::open_in_memory().expect("open in-memory db");
        let subject = queries::subjects::create_subject(&db.conn, "Biology").expect("subject");
        let start = Local::now();
        let end = start + Duration::hours(1);
        let event =
            events::create_event(&db.conn, "Revision block", "", start, end, Some(subject.id), None).expect("event");
        let task =
            queries::tasks::create_task(&db.conn, "Read notes", None, Some(event.id), Some(subject.id)).expect("task");
        queries::tasks::update_task(
            &db.conn,
            task.id,
            "Read notes",
            "",
            None,
            Some(event.id),
            Some(subject.id),
            false,
            None,
            Some(30),
        )
        .expect("set estimate");
        queries::tasks::set_completed(&db.conn, task.id, true).expect("complete");

        let today = Local::now().date_naive();
        let (week_start, week_end) = period_bounds(HoursPeriod::Week, today);
        let totals = minutes_per_subject_in_range(&db, week_start, week_end);
        // 60 minutes from the event alone — the task's 30-minute
        // estimate must not be added on top.
        assert_eq!(totals.get(&subject.id), Some(&60));
    }

    #[test]
    fn minutes_per_subject_in_range_ignores_estimate_for_an_incomplete_task() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let subject = queries::subjects::create_subject(&db.conn, "History").expect("subject");
        let task = queries::tasks::create_task(&db.conn, "Read chapter 3", None, None, Some(subject.id)).expect("task");
        queries::tasks::update_task(&db.conn, task.id, "Read chapter 3", "", None, None, Some(subject.id), false, None, Some(45))
            .expect("set estimate");

        let today = Local::now().date_naive();
        let (week_start, week_end) = period_bounds(HoursPeriod::Week, today);
        let totals = minutes_per_subject_in_range(&db, week_start, week_end);
        assert_eq!(totals.get(&subject.id), None);
    }

    #[test]
    fn rolled_up_adds_a_subtopics_minutes_to_its_parent() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let physics = queries::subjects::create_subject(&db.conn, "Physics").expect("parent");
        let mechanics = queries::subjects::create_subtopic(&db.conn, physics.id, "Mechanics").expect("subtopic");

        let start = Local::now();
        events::create_event(&db.conn, "Physics block", "", start, start + Duration::minutes(20), Some(physics.id), None)
            .expect("event a");
        events::create_event(&db.conn, "Mechanics block", "", start, start + Duration::minutes(40), Some(mechanics.id), None)
            .expect("event b");

        let today = Local::now().date_naive();
        let (week_start, week_end) = period_bounds(HoursPeriod::Week, today);
        let rolled_up = minutes_per_subject_rolled_up(&db, week_start, week_end);

        assert_eq!(rolled_up.get(&physics.id), Some(&60), "parent's total includes its own time plus its subtopic's");
        assert_eq!(rolled_up.get(&mechanics.id), None, "a subtopic never appears as its own separate entry once rolled up");
    }

    #[test]
    fn rolled_up_includes_a_subject_with_only_a_subtopics_time_logged() {
        // The parent itself might have zero minutes of its own — its
        // rolled-up total should still show up, entirely from its
        // subtopic's time.
        let db = Database::open_in_memory().expect("open in-memory db");
        let physics = queries::subjects::create_subject(&db.conn, "Physics").expect("parent");
        let mechanics = queries::subjects::create_subtopic(&db.conn, physics.id, "Mechanics").expect("subtopic");

        let start = Local::now();
        events::create_event(&db.conn, "Mechanics block", "", start, start + Duration::minutes(30), Some(mechanics.id), None)
            .expect("event");

        let today = Local::now().date_naive();
        let (week_start, week_end) = period_bounds(HoursPeriod::Week, today);
        let rolled_up = minutes_per_subject_rolled_up(&db, week_start, week_end);
        assert_eq!(rolled_up.get(&physics.id), Some(&30));
    }

    #[test]
    fn rolled_up_matches_the_raw_total_when_there_are_no_subtopics_at_all() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let physics = queries::subjects::create_subject(&db.conn, "Physics").expect("subject");
        let start = Local::now();
        events::create_event(&db.conn, "Physics block", "", start, start + Duration::minutes(45), Some(physics.id), None)
            .expect("event");

        let today = Local::now().date_naive();
        let (week_start, week_end) = period_bounds(HoursPeriod::Week, today);
        let raw = minutes_per_subject_in_range(&db, week_start, week_end);
        let rolled_up = minutes_per_subject_rolled_up(&db, week_start, week_end);
        assert_eq!(raw, rolled_up);
    }

    #[test]
    fn total_minutes_by_week_returns_the_requested_number_of_weeks_oldest_first() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let weeks = total_minutes_by_week(&db, 4);
        assert_eq!(weeks.len(), 4);
        // Oldest first: each entry's Monday should be strictly after
        // the previous entry's.
        for pair in weeks.windows(2) {
            assert!(pair[0].0 < pair[1].0);
        }
        // Last entry is the current week.
        let today = Local::now().date_naive();
        let (this_week_start, _) = period_bounds(HoursPeriod::Week, today);
        assert_eq!(weeks.last().unwrap().0, this_week_start);
    }

    #[test]
    fn total_minutes_by_week_sums_across_every_subject() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let physics = queries::subjects::create_subject(&db.conn, "Physics").expect("subject");
        let chem = queries::subjects::create_subject(&db.conn, "Chemistry").expect("subject");
        let start = Local::now();
        events::create_event(&db.conn, "Physics block", "", start, start + Duration::hours(1), Some(physics.id), None)
            .expect("event a");
        events::create_event(&db.conn, "Chem block", "", start, start + Duration::minutes(30), Some(chem.id), None)
            .expect("event b");

        let weeks = total_minutes_by_week(&db, 1);
        assert_eq!(weeks[0].1, 90);
    }

    #[test]
    fn total_minutes_by_week_per_subject_keeps_each_subject_separate() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let physics = queries::subjects::create_subject(&db.conn, "Physics").expect("subject");
        let chem = queries::subjects::create_subject(&db.conn, "Chemistry").expect("subject");
        let start = Local::now();
        events::create_event(&db.conn, "Physics block", "", start, start + Duration::hours(1), Some(physics.id), None)
            .expect("event a");
        events::create_event(&db.conn, "Chem block", "", start, start + Duration::minutes(30), Some(chem.id), None)
            .expect("event b");

        let weeks = total_minutes_by_week_per_subject(&db, 1);
        assert_eq!(weeks.len(), 1);
        assert_eq!(weeks[0].1.get(&physics.id), Some(&60));
        assert_eq!(weeks[0].1.get(&chem.id), Some(&30));
    }

    #[test]
    fn total_minutes_by_week_per_subject_matches_the_summed_total() {
        // The two functions must never drift apart on what counts as
        // a "week" or what counts toward it — this pins that down
        // directly rather than trusting it by inspection.
        let db = Database::open_in_memory().expect("open in-memory db");
        let physics = queries::subjects::create_subject(&db.conn, "Physics").expect("subject");
        let chem = queries::subjects::create_subject(&db.conn, "Chemistry").expect("subject");
        let start = Local::now();
        events::create_event(&db.conn, "Physics block", "", start, start + Duration::hours(1), Some(physics.id), None)
            .expect("event a");
        events::create_event(&db.conn, "Chem block", "", start, start + Duration::minutes(30), Some(chem.id), None)
            .expect("event b");

        let totals = total_minutes_by_week(&db, 3);
        let per_subject = total_minutes_by_week_per_subject(&db, 3);
        assert_eq!(totals.len(), per_subject.len());
        for ((total_week, total_minutes), (per_subject_week, per_subject_minutes)) in totals.iter().zip(&per_subject) {
            assert_eq!(total_week, per_subject_week);
            assert_eq!(*total_minutes, per_subject_minutes.values().sum::<i64>());
        }
    }

    #[test]
    fn week_over_week_change_reports_an_increase() {
        let today = Local::now().date_naive();
        let weeks = vec![(today - Duration::weeks(1), 60), (today, 90)];
        assert_eq!(week_over_week_change(&weeks).as_deref(), Some("up 50% from last week"));
    }

    #[test]
    fn week_over_week_change_reports_a_decrease() {
        let today = Local::now().date_naive();
        let weeks = vec![(today - Duration::weeks(1), 100), (today, 75)];
        assert_eq!(week_over_week_change(&weeks).as_deref(), Some("down 25% from last week"));
    }

    #[test]
    fn week_over_week_change_handles_no_previous_data_without_dividing_by_zero() {
        let today = Local::now().date_naive();
        let weeks = vec![(today - Duration::weeks(1), 0), (today, 60)];
        assert_eq!(week_over_week_change(&weeks).as_deref(), Some("no time logged last week to compare against"));
    }

    #[test]
    fn week_over_week_change_is_none_with_fewer_than_two_weeks() {
        let today = Local::now().date_naive();
        assert_eq!(week_over_week_change(&[(today, 60)]), None);
        assert_eq!(week_over_week_change(&[]), None);
    }
}
