/// The initial task shape from the plan's Section 9 data model.
/// `position` is Revise-internal (drives manual ordering) and isn't
/// part of the plan's diagram, but is required to satisfy "task
/// ordering" as a persisted, user-controllable property.
#[derive(Debug, Clone)]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub completed: bool,
    pub created_at: String,
    pub due_date: Option<String>,
    pub position: i64,
    /// The revision session this task belongs to, if any (Sprint 4).
    /// Tasks are free-standing by default — this is opt-in.
    pub event_id: Option<i64>,
    /// When this task was last marked complete (RFC3339), cleared to
    /// `None` when un-completed. Drives the Dashboard's basic streak
    /// (Sprint 5) — a day "counts" if at least one task was completed
    /// on it.
    pub completed_at: Option<String>,
    /// The revision subject this task belongs to, if any (Sprint 9).
    /// Independent of `event_id` — a task can carry a subject whether
    /// or not it's attached to a session.
    pub subject_id: Option<i64>,
    /// The specific day this task is *planned* to be worked on, if
    /// any (feature request: "Task Assignment") — independent of
    /// `due_date`, which is when it's actually due. `Today's Tasks`
    /// (Dashboard) and the Calendar Day view's "Tasks for this day"
    /// section both read this, not `due_date`, for "should this show
    /// up today" — a task due next week that's been assigned to today
    /// shows up today; a task due today that hasn't been assigned
    /// anywhere still shows up today too (the pre-existing default
    /// behavior this feature is additive to, not a replacement for).
    pub assigned_date: Option<String>,
    /// Feature request: "Repeating Tasks" — per the request's own
    /// scope, weekly only, originally. Still the master on/off switch
    /// after "Tasks Repeating By Day" added a second cadence — a task
    /// "repeats" (this flag) at whatever cadence
    /// `repeat_interval_days` says. See `queries::tasks::set_completed`
    /// for where this is actually applied.
    pub repeat_weekly: bool,
    /// Feature request: "Sub Tasks" — `None` means top-level.
    /// A task with children can only be completed once all of them
    /// are (enforced in `queries::tasks::set_completed`).
    pub parent_task_id: Option<i64>,
    /// Feature request: "Task Timings" — an optional self-reported
    /// estimate, in minutes, independent of any linked event's own
    /// duration.
    pub estimated_minutes: Option<i64>,
    /// Feature request: "Tasks Repeating Through A List" — a
    /// sub-option of `repeat_weekly` (meaningless without it). When
    /// set, `title` isn't the "real" name any more — it's
    /// `"{repeat_list_base_title}: {current topic}"`, regenerated
    /// each time `queries::tasks::reset_due_repeating_tasks` resets
    /// this task, advancing through `repeat_list_topics` according to
    /// `repeat_list_mode`. See `tasks::repeat_list` for the actual
    /// topic-selection logic.
    pub repeat_through_list: bool,
    /// The task's name with no topic suffix — what the person
    /// actually typed into the Title field. `title` itself becomes
    /// derived (`"{base}: {topic}"`) once `repeat_through_list` is on,
    /// so this is what a future edit should show back in that field,
    /// not the combined `title`.
    pub repeat_list_base_title: Option<String>,
    /// Newline-delimited list of topics to cycle through — parsed by
    /// `tasks::repeat_list::parse_topics`, the same shape
    /// `flashcards::import::parse_pairs` already uses for one-item-
    /// per-line text.
    pub repeat_list_topics: Option<String>,
    /// Which order `repeat_list_topics` advances in — see
    /// `tasks::repeat_list::RepeatListMode`.
    pub repeat_list_mode: Option<String>,
    /// `RepeatListMode`-specific encoding of "where things currently
    /// are" in the list — an index, a shuffled queue, or per-topic
    /// show-counts, depending on `repeat_list_mode`. Opaque outside
    /// `tasks::repeat_list`; never meant to be read or written
    /// directly anywhere else.
    pub repeat_list_state: Option<String>,
    /// Feature request: "Tasks Repeating By Day" — "like repeating
    /// events, tasks should have the option to repeat by day." How
    /// many days `reset_due_repeating_tasks` advances `due_date` by
    /// each time a `repeat_weekly` task resets: `1` for daily, `7` for
    /// weekly (the app's only two choices, offered as a Day/Week
    /// picker in the edit modal — not a free-form number — same as
    /// `repeat_list_mode` offers a fixed choice rather than open text).
    /// Every task predating this request keeps its implicit weekly
    /// cadence via the column's `DEFAULT 7`, so `repeat_weekly` alone
    /// still means exactly what it always did for them. A field of
    /// its own rather than folding into `repeat_weekly` itself (e.g. a
    /// `Frequency` enum) — the request only asked for one more
    /// cadence, not the open-ended recurrence model
    /// `events::Frequency`/`RecurrenceRule` already have; reusing that
    /// richer model here for two fixed choices would be more
    /// machinery than this feature calls for.
    pub repeat_interval_days: i64,
    /// RETIRED. This used to turn a repeat-through-list task into a
    /// hidden *template* that Auto-assign filled with one generated
    /// task per day. Per the request "tasks which loop through a list
    /// should only show the parent task, renamed daily to that day's
    /// topic", nothing sets it any more: a topic-list task is always a
    /// single task, renamed by `reset_due_repeating_tasks`. The column
    /// remains (SQLite can't cheaply drop it) and
    /// `queries::tasks::retire_topic_planning` clears it on any row
    /// saved under the old scheme, so it is `false` in practice.
    pub plan_topics_ahead: bool,
    /// RETIRED with `plan_topics_ahead`: the template a per-day task was
    /// generated from. `retire_topic_planning` deletes the unfinished
    /// ones; finished ones are cleaned up like any completed task.
    pub generated_from: Option<i64>,
    /// `assigned_date` was chosen by Auto-assign rather than a
    /// person, so the next run is free to move it. Cleared by any
    /// manual assignment (`queries::tasks::set_assigned_date`).
    pub auto_assigned: bool,
}
