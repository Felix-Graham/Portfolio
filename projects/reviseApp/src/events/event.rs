use chrono::{DateTime, Duration, Local, NaiveDate};

/// A scheduled revision session (Section 10's data model). Storage
/// keeps `start_time`/`end_time` as RFC3339 strings; this struct holds
/// them parsed so callers get real datetime math (duration, day
/// membership, midnight-crossing) instead of re-parsing everywhere.
#[derive(Debug, Clone)]
pub struct Event {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub start_time: DateTime<Local>,
    pub end_time: DateTime<Local>,
    /// The revision subject this session belongs to, if any (Sprint 9
    /// — replaces the free-text label used through Sprint 8).
    pub subject_id: Option<i64>,
    /// If this event repeats, the id of its `RecurrenceRule`. `None`
    /// means it's a one-off. This row's own `start_time`/`end_time`
    /// always represent the *first* occurrence — the rule projects
    /// the rest forward without duplicating this row (Sprint 6).
    pub recurrence_id: Option<i64>,
    /// Feature request: "Startup Scripts/Commands" — a shell command
    /// to run once, at this event's own start time (e.g. opening a
    /// browser or a terminal session). `None` is the default and
    /// overwhelmingly common case; see `notifications::check_reminders`
    /// for where this actually gets invoked, gated behind its own
    /// opt-in setting given the security surface of running an
    /// arbitrary shell string unattended.
    pub exec_command: Option<String>,
    /// Feature request: "Event Logging" — whether this event's
    /// occurrences should show the Done/Not-Done session-logging
    /// controls and the "Start Session" button at all. `true` (the
    /// default, preserving every pre-existing event's behavior) means
    /// yes; `false` is for calendar blocks that were never a revision
    /// session to begin with — a meeting, say — so they don't carry
    /// controls that don't apply to them.
    pub track_session: bool,
}

impl Event {
    pub fn duration(&self) -> Duration {
        self.end_time - self.start_time
    }

    /// True if the event's end falls on a different calendar day than
    /// its start — the "crossing normal hour boundaries" case the
    /// Sprint 3 acceptance criteria call out explicitly.
    pub fn crosses_midnight(&self) -> bool {
        self.start_time.date_naive() != self.end_time.date_naive()
    }

    /// Which day this event belongs to on the calendar. An event is
    /// shown on the day it starts, even if it runs past midnight.
    pub fn day(&self) -> NaiveDate {
        self.start_time.date_naive()
    }
}
