use std::collections::HashMap;

use chrono::{Datelike, Duration as ChronoDuration, Local, NaiveDate, Timelike, Weekday};
use eframe::egui::{self, Frame, CornerRadius, RichText, Stroke};

use crate::database::{queries, Database};
use crate::events::{occurrence, Event, Frequency, RecurrenceRule};
use crate::subjects::Subject;
use crate::tasks::Task;
use crate::theme::{Border, Spacing, Theme};
use crate::ui::components::{checkbox, Input, Modal, Panel, RButton};

const WEEKDAY_ORDER: [Weekday; 7] =
    [Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri, Weekday::Sat, Weekday::Sun];

/// Vertical scale for the Day view's proportional event bars: a
/// 1-hour event gets a bar this tall, a 2-hour event twice as tall,
/// and so on.
const PIXELS_PER_HOUR: f32 = 80.0;
/// Floor on the accent bar's height so a very short event (or one
/// with a zero/negative duration from bad data) still renders a
/// visible sliver rather than nothing.
const MIN_BAR_HEIGHT: f32 = 12.0;

/// The Day view's proportional accent-bar height for an event of a
/// given duration — pure and separate from rendering so the scaling
/// rule itself (not just the layout code that uses it) is tested
/// directly.
fn bar_height_for_duration(duration_minutes: f32) -> f32 {
    (duration_minutes / 60.0 * PIXELS_PER_HOUR).max(MIN_BAR_HEIGHT)
}

/// Vertical pixel offset from the top of the Day timeline for an
/// event starting at `start_time`, given the timeline's earliest
/// displayed hour. Pulled out from the rendering code so the actual
/// alignment math — the thing that was wrong before (an event's card
/// wasn't tied to the same scale as the hour ruler beside it) — is
/// tested directly rather than only visually inspected. Clamped to
/// zero so a start time before `earliest_hour` (shouldn't normally
/// happen, since `earliest_hour` is derived from the same
/// occurrences, but data can be stale) doesn't push a card above the
/// timeline's top edge.
fn timeline_offset_minutes(start_time: chrono::DateTime<Local>, earliest_hour: u32) -> f32 {
    let hour_frac = start_time.hour() as f32 + start_time.minute() as f32 / 60.0;
    (hour_frac - earliest_hour as f32).max(0.0) * PIXELS_PER_HOUR
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarSubview {
    Day,
    Week,
    Month,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatOption {
    Never,
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndMode {
    Never,
    OnDate,
    AfterCount,
}

/// In-progress fields for the create/edit event modal. `id: None`
/// means "creating"; `Some(id)` means "editing that event". Kept
/// separate from `Event` so a cancelled edit never touches the list.
pub struct EventEditState {
    pub id: Option<i64>,
    pub title: String,
    pub description: String,
    pub subject_id: Option<i64>,
    pub date: String,
    pub start_time: String,
    pub end_time: String,
    pub error: Option<String>,
    /// Sprint 4: quick-add field for attaching a new task directly
    /// from the edit modal. Only usable once the event has an id
    /// (i.e. not while still creating a brand new event).
    pub new_task_input: String,
    /// Sprint 8: quick-add field for attaching a new note directly
    /// from the edit modal, mirroring `new_task_input` above.
    pub new_note_input: String,
    /// Sprint 6: repeat rule fields. `original_recurrence_id` is the
    /// rule the event had *before* this edit started (`None` for a
    /// brand new event, or an existing non-recurring one) — kept so
    /// Save can tell "unchanged", "newly recurring", "now stopped
    /// recurring", and "recurring, rule changed" apart without an
    /// extra database round-trip.
    pub original_recurrence_id: Option<i64>,
    pub repeat_frequency: RepeatOption,
    pub repeat_interval: String,
    pub repeat_weekdays: [bool; 7],
    pub repeat_end_mode: EndMode,
    pub repeat_end_date: String,
    pub repeat_occurrence_limit: String,
    /// Feature request: "Startup Scripts/Commands". Empty string
    /// means "no command" — matching `description`'s convention of
    /// an empty string rather than an `Option` in this in-progress
    /// edit state, with the `Option<String>` translation to/from the
    /// database happening at save time (`try_save_event`) and load
    /// time (`from_event`).
    pub exec_command: String,
    /// Feature request: "Event Logging" — whether this event should
    /// show the Done/Not-Done/Start-Session controls at all. Exposed
    /// as a checkbox right in this same creation/edit form, so it's
    /// declared explicitly rather than assumed, per the request.
    pub track_session: bool,
}

impl EventEditState {
    pub(crate) fn new_for(day: NaiveDate) -> Self {
        Self {
            id: None,
            title: String::new(),
            description: String::new(),
            subject_id: None,
            date: day.format("%Y-%m-%d").to_string(),
            start_time: "09:00".to_owned(),
            end_time: "10:00".to_owned(),
            error: None,
            new_task_input: String::new(),
            new_note_input: String::new(),
            original_recurrence_id: None,
            repeat_frequency: RepeatOption::Never,
            repeat_interval: "1".to_owned(),
            repeat_weekdays: [false; 7],
            repeat_end_mode: EndMode::Never,
            repeat_end_date: String::new(),
            repeat_occurrence_limit: String::new(),
            exec_command: String::new(),
            track_session: true,
        }
    }

    fn from_event(event: &Event) -> Self {
        Self {
            id: Some(event.id),
            title: event.title.clone(),
            description: event.description.clone(),
            subject_id: event.subject_id,
            date: event.start_time.format("%Y-%m-%d").to_string(),
            start_time: event.start_time.format("%H:%M").to_string(),
            end_time: event.end_time.format("%H:%M").to_string(),
            error: None,
            new_task_input: String::new(),
            new_note_input: String::new(),
            original_recurrence_id: event.recurrence_id,
            repeat_frequency: RepeatOption::Never,
            repeat_interval: "1".to_owned(),
            repeat_weekdays: [false; 7],
            repeat_end_mode: EndMode::Never,
            repeat_end_date: String::new(),
            repeat_occurrence_limit: String::new(),
            exec_command: event.exec_command.clone().unwrap_or_default(),
            track_session: event.track_session,
        }
    }

    /// Populates the repeat fields from an existing rule — called
    /// separately from `from_event` since reading the rule needs a
    /// database lookup by `recurrence_id`.
    fn apply_rule(&mut self, rule: &RecurrenceRule) {
        self.repeat_frequency = match rule.frequency {
            Frequency::Daily => RepeatOption::Daily,
            Frequency::Weekly => RepeatOption::Weekly,
            Frequency::Monthly => RepeatOption::Monthly,
        };
        self.repeat_interval = rule.interval.to_string();
        self.repeat_weekdays = [false; 7];
        for day in &rule.weekdays {
            if let Some(index) = WEEKDAY_ORDER.iter().position(|d| d == day) {
                self.repeat_weekdays[index] = true;
            }
        }
        if let Some(end) = rule.end_date {
            self.repeat_end_mode = EndMode::OnDate;
            self.repeat_end_date = end.format("%Y-%m-%d").to_string();
        } else if let Some(limit) = rule.occurrence_limit {
            self.repeat_end_mode = EndMode::AfterCount;
            self.repeat_occurrence_limit = limit.to_string();
        } else {
            self.repeat_end_mode = EndMode::Never;
        }
    }
}

/// UI-only state: subview, which day is selected, and any in-progress
/// edit. Events (and their tasks) are queried fresh every render —
/// see the module doc on `app::AppState` for why.
pub struct CalendarState {
    pub subview: CalendarSubview,
    pub selected_day: NaiveDate,
    pub editing: Option<EventEditState>,
    /// Sprint 9: narrows Day/Week/Month to a single subject's events
    /// when set. `None` means "show everything" — the shared filter
    /// state lives here, at the top of the Calendar view, rather than
    /// per-subview, so switching between Day/Week/Month doesn't reset
    /// or duplicate it.
    pub subject_filter: Option<i64>,
    /// The "log this session's hours" modal (feature request pass) —
    /// in-progress state for logging a custom actual duration on one
    /// occurrence. `None` when closed.
    pub log_editing: Option<LogEditState>,
}

impl Default for CalendarState {
    fn default() -> Self {
        Self {
            subview: CalendarSubview::Day,
            selected_day: Local::now().date_naive(),
            editing: None,
            subject_filter: None,
            log_editing: None,
        }
    }
}

/// In-progress fields for the "log session hours" modal — logging a
/// custom actual duration for one specific occurrence (feature
/// request pass: "account for additional hours which they have done").
pub struct LogEditState {
    pub event_id: i64,
    pub date: NaiveDate,
    pub minutes_input: String,
    pub error: Option<String>,
}

/// One event as it appears on a specific day — either the literal
/// stored row (`is_anchor: true`) or a virtual occurrence synthesized
/// from a recurrence rule, with `start`/`end` shifted to that day but
/// otherwise identical. Nothing about a virtual occurrence is
/// persisted; recomputed fresh every render, per the plan's "don't
/// duplicate the underlying event definition" guidance.
#[derive(Clone)]
struct DayOccurrence {
    event: Event,
    is_anchor: bool,
    rule_label: Option<String>,
}

/// Every occurrence — literal or generated from a recurrence rule —
/// across all `events` that falls within `[range_start, range_end]`
/// (inclusive), keyed by the date it appears on and sorted by start
/// time within each day. The single-day case (`occurrences_on_day`,
/// used by the Day view) and the multi-day case (Week/Month views)
/// share this one implementation rather than the Day view doing its
/// own per-day recurrence expansion and Week/Month doing another.
fn occurrences_in_window(
    db: &Database,
    events: &[Event],
    range_start: NaiveDate,
    range_end: NaiveDate,
) -> HashMap<NaiveDate, Vec<DayOccurrence>> {
    let mut by_day: HashMap<NaiveDate, Vec<DayOccurrence>> = HashMap::new();

    for event in events {
        let anchor_date = event.day();

        match event.recurrence_id {
            None => {
                if anchor_date >= range_start && anchor_date <= range_end {
                    by_day.entry(anchor_date).or_default().push(DayOccurrence {
                        event: event.clone(),
                        is_anchor: true,
                        rule_label: None,
                    });
                }
            }
            Some(rule_id) => {
                let Ok(Some(rule)) = queries::recurrence::get_rule(&db.conn, rule_id) else { continue };
                let dates = occurrence::occurrences_in_range(anchor_date, &rule, range_start, range_end);
                if dates.is_empty() {
                    continue;
                }
                let exceptions = queries::recurrence::list_exceptions(&db.conn, event.id).unwrap_or_default();
                let rule_label = rule.describe();

                for date in dates {
                    if exceptions.contains(&date) {
                        continue;
                    }
                    let occ = if date == anchor_date {
                        DayOccurrence { event: event.clone(), is_anchor: true, rule_label: Some(rule_label.clone()) }
                    } else {
                        let delta = date - anchor_date;
                        let mut shifted = event.clone();
                        shifted.start_time = event.start_time + delta;
                        shifted.end_time = event.end_time + delta;
                        DayOccurrence { event: shifted, is_anchor: false, rule_label: Some(rule_label.clone()) }
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
fn occurrences_on_day(db: &Database, events: &[Event], day: NaiveDate) -> Vec<DayOccurrence> {
    occurrences_in_window(db, events, day, day).remove(&day).unwrap_or_default()
}

/// The Monday..Sunday week containing `day`.
fn week_bounds(day: NaiveDate) -> (NaiveDate, NaiveDate) {
    let start = day - ChronoDuration::days(day.weekday().num_days_from_monday() as i64);
    (start, start + ChronoDuration::days(6))
}

/// Shifts `day` by whole months, landing on the 1st of the target
/// month — used for the Month view's ◀ ▶ navigation, where "which
/// day of the month" doesn't carry meaning across a month jump the
/// way it does for day/week navigation.
fn shift_month(day: NaiveDate, delta: i32) -> NaiveDate {
    let total = day.year() * 12 + (day.month() as i32 - 1) + delta;
    let year = total.div_euclid(12);
    let month = (total.rem_euclid(12) + 1) as u32;
    NaiveDate::from_ymd_opt(year, month, 1).expect("valid year/month/1 date")
}

/// A full calendar month as whole Monday..Sunday weeks, padded with
/// the tail of the previous month and the head of the next so every
/// row has exactly 7 days — the standard month-grid layout. Returns
/// 4-6 weeks depending on how the month falls against week
/// boundaries, never a fixed 6 (which would sometimes render a
/// trailing row of nothing but padding).
fn month_grid_weeks(year: i32, month: u32) -> Vec<Vec<NaiveDate>> {
    let first_of_month = NaiveDate::from_ymd_opt(year, month, 1).expect("valid year/month/1 date");
    let last_of_month = shift_month(first_of_month, 1) - ChronoDuration::days(1);

    let (grid_start, _) = week_bounds(first_of_month);
    let (_, grid_end) = week_bounds(last_of_month);

    let total_days = (grid_end - grid_start).num_days() + 1;
    let mut weeks = Vec::with_capacity((total_days / 7) as usize);
    let mut week_start = grid_start;
    while week_start <= grid_end {
        weeks.push((0..7).map(|i| week_start + ChronoDuration::days(i)).collect());
        week_start += ChronoDuration::weeks(1);
    }
    weeks
}

fn format_duration(duration: ChronoDuration) -> String {
    let total_minutes = duration.num_minutes().max(0);
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    match (hours, minutes) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// Calendar shell. Sprint 1 built the Day/Week/Month switcher; Sprint 3
/// wired the Day view to real, persisted events; Sprint 4 attached
/// tasks to events; Sprint 6 adds recurrence — an event can repeat
/// daily, weekly (optionally on selected weekdays), or monthly, with
/// an optional end date or occurrence count, and a single occurrence
/// can be skipped without touching the rest of the series.
pub fn show(
    ui: &mut egui::Ui,
    theme: &Theme,
    db: Option<&Database>,
    state: &mut CalendarState,
) -> Option<(i64, NaiveDate)> {
    let p = &theme.palette;

    ui.add_space(Spacing::MD);
    ui.horizontal(|ui| {
        for (label, sv) in [
            ("Day", CalendarSubview::Day),
            ("Week", CalendarSubview::Week),
            ("Month", CalendarSubview::Month),
        ] {
            let active = state.subview == sv;
            let text = RichText::new(label)
                .font(theme.label_font())
                .color(if active { p.text } else { p.text_muted });
            if ui.selectable_label(active, text).clicked() {
                state.subview = sv;
            }
            ui.add_space(Spacing::SM);
        }
    });
    ui.add_space(Spacing::SM);

    // Left/Right step through days, weeks, or months depending on
    // the active subview — the keyboard equivalent of the ◀ ▶
    // buttons each subview already has. Gated on nothing being
    // focused so it doesn't fire while typing in a modal field.
    if ui.ctx().memory(|m| m.focused().is_none()) {
        ui.ctx().input(|input| {
            if input.key_pressed(egui::Key::ArrowLeft) {
                match state.subview {
                    CalendarSubview::Day => state.selected_day -= ChronoDuration::days(1),
                    CalendarSubview::Week => state.selected_day -= ChronoDuration::days(7),
                    CalendarSubview::Month => state.selected_day = shift_month(state.selected_day, -1),
                }
            }
            if input.key_pressed(egui::Key::ArrowRight) {
                match state.subview {
                    CalendarSubview::Day => state.selected_day += ChronoDuration::days(1),
                    CalendarSubview::Week => state.selected_day += ChronoDuration::days(7),
                    CalendarSubview::Month => state.selected_day = shift_month(state.selected_day, 1),
                }
            }
        });
    }

    let Some(db) = db else {
        return None;
    };

    let subjects = queries::subjects::list_all_subjects(&db.conn).unwrap_or_default();
    subject_filter_row(ui, theme, &subjects, &mut state.subject_filter);
    ui.add_space(Spacing::MD);

    match state.subview {
        CalendarSubview::Day => day_view(ui, theme, db, state, &subjects),
        CalendarSubview::Week => {
            week_view(ui, theme, db, state, &subjects);
            None
        }
        CalendarSubview::Month => {
            month_view(ui, theme, db, state, &subjects);
            None
        }
    }
}

/// A one-line "Subject: <picker>" row shared by Day/Week/Month —
/// Sprint 9's "filter by subject", applying consistently regardless
/// of which calendar view is active (its acceptance criterion).
fn subject_filter_row(ui: &mut egui::Ui, theme: &Theme, subjects: &[Subject], filter: &mut Option<i64>) {
    let p = &theme.palette;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Subject").font(theme.small_font()).color(p.text_faint));
        let current_label = filter
            .and_then(|id| subjects.iter().find(|s| s.id == id))
            .map(|s| s.name.as_str())
            .unwrap_or("All subjects");
        egui::ComboBox::from_id_salt("calendar_subject_filter").selected_text(current_label).show_ui(ui, |ui| {
            ui.selectable_value(filter, None, "All subjects");
            for subject in subjects {
                let label =
                    if subject.archived { format!("{} (archived)", subject.name) } else { subject.name.clone() };
                ui.selectable_value(filter, Some(subject.id), label);
            }
        });
    });
}

/// Events matching the active subject filter, if any.
fn apply_subject_filter(events: Vec<Event>, filter: Option<i64>) -> Vec<Event> {
    match filter {
        None => events,
        Some(id) => events.into_iter().filter(|e| e.subject_id == Some(id)).collect(),
    }
}

fn subject_name(subjects: &[Subject], id: Option<i64>) -> Option<&str> {
    id.and_then(|id| subjects.iter().find(|s| s.id == id)).map(|s| s.name.as_str())
}

/// "Tasks for this day" — the Calendar half of the feature request
/// "Task Assignment": tasks whose `assigned_date` is this specific
/// day, shown in their own section at the top of the Day view, plus
/// a way to assign an existing task to this day directly from here
/// (the request's own "This would also be possible through the
/// calendar"). Deliberately keyed on `assigned_date` only, not
/// `due_date` — the Day view's timeline below already shows the
/// day's *scheduled* Calendar events; this section is specifically
/// about the newer, independent "planned to work on this today"
/// concept, not a second rendering of due-today tasks.
fn tasks_for_day_section(ui: &mut egui::Ui, theme: &Theme, db: &Database, day: NaiveDate) {
    let p = &theme.palette;
    let day_str = day.format("%Y-%m-%d").to_string();
    let all_tasks = queries::tasks::list_tasks(&db.conn).unwrap_or_default();
    let assigned: Vec<&Task> = all_tasks.iter().filter(|t| t.assigned_date.as_deref() == Some(day_str.as_str())).collect();

    let mut toggle: Option<(i64, bool)> = None;
    let mut unassign: Option<i64> = None;
    let mut assign: Option<i64> = None;

    Panel::new()
        .title(&format!(
            "Tasks for this day — {}/{}",
            assigned.iter().filter(|t| t.completed).count(),
            assigned.len()
        ))
        .show(ui, theme, |ui| {
            if assigned.is_empty() {
                ui.label(
                    RichText::new("No tasks assigned to this day yet.").font(theme.small_font()).color(p.text_faint),
                );
            }
            for task in &assigned {
                ui.horizontal(|ui| {
                    let mut completed = task.completed;
                    if crate::ui::components::checkbox(ui, theme, &mut completed, &task.title).clicked() {
                        toggle = Some((task.id, completed));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Unassign").clicked() {
                            unassign = Some(task.id);
                        }
                    });
                });
            }

            let assignable: Vec<&Task> =
                all_tasks.iter().filter(|t| !t.completed && t.assigned_date.as_deref() != Some(day_str.as_str())).collect();
            if !assignable.is_empty() {
                ui.add_space(Spacing::XS);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Assign a task to this day").font(theme.small_font()).color(p.text_faint));
                    egui::ComboBox::from_id_salt("day_assign_task_picker").selected_text("Choose a task…").show_ui(
                        ui,
                        |ui| {
                            for task in &assignable {
                                if ui.selectable_label(false, &task.title).clicked() {
                                    assign = Some(task.id);
                                }
                            }
                        },
                    );
                });
            }
        });

    if let Some((id, completed)) = toggle {
        let _ = queries::tasks::set_completed(&db.conn, id, completed);
    }
    if let Some(id) = unassign {
        let _ = queries::tasks::set_assigned_date(&db.conn, id, None);
    }
    if let Some(id) = assign {
        let _ = queries::tasks::set_assigned_date(&db.conn, id, Some(&day_str));
    }
}

fn day_view(
    ui: &mut egui::Ui,
    theme: &Theme,
    db: &Database,
    state: &mut CalendarState,
    subjects: &[Subject],
) -> Option<(i64, NaiveDate)> {
    let p = &theme.palette;
    let today = Local::now().date_naive();
    let events = apply_subject_filter(queries::events::list_all_events(&db.conn).unwrap_or_default(), state.subject_filter);

    ui.horizontal(|ui| {
        if ui.small_button("◀").clicked() {
            state.selected_day -= ChronoDuration::days(1);
        }
        ui.add_space(Spacing::SM);
        let heading = if state.selected_day == today {
            format!("Today — {}", state.selected_day.format("%A, %d %B"))
        } else {
            state.selected_day.format("%A, %d %B").to_string()
        };
        ui.label(RichText::new(heading).font(theme.label_font()).color(p.text));
        ui.add_space(Spacing::SM);
        if ui.small_button("▶").clicked() {
            state.selected_day += ChronoDuration::days(1);
        }
        if state.selected_day != today {
            ui.add_space(Spacing::SM);
            if ui.small_button("Today").clicked() {
                state.selected_day = today;
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if RButton::new("New Event").primary().show(ui, theme).clicked() {
                state.editing = Some(EventEditState::new_for(state.selected_day));
                ui.ctx().memory_mut(|m| m.request_focus(egui::Id::new("event_title")));
            }
        });
    });

    ui.add_space(Spacing::MD);

    tasks_for_day_section(ui, theme, db, state.selected_day);
    ui.add_space(Spacing::MD);

    let day = state.selected_day;
    let day_occurrences = occurrences_on_day(db, &events, day);

    let mut delete_id: Option<i64> = None;
    let mut edit_target: Option<i64> = None;
    let mut toggle_task: Option<(i64, bool)> = None;
    let mut skip_occurrence: Option<(i64, NaiveDate)> = None;
    let mut log_action: Option<(i64, NaiveDate, crate::events::SessionStatus)> = None;
    let mut log_clear: Option<(i64, NaiveDate)> = None;
    let mut open_log_editor: Option<(i64, NaiveDate)> = None;
    let mut start_session: Option<(i64, NaiveDate)> = None;

    Panel::new().show(ui, theme, |ui| {
        if day_occurrences.is_empty() {
            ui.label(RichText::new("Nothing scheduled.").font(theme.small_font()).color(p.text_faint));
            return;
        }

        let earliest_hour = day_occurrences
            .iter()
            .map(|occ| occ.event.start_time.format("%H").to_string().parse::<u32>().unwrap_or(9))
            .min()
            .unwrap_or(9)
            .saturating_sub(1);
        let latest_hour = day_occurrences
            .iter()
            .map(|occ| occ.event.end_time.format("%H").to_string().parse::<u32>().unwrap_or(18))
            .max()
            .unwrap_or(18)
            .min(23)
            + 1;
        let latest_hour = latest_hour.min(23);

        // A true proportional timeline: hour gridlines and event
        // cards are positioned on the *same* `PIXELS_PER_HOUR`
        // vertical scale, computed from absolute clock time — not,
        // as before, a fixed-height hour label followed by a
        // separately-scaled accent bar with manual padding to
        // approximate alignment. That earlier approach meant a
        // 2-hour event's bar was twice as tall as a 1-hour one's, but
        // neither was actually tied to the hour ruler beside it, so
        // an event effectively "started and finished" at its own
        // label regardless of how long it ran. Here, a 2-hour event's
        // card visibly spans exactly two hour gridlines.
        let hour_span = (latest_hour - earliest_hour + 1).max(1);
        let timeline_height = hour_span as f32 * PIXELS_PER_HOUR;
        const TIME_LABEL_WIDTH: f32 = 48.0;

        let (timeline_rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), timeline_height), egui::Sense::hover());

        let ruler_x = timeline_rect.left() + TIME_LABEL_WIDTH;
        for hour in earliest_hour..=latest_hour {
            let y = timeline_rect.top() + (hour - earliest_hour) as f32 * PIXELS_PER_HOUR;
            ui.painter().text(
                egui::pos2(timeline_rect.left(), y),
                egui::Align2::LEFT_TOP,
                format!("{hour:02}:00"),
                theme.small_font(),
                p.text_faint,
            );
            ui.painter().hline(ruler_x..=timeline_rect.right(), y, Stroke::new(Border::HAIRLINE, p.border));
        }
        // The closing line at the bottom of the last hour, so that
        // hour's row reads as bounded too, not just its top edge.
        ui.painter().hline(
            ruler_x..=timeline_rect.right(),
            timeline_rect.top() + timeline_height,
            Stroke::new(Border::HAIRLINE, p.border),
        );

        for occ in &day_occurrences {
            // Tasks are attached to the parent event row, not to
            // individual occurrences — showing them only on the
            // anchor keeps "completing a task" from looking like
            // it silently applies to every future occurrence too.
            let event_tasks = if occ.is_anchor {
                queries::tasks::list_tasks_for_event(&db.conn, occ.event.id).unwrap_or_default()
            } else {
                Vec::new()
            };
            let current_log = queries::session_log::get_log(&db.conn, occ.event.id, occ.event.day()).unwrap_or(None);

            let top = timeline_rect.top() + timeline_offset_minutes(occ.event.start_time, earliest_hour);
            let duration_minutes = occ.event.duration().num_minutes().max(0) as f32;
            let bar_height = bar_height_for_duration(duration_minutes);

            let card_rect = egui::Rect::from_min_size(
                egui::pos2(ruler_x + Spacing::SM, top),
                egui::vec2((timeline_rect.right() - ruler_x - Spacing::SM).max(200.0), bar_height),
            );

            ui.scope_builder(egui::UiBuilder::new().max_rect(card_rect), |ui| {
                ui.horizontal(|ui| {
                    let (bar_rect, _) = ui.allocate_exact_size(egui::vec2(3.0, bar_height), egui::Sense::hover());
                    ui.painter().rect_filled(
                        bar_rect,
                        CornerRadius::same(1),
                        if occ.rule_label.is_some() { p.text_muted } else { p.focus },
                    );
                    ui.add_space(Spacing::SM);
                    ui.vertical(|ui| {
                        event_block(
                            ui,
                            theme,
                            occ,
                            subjects,
                            &event_tasks,
                            current_log.as_ref(),
                            &mut delete_id,
                            &mut edit_target,
                            &mut skip_occurrence,
                            &mut toggle_task,
                            &mut log_action,
                            &mut log_clear,
                            &mut open_log_editor,
                            &mut start_session,
                        );
                    });
                });
            });
        }
    });

    if let Some((id, completed)) = toggle_task {
        let _ = queries::tasks::set_completed(&db.conn, id, completed);
    }

    if let Some((event_id, date)) = skip_occurrence {
        let _ = queries::recurrence::add_exception(&db.conn, event_id, date);
    }

    if let Some((event_id, date, status)) = log_action {
        let _ = queries::session_log::log_session(&db.conn, event_id, date, status, None);
    }
    if let Some((event_id, date)) = log_clear {
        let _ = queries::session_log::clear_log(&db.conn, event_id, date);
    }
    if let Some((event_id, date)) = open_log_editor {
        let existing = queries::session_log::get_log(&db.conn, event_id, date).unwrap_or(None);
        let default_minutes = events
            .iter()
            .find(|e| e.id == event_id)
            .map(|e| e.duration().num_minutes().max(0))
            .unwrap_or(0);
        let minutes_input = existing
            .as_ref()
            .and_then(|l| l.actual_minutes)
            .unwrap_or(default_minutes)
            .to_string();
        state.log_editing = Some(LogEditState { event_id, date, minutes_input, error: None });
        ui.ctx().memory_mut(|m| m.request_focus(egui::Id::new("log_edit_minutes")));
    }

    if let Some(id) = delete_id {
        let _ = queries::events::delete_event(&db.conn, id);
    }

    if let Some(id) = edit_target {
        if let Some(event) = events.iter().find(|e| e.id == id) {
            let mut edit = EventEditState::from_event(event);
            if let Some(rule_id) = event.recurrence_id {
                if let Ok(Some(rule)) = queries::recurrence::get_rule(&db.conn, rule_id) {
                    edit.apply_rule(&rule);
                }
            }
            state.editing = Some(edit);
            ui.ctx().memory_mut(|m| m.request_focus(egui::Id::new("event_title")));
        }
    }

    edit_modal(ui, theme, db, state, subjects);
    log_edit_modal(ui, theme, db, state);

    start_session
}

/// Opens the New Event modal defaulting to `day`, focusing its title
/// field — the same "create an event" entry point Day view exposes,
/// reused by Week and Month so creation works consistently everywhere
/// (Sprint 7's "creating an event from the calendar works").
fn open_new_event(ui: &egui::Ui, state: &mut CalendarState, day: NaiveDate) {
    state.editing = Some(EventEditState::new_for(day));
    ui.ctx().memory_mut(|m| m.request_focus(egui::Id::new("event_title")));
}

fn week_view(ui: &mut egui::Ui, theme: &Theme, db: &Database, state: &mut CalendarState, _subjects: &[Subject]) {
    let p = &theme.palette;
    let today = Local::now().date_naive();
    let events = apply_subject_filter(queries::events::list_all_events(&db.conn).unwrap_or_default(), state.subject_filter);

    let (week_start, week_end) = week_bounds(state.selected_day);

    ui.horizontal(|ui| {
        if ui.small_button("◀").clicked() {
            state.selected_day -= ChronoDuration::days(7);
        }
        ui.add_space(Spacing::SM);
        let in_current_week = today >= week_start && today <= week_end;
        let heading = if in_current_week {
            format!("This week — {} – {}", week_start.format("%d %b"), week_end.format("%d %b"))
        } else {
            format!("{} – {}", week_start.format("%d %b"), week_end.format("%d %b"))
        };
        ui.label(RichText::new(heading).font(theme.label_font()).color(p.text));
        ui.add_space(Spacing::SM);
        if ui.small_button("▶").clicked() {
            state.selected_day += ChronoDuration::days(7);
        }
        if !in_current_week {
            ui.add_space(Spacing::SM);
            if ui.small_button("Today").clicked() {
                state.selected_day = today;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if RButton::new("New Event").primary().show(ui, theme).clicked() {
                open_new_event(ui, state, state.selected_day);
            }
        });
    });

    ui.add_space(Spacing::MD);

    let by_day = occurrences_in_window(db, &events, week_start, week_end);
    let mut go_to_day: Option<NaiveDate> = None;

    ui.columns(7, |cols| {
        for (index, col) in cols.iter_mut().enumerate() {
            let day = week_start + ChronoDuration::days(index as i64);
            let is_today = day == today;
            let occurrences = by_day.get(&day).cloned().unwrap_or_default();

            Frame::NONE
                .fill(p.surface)
                .stroke(Stroke::new(
                    if is_today { Border::THICK } else { Border::HAIRLINE },
                    if is_today { p.focus } else { p.border },
                ))
                .corner_radius(CornerRadius::same(Border::RADIUS))
                .inner_margin(egui::Margin::same(Spacing::XS as i8))
                .show(col, |ui| {
                    ui.set_min_height(140.0);
                    let header = RichText::new(day.format("%a %d").to_string())
                        .font(theme.small_font())
                        .color(if is_today { p.text } else { p.text_muted });
                    if ui.selectable_label(false, header).clicked() {
                        go_to_day = Some(day);
                    }
                    ui.add_space(Spacing::XS);

                    if occurrences.is_empty() {
                        ui.label(RichText::new("—").font(theme.small_font()).color(p.text_faint));
                    }
                    for occ in &occurrences {
                        let recurring_mark = if occ.rule_label.is_some() { "↻ " } else { "" };
                        let line = format!("{}{}{}", recurring_mark, occ.event.start_time.format("%H:%M "), occ.event.title);
                        if ui
                            .selectable_label(
                                false,
                                RichText::new(line).font(theme.small_font()).color(p.text),
                            )
                            .clicked()
                        {
                            go_to_day = Some(day);
                        }
                    }
                });
        }
    });

    if let Some(day) = go_to_day {
        state.selected_day = day;
        state.subview = CalendarSubview::Day;
    }
}

fn month_view(ui: &mut egui::Ui, theme: &Theme, db: &Database, state: &mut CalendarState, _subjects: &[Subject]) {
    let p = &theme.palette;
    let today = Local::now().date_naive();
    let events = apply_subject_filter(queries::events::list_all_events(&db.conn).unwrap_or_default(), state.subject_filter);

    let year = state.selected_day.year();
    let month = state.selected_day.month();
    let weeks = month_grid_weeks(year, month);
    let grid_start = weeks[0][0];
    let grid_end = *weeks.last().unwrap().last().unwrap();

    ui.horizontal(|ui| {
        if ui.small_button("◀").clicked() {
            state.selected_day = shift_month(state.selected_day, -1);
        }
        ui.add_space(Spacing::SM);
        ui.label(
            RichText::new(state.selected_day.format("%B %Y").to_string()).font(theme.label_font()).color(p.text),
        );
        ui.add_space(Spacing::SM);
        if ui.small_button("▶").clicked() {
            state.selected_day = shift_month(state.selected_day, 1);
        }
        let in_current_month = today.year() == year && today.month() == month;
        if !in_current_month {
            ui.add_space(Spacing::SM);
            if ui.small_button("Today").clicked() {
                state.selected_day = today;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if RButton::new("New Event").primary().show(ui, theme).clicked() {
                open_new_event(ui, state, state.selected_day);
            }
        });
    });

    ui.add_space(Spacing::MD);

    let by_day = occurrences_in_window(db, &events, grid_start, grid_end);
    let mut go_to_day: Option<NaiveDate> = None;

    // Uses the same `ui.columns(7, ...)` mechanism as the day grid
    // below, rather than hand-dividing `ui.available_width() / 7.0`
    // per label — that recomputed the divisor *after* each label had
    // already consumed space, so the denominator shrank a little more
    // each iteration and the labels drifted out of alignment with the
    // grid by the fourth or fifth column.
    ui.columns(7, |cols| {
        for (col, label) in cols.iter_mut().zip(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]) {
            col.label(RichText::new(label).font(theme.small_font()).color(p.text_faint));
        }
    });

    for week in &weeks {
        ui.columns(7, |cols| {
            for (index, col) in cols.iter_mut().enumerate() {
                let day = week[index];
                let is_today = day == today;
                let in_month = day.month() == month;
                let count = by_day.get(&day).map(Vec::len).unwrap_or(0);

                Frame::NONE
                    .fill(if in_month { p.surface } else { p.background })
                    .stroke(Stroke::new(
                        if is_today { Border::THICK } else { Border::HAIRLINE },
                        if is_today { p.focus } else { p.border },
                    ))
                    .corner_radius(CornerRadius::same(Border::RADIUS))
                    .inner_margin(egui::Margin::same(Spacing::XS as i8))
                    .show(col, |ui| {
                        ui.set_min_height(52.0);
                        let day_number = RichText::new(day.day().to_string())
                            .font(theme.small_font())
                            .color(if in_month { p.text } else { p.text_faint });
                        if ui.selectable_label(false, day_number).clicked() {
                            go_to_day = Some(day);
                        }
                        if count > 0 {
                            let label = if count == 1 { "1 session".to_owned() } else { format!("{count} sessions") };
                            ui.label(RichText::new(label).font(theme.small_font()).color(p.text_muted));
                        }
                    });
            }
        });
    }

    if let Some(day) = go_to_day {
        state.selected_day = day;
        state.subview = CalendarSubview::Day;
    }
}

#[allow(clippy::too_many_arguments)]
fn event_block(
    ui: &mut egui::Ui,
    theme: &Theme,
    occ: &DayOccurrence,
    subjects: &[Subject],
    event_tasks: &[Task],
    current_log: Option<&crate::events::SessionLogEntry>,
    delete_id: &mut Option<i64>,
    edit_target: &mut Option<i64>,
    skip_occurrence: &mut Option<(i64, NaiveDate)>,
    toggle_task: &mut Option<(i64, bool)>,
    log_action: &mut Option<(i64, NaiveDate, crate::events::SessionStatus)>,
    log_clear: &mut Option<(i64, NaiveDate)>,
    open_log_editor: &mut Option<(i64, NaiveDate)>,
    start_session: &mut Option<(i64, NaiveDate)>,
) {
    let p = &theme.palette;
    let event = &occ.event;
    Frame::NONE
        .fill(p.surface)
        .stroke(Stroke::new(Border::HAIRLINE, p.border))
        .corner_radius(CornerRadius::same(Border::RADIUS))
        .inner_margin(egui::Margin::same(Spacing::SM as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width().min(480.0));
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&event.title).font(theme.label_font()).color(p.text));
                        if let Some(subject) = subject_name(subjects, event.subject_id) {
                            ui.add_space(Spacing::SM);
                            ui.label(
                                RichText::new(subject.to_uppercase()).font(theme.small_font()).color(p.text_faint),
                            );
                        }
                        if let Some(label) = &occ.rule_label {
                            ui.add_space(Spacing::SM);
                            ui.label(RichText::new(format!("↻ {label}")).font(theme.small_font()).color(p.text_muted));
                        }
                    });
                    let mut time_range = format!(
                        "{}–{} ({})",
                        event.start_time.format("%H:%M"),
                        event.end_time.format("%H:%M"),
                        format_duration(event.duration())
                    );
                    if event.crosses_midnight() {
                        time_range.push_str(" (+1 day)");
                    }
                    ui.label(RichText::new(time_range).font(theme.small_font()).color(p.text_muted));
                    if !event.description.is_empty() {
                        ui.label(
                            RichText::new(&event.description).font(theme.small_font()).color(p.text_faint),
                        );
                    }

                    if !event_tasks.is_empty() {
                        ui.add_space(Spacing::XS);
                        let completed = event_tasks.iter().filter(|t| t.completed).count();
                        for task in event_tasks {
                            let mut is_completed = task.completed;
                            if checkbox(ui, theme, &mut is_completed, &task.title).clicked() {
                                *toggle_task = Some((task.id, is_completed));
                            }
                        }
                        ui.label(
                            RichText::new(format!("{completed} / {} complete", event_tasks.len()))
                                .font(theme.small_font())
                                .color(p.text_faint),
                        );
                    }

                    // Session logging (feature request pass): report
                    // whether this specific occurrence actually
                    // happened as scheduled, didn't happen, or ran for
                    // a different duration — without touching the
                    // event/rule itself. Only offered once the
                    // occurrence has started; logging something that
                    // hasn't happened yet doesn't make sense. Skipped
                    // entirely for events that opted out of session
                    // tracking at creation (feature request: "Event
                    // Logging" — meetings and the like).
                    if event.track_session && event.start_time <= Local::now() {
                        ui.add_space(Spacing::XS);
                        ui.horizontal(|ui| {
                            use crate::events::SessionStatus;
                            let status_label = match current_log.map(|l| l.status) {
                                Some(SessionStatus::Completed) => Some("Logged: completed".to_owned()),
                                Some(SessionStatus::Denied) => Some("Logged: not done".to_owned()),
                                Some(SessionStatus::Adjusted) => {
                                    let minutes = current_log.and_then(|l| l.actual_minutes).unwrap_or(0);
                                    Some(format!("Logged: {minutes}m"))
                                }
                                None => None,
                            };
                            if let Some(label) = status_label {
                                ui.label(RichText::new(label).font(theme.small_font()).color(p.text_faint));
                                ui.add_space(Spacing::SM);
                            }
                            // Feature request: "Event Logging" — once
                            // an occurrence has been marked one way or
                            // the other, the Done/Not-Done buttons
                            // themselves disappear rather than sitting
                            // there indefinitely; the logged outcome
                            // is still changeable, just via "Edit
                            // hours" (which now doubles as the general
                            // "change what I logged" action) instead
                            // of a full row of buttons.
                            if current_log.is_none() {
                                if ui.small_button("Done").clicked() {
                                    *log_action = Some((event.id, event.day(), SessionStatus::Completed));
                                }
                                if ui.small_button("Not done").clicked() {
                                    *log_action = Some((event.id, event.day(), SessionStatus::Denied));
                                }
                            } else {
                                if ui.small_button("Edit hours").clicked() {
                                    *open_log_editor = Some((event.id, event.day()));
                                }
                                if ui.small_button("Clear").clicked() {
                                    *log_clear = Some((event.id, event.day()));
                                }
                            }
                        });
                    }
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    if event.recurrence_id.is_some() {
                        if ui.small_button("Delete series").clicked() {
                            *delete_id = Some(event.id);
                        }
                        if ui.small_button("Skip").clicked() {
                            *skip_occurrence = Some((event.id, event.day()));
                        }
                    } else if ui.small_button("Delete").clicked() {
                        *delete_id = Some(event.id);
                    }
                    if ui.small_button("Edit").clicked() {
                        *edit_target = Some(event.id);
                    }
                    // Sprint 12: enters focus mode for this occurrence.
                    // Allowed from any occurrence — past, today, or
                    // future — rather than restricted by time; the
                    // plan doesn't call for restricting this, and
                    // over-restricting adds complexity for no clear
                    // benefit. Feature request: "Event Logging" — like
                    // the Done/Not-Done row above, hidden entirely for
                    // events that opted out of session tracking, and
                    // hidden once the occurrence has already been
                    // logged one way or the other (starting a session
                    // for something already marked Done/Not Done
                    // doesn't make sense, and this saves the same kind
                    // of space the request is asking for).
                    if event.track_session && current_log.is_none() {
                        if RButton::new("Start Session").primary().show(ui, theme).clicked() {
                            *start_session = Some((event.id, event.day()));
                        }
                    }
                });
            });
        });
}

fn edit_modal(ui: &mut egui::Ui, theme: &Theme, db: &Database, state: &mut CalendarState, subjects: &[Subject]) {
    let p = &theme.palette;
    let mut modal_open = state.editing.is_some();
    let mut save_requested = false;
    let mut cancel_requested = false;
    let mut delete_requested = false;
    let mut add_task_requested = false;
    let mut add_note_requested = false;
    let mut task_toggle: Option<(i64, bool)> = None;
    let mut task_detach: Option<i64> = None;

    let title = if matches!(&state.editing, Some(e) if e.id.is_some()) {
        "Edit Event"
    } else {
        "New Event"
    };

    let editing_id = state.editing.as_ref().and_then(|e| e.id);
    let event_tasks = editing_id
        .map(|id| queries::tasks::list_tasks_for_event(&db.conn, id).unwrap_or_default())
        .unwrap_or_default();
    let event_notes = editing_id
        .map(|id| queries::notes::list_notes_for_event(&db.conn, id).unwrap_or_default())
        .unwrap_or_default();

    Modal::new("event_modal", title).show(ui.ctx(), theme, &mut modal_open, |ui| {
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let mut add_task_via_enter = false;
        let mut add_note_via_enter = false;
        if let Some(edit) = &mut state.editing {
            ui.label(RichText::new("Title").font(theme.small_font()).color(p.text_faint));
            Input::new(&mut edit.title, "Title").id("event_title").show(ui, theme);
            ui.add_space(Spacing::SM);

            ui.label(RichText::new("Subject").font(theme.small_font()).color(p.text_faint));
            let current_label = subject_name(subjects, edit.subject_id).unwrap_or("None");
            egui::ComboBox::from_id_salt("event_subject").selected_text(current_label).show_ui(ui, |ui| {
                ui.selectable_value(&mut edit.subject_id, None, "None");
                for subject in subjects {
                    let label =
                        if subject.archived { format!("{} (archived)", subject.name) } else { subject.name.clone() };
                    ui.selectable_value(&mut edit.subject_id, Some(subject.id), label);
                }
            });
            ui.add_space(Spacing::SM);

            ui.label(RichText::new("Description").font(theme.small_font()).color(p.text_faint));
            Input::new(&mut edit.description, "Optional").show(ui, theme);
            ui.add_space(Spacing::SM);

            // Feature request: "Startup Scripts/Commands". Only
            // actually runs if enabled in Settings (off by default) —
            // the field itself is always available to fill in, since
            // there's no harm in it sitting unused if the setting's
            // off, only in it running unattended without the person
            // having opted in.
            ui.label(RichText::new("Startup command (advanced)").font(theme.small_font()).color(p.text_faint));
            Input::new(&mut edit.exec_command, "e.g. firefox https://example.com").show(ui, theme);
            ui.label(
                RichText::new(
                    "Runs once, at this event's start time, if enabled under Settings → Startup Commands.",
                )
                .font(theme.small_font())
                .color(p.text_faint),
            );
            ui.add_space(Spacing::SM);

            // Feature request: "Event Logging". Unchecking this is
            // meant for calendar blocks that were never a revision
            // session to begin with (a meeting, say) — so their
            // occurrences don't carry Done/Not-Done/Start-Session
            // controls that don't apply to them.
            ui.checkbox(&mut edit.track_session, "Track as a revision session (Done/Not Done, Start Session)");
            ui.add_space(Spacing::SM);

            ui.label(RichText::new("Date").font(theme.small_font()).color(p.text_faint));
            Input::new(&mut edit.date, "YYYY-MM-DD, 'today', 'tomorrow', 'next tuesday'…").show(ui, theme);
            ui.add_space(Spacing::SM);

            ui.columns(2, |cols| {
                cols[0].label(RichText::new("Start").font(theme.small_font()).color(p.text_faint));
                Input::new(&mut edit.start_time, "HH:MM").show(&mut cols[0], theme);
                cols[1].label(RichText::new("End").font(theme.small_font()).color(p.text_faint));
                Input::new(&mut edit.end_time, "HH:MM").show(&mut cols[1], theme);
            });
            ui.label(
                RichText::new("If end is earlier than start, it's treated as ending the next day.")
                    .font(theme.small_font())
                    .color(p.text_faint),
            );

            // --- Repeat (Sprint 6) -------------------------------------
            ui.add_space(Spacing::MD);
            ui.separator();
            ui.add_space(Spacing::SM);
            ui.label(RichText::new("Repeat").font(theme.small_font()).color(p.text_faint));
            ui.add_space(Spacing::XS);

            egui::ComboBox::from_id_salt("repeat_frequency")
                .selected_text(match edit.repeat_frequency {
                    RepeatOption::Never => "Never",
                    RepeatOption::Daily => "Daily",
                    RepeatOption::Weekly => "Weekly",
                    RepeatOption::Monthly => "Monthly",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut edit.repeat_frequency, RepeatOption::Never, "Never");
                    ui.selectable_value(&mut edit.repeat_frequency, RepeatOption::Daily, "Daily");
                    ui.selectable_value(&mut edit.repeat_frequency, RepeatOption::Weekly, "Weekly");
                    ui.selectable_value(&mut edit.repeat_frequency, RepeatOption::Monthly, "Monthly");
                });

            if edit.repeat_frequency != RepeatOption::Never {
                ui.add_space(Spacing::XS);
                let unit = match edit.repeat_frequency {
                    RepeatOption::Daily => "day(s)",
                    RepeatOption::Weekly => "week(s)",
                    RepeatOption::Monthly => "month(s)",
                    RepeatOption::Never => unreachable!(),
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Every").font(theme.small_font()).color(p.text_faint));
                    Input::new(&mut edit.repeat_interval, "1").width(48.0).show(ui, theme);
                    ui.label(RichText::new(unit).font(theme.small_font()).color(p.text_faint));
                });

                if edit.repeat_frequency == RepeatOption::Weekly {
                    ui.add_space(Spacing::XS);
                    ui.label(
                        RichText::new("On (leave all off to repeat the same weekday as the start date)")
                            .font(theme.small_font())
                            .color(p.text_faint),
                    );
                    ui.horizontal(|ui| {
                        for (index, label) in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].iter().enumerate() {
                            let selected = edit.repeat_weekdays[index];
                            if ui.selectable_label(selected, *label).clicked() {
                                edit.repeat_weekdays[index] = !selected;
                            }
                        }
                    });
                }

                ui.add_space(Spacing::XS);
                ui.label(RichText::new("Ends").font(theme.small_font()).color(p.text_faint));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut edit.repeat_end_mode, EndMode::Never, "Never");
                    ui.selectable_value(&mut edit.repeat_end_mode, EndMode::OnDate, "On date");
                    ui.selectable_value(&mut edit.repeat_end_mode, EndMode::AfterCount, "After N times");
                });
                match edit.repeat_end_mode {
                    EndMode::OnDate => {
                        Input::new(&mut edit.repeat_end_date, "YYYY-MM-DD").show(ui, theme);
                    }
                    EndMode::AfterCount => {
                        Input::new(&mut edit.repeat_occurrence_limit, "e.g. 10").width(64.0).show(ui, theme);
                    }
                    EndMode::Never => {}
                }
            }

            if let Some(error) = &edit.error {
                ui.add_space(Spacing::SM);
                ui.label(RichText::new(error).font(theme.small_font()).color(p.text));
            }

            // --- Attached tasks (Sprint 4) — only once the event
            // actually exists, since a task needs a real event id.
            // Tasks are shared by the whole series (Sprint 6 doesn't
            // track completion per-occurrence), so this is managed
            // here at the series level regardless of which occurrence
            // you clicked Edit from.
            if edit.id.is_some() {
                ui.add_space(Spacing::MD);
                ui.separator();
                ui.add_space(Spacing::SM);
                ui.label(RichText::new("Tasks").font(theme.small_font()).color(p.text_faint));
                ui.add_space(Spacing::XS);

                for task in &event_tasks {
                    ui.horizontal(|ui| {
                        let mut completed = task.completed;
                        if checkbox(ui, theme, &mut completed, &task.title).clicked() {
                            task_toggle = Some((task.id, completed));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Remove").clicked() {
                                task_detach = Some(task.id);
                            }
                        });
                    });
                }
                if !event_tasks.is_empty() {
                    let completed = event_tasks.iter().filter(|t| t.completed).count();
                    ui.label(
                        RichText::new(format!("{completed} / {} complete", event_tasks.len()))
                            .font(theme.small_font())
                            .color(p.text_faint),
                    );
                }

                ui.add_space(Spacing::XS);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let add_clicked = RButton::new("Add").show(ui, theme).clicked();
                        ui.add_space(Spacing::SM);
                        let new_task_response =
                            Input::new(&mut edit.new_task_input, "Add a task to this session…").show(ui, theme);
                        if new_task_response.lost_focus() && enter_pressed {
                            add_task_via_enter = true;
                        }
                        if add_clicked || add_task_via_enter {
                            add_task_requested = true;
                        }
                    });
                });
            }

            // --- Related notes (Sprint 8) — "opening a revision
            // session can show related notes". Display + quick-add
            // only; full editing stays in the Notes view.
            if edit.id.is_some() {
                ui.add_space(Spacing::MD);
                ui.separator();
                ui.add_space(Spacing::SM);
                ui.label(RichText::new("Notes").font(theme.small_font()).color(p.text_faint));
                ui.add_space(Spacing::XS);

                if event_notes.is_empty() {
                    ui.label(
                        RichText::new("No notes for this session yet.")
                            .font(theme.small_font())
                            .color(p.text_faint),
                    );
                }
                for note in &event_notes {
                    ui.label(RichText::new(&note.title).font(theme.label_font()).color(p.text));
                    let preview = note.preview(100);
                    if !preview.is_empty() {
                        ui.label(RichText::new(preview).font(theme.small_font()).color(p.text_faint));
                    }
                    ui.add_space(Spacing::XS);
                }

                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let add_clicked = RButton::new("Add").show(ui, theme).clicked();
                        ui.add_space(Spacing::SM);
                        let new_note_response =
                            Input::new(&mut edit.new_note_input, "Add a note for this session…").show(ui, theme);
                        if new_note_response.lost_focus() && enter_pressed {
                            add_note_via_enter = true;
                        }
                        if add_clicked || add_note_via_enter {
                            add_note_requested = true;
                        }
                    });
                });
            }

            ui.add_space(Spacing::MD);
            ui.horizontal(|ui| {
                if RButton::new("Save").primary().show(ui, theme).clicked()
                    || (enter_pressed && !add_task_via_enter && !add_note_via_enter)
                {
                    save_requested = true;
                }
                if RButton::new("Cancel").show(ui, theme).clicked() {
                    cancel_requested = true;
                }
                if edit.id.is_some() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Delete").clicked() {
                            delete_requested = true;
                        }
                    });
                }
            });
        }
    });

    if let Some((id, completed)) = task_toggle {
        let _ = queries::tasks::set_completed(&db.conn, id, completed);
    }
    if let Some(id) = task_detach {
        let _ = queries::tasks::set_event(&db.conn, id, None);
    }
    if add_task_requested {
        if let (Some(edit), Some(event_id)) = (&mut state.editing, editing_id) {
            let title = edit.new_task_input.trim();
            if !title.is_empty() {
                let _ = queries::tasks::create_task(&db.conn, title, None, Some(event_id), None);
                edit.new_task_input.clear();
            }
        }
        return;
    }

    if add_note_requested {
        if let (Some(edit), Some(event_id)) = (&mut state.editing, editing_id) {
            let title = edit.new_note_input.trim();
            if !title.is_empty() {
                let _ = queries::notes::create_note(&db.conn, title, "", None, Some(event_id), None);
                edit.new_note_input.clear();
            }
        }
        return;
    }

    if delete_requested {
        if let Some(edit) = &state.editing {
            if let Some(id) = edit.id {
                let _ = queries::events::delete_event(&db.conn, id);
            }
        }
        state.editing = None;
        return;
    }

    if save_requested {
        try_save_event(db, state);
        return;
    }

    if cancel_requested || !modal_open {
        state.editing = None;
    }
}

fn log_edit_modal(ui: &mut egui::Ui, theme: &Theme, db: &Database, state: &mut CalendarState) {
    let p = &theme.palette;
    let mut modal_open = state.log_editing.is_some();
    let mut save_requested = false;
    let mut cancel_requested = false;

    Modal::new("log_edit_modal", "Log Session Hours").show(ui.ctx(), theme, &mut modal_open, |ui| {
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if let Some(edit) = &mut state.log_editing {
            ui.label(
                RichText::new("How many minutes did this session actually take?")
                    .font(theme.small_font())
                    .color(p.text_faint),
            );
            ui.add_space(Spacing::SM);
            Input::new(&mut edit.minutes_input, "e.g. 45").id("log_edit_minutes").width(80.0).show(ui, theme);

            if let Some(error) = &edit.error {
                ui.add_space(Spacing::SM);
                ui.label(RichText::new(error).font(theme.small_font()).color(p.text));
            }

            ui.add_space(Spacing::MD);
            ui.horizontal(|ui| {
                if RButton::new("Save").primary().show(ui, theme).clicked() || enter_pressed {
                    save_requested = true;
                }
                if RButton::new("Cancel").show(ui, theme).clicked() {
                    cancel_requested = true;
                }
            });
        }
    });

    if save_requested {
        if let Some(edit) = &mut state.log_editing {
            match edit.minutes_input.trim().parse::<i64>() {
                Ok(minutes) if minutes >= 0 => {
                    let _ = queries::session_log::log_session(
                        &db.conn,
                        edit.event_id,
                        edit.date,
                        crate::events::SessionStatus::Adjusted,
                        Some(minutes),
                    );
                    state.log_editing = None;
                }
                _ => edit.error = Some("Enter a whole number of minutes, zero or more.".to_owned()),
            }
        }
        return;
    }

    if cancel_requested || !modal_open {
        state.log_editing = None;
    }
}

fn try_save_event(db: &Database, state: &mut CalendarState) {
    let Some(edit) = &mut state.editing else { return };

    if edit.title.trim().is_empty() {
        edit.error = Some("Title can't be empty.".to_owned());
        return;
    }

    let date = match crate::natural_date::parse_natural_date(&edit.date, Local::now().date_naive()) {
        Ok(Some(d)) => d,
        Ok(None) => {
            edit.error = Some("Date can't be empty.".to_owned());
            return;
        }
        Err(message) => {
            edit.error = Some(message);
            return;
        }
    };

    let (start, end) = match crate::ui::event_time::parse_event_times(date, &edit.start_time, &edit.end_time) {
        Ok(times) => times,
        Err(message) => {
            edit.error = Some(message);
            return;
        }
    };

    let desired_rule: Option<RecurrenceRule> = if edit.repeat_frequency == RepeatOption::Never {
        None
    } else {
        let interval: u32 = match edit.repeat_interval.trim().parse::<u32>() {
            Ok(n) if n >= 1 => n,
            _ => {
                edit.error = Some("Repeat interval must be a positive number.".to_owned());
                return;
            }
        };
        let frequency = match edit.repeat_frequency {
            RepeatOption::Daily => Frequency::Daily,
            RepeatOption::Weekly => Frequency::Weekly,
            RepeatOption::Monthly => Frequency::Monthly,
            RepeatOption::Never => unreachable!(),
        };
        let weekdays: Vec<Weekday> = if frequency == Frequency::Weekly {
            WEEKDAY_ORDER
                .iter()
                .zip(edit.repeat_weekdays.iter())
                .filter(|(_, &selected)| selected)
                .map(|(day, _)| *day)
                .collect()
        } else {
            Vec::new()
        };
        let end_date = match edit.repeat_end_mode {
            EndMode::OnDate => match crate::natural_date::parse_natural_date(&edit.repeat_end_date, Local::now().date_naive()) {
                Ok(Some(d)) => Some(d),
                Ok(None) => {
                    edit.error = Some("Repeat end date can't be empty.".to_owned());
                    return;
                }
                Err(message) => {
                    edit.error = Some(message);
                    return;
                }
            },
            _ => None,
        };
        let occurrence_limit = match edit.repeat_end_mode {
            EndMode::AfterCount => match edit.repeat_occurrence_limit.trim().parse::<u32>() {
                Ok(n) if n >= 1 => Some(n),
                _ => {
                    edit.error = Some("Occurrence count must be a positive number.".to_owned());
                    return;
                }
            },
            _ => None,
        };

        Some(RecurrenceRule { id: None, frequency, interval, weekdays, end_date, occurrence_limit })
    };

    let recurrence_id: Option<i64> = match (&desired_rule, edit.original_recurrence_id) {
        (None, None) => None,
        (None, Some(old_rule_id)) => {
            let _ = queries::recurrence::delete_rule(&db.conn, old_rule_id);
            if let Some(event_id) = edit.id {
                let _ = queries::recurrence::delete_exceptions_for_event(&db.conn, event_id);
            }
            None
        }
        (Some(rule), None) => match queries::recurrence::create_rule(&db.conn, rule) {
            Ok(id) => Some(id),
            Err(err) => {
                edit.error = Some(format!("Could not save repeat rule: {err}"));
                return;
            }
        },
        (Some(rule), Some(old_rule_id)) => {
            if let Err(err) = queries::recurrence::update_rule(&db.conn, old_rule_id, rule) {
                edit.error = Some(format!("Could not update repeat rule: {err}"));
                return;
            }
            Some(old_rule_id)
        }
    };

    let subject_id = edit.subject_id;
    let description = edit.description.trim();
    let title = edit.title.trim();
    let exec_command = {
        let trimmed = edit.exec_command.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    };

    let result: Result<i64, rusqlite::Error> = match edit.id {
        Some(id) => queries::events::update_event(&db.conn, id, title, description, start, end, subject_id, recurrence_id)
            .map(|()| id),
        None => queries::events::create_event(&db.conn, title, description, start, end, subject_id, recurrence_id)
            .map(|event| event.id),
    };

    match result {
        Ok(event_id) => {
            let _ = queries::events::set_exec_command(&db.conn, event_id, exec_command.as_deref());
            let _ = queries::events::set_track_session(&db.conn, event_id, edit.track_session);
            state.editing = None;
        }
        Err(err) => {
            if let Some(edit) = &mut state.editing {
                edit.error = Some(format!("Could not save: {err}"));
            }
        }
    }
}

#[cfg(test)]
mod calendar_math_tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn week_bounds_returns_monday_to_sunday() {
        // 2026-08-27 is a Thursday.
        let (start, end) = week_bounds(date(2026, 8, 27));
        assert_eq!(start, date(2026, 8, 24)); // Monday
        assert_eq!(end, date(2026, 8, 30)); // Sunday
        assert_eq!(start.weekday(), Weekday::Mon);
        assert_eq!(end.weekday(), Weekday::Sun);
    }

    #[test]
    fn week_bounds_on_a_sunday_still_ends_that_sunday() {
        let sunday = date(2026, 8, 30);
        let (start, end) = week_bounds(sunday);
        assert_eq!(start, date(2026, 8, 24));
        assert_eq!(end, sunday);
    }

    #[test]
    fn shift_month_moves_forward_and_lands_on_the_first() {
        assert_eq!(shift_month(date(2026, 1, 31), 1), date(2026, 2, 1));
        assert_eq!(shift_month(date(2026, 1, 15), 3), date(2026, 4, 1));
    }

    #[test]
    fn shift_month_moves_backward_across_a_year_boundary() {
        assert_eq!(shift_month(date(2026, 1, 15), -1), date(2025, 12, 1));
    }

    #[test]
    fn month_grid_covers_the_whole_month_padded_to_full_weeks() {
        let weeks = month_grid_weeks(2026, 8); // August 2026: Sat 1 - Mon 31
        let all_days: Vec<NaiveDate> = weeks.iter().flatten().copied().collect();

        // Every day of August must be present.
        for day in 1..=31 {
            assert!(all_days.contains(&date(2026, 8, day)), "missing Aug {day}");
        }
        // Every week is exactly 7 days, Monday-first.
        for week in &weeks {
            assert_eq!(week.len(), 7);
            assert_eq!(week[0].weekday(), Weekday::Mon);
            assert_eq!(week[6].weekday(), Weekday::Sun);
        }
        // Grid starts on or before the 1st, ends on or after the 31st.
        assert!(all_days[0] <= date(2026, 8, 1));
        assert!(*all_days.last().unwrap() >= date(2026, 8, 31));
    }

    #[test]
    fn month_grid_week_count_is_not_always_six() {
        // February 2026 starts on a Sunday and has 28 days — should
        // fit in fewer than 6 rows, unlike a month that starts near
        // the end of a week.
        let weeks = month_grid_weeks(2026, 2);
        assert!(weeks.len() < 6, "expected fewer than 6 weeks, got {}", weeks.len());
    }

    #[test]
    fn bar_height_scales_linearly_with_duration() {
        assert_eq!(bar_height_for_duration(60.0), PIXELS_PER_HOUR);
        assert_eq!(bar_height_for_duration(120.0), PIXELS_PER_HOUR * 2.0);
        assert_eq!(bar_height_for_duration(30.0), PIXELS_PER_HOUR / 2.0);
    }

    #[test]
    fn bar_height_never_drops_below_the_floor() {
        assert_eq!(bar_height_for_duration(1.0), MIN_BAR_HEIGHT);
        assert_eq!(bar_height_for_duration(0.0), MIN_BAR_HEIGHT);
        assert_eq!(bar_height_for_duration(-5.0), MIN_BAR_HEIGHT);
    }

    #[test]
    fn timeline_offset_is_zero_at_the_earliest_hour() {
        use chrono::TimeZone;
        let start = Local.with_ymd_and_hms(2026, 3, 10, 8, 0, 0).unwrap();
        assert_eq!(timeline_offset_minutes(start, 8), 0.0);
    }

    #[test]
    fn timeline_offset_scales_with_pixels_per_hour_not_a_fixed_row_height() {
        // The bug this timeline rewrite fixes: an event's vertical
        // position (and, via `bar_height_for_duration`, its height)
        // must be on the *same* scale as the hour ruler beside it, so
        // a 2-hour event visibly spans two hour lines instead of
        // just rendering a tall bar next to a grid that doesn't
        // otherwise reflect duration.
        use chrono::TimeZone;
        let nine_thirty = Local.with_ymd_and_hms(2026, 3, 10, 9, 30, 0).unwrap();
        assert_eq!(timeline_offset_minutes(nine_thirty, 8), 1.5 * PIXELS_PER_HOUR);

        let eleven = Local.with_ymd_and_hms(2026, 3, 10, 11, 0, 0).unwrap();
        assert_eq!(timeline_offset_minutes(eleven, 8), 3.0 * PIXELS_PER_HOUR);
    }

    #[test]
    fn timeline_offset_clamps_to_zero_if_start_precedes_the_earliest_hour() {
        use chrono::TimeZone;
        let start = Local.with_ymd_and_hms(2026, 3, 10, 7, 0, 0).unwrap();
        assert_eq!(timeline_offset_minutes(start, 8), 0.0);
    }

    #[test]
    fn occurrences_in_window_expands_a_recurring_event_across_the_range() {
        use chrono::{Local, TimeZone};

        let db = crate::database::Database::open_in_memory().expect("open in-memory db");
        let rule = RecurrenceRule {
            id: None,
            frequency: Frequency::Daily,
            interval: 1,
            weekdays: Vec::new(),
            end_date: None,
            occurrence_limit: None,
        };
        let rule_id = queries::recurrence::create_rule(&db.conn, &rule).expect("create rule");
        let start = Local.with_ymd_and_hms(2026, 8, 24, 9, 0, 0).unwrap();
        let end = Local.with_ymd_and_hms(2026, 8, 24, 10, 0, 0).unwrap();
        let event = queries::events::create_event(&db.conn, "Daily review", "", start, end, None, Some(rule_id))
            .expect("create event");

        let by_day = occurrences_in_window(&db, &[event], date(2026, 8, 24), date(2026, 8, 26));
        assert_eq!(by_day.len(), 3);
        assert!(by_day[&date(2026, 8, 24)][0].is_anchor);
        assert!(!by_day[&date(2026, 8, 25)][0].is_anchor);
        assert_eq!(by_day[&date(2026, 8, 25)][0].event.start_time.date_naive(), date(2026, 8, 25));
    }
}
