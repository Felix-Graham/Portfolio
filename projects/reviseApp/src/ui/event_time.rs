use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime, TimeZone};

/// Parses "HH:MM" start/end strings against a given date into real
/// `DateTime<Local>` values, handling the midnight-crossing case: if
/// the end time is earlier than or equal to the start time, it's
/// treated as ending the next day.
///
/// Shared by the Calendar's New/Edit Event modal and the Dashboard's
/// quick-event form (Sprint 5), so both give identical error messages
/// and identical midnight-crossing behavior rather than two slightly
/// different reimplementations drifting apart.
pub fn parse_event_times(
    date: NaiveDate,
    start: &str,
    end: &str,
) -> Result<(DateTime<Local>, DateTime<Local>), String> {
    let start_time =
        NaiveTime::parse_from_str(start.trim(), "%H:%M").map_err(|_| "Start time must look like 09:00.".to_owned())?;
    let end_time =
        NaiveTime::parse_from_str(end.trim(), "%H:%M").map_err(|_| "End time must look like 10:00.".to_owned())?;

    let start_naive = date.and_time(start_time);
    let mut end_naive = date.and_time(end_time);
    if end_naive <= start_naive {
        end_naive += Duration::days(1);
    }

    let start = Local
        .from_local_datetime(&start_naive)
        .single()
        .ok_or_else(|| "That start time is ambiguous (DST change) — try a different time.".to_owned())?;
    let end = Local
        .from_local_datetime(&end_naive)
        .single()
        .ok_or_else(|| "That end time is ambiguous (DST change) — try a different time.".to_owned())?;

    Ok((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn ordinary_same_day_range() {
        let (start, end) = parse_event_times(date(2026, 8, 25), "09:00", "10:30").unwrap();
        assert_eq!(start.date_naive(), end.date_naive());
        assert_eq!((end - start).num_minutes(), 90);
    }

    #[test]
    fn end_before_start_rolls_to_next_day() {
        let (start, end) = parse_event_times(date(2026, 8, 25), "23:30", "00:30").unwrap();
        assert_eq!(end.date_naive(), start.date_naive() + Duration::days(1));
        assert_eq!((end - start).num_minutes(), 60);
    }

    #[test]
    fn malformed_time_is_rejected() {
        assert!(parse_event_times(date(2026, 8, 25), "9am", "10:00").is_err());
    }
}
