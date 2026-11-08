/// A revision subject — Mathematics, Physics, and so on (Section 16's
/// "first-class organisational concept"). Shared by events, tasks,
/// and notes via a `subject_id` foreign key on each.
///
/// `created_at` is informational only and deliberately never parsed
/// as a real datetime: rows created by the app use
/// `chrono::Local::now().to_rfc3339()`, but the Sprint 9 migration
/// backfills subjects from pre-existing free-text values using
/// SQLite's `datetime('now')`, which isn't RFC3339. Both are valid
/// human-readable strings; nothing does date math on this field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    pub id: i64,
    pub name: String,
    /// Archived subjects stay assignable to nothing new but remain
    /// fully intact on whatever already references them — "archived
    /// subjects remain available for historical data" is the Sprint 9
    /// acceptance criterion this exists to satisfy. There is
    /// deliberately no delete.
    pub archived: bool,
    pub created_at: String,
    /// Feature request: "Subject Sub Topics" — one level of nesting
    /// only, same precedent as `Task::parent_task_id`/
    /// `Folder::parent_id`. `None` (every subject before this
    /// request, and every ordinary top-level subject since) means
    /// "top-level." A subtopic is a completely ordinary `Subject`
    /// otherwise — nothing about it is special-cased in the rest of
    /// the app, which is exactly what lets it be "referenced as its
    /// own subject" (the request's own words) on a task, event, or
    /// note with no further changes needed anywhere else. Its own
    /// logged time counts toward its parent's total too — see
    /// `stats::minutes_per_subject_rolled_up`.
    pub parent_subject_id: Option<i64>,
    /// Feature request: "Colour" — the person's own chosen color for
    /// this subject, as `"#rrggbb"` (the only format
    /// `theme::color_to_hex` ever produces, and the only one this
    /// field is ever set to). `None` — every subject before this
    /// request, and any new one until it's explicitly colored in
    /// "Manage Subjects" — means "use this id's entry in
    /// `theme::DEFAULT_SUBJECT_COLORS` instead," never "no color at
    /// all"; see `theme::subject_color`, the one place this field
    /// should be read from rather than matched on directly.
    pub color: Option<String>,
}
