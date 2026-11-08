use std::collections::HashMap;

use chrono::{Datelike, Duration as ChronoDuration, Local, NaiveDate, Timelike, Weekday};
use eframe::egui::{self, Frame, CornerRadius, RichText, Stroke};

use crate::database::{queries, Database};
use crate::events::{occurrence, Event, Frequency, RecurrenceRule};
use crate::subjects::Subject;
use crate::tasks::{Task, TimeBlock};
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

/// The first and last clock hour a span touches on `day`. A span that
/// began on an earlier day counts from midnight and one that ends on a
/// later day runs to the last hour, so something crossing midnight
/// still lands on this day's timeline (it used to read the raw end
/// hour — `1` for a 23:00–01:00 event — which put the timeline's
/// bottom above its top).
fn span_hours_on_day(start: chrono::DateTime<Local>, end: chrono::DateTime<Local>, day: NaiveDate) -> (u32, u32) {
    let first = if start.date_naive() < day { 0 } else { start.hour() };
    let last = if end.date_naive() > day { 23 } else { end.hour() };
    (first, last.max(first))
}

/// The Day timeline's `(earliest, latest)` hour for a set of
/// `(first hour, last hour)` spans — events, time blocks and the
/// working window together. One hour of padding above, and the closing
/// line below the last hour, capped at 23. `latest >= earliest` always,
/// so the timeline height can never underflow. `None` when there is
/// nothing to show.
fn visible_hour_range(spans: &[(u32, u32)]) -> Option<(u32, u32)> {
    let first = spans.iter().map(|span| span.0).min()?;
    let last = spans.iter().map(|span| span.1).max()?;
    let earliest = first.saturating_sub(1).min(23);
    let latest = (last + 1).min(23).max(earliest);
    Some((earliest, latest))
}

/// Width of the hour-label column at the left of the Day timeline.
const TIME_LABEL_WIDTH: f32 = 48.0;

/// How far left of the hour labels the "now" pointer starts, so it reads as
/// a marker standing outside the ruler rather than part of it.
const NOW_POINTER_LEAD: f32 = 6.0;

/// Half the height of the hover target around the pointer, so a one-and-a-
/// half point line is still easy to rest the mouse on.
const NOW_POINTER_HOVER_HALF_HEIGHT: f32 = 4.0;

/// Feature request: a visible "now" pointer on the calendar. The y at
/// which the current time sits on the Day timeline, on the same
/// `PIXELS_PER_HOUR` scale the hour lines, events and blocks use, so it
/// moves down the day relative to them. Seconds count, so it creeps rather
/// than jumping a minute at a time. `None` when the time falls outside the
/// hours the timeline shows (it spans `earliest_hour:00` to the end of
/// `latest_hour`) — there is nowhere honest to draw it.
fn now_pointer_y(now: chrono::NaiveTime, timeline_top: f32, earliest_hour: u32, latest_hour: u32) -> Option<f32> {
    let hours = now.num_seconds_from_midnight() as f32 / 3600.0;
    let first = earliest_hour as f32;
    let end = (latest_hour + 1) as f32;
    if hours < first || hours > end {
        return None;
    }
    Some(timeline_top + (hours - first) * PIXELS_PER_HOUR)
}

/// The pointer's `(left, right)`: starting a little left of the hour labels
/// and running across the whole timeline to its right edge.
fn now_pointer_span(timeline_rect: egui::Rect) -> (f32, f32) {
    (timeline_rect.left() - NOW_POINTER_LEAD, timeline_rect.right())
}

/// Feature request: dim the part of today's timeline that has already
/// happened. The y the dim reaches down to: the pointer's, or the bottom of
/// the timeline once the day's shown hours are all over, or `None` while none
/// of them has begun (nothing is past yet).
fn past_dim_bottom(now: chrono::NaiveTime, timeline_top: f32, earliest_hour: u32, latest_hour: u32) -> Option<f32> {
    if let Some(y) = now_pointer_y(now, timeline_top, earliest_hour, latest_hour) {
        return Some(y);
    }
    let hours = now.num_seconds_from_midnight() as f32 / 3600.0;
    (hours > (latest_hour + 1) as f32).then(|| timeline_top + (latest_hour + 1 - earliest_hour) as f32 * PIXELS_PER_HOUR)
}

/// The dim: the page background at `percent` opacity, so past hours sink into
/// the background rather than turning a different colour.
fn past_dim_color(background: egui::Color32, percent: u8) -> egui::Color32 {
    let alpha = (percent.min(100) as f32 / 100.0 * 255.0).round() as u8;
    egui::Color32::from_rgba_unmultiplied(background.r(), background.g(), background.b(), alpha)
}

/// What the pointer's hover text says: the time including seconds.
fn now_pointer_label(now: chrono::NaiveTime) -> String {
    format!("Now — {}", now.format("%H:%M:%S"))
}

/// How tall the grab strip along a block's bottom edge is.
const RESIZE_HANDLE_PX: f32 = 6.0;

/// A block's `(start, end)` as minutes since midnight on its start day.
/// The end is `start + length`, so a block that runs past midnight
/// keeps its real length rather than wrapping.
fn block_minutes(block: &TimeBlock) -> (i64, i64) {
    let start = i64::from(block.start_time.hour()) * 60 + i64::from(block.start_time.minute());
    (start, start + block.duration().num_minutes().max(0))
}

/// A block's card on the Day timeline, from minutes since midnight: same
/// left edge, width and vertical scale as an event card, so the two line
/// up exactly.
fn block_rect_minutes(
    timeline_rect: egui::Rect,
    ruler_x: f32,
    start_minutes: i64,
    end_minutes: i64,
    earliest_hour: u32,
) -> egui::Rect {
    let top = timeline_rect.top() + (start_minutes - i64::from(earliest_hour) * 60) as f32 / 60.0 * PIXELS_PER_HOUR;
    let height = bar_height_for_duration((end_minutes - start_minutes).max(0) as f32);
    egui::Rect::from_min_size(
        egui::pos2(ruler_x + Spacing::SM, top),
        egui::vec2((timeline_rect.right() - ruler_x - Spacing::SM).max(200.0), height),
    )
}

/// The pointer's height on the timeline as minutes since midnight —
/// the inverse of the offset the cards are drawn at.
fn y_to_minutes(y: f32, timeline_top: f32, earliest_hour: u32) -> i64 {
    i64::from(earliest_hour) * 60 + ((y - timeline_top) / PIXELS_PER_HOUR * 60.0).round() as i64
}

/// Rounds to the nearest multiple of `granularity` minutes (a tie goes
/// later); a granularity below one is treated as one.
fn snap_minutes(minutes: i64, granularity: i64) -> i64 {
    let step = granularity.max(1);
    (minutes + step / 2).div_euclid(step) * step
}

/// Where a block of `len` minutes may start so it stays inside
/// `[lo, hi]`. A block longer than the whole span just sits at `lo`.
fn clamp_move(start: i64, len: i64, lo: i64, hi: i64) -> i64 {
    if len >= hi - lo { lo } else { start.clamp(lo, hi - len) }
}

/// Where a resized block may end: at least `min_len` after its start and
/// no later than `hi` (but never before the minimum, even if `hi` is).
fn clamp_resize_end(start: i64, end: i64, min_len: i64, hi: i64) -> i64 {
    let floor = start + min_len.max(1);
    end.clamp(floor, hi.max(floor))
}

/// The inverse of [`minutes_to_naive`]: minutes after midnight on `day`.
fn minutes_on_day(day: NaiveDate, at: chrono::NaiveDateTime) -> i64 {
    (at - day.and_time(chrono::NaiveTime::MIN)).num_minutes()
}

/// A small label that follows the pointer while a task is dragged, on the
/// topmost layer so nothing covers it.
fn paint_task_drag_ghost(
    ctx: &egui::Context,
    theme: &Theme,
    pointer: egui::Pos2,
    task: &Task,
    default_minutes: rusqlite::Result<i64>,
) {
    let p = &theme.palette;
    let minutes = task
        .estimated_minutes
        .filter(|m| *m > 0)
        .or_else(|| default_minutes.ok())
        .unwrap_or(crate::tasks::auto_assign::DEFAULT_TASK_MINUTES);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("task_drag_ghost")));
    let galley = painter.layout_no_wrap(format!("{} · ~{minutes}m", task.title), theme.small_font(), p.text);
    let rect = egui::Rect::from_min_size(pointer + egui::vec2(14.0, 14.0), galley.size() + egui::vec2(12.0, 6.0));
    painter.rect_filled(rect, CornerRadius::same(2), p.surface);
    painter.rect_stroke(rect, CornerRadius::same(2), Stroke::new(Border::THICK, p.focus), egui::StrokeKind::Inside);
    painter.galley(rect.min + egui::vec2(6.0, 3.0), galley, p.text);
}

/// Minutes since midnight on `day`, as wall-clock time.
fn minutes_to_naive(day: NaiveDate, minutes: i64) -> chrono::NaiveDateTime {
    day.and_time(chrono::NaiveTime::MIN) + ChronoDuration::minutes(minutes)
}

/// Whether `[start, end)` minutes on `day` would sit on top of any of
/// `obstacles` (the day's events and its other blocks). Touching is fine.
fn drag_would_overlap(
    day: NaiveDate,
    start: i64,
    end: i64,
    obstacles: &[crate::tasks::time_blocking::Interval],
) -> bool {
    let dragged = crate::tasks::time_blocking::Interval::new(minutes_to_naive(day, start), minutes_to_naive(day, end));
    obstacles.iter().any(|o| o.overlaps(&dragged))
}

/// How far inside a card its text starts: a little more when the border
/// is two lines, so the text never touches the inner one.
fn outline_inset(double: bool) -> f32 {
    if double { 5.0 } else { 3.0 }
}

/// The rectangles a card's border is drawn as: one for something that can
/// be moved, two nested for something that can't (a repeating event, or a
/// block that is being or has been worked). The inner one is dropped when
/// the card is too short to fit it, so a tiny card never draws a border
/// that crosses itself.
fn outline_rects(rect: egui::Rect, double: bool) -> Vec<egui::Rect> {
    let mut rects = vec![rect];
    if double {
        let inner = rect.shrink(3.0);
        if inner.width() > 4.0 && inner.height() > 4.0 {
            rects.push(inner);
        }
    }
    rects
}

/// Paints a card's border (see [`outline_rects`]). A double border uses
/// the thinner stroke for each line so the pair reads as one heavier edge.
fn paint_outline(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32, double: bool, heavy: bool) {
    let width = match (double, heavy) {
        (true, false) => Border::HAIRLINE,
        (true, true) => Border::THICK,
        (false, true) => Border::THICK + 1.0,
        (false, false) => Border::THICK,
    };
    for outline in outline_rects(rect, double) {
        painter.rect_stroke(outline, CornerRadius::same(2), Stroke::new(width, color), egui::StrokeKind::Inside);
    }
}

/// Whether an event occurrence is *finished*: its session was logged as
/// completed, or as a different length than scheduled (which is also work
/// that happened). "Not done" is a resolved occurrence but not a finished
/// one, and an occurrence that merely lies in the past is not finished
/// either — only a logged outcome says it was.
fn event_finished(current_log: Option<&crate::events::SessionLogEntry>) -> bool {
    use crate::events::SessionStatus;
    matches!(current_log.map(|l| l.status), Some(SessionStatus::Completed | SessionStatus::Adjusted))
}

/// The small notes after an event card's title and times, in order. A
/// finished event has none — it reads as just its name, times and colour.
fn event_card_notes(
    occ: &DayOccurrence,
    event_tasks: &[Task],
    current_log: Option<&crate::events::SessionLogEntry>,
) -> Vec<String> {
    if event_finished(current_log) {
        return Vec::new();
    }
    let mut notes = Vec::new();
    if let Some(label) = &occ.rule_label {
        notes.push(format!("↻ {label}"));
    }
    if !event_tasks.is_empty() {
        let completed = event_tasks.iter().filter(|t| t.completed).count();
        notes.push(format!("{completed} / {} tasks", event_tasks.len()));
    }
    if matches!(current_log.map(|l| l.status), Some(crate::events::SessionStatus::Denied)) {
        notes.push("not done".to_owned());
    }
    notes
}

/// An event's start–end as text, e.g. `09:00–11:00` or `23:00–01:00 (+1 day)`.
fn event_time_range(event: &Event) -> String {
    let mut range = format!("{}–{}", event.start_time.format("%H:%M"), event.end_time.format("%H:%M"));
    if event.crosses_midnight() {
        range.push_str(" (+1 day)");
    }
    range
}

/// The hover text of an event card — everything that used to sit on the
/// card itself and no longer fits the compact look: the subject, the
/// length, the repeat rule, the description, the tasks and the logged outcome.
fn event_tooltip(
    occ: &DayOccurrence,
    subjects: &[Subject],
    event_tasks: &[Task],
    current_log: Option<&crate::events::SessionLogEntry>,
) -> String {
    use crate::events::SessionStatus;
    let event = &occ.event;
    let mut lines = vec![event.title.clone()];
    if let Some(subject) = subject_name(subjects, event.subject_id) {
        lines.push(subject.to_string());
    }
    lines.push(format!("{} ({})", event_time_range(event), format_duration(event.duration())));
    if let Some(label) = &occ.rule_label {
        lines.push(format!("↻ {label}"));
    }
    if !event.description.is_empty() {
        lines.push(event.description.clone());
    }
    if !event_tasks.is_empty() {
        let completed = event_tasks.iter().filter(|t| t.completed).count();
        lines.push(format!("{completed} / {} tasks complete", event_tasks.len()));
    }
    match current_log.map(|l| l.status) {
        Some(SessionStatus::Completed) => lines.push("Logged: completed".to_owned()),
        Some(SessionStatus::Denied) => lines.push("Logged: not done".to_owned()),
        Some(SessionStatus::Adjusted) => {
            lines.push(format!("Logged: {}m", current_log.and_then(|l| l.actual_minutes).unwrap_or(0)))
        }
        None => {}
    }
    lines.push(String::new());
    if event.movable && event.recurrence_id.is_none() {
        lines.push("Drag to move, drag the bottom edge to resize".to_owned());
    }
    lines.push("Right-click for actions".to_owned());
    lines.join("\n")
}

/// Draws one event as a card in the same style as a time block: an outline
/// in its subject's colour, its name, its times and a few small notes. Every
/// button the old card carried is in its right-click menu instead. A
/// repeating event's border is double — an occurrence can't be moved on its
/// own — and a finished event shows only its name, times and colour.
#[allow(clippy::too_many_arguments)]
fn draw_event_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
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
    use crate::events::SessionStatus;
    let p = &theme.palette;
    let event = &occ.event;
    let double = occ.rule_label.is_some();

    // A subject-linked event takes its subject's colour; one with none falls
    // back to the recurring / one-off distinction the accent bar always drew.
    let accent = match event.subject_id {
        Some(subject_id) => crate::theme::subject_color(subjects, subject_id),
        None if double => p.text_muted,
        None => p.focus,
    };
    paint_outline(ui.painter(), rect, accent, double, false);

    let inner = rect.shrink2(egui::vec2(Spacing::SM, outline_inset(double)));
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.set_clip_rect(rect.intersect(ui.clip_rect()));
        ui.horizontal(|ui| {
            ui.label(RichText::new(&event.title).font(theme.label_font()).color(p.text));
            ui.label(RichText::new(event_time_range(event)).font(theme.small_font()).color(p.text_muted));
            for note in event_card_notes(occ, event_tasks, current_log) {
                ui.label(RichText::new(note).font(theme.small_font()).color(p.text_faint));
            }
        });
    });

    let response = ui.interact(
        rect,
        egui::Id::new(("event_card", event.id, event.day())),
        egui::Sense::click(),
    );
    response.context_menu(|ui| {
        let day = event.day();
        let has_started = event.start_time <= Local::now();
        if event.track_session && current_log.is_none() && ui.button("Start session").clicked() {
            *start_session = Some((event.id, day));
            ui.close();
        }
        if event.track_session && has_started {
            if current_log.is_none() {
                if ui.button("Mark done").clicked() {
                    *log_action = Some((event.id, day, SessionStatus::Completed));
                    ui.close();
                }
                if ui.button("Mark not done").clicked() {
                    *log_action = Some((event.id, day, SessionStatus::Denied));
                    ui.close();
                }
            } else {
                if ui.button("Edit logged hours").clicked() {
                    *open_log_editor = Some((event.id, day));
                    ui.close();
                }
                if ui.button("Clear log").clicked() {
                    *log_clear = Some((event.id, day));
                    ui.close();
                }
            }
        }
        if ui.button("Edit").clicked() {
            *edit_target = Some(event.id);
            ui.close();
        }
        if !event_tasks.is_empty() {
            ui.separator();
            for task in event_tasks {
                let mut is_completed = task.completed;
                if checkbox(ui, theme, &mut is_completed, &task.title).clicked() {
                    *toggle_task = Some((task.id, is_completed));
                }
            }
        }
        ui.separator();
        if event.recurrence_id.is_some() {
            if ui.button("Skip this occurrence").clicked() {
                *skip_occurrence = Some((event.id, day));
                ui.close();
            }
            if ui.button("Delete series").clicked() {
                *delete_id = Some(event.id);
                ui.close();
            }
        } else if ui.button("Delete").clicked() {
            *delete_id = Some(event.id);
            ui.close();
        }
    });
    response.on_hover_text(event_tooltip(occ, subjects, event_tasks, current_log));
}

/// Draws one time block — the style events now share. An outline in the
/// task's subject colour, its name, and a few small notes. A block that
/// can't be moved (being worked, or already worked) has a **double**
/// border. A *finished* block (worked, or its task is done) shows only its
/// name, its times and its subject colour — no strike-through, no dimming,
/// no labels. A planned one shows its length, and says "pinned" if it is;
/// one that an event has since landed on says so too — the palette is
/// monochrome, so that is text and a heavier outline rather than a colour.
/// A selected block gets a heavier outline; a block being dragged somewhere
/// it can't go is drawn faint. The interactions (drag, resize, menu) are
/// wired up by the caller.
#[allow(clippy::too_many_arguments)]
fn draw_time_block(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    block: &TimeBlock,
    task: Option<&Task>,
    subjects: &[Subject],
    conflicting: bool,
    selected: bool,
    invalid_drop: bool,
) {
    let p = &theme.palette;
    let worked = block.status == crate::tasks::BlockStatus::Done;
    let running = block.status == crate::tasks::BlockStatus::InProgress;
    // Finished = the task is done, or this block records work that happened.
    let finished = task.is_some_and(|t| t.completed) || worked;
    // A block being or already worked can't be moved: double border.
    let double = block.is_settled();
    let accent = match task.and_then(|t| t.subject_id) {
        Some(subject_id) => crate::theme::subject_color(subjects, subject_id),
        None => p.text_muted,
    };
    // Subject colour, whatever the state; only a refused drop (faint) and a
    // clash (the text colour) override it.
    let (color, heavy) = if invalid_drop {
        (p.text_faint, false)
    } else if conflicting {
        (p.text, false)
    } else {
        (accent, selected)
    };
    paint_outline(ui.painter(), rect, color, double, heavy);

    // The label shows the length being drawn, so a drag reads live.
    let minutes = ((rect.height() / PIXELS_PER_HOUR) * 60.0).round() as i64;
    let title = task.map(|t| t.title.as_str()).unwrap_or("(task removed)");

    let inner = rect.shrink2(egui::vec2(Spacing::SM, outline_inset(double)));
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.set_clip_rect(rect.intersect(ui.clip_rect()));
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).font(theme.label_font()).color(p.text));
            if finished {
                // Blank but for the name, the times and the colour.
                let times = format!("{}–{}", block.start_time.format("%H:%M"), block.end_time.format("%H:%M"));
                ui.label(RichText::new(times).font(theme.small_font()).color(p.text_muted));
            } else {
                ui.label(RichText::new(format!("~{minutes}m")).font(theme.small_font()).color(p.text_faint));
                if running {
                    ui.label(RichText::new("in progress").font(theme.small_font()).color(p.text));
                } else if block.locked {
                    ui.label(RichText::new("pinned").font(theme.small_font()).color(p.text_muted));
                }
            }
        });
        if conflicting {
            ui.label(RichText::new("clashes with an event").font(theme.small_font()).color(p.text));
        }
    });
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
    /// Feature request: "Movable events" — whether the event can be dragged and
    /// resized in the Day view like a task's block.
    pub movable: bool,
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
            movable: false,
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
            movable: event.movable,
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
    /// Feature request: "Time-blocking" — the planned block the person
    /// last clicked or dragged in the Day view (drawn with a heavier
    /// outline), and the drag in progress, if any. UI-only.
    pub selected_block: Option<i64>,
    pub block_drag: Option<BlockDrag>,
    /// Feature request: dragging a task from "Tasks for this day" onto the
    /// timeline — the task being dragged, from the press on its grip until
    /// the button is released. UI-only.
    pub task_drag: Option<i64>,
    /// Feature request: "Movable events" — the drag in progress on a movable
    /// event (`block_id` is the event's id). UI-only, like `block_drag`.
    pub event_drag: Option<BlockDrag>,
    /// What the last drop did (the day it was on, and a one-line summary),
    /// shown above the timeline while that day is open.
    pub drop_notice: Option<(NaiveDate, String)>,
}

/// A block being dragged (moved) or resized on the Day timeline. Times
/// are minutes since midnight on the day shown; `start`/`end` are the
/// live preview, not yet saved — the block is written once, on release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockDrag {
    pub block_id: i64,
    /// `true` = the bottom edge is being dragged (resize); `false` = the
    /// body (move).
    pub resize: bool,
    /// How far below the block's top the pointer grabbed it, so a move
    /// follows the pointer without the block jumping to it.
    pub grab_offset: i64,
    pub start: i64,
    pub end: i64,
}

/// What the person asked a block to do, applied after the frame is drawn.
enum BlockAction {
    /// Save a finished drag: move, or resize (the task's estimate is left
    /// alone; the time a shorter block no longer covers is planned again later).
    Commit { id: i64, start: i64, end: i64, resize: bool },
    /// Save a finished drag of a movable event: its new start and end.
    CommitEvent { id: i64, start: i64, end: i64 },
    SetLocked(i64, bool),
    Remove(i64),
    Replan,
    /// Cut a block in two, so each half can be moved and resized separately.
    Split(i64),
    /// A task from "Tasks for this day" was released over the timeline at
    /// `drop_minutes` after midnight, inside `bounds`.
    DropTask { task_id: i64, drop_minutes: i64, bounds: (i64, i64) },
}

impl Default for CalendarState {
    fn default() -> Self {
        Self {
            subview: CalendarSubview::Day,
            selected_day: Local::now().date_naive(),
            editing: None,
            subject_filter: None,
            log_editing: None,
            selected_block: None,
            block_drag: None,
            task_drag: None,
            event_drag: None,
            drop_notice: None,
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

/// The Calendar views' name for the UI-free occurrence type; the
/// expansion logic itself lives in `events::occurrence` so the block
/// planner can share it.
type DayOccurrence = occurrence::Occurrence;

/// Thin `&Database` adapters over `events::occurrence`, kept so the
/// views' call sites stay as they were.
fn occurrences_in_window(
    db: &Database,
    events: &[Event],
    range_start: NaiveDate,
    range_end: NaiveDate,
) -> HashMap<NaiveDate, Vec<DayOccurrence>> {
    occurrence::occurrences_in_window(&db.conn, events, range_start, range_end)
}

/// Every occurrence that falls on `day`, sorted by start time.
fn occurrences_on_day(db: &Database, events: &[Event], day: NaiveDate) -> Vec<DayOccurrence> {
    occurrence::occurrences_on_day(&db.conn, events, day)
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

/// Splits the tasks assigned to `day_str` into those a person placed
/// and those Auto-assign did — feature request: an extra Calendar
/// section for auto-assigned tasks, "to make it easier to clarify"
/// which is which. Auto-assign's own work is `auto_assigned` tasks
/// (days the planner picked) *and* generated per-day topic tasks
/// (which it created and fixed to their day); everything else on the
/// day was assigned by hand. Returned as `(manual, auto)`, each in the
/// input's order.
fn split_assigned<'a>(tasks: &'a [Task], day_str: &str) -> (Vec<&'a Task>, Vec<&'a Task>) {
    tasks
        .iter()
        .filter(|t| t.assigned_date.as_deref() == Some(day_str))
        .partition(|t| !(t.auto_assigned || t.generated_from.is_some()))
}

/// Total planned minutes of `tasks`, counting a task with no estimate
/// as `default_minutes` — the same assumption Auto-assign plans with.
fn planned_minutes(tasks: &[&Task], default_minutes: i64) -> i64 {
    tasks.iter().map(|t| t.estimated_minutes.filter(|m| *m > 0).unwrap_or(default_minutes)).sum()
}

/// The grip at the start of a task row. Dragging it onto the Day timeline
/// gives the task a time (see `BlockAction::DropTask`). A task that takes
/// no block (finished, a container, tied to a session) gets blank space
/// instead, so the checkboxes stay in one column. Drawn by hand — three
/// short lines — so it never depends on a font having the glyph.
fn task_drag_grip(ui: &mut egui::Ui, theme: &Theme, task_id: i64, draggable: bool, drag_start: &mut Option<i64>) {
    let sense = if draggable { egui::Sense::drag() } else { egui::Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(14.0, 18.0), sense);
    if !draggable {
        return;
    }
    // Test-only: lets the headless render tests find the grip. Compiled out of the real app.
    #[cfg(test)]
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(("test_task_grip", task_id)), rect));
    let color = if response.hovered() || response.dragged() { theme.palette.text } else { theme.palette.text_faint };
    for i in 0..3 {
        let y = rect.center().y + (i as f32 - 1.0) * 4.0;
        ui.painter().hline(rect.left() + 2.0..=rect.right() - 2.0, y, Stroke::new(Border::THICK, color));
    }
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    response.clone().on_hover_text("Drag onto the timeline to give this task a time");
    if response.drag_started() {
        *drag_start = Some(task_id);
    }
}

/// One row in either assigned-tasks section: a completion checkbox,
/// plus "Unassign" on the right when `allow_unassign` (not offered for
/// generated topic tasks — they're fixed to their day and belong to a
/// template, so un-assigning one would just leave it due that day).
fn assigned_task_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    task: &Task,
    note: Option<&str>,
    allow_unassign: bool,
    draggable: bool,
    progress: Option<crate::tasks::time_bar::TaskProgress>,
    toggle: &mut Option<(i64, bool)>,
    unassign: &mut Option<i64>,
    drag_start: &mut Option<i64>,
) {
    ui.horizontal(|ui| {
        task_drag_grip(ui, theme, task.id, draggable, drag_start);
        let mut completed = task.completed;
        if crate::ui::components::checkbox(ui, theme, &mut completed, &task.title).clicked() {
            *toggle = Some((task.id, completed));
        }
        // Feature request: task "bars" — time spent against the budget, beside the name.
        if let Some(progress) = progress {
            crate::ui::components::task_bar::task_bar(ui, theme, progress);
        }
        if let Some(minutes) = task.estimated_minutes {
            ui.label(RichText::new(format!("~{minutes}m")).font(theme.small_font()).color(theme.palette.text_faint));
        }
        // Time blocking: when it's planned, or "unscheduled" (only while time blocking is on).
        if let Some(note) = note {
            ui.label(RichText::new(note).font(theme.small_font()).color(theme.palette.text_muted));
        }
        if allow_unassign {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Unassign").clicked() {
                    *unassign = Some(task.id);
                }
            });
        }
    });
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
///
/// Feature request: "Auto Assigning" — tasks Auto-assign placed on
/// this day get their own "Auto-assigned" section right below (only
/// when there are any), so what you assigned and what the planner did
/// stay distinct; see `split_assigned`.
///
/// Feature request: a task row that takes a time block has a grip on its
/// left; the id of a task whose grip started being dragged this frame is
/// returned (the Day view carries the drag from there).
fn tasks_for_day_section(ui: &mut egui::Ui, theme: &Theme, db: &Database, day: NaiveDate) -> Option<i64> {
    let p = &theme.palette;
    let day_str = day.format("%Y-%m-%d").to_string();
    let all_tasks = queries::tasks::list_tasks(&db.conn).unwrap_or_default();
    let (manual, auto) = split_assigned(&all_tasks, &day_str);
    let default_minutes = queries::settings::auto_assign_default_task_minutes(&db.conn)
        .unwrap_or(crate::tasks::auto_assign::DEFAULT_TASK_MINUTES);

    let mut toggle: Option<(i64, bool)> = None;
    let mut unassign: Option<i64> = None;
    let mut assign: Option<i64> = None;
    let mut drag_start: Option<i64> = None;
    // Feature request: task "bars" — time spent per task.
    let progress = queries::task_time::progress_for(&db.conn, &all_tasks).unwrap_or_default();

    // Which rows can be dragged onto the timeline: the ones that take a
    // block at all, whether or not time blocking is on (a hand-placed
    // block is drawn either way).
    let containers = queries::auto_assign::container_ids(&all_tasks);
    let can_drag = |task: &Task| queries::time_blocking::is_blockable(task, &containers);

    // Time blocking: each row says when the task is planned that day, or
    // "unscheduled" if it should have a time and doesn't. Nothing is
    // computed (and nothing shown) while time blocking is off.
    let blocking_on = queries::settings::time_blocking_enabled(&db.conn).unwrap_or(false);
    let day_blocks = if blocking_on {
        queries::task_blocks::list_blocks_between(&db.conn, day, day).unwrap_or_default()
    } else {
        Vec::new()
    };
    let note_for = |task: &Task| -> Option<String> {
        let mine: Vec<&TimeBlock> = day_blocks.iter().filter(|b| b.task_id == task.id).collect();
        crate::tasks::block::schedule_note(blocking_on, can_drag(task), &mine)
    };

    Panel::new()
        .title(&format!(
            "Tasks for this day — {}/{}",
            manual.iter().filter(|t| t.completed).count(),
            manual.len()
        ))
        .show(ui, theme, |ui| {
            if manual.is_empty() {
                let text = if auto.is_empty() {
                    "No tasks assigned to this day yet."
                } else {
                    "Nothing assigned by hand — see Auto-assigned below."
                };
                ui.label(RichText::new(text).font(theme.small_font()).color(p.text_faint));
            }
            for task in &manual {
                assigned_task_row(
                    ui,
                    theme,
                    task,
                    note_for(task).as_deref(),
                    true,
                    can_drag(task),
                    progress.get(&task.id).copied(),
                    &mut toggle,
                    &mut unassign,
                    &mut drag_start,
                );
            }
            if manual.iter().chain(auto.iter()).any(|t| can_drag(t)) {
                ui.label(
                    RichText::new("Drag a task by its grip onto the timeline below to give it a time.")
                        .font(theme.small_font())
                        .color(p.text_faint),
                );
            }

            let assignable: Vec<&Task> = all_tasks
                .iter()
                .filter(|t| !t.completed && !t.plan_topics_ahead && t.assigned_date.as_deref() != Some(day_str.as_str()))
                .collect();
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

    if !auto.is_empty() {
        ui.add_space(Spacing::MD);
        Panel::new()
            .title(&format!(
                "Auto-assigned — {}/{} · {} min",
                auto.iter().filter(|t| t.completed).count(),
                auto.len(),
                planned_minutes(&auto, default_minutes)
            ))
            .show(ui, theme, |ui| {
                for task in &auto {
                    assigned_task_row(
                        ui,
                        theme,
                        task,
                        note_for(task).as_deref(),
                        task.generated_from.is_none(),
                        can_drag(task),
                        progress.get(&task.id).copied(),
                        &mut toggle,
                        &mut unassign,
                        &mut drag_start,
                    );
                }
            });
    }

    if let Some((id, completed)) = toggle {
        let _ = queries::tasks::set_completed(&db.conn, id, completed);
    }
    if let Some(id) = unassign {
        let _ = queries::tasks::set_assigned_date(&db.conn, id, None);
    }
    if let Some(id) = assign {
        let _ = queries::tasks::set_assigned_date(&db.conn, id, Some(&day_str));
    }
    drag_start
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
    let events = apply_subject_filter(queries::events::list_visible_events(&db.conn).unwrap_or_default(), state.subject_filter);

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

    if let Some(task_id) = tasks_for_day_section(ui, theme, db, state.selected_day) {
        state.task_drag = Some(task_id);
        state.drop_notice = None;
    }
    ui.add_space(Spacing::MD);

    if let Some((noted_day, text)) = &state.drop_notice {
        if *noted_day == state.selected_day {
            ui.label(RichText::new(text).font(theme.small_font()).color(p.text_muted));
            ui.add_space(Spacing::XS);
        }
    }
    // The task being dragged, for the preview, the ghost and the summary.
    let dragged_task: Option<Task> = state
        .task_drag
        .and_then(|id| queries::tasks::list_tasks(&db.conn).ok()?.into_iter().find(|t| t.id == id));

    let day = state.selected_day;
    let day_occurrences = occurrences_on_day(db, &events, day);

    // Feature request: "Time-blocking" — planned blocks for this day
    // (drawn read-only on the same timeline as the events), the tasks
    // they belong to, and which pinned ones an event has landed on.
    let day_blocks = queries::task_blocks::list_blocks_between(&db.conn, day, day).unwrap_or_default();
    let block_tasks: Vec<Task> =
        if day_blocks.is_empty() { Vec::new() } else { queries::tasks::list_tasks(&db.conn).unwrap_or_default() };
    let event_spans: Vec<crate::tasks::time_blocking::Interval> = day_occurrences
        .iter()
        .map(|occ| {
            crate::tasks::time_blocking::Interval::new(
                occ.event.start_time.naive_local(),
                occ.event.end_time.naive_local(),
            )
        })
        .collect();
    let clashing_blocks: Vec<i64> = {
        let pinned: Vec<(i64, crate::tasks::time_blocking::Interval)> =
            day_blocks.iter().filter(|b| b.locked && !b.is_settled()).map(|b| (b.id, b.interval())).collect();
        crate::tasks::time_blocking::find_conflicts(&pinned, &event_spans)
    };
    let granularity = queries::settings::slot_granularity_minutes(&db.conn).unwrap_or(15);
    // The working window widens the timeline only while time blocking
    // is on, so the Day view is unchanged for anyone who hasn't opted in.
    // A weekday set to "off" has no window, so the timeline isn't widened
    // for it and blocks can be dragged anywhere in the day.
    let work_window = if queries::settings::time_blocking_enabled(&db.conn).unwrap_or(false) {
        match queries::settings::work_day_for(&db.conn, day) {
            Ok(crate::tasks::time_blocking::WorkDay::Hours(start, end)) => Some((start, end)),
            _ => None,
        }
    } else {
        None
    };

    let mut delete_id: Option<i64> = None;
    let mut edit_target: Option<i64> = None;
    let mut toggle_task: Option<(i64, bool)> = None;
    let mut skip_occurrence: Option<(i64, NaiveDate)> = None;
    let mut log_action: Option<(i64, NaiveDate, crate::events::SessionStatus)> = None;
    let mut log_clear: Option<(i64, NaiveDate)> = None;
    let mut open_log_editor: Option<(i64, NaiveDate)> = None;
    let mut start_session: Option<(i64, NaiveDate)> = None;
    let mut block_action: Option<BlockAction> = None;

    Panel::new().show(ui, theme, |ui| {
        // (While a task is being dragged the timeline is drawn even on an empty day,
        // so there is somewhere to drop it.)
        if day_occurrences.is_empty() && day_blocks.is_empty() && state.task_drag.is_none() {
            ui.label(RichText::new("Nothing scheduled.").font(theme.small_font()).color(p.text_faint));
            return;
        }

        let mut hour_spans: Vec<(u32, u32)> = day_occurrences
            .iter()
            .map(|occ| span_hours_on_day(occ.event.start_time, occ.event.end_time, day))
            .collect();
        hour_spans.extend(day_blocks.iter().map(|b| span_hours_on_day(b.start_time, b.end_time, day)));
        if let Some((work_start, work_end)) = work_window {
            hour_spans.push((work_start as u32 / 60, (work_end as u32 - 1) / 60));
        }
        let (earliest_hour, latest_hour) = visible_hour_range(&hour_spans).unwrap_or((8, 18));

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

        let (timeline_rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), timeline_height), egui::Sense::hover());
        // Test-only: lets the headless render tests (`render_tests`) read
        // where the timeline was laid out. Compiled out of the real app.
        #[cfg(test)]
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("test_day_timeline_rect"), timeline_rect));

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

            // Feature request: "Movable events" — a one-off event marked movable that
            // lies within this day and hasn't finished can be dragged like a block.
            let event = &occ.event;
            let draggable = event.movable
                && event.recurrence_id.is_none()
                && event.day() == day
                && !event.crosses_midnight()
                && !event_finished(current_log.as_ref());
            let own = (
                minutes_on_day(day, event.start_time.naive_local()),
                minutes_on_day(day, event.end_time.naive_local()),
            );
            let event_preview = state.event_drag.as_ref().filter(|d| d.block_id == event.id).map(|d| (d.start, d.end));

            let top = timeline_rect.top() + timeline_offset_minutes(occ.event.start_time, earliest_hour);
            let duration_minutes = occ.event.duration().num_minutes().max(0) as f32;
            let bar_height = bar_height_for_duration(duration_minutes);
            let card_rect = match event_preview {
                Some((start_min, end_min)) => block_rect_minutes(timeline_rect, ruler_x, start_min, end_min, earliest_hour),
                None => egui::Rect::from_min_size(
                    egui::pos2(ruler_x + Spacing::SM, top),
                    egui::vec2((timeline_rect.right() - ruler_x - Spacing::SM).max(200.0), bar_height),
                ),
            };

            draw_event_card(
                ui,
                theme,
                card_rect,
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

            if draggable {
                let ev_bounds = (i64::from(earliest_hour) * 60, i64::from(latest_hour + 1) * 60);
                // What it must not land on: the day's other events and any block that is
                // pinned or already worked. (An unpinned planned block it lands on is just
                // flagged as clashing, as when an event is added; re-planning moves it.)
                let mut obstacles: Vec<crate::tasks::time_blocking::Interval> = day_occurrences
                    .iter()
                    .filter(|o| o.event.id != event.id)
                    .map(|o| {
                        crate::tasks::time_blocking::Interval::new(
                            o.event.start_time.naive_local(),
                            o.event.end_time.naive_local(),
                        )
                    })
                    .collect();
                obstacles.extend(day_blocks.iter().filter(|b| b.locked || b.is_settled()).map(|b| b.interval()));
                if event_preview.is_some_and(|(s, e)| drag_would_overlap(day, s, e, &obstacles)) {
                    paint_outline(ui.painter(), card_rect, p.incorrect, false, true);
                }

                let handle_rect = egui::Rect::from_min_max(
                    egui::pos2(card_rect.left(), (card_rect.bottom() - RESIZE_HANDLE_PX).max(card_rect.top())),
                    card_rect.right_bottom(),
                );
                let body = ui.interact(card_rect, egui::Id::new(("event_drag", event.id)), egui::Sense::drag());
                let handle = ui.interact(handle_rect, egui::Id::new(("event_drag_handle", event.id)), egui::Sense::drag());
                if handle.hovered() || handle.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                } else if body.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                } else if body.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                let pointer_minutes =
                    ui.input(|i| i.pointer.interact_pos()).map(|pos| y_to_minutes(pos.y, timeline_rect.top(), earliest_hour));
                if handle.drag_started() || body.drag_started() {
                    let grabbed_at = ui
                        .input(|i| i.pointer.press_origin())
                        .map(|pos| y_to_minutes(pos.y, timeline_rect.top(), earliest_hour))
                        .or(pointer_minutes);
                    if let Some(grabbed) = grabbed_at {
                        state.event_drag = Some(BlockDrag {
                            block_id: event.id,
                            resize: handle.drag_started(),
                            grab_offset: grabbed - own.0,
                            start: own.0,
                            end: own.1,
                        });
                    }
                }
                if handle.dragged() || body.dragged() {
                    if let (Some(pointer), Some(drag)) = (pointer_minutes, state.event_drag.as_mut()) {
                        if drag.block_id == event.id {
                            if drag.resize {
                                drag.end = clamp_resize_end(own.0, snap_minutes(pointer, granularity), granularity, ev_bounds.1);
                            } else {
                                let len = own.1 - own.0;
                                let start = clamp_move(
                                    snap_minutes(pointer - drag.grab_offset, granularity),
                                    len,
                                    ev_bounds.0,
                                    ev_bounds.1,
                                );
                                drag.start = start;
                                drag.end = start + len;
                            }
                        }
                    }
                }
                if handle.drag_stopped() || body.drag_stopped() {
                    if let Some(drag) = state.event_drag.take().filter(|d| d.block_id == event.id) {
                        let changed = (drag.start, drag.end) != own;
                        if changed && !drag_would_overlap(day, drag.start, drag.end, &obstacles) {
                            block_action =
                                Some(BlockAction::CommitEvent { id: event.id, start: drag.start, end: drag.end });
                        }
                    }
                }
            }
        }

        // Planned blocks, drawn after the events so an outline never
        // sits underneath an event card. They can be dragged to move,
        // dragged by the bottom edge to resize, and right-clicked for a
        // menu; every edit pins the block (see `task_blocks`).
        let bounds = work_window.unwrap_or((0, 24 * 60));
        for block in &day_blocks {
            let task = block_tasks.iter().find(|t| t.id == block.task_id);
            let (own_start, own_end) = block_minutes(block);
            let preview = state.block_drag.as_ref().filter(|d| d.block_id == block.id).map(|d| (d.start, d.end));
            let (start_min, end_min) = preview.unwrap_or((own_start, own_end));
            let rect = block_rect_minutes(timeline_rect, ruler_x, start_min, end_min, earliest_hour);

            // Everything this block must not land on: the day's events
            // and its other blocks.
            let mut obstacles = event_spans.clone();
            obstacles.extend(day_blocks.iter().filter(|b| b.id != block.id).map(|b| b.interval()));
            let invalid_drop = preview.is_some_and(|(s, e)| drag_would_overlap(day, s, e, &obstacles));

            draw_time_block(
                ui,
                theme,
                rect,
                block,
                task,
                subjects,
                clashing_blocks.contains(&block.id),
                state.selected_block == Some(block.id),
                invalid_drop,
            );

            // The handle is added after the body so it wins where they overlap.
            let handle_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left(), (rect.bottom() - RESIZE_HANDLE_PX).max(rect.top())),
                rect.right_bottom(),
            );
            // A block being worked, or already worked, records what happened: it
            // can be selected and read, but not dragged, resized or re-pinned.
            let editable = !block.is_settled();
            let body = ui.interact(
                rect,
                egui::Id::new(("time_block", block.id)),
                if editable { egui::Sense::click_and_drag() } else { egui::Sense::click() },
            );
            let handle = ui.interact(
                handle_rect,
                egui::Id::new(("time_block_handle", block.id)),
                if editable { egui::Sense::drag() } else { egui::Sense::hover() },
            );

            if editable {
                if handle.hovered() || handle.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                } else if body.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                } else if body.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
            }
            if body.clicked() {
                state.selected_block = Some(block.id);
            }

            let pointer_minutes = ui
                .input(|i| i.pointer.interact_pos())
                .map(|pos| y_to_minutes(pos.y, timeline_rect.top(), earliest_hour));

            if handle.drag_started() || body.drag_started() {
                // Measure the grab from where the button went DOWN, not from the
                // pointer now: egui only reports a drag once the pointer has moved
                // past a small threshold, and measuring from there would leave the
                // block lagging the pointer by that initial movement.
                let grabbed_at = ui
                    .input(|i| i.pointer.press_origin())
                    .map(|pos| y_to_minutes(pos.y, timeline_rect.top(), earliest_hour))
                    .or(pointer_minutes);
                if let Some(grabbed) = grabbed_at {
                    state.block_drag = Some(BlockDrag {
                        block_id: block.id,
                        resize: handle.drag_started(),
                        grab_offset: grabbed - own_start,
                        start: own_start,
                        end: own_end,
                    });
                    state.selected_block = Some(block.id);
                }
            }
            if handle.dragged() || body.dragged() {
                if let (Some(pointer), Some(drag)) = (pointer_minutes, state.block_drag.as_mut()) {
                    if drag.block_id == block.id {
                        if drag.resize {
                            drag.end = clamp_resize_end(own_start, snap_minutes(pointer, granularity), granularity, bounds.1);
                        } else {
                            let len = own_end - own_start;
                            let start = clamp_move(
                                snap_minutes(pointer - drag.grab_offset, granularity),
                                len,
                                bounds.0,
                                bounds.1,
                            );
                            drag.start = start;
                            drag.end = start + len;
                        }
                    }
                }
            }
            if handle.drag_stopped() || body.drag_stopped() {
                if let Some(drag) = state.block_drag.take().filter(|d| d.block_id == block.id) {
                    let changed = (drag.start, drag.end) != (own_start, own_end);
                    if changed && !drag_would_overlap(day, drag.start, drag.end, &obstacles) {
                        block_action = Some(BlockAction::Commit {
                            id: block.id,
                            start: drag.start,
                            end: drag.end,
                            resize: drag.resize,
                        });
                    }
                }
            }

            if editable {
            body.context_menu(|ui| {
                if block.locked {
                    if ui.button("Unpin (let re-planning move it)").clicked() {
                        block_action = Some(BlockAction::SetLocked(block.id, false));
                        ui.close();
                    }
                } else if ui.button("Pin here").clicked() {
                    block_action = Some(BlockAction::SetLocked(block.id, true));
                    ui.close();
                }
                // Feature request: Split — two blocks, each moved and resized on its own.
                let can_split = block.status == crate::tasks::BlockStatus::Planned
                    && (own_end - own_start) >= 2 * granularity;
                if ui.add_enabled(can_split, egui::Button::new("Split in two")).clicked() {
                    block_action = Some(BlockAction::Split(block.id));
                    ui.close();
                }
                if ui.button("Remove this block").clicked() {
                    block_action = Some(BlockAction::Remove(block.id));
                    ui.close();
                }
                if ui.button("Re-plan times").clicked() {
                    block_action = Some(BlockAction::Replan);
                    ui.close();
                }
            });
            }
            body.on_hover_text(format!(
                "{} – {}{}",
                minutes_to_naive(day, start_min).format("%H:%M"),
                minutes_to_naive(day, end_min).format("%H:%M"),
                match block.status {
                    crate::tasks::BlockStatus::InProgress => " (a timer or stopwatch is running against it)",
                    crate::tasks::BlockStatus::Done => " (worked: the time actually spent)",
                    crate::tasks::BlockStatus::Planned if block.locked => " (pinned: re-planning won't move it)",
                    crate::tasks::BlockStatus::Planned => "",
                }
            ));
        }

        // Feature request: a task dragged out of "Tasks for this day". While it is
        // over the timeline, show the block it would get (the same call the drop
        // saves with, so preview and result can't disagree); on release, drop it.
        if let Some(task_id) = state.task_drag {
            let released = ui.input(|i| i.pointer.any_released());
            let drop_area = egui::Rect::from_min_max(
                egui::pos2(ruler_x, timeline_rect.top()),
                egui::pos2(timeline_rect.right(), timeline_rect.bottom()),
            );
            let over = ui.input(|i| i.pointer.interact_pos()).filter(|pos| drop_area.contains(*pos));
            if let Some(pos) = over {
                let drop_minutes = y_to_minutes(pos.y, timeline_rect.top(), earliest_hour);
                match queries::time_blocking::plan_drop(&db.conn, task_id, day, drop_minutes, bounds) {
                    Ok(queries::time_blocking::DropPlan::Place(placement)) => {
                        let rect = block_rect_minutes(
                            timeline_rect,
                            ruler_x,
                            minutes_on_day(day, placement.start),
                            minutes_on_day(day, placement.end),
                            earliest_hour,
                        );
                        ui.painter().rect_filled(rect, CornerRadius::same(2), p.focus.gamma_multiply(0.18));
                        paint_outline(ui.painter(), rect, p.focus, false, true);
                        let mut label = format!("{}–{}", placement.start.format("%H:%M"), placement.end.format("%H:%M"));
                        if placement.is_short() {
                            label.push_str(&format!(" · {} of {} min fit", placement.minutes(), placement.wanted));
                        }
                        ui.painter().text(
                            rect.left_top() + egui::vec2(Spacing::SM, 3.0),
                            egui::Align2::LEFT_TOP,
                            label,
                            theme.small_font(),
                            p.text,
                        );
                    }
                    Ok(queries::time_blocking::DropPlan::NoRoom) | Ok(queries::time_blocking::DropPlan::NothingToPlace) | Err(_) => {
                        ui.painter().text(
                            pos + egui::vec2(16.0, -18.0),
                            egui::Align2::LEFT_TOP,
                            "No free time here",
                            theme.small_font(),
                            p.incorrect,
                        );
                    }
                }
                if released {
                    block_action = Some(BlockAction::DropTask { task_id, drop_minutes, bounds });
                }
            }
        }

        // Feature request: a single white line marking the current time,
        // starting left of the hour labels and running across the whole
        // timeline, with everything above it dimmed (the past). Drawn last so
        // events and blocks never cover it; hovering the line shows the time
        // including seconds. Only on today's timeline.
        if day == today {
            let now = Local::now().time();
            let pointer_y = now_pointer_y(now, timeline_rect.top(), earliest_hour, latest_hour);
            // Dim the past first, so the line is drawn over the dim.
            let dim_percent = queries::settings::calendar_past_dim_percent(&db.conn)
                .unwrap_or(queries::settings::DEFAULT_PAST_DIM_PERCENT);
            let dim_bottom = past_dim_bottom(now, timeline_rect.top(), earliest_hour, latest_hour);
            if let (Some(bottom), true) = (dim_bottom, dim_percent > 0) {
                let dimmed = egui::Rect::from_min_max(timeline_rect.min, egui::pos2(timeline_rect.right(), bottom));
                ui.painter().rect_filled(dimmed, 0.0, past_dim_color(p.background, dim_percent));
            }
            #[cfg(test)]
            ui.ctx().data_mut(|d| {
                let id = egui::Id::new("test_past_dim_bottom");
                if let (Some(bottom), true) = (dim_bottom, dim_percent > 0) {
                    d.insert_temp(id, bottom);
                } else {
                    d.remove_temp::<f32>(id);
                }
            });
            if let Some(y) = pointer_y {
                let (left, right) = now_pointer_span(timeline_rect);
                ui.painter().hline(left..=right, y, Stroke::new(Border::THICK, p.focus));
                let hover_rect = egui::Rect::from_min_max(
                    egui::pos2(left, y - NOW_POINTER_HOVER_HALF_HEIGHT),
                    egui::pos2(right, y + NOW_POINTER_HOVER_HALF_HEIGHT),
                );
                ui.interact(hover_rect, egui::Id::new("now_pointer"), egui::Sense::hover())
                    .on_hover_text(now_pointer_label(now));
            }
            // Test-only: where the pointer was drawn (cleared when it wasn't).
            #[cfg(test)]
            ui.ctx().data_mut(|d| {
                let id = egui::Id::new("test_now_pointer_y");
                if let Some(y) = pointer_y {
                    d.insert_temp(id, y);
                } else {
                    d.remove_temp::<f32>(id);
                }
            });
            // Nothing else repaints an idle window, so ask for a frame every
            // second: the line creeps down and the hover seconds tick.
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        } else {
            #[cfg(test)]
            ui.ctx().data_mut(|d| {
                d.remove_temp::<f32>(egui::Id::new("test_now_pointer_y"));
                d.remove_temp::<f32>(egui::Id::new("test_past_dim_bottom"));
            });
        }
    });

    // The ghost that follows the pointer while a task is dragged, and the end
    // of the drag (the button came up, or is no longer down).
    if let Some(task) = &dragged_task {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            paint_task_drag_ghost(ui.ctx(), theme, pos, task, queries::settings::auto_assign_default_task_minutes(&db.conn));
        }
    }
    if state.task_drag.is_some() && ui.input(|i| i.pointer.any_released() || !i.pointer.any_down()) {
        state.task_drag = None;
    }

    if let Some(action) = block_action {
        match action {
            BlockAction::Commit { id, start, end, resize } => {
                let start_time = crate::tasks::time_blocking::naive_to_local(minutes_to_naive(day, start));
                let end_time = crate::tasks::time_blocking::naive_to_local(minutes_to_naive(day, end));
                let _ = if resize {
                    // The estimate is left alone; what a shorter block no longer covers is
                    // planned again after it (see `resize_block`).
                    let now = Local::now();
                    queries::task_blocks::resize_block(&db.conn, id, start_time, end_time, now.date_naive(), now.naive_local())
                        .map(|_| ())
                } else {
                    queries::task_blocks::update_block_times(&db.conn, id, start_time, end_time)
                };
            }
            BlockAction::CommitEvent { id, start, end } => {
                let start_time = crate::tasks::time_blocking::naive_to_local(minutes_to_naive(day, start));
                let end_time = crate::tasks::time_blocking::naive_to_local(minutes_to_naive(day, end));
                let _ = queries::events::update_event_times(&db.conn, id, start_time, end_time);
                // The event is busy time for the planner. With automatic re-planning on, the
                // unpinned blocks it now sits on move aside; otherwise they are flagged.
                if queries::settings::time_blocking_enabled(&db.conn).unwrap_or(false)
                    && queries::settings::auto_replan_blocks(&db.conn).unwrap_or(false)
                {
                    let now = Local::now();
                    let _ = queries::time_blocking::plan_blocks(&db.conn, now.date_naive(), now.naive_local());
                }
            }
            BlockAction::SetLocked(id, locked) => {
                let _ = queries::task_blocks::set_block_locked(&db.conn, id, locked);
            }
            BlockAction::Remove(id) => {
                let _ = queries::task_blocks::delete_block(&db.conn, id);
            }
            BlockAction::Split(id) => {
                let _ = queries::task_blocks::split_block(&db.conn, id, granularity);
            }
            BlockAction::Replan => {
                let now = Local::now();
                let _ = queries::time_blocking::plan_blocks(&db.conn, now.date_naive(), now.naive_local());
            }
            BlockAction::DropTask { task_id, drop_minutes, bounds } => {
                let now = Local::now();
                let title = dragged_task.as_ref().map_or("task", |t| t.title.as_str());
                let text = match queries::time_blocking::drop_task(
                    &db.conn,
                    task_id,
                    day,
                    drop_minutes,
                    bounds,
                    now.date_naive(),
                    now.naive_local(),
                ) {
                    Ok(outcome) => outcome.summary(title),
                    Err(e) => format!("Couldn't place “{title}”: {e}"),
                };
                state.drop_notice = Some((day, text));
            }
        }
        ui.ctx().request_repaint();
    }

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

fn week_view(ui: &mut egui::Ui, theme: &Theme, db: &Database, state: &mut CalendarState, subjects: &[Subject]) {
    let p = &theme.palette;
    let today = Local::now().date_naive();
    let events = apply_subject_filter(queries::events::list_visible_events(&db.conn).unwrap_or_default(), state.subject_filter);

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

    // Time blocking: each day also lists its planned blocks, as small
    // read-only chips (`~09:00 Title`; the `~` matches the `~45m`
    // estimates elsewhere). Nothing is loaded while time blocking is off.
    let blocks_by_day = if queries::settings::time_blocking_enabled(&db.conn).unwrap_or(false) {
        crate::tasks::block::group_blocks_by_day(
            queries::task_blocks::list_blocks_between(&db.conn, week_start, week_end).unwrap_or_default(),
        )
    } else {
        HashMap::new()
    };
    let block_tasks: Vec<Task> =
        if blocks_by_day.is_empty() { Vec::new() } else { queries::tasks::list_tasks(&db.conn).unwrap_or_default() };

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

                    let day_blocks = blocks_by_day.get(&day).map(Vec::as_slice).unwrap_or(&[]);
                    if occurrences.is_empty() && day_blocks.is_empty() {
                        ui.label(RichText::new("—").font(theme.small_font()).color(p.text_faint));
                    }
                    for occ in &occurrences {
                        let recurring_mark = if occ.rule_label.is_some() { "↻ " } else { "" };
                        let line = format!("{}{}{}", recurring_mark, occ.event.start_time.format("%H:%M "), occ.event.title);
                        // Feature request: "Colour" — extended from
                        // the Day view's accent bar to Week view's own
                        // event lines the same way: a subject-linked
                        // event's own color, or the ordinary text
                        // color for one with no subject.
                        let line_color = occ
                            .event
                            .subject_id
                            .map(|id| crate::theme::subject_color(subjects, id))
                            .unwrap_or(p.text);
                        if ui
                            .selectable_label(
                                false,
                                RichText::new(line).font(theme.small_font()).color(line_color),
                            )
                            .clicked()
                        {
                            go_to_day = Some(day);
                        }
                    }
                    for block in day_blocks {
                        let task = block_tasks.iter().find(|t| t.id == block.task_id);
                        let title = task.map(|t| t.title.as_str()).unwrap_or("(task removed)");
                        let done = task.is_some_and(|t| t.completed);
                        let mut chip = RichText::new(format!("~{} {}", block.start_time.format("%H:%M"), title))
                            .font(theme.small_font())
                            .color(if done { p.text_faint } else { p.text_muted });
                        if done {
                            chip = chip.strikethrough();
                        }
                        if ui.selectable_label(false, chip).clicked() {
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

fn month_view(ui: &mut egui::Ui, theme: &Theme, db: &Database, state: &mut CalendarState, subjects: &[Subject]) {
    let p = &theme.palette;
    let today = Local::now().date_naive();
    let events = apply_subject_filter(queries::events::list_visible_events(&db.conn).unwrap_or_default(), state.subject_filter);

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
                let day_occurrences = by_day.get(&day);
                let count = day_occurrences.map(Vec::len).unwrap_or(0);
                // Feature request: "Colour" — Month view's cells are
                // too small to show each event's own title/subject
                // text the way Day and Week do, so instead this shows
                // one small dot per distinct subject scheduled that
                // day (first-seen order, capped at 6 so a day packed
                // with sessions can't overflow so small a cell).
                let subject_dots: Vec<egui::Color32> = day_occurrences
                    .map(|occs| {
                        let mut seen_ids = Vec::new();
                        for occ in occs {
                            if let Some(subject_id) = occ.event.subject_id {
                                if !seen_ids.contains(&subject_id) {
                                    seen_ids.push(subject_id);
                                }
                            }
                        }
                        seen_ids.into_iter().take(6).map(|id| crate::theme::subject_color(subjects, id)).collect()
                    })
                    .unwrap_or_default();

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
                        if !subject_dots.is_empty() {
                            ui.horizontal(|ui| {
                                for color in &subject_dots {
                                    let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                    ui.painter().circle_filled(dot_rect.center(), 3.0, *color);
                                }
                            });
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
            ui.checkbox(&mut edit.movable, "Movable: drag and resize it in the Day view, like a task (one-off events)");
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
            let _ = queries::events::set_event_movable(&db.conn, event_id, edit.movable);
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


    // --- Day timeline hour range / block geometry (time-blocking) -------------

    fn local_at(d: NaiveDate, h: u32, m: u32) -> chrono::DateTime<Local> {
        use chrono::TimeZone;
        Local.from_local_datetime(&d.and_hms_opt(h, m, 0).unwrap()).earliest().unwrap()
    }

    #[test]
    fn span_hours_are_the_start_and_end_hour_for_a_same_day_span() {
        let day = date(2026, 10, 14);
        assert_eq!(span_hours_on_day(local_at(day, 9, 30), local_at(day, 11, 15), day), (9, 11));
    }

    #[test]
    fn a_span_crossing_midnight_runs_to_the_last_hour_of_its_start_day() {
        let day = date(2026, 10, 14);
        let next = date(2026, 10, 15);
        assert_eq!(span_hours_on_day(local_at(day, 23, 0), local_at(next, 1, 0), day), (23, 23));
        assert_eq!(span_hours_on_day(local_at(day, 20, 0), local_at(next, 1, 0), day), (20, 23));
    }

    #[test]
    fn a_span_that_began_on_an_earlier_day_counts_from_midnight() {
        let day = date(2026, 10, 14);
        let before = date(2026, 10, 13);
        assert_eq!(span_hours_on_day(local_at(before, 22, 0), local_at(day, 2, 0), day), (0, 2));
    }

    #[test]
    fn an_inverted_span_never_ends_before_it_starts() {
        let day = date(2026, 10, 14);
        let (first, last) = span_hours_on_day(local_at(day, 15, 0), local_at(day, 9, 0), day);
        assert!(last >= first);
    }

    #[test]
    fn visible_range_pads_one_hour_above_and_closes_one_hour_below() {
        // The pre-existing behaviour for events alone: earliest = first - 1, latest = last + 1.
        assert_eq!(visible_hour_range(&[(9, 10), (13, 14)]), Some((8, 15)));
    }

    #[test]
    fn visible_range_is_clamped_to_the_day_and_never_inverted() {
        assert_eq!(visible_hour_range(&[(0, 0)]), Some((0, 1)));
        assert_eq!(visible_hour_range(&[(22, 23)]), Some((21, 23)));
        let (earliest, latest) = visible_hour_range(&[(23, 23)]).unwrap();
        assert!(latest >= earliest, "the timeline height is (latest - earliest + 1) and must not underflow");
    }

    #[test]
    fn visible_range_widens_to_cover_blocks_and_the_working_window() {
        // An event at 14:00 alone, then the working window 09:00-21:00 joins it.
        assert_eq!(visible_hour_range(&[(14, 15)]), Some((13, 16)));
        assert_eq!(visible_hour_range(&[(14, 15), (9, 20)]), Some((8, 21)));
    }

    #[test]
    fn visible_range_of_nothing_is_none() {
        assert_eq!(visible_hour_range(&[]), None);
    }

    // --- The "now" pointer ----------------------------------------------------

    fn time(h: u32, m: u32, sec: u32) -> chrono::NaiveTime {
        chrono::NaiveTime::from_hms_opt(h, m, sec).unwrap()
    }

    #[test]
    fn the_pointer_sits_on_the_hour_line_at_the_top_of_an_hour() {
        // Timeline starts at 08:00, top at y = 100.
        assert_eq!(now_pointer_y(time(8, 0, 0), 100.0, 8, 17), Some(100.0));
        assert_eq!(now_pointer_y(time(10, 0, 0), 100.0, 8, 17), Some(100.0 + 2.0 * PIXELS_PER_HOUR));
    }

    #[test]
    fn the_pointer_is_halfway_down_an_hour_at_half_past() {
        let y = now_pointer_y(time(9, 30, 0), 0.0, 8, 17).unwrap();
        assert!((y - 1.5 * PIXELS_PER_HOUR).abs() < 1e-3, "{y}");
    }

    #[test]
    fn the_pointer_moves_with_seconds_not_just_minutes() {
        let a = now_pointer_y(time(9, 0, 0), 0.0, 8, 17).unwrap();
        let b = now_pointer_y(time(9, 0, 30), 0.0, 8, 17).unwrap();
        let c = now_pointer_y(time(9, 1, 0), 0.0, 8, 17).unwrap();
        assert!(a < b && b < c, "{a} {b} {c}");
        assert!(((b - a) - (c - b)).abs() < 1e-3, "it moves smoothly");
    }

    #[test]
    fn the_pointer_shares_the_scale_that_events_are_drawn_on() {
        // An event starting 09:30 and the pointer at 09:30:00 land on the same y.
        let day = date(2026, 10, 14);
        let event_y = 50.0 + timeline_offset_minutes(local_at(day, 9, 30), 8);
        assert!((now_pointer_y(time(9, 30, 0), 50.0, 8, 17).unwrap() - event_y).abs() < 1e-3);
    }

    #[test]
    fn the_pointer_is_hidden_outside_the_hours_the_timeline_shows() {
        assert_eq!(now_pointer_y(time(7, 59, 59), 0.0, 8, 17), None, "before the first hour");
        assert_eq!(now_pointer_y(time(18, 0, 1), 0.0, 8, 17), None, "after the closing line");
        // The last hour runs to its closing line at 18:00 exactly.
        assert_eq!(now_pointer_y(time(18, 0, 0), 0.0, 8, 17), Some(10.0 * PIXELS_PER_HOUR));
        assert!(now_pointer_y(time(17, 59, 59), 0.0, 8, 17).is_some());
    }

    #[test]
    fn the_pointer_works_on_a_timeline_that_covers_the_whole_day() {
        assert_eq!(now_pointer_y(time(0, 0, 0), 0.0, 0, 23), Some(0.0));
        assert!(now_pointer_y(time(23, 59, 59), 0.0, 0, 23).is_some());
    }

    #[test]
    fn the_pointer_starts_left_of_the_hour_labels_and_runs_to_the_right_edge() {
        let rect = egui::Rect::from_min_size(egui::pos2(20.0, 0.0), egui::vec2(800.0, 400.0));
        let (left, right) = now_pointer_span(rect);
        assert!(left < rect.left(), "starts to the left of the hours");
        assert_eq!(left, rect.left() - NOW_POINTER_LEAD);
        assert_eq!(right, rect.right(), "finishes at the far edge of the calendar");
    }

    #[test]
    fn the_past_is_dimmed_down_to_the_pointer() {
        let y = now_pointer_y(time(9, 30, 0), 100.0, 8, 17).unwrap();
        assert_eq!(past_dim_bottom(time(9, 30, 0), 100.0, 8, 17), Some(y));
    }

    #[test]
    fn nothing_is_dimmed_before_the_first_shown_hour() {
        assert_eq!(past_dim_bottom(time(7, 0, 0), 100.0, 8, 17), None);
    }

    #[test]
    fn the_whole_timeline_is_dimmed_once_its_hours_are_over() {
        // 08:00 to 18:00 is ten hours tall.
        let bottom = past_dim_bottom(time(20, 0, 0), 100.0, 8, 17).unwrap();
        assert!((bottom - (100.0 + 10.0 * PIXELS_PER_HOUR)).abs() < 1e-3);
    }

    #[test]
    fn the_dim_is_the_background_at_the_chosen_opacity() {
        let bg = egui::Color32::from_rgb(10, 10, 10);
        let none = past_dim_color(bg, 0);
        let half = past_dim_color(bg, 50);
        let full = past_dim_color(bg, 100);
        assert_eq!(none.a(), 0);
        assert!((half.a() as i32 - 128).abs() <= 1, "{}", half.a());
        assert_eq!(full.a(), 255);
        let [r, g, b, _] = half.to_srgba_unmultiplied();
        assert!((r as i32 - 10).abs() <= 1 && (g as i32 - 10).abs() <= 1 && (b as i32 - 10).abs() <= 1, "same colour as the background");
        assert_eq!(past_dim_color(bg, 250).a(), 255, "over 100 is held to opaque");
    }

    #[test]
    fn the_pointer_label_includes_seconds() {
        assert_eq!(now_pointer_label(time(14, 3, 7)), "Now — 14:03:07");
        assert_eq!(now_pointer_label(time(0, 0, 0)), "Now — 00:00:00");
    }

    #[test]
    fn a_block_card_shares_the_event_scale_and_left_edge() {
        use crate::tasks::TimeBlock;
        let day = date(2026, 10, 14);
        let block = TimeBlock {
            id: 1,
            task_id: 1,
            start_time: local_at(day, 10, 30),
            end_time: local_at(day, 12, 0),
            locked: false,
            chunk_index: 0,
            status: crate::tasks::BlockStatus::Planned,
        };
        let timeline = egui::Rect::from_min_size(egui::pos2(0.0, 100.0), egui::vec2(600.0, 800.0));
        let (start, end) = block_minutes(&block);
        let rect = block_rect_minutes(timeline, 48.0, start, end, 9);

        // 10:30 is 1.5 hours below 09:00 on the shared 80px/hour scale.
        assert_eq!(rect.top(), 100.0 + 1.5 * PIXELS_PER_HOUR);
        assert_eq!(rect.height(), 1.5 * PIXELS_PER_HOUR);
        assert_eq!(rect.left(), 48.0 + Spacing::SM, "same left edge as an event card");
    }


    // --- Block drag / resize math (time-blocking Phase 2) ----------------------

    #[test]
    fn y_and_minutes_are_inverses_on_the_shared_scale() {
        // Timeline top at y=100, earliest hour 9: 9:00 is y=100, 10:30 is 1.5h lower.
        assert_eq!(y_to_minutes(100.0, 100.0, 9), 9 * 60);
        assert_eq!(y_to_minutes(100.0 + 1.5 * PIXELS_PER_HOUR, 100.0, 9), 10 * 60 + 30);
        // Round-trips the position a block card is drawn at.
        let top = 100.0 + (10 * 60 + 15 - 9 * 60) as f32 / 60.0 * PIXELS_PER_HOUR;
        assert_eq!(y_to_minutes(top, 100.0, 9), 10 * 60 + 15);
    }

    #[test]
    fn snapping_rounds_to_the_nearest_grid_step() {
        assert_eq!(snap_minutes(7, 15), 0);
        assert_eq!(snap_minutes(8, 15), 15);
        assert_eq!(snap_minutes(15, 15), 15);
        assert_eq!(snap_minutes(22, 15), 15);
        assert_eq!(snap_minutes(23, 15), 30);
        assert_eq!(snap_minutes(541, 5), 540);
        assert_eq!(snap_minutes(-4, 15), 0, "negative minutes still snap sanely");
        assert_eq!(snap_minutes(100, 0), 100, "a zero grid is treated as one minute");
    }

    #[test]
    fn a_moved_block_is_kept_inside_the_working_window_with_its_length() {
        // 60-minute block in a 09:00-21:00 window.
        assert_eq!(clamp_move(8 * 60, 60, 9 * 60, 21 * 60), 9 * 60, "pushed down from before the window");
        assert_eq!(clamp_move(20 * 60 + 30, 60, 9 * 60, 21 * 60), 20 * 60, "pulled up so its end fits");
        assert_eq!(clamp_move(12 * 60, 60, 9 * 60, 21 * 60), 12 * 60, "untouched inside");
        // A 13-hour block in a 09:00-18:00 window cannot fit: it sits at the start.
        assert_eq!(clamp_move(10 * 60, 13 * 60, 9 * 60, 18 * 60), 9 * 60);
    }

    #[test]
    fn a_resized_block_keeps_a_minimum_length_and_stays_in_the_window() {
        // Start 10:00, grid 15, window ends 21:00.
        assert_eq!(clamp_resize_end(600, 605, 15, 1260), 615, "can't shrink below one grid step");
        assert_eq!(clamp_resize_end(600, 500, 15, 1260), 615, "can't drag above the start");
        assert_eq!(clamp_resize_end(600, 700, 15, 1260), 700);
        assert_eq!(clamp_resize_end(600, 1400, 15, 1260), 1260, "can't run past the window");
        assert_eq!(clamp_resize_end(1255, 1300, 15, 1260), 1270, "the minimum wins even at the window's edge");
    }

    #[test]
    fn dropping_on_an_event_or_another_block_is_rejected_but_touching_is_fine() {
        use crate::tasks::time_blocking::Interval;
        let day = date(2026, 10, 14);
        let at = |h: u32, m: u32| day.and_hms_opt(h, m, 0).unwrap();
        let obstacles = [Interval::new(at(10, 0), at(11, 0))];

        assert!(drag_would_overlap(day, 10 * 60 + 30, 11 * 60 + 30, &obstacles), "overlaps the tail");
        assert!(drag_would_overlap(day, 9 * 60 + 30, 10 * 60 + 30, &obstacles), "overlaps the head");
        assert!(drag_would_overlap(day, 10 * 60 + 15, 10 * 60 + 45, &obstacles), "inside");
        assert!(!drag_would_overlap(day, 11 * 60, 12 * 60, &obstacles), "starting where it ends is fine");
        assert!(!drag_would_overlap(day, 9 * 60, 10 * 60, &obstacles), "ending where it starts is fine");
        assert!(!drag_would_overlap(day, 9 * 60, 10 * 60, &[]));
    }

    #[test]
    fn block_minutes_runs_past_midnight_without_wrapping() {
        use crate::tasks::TimeBlock;
        let day = date(2026, 10, 14);
        let block = TimeBlock {
            id: 1,
            task_id: 1,
            start_time: local_at(day, 23, 0),
            end_time: local_at(date(2026, 10, 15), 0, 30),
            locked: false,
            chunk_index: 0,
            status: crate::tasks::BlockStatus::Planned,
        };
        assert_eq!(block_minutes(&block), (23 * 60, 23 * 60 + 90));
    }


    // --- Card style shared by events and blocks ---------------------------------------

    fn log_entry(status: crate::events::SessionStatus, minutes: Option<i64>) -> crate::events::SessionLogEntry {
        crate::events::SessionLogEntry {
            id: 1,
            event_id: 1,
            occurrence_date: date(2026, 10, 14),
            status,
            actual_minutes: minutes,
        }
    }

    /// One stored event as an occurrence, optionally recurring daily.
    fn occurrence(db: &Database, title: &str, recurring: bool) -> DayOccurrence {
        let rule = recurring.then(|| {
            queries::recurrence::create_rule(
                &db.conn,
                &RecurrenceRule {
                    id: None,
                    frequency: Frequency::Daily,
                    interval: 1,
                    weekdays: Vec::new(),
                    end_date: None,
                    occurrence_limit: None,
                },
            )
            .expect("rule")
        });
        let day = date(2026, 10, 14);
        let event = queries::events::create_event(
            &db.conn,
            title,
            "Bring the red folder",
            local_at(day, 9, 0),
            local_at(day, 11, 0),
            None,
            rule,
        )
        .expect("event");
        occurrences_on_day(db, &[event], day).remove(0)
    }

    #[test]
    fn a_single_border_is_one_rectangle_and_a_double_border_is_two_nested() {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(300.0, 60.0));
        assert_eq!(outline_rects(rect, false), vec![rect]);

        let double = outline_rects(rect, true);
        assert_eq!(double.len(), 2);
        assert_eq!(double[0], rect);
        assert!(rect.contains_rect(double[1]), "the inner line is inside the outer");
        assert_eq!(double[1], rect.shrink(3.0));
    }

    #[test]
    fn a_card_too_short_for_two_lines_still_gets_a_sane_border() {
        let tiny = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 8.0));
        assert_eq!(outline_rects(tiny, true), vec![tiny], "no inner line that would cross itself");
        let narrow = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(8.0, 60.0));
        assert_eq!(outline_rects(narrow, true), vec![narrow]);
    }

    #[test]
    fn text_sits_further_in_when_the_border_is_double() {
        assert!(outline_inset(true) > outline_inset(false));
        assert!(outline_inset(true) > 3.0 + 1.0, "clear of the inner line (3px in)");
    }

    #[test]
    fn only_a_logged_completed_or_adjusted_session_is_finished() {
        use crate::events::SessionStatus::*;
        assert!(event_finished(Some(&log_entry(Completed, None))));
        assert!(event_finished(Some(&log_entry(Adjusted, Some(40)))));
        assert!(!event_finished(Some(&log_entry(Denied, None))), "not done is resolved, not finished");
        assert!(!event_finished(None), "an event merely in the past is not finished");
    }

    #[test]
    fn an_unfinished_one_off_event_has_no_notes() {
        let db = Database::open_in_memory().unwrap();
        let occ = occurrence(&db, "Lecture", false);
        assert!(event_card_notes(&occ, &[], None).is_empty());
    }

    #[test]
    fn a_repeating_event_notes_its_rule_and_a_task_count_and_not_done_is_noted() {
        let db = Database::open_in_memory().unwrap();
        let occ = occurrence(&db, "Lecture", true);
        let mut done = queries::tasks::create_task(&db.conn, "A", None, None, None).unwrap();
        let open = queries::tasks::create_task(&db.conn, "B", None, None, None).unwrap();
        done.completed = true;

        let notes = event_card_notes(&occ, &[done, open], Some(&log_entry(crate::events::SessionStatus::Denied, None)));

        assert!(notes[0].starts_with("↻ "), "{notes:?}");
        assert_eq!(notes[1], "1 / 2 tasks");
        assert_eq!(notes[2], "not done");
    }

    #[test]
    fn a_finished_event_is_blank_apart_from_its_name_and_times() {
        // Even a repeating event with tasks and a description drops every note once it is finished.
        let db = Database::open_in_memory().unwrap();
        let occ = occurrence(&db, "Lecture", true);
        let task = queries::tasks::create_task(&db.conn, "A", None, None, None).unwrap();
        let finished = log_entry(crate::events::SessionStatus::Completed, None);

        assert!(event_card_notes(&occ, &[task], Some(&finished)).is_empty());
    }

    #[test]
    fn the_time_range_marks_an_event_that_runs_past_midnight() {
        let db = Database::open_in_memory().unwrap();
        let day = date(2026, 10, 14);
        let late = queries::events::create_event(
            &db.conn,
            "Night shift",
            "",
            local_at(day, 23, 0),
            local_at(date(2026, 10, 15), 1, 0),
            None,
            None,
        )
        .unwrap();
        assert_eq!(event_time_range(&late), "23:00–01:00 (+1 day)");
        let normal = queries::events::create_event(&db.conn, "Lecture", "", local_at(day, 9, 0), local_at(day, 11, 0), None, None)
            .unwrap();
        assert_eq!(event_time_range(&normal), "09:00–11:00");
    }

    #[test]
    fn the_tooltip_carries_what_the_compact_card_no_longer_shows() {
        let db = Database::open_in_memory().unwrap();
        let occ = occurrence(&db, "Lecture", true);
        let tooltip = event_tooltip(&occ, &[], &[], Some(&log_entry(crate::events::SessionStatus::Adjusted, Some(95))));

        assert!(tooltip.starts_with("Lecture"));
        assert!(tooltip.contains("09:00–11:00 (2h)"), "{tooltip}");
        assert!(tooltip.contains("↻ "));
        assert!(tooltip.contains("Bring the red folder"), "the description lives here now");
        assert!(tooltip.contains("Logged: 95m"));
        assert!(tooltip.ends_with("Right-click for actions"), "so the hidden buttons can be found");
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

    fn assigned_task(id: i64, day: &str, auto: bool, generated_from: Option<i64>, minutes: Option<i64>) -> Task {
        Task {
            id,
            title: format!("Task {id}"),
            description: String::new(),
            completed: false,
            created_at: "2026-01-01T00:00:00+00:00".to_owned(),
            due_date: None,
            position: id,
            event_id: None,
            completed_at: None,
            subject_id: None,
            assigned_date: Some(day.to_owned()),
            repeat_weekly: false,
            parent_task_id: None,
            estimated_minutes: minutes,
            repeat_through_list: false,
            repeat_list_base_title: None,
            repeat_list_topics: None,
            repeat_list_mode: None,
            repeat_list_state: None,
            repeat_interval_days: 7,
            plan_topics_ahead: false,
            generated_from,
            auto_assigned: auto,
        }
    }

    #[test]
    fn split_assigned_separates_manual_from_planner_placed_tasks() {
        let tasks = vec![
            assigned_task(1, "2026-09-29", false, None, None),
            assigned_task(2, "2026-09-29", true, None, None),
            assigned_task(3, "2026-09-29", false, Some(9), None),
        ];
        let (manual, auto) = split_assigned(&tasks, "2026-09-29");
        assert_eq!(manual.iter().map(|t| t.id).collect::<Vec<_>>(), vec![1]);
        assert_eq!(auto.iter().map(|t| t.id).collect::<Vec<_>>(), vec![2, 3]);
    }

    #[test]
    fn split_assigned_ignores_other_days_and_unassigned_tasks() {
        let mut unassigned = assigned_task(3, "2026-09-29", true, None, None);
        unassigned.assigned_date = None;
        let tasks = vec![assigned_task(1, "2026-09-30", true, None, None), unassigned];
        let (manual, auto) = split_assigned(&tasks, "2026-09-29");
        assert!(manual.is_empty());
        assert!(auto.is_empty());
    }

    #[test]
    fn planned_minutes_uses_estimates_and_falls_back_to_the_default() {
        let tasks = vec![
            assigned_task(1, "2026-09-29", true, None, Some(45)),
            assigned_task(2, "2026-09-29", true, None, None),
            assigned_task(3, "2026-09-29", true, None, Some(0)),
        ];
        let refs: Vec<&Task> = tasks.iter().collect();
        assert_eq!(planned_minutes(&refs, 30), 45 + 30 + 30);
        assert_eq!(planned_minutes(&[], 30), 0);
    }
}

/// Headless render tests: real egui layout and input handling through
/// `Context::run_ui` (no GPU), the same approach as the Dashboard's
/// `paged_tests::Harness`. They exercise the Day view's time blocks end
/// to end — where a block is drawn, and what dragging, resizing and a
/// refused drop actually do to the database.
#[cfg(test)]
mod render_tests {
    use super::*;
    use chrono::TimeZone;
    use eframe::egui::{Pos2, Rect, Vec2};

    const SCREEN: Vec2 = Vec2::new(1100.0, 2400.0);

    fn test_day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 14).unwrap()
    }

    fn at(h: u32, m: u32) -> chrono::DateTime<Local> {
        Local.from_local_datetime(&test_day().and_hms_opt(h, m, 0).unwrap()).earliest().unwrap()
    }

    struct Harness {
        ctx: egui::Context,
        theme: Theme,
        db: Database,
        state: CalendarState,
        time: f64,
    }

    impl Harness {
        fn new() -> Self {
            let ctx = egui::Context::default();
            let theme = Theme::new();
            theme.apply(&ctx);
            let mut state = CalendarState::default();
            state.subview = CalendarSubview::Day;
            state.selected_day = test_day();
            Self { ctx, theme, db: Database::open_in_memory().unwrap(), state, time: 100.0 }
        }

        /// One frame on the test screen with the given input events.
        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 0.05;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let Self { ctx, theme, db, state, .. } = self;
            let mut output = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
                    show(ui, theme, Some(db), state);
                });
            });
            output.textures_delta.clear();
        }

        fn settle(&mut self) {
            for _ in 0..3 {
                self.frame(vec![]);
            }
        }

        fn timeline_top(&self) -> f32 {
            self.ctx
                .data(|d| d.get_temp::<Rect>(egui::Id::new("test_day_timeline_rect")))
                .expect("the Day view laid out a timeline")
                .top()
        }

        /// Where the block's card was laid out.
        fn block_rect(&self, block_id: i64) -> Rect {
            self.ctx
                .read_response(egui::Id::new(("time_block", block_id)))
                .unwrap_or_else(|| panic!("block {block_id} was not drawn"))
                .rect
        }

        fn press(&mut self, pos: Pos2) {
            self.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }

        fn drag_to(&mut self, from: Pos2, to: Pos2) {
            // Several small moves, as a real pointer makes, so egui registers a drag.
            for step in 1..=6 {
                let t = step as f32 / 6.0;
                self.frame(vec![egui::Event::PointerMoved(from + (to - from) * t)]);
            }
        }

        fn release(&mut self, pos: Pos2) {
            self.frame(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            self.settle();
        }

        /// A full press-drag-release.
        fn drag(&mut self, from: Pos2, to: Pos2) {
            self.press(from);
            self.drag_to(from, to);
            self.release(to);
        }

        fn task_with_block(&self, title: &str, start: (u32, u32), end: (u32, u32), locked_task_minutes: i64) -> (i64, i64) {
            let task = queries::tasks::create_task(&self.db.conn, title, None, None, None).expect("task").id;
            self.db
                .conn
                .execute(
                    "UPDATE tasks SET estimated_minutes = ?1 WHERE id = ?2",
                    rusqlite::params![locked_task_minutes, task],
                )
                .expect("estimate");
            let block = queries::task_blocks::insert_locked_block(
                &self.db.conn,
                task,
                at(start.0, start.1),
                at(end.0, end.1),
                0,
            )
            .expect("block");
            queries::task_blocks::set_block_locked(&self.db.conn, block.id, false).expect("make it an auto block");
            (task, block.id)
        }

        fn event(&self, start: (u32, u32), end: (u32, u32)) {
            queries::events::create_event(&self.db.conn, "Lecture", "", at(start.0, start.1), at(end.0, end.1), None, None)
                .expect("event");
        }

        fn stored(&self, block_id: i64) -> TimeBlock {
            queries::task_blocks::list_blocks_between(&self.db.conn, test_day(), test_day())
                .expect("list")
                .into_iter()
                .find(|b| b.id == block_id)
                .expect("block still exists")
        }
    }

    /// The y a given time is drawn at, from the laid-out timeline and the
    /// hour the timeline starts at.
    fn expected_y(timeline_top: f32, earliest_hour: u32, h: u32, m: u32) -> f32 {
        timeline_top + ((h * 60 + m) as f32 - earliest_hour as f32 * 60.0) / 60.0 * PIXELS_PER_HOUR
    }

    #[test]
    fn a_block_is_drawn_at_the_y_for_its_time_and_clear_of_the_event() {
        let mut h = Harness::new();
        h.event((10, 0), (11, 0));
        let (_, early) = h.task_with_block("Before", (9, 0), (10, 0), 60);
        let (_, late) = h.task_with_block("After", (11, 0), (12, 30), 90);
        h.settle();

        // Hours shown: first used hour 9 -> earliest 8.
        let top = h.timeline_top();
        let early_rect = h.block_rect(early);
        let late_rect = h.block_rect(late);
        assert!((early_rect.top() - expected_y(top, 8, 9, 0)).abs() < 0.6, "09:00 block at the 09:00 line");
        assert!((early_rect.height() - PIXELS_PER_HOUR).abs() < 0.6, "an hour is one hour tall");
        assert!((late_rect.top() - expected_y(top, 8, 11, 0)).abs() < 0.6);
        assert!((late_rect.height() - 1.5 * PIXELS_PER_HOUR).abs() < 0.6);

        // The event sits between them on the same scale: 10:00-11:00.
        let event_top = expected_y(top, 8, 10, 0);
        let event_bottom = expected_y(top, 8, 11, 0);
        assert!(early_rect.bottom() <= event_top + 0.6, "the early block ends where the event starts");
        assert!(late_rect.top() >= event_bottom - 0.6, "the late block starts where the event ends");
    }

    #[test]
    fn dragging_a_block_moves_it_snaps_to_the_grid_and_pins_it() {
        let mut h = Harness::new();
        h.event((10, 0), (11, 0));
        let (task, block) = h.task_with_block("Essay", (9, 0), (10, 0), 60);
        h.settle();
        let rect = h.block_rect(block);

        // Grab the middle of the block and drag it down 5 hours + a few pixels off the grid.
        let from = Pos2::new(rect.center().x, rect.top() + 20.0);
        let to = from + Vec2::new(0.0, 5.0 * PIXELS_PER_HOUR + 5.0);
        h.drag(from, to);

        let moved = h.stored(block);
        assert_eq!(moved.start_time, at(14, 0), "09:00 + 5h, snapped to the 15-minute grid");
        assert_eq!(moved.end_time, at(15, 0), "its length is unchanged");
        assert!(moved.locked, "a drag pins the block");
        let t = queries::tasks::list_tasks(&h.db.conn).unwrap().into_iter().find(|t| t.id == task).unwrap();
        assert_eq!(t.estimated_minutes, Some(60), "moving does not change the estimate");
    }

    #[test]
    fn dropping_a_block_onto_an_event_is_refused_and_nothing_changes() {
        let mut h = Harness::new();
        h.event((10, 0), (11, 0));
        let (_, block) = h.task_with_block("Essay", (13, 0), (14, 0), 60);
        h.settle();
        let rect = h.block_rect(block);

        // Drag it up onto the 10:00-11:00 event.
        let from = Pos2::new(rect.center().x, rect.top() + 20.0);
        let to = from - Vec2::new(0.0, 3.0 * PIXELS_PER_HOUR);
        h.drag(from, to);

        let after = h.stored(block);
        assert_eq!(after.start_time, at(13, 0));
        assert!(!after.locked, "a refused drop does not even pin it");
    }

    #[test]
    fn dragging_the_bottom_edge_resizes_and_pins_without_changing_the_estimate() {
        let mut h = Harness::new();
        let (task, block) = h.task_with_block("Essay", (9, 0), (10, 0), 60);
        h.settle();
        let rect = h.block_rect(block);

        // The 6px strip along the bottom edge; pull it down 45 minutes.
        let from = Pos2::new(rect.center().x, rect.bottom() - 2.0);
        let to = from + Vec2::new(0.0, 0.75 * PIXELS_PER_HOUR);
        h.drag(from, to);

        let resized = h.stored(block);
        assert_eq!(resized.start_time, at(9, 0), "the top stays put");
        assert_eq!(resized.end_time, at(10, 45), "an hour plus 45 minutes");
        assert!(resized.locked);
        let t = queries::tasks::list_tasks(&h.db.conn).unwrap().into_iter().find(|t| t.id == task).unwrap();
        assert_eq!(t.estimated_minutes, Some(60), "a longer block is not a longer task: the estimate stays");
    }

    #[test]
    fn a_resize_cannot_shrink_a_block_below_one_grid_step() {
        let mut h = Harness::new();
        let (_, block) = h.task_with_block("Essay", (9, 0), (10, 0), 60);
        h.settle();
        let rect = h.block_rect(block);

        let from = Pos2::new(rect.center().x, rect.bottom() - 2.0);
        let to = from - Vec2::new(0.0, 3.0 * PIXELS_PER_HOUR); // far above the block's own top
        h.drag(from, to);

        let shrunk = h.stored(block);
        assert_eq!(shrunk.start_time, at(9, 0));
        assert_eq!(shrunk.end_time, at(9, 15), "clamped to the 15-minute minimum");
    }

    #[test]
    fn a_click_without_dragging_selects_but_changes_nothing() {
        let mut h = Harness::new();
        let (_, block) = h.task_with_block("Essay", (9, 0), (10, 0), 60);
        h.settle();
        let rect = h.block_rect(block);
        let at_pos = Pos2::new(rect.center().x, rect.top() + 20.0);

        h.press(at_pos);
        h.release(at_pos);

        assert_eq!(h.state.selected_block, Some(block));
        let after = h.stored(block);
        assert_eq!(after.start_time, at(9, 0));
        assert!(!after.locked);
    }


    #[test]
    fn the_week_view_renders_with_blocks_when_time_blocking_is_on_and_without_it_off() {
        let mut h = Harness::new();
        h.state.subview = CalendarSubview::Week;
        let (_, _block) = h.task_with_block("Essay", (9, 0), (10, 0), 60);

        // Off: no block data is even looked at; the view just renders.
        h.settle();
        // On: the chip path runs (a panic or layout assert here would fail the test).
        queries::settings::set_time_blocking_enabled(&h.db.conn, true).expect("enable");
        h.settle();
    }

    #[test]
    fn the_day_view_task_rows_say_unscheduled_only_while_time_blocking_is_on() {
        // The pure rule behind the rows; the rendering of the row text is a plain label.
        let b = TimeBlock {
            id: 1,
            task_id: 1,
            start_time: at(9, 0),
            end_time: at(10, 0),
            locked: false,
            chunk_index: 0,
            status: crate::tasks::BlockStatus::Planned,
        };
        assert_eq!(crate::tasks::block::schedule_note(false, true, &[]), None);
        assert_eq!(crate::tasks::block::schedule_note(true, true, &[]).as_deref(), Some("unscheduled"));
        assert_eq!(crate::tasks::block::schedule_note(true, true, &[&b]).as_deref(), Some("09:00–10:00"));
    }


    #[test]
    fn a_worked_block_cannot_be_dragged_or_resized() {
        let mut h = Harness::new();
        let (_, block) = h.task_with_block("Essay", (9, 0), (10, 0), 60);
        queries::task_blocks::set_block_status(&h.db.conn, block, crate::tasks::BlockStatus::Done).unwrap();
        h.settle();
        let rect = h.block_rect(block);

        // Try to move it...
        let from = Pos2::new(rect.center().x, rect.top() + 20.0);
        h.drag(from, from + Vec2::new(0.0, 4.0 * PIXELS_PER_HOUR));
        // ...and to resize it.
        let rect = h.block_rect(block);
        let edge = Pos2::new(rect.center().x, rect.bottom() - 2.0);
        h.drag(edge, edge + Vec2::new(0.0, 2.0 * PIXELS_PER_HOUR));

        let after = h.stored(block);
        assert_eq!((after.start_time, after.end_time), (at(9, 0), at(10, 0)), "a record of work does not move");
        assert!(!after.locked);
    }

    #[test]
    fn a_block_in_progress_cannot_be_dragged_either() {
        let mut h = Harness::new();
        let (_, block) = h.task_with_block("Essay", (9, 0), (10, 0), 60);
        queries::task_blocks::set_block_status(&h.db.conn, block, crate::tasks::BlockStatus::InProgress).unwrap();
        h.settle();
        let rect = h.block_rect(block);

        let from = Pos2::new(rect.center().x, rect.top() + 20.0);
        h.drag(from, from + Vec2::new(0.0, 4.0 * PIXELS_PER_HOUR));

        assert_eq!(h.stored(block).start_time, at(9, 0));
    }

    #[test]
    fn a_worked_block_that_overran_its_plan_is_drawn_at_its_real_length() {
        // The stopwatch ran 09:00 -> 10:40; the block now records that, so it is 100 minutes tall.
        let mut h = Harness::new();
        let (_, block) = h.task_with_block("Essay", (9, 0), (10, 0), 60);
        queries::task_blocks::settle_block(
            &h.db.conn,
            block,
            at(9, 0),
            at(10, 40),
            crate::tasks::BlockStatus::Done,
        )
        .unwrap();
        h.settle();

        let rect = h.block_rect(block);
        assert!((rect.height() - 100.0 / 60.0 * PIXELS_PER_HOUR).abs() < 0.6, "{}", rect.height());
    }

    #[test]
    fn a_planned_block_can_still_be_dragged_next_to_a_worked_one_but_not_onto_it() {
        let mut h = Harness::new();
        let (_, worked) = h.task_with_block("Worked", (9, 0), (10, 0), 60);
        queries::task_blocks::set_block_status(&h.db.conn, worked, crate::tasks::BlockStatus::Done).unwrap();
        let (_, planned) = h.task_with_block("Planned", (13, 0), (14, 0), 60);
        h.settle();
        let rect = h.block_rect(planned);
        let from = Pos2::new(rect.center().x, rect.top() + 20.0);

        // Onto the worked block's time: refused.
        h.drag(from, from - Vec2::new(0.0, 4.0 * PIXELS_PER_HOUR));
        assert_eq!(h.stored(planned).start_time, at(13, 0));

        // To a free spot: allowed.
        let rect = h.block_rect(planned);
        let from = Pos2::new(rect.center().x, rect.top() + 20.0);
        h.drag(from, from + Vec2::new(0.0, 2.0 * PIXELS_PER_HOUR));
        assert_eq!(h.stored(planned).start_time, at(15, 0));
    }


    #[test]
    fn an_event_card_sits_on_the_same_scale_as_a_block() {
        let mut h = Harness::new();
        h.event((10, 0), (11, 30));
        let (_, block) = h.task_with_block("Essay", (13, 0), (14, 0), 60);
        h.settle();
        let event = queries::events::list_all_events(&h.db.conn).unwrap().remove(0);

        let card = h
            .ctx
            .read_response(egui::Id::new(("event_card", event.id, event.day())))
            .expect("the event card was drawn")
            .rect;
        let top = h.timeline_top();
        // Hours shown: first used hour 10 -> earliest 9.
        assert!((card.top() - expected_y(top, 9, 10, 0)).abs() < 0.6, "10:00 at the 10:00 line");
        assert!((card.height() - 1.5 * PIXELS_PER_HOUR).abs() < 0.6, "an hour and a half tall");
        assert_eq!(card.left(), h.block_rect(block).left(), "same left edge as a block");
        assert_eq!(card.width(), h.block_rect(block).width(), "same width as a block");
    }

    #[test]
    fn an_event_cannot_be_dragged() {
        let mut h = Harness::new();
        h.event((10, 0), (11, 0));
        h.settle();
        let event = queries::events::list_all_events(&h.db.conn).unwrap().remove(0);
        let rect = h
            .ctx
            .read_response(egui::Id::new(("event_card", event.id, event.day())))
            .expect("drawn")
            .rect;

        let from = Pos2::new(rect.center().x, rect.top() + 12.0);
        h.drag(from, from + Vec2::new(0.0, 3.0 * PIXELS_PER_HOUR));

        let after = queries::events::list_all_events(&h.db.conn).unwrap().remove(0);
        assert_eq!(after.start_time, at(10, 0), "events change through Edit, never by dragging");
    }

    #[test]
    fn right_clicking_an_event_opens_its_menu_without_changing_anything() {
        let mut h = Harness::new();
        h.event((10, 0), (11, 0));
        h.settle();
        let event = queries::events::list_all_events(&h.db.conn).unwrap().remove(0);
        let rect = h
            .ctx
            .read_response(egui::Id::new(("event_card", event.id, event.day())))
            .expect("drawn")
            .rect;
        let pos = rect.center();

        h.frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        h.frame(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        h.settle(); // the menu's layout runs without panicking

        let after = queries::events::list_all_events(&h.db.conn).unwrap().remove(0);
        assert_eq!((after.start_time, after.end_time), (at(10, 0), at(11, 0)));
    }

    #[test]
    fn events_and_blocks_of_every_kind_render_together_without_clipping_the_timeline() {
        // One of each: plain event, repeating (double border), logged-finished event,
        // planned block, worked block (double border), in-progress block.
        let mut h = Harness::new();
        h.event((8, 0), (9, 0));
        let rule = queries::recurrence::create_rule(
            &h.db.conn,
            &crate::events::RecurrenceRule {
                id: None,
                frequency: crate::events::Frequency::Daily,
                interval: 1,
                weekdays: Vec::new(),
                end_date: None,
                occurrence_limit: None,
            },
        )
        .unwrap();
        queries::events::create_event(&h.db.conn, "Weekly", "", at(9, 0), at(10, 0), None, Some(rule)).unwrap();
        let finished = queries::events::create_event(&h.db.conn, "Done session", "", at(10, 0), at(11, 0), None, None).unwrap();
        queries::session_log::log_session(
            &h.db.conn,
            finished.id,
            finished.day(),
            crate::events::SessionStatus::Completed,
            None,
        )
        .unwrap();
        let (_, planned) = h.task_with_block("Planned", (11, 0), (12, 0), 60);
        let (_, worked) = h.task_with_block("Worked", (12, 0), (13, 0), 60);
        queries::task_blocks::set_block_status(&h.db.conn, worked, crate::tasks::BlockStatus::Done).unwrap();
        let (_, running) = h.task_with_block("Running", (13, 0), (14, 0), 60);
        queries::task_blocks::set_block_status(&h.db.conn, running, crate::tasks::BlockStatus::InProgress).unwrap();

        h.settle();

        // Everything was laid out, one after another, none overlapping the next.
        let rects: Vec<Rect> = [planned, worked, running].iter().map(|id| h.block_rect(*id)).collect();
        for pair in rects.windows(2) {
            assert!(pair[0].bottom() <= pair[1].top() + 0.6, "{pair:?}");
        }
    }


    #[test]
    fn a_hidden_event_is_not_drawn_on_the_day_view_but_a_normal_one_is() {
        let mut h = Harness::new();
        h.event((10, 0), (11, 0));
        let maths = queries::subjects::create_subject(&h.db.conn, "Maths").expect("subject").id;
        // The event a linked timer run leaves behind for the stats.
        queries::pomodoro::record_focus_run(
            &h.db.conn,
            queries::pomodoro::FocusSource::Timer,
            "Timer focus session",
            Some(maths),
            25,
            25,
            at(14, 25),
            true,
        )
        .unwrap();
        h.settle();

        let all = queries::events::list_all_events(&h.db.conn).unwrap();
        assert_eq!(all.len(), 2);
        let drawn = |id: i64, day| h.ctx.read_response(egui::Id::new(("event_card", id, day))).is_some();
        let normal = all.iter().find(|e| e.title == "Lecture").unwrap();
        let hidden = all.iter().find(|e| e.title == "Timer focus session").unwrap();
        assert!(drawn(normal.id, normal.day()), "the person's own event is drawn");
        assert!(!drawn(hidden.id, hidden.day()), "the run's event is kept off the calendar");
        // Yet the time is registered.
        assert_eq!(crate::stats::minutes_per_subject_in_range(&h.db, test_day(), test_day())[&maths], 25);
    }

    #[test]
    fn a_day_with_only_a_hidden_event_reads_as_empty() {
        let mut h = Harness::new();
        let maths = queries::subjects::create_subject(&h.db.conn, "Maths").expect("subject").id;
        queries::pomodoro::record_focus_run(
            &h.db.conn,
            queries::pomodoro::FocusSource::Timer,
            "Timer focus session",
            Some(maths),
            25,
            25,
            at(14, 25),
            true,
        )
        .unwrap();
        h.settle();
        // No timeline at all: the Day view has nothing to draw.
        assert!(
            h.ctx.data(|d| d.get_temp::<Rect>(egui::Id::new("test_day_timeline_rect"))).is_none(),
            "a day whose only event is hidden shows 'Nothing scheduled.'"
        );
    }

    #[test]
    fn a_day_with_only_a_block_still_draws_the_timeline() {
        // Previously "Nothing scheduled." whenever there were no events.
        let mut h = Harness::new();
        let (_, block) = h.task_with_block("Essay", (9, 0), (10, 0), 60);
        h.settle();
        assert!(h.block_rect(block).height() > 0.0);
    }

    // --- The "now" pointer, through real layout ------------------------------

    fn today_at(h: u32, m: u32) -> chrono::DateTime<Local> {
        Local.from_local_datetime(&Local::now().date_naive().and_hms_opt(h, m, 0).unwrap()).earliest().unwrap()
    }

    /// A harness looking at today, with an event across the whole day so the
    /// timeline always covers the current time whenever the test runs.
    fn today_harness() -> Harness {
        let mut h = Harness::new();
        h.state.selected_day = Local::now().date_naive();
        queries::events::create_event(&h.db.conn, "All day", "", today_at(0, 0), today_at(23, 59), None, None)
            .expect("event");
        h
    }

    fn pointer_y(h: &Harness) -> Option<f32> {
        h.ctx.data(|d| d.get_temp::<f32>(egui::Id::new("test_now_pointer_y")))
    }

    #[test]
    fn the_now_pointer_is_drawn_on_todays_timeline_at_the_current_time() {
        let mut h = today_harness();
        h.settle();
        let y = pointer_y(&h).expect("the pointer was drawn");
        let before = Local::now().time();
        let expected = h.timeline_top() + before.num_seconds_from_midnight() as f32 / 3600.0 * PIXELS_PER_HOUR;
        // The timeline covers the whole day (earliest hour 0), and a few frames
        // took at most a moment: allow a couple of seconds of drift.
        assert!((y - expected).abs() < 0.2, "pointer at {y}, expected about {expected}");
    }

    #[test]
    fn the_now_pointer_is_not_drawn_on_another_day() {
        let mut h = today_harness();
        h.settle();
        assert!(pointer_y(&h).is_some());
        h.state.selected_day += ChronoDuration::days(1);
        queries::events::create_event(
            &h.db.conn,
            "Tomorrow",
            "",
            today_at(9, 0) + ChronoDuration::days(1),
            today_at(10, 0) + ChronoDuration::days(1),
            None,
            None,
        )
        .expect("event");
        h.settle();
        assert!(pointer_y(&h).is_none(), "no pointer on tomorrow");
    }


    fn dim_bottom(h: &Harness) -> Option<f32> {
        h.ctx.data(|d| d.get_temp::<f32>(egui::Id::new("test_past_dim_bottom")))
    }

    #[test]
    fn the_past_is_dimmed_down_to_the_now_line_on_todays_timeline() {
        let mut h = today_harness();
        h.settle();
        assert_eq!(dim_bottom(&h), pointer_y(&h), "the dim ends exactly at the line");
    }

    #[test]
    fn a_zero_percent_dim_turns_the_filter_off_but_keeps_the_line() {
        let mut h = today_harness();
        queries::settings::set_calendar_past_dim_percent(&h.db.conn, 0).expect("set");
        h.settle();
        assert!(dim_bottom(&h).is_none());
        assert!(pointer_y(&h).is_some());
    }

    #[test]
    fn another_day_has_no_dim() {
        let mut h = today_harness();
        h.settle();
        assert!(dim_bottom(&h).is_some());
        h.state.selected_day += ChronoDuration::days(1);
        queries::events::create_event(
            &h.db.conn,
            "Tomorrow",
            "",
            today_at(9, 0) + ChronoDuration::days(1),
            today_at(10, 0) + ChronoDuration::days(1),
            None,
            None,
        )
        .expect("event");
        h.settle();
        assert!(dim_bottom(&h).is_none());
    }

    #[test]
    fn the_now_pointer_asks_for_a_repaint_so_it_keeps_moving() {
        let mut h = today_harness();
        // egui reports the soonest requested repaint in the frame's output.
        h.time += 0.05;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
            time: Some(h.time),
            ..Default::default()
        };
        let Harness { ctx, theme, db, state, .. } = &mut h;
        let mut output = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
                show(ui, theme, Some(db), state);
            });
        });
        output.textures_delta.clear();
        let delay = output
            .viewport_output
            .values()
            .map(|v| v.repaint_delay)
            .min()
            .expect("a viewport");
        assert!(delay <= std::time::Duration::from_secs(1), "repaint within a second, got {delay:?}");
    }

    #[test]
    fn a_block_can_still_be_dragged_where_the_now_pointer_crosses_it() {
        // The pointer's hover target must not steal the grab from a block
        // underneath it. A block covering (nearly) the whole day always has the
        // pointer across it.
        let mut h = Harness::new();
        h.state.selected_day = Local::now().date_naive();
        let task = queries::tasks::create_task(&h.db.conn, "Essay", None, None, None).expect("task").id;
        let block = queries::task_blocks::insert_locked_block(&h.db.conn, task, today_at(0, 0), today_at(23, 59), 0)
            .expect("block");
        queries::task_blocks::set_block_locked(&h.db.conn, block.id, false).expect("auto block");
        h.settle();

        let y = pointer_y(&h).expect("pointer drawn");
        let rect = h.block_rect(block.id);
        // Grab exactly on the pointer line, at an x both the line and the block cover.
        let x = rect.left() + 20.0;
        let timeline = h
            .ctx
            .data(|d| d.get_temp::<Rect>(egui::Id::new("test_day_timeline_rect")))
            .expect("timeline laid out");
        assert!(x < now_pointer_span(timeline).1, "the grab point is under the pointer line's span");
        h.press(Pos2::new(x, y));
        h.drag_to(Pos2::new(x, y), Pos2::new(x, y + 40.0));
        h.release(Pos2::new(x, y + 40.0));

        let after = queries::task_blocks::list_blocks_between(&h.db.conn, Local::now().date_naive(), Local::now().date_naive())
            .unwrap()
            .into_iter()
            .find(|b| b.id == block.id)
            .unwrap();
        assert_ne!(after.start_time, today_at(0, 0), "the drag moved the block even under the pointer");
    }

    // --- dragging a task from "Tasks for this day" onto the timeline ------

    impl Harness {
        /// A task assigned to the test day, with an estimate.
        fn assigned_task(&self, title: &str, minutes: i64) -> i64 {
            let task = queries::tasks::create_task(&self.db.conn, title, None, None, None).expect("task").id;
            queries::tasks::set_assigned_date(&self.db.conn, task, Some(&test_day().format("%Y-%m-%d").to_string()))
                .expect("assign");
            self.db
                .conn
                .execute("UPDATE tasks SET estimated_minutes = ?1 WHERE id = ?2", rusqlite::params![minutes, task])
                .expect("estimate");
            task
        }

        fn grip(&self, task: i64) -> Option<Rect> {
            self.ctx.data(|d| d.get_temp::<Rect>(egui::Id::new(("test_task_grip", task))))
        }

        /// Presses the task's grip, drags it down off the task list so the drag
        /// begins, and returns where the pointer is.
        fn pick_up(&mut self, task: i64) -> Pos2 {
            let grip = self.grip(task).expect("the task row has a grip").center();
            self.press(grip);
            let lifted = grip + Vec2::new(0.0, 30.0);
            self.drag_to(grip, lifted);
            self.frame(vec![]);
            lifted
        }

        fn blocks_of_task(&self, task: i64) -> Vec<(chrono::NaiveDateTime, chrono::NaiveDateTime)> {
            queries::task_blocks::list_blocks_for_task(&self.db.conn, task)
                .expect("blocks")
                .iter()
                .map(|b| (b.start_time.naive_local(), b.end_time.naive_local()))
                .collect()
        }
    }

    fn naive(h: u32, m: u32) -> chrono::NaiveDateTime {
        test_day().and_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn dragging_a_task_onto_the_timeline_gives_it_a_block_of_its_full_length() {
        let mut h = Harness::new();
        let task = h.assigned_task("Essay", 60);
        h.settle();

        let from = h.pick_up(task);
        // The timeline is shown for the drop even though the day was empty (8:00 is its first line).
        let target = Pos2::new(500.0, expected_y(h.timeline_top(), 8, 14, 0));
        h.drag_to(from, target);
        h.release(target);

        assert_eq!(h.blocks_of_task(task), vec![(naive(14, 0), naive(15, 0))]);
        assert!(h.state.task_drag.is_none(), "the drag is over");
        let notice = h.state.drop_notice.clone().expect("the drop is reported").1;
        assert!(notice.contains("14:00–15:00"), "{notice}");
    }

    #[test]
    fn a_task_dropped_above_an_event_takes_the_gap_and_is_assumed_unfinished() {
        let mut h = Harness::new();
        h.event((15, 0), (16, 0));
        let task = h.assigned_task("Revision", 120);
        h.settle();

        let from = h.pick_up(task);
        // The 15:00 event makes the first hour shown 14:00, so that line is the timeline's top.
        let target = Pos2::new(500.0, expected_y(h.timeline_top(), 14, 14, 0) + 1.0);
        h.drag_to(from, target);
        h.release(target);

        assert_eq!(h.blocks_of_task(task), vec![(naive(14, 0), naive(15, 0))], "an hour fits before the event");
        let estimate = queries::tasks::list_tasks(&h.db.conn).unwrap().into_iter().find(|t| t.id == task).unwrap();
        assert_eq!(estimate.estimated_minutes, Some(120), "still two hours of work to do");
        let notice = h.state.drop_notice.clone().expect("reported").1;
        assert!(notice.contains("Only 60 of 120"), "{notice}");
    }

    #[test]
    fn releasing_a_task_away_from_the_timeline_places_nothing() {
        let mut h = Harness::new();
        let task = h.assigned_task("Essay", 60);
        h.settle();

        let from = h.pick_up(task);
        let above_everything = Pos2::new(500.0, 3.0);
        h.drag_to(from, above_everything);
        h.release(above_everything);

        assert!(h.blocks_of_task(task).is_empty());
        assert!(h.state.task_drag.is_none());
        assert!(h.state.drop_notice.is_none());
    }

    #[test]
    fn a_finished_task_has_no_grip_to_drag() {
        let mut h = Harness::new();
        let task = h.assigned_task("Done already", 30);
        queries::tasks::set_completed(&h.db.conn, task, true).unwrap();
        h.settle();
        assert!(h.grip(task).is_none());
    }

    #[test]
    fn a_task_with_time_logged_shows_a_bar_empty_partly_filled_or_overfilled_without_trouble() {
        let mut h = Harness::new();
        let empty = h.assigned_task("Untouched", 60);
        let partial = h.assigned_task("Under way", 60);
        let over = h.assigned_task("Overran", 60);
        let log = |task, minutes| {
            queries::task_time::log_task_time(
                &h.db.conn,
                task,
                test_day(),
                minutes,
                queries::pomodoro::FocusSource::Stopwatch,
            )
            .expect("log")
        };
        log(partial, 30);
        log(over, 150);
        h.settle();

        let progress = queries::task_time::progress_for_all(&h.db.conn).unwrap();
        assert_eq!(progress[&empty].spent, 0);
        assert!(!progress[&partial].is_over());
        assert!(progress[&over].is_over());
        for task in [empty, partial, over] {
            assert!(h.grip(task).is_some(), "the rows still draw, bar and all");
        }
    }

    // --- movable events ----------------------------------------------------

    impl Harness {
        fn movable_event(&self, title: &str, start: (u32, u32), end: (u32, u32), movable: bool) -> i64 {
            let id = queries::events::create_event(&self.db.conn, title, "", at(start.0, start.1), at(end.0, end.1), None, None)
                .expect("event")
                .id;
            queries::events::set_event_movable(&self.db.conn, id, movable).expect("movable");
            id
        }

        fn event_card_rect(&self, id: i64) -> Option<Rect> {
            self.ctx.read_response(egui::Id::new(("event_drag", id))).map(|r| r.rect)
        }

        fn event_times(&self, id: i64) -> (chrono::DateTime<Local>, chrono::DateTime<Local>) {
            let e = queries::events::list_all_events(&self.db.conn).unwrap().into_iter().find(|e| e.id == id).unwrap();
            (e.start_time, e.end_time)
        }
    }

    #[test]
    fn dragging_a_movable_event_moves_it_and_snaps_to_the_grid() {
        let mut h = Harness::new();
        let id = h.movable_event("Lecture", (10, 0), (11, 0), true);
        h.settle();
        let rect = h.event_card_rect(id).expect("a movable event has a drag area");

        let from = rect.center();
        h.drag(from, from + Vec2::new(0.0, 0.5 * PIXELS_PER_HOUR));

        assert_eq!(h.event_times(id), (at(10, 30), at(11, 30)), "moved half an hour, length unchanged");
    }

    #[test]
    fn dragging_the_bottom_edge_of_a_movable_event_resizes_it() {
        let mut h = Harness::new();
        let id = h.movable_event("Lecture", (10, 0), (11, 0), true);
        h.settle();
        let rect = h.event_card_rect(id).unwrap();

        let from = Pos2::new(rect.center().x, rect.bottom() - 2.0);
        h.drag(from, from + Vec2::new(0.0, 0.5 * PIXELS_PER_HOUR));

        assert_eq!(h.event_times(id), (at(10, 0), at(11, 30)), "the start stays, the end moves");
    }

    #[test]
    fn an_event_that_is_not_movable_cannot_be_dragged() {
        let mut h = Harness::new();
        let id = h.movable_event("Fixed", (10, 0), (11, 0), false);
        h.settle();
        assert!(h.event_card_rect(id).is_none(), "no drag area is made for it");
        assert_eq!(h.event_times(id), (at(10, 0), at(11, 0)));
    }

    #[test]
    fn a_movable_event_cannot_be_dropped_onto_another_event() {
        let mut h = Harness::new();
        let moving = h.movable_event("Moving", (10, 0), (11, 0), true);
        h.movable_event("In the way", (12, 0), (13, 0), false);
        h.settle();
        let rect = h.event_card_rect(moving).unwrap();

        let from = rect.center();
        h.drag(from, from + Vec2::new(0.0, 2.0 * PIXELS_PER_HOUR));

        assert_eq!(h.event_times(moving), (at(10, 0), at(11, 0)), "the drop is refused and nothing changes");
    }

    #[test]
    fn a_movable_event_cannot_be_dropped_onto_a_pinned_block() {
        let mut h = Harness::new();
        let moving = h.movable_event("Moving", (10, 0), (11, 0), true);
        let (_, block) = h.task_with_block("Pinned", (12, 0), (13, 0), 60);
        queries::task_blocks::set_block_locked(&h.db.conn, block, true).unwrap();
        h.settle();
        let rect = h.event_card_rect(moving).unwrap();

        let from = rect.center();
        h.drag(from, from + Vec2::new(0.0, 2.0 * PIXELS_PER_HOUR));

        assert_eq!(h.event_times(moving), (at(10, 0), at(11, 0)));
    }

    #[test]
    fn events_default_to_not_movable() {
        let h = Harness::new();
        let id = queries::events::create_event(&h.db.conn, "Plain", "", at(9, 0), at(10, 0), None, None).unwrap().id;
        let stored = queries::events::list_all_events(&h.db.conn).unwrap().into_iter().find(|e| e.id == id).unwrap();
        assert!(!stored.movable);
    }
}

