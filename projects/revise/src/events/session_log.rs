use chrono::NaiveDate;

/// How a specific occurrence of a revision session actually went,
/// reported by the user after the fact. Keyed by `(event_id,
/// occurrence_date)` rather than being a property of the `Event`
/// itself — a recurring session can go differently each week, and
/// logging one occurrence must never touch the rule or any other
/// occurrence (the same principle Sprint 6's exceptions follow for
/// skipping).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    /// Happened as scheduled — counts its full scheduled duration.
    Completed,
    /// Didn't happen — "they should be able to report this to not
    /// have the false data tracked". Counts as zero.
    Denied,
    /// Happened, but for a different duration than scheduled —
    /// "account for additional hours which they have done" (or
    /// fewer). Counts `actual_minutes`.
    Adjusted,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionStatus::Completed => "completed",
            SessionStatus::Denied => "denied",
            SessionStatus::Adjusted => "adjusted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "completed" => Some(SessionStatus::Completed),
            "denied" => Some(SessionStatus::Denied),
            "adjusted" => Some(SessionStatus::Adjusted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionLogEntry {
    pub id: i64,
    pub event_id: i64,
    pub occurrence_date: NaiveDate,
    pub status: SessionStatus,
    pub actual_minutes: Option<i64>,
}

/// How many minutes a specific occurrence should count for
/// "hours per subject" style stats: its logged outcome if one
/// exists, otherwise its full scheduled duration (the same
/// "assume it happened unless told otherwise" default the rest of
/// the app uses — e.g. task due dates were free text long before
/// they were validated).
///
/// Pure and independent of the database/UI, so this rule is tested
/// directly.
pub fn effective_minutes(scheduled_minutes: i64, log: Option<&SessionLogEntry>) -> i64 {
    match log {
        None => scheduled_minutes,
        Some(entry) => match entry.status {
            SessionStatus::Completed => scheduled_minutes,
            SessionStatus::Denied => 0,
            SessionStatus::Adjusted => entry.actual_minutes.unwrap_or(scheduled_minutes),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(status: SessionStatus, actual_minutes: Option<i64>) -> SessionLogEntry {
        SessionLogEntry {
            id: 1,
            event_id: 1,
            occurrence_date: NaiveDate::from_ymd_opt(2026, 8, 25).unwrap(),
            status,
            actual_minutes,
        }
    }

    #[test]
    fn no_log_entry_defaults_to_scheduled_duration() {
        assert_eq!(effective_minutes(60, None), 60);
    }

    #[test]
    fn completed_counts_the_full_scheduled_duration() {
        assert_eq!(effective_minutes(60, Some(&entry(SessionStatus::Completed, None))), 60);
    }

    #[test]
    fn denied_counts_as_zero() {
        assert_eq!(effective_minutes(60, Some(&entry(SessionStatus::Denied, None))), 0);
    }

    #[test]
    fn adjusted_counts_the_actual_minutes() {
        assert_eq!(effective_minutes(60, Some(&entry(SessionStatus::Adjusted, Some(90)))), 90);
    }

    #[test]
    fn adjusted_with_no_actual_minutes_falls_back_to_scheduled() {
        assert_eq!(effective_minutes(60, Some(&entry(SessionStatus::Adjusted, None))), 60);
    }

    #[test]
    fn status_round_trips_through_storage_strings() {
        for status in [SessionStatus::Completed, SessionStatus::Denied, SessionStatus::Adjusted] {
            assert_eq!(SessionStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(SessionStatus::parse("garbage"), None);
    }
}
