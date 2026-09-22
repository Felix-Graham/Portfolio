use std::collections::HashSet;

use chrono::{Datelike, Duration as ChronoDuration, Local, Timelike};
use eframe::egui::{self, RichText};

use crate::database::{queries, Database};
use crate::pomodoro::{Phase, PomodoroState};
use crate::tasks::streak;
use crate::theme::{Spacing, Theme};
use crate::ui::components::{pomodoro_circle, EventCard, Input, Modal, Panel, RButton, TaskItem};

#[derive(Default)]
pub struct DashboardState {
    pub quick_task_input: String,
    /// Pending title for the "Pomodoro Stopwatch" feature's start
    /// form — kept here rather than inside `ActiveSession` itself,
    /// since there's nothing to track until "Start" is actually
    /// clicked.
    pub stopwatch_title: String,
    pub stopwatch_subject_id: Option<i64>,
    /// Feature request: "Improved Hours Per Subject View" — which
    /// window the chart is showing. Defaults to `Week` via
    /// `HoursPeriod`'s own `Default` impl.
    pub hours_period: crate::stats::HoursPeriod,
    /// Feature request: "Task Assignment" — `(task_id, date input
    /// text)` for the small "assign this task to a day" popup opened
    /// from Today's Tasks. `None` when closed.
    pub assigning_task: Option<(i64, String)>,
}

fn greeting(hour: u32) -> &'static str {
    match hour {
        5..=11 => "GOOD MORNING",
        12..=16 => "GOOD AFTERNOON",
        17..=21 => "GOOD EVENING",
        _ => "GOOD NIGHT",
    }
}

/// Dashboard shell — Sprint 5's focus: this is the screen that should
/// need no configuration to be useful the moment the app opens. Every
/// item in the plan's "Necessary Features" list has a home here:
/// greeting, date, today's events, today's tasks, the next session,
/// a completion summary, a basic streak, and quick task/event
/// creation without leaving the screen.
pub fn show(
    ui: &mut egui::Ui,
    theme: &Theme,
    db: Option<&Database>,
    state: &mut DashboardState,
    pomodoro: &mut PomodoroState,
    revision_session: &mut Option<crate::revision_session::ActiveSession>,
    calendar_state: &mut crate::ui::calendar::CalendarState,
    active_view: &mut crate::app::view::View,
) {
    let p = &theme.palette;
    let now = Local::now();
    let today = now.date_naive();

    ui.add_space(Spacing::MD);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(greeting(now.hour())).font(theme.heading_font()).color(p.text));
            ui.label(
                RichText::new(now.format("%A, %d %B").to_string().to_uppercase())
                    .font(theme.small_font())
                    .color(p.text_muted),
            );
        });
    });
    ui.add_space(Spacing::MD);

    let Some(db) = db else {
        return;
    };

    // --- Quick creation ------------------------------------------------
    // Buttons are laid out first, right-to-left, so they claim their
    // space before the input is asked how wide it can be — otherwise
    // the input fills the *entire* row (nothing has claimed space yet)
    // and pushes both buttons out to the window's far edge.
    let mut submitted_by_enter = false;
    let mut add_clicked = false;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if RButton::new("New Event").primary().show(ui, theme).clicked() {
                // Feature request: "Improved Task/Event Addition" —
                // this used to open a separate, thinner "quick add"
                // modal (title/subject/times only). Rather than
                // maintaining two event-creation forms that could
                // drift out of sync, this now jumps to the Calendar's
                // own full Edit Event modal (description, recurrence,
                // startup command, everything) in creation mode —
                // the same modal "Edit" already uses, just pre-filled
                // for today instead of an existing event.
                calendar_state.editing = Some(crate::ui::calendar::EventEditState::new_for(today));
                calendar_state.subview = crate::ui::calendar::CalendarSubview::Day;
                calendar_state.selected_day = today;
                *active_view = crate::app::view::View::Calendar;
            }
            ui.add_space(Spacing::SM);
            add_clicked = RButton::new("Add").show(ui, theme).clicked();
            ui.add_space(Spacing::SM);
            let response = Input::new(&mut state.quick_task_input, "Quick add a task… (i)")
                .id("dashboard_quick_task")
                .show(ui, theme);
            submitted_by_enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        });
    });
    if add_clicked || submitted_by_enter {
        add_quick_task(db, state, today);
    }
    ui.add_space(Spacing::MD);

    let tasks = queries::tasks::list_tasks(&db.conn).unwrap_or_default();
    let events = queries::events::list_all_events(&db.conn).unwrap_or_default();
    let completed_dates = queries::tasks::list_completed_dates(&db.conn).unwrap_or_default();

    let mut todays_events: Vec<_> = events.iter().filter(|e| e.day() == today).collect();
    todays_events.sort_by_key(|e| e.start_time);

    let current = todays_events.iter().find(|e| e.start_time <= now && now <= e.end_time);
    let next = todays_events.iter().find(|e| e.start_time > now);

    // Progress per event, computed from the same `tasks` table the
    // Calendar and To Do views read — no separate "progress" state.
    let progress_for = |event_id: i64| -> Option<(usize, usize)> {
        let attached: Vec<_> = tasks.iter().filter(|t| t.event_id == Some(event_id)).collect();
        if attached.is_empty() {
            None
        } else {
            Some((attached.iter().filter(|t| t.completed).count(), attached.len()))
        }
    };

    let today_str = today.format("%Y-%m-%d").to_string();
    let today_event_ids: HashSet<i64> = todays_events.iter().map(|e| e.id).collect();
    // Feature request: "Task Assignment" — an explicit assignment, if
    // present, decides whether a task shows up today *instead of* the
    // due-date default, not in addition to it. That's the actual
    // point of the request ("instead of assuming they should be
    // completed on the day of the deadline"): a task due today that's
    // been deliberately assigned to a later day shouldn't still
    // clutter today's list just because its deadline happens to be
    // today.
    let todays_tasks: Vec<_> = tasks
        .iter()
        .filter(|t| {
            let matches_day = match &t.assigned_date {
                Some(assigned) => assigned.as_str() == today_str,
                None => t.due_date.as_deref() == Some(today_str.as_str()),
            };
            matches_day || t.event_id.is_some_and(|id| today_event_ids.contains(&id))
        })
        .collect();
    let todays_completed = todays_tasks.iter().filter(|t| t.completed).count();

    let current_streak = streak::current_streak(&completed_dates, today);

    // --- Streak + Current/Next session ----------------------------------
    ui.columns(2, |cols| {
        Panel::new().title("Streak").show(&mut cols[0], theme, |ui| {
            ui.label(
                RichText::new(format!("{current_streak} day{}", if current_streak == 1 { "" } else { "s" }))
                    .font(theme.heading_font())
                    .color(p.text),
            );
            ui.label(
                RichText::new(if todays_tasks.is_empty() {
                    "Nothing due today.".to_owned()
                } else {
                    format!("{todays_completed} / {} tasks complete today", todays_tasks.len())
                })
                .font(theme.small_font())
                .color(p.text_faint),
            );
        });

        Panel::new().title("Session").show(&mut cols[1], theme, |ui| {
            match current {
                Some(event) => {
                    let label = format!("Now — {}", event.title);
                    let time = event.start_time.format("%H:%M").to_string();
                    let mut card = EventCard::new(&time, &label);
                    if let Some((c, t)) = progress_for(event.id) {
                        card = card.progress(c, t);
                    }
                    card.show(ui, theme);
                }
                None => {
                    ui.label(
                        RichText::new("Nothing in progress.").font(theme.small_font()).color(p.text_faint),
                    );
                }
            }
            if let Some(event) = next {
                if current.is_some() {
                    ui.add_space(Spacing::XS);
                }
                let label = format!("Next — {}", event.title);
                let time = event.start_time.format("%H:%M").to_string();
                EventCard::new(&time, &label).show(ui, theme);
            }
            if current.is_none() && next.is_none() {
                ui.label(RichText::new("Day complete.").font(theme.small_font()).color(p.text_faint));
            }
        });
    });

    ui.add_space(Spacing::MD);

    // --- Today's tasks + Today's schedule --------------------------------
    let mut toggle_id: Option<(i64, bool)> = None;

    ui.columns(2, |cols| {
        Panel::new()
            .title(&format!("Today's Tasks — {todays_completed}/{}", todays_tasks.len()))
            .show(&mut cols[0], theme, |ui| {
                if todays_tasks.is_empty() {
                    ui.label(RichText::new("Nothing due today.").font(theme.small_font()).color(p.text_faint));
                }
                for task in &todays_tasks {
                    ui.horizontal(|ui| {
                        let mut completed = task.completed;
                        if TaskItem::new(&task.title, &mut completed).show(ui, theme).clicked() {
                            toggle_id = Some((task.id, completed));
                        }
                        // Feature request: "Task Assignment" — assign
                        // (or reassign) this task to a specific day,
                        // right from Today's Tasks, rather than only
                        // being reachable via the To Do view's full
                        // edit modal.
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Assign…").clicked() {
                                state.assigning_task =
                                    Some((task.id, task.assigned_date.clone().unwrap_or_default()));
                            }
                        });
                    });
                }
            });

        Panel::new().title("Today's Schedule").show(&mut cols[1], theme, |ui| {
            if todays_events.is_empty() {
                ui.label(
                    RichText::new("No revision sessions today.")
                        .font(theme.small_font())
                        .color(p.text_faint),
                );
            }
            for (index, event) in todays_events.iter().enumerate() {
                let time_label = event.start_time.format("%H:%M").to_string();
                let mut card = EventCard::new(&time_label, &event.title);
                if let Some((completed, total)) = progress_for(event.id) {
                    card = card.progress(completed, total);
                }
                card.show(ui, theme);
                if index + 1 < todays_events.len() {
                    ui.add_space(Spacing::SM);
                }
            }
        });
    });

    if let Some((id, completed)) = toggle_id {
        let _ = queries::tasks::set_completed(&db.conn, id, completed);
    }

    ui.add_space(Spacing::MD);
    focus_panel(ui, theme, db, pomodoro);
    ui.add_space(Spacing::MD);
    stopwatch_panel(ui, theme, db, state, revision_session);

    ui.add_space(Spacing::MD);
    stats_panel(ui, theme, db, state);

    assign_task_modal(ui, theme, db, state, today);
}

/// The compact Pomodoro card. Pressing Start immediately enters
/// full-screen focus mode (rendered separately by `app::mod`, which
/// intercepts rendering entirely while `pomodoro.focused` is set) —
/// this card only ever needs to show the non-focused controls, since
/// the Dashboard itself never renders while focused.
fn focus_panel(ui: &mut egui::Ui, theme: &Theme, db: &Database, pomodoro: &mut PomodoroState) {
    let p = &theme.palette;
    let subjects = queries::subjects::list_active_subjects(&db.conn).unwrap_or_default();
    let subject_label = |id: Option<i64>| -> &str {
        id.and_then(|id| subjects.iter().find(|s| s.id == id)).map(|s| s.name.as_str()).unwrap_or("No subject")
    };
    Panel::new().title("Focus").show(ui, theme, |ui| {
        ui.horizontal(|ui| {
            pomodoro_circle::show(
                ui,
                theme,
                pomodoro.progress(),
                &crate::pomodoro::format_remaining(pomodoro.remaining),
                match pomodoro.phase {
                    Phase::Work => "Focus",
                    Phase::Break => "Break",
                },
                40.0,
            );
            ui.add_space(Spacing::MD);
            ui.vertical(|ui| {
                if pomodoro.running {
                    ui.label(
                        RichText::new(format!(
                            "{} minutes {}",
                            match pomodoro.phase {
                                Phase::Work => pomodoro.work_minutes,
                                Phase::Break => pomodoro.break_minutes,
                            },
                            match pomodoro.phase {
                                Phase::Work => "focus session",
                                Phase::Break => "break",
                            }
                        ))
                        .font(theme.small_font())
                        .color(p.text_faint),
                    );
                    // Feature request: attribute completed Work
                    // phases to a subject. Shown read-only once
                    // running — switching subjects mid-session would
                    // leave it ambiguous which subject a just-completed
                    // session should count toward, so the pick is
                    // locked in for the phase currently in progress.
                    if pomodoro.subject_id.is_some() {
                        ui.label(
                            RichText::new(format!("Logging to: {}", subject_label(pomodoro.subject_id)))
                                .font(theme.small_font())
                                .color(p.text_faint),
                        );
                    }
                    ui.add_space(Spacing::XS);
                    ui.horizontal(|ui| {
                        if RButton::new("Focus").primary().show(ui, theme).clicked() {
                            pomodoro.focused = true;
                        }
                        if ui.small_button("Pause").clicked() {
                            pomodoro.pause();
                        }
                        if ui.small_button("Reset").clicked() {
                            pomodoro.reset();
                        }
                    });
                } else {
                    ui.label(
                        RichText::new(format!("{} minute focus sessions", pomodoro.work_minutes))
                            .font(theme.small_font())
                            .color(p.text_faint),
                    );
                    ui.add_space(Spacing::XS);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Subject").font(theme.small_font()).color(p.text_faint));
                        egui::ComboBox::from_id_salt("pomodoro_subject_picker")
                            .selected_text(subject_label(pomodoro.subject_id))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut pomodoro.subject_id, None, "No subject");
                                for subject in &subjects {
                                    ui.selectable_value(
                                        &mut pomodoro.subject_id,
                                        Some(subject.id),
                                        subject.name.clone(),
                                    );
                                }
                            });
                    });
                    ui.add_space(Spacing::XS);
                    ui.horizontal(|ui| {
                        let label = if pomodoro.remaining < pomodoro.phase_total() { "Resume" } else { "Start" };
                        if RButton::new(label).primary().show(ui, theme).clicked() {
                            pomodoro.start();
                        }
                        if pomodoro.remaining < pomodoro.phase_total() && ui.small_button("Reset").clicked() {
                            pomodoro.reset();
                        }
                    });
                }
            });
        });
    });
}

/// Feature request: "Pomodoro Stopwatch" — a free-standing count-up
/// timer, started with no Calendar event required up front. Only
/// shown when no session (Revision Session *or* Stopwatch — they're
/// the same underlying `ActiveSession` type) is already active,
/// since starting a second one would just silently discard whichever
/// is running.
fn stopwatch_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    db: &Database,
    state: &mut DashboardState,
    revision_session: &mut Option<crate::revision_session::ActiveSession>,
) {
    if revision_session.is_some() {
        return;
    }

    let p = &theme.palette;
    let subjects = queries::subjects::list_active_subjects(&db.conn).unwrap_or_default();

    Panel::new().title("Stopwatch").show(ui, theme, |ui| {
        ui.label(
            RichText::new("Start timing something without scheduling it on the Calendar first")
                .font(theme.small_font())
                .color(p.text_faint),
        );
        ui.add_space(Spacing::XS);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.stopwatch_title)
                    .hint_text("What are you working on?")
                    .desired_width(220.0),
            );
            ui.add_space(Spacing::SM);
            let current_label = state
                .stopwatch_subject_id
                .and_then(|id| subjects.iter().find(|s| s.id == id))
                .map(|s| s.name.as_str())
                .unwrap_or("No subject");
            egui::ComboBox::from_id_salt("stopwatch_subject_picker").selected_text(current_label).show_ui(
                ui,
                |ui| {
                    ui.selectable_value(&mut state.stopwatch_subject_id, None, "No subject");
                    for subject in &subjects {
                        ui.selectable_value(&mut state.stopwatch_subject_id, Some(subject.id), subject.name.clone());
                    }
                },
            );
        });
        ui.add_space(Spacing::XS);
        let title = state.stopwatch_title.trim().to_owned();
        let start_enabled = !title.is_empty();
        ui.add_enabled_ui(start_enabled, |ui| {
            if RButton::new("Start Stopwatch").primary().show(ui, theme).clicked() {
                *revision_session = Some(crate::revision_session::ActiveSession::new(
                    None,
                    None,
                    title.clone(),
                    state.stopwatch_subject_id,
                    state
                        .stopwatch_subject_id
                        .and_then(|id| subjects.iter().find(|s| s.id == id))
                        .map(|s| s.name.clone()),
                    0,
                    Vec::new(),
                ));
                state.stopwatch_title.clear();
            }
        });
        if !start_enabled {
            ui.label(RichText::new("Give it a title first").font(theme.small_font()).color(p.text_faint));
        }
    });
}

/// The "assign this task to a day" popup (feature request: "Task
/// Assignment"). Deliberately tiny — one field, reusing the same
/// natural-language date parsing already used for due dates
/// (`natural_date::parse_natural_date`) — since the whole point is a
/// one-click action from Today's Tasks, not a detour through the full
/// To Do edit modal.
fn assign_task_modal(ui: &mut egui::Ui, theme: &Theme, db: &Database, state: &mut DashboardState, today: chrono::NaiveDate) {
    let p = &theme.palette;
    let mut modal_open = state.assigning_task.is_some();
    let mut save_requested = false;
    let mut clear_requested = false;
    let mut cancel_requested = false;

    Modal::new("assign_task_modal", "Assign Task").show(ui.ctx(), theme, &mut modal_open, |ui| {
        let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if let Some((_, date_input)) = &mut state.assigning_task {
            ui.label(
                RichText::new("Which day should this be worked on?").font(theme.small_font()).color(p.text_faint),
            );
            ui.add_space(Spacing::SM);
            let placeholder = format!("YYYY-MM-DD, 'today', 'tomorrow', 'next tuesday' — today is {today}");
            Input::new(date_input, &placeholder).id("assign_task_date").show(ui, theme);

            ui.add_space(Spacing::MD);
            ui.horizontal(|ui| {
                if RButton::new("Assign").primary().show(ui, theme).clicked() || enter_pressed {
                    save_requested = true;
                }
                if ui.small_button("Clear assignment").clicked() {
                    clear_requested = true;
                }
                if RButton::new("Cancel").show(ui, theme).clicked() {
                    cancel_requested = true;
                }
            });
        }
    });

    if save_requested {
        if let Some((id, date_input)) = &state.assigning_task {
            match crate::natural_date::parse_natural_date(date_input, today) {
                Ok(date) => {
                    let formatted = date.map(|d| d.format("%Y-%m-%d").to_string());
                    let _ = queries::tasks::set_assigned_date(&db.conn, *id, formatted.as_deref());
                    state.assigning_task = None;
                }
                Err(_) => {
                    // Left open with the same text still in the
                    // field — no separate error label here since this
                    // popup has no room set aside for one; the
                    // placeholder text already spells out the
                    // accepted formats.
                }
            }
        }
        return;
    }
    if clear_requested {
        if let Some((id, _)) = &state.assigning_task {
            let _ = queries::tasks::set_assigned_date(&db.conn, *id, None);
        }
        state.assigning_task = None;
        return;
    }
    if cancel_requested || !modal_open {
        state.assigning_task = None;
    }
}

fn stats_panel(ui: &mut egui::Ui, theme: &Theme, db: &Database, state: &mut DashboardState) {
    let p = &theme.palette;
    Panel::new().title("Stats").show(ui, theme, |ui| {
        let focus_minutes = queries::pomodoro::total_minutes(&db.conn).unwrap_or(0);
        let pomodoros_completed = queries::pomodoro::count_sessions(&db.conn).unwrap_or(0);
        // Feature request: "Flashcards" — all-time count of cards
        // whose answer has been revealed during study, from the
        // append-only `flashcards_seen_log` (same pattern as the two
        // Pomodoro stats above).
        let flashcards_seen = queries::flashcards::total_seen(&db.conn).unwrap_or(0);

        ui.columns(3, |cols| {
            cols[0].label(
                RichText::new(format!("Focus time: {}h {}m", focus_minutes / 60, focus_minutes % 60))
                    .font(theme.label_font())
                    .color(p.text),
            );
            cols[1].label(
                RichText::new(format!("Pomodoros completed: {pomodoros_completed}"))
                    .font(theme.label_font())
                    .color(p.text),
            );
            cols[2].label(
                RichText::new(format!("Flashcards seen: {flashcards_seen}")).font(theme.label_font()).color(p.text),
            );
        });

        ui.add_space(Spacing::SM);
        ui.label(RichText::new("Tasks done").font(theme.small_font()).color(p.text_faint));
        ui.add_space(Spacing::XS);
        tasks_heatmap(ui, theme, &crate::stats::tasks_done_by_day(db));

        ui.add_space(Spacing::MD);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Hours per subject").font(theme.small_font()).color(p.text_faint));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for period in [crate::stats::HoursPeriod::Year, crate::stats::HoursPeriod::Month, crate::stats::HoursPeriod::Week] {
                    let selected = state.hours_period == period;
                    if ui.selectable_label(selected, period.label()).clicked() {
                        state.hours_period = period;
                    }
                }
            });
        });
        ui.add_space(Spacing::XS);
        subject_hours_chart(ui, theme, db, state.hours_period);

        ui.add_space(Spacing::MD);
        ui.label(RichText::new("Total time by week").font(theme.small_font()).color(p.text_faint));
        ui.add_space(Spacing::XS);
        total_time_by_week_chart(ui, theme, db);
    });
}

/// A GitHub-commit-style heatmap of the last 10 weeks of task
/// completions: one column per week, one row per weekday, shaded by
/// how many tasks were completed that day.
fn tasks_heatmap(ui: &mut egui::Ui, theme: &Theme, counts: &std::collections::HashMap<chrono::NaiveDate, usize>) {
    let p = &theme.palette;
    let today = Local::now().date_naive();
    let weeks = 10;
    let start = today - ChronoDuration::days(today.weekday().num_days_from_monday() as i64)
        - ChronoDuration::weeks(weeks - 1);

    let cell = 12.0;
    let gap = 3.0;
    let size = egui::vec2((cell + gap) * weeks as f32, (cell + gap) * 7.0);
    let (response, painter) = ui.allocate_painter(size, egui::Sense::hover());
    let origin = response.rect.left_top();

    let max_count = counts.values().copied().max().unwrap_or(0).max(1);

    for week in 0..weeks {
        for weekday in 0..7 {
            let date = start + ChronoDuration::weeks(week) + ChronoDuration::days(weekday);
            if date > today {
                continue;
            }
            let count = counts.get(&date).copied().unwrap_or(0);
            let intensity = count as f32 / max_count as f32;
            let color = if count == 0 {
                p.surface
            } else {
                egui::Color32::from_rgb(
                    lerp_u8(p.surface.r(), p.focus.r(), intensity),
                    lerp_u8(p.surface.g(), p.focus.g(), intensity),
                    lerp_u8(p.surface.b(), p.focus.b(), intensity),
                )
            };
            let rect_min = origin + egui::vec2(week as f32 * (cell + gap), weekday as f32 * (cell + gap));
            let rect = egui::Rect::from_min_size(rect_min, egui::vec2(cell, cell));
            painter.rect_filled(rect, egui::CornerRadius::same(1), color);
            painter.rect_stroke(
                rect,
                egui::CornerRadius::same(1),
                egui::Stroke::new(1.0, p.border),
                egui::StrokeKind::Inside,
            );
        }
    }
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t.clamp(0.0, 1.0)) as u8
}

/// A horizontal bar per subject with logged hours in `period`,
/// longest first (feature request: "Improved Hours Per Subject
/// View" — selectable time window, defaulting to this-week).
fn subject_hours_chart(ui: &mut egui::Ui, theme: &Theme, db: &Database, period: crate::stats::HoursPeriod) {
    let p = &theme.palette;
    let today = Local::now().date_naive();
    let (start, end) = crate::stats::period_bounds(period, today);
    let totals = crate::stats::minutes_per_subject_rolled_up(db, start, end);
    if totals.is_empty() {
        ui.label(
            RichText::new(format!("No hours logged {}.", period.label().to_lowercase()))
                .font(theme.small_font())
                .color(p.text_faint),
        );
        return;
    }

    let subjects = queries::subjects::list_all_subjects(&db.conn).unwrap_or_default();
    let mut rows: Vec<(String, i64)> = totals
        .into_iter()
        .map(|(id, minutes)| {
            let name = subjects.iter().find(|s| s.id == id).map(|s| s.name.clone()).unwrap_or_default();
            (name, minutes)
        })
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));

    let max_minutes = rows.iter().map(|(_, m)| *m).max().unwrap_or(1).max(1);
    let bar_max_width = 220.0;

    for (name, minutes) in &rows {
        ui.horizontal(|ui| {
            ui.add_sized([90.0, 0.0], egui::Label::new(RichText::new(name).font(theme.small_font()).color(p.text)));
            let width = (*minutes as f32 / max_minutes as f32) * bar_max_width;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width.max(2.0), 14.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, egui::CornerRadius::same(1), p.focus);
            ui.label(
                RichText::new(format!("{}h {}m", minutes / 60, minutes % 60))
                    .font(theme.small_font())
                    .color(p.text_faint),
            );
        });
        ui.add_space(2.0);
    }

    // Feature request: "Time Per Subject" pie chart, alongside the
    // existing bar chart above rather than replacing it — the request
    // asked for "as well as", not "instead of".
    ui.add_space(Spacing::SM);
    pie_chart(ui, theme, &rows);
}

/// A small, fixed set of visually distinct colors used only for the
/// per-subject line graph below (feature request: "Improved Line
/// Graph" — "show a line for each subject, coloured accordingly").
/// A deliberate, scoped exception to the rest of the app's monochrome
/// design (see `theme::Palette`'s own "no color accents" framing) —
/// distinguishing several overlapping lines by grayscale alone isn't
/// really possible, and the request asked for color specifically.
/// Assigned by `subject.id % SUBJECT_LINE_COLORS.len()` wherever a
/// subject needs a color, so a given subject's line color stays the
/// same across renders regardless of which other subjects currently
/// have time logged (a positional/index-based assignment would let a
/// subject's color shift whenever a different subject is added or
/// removed from the picture).
const SUBJECT_LINE_COLORS: &[egui::Color32] = &[
    egui::Color32::from_rgb(0xE0, 0x6C, 0x5A), // terracotta
    egui::Color32::from_rgb(0x5A, 0x9E, 0xE0), // steel blue
    egui::Color32::from_rgb(0x6F, 0xB8, 0x6A), // sage green
    egui::Color32::from_rgb(0xD9, 0xA9, 0x3B), // ochre
    egui::Color32::from_rgb(0xA8, 0x7C, 0xD9), // lavender
    egui::Color32::from_rgb(0xD9, 0x6F, 0xA8), // dusty pink
    egui::Color32::from_rgb(0x4C, 0xB8, 0xAE), // teal
    egui::Color32::from_rgb(0xB8, 0x8A, 0x5A), // tan
];

fn subject_line_color(subject_id: i64) -> egui::Color32 {
    SUBJECT_LINE_COLORS[(subject_id.rem_euclid(SUBJECT_LINE_COLORS.len() as i64)) as usize]
}

/// A hand-drawn line chart of logged minutes per week, one line per
/// subject (feature request: "Improved Line Graph" — "instead of the
/// line representing the total time, show a line for each subject,
/// coloured accordingly"), plus the week-over-week comparison message
/// for the combined total (unchanged from before — the request asked
/// to change how the lines are drawn, not to drop the summary). 8
/// weeks — enough to see a real trend without cramming the Dashboard.
fn total_time_by_week_chart(ui: &mut egui::Ui, theme: &Theme, db: &Database) {
    let p = &theme.palette;
    const NUM_WEEKS: usize = 8;
    let weeks = crate::stats::total_minutes_by_week_per_subject(db, NUM_WEEKS);

    let any_minutes_logged = weeks.iter().any(|(_, by_subject)| by_subject.values().any(|m| *m > 0));
    if !any_minutes_logged {
        ui.label(RichText::new("No hours logged yet.").font(theme.small_font()).color(p.text_faint));
        return;
    }

    // Derived from the same per-subject data the lines below are
    // drawn from, rather than a second query, so the "up 14%"
    // message and the lines can never disagree about what a week's
    // total was.
    let totals: Vec<(chrono::NaiveDate, i64)> =
        weeks.iter().map(|(week, by_subject)| (*week, by_subject.values().sum())).collect();
    if let Some(message) = crate::stats::week_over_week_change(&totals) {
        ui.label(RichText::new(message).font(theme.label_font()).color(p.text));
        ui.add_space(Spacing::XS);
    }

    const CHART_WIDTH: f32 = 320.0;
    const CHART_HEIGHT: f32 = 80.0;
    let max_minutes = totals.iter().map(|(_, m)| *m).max().unwrap_or(1).max(1) as f32;

    // Every subject with at least one logged minute somewhere in the
    // window gets its own line — sorted by id purely so the legend
    // and draw order are stable frame to frame, not because id order
    // means anything.
    let mut subject_ids: Vec<i64> =
        weeks.iter().flat_map(|(_, by_subject)| by_subject.keys().copied()).collect::<HashSet<_>>().into_iter().collect();
    subject_ids.sort_unstable();

    let (rect, _) = ui.allocate_exact_size(egui::vec2(CHART_WIDTH, CHART_HEIGHT), egui::Sense::hover());
    let painter = ui.painter();
    painter.hline(rect.left()..=rect.right(), rect.bottom(), egui::Stroke::new(1.0, p.border));

    for &subject_id in &subject_ids {
        let values: Vec<f32> = weeks.iter().map(|(_, by_subject)| *by_subject.get(&subject_id).unwrap_or(&0) as f32).collect();
        let color = subject_line_color(subject_id);
        let points = line_chart_points(rect, &values, max_minutes);
        if points.len() >= 2 {
            painter.add(egui::Shape::line(points.clone(), egui::Stroke::new(2.0, color)));
        }
        for point in &points {
            painter.circle_filled(*point, 2.5, color);
        }
    }

    ui.add_space(Spacing::XS);
    ui.horizontal(|ui| {
        for (index, (week_start, _)) in weeks.iter().enumerate() {
            // Every other label, to avoid crowding 8 dates into a
            // 320px-wide row.
            let text = if index % 2 == 0 { week_start.format("%d %b").to_string() } else { String::new() };
            ui.add_sized(
                [CHART_WIDTH / NUM_WEEKS as f32, 0.0],
                egui::Label::new(RichText::new(text).font(theme.small_font()).color(p.text_faint)),
            );
        }
    });

    // Legend — without it the colors alone don't say which subject is
    // which.
    if !subject_ids.is_empty() {
        ui.add_space(Spacing::XS);
        let subjects = queries::subjects::list_all_subjects(&db.conn).unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            for &subject_id in &subject_ids {
                let name = subjects.iter().find(|s| s.id == subject_id).map(|s| s.name.as_str()).unwrap_or("Unknown");
                let color = subject_line_color(subject_id);
                let (swatch_rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().rect_filled(swatch_rect, egui::CornerRadius::same(1), color);
                ui.label(RichText::new(name).font(theme.small_font()).color(p.text_faint));
                ui.add_space(Spacing::SM);
            }
        });
    }
}

/// The on-screen point for each value in `values` (assumed
/// chronological, oldest first), evenly spaced across `rect`'s width
/// and scaled so `max_value` sits at the top — pulled out as its own
/// pure function so the layout math is tested directly rather than
/// only visible through the painted result.
fn line_chart_points(rect: egui::Rect, values: &[f32], max_value: f32) -> Vec<egui::Pos2> {
    if values.is_empty() {
        return Vec::new();
    }
    let step = if values.len() > 1 { rect.width() / (values.len() - 1) as f32 } else { 0.0 };
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let x = rect.left() + step * index as f32;
            let fraction = if max_value > 0.0 { (value / max_value).clamp(0.0, 1.0) } else { 0.0 };
            let y = rect.bottom() - fraction * rect.height();
            egui::pos2(x, y)
        })
        .collect()
}

/// A hand-drawn pie chart (feature request: "Time Per Subject" —
/// alongside the existing bar chart, same data). Slices are shaded in
/// a gray gradient rather than distinct hues — this app's design
/// system is explicitly monochrome ("no color accents"), and a
/// gradient of lightness is still enough to tell adjacent slices
/// apart, the same reasoning behind Notes Folders' curated swatch set
/// staying muted instead of using a full RGB picker.
fn pie_chart(ui: &mut egui::Ui, theme: &Theme, rows: &[(String, i64)]) {
    let p = &theme.palette;
    let total: i64 = rows.iter().map(|(_, m)| *m).sum();
    if total <= 0 {
        return;
    }

    const DIAMETER: f32 = 140.0;
    let radius = DIAMETER / 2.0;

    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(DIAMETER, DIAMETER), egui::Sense::hover());
        let center = rect.center();
        let painter = ui.painter();

        let n = rows.len().max(1);
        let mut cumulative_frac = 0.0_f32;
        for (index, (_, minutes)) in rows.iter().enumerate() {
            let slice_frac = *minutes as f32 / total as f32;
            let color = pie_slice_color(p.text, p.text_faint, index, n);
            let points = pie_slice_points(center, radius, cumulative_frac, cumulative_frac + slice_frac);
            painter.add(egui::Shape::convex_polygon(points, color, egui::Stroke::new(1.5, p.background)));
            cumulative_frac += slice_frac;
        }

        ui.add_space(Spacing::MD);
        ui.vertical(|ui| {
            for (index, (name, minutes)) in rows.iter().enumerate() {
                let color = pie_slice_color(p.text, p.text_faint, index, n);
                let percent = (*minutes as f64 / total as f64 * 100.0).round() as i64;
                ui.horizontal(|ui| {
                    let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot_rect.center(), 4.0, color);
                    ui.label(
                        RichText::new(format!("{name} — {percent}%")).font(theme.small_font()).color(p.text),
                    );
                });
            }
        });
    });
}

/// Evenly spaced gray shades across `[light, dark]`, darkest first —
/// pulled out as its own pure function so the interpolation math is
/// unit-tested directly.
fn pie_slice_color(dark: egui::Color32, light: egui::Color32, index: usize, count: usize) -> egui::Color32 {
    let t = if count <= 1 { 0.0 } else { index as f32 / (count - 1) as f32 };
    let lerp = |a: u8, b: u8| -> u8 { (a as f32 + (b as f32 - a as f32) * t).round() as u8 };
    egui::Color32::from_rgb(lerp(dark.r(), light.r()), lerp(dark.g(), light.g()), lerp(dark.b(), light.b()))
}

/// The polygon points for one pie slice spanning `[start_frac,
/// end_frac)` of a full circle (0.0 = 12 o'clock, going clockwise) —
/// a fan of triangles from `center` out to an arc approximated with
/// straight segments. Pulled out as its own function (rather than
/// inlined into the painter loop) purely so the point-count/shape
/// logic can be reasoned about and tested independently of any actual
/// painting.
fn pie_slice_points(center: egui::Pos2, radius: f32, start_frac: f32, end_frac: f32) -> Vec<egui::Pos2> {
    let start_angle = start_frac * std::f32::consts::TAU;
    let end_angle = end_frac * std::f32::consts::TAU;
    let steps = (((end_frac - start_frac) * 64.0).ceil() as usize).max(1);

    let mut points = Vec::with_capacity(steps + 2);
    points.push(center);
    for i in 0..=steps {
        let t = start_angle + (end_angle - start_angle) * (i as f32 / steps as f32);
        points.push(center + radius * egui::vec2(t.sin(), -t.cos()));
    }
    points
}

fn add_quick_task(db: &Database, state: &mut DashboardState, today: chrono::NaiveDate) {
    let title = state.quick_task_input.trim();
    if title.is_empty() {
        return;
    }
    let due = today.format("%Y-%m-%d").to_string();
    if queries::tasks::create_task(&db.conn, title, Some(&due), None, None).is_ok() {
        state.quick_task_input.clear();
    }
}

#[cfg(test)]
mod pie_chart_tests {
    use super::*;

    #[test]
    fn line_chart_points_first_point_is_at_the_rects_left_edge() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 50.0));
        let points = line_chart_points(rect, &[10.0, 20.0, 30.0], 30.0);
        assert_eq!(points[0].x, 0.0);
    }

    #[test]
    fn line_chart_points_last_point_is_at_the_rects_right_edge() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 50.0));
        let points = line_chart_points(rect, &[10.0, 20.0, 30.0], 30.0);
        assert_eq!(points.last().unwrap().x, 100.0);
    }

    #[test]
    fn line_chart_points_max_value_sits_at_the_top_of_the_rect() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 50.0));
        let points = line_chart_points(rect, &[30.0], 30.0);
        assert_eq!(points[0].y, rect.top());
    }

    #[test]
    fn line_chart_points_zero_value_sits_at_the_bottom_of_the_rect() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 50.0));
        let points = line_chart_points(rect, &[0.0], 30.0);
        assert_eq!(points[0].y, rect.bottom());
    }

    #[test]
    fn line_chart_points_empty_input_returns_no_points() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 50.0));
        assert!(line_chart_points(rect, &[], 10.0).is_empty());
    }

    #[test]
    fn line_chart_points_a_zero_max_never_divides_by_zero() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 50.0));
        let points = line_chart_points(rect, &[0.0, 0.0], 0.0);
        assert_eq!(points.len(), 2);
        assert!(points.iter().all(|p| p.y.is_finite()));
    }

    #[test]
    fn pie_slice_color_first_is_dark_last_is_light() {
        let dark = egui::Color32::from_gray(0x20);
        let light = egui::Color32::from_gray(0xE0);
        assert_eq!(pie_slice_color(dark, light, 0, 4), dark);
        assert_eq!(pie_slice_color(dark, light, 3, 4), light);
    }

    #[test]
    fn pie_slice_color_single_slice_is_dark() {
        // count <= 1 must not divide by zero.
        let dark = egui::Color32::from_gray(0x20);
        let light = egui::Color32::from_gray(0xE0);
        assert_eq!(pie_slice_color(dark, light, 0, 1), dark);
    }

    #[test]
    fn pie_slice_points_starts_and_ends_with_the_center() {
        let center = egui::pos2(50.0, 50.0);
        let points = pie_slice_points(center, 20.0, 0.0, 0.25);
        assert_eq!(points[0], center);
        assert!(points.len() >= 3, "a slice needs at least a center + two arc points to be a real polygon");
    }

    #[test]
    fn pie_slice_points_a_quarter_slice_starts_at_top_and_ends_at_the_right() {
        let center = egui::pos2(0.0, 0.0);
        let radius = 10.0;
        let points = pie_slice_points(center, radius, 0.0, 0.25);
        let first_arc_point = points[1];
        let last_arc_point = *points.last().unwrap();
        // 0.0 = straight up (12 o'clock): (0, -radius).
        assert!((first_arc_point.x).abs() < 0.001);
        assert!((first_arc_point.y - -radius).abs() < 0.001);
        // 0.25 = a quarter turn clockwise = 3 o'clock: (radius, 0).
        assert!((last_arc_point.x - radius).abs() < 0.001);
        assert!((last_arc_point.y).abs() < 0.001);
    }

    #[test]
    fn pie_slice_points_more_steps_for_a_bigger_slice() {
        let center = egui::pos2(0.0, 0.0);
        let small = pie_slice_points(center, 10.0, 0.0, 0.1);
        let big = pie_slice_points(center, 10.0, 0.0, 0.9);
        assert!(big.len() > small.len(), "a near-full circle needs more segments than a sliver to look round");
    }
}
