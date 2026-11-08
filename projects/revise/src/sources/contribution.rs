use chrono::NaiveDate;

/// One block of time a registered `Source` reports having happened —
/// "it would add hours/contribute to statistics." Shaped like
/// `events::SessionLogEntry` (a subject, a date, a minute count) but
/// without an `event_id`: nothing scheduled this in Calendar, so there
/// is no event to log an outcome *against*. `stats::
/// minutes_per_subject_in_range` adds these straight into a subject's
/// total, the same way it already adds a completed standalone task's
/// `estimated_minutes` — see that function for why both exist as
/// separate small additions rather than trying to unify every source
/// of "time spent" into one table.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalContribution {
    pub id: i64,
    pub source_id: i64,
    pub subject_id: i64,
    pub occurred_on: NaiveDate,
    pub minutes: i64,
    pub note: Option<String>,
    pub created_at: String,
}
