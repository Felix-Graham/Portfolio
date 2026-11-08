use eframe::egui::{self, RichText};

use crate::database::{queries, Database};
use crate::tasks::time_blocking::{self, WorkDay};
use crate::theme::{Spacing, Theme};
use crate::ui::components::{Input, Modal, RButton};

/// State for the global "Settings" modal. Notification preferences
/// are read fresh from the database each time the modal opens, then
/// held here as plain editable fields (mirroring how the Edit Task/
/// Event/Note modals hold in-progress edits) until Save writes them
/// back — so opening Settings, changing your mind, and closing
/// without saving doesn't touch anything.
pub struct SettingsModalState {
    pub open: bool,
    pub notifications_enabled: bool,
    pub task_reminders_enabled: bool,
    pub lead_minutes_input: String,
    /// Feature request: "Startup Scripts/Commands" — master switch,
    /// off by default (see `queries::settings::exec_commands_enabled`
    /// for why).
    pub exec_commands_enabled: bool,
    /// Feature request: "Toggle Switch In App Settings from Hyprctl
    /// Notify to Caelestia Notify".
    pub notification_backend: crate::notifications::NotificationBackend,
    /// Feature request: "Auto Assigning" — free-text minutes/days,
    /// parsed on save (same as `lead_minutes_input`).
    pub daily_task_minutes_input: String,
    pub default_task_minutes_input: String,
    pub plan_ahead_days_input: String,
    /// Feature request: "Time-blocking" — free-text like the fields
    /// above; the working hours are `HH:MM`. Parsed (and validated
    /// together) on save by `parse_time_blocking`.
    pub time_blocking_enabled: bool,
    pub auto_replan_blocks: bool,
    pub block_reminders: bool,
    pub work_start_input: String,
    pub work_end_input: String,
    /// Per-weekday hours (Monday first). A row that is neither off nor
    /// custom follows the default window above.
    pub weekdays: [WeekdayRow; 7],
    pub granularity_input: String,
    pub min_block_input: String,
    pub buffer_input: String,
    pub max_chunks_input: String,
    /// Feature request: tasks longer than this many minutes are planned as several blocks.
    pub split_over_input: String,
    /// Feature request: the longest block made when a task is split.
    pub max_block_input: String,
    /// How close (minutes) a timer or stopwatch must start to a block's start
    /// to count as starting that block.
    pub start_tolerance_input: String,
    /// Feature request: "Startup Questions" — whether launch opens with a
    /// flashcard quiz, and how many questions it asks (free text, parsed on save
    /// like the fields above).
    pub startup_quiz_enabled: bool,
    pub startup_quiz_questions_input: String,
    /// How strongly the Day view dims the past part of today (percent, 0 = off).
    pub past_dim_input: String,
    pub error: Option<String>,
}

impl Default for SettingsModalState {
    fn default() -> Self {
        Self {
            open: false,
            notifications_enabled: true,
            task_reminders_enabled: true,
            lead_minutes_input: "10".to_owned(),
            exec_commands_enabled: false,
            notification_backend: crate::notifications::NotificationBackend::default(),
            daily_task_minutes_input: crate::tasks::auto_assign::DEFAULT_DAILY_MINUTES.to_string(),
            default_task_minutes_input: crate::tasks::auto_assign::DEFAULT_TASK_MINUTES.to_string(),
            plan_ahead_days_input: crate::tasks::auto_assign::DEFAULT_HORIZON_DAYS.to_string(),
            time_blocking_enabled: false,
            auto_replan_blocks: false,
            block_reminders: true,
            work_start_input: format_clock(time_blocking::DEFAULT_WORK_START_MINUTES),
            work_end_input: format_clock(time_blocking::DEFAULT_WORK_END_MINUTES),
            weekdays: Default::default(),
            granularity_input: time_blocking::DEFAULT_GRANULARITY_MINUTES.to_string(),
            min_block_input: time_blocking::DEFAULT_MIN_BLOCK_MINUTES.to_string(),
            buffer_input: time_blocking::DEFAULT_BUFFER_MINUTES.to_string(),
            max_chunks_input: time_blocking::DEFAULT_MAX_CHUNKS.to_string(),
            split_over_input: time_blocking::DEFAULT_SPLIT_OVER_MINUTES.to_string(),
            max_block_input: time_blocking::DEFAULT_MAX_BLOCK_MINUTES.to_string(),
            start_tolerance_input: crate::tasks::block::DEFAULT_START_TOLERANCE_MINUTES.to_string(),
            startup_quiz_enabled: true,
            startup_quiz_questions_input: crate::flashcards::quiz::DEFAULT_QUESTIONS.to_string(),
            past_dim_input: queries::settings::DEFAULT_PAST_DIM_PERCENT.to_string(),
            error: None,
        }
    }
}

impl SettingsModalState {
    /// Opens the modal, loading current values from the database so
    /// it doesn't show stale defaults if settings were changed
    /// elsewhere (or on a previous run).
    pub fn open_from(&mut self, db: &Database) {
        let conn = &db.conn;
        self.notifications_enabled = queries::settings::notifications_enabled(conn).unwrap_or(true);
        self.task_reminders_enabled = queries::settings::task_reminders_enabled(conn).unwrap_or(true);
        self.lead_minutes_input = queries::settings::reminder_lead_minutes(conn).unwrap_or(10).to_string();
        self.exec_commands_enabled = queries::settings::exec_commands_enabled(conn).unwrap_or(false);
        self.notification_backend = queries::settings::notification_backend(conn).unwrap_or_default();
        self.daily_task_minutes_input = queries::settings::auto_assign_daily_minutes(conn)
            .unwrap_or(crate::tasks::auto_assign::DEFAULT_DAILY_MINUTES)
            .to_string();
        self.default_task_minutes_input = queries::settings::auto_assign_default_task_minutes(conn)
            .unwrap_or(crate::tasks::auto_assign::DEFAULT_TASK_MINUTES)
            .to_string();
        self.plan_ahead_days_input = queries::settings::auto_assign_horizon_days(conn)
            .unwrap_or(crate::tasks::auto_assign::DEFAULT_HORIZON_DAYS)
            .to_string();
        self.time_blocking_enabled = queries::settings::time_blocking_enabled(conn).unwrap_or(false);
        self.auto_replan_blocks = queries::settings::auto_replan_blocks(conn).unwrap_or(false);
        self.block_reminders = queries::settings::block_reminders_enabled(conn).unwrap_or(true);
        let (work_start, work_end) = queries::settings::work_window_minutes(conn)
            .unwrap_or((time_blocking::DEFAULT_WORK_START_MINUTES, time_blocking::DEFAULT_WORK_END_MINUTES));
        self.work_start_input = format_clock(work_start);
        self.work_end_input = format_clock(work_end);
        for (row, (weekday, _)) in self.weekdays.iter_mut().zip(WEEKDAYS) {
            *row = match queries::settings::weekday_override(conn, weekday).unwrap_or(None) {
                Some(WorkDay::Off) => WeekdayRow { off: true, ..WeekdayRow::default() },
                Some(WorkDay::Hours(start, end)) => {
                    WeekdayRow { off: false, custom: true, start: format_clock(start), end: format_clock(end) }
                }
                None => WeekdayRow::default(),
            };
        }
        self.granularity_input = queries::settings::slot_granularity_minutes(conn)
            .unwrap_or(time_blocking::DEFAULT_GRANULARITY_MINUTES)
            .to_string();
        self.min_block_input =
            queries::settings::min_block_minutes(conn).unwrap_or(time_blocking::DEFAULT_MIN_BLOCK_MINUTES).to_string();
        self.buffer_input =
            queries::settings::event_buffer_minutes(conn).unwrap_or(time_blocking::DEFAULT_BUFFER_MINUTES).to_string();
        self.max_chunks_input =
            queries::settings::max_chunks(conn).unwrap_or(time_blocking::DEFAULT_MAX_CHUNKS).to_string();
        self.split_over_input = queries::settings::split_over_minutes(conn)
            .unwrap_or(time_blocking::DEFAULT_SPLIT_OVER_MINUTES)
            .to_string();
        self.max_block_input =
            queries::settings::max_block_minutes(conn).unwrap_or(time_blocking::DEFAULT_MAX_BLOCK_MINUTES).to_string();
        self.start_tolerance_input = queries::settings::block_start_tolerance_minutes(conn)
            .unwrap_or(crate::tasks::block::DEFAULT_START_TOLERANCE_MINUTES)
            .to_string();
        self.startup_quiz_enabled = queries::settings::startup_quiz_enabled(conn).unwrap_or(true);
        self.startup_quiz_questions_input = queries::settings::startup_quiz_questions(conn)
            .unwrap_or(crate::flashcards::quiz::DEFAULT_QUESTIONS)
            .to_string();
        self.past_dim_input = queries::settings::calendar_past_dim_percent(conn)
            .unwrap_or(queries::settings::DEFAULT_PAST_DIM_PERCENT)
            .to_string();
        self.error = None;
        self.open = true;
    }
}

/// Minutes after midnight → `HH:MM` (what the working-hours fields show).
fn format_clock(minutes: i64) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// `HH:MM` (or `H:MM`) → minutes after midnight. `24:00` is allowed so
/// a working day can end at midnight; anything else out of range, or
/// not shaped like a time, is `None`.
fn parse_clock(text: &str) -> Option<i64> {
    let (hours, minutes) = text.trim().split_once(':')?;
    let hours: i64 = hours.trim().parse().ok()?;
    let minutes: i64 = minutes.trim().parse().ok()?;
    let total = hours * 60 + minutes;
    ((0..=23).contains(&hours) && (0..=59).contains(&minutes) || (hours == 24 && minutes == 0)).then_some(total)
}

/// One weekday's row in the "Different hours on some days" section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeekdayRow {
    /// A day off: nothing is planned and Auto-assign gives it no work.
    pub off: bool,
    /// Hours of its own (`start`/`end` below); otherwise it follows the
    /// default. Ignored while `off`.
    pub custom: bool,
    pub start: String,
    pub end: String,
}

impl Default for WeekdayRow {
    fn default() -> Self {
        Self {
            off: false,
            custom: false,
            start: format_clock(time_blocking::DEFAULT_WORK_START_MINUTES),
            end: format_clock(time_blocking::DEFAULT_WORK_END_MINUTES),
        }
    }
}

const WEEKDAYS: [(chrono::Weekday, &str); 7] = [
    (chrono::Weekday::Mon, "Monday"),
    (chrono::Weekday::Tue, "Tuesday"),
    (chrono::Weekday::Wed, "Wednesday"),
    (chrono::Weekday::Thu, "Thursday"),
    (chrono::Weekday::Fri, "Friday"),
    (chrono::Weekday::Sat, "Saturday"),
    (chrono::Weekday::Sun, "Sunday"),
];

/// How strongly the Day view dims the past: a whole percentage from 0 (off) up
/// to the cap.
fn parse_past_dim(text: &str) -> Option<u8> {
    text.trim().parse::<u8>().ok().filter(|n| *n <= queries::settings::MAX_PAST_DIM_PERCENT)
}

/// The startup quiz's question count: a whole number from 1 up to the cap.
fn parse_question_count(text: &str) -> Option<usize> {
    text.trim().parse::<usize>().ok().filter(|n| (1..=crate::flashcards::quiz::MAX_QUESTIONS).contains(n))
}

/// The Time-blocking section's fields, parsed and validated together.
#[derive(Debug, PartialEq, Eq)]
struct TimeBlockingValues {
    enabled: bool,
    auto_replan: bool,
    block_reminders: bool,
    work_start: i64,
    work_end: i64,
    /// What to store for each weekday (Monday first): `None` follows the default.
    weekdays: [Option<WorkDay>; 7],
    granularity: i64,
    min_block: i64,
    buffer: i64,
    max_chunks: usize,
    split_over: i64,
    max_block: i64,
    start_tolerance: i64,
}

/// Validates every Time-blocking field at once, so Save either writes
/// all of them or none (the error says which field to fix).
fn parse_time_blocking(state: &SettingsModalState) -> Result<TimeBlockingValues, String> {
    let (Some(work_start), Some(work_end)) = (parse_clock(&state.work_start_input), parse_clock(&state.work_end_input))
    else {
        return Err("Working hours must be times like 09:00 and 21:00.".to_owned());
    };
    if work_start >= work_end {
        return Err("Working hours must end after they start.".to_owned());
    }
    let mut weekdays: [Option<WorkDay>; 7] = [None; 7];
    for ((slot, row), (_, name)) in weekdays.iter_mut().zip(&state.weekdays).zip(WEEKDAYS) {
        if row.off {
            *slot = Some(WorkDay::Off);
        } else if row.custom {
            let (Some(start), Some(end)) = (parse_clock(&row.start), parse_clock(&row.end)) else {
                return Err(format!("{name}'s hours must be times like 09:00 and 21:00."));
            };
            if start >= end {
                return Err(format!("{name}'s hours must end after they start."));
            }
            *slot = Some(WorkDay::Hours(start, end));
        }
    }
    let granularity = state.granularity_input.trim().parse::<i64>().ok().filter(|m| (1..=60).contains(m));
    let Some(granularity) = granularity else {
        return Err("Times must snap to a whole number of minutes between 1 and 60.".to_owned());
    };
    let Some(min_block) = state.min_block_input.trim().parse::<i64>().ok().filter(|m| *m >= 1) else {
        return Err("The shortest usable gap must be a positive whole number of minutes.".to_owned());
    };
    let Some(buffer) = state.buffer_input.trim().parse::<i64>().ok().filter(|m| *m >= 0) else {
        return Err("The gap around events must be a whole number of minutes (0 for none).".to_owned());
    };
    let Some(max_chunks) = state.max_chunks_input.trim().parse::<usize>().ok().filter(|m| *m >= 1) else {
        return Err("A task must be split into at least 1 piece (1 means never split).".to_owned());
    };
    let Some(split_over) = state.split_over_input.trim().parse::<i64>().ok().filter(|m| *m >= 1) else {
        return Err("Tasks are split above a positive whole number of minutes (30, say).".to_owned());
    };
    let Some(max_block) = state.max_block_input.trim().parse::<i64>().ok().filter(|m| *m >= 1) else {
        return Err("The longest block must be a positive whole number of minutes.".to_owned());
    };
    if max_block < min_block {
        return Err("The longest block can't be shorter than the shortest block.".to_owned());
    }
    let Some(start_tolerance) = state.start_tolerance_input.trim().parse::<i64>().ok().filter(|m| (1..=240).contains(m))
    else {
        return Err("\"Near a block\" must be a whole number of minutes between 1 and 240.".to_owned());
    };
    Ok(TimeBlockingValues {
        enabled: state.time_blocking_enabled,
        auto_replan: state.auto_replan_blocks,
        block_reminders: state.block_reminders,
        work_start,
        work_end,
        weekdays,
        granularity,
        min_block,
        buffer,
        max_chunks,
        split_over,
        max_block,
        start_tolerance,
    })
}

pub fn show(ctx: &egui::Context, theme: &Theme, db: Option<&Database>, state: &mut SettingsModalState) {
    if !state.open {
        return;
    }
    let Some(db) = db else {
        return;
    };

    let p = &theme.palette;
    let mut modal_open = true;
    let mut save_requested = false;
    let mut close_requested = false;

    Modal::new("settings_modal", "Settings").show(ctx, theme, &mut modal_open, |ui| {
        ui.label(RichText::new("Notifications").font(theme.small_font()).color(p.text_faint));
        ui.add_space(Spacing::XS);

        ui.checkbox(&mut state.notifications_enabled, "Enable session reminders");
        ui.add_space(Spacing::XS);

        ui.add_enabled_ui(state.notifications_enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Remind me").font(theme.small_font()).color(p.text_faint));
                Input::new(&mut state.lead_minutes_input, "10").width(48.0).show(ui, theme);
                ui.label(
                    RichText::new("minutes before a session starts").font(theme.small_font()).color(p.text_faint),
                );
            });
        });
        ui.add_space(Spacing::SM);

        ui.checkbox(&mut state.task_reminders_enabled, "Remind me about tasks due today");

        ui.label(
            RichText::new(
                "Reminders only fire while Revise is open, unless the optional revise-reminderd \
                 background daemon is set up separately (see packaging/README.md) — it isn't \
                 installed or running by default.",
            )
            .font(theme.small_font())
            .color(p.text_faint),
        );
        ui.add_space(Spacing::SM);

        // Feature request: "Toggle Switch In App Settings from
        // Hyprctl Notify to Caelestia Notify".
        ui.label(RichText::new("Notification backend").font(theme.small_font()).color(p.text_faint));
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut state.notification_backend,
                crate::notifications::NotificationBackend::Hyprctl,
                "Hyprctl",
            );
            ui.selectable_value(
                &mut state.notification_backend,
                crate::notifications::NotificationBackend::Caelestia,
                "Caelestia",
            );
        });
        ui.label(
            RichText::new(
                "Hyprctl uses Hyprland's own built-in notification popup. Caelestia sends a \
                 standard desktop notification instead (via notify-send), which caelestia-shell \
                 picks up and renders in its own style — requires libnotify and caelestia-shell \
                 to actually be running.",
            )
            .font(theme.small_font())
            .color(p.text_faint),
        );

        ui.add_space(Spacing::MD);
        ui.separator();
        ui.add_space(Spacing::SM);
        // Feature request: "Auto Assigning" — the knobs behind the
        // To Do view's Auto-assign button.
        ui.label(RichText::new("Auto-assign").font(theme.small_font()).color(p.text_faint));
        ui.add_space(Spacing::XS);
        ui.horizontal(|ui| {
            ui.label(RichText::new("A day should hold").font(theme.small_font()).color(p.text_faint));
            Input::new(&mut state.daily_task_minutes_input, "120").width(48.0).show(ui, theme);
            ui.label(RichText::new("minutes of tasks").font(theme.small_font()).color(p.text_faint));
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("A task with no estimate takes").font(theme.small_font()).color(p.text_faint));
            Input::new(&mut state.default_task_minutes_input, "30").width(48.0).show(ui, theme);
            ui.label(RichText::new("minutes").font(theme.small_font()).color(p.text_faint));
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Plan the next").font(theme.small_font()).color(p.text_faint));
            Input::new(&mut state.plan_ahead_days_input, "14").width(48.0).show(ui, theme);
            ui.label(RichText::new("days").font(theme.small_font()).color(p.text_faint));
        });

        ui.add_space(Spacing::MD);
        ui.separator();
        ui.add_space(Spacing::SM);
        // Feature request: "Time-blocking" — the knobs behind the To Do
        // view's "Plan times" button and the blocks drawn in the Day view.
        ui.label(RichText::new("Time blocking").font(theme.small_font()).color(p.text_faint));
        ui.add_space(Spacing::XS);
        ui.checkbox(&mut state.time_blocking_enabled, "Place each day's tasks at times between my events");
        ui.add_enabled_ui(state.time_blocking_enabled, |ui| {
            ui.checkbox(&mut state.auto_replan_blocks, "Re-plan times automatically at launch and each new day");
            ui.checkbox(&mut state.block_reminders, "Notify me shortly before a planned block starts");
            ui.horizontal(|ui| {
                ui.label(RichText::new("Working hours").font(theme.small_font()).color(p.text_faint));
                Input::new(&mut state.work_start_input, "09:00").width(56.0).show(ui, theme);
                ui.label(RichText::new("to").font(theme.small_font()).color(p.text_faint));
                Input::new(&mut state.work_end_input, "21:00").width(56.0).show(ui, theme);
            });
            egui::CollapsingHeader::new(
                RichText::new("Different hours on some days").font(theme.small_font()).color(p.text_faint),
            )
            .default_open(state.weekdays.iter().any(|row| row.off || row.custom))
            .show(ui, |ui| {
                for (row, (_, name)) in state.weekdays.iter_mut().zip(WEEKDAYS) {
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [84.0, 18.0],
                            egui::Label::new(RichText::new(name).font(theme.small_font()).color(p.text_faint)),
                        );
                        ui.checkbox(&mut row.off, "Day off");
                        ui.add_enabled_ui(!row.off, |ui| {
                            ui.checkbox(&mut row.custom, "Own hours");
                            ui.add_enabled_ui(row.custom, |ui| {
                                Input::new(&mut row.start, "09:00").width(56.0).show(ui, theme);
                                ui.label(RichText::new("to").font(theme.small_font()).color(p.text_faint));
                                Input::new(&mut row.end, "21:00").width(56.0).show(ui, theme);
                            });
                        });
                    });
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Snap times to").font(theme.small_font()).color(p.text_faint));
                Input::new(&mut state.granularity_input, "15").width(48.0).show(ui, theme);
                ui.label(RichText::new("minutes").font(theme.small_font()).color(p.text_faint));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Shortest block").font(theme.small_font()).color(p.text_faint));
                Input::new(&mut state.min_block_input, "25").width(48.0).show(ui, theme);
                ui.label(RichText::new("minutes (shorter gaps are skipped)").font(theme.small_font()).color(p.text_faint));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Longest block").font(theme.small_font()).color(p.text_faint));
                Input::new(&mut state.max_block_input, "60").width(48.0).show(ui, theme);
                ui.label(RichText::new("minutes").font(theme.small_font()).color(p.text_faint));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Split tasks longer than").font(theme.small_font()).color(p.text_faint));
                Input::new(&mut state.split_over_input, "30").width(48.0).show(ui, theme);
                ui.label(RichText::new("minutes into several blocks").font(theme.small_font()).color(p.text_faint));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Keep").font(theme.small_font()).color(p.text_faint));
                Input::new(&mut state.buffer_input, "0").width(48.0).show(ui, theme);
                ui.label(RichText::new("minutes clear around events").font(theme.small_font()).color(p.text_faint));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("A timer or stopwatch started within").font(theme.small_font()).color(p.text_faint));
                Input::new(&mut state.start_tolerance_input, "15").width(48.0).show(ui, theme);
                ui.label(RichText::new("minutes of a block starts it").font(theme.small_font()).color(p.text_faint));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Split a task into at most").font(theme.small_font()).color(p.text_faint));
                Input::new(&mut state.max_chunks_input, "1").width(48.0).show(ui, theme);
                ui.label(RichText::new("pieces (1 = never split)").font(theme.small_font()).color(p.text_faint));
            });
        });
        ui.label(
            RichText::new(
                "Adds a Plan times button to the To Do view, and makes Auto-assign plan times too \
                 and leave room for your events. Re-planning never moves a pinned block.",
            )
            .font(theme.small_font())
            .color(p.text_faint),
        );

        ui.add_space(Spacing::MD);
        ui.separator();
        ui.add_space(Spacing::SM);
        ui.label(RichText::new("Calendar").font(theme.small_font()).color(p.text_faint));
        ui.add_space(Spacing::XS);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Dim the past on today's timeline by").font(theme.small_font()).color(p.text_faint));
            Input::new(&mut state.past_dim_input, "40").width(48.0).show(ui, theme);
            ui.label(
                RichText::new(format!("% (0 = off, up to {})", queries::settings::MAX_PAST_DIM_PERCENT))
                    .font(theme.small_font())
                    .color(p.text_faint),
            );
        });
        ui.label(
            RichText::new(
                "Everything above the white current-time line in the Day view fades towards the \
                 background. Higher is dimmer.",
            )
            .font(theme.small_font())
            .color(p.text_faint),
        );

        ui.add_space(Spacing::MD);
        ui.separator();
        ui.add_space(Spacing::SM);
        ui.label(RichText::new("Startup questions").font(theme.small_font()).color(p.text_faint));
        ui.add_space(Spacing::XS);
        ui.checkbox(&mut state.startup_quiz_enabled, "Open Revise with a few flashcard questions");
        ui.add_enabled_ui(state.startup_quiz_enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Ask").font(theme.small_font()).color(p.text_faint));
                Input::new(&mut state.startup_quiz_questions_input, "5").width(48.0).show(ui, theme);
                ui.label(
                    RichText::new(format!("questions (1–{})", crate::flashcards::quiz::MAX_QUESTIONS))
                        .font(theme.small_font())
                        .color(p.text_faint),
                );
            });
        });
        ui.label(
            RichText::new(
                "Multiple choice, drawn at random from your flashcard sets and decks, full-screen \
                 at launch, with a Skip button. Nothing is asked if you have no flashcards yet.",
            )
            .font(theme.small_font())
            .color(p.text_faint),
        );

        ui.add_space(Spacing::MD);
        ui.separator();
        ui.add_space(Spacing::SM);
        ui.label(RichText::new("Startup Commands").font(theme.small_font()).color(p.text_faint));
        ui.add_space(Spacing::XS);
        ui.checkbox(&mut state.exec_commands_enabled, "Run an event's startup command at its start time");
        ui.label(
            RichText::new(
                "Off by default. When on, Revise runs whatever shell command is set on an \
                 event (e.g. opening a browser) automatically at its start time, with no \
                 further confirmation — only enable this if you trust everything you (or \
                 anyone with access to this database) put in an event's startup command.",
            )
            .font(theme.small_font())
            .color(p.text_faint),
        );

        if let Some(error) = &state.error {
            ui.add_space(Spacing::SM);
            ui.label(RichText::new(error).font(theme.small_font()).color(p.text));
        }

        ui.add_space(Spacing::MD);
        ui.horizontal(|ui| {
            if RButton::new("Save").primary().show(ui, theme).clicked() {
                save_requested = true;
            }
            if RButton::new("Close").show(ui, theme).clicked() {
                close_requested = true;
            }
        });
    });

    if save_requested {
        let lead = state.lead_minutes_input.trim().parse::<i64>().ok().filter(|m| *m >= 1);
        let daily = state.daily_task_minutes_input.trim().parse::<i64>().ok().filter(|m| *m >= 1);
        let default_task = state.default_task_minutes_input.trim().parse::<i64>().ok().filter(|m| *m >= 1);
        let plan_ahead = state.plan_ahead_days_input.trim().parse::<i64>().ok().filter(|m| *m >= 1);
        let blocking = parse_time_blocking(state);
        let quiz_questions = parse_question_count(&state.startup_quiz_questions_input);
        let past_dim = parse_past_dim(&state.past_dim_input);
        match (lead, daily, default_task, plan_ahead) {
            (Some(_), Some(_), Some(_), Some(_)) if past_dim.is_none() => {
                state.error = Some(format!(
                    "The calendar's past dim must be a whole percentage from 0 to {}.",
                    queries::settings::MAX_PAST_DIM_PERCENT
                ));
            }
            (Some(_), Some(_), Some(_), Some(_)) if quiz_questions.is_none() => {
                state.error = Some(format!(
                    "Startup questions must be a whole number from 1 to {}.",
                    crate::flashcards::quiz::MAX_QUESTIONS
                ));
            }
            (Some(_), Some(_), Some(_), Some(_)) if blocking.is_err() => {
                state.error = blocking.err();
            }
            (Some(minutes), Some(daily), Some(default_task), Some(plan_ahead)) => {
                let conn = &db.conn;
                if let Ok(b) = &blocking {
                    let _ = queries::settings::set_time_blocking_enabled(conn, b.enabled);
                    let _ = queries::settings::set_auto_replan_blocks(conn, b.auto_replan);
                    let _ = queries::settings::set_block_reminders_enabled(conn, b.block_reminders);
                    let _ = queries::settings::set_work_window_minutes(conn, b.work_start, b.work_end);
                    for ((weekday, _), work_day) in WEEKDAYS.iter().zip(b.weekdays) {
                        let _ = queries::settings::set_weekday_override(conn, *weekday, work_day);
                    }
                    let _ = queries::settings::set_slot_granularity_minutes(conn, b.granularity);
                    let _ = queries::settings::set_min_block_minutes(conn, b.min_block);
                    let _ = queries::settings::set_event_buffer_minutes(conn, b.buffer);
                    let _ = queries::settings::set_max_chunks(conn, b.max_chunks);
                    let _ = queries::settings::set_split_over_minutes(conn, b.split_over);
                    let _ = queries::settings::set_max_block_minutes(conn, b.max_block);
                    let _ = queries::settings::set_block_start_tolerance_minutes(conn, b.start_tolerance);
                }
                let _ = queries::settings::set_notifications_enabled(conn, state.notifications_enabled);
                let _ = queries::settings::set_task_reminders_enabled(conn, state.task_reminders_enabled);
                let _ = queries::settings::set_reminder_lead_minutes(conn, minutes);
                let _ = queries::settings::set_exec_commands_enabled(conn, state.exec_commands_enabled);
                let _ = queries::settings::set_notification_backend(conn, state.notification_backend);
                let _ = queries::settings::set_auto_assign_daily_minutes(conn, daily);
                let _ = queries::settings::set_auto_assign_default_task_minutes(conn, default_task);
                let _ = queries::settings::set_auto_assign_horizon_days(conn, plan_ahead);
                let _ = queries::settings::set_startup_quiz_enabled(conn, state.startup_quiz_enabled);
                if let Some(percent) = past_dim {
                    let _ = queries::settings::set_calendar_past_dim_percent(conn, percent);
                }
                if let Some(questions) = quiz_questions {
                    let _ = queries::settings::set_startup_quiz_questions(conn, questions);
                }
                state.error = None;
                state.open = false;
            }
            _ => {
                state.error = Some(
                    "Lead time, the daily task time, the default task length and the planning window must \
                     each be a positive whole number."
                        .to_owned(),
                );
            }
        }
        return;
    }

    if !modal_open || close_requested {
        state.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_text_round_trips() {
        for minutes in [0, 5, 540, 725, 1260, 1439] {
            assert_eq!(parse_clock(&format_clock(minutes)), Some(minutes));
        }
        assert_eq!(format_clock(540), "09:00");
        assert_eq!(format_clock(1260), "21:00");
    }

    #[test]
    fn parse_clock_accepts_loose_but_sensible_times() {
        assert_eq!(parse_clock("9:00"), Some(540));
        assert_eq!(parse_clock(" 09:30 "), Some(570));
        assert_eq!(parse_clock("24:00"), Some(1440), "a working day may end at midnight");
        assert_eq!(parse_clock("0:00"), Some(0));
    }

    #[test]
    fn parse_clock_rejects_anything_that_is_not_a_time() {
        for bad in ["", "9", "9:5x", "25:00", "24:30", "12:60", "-1:00", "ab:cd", "9:00:00", ":30"] {
            assert_eq!(parse_clock(bad), None, "{bad:?} should not parse");
        }
    }

    fn valid_state() -> SettingsModalState {
        SettingsModalState { time_blocking_enabled: true, ..SettingsModalState::default() }
    }

    #[test]
    fn the_defaults_validate_to_the_default_values() {
        let values = parse_time_blocking(&valid_state()).expect("defaults are valid");
        assert_eq!(
            values,
            TimeBlockingValues {
                enabled: true,
                auto_replan: false,
                block_reminders: true,
                work_start: 540,
                work_end: 1260,
                weekdays: [None; 7],
                granularity: 15,
                min_block: 25,
                buffer: 0,
                max_chunks: 6,
                split_over: 30,
                max_block: 60,
                start_tolerance: 15,
            }
        );
    }

    #[test]
    fn a_working_day_must_end_after_it_starts_and_be_real_times() {
        let mut state = valid_state();
        state.work_end_input = "09:00".to_owned();
        assert!(parse_time_blocking(&state).unwrap_err().contains("end after"));

        state.work_end_input = "later".to_owned();
        assert!(parse_time_blocking(&state).unwrap_err().contains("times like"));
    }

    #[test]
    fn each_number_field_is_validated_with_its_own_message() {
        let cases: [(&str, fn(&mut SettingsModalState)); 7] = [
            ("snap", |s| s.granularity_input = "0".to_owned()),
            ("shortest", |s| s.min_block_input = "-3".to_owned()),
            ("around events", |s| s.buffer_input = "-1".to_owned()),
            ("split", |s| s.max_chunks_input = "0".to_owned()),
            ("split above", |s| s.split_over_input = "0".to_owned()),
            ("longest", |s| s.max_block_input = "soon".to_owned()),
            ("longest", |s| s.max_block_input = "10".to_owned()), // shorter than the 25-minute shortest
        ];
        for (needle, mutate) in cases {
            let mut state = valid_state();
            mutate(&mut state);
            let message = parse_time_blocking(&state).unwrap_err();
            assert!(message.to_lowercase().contains(needle), "{message:?} should mention {needle:?}");
        }
    }

    #[test]
    fn a_zero_event_gap_is_valid_but_a_grid_over_an_hour_is_not() {
        let mut state = valid_state();
        state.buffer_input = "0".to_owned();
        assert!(parse_time_blocking(&state).is_ok());
        state.granularity_input = "61".to_owned();
        assert!(parse_time_blocking(&state).is_err());
        state.granularity_input = "60".to_owned();
        assert!(parse_time_blocking(&state).is_ok());
    }

    // --- per-weekday hours ---------------------------------------------------------------

    #[test]
    fn untouched_weekday_rows_follow_the_default_and_store_nothing() {
        let values = parse_time_blocking(&valid_state()).expect("valid");
        assert_eq!(values.weekdays, [None; 7]);
    }

    #[test]
    fn a_day_off_and_a_custom_day_are_read_from_their_rows() {
        let mut state = valid_state();
        state.weekdays[6].off = true; // Sunday
        state.weekdays[4] = WeekdayRow { off: false, custom: true, start: "09:00".to_owned(), end: "13:00".to_owned() };

        let values = parse_time_blocking(&state).expect("valid");

        assert_eq!(values.weekdays[6], Some(WorkDay::Off));
        assert_eq!(values.weekdays[4], Some(WorkDay::Hours(540, 780)));
        assert_eq!(values.weekdays[0], None);
    }

    #[test]
    fn a_day_off_wins_over_own_hours_and_ignores_whatever_is_typed_in_them() {
        let mut state = valid_state();
        state.weekdays[5] =
            WeekdayRow { off: true, custom: true, start: "nonsense".to_owned(), end: "also nonsense".to_owned() };

        assert_eq!(parse_time_blocking(&state).expect("valid").weekdays[5], Some(WorkDay::Off));
    }

    #[test]
    fn own_hours_must_be_real_and_the_error_names_the_day() {
        let mut state = valid_state();
        state.weekdays[2] = WeekdayRow { off: false, custom: true, start: "9am".to_owned(), end: "17:00".to_owned() };
        assert!(parse_time_blocking(&state).unwrap_err().starts_with("Wednesday's hours must be times"));

        state.weekdays[2] = WeekdayRow { off: false, custom: true, start: "17:00".to_owned(), end: "09:00".to_owned() };
        assert!(parse_time_blocking(&state).unwrap_err().starts_with("Wednesday's hours must end after"));
    }

    #[test]
    fn untouched_hours_in_a_row_that_is_not_custom_are_not_validated() {
        let mut state = valid_state();
        state.weekdays[1] = WeekdayRow { off: false, custom: false, start: "junk".to_owned(), end: "junk".to_owned() };
        assert!(parse_time_blocking(&state).is_ok(), "a row that follows the default has no hours of its own");
    }

    #[test]
    fn reopening_the_dialog_shows_the_saved_weekday_hours_and_unsaved_days_follow_the_default() {
        let db = Database::open_in_memory().expect("open in-memory db");
        queries::settings::set_weekday_override(&db.conn, chrono::Weekday::Sun, Some(WorkDay::Off)).unwrap();
        queries::settings::set_weekday_override(&db.conn, chrono::Weekday::Fri, Some(WorkDay::Hours(540, 780))).unwrap();

        let mut state = SettingsModalState::default();
        state.open_from(&db);

        assert!(state.weekdays[6].off, "Sunday");
        assert_eq!(
            state.weekdays[4],
            WeekdayRow { off: false, custom: true, start: "09:00".to_owned(), end: "13:00".to_owned() },
            "Friday"
        );
        assert_eq!(state.weekdays[0], WeekdayRow::default(), "Monday follows the default");
    }

    #[test]
    fn what_the_dialog_would_save_round_trips_through_the_stored_overrides() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let mut state = valid_state();
        state.weekdays[6].off = true;
        state.weekdays[4] = WeekdayRow { off: false, custom: true, start: "10:00".to_owned(), end: "12:30".to_owned() };
        let values = parse_time_blocking(&state).expect("valid");

        // The same writes `show` makes on Save.
        for ((weekday, _), work_day) in WEEKDAYS.iter().zip(values.weekdays) {
            queries::settings::set_weekday_override(&db.conn, *weekday, work_day).unwrap();
        }
        let mut reopened = SettingsModalState::default();
        reopened.open_from(&db);

        assert_eq!(reopened.weekdays, state.weekdays);
        // Saving again with a row switched back to "follow the default" clears its override.
        state.weekdays[6].off = false;
        let values = parse_time_blocking(&state).expect("valid");
        for ((weekday, _), work_day) in WEEKDAYS.iter().zip(values.weekdays) {
            queries::settings::set_weekday_override(&db.conn, *weekday, work_day).unwrap();
        }
        assert_eq!(queries::settings::weekday_override(&db.conn, chrono::Weekday::Sun).unwrap(), None);
    }

    #[test]
    fn the_start_tolerance_is_validated_and_defaults_to_fifteen() {
        assert_eq!(parse_time_blocking(&valid_state()).unwrap().start_tolerance, 15);

        let mut state = valid_state();
        state.start_tolerance_input = "30".to_owned();
        assert_eq!(parse_time_blocking(&state).unwrap().start_tolerance, 30);

        for bad in ["0", "-5", "soon", "241", ""] {
            state.start_tolerance_input = bad.to_owned();
            assert!(parse_time_blocking(&state).unwrap_err().contains("Near a block"), "{bad:?}");
        }
    }

    #[test]
    fn the_startup_question_count_must_be_a_whole_number_within_the_cap() {
        assert_eq!(parse_question_count("5"), Some(5));
        assert_eq!(parse_question_count(" 12 "), Some(12));
        assert_eq!(parse_question_count("1"), Some(1));
        assert_eq!(parse_question_count("50"), Some(50));
        for bad in ["0", "51", "-1", "", "five", "2.5"] {
            assert_eq!(parse_question_count(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_startup_quiz_defaults_are_on_with_five_questions() {
        let state = SettingsModalState::default();
        assert!(state.startup_quiz_enabled);
        assert_eq!(parse_question_count(&state.startup_quiz_questions_input), Some(5));
    }

    #[test]
    fn the_past_dim_must_be_a_whole_percentage_within_the_cap() {
        assert_eq!(parse_past_dim("0"), Some(0));
        assert_eq!(parse_past_dim(" 40 "), Some(40));
        assert_eq!(parse_past_dim("90"), Some(90));
        for bad in ["91", "100", "-1", "", "medium", "12.5"] {
            assert_eq!(parse_past_dim(bad), None, "{bad:?}");
        }
        assert_eq!(parse_past_dim(&SettingsModalState::default().past_dim_input), Some(40));
    }
}
