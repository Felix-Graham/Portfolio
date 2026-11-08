use std::collections::HashSet;

use chrono::{Datelike, Duration as ChronoDuration, Local, Timelike};
use eframe::egui::{self, Align, Layout, Rect, RichText, UiBuilder, Vec2};

use crate::database::{queries, Database};
use crate::pomodoro::{Phase, PomodoroState};
use crate::timer::TimerState;
use crate::subjects::Subject;
use crate::tasks::streak;
use crate::theme::{Spacing, Theme};
use crate::ui::components::{pomodoro_circle, progress_dashes, scroll_select, EventCard, Input, Modal, Panel, RButton, TaskItem};
use crate::ui::pager::{self, Pager};

#[derive(Default)]
pub struct DashboardState {
    pub quick_task_input: String,
    /// Pending title for the "Pomodoro Stopwatch" feature's start
    /// form — kept here rather than inside `ActiveSession` itself,
    /// since there's nothing to track until "Start" is actually
    /// clicked.
    pub stopwatch_title: String,
    pub stopwatch_subject_id: Option<i64>,
    /// Feature request: task "bars" — the optional `task` the pending
    /// stopwatch will log its time against.
    pub stopwatch_task_id: Option<i64>,
    /// Feature request: "Improved Hours Per Subject View" — which
    /// window the chart is showing. Defaults to `Week` via
    /// `HoursPeriod`'s own `Default` impl.
    pub hours_period: crate::stats::HoursPeriod,
    /// Feature request: "Task Assignment" — `(task_id, date input
    /// text)` for the small "assign this task to a day" popup opened
    /// from Today's Tasks. `None` when closed.
    pub assigning_task: Option<(i64, String)>,
    /// Feature request: the Dashboard's three sections are full-screen
    /// pages. Which one is showing, plus the scroll-gesture
    /// bookkeeping — see `ui::pager`.
    pub pager: Pager,
}

fn greeting(hour: u32) -> &'static str {
    match hour {
        5..=11 => "GOOD MORNING",
        12..=16 => "GOOD AFTERNOON",
        17..=21 => "GOOD EVENING",
        _ => "GOOD NIGHT",
    }
}

/// The Dashboard's pages, in order: `today`, `focus`, `stats`.
pub const PAGE_COUNT: usize = 3;
/// Height reserved at the top of a page for its small section label.
const LABEL_H: f32 = 24.0;
/// Height of the greeting / quick-add header on the `today` page.
const HEADER_H: f32 = 60.0;
/// Space between the cells of a page.
const CELL_GAP: f32 = Spacing::MD;

/// Dashboard shell — Sprint 5's focus: this is the screen that should
/// need no configuration to be useful the moment the app opens. Every
/// item in the plan's "Necessary Features" list has a home here:
/// greeting, date, today's events, today's tasks, the next session,
/// a completion summary, a basic streak, and quick task/event
/// creation without leaving the screen.
///
/// Feature request: "each of the sections... a full screen item (not
/// possible to see next or previous tabs), snap to scroll to the next
/// tab, items arranged to fill space, momentary side bar to display
/// application position". The Dashboard is therefore three pages —
/// today, focus, stats — each exactly the size of the screen, with
/// only the current one drawn (and, mid-slide, the one it is sliding
/// to). Scroll gestures, `PageUp`/`PageDown` (handled in `app`) and
/// the side indicator move between them; `ui::pager` holds the
/// decisions, this function only wires them to `egui`. Unlike the
/// other views it does not sit inside the shell's `ScrollArea`: it
/// claims the whole central area itself.
///
/// `paging_enabled` is false while a dialog, the palette or the help
/// overlay is up, so scrolling inside those can't turn the page.
pub fn show(
    ui: &mut egui::Ui,
    theme: &Theme,
    db: Option<&Database>,
    state: &mut DashboardState,
    pomodoro: &mut PomodoroState,
    timer: &mut TimerState,
    revision_session: &mut Option<crate::revision_session::ActiveSession>,
    calendar_state: &mut crate::ui::calendar::CalendarState,
    active_view: &mut crate::app::view::View,
    db_error: Option<&str>,
    paging_enabled: bool,
) {
    let ctx = ui.ctx().clone();
    let now_s = ctx.input(|i| i.time);
    let viewport = ui.available_rect_before_wrap();
    ui.allocate_rect(viewport, egui::Sense::hover());

    // --- input: scroll gestures turn the page ----------------------------
    // Cells report (via `Panel::fill`) whether they can still scroll;
    // a gesture over one that can is theirs, not the pager's. Taking
    // the reports every frame — even with paging disabled — keeps the
    // list from growing.
    let reports = pager::take_reports(&ctx);
    if paging_enabled {
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let delta = ctx.input(|i| i.smooth_scroll_delta.y);
        if delta.abs() > pager::QUIET && pager::inner_handles(&reports, pointer, delta) {
            state.pager.hold_off(now_s);
        } else {
            state.pager.feed(delta, now_s, PAGE_COUNT);
        }
    }

    // --- the pages ---------------------------------------------------------
    let position = ctx.animate_value_with_time(
        egui::Id::new("dashboard_page_position"),
        state.pager.page as f32,
        pager::SLIDE_SECONDS,
    );
    let content = viewport.shrink(Spacing::MD);
    for index in 0..PAGE_COUNT {
        if !pager::is_visible(index, position) {
            continue;
        }
        let offset = pager::slide_offset(index, position, viewport.height());
        let page_rect = content.translate(egui::vec2(0.0, offset));
        // `new_child`, not `scope_builder`: pages sliding off-screen
        // must not be allocated into (and grow) the parent.
        let mut page_ui = ui.new_child(
            UiBuilder::new().max_rect(page_rect).layout(Layout::top_down(Align::Min)),
        );
        page_ui.set_clip_rect(viewport.intersect(ui.clip_rect()));
        match index {
            0 => today_page(&mut page_ui, theme, db, state, calendar_state, active_view, db_error),
            1 => focus_page(&mut page_ui, theme, db, state, pomodoro, timer, revision_session),
            _ => stats_page(&mut page_ui, theme, db, state),
        }
    }

    if let Some(db) = db {
        assign_task_modal(ui, theme, db, state, Local::now().date_naive());
    }

    // --- the momentary position indicator -----------------------------------
    // Hovering where it sits reveals it, so it stays discoverable.
    let column = progress_dashes::column_rect(viewport, viewport.right() - 5.0, PAGE_COUNT);
    if progress_dashes::is_hovered(ui, column) {
        state.pager.touch(now_s);
    }
    let opacity = state.pager.indicator_opacity(now_s);
    if let Some(page) = progress_dashes::show(ui, theme, column, position, PAGE_COUNT, opacity) {
        state.pager.go_to(page, now_s, PAGE_COUNT);
    }
    // Nothing else repaints for the sake of a fade.
    if let Some(wait) = state.pager.repaint_after(now_s) {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
    }
}

/// Splits off the top `height` of `area`: `(top strip, the rest)`.
fn take_top(area: Rect, height: f32) -> (Rect, Rect) {
    let split_y = (area.top() + height).min(area.bottom());
    (
        Rect::from_min_max(area.min, egui::pos2(area.right(), split_y)),
        Rect::from_min_max(egui::pos2(area.left(), split_y), area.max),
    )
}

/// Runs `add_contents` in a child UI confined to `rect` — how a page
/// places each of its cells.
fn in_cell<R>(ui: &mut egui::Ui, rect: Rect, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)), add_contents).inner
}

/// Page 1, "today": welcome and quick add across the top, then the
/// streak and session cards, then today's tasks and schedule filling
/// the rest.
fn today_page(
    ui: &mut egui::Ui,
    theme: &Theme,
    db: Option<&Database>,
    state: &mut DashboardState,
    calendar_state: &mut crate::ui::calendar::CalendarState,
    active_view: &mut crate::app::view::View,
    db_error: Option<&str>,
) {
    let p = &theme.palette;
    let now = Local::now();
    let today = now.date_naive();

    let (label_area, body) = take_top(ui.max_rect(), LABEL_H);
    in_cell(ui, label_area, |ui| section_label(ui, theme, "today"));

    // --- header: greeting on the left, quick creation on the right -------
    let (header, rest) = take_top(body, HEADER_H);
    let header_cells = pager::grid(header, &[1.0], &[1.0, 1.3], CELL_GAP);
    in_cell(ui, header_cells[0][0], |ui| {
        ui.label(RichText::new(greeting(now.hour())).font(theme.heading_font()).color(p.text));
        ui.label(
            RichText::new(now.format("%A, %d %B").to_string().to_uppercase())
                .font(theme.small_font())
                .color(p.text_muted),
        );
        if let Some(err) = db_error {
            ui.label(RichText::new(err).font(theme.small_font()).color(p.text_faint));
        }
    });

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
    in_cell(ui, header_cells[0][1], |ui| {
        ui.add_space(Spacing::SM);
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
    });
    if add_clicked || submitted_by_enter {
        add_quick_task(db, state, today);
    }

    let tasks = queries::tasks::list_tasks(&db.conn).unwrap_or_default();
    let events = queries::events::list_visible_events(&db.conn).unwrap_or_default();
    let completed_dates = queries::tasks::list_completed_dates(&db.conn).unwrap_or_default();
    // Feature request: "Colour" — read once here rather than inside
    // each `EventCard` call site below, since there are three of them
    // on this view alone.
    let subjects = queries::subjects::list_all_subjects(&db.conn).unwrap_or_default();
    let accent_for = |subject_id: Option<i64>| subject_id.map(|id| crate::theme::subject_color(&subjects, id));

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
    let mut todays_tasks: Vec<_> = tasks
        .iter()
        .filter(|t| {
            // Feature request: "Auto Assigning" — a topic-planning
            // template isn't itself work for any day; its per-day
            // generated tasks are.
            if t.plan_topics_ahead {
                return false;
            }
            let matches_day = match &t.assigned_date {
                Some(assigned) => assigned.as_str() == today_str,
                None => t.due_date.as_deref() == Some(today_str.as_str()),
            };
            matches_day || t.event_id.is_some_and(|id| today_event_ids.contains(&id))
        })
        .collect();
    // Time blocking: tasks with a planned time today are listed in time
    // order (the rest keep their order after them) and show that time.
    // Text only — no layout change. Nothing happens while it's off.
    let today_blocks: Vec<crate::tasks::TimeBlock> =
        if queries::settings::time_blocking_enabled(&db.conn).unwrap_or(false) {
            queries::task_blocks::list_blocks_between(&db.conn, today, today).unwrap_or_default()
        } else {
            Vec::new()
        };
    if !today_blocks.is_empty() {
        let first = crate::tasks::block::first_block_start_by_task(&today_blocks);
        todays_tasks.sort_by_key(|t| (!first.contains_key(&t.id), first.get(&t.id).copied()));
    }
    let todays_completed = todays_tasks.iter().filter(|t| t.completed).count();

    let current_streak = streak::current_streak(&completed_dates, today);

    // Two rows of two cells filling the rest of the page: the short
    // streak/session cards on top, the (taller, scrolling) lists below.
    let cells = pager::grid(take_top(rest, CELL_GAP).1, &[1.0, 2.6], &[1.0, 1.0], CELL_GAP);

    // --- Streak + Current/Next session ----------------------------------
    in_cell(ui, cells[0][0], |ui| {
        Panel::new().title("Streak").fill(cells[0][0].size()).show(ui, theme, |ui| {
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
    });

    in_cell(ui, cells[0][1], |ui| {
        Panel::new().title("Session").fill(cells[0][1].size()).show(ui, theme, |ui| {
            match current {
                Some(event) => {
                    let label = format!("Now — {}", event.title);
                    let time = event.start_time.format("%H:%M").to_string();
                    let mut card = EventCard::new(&time, &label);
                    if let Some(color) = accent_for(event.subject_id) {
                        card = card.accent_color(color);
                    }
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
                let mut card = EventCard::new(&time, &label);
                if let Some(color) = accent_for(event.subject_id) {
                    card = card.accent_color(color);
                }
                card.show(ui, theme);
            }
            if current.is_none() && next.is_none() {
                ui.label(RichText::new("Day complete.").font(theme.small_font()).color(p.text_faint));
            }
        });
    });

    // --- Today's tasks + Today's schedule --------------------------------
    let mut toggle_id: Option<(i64, bool)> = None;

    in_cell(ui, cells[1][0], |ui| {
        Panel::new()
            .title(&format!("Today's Tasks — {todays_completed}/{}", todays_tasks.len()))
            .fill(cells[1][0].size())
            .show(ui, theme, |ui| {
                if todays_tasks.is_empty() {
                    ui.label(RichText::new("Nothing due today.").font(theme.small_font()).color(p.text_faint));
                }
                for task in &todays_tasks {
                    ui.horizontal(|ui| {
                        let mut completed = task.completed;
                        if TaskItem::new(&task.title, &mut completed).show(ui, theme).clicked() {
                            toggle_id = Some((task.id, completed));
                        }
                        let mine: Vec<&crate::tasks::TimeBlock> =
                            today_blocks.iter().filter(|b| b.task_id == task.id).collect();
                        if !mine.is_empty() {
                            ui.label(
                                RichText::new(crate::tasks::block::format_block_times(&mine))
                                    .font(theme.small_font())
                                    .color(p.text_muted),
                            );
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
    });

    in_cell(ui, cells[1][1], |ui| {
        Panel::new().title("Today's Schedule").fill(cells[1][1].size()).show(ui, theme, |ui| {
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
                if let Some(color) = accent_for(event.subject_id) {
                    card = card.accent_color(color);
                }
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
}

/// Page 2, "focus": Timer and Pomodoro side by side, the Stopwatch
/// filling the row beneath (or, while a session is running and the
/// Stopwatch is hidden, the first row simply takes the whole page).
fn focus_page(
    ui: &mut egui::Ui,
    theme: &Theme,
    db: Option<&Database>,
    state: &mut DashboardState,
    pomodoro: &mut PomodoroState,
    timer: &mut TimerState,
    revision_session: &mut Option<crate::revision_session::ActiveSession>,
) {
    let (label_area, body) = take_top(ui.max_rect(), LABEL_H);
    in_cell(ui, label_area, |ui| section_label(ui, theme, "focus"));
    let Some(db) = db else {
        return;
    };

    let show_stopwatch = revision_session.is_none();
    let (top, bottom) = if show_stopwatch {
        let rows = pager::grid(body, &[1.35, 1.0], &[1.0], CELL_GAP);
        (rows[0][0], Some(rows[1][0]))
    } else {
        (body, None)
    };
    let columns = pager::grid(top, &[1.0], &[1.0, 1.0], CELL_GAP);
    in_cell(ui, columns[0][0], |ui| timer_panel(ui, theme, db, timer, columns[0][0].size()));
    in_cell(ui, columns[0][1], |ui| focus_panel(ui, theme, db, pomodoro, columns[0][1].size()));
    if let Some(bottom) = bottom {
        in_cell(ui, bottom, |ui| stopwatch_panel(ui, theme, db, state, revision_session, bottom.size()));
    }
}

/// Page 3, "stats": the three headline figures in a strip across the
/// top, then the task heatmap beside hours-per-subject, and the
/// weekly line chart across the bottom.
fn stats_page(ui: &mut egui::Ui, theme: &Theme, db: Option<&Database>, state: &mut DashboardState) {
    let p = &theme.palette;
    let (label_area, body) = take_top(ui.max_rect(), LABEL_H);
    in_cell(ui, label_area, |ui| section_label(ui, theme, "stats"));
    let Some(db) = db else {
        return;
    };

    // Tall enough for the panel's border and padding, its title, and a
    // heading-size figure without the cell having to scroll.
    let (strip, rest) = take_top(body, 104.0);
    let rest = take_top(rest, CELL_GAP).1;

    let focus_minutes = queries::pomodoro::total_minutes(&db.conn).unwrap_or(0);
    let pomodoros_completed = queries::pomodoro::count_sessions(&db.conn).unwrap_or(0);
    // Feature request: "Flashcards" — all-time count of cards
    // whose answer has been revealed during study, from the
    // append-only `flashcards_seen_log` (same pattern as the two
    // Pomodoro stats above).
    let flashcards_seen = queries::flashcards::total_seen(&db.conn).unwrap_or(0);

    let figures = pager::grid(strip, &[1.0], &[1.0, 1.0, 1.0], CELL_GAP);
    let figure_values = [
        ("Focus time", format!("{}h {}m", focus_minutes / 60, focus_minutes % 60)),
        ("Pomodoros completed", pomodoros_completed.to_string()),
        ("Flashcards seen", flashcards_seen.to_string()),
    ];
    for (cell, (title, value)) in figures[0].iter().zip(figure_values) {
        in_cell(ui, *cell, |ui| {
            Panel::new().title(title).fill(cell.size()).show(ui, theme, |ui| {
                let response = ui.label(RichText::new(value).font(theme.heading_font()).color(p.text));
                if title == "Focus time" {
                    response.on_hover_text("Pomodoro, stopwatch and timer sessions combined");
                }
            });
        });
    }

    let rows = pager::grid(rest, &[1.0, 1.0], &[1.0], CELL_GAP);
    let upper = pager::grid(rows[0][0], &[1.0], &[0.8, 1.2, 0.9], CELL_GAP);

    in_cell(ui, upper[0][0], |ui| {
        Panel::new().title("Tasks done").fill(upper[0][0].size()).show(ui, theme, |ui| {
            tasks_heatmap(ui, theme, &crate::stats::tasks_done_by_day(db));
        });
    });

    in_cell(ui, upper[0][1], |ui| {
        Panel::new().title("Hours per subject").fill(upper[0][1].size()).show(ui, theme, |ui| {
            ui.horizontal(|ui| {
                for period in [crate::stats::HoursPeriod::Week, crate::stats::HoursPeriod::Month, crate::stats::HoursPeriod::Year] {
                    let selected = state.hours_period == period;
                    if ui.selectable_label(selected, period.label()).clicked() {
                        state.hours_period = period;
                    }
                }
            });
            ui.add_space(Spacing::XS);
            subject_hours_chart(ui, theme, db, state.hours_period);
        });
    });

    // The pie follows the period chosen in the bar chart's selector
    // beside it, so the two always show the same window.
    in_cell(ui, upper[0][2], |ui| {
        Panel::new()
            .title(&format!("Time per subject — {}", state.hours_period.label()))
            .fill(upper[0][2].size())
            .show_sized(ui, theme, |ui, visible| {
                subject_time_pie(ui, theme, db, state.hours_period, visible);
            });
    });

    in_cell(ui, rows[1][0], |ui| {
        Panel::new().title("Total time by week").fill(rows[1][0].size()).show_sized(ui, theme, |ui, visible| {
            total_time_by_week_chart(ui, theme, db, visible);
        });
    });
}

/// Feature request: task "bars" — the tasks the optional `task` field on the
/// Pomodoro, Timer and Stopwatch can pick: unfinished ones that do the work
/// themselves (not a parent whose subtasks carry it, nor a template).
fn task_choices(db: &Database) -> Vec<crate::tasks::Task> {
    let all = queries::tasks::list_tasks(&db.conn).unwrap_or_default();
    let containers = queries::auto_assign::container_ids(&all);
    all.into_iter().filter(|t| !t.completed && !t.plan_topics_ahead && !containers.contains(&t.id)).collect()
}

/// The title of the picked task, shortened to fit a small card.
fn task_label(tasks: &[crate::tasks::Task], id: Option<i64>) -> String {
    let Some(task) = id.and_then(|id| tasks.iter().find(|t| t.id == id)) else { return "No task".to_owned() };
    const MAX: usize = 28;
    if task.title.chars().count() <= MAX {
        task.title.clone()
    } else {
        format!("{}…", task.title.chars().take(MAX - 1).collect::<String>())
    }
}

/// The optional `task` field: pick a task and the time this run takes is
/// logged against it (and fills its bar).
fn task_picker(ui: &mut egui::Ui, theme: &Theme, id_salt: &str, selected: &mut Option<i64>, tasks: &[crate::tasks::Task]) {
    ui.label(RichText::new("Task").font(theme.small_font()).color(theme.palette.text_faint));
    egui::ComboBox::from_id_salt(id_salt).selected_text(task_label(tasks, *selected)).width(150.0).show_ui(ui, |ui| {
        ui.selectable_value(selected, None, "No task");
        for task in tasks {
            ui.selectable_value(selected, Some(task.id), task_label(tasks, Some(task.id)));
        }
    });
}

/// A section's small uppercase label ("TODAY", "FOCUS", "STATS").
fn section_label(ui: &mut egui::Ui, theme: &Theme, label: &str) {
    ui.label(RichText::new(label.to_uppercase()).font(theme.small_font()).color(theme.palette.text_muted));
    ui.add_space(Spacing::SM);
}

/// The compact Pomodoro card. Pressing Start immediately enters
/// full-screen focus mode (rendered separately by `app::mod`, which
/// intercepts rendering entirely while `pomodoro.focused` is set) —
/// this card only ever needs to show the non-focused controls, since
/// the Dashboard itself never renders while focused.
fn focus_panel(ui: &mut egui::Ui, theme: &Theme, db: &Database, pomodoro: &mut PomodoroState, size: Vec2) {
    let p = &theme.palette;
    let subjects = queries::subjects::list_active_subjects(&db.conn).unwrap_or_default();
    let subject_label = |id: Option<i64>| -> &str {
        id.and_then(|id| subjects.iter().find(|s| s.id == id)).map(|s| s.name.as_str()).unwrap_or("No subject")
    };
    let tasks = task_choices(db);
    Panel::new().title("Pomodoro").fill(size).show(ui, theme, |ui| {
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
                    if pomodoro.task_id.is_some() {
                        ui.label(
                            RichText::new(format!("Task: {}", task_label(&tasks, pomodoro.task_id)))
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
                    ui.horizontal(|ui| {
                        task_picker(ui, theme, "pomodoro_task_picker", &mut pomodoro.task_id, &tasks);
                    });
                    ui.add_space(Spacing::XS);
                    ui.horizontal(|ui| {
                        let is_resume = pomodoro.remaining < pomodoro.phase_total();
                        let label = if is_resume { "Resume" } else { "Start" };
                        if RButton::new(label).primary().show(ui, theme).clicked() {
                            pomodoro.start();
                            // Feature request: "Notifications
                            // Extended" — "notifications... should be
                            // created when starting a study session,
                            // event or pomodoro timer." Distinct from
                            // (and in addition to) the existing
                            // phase-completion notifications in
                            // `AppState::tick_pomodoro` — this fires
                            // right away, at the click, rather than
                            // minutes later at the end of the phase.
                            let phase_label = match pomodoro.phase {
                                Phase::Work => "Focus session",
                                Phase::Break => "Break",
                            };
                            let minutes = match pomodoro.phase {
                                Phase::Work => pomodoro.work_minutes,
                                Phase::Break => pomodoro.break_minutes,
                            };
                            crate::notifications::send(
                                Some(&db.conn),
                                &format!("{phase_label} {}", if is_resume { "resumed" } else { "started" }),
                                &format!("{minutes} minutes."),
                            );
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

/// Feature request: "There should also be a timer. Like the pomodoro
/// but with a customisable time ... like the default iOS Clock app,
/// with a scroll select for hours, minutes, seconds." While no
/// countdown is running, this shows three wheels (see
/// `components::scroll_select`) to set the length, an optional
/// subject (as on the Pomodoro card) and Start. While one is running
/// it shows the same circular countdown the Pomodoro card uses, with
/// Pause/Resume and Cancel. A finished timer is logged as focus time
/// by `AppState::tick_timer`.
fn timer_panel(ui: &mut egui::Ui, theme: &Theme, db: &Database, timer: &mut TimerState, size: Vec2) {
    let p = &theme.palette;
    let subjects = queries::subjects::list_active_subjects(&db.conn).unwrap_or_default();
    let subject_label = |id: Option<i64>| -> &str {
        id.and_then(|id| subjects.iter().find(|s| s.id == id)).map(|s| s.name.as_str()).unwrap_or("No subject")
    };
    let tasks = task_choices(db);

    Panel::new().title("Timer").fill(size).show(ui, theme, |ui| {
        if timer.active {
            ui.horizontal(|ui| {
                pomodoro_circle::show(
                    ui,
                    theme,
                    timer.progress(),
                    &crate::timer::format_remaining(timer.remaining),
                    "Timer",
                    40.0,
                );
                ui.add_space(Spacing::MD);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(format!("{} timer", crate::timer::format_remaining(timer.total)))
                            .font(theme.small_font())
                            .color(p.text_faint),
                    );
                    if timer.subject_id.is_some() {
                        ui.label(
                            RichText::new(format!("Logging to: {}", subject_label(timer.subject_id)))
                                .font(theme.small_font())
                                .color(p.text_faint),
                        );
                    }
                    if timer.task_id.is_some() {
                        ui.label(
                            RichText::new(format!("Task: {}", task_label(&tasks, timer.task_id)))
                                .font(theme.small_font())
                                .color(p.text_faint),
                        );
                    }
                    ui.add_space(Spacing::XS);
                    ui.horizontal(|ui| {
                        if timer.running {
                            if RButton::new("Pause").show(ui, theme).clicked() {
                                timer.pause();
                            }
                        } else if RButton::new("Resume").primary().show(ui, theme).clicked() {
                            timer.resume();
                        }
                        if ui.small_button("Cancel").clicked() {
                            // A cancelled run started against a planned block gives it back.
                            if let Some(block_id) = timer.linked_block {
                                let _ = crate::database::queries::block_sessions::unlink_block(&db.conn, block_id);
                            }
                            timer.cancel();
                        }
                    });
                });
            });
            return;
        }

        ui.horizontal(|ui| {
            scroll_select::show(ui, theme, "timer_hours", &mut timer.hours, 24, "hours");
            ui.add_space(Spacing::SM);
            scroll_select::show(ui, theme, "timer_minutes", &mut timer.minutes, 60, "min");
            ui.add_space(Spacing::SM);
            scroll_select::show(ui, theme, "timer_seconds", &mut timer.seconds, 60, "sec");
        });
        ui.add_space(Spacing::SM);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Subject").font(theme.small_font()).color(p.text_faint));
            egui::ComboBox::from_id_salt("timer_subject_picker").selected_text(subject_label(timer.subject_id)).show_ui(
                ui,
                |ui| {
                    ui.selectable_value(&mut timer.subject_id, None, "No subject");
                    for subject in &subjects {
                        ui.selectable_value(&mut timer.subject_id, Some(subject.id), subject.name.clone());
                    }
                },
            );
        });
        ui.horizontal(|ui| {
            task_picker(ui, theme, "timer_task_picker", &mut timer.task_id, &tasks);
        });
        ui.add_space(Spacing::XS);
        let can_start = timer.can_start();
        ui.add_enabled_ui(can_start, |ui| {
            if RButton::new("Start Timer").primary().show(ui, theme).clicked() {
                timer.start();
                // Started near a planned block? Then this run is that block's work.
                let linked = crate::database::queries::block_sessions::link_session_start_for(
                    &db.conn,
                    chrono::Local::now(),
                    timer.subject_id,
                    timer.task_id,
                )
                .ok()
                .flatten();
                if let Some(link) = &linked {
                    timer.linked_block = Some(link.block_id);
                    if timer.subject_id.is_none() {
                        timer.link_subject = link.subject_id;
                    }
                }
                crate::notifications::send(
                    Some(&db.conn),
                    "Timer started",
                    &match &linked {
                        Some(link) => format!(
                            "{} — planned block: {}",
                            crate::timer::format_remaining(timer.total),
                            link.task_title
                        ),
                        None => crate::timer::format_remaining(timer.total),
                    },
                );
            }
        });
        if !can_start {
            ui.label(RichText::new("Pick a length first").font(theme.small_font()).color(p.text_faint));
        }
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
    size: Vec2,
) {
    if revision_session.is_some() {
        return;
    }

    let p = &theme.palette;
    let subjects = queries::subjects::list_active_subjects(&db.conn).unwrap_or_default();
    let tasks = task_choices(db);

    Panel::new().title("Stopwatch").fill(size).show(ui, theme, |ui| {
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
        ui.horizontal(|ui| {
            task_picker(ui, theme, "stopwatch_task_picker", &mut state.stopwatch_task_id, &tasks);
        });
        ui.add_space(Spacing::XS);
        // A picked task can stand in for the title: timing "Essay" needs no second name.
        let picked_task = state.stopwatch_task_id.and_then(|id| tasks.iter().find(|t| t.id == id));
        let title = match (state.stopwatch_title.trim(), picked_task) {
            ("", Some(task)) => task.title.clone(),
            (typed, _) => typed.to_owned(),
        };
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
                // Feature request: task "bars" — the time is logged against the picked task,
                // and a run with no subject of its own counts toward the task's.
                if let Some(session) = revision_session.as_mut() {
                    session.task_id = state.stopwatch_task_id;
                    if session.subject_id.is_none() {
                        if let Some(subject_id) =
                            state.stopwatch_task_id.and_then(|id| queries::task_time::task_subject(&db.conn, id).ok().flatten())
                        {
                            session.subject_id = Some(subject_id);
                            session.subject_name =
                                subjects.iter().find(|s| s.id == subject_id).map(|s| s.name.clone());
                        }
                    }
                }
                // Feature request: "Notifications Extended" —
                // "notifications... should be created when starting a
                // study session, event or pomodoro timer." This is
                // the "study session" case — a session started
                // without a Calendar event; `start_revision_session`
                // in `app::mod` covers the "event" case the same way.
                // Started near a planned block? Then this stopwatch is that block's work,
                // and takes the task's subject if none was picked so the time counts.
                let linked = revision_session.as_mut().and_then(|session| {
                    let link = crate::database::queries::block_sessions::link_session_start_for(
                        &db.conn,
                        session.started_at,
                        session.subject_id,
                        session.task_id,
                    )
                    .ok()
                    .flatten()?;
                    session.linked_block = Some(link.block_id);
                    if session.subject_id.is_none() {
                        if let Some(subject_id) = link.subject_id {
                            session.subject_id = Some(subject_id);
                            session.subject_name =
                                subjects.iter().find(|s| s.id == subject_id).map(|s| s.name.clone());
                        }
                    }
                    Some(link)
                });
                let body = match &linked {
                    Some(link) => format!("{title} — planned block: {}", link.task_title),
                    None => title.clone(),
                };
                crate::notifications::send(Some(&db.conn), "Stopwatch started", &body);
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

/// Per-subject logged minutes in `period`, longest first, with the
/// subject list their colours come from. Shared by the bar chart and
/// the pie chart (each now sits in its own cell) so the two always
/// describe the same data. Each row keeps its subject id rather than a
/// resolved colour, so every chart looks the colour up itself.
fn subject_hours_rows(
    db: &Database,
    period: crate::stats::HoursPeriod,
) -> (Vec<Subject>, Vec<(i64, String, i64)>) {
    let today = Local::now().date_naive();
    let (start, end) = crate::stats::period_bounds(period, today);
    let totals = crate::stats::minutes_per_subject_rolled_up(db, start, end);
    let subjects = queries::subjects::list_all_subjects(&db.conn).unwrap_or_default();
    let mut rows: Vec<(i64, String, i64)> = totals
        .into_iter()
        .map(|(id, minutes)| {
            let name = subjects.iter().find(|s| s.id == id).map(|s| s.name.clone()).unwrap_or_default();
            (id, name, minutes)
        })
        .collect();
    rows.sort_by(|a, b| b.2.cmp(&a.2));
    (subjects, rows)
}

fn no_hours_label(ui: &mut egui::Ui, theme: &Theme, period: crate::stats::HoursPeriod) {
    ui.label(
        RichText::new(format!("No hours logged {}.", period.label().to_lowercase()))
            .font(theme.small_font())
            .color(theme.palette.text_faint),
    );
}

/// A horizontal bar per subject with logged hours in `period`,
/// longest first (feature request: "Improved Hours Per Subject
/// View" — selectable time window, defaulting to this-week). The
/// pie chart of the same data ("Time Per Subject") is
/// `subject_time_pie`, in the cell beside this one.
fn subject_hours_chart(ui: &mut egui::Ui, theme: &Theme, db: &Database, period: crate::stats::HoursPeriod) {
    let p = &theme.palette;
    let (subjects, rows) = subject_hours_rows(db, period);
    if rows.is_empty() {
        no_hours_label(ui, theme, period);
        return;
    }

    let max_minutes = rows.iter().map(|(_, _, m)| *m).max().unwrap_or(1).max(1);
    // Scales with the cell the chart is in, rather than a fixed 220.
    let bar_max_width = (ui.available_width() - 190.0).clamp(60.0, 360.0);

    // Compact rows (egui's default is an 18pt minimum plus 3pt of
    // spacing) so a handful of subjects fit the cell without scrolling.
    ui.spacing_mut().interact_size.y = 16.0;
    ui.spacing_mut().item_spacing.y = 2.0;
    for (id, name, minutes) in &rows {
        ui.horizontal(|ui| {
            ui.add_sized([90.0, 0.0], egui::Label::new(RichText::new(name).font(theme.small_font()).color(p.text)));
            let width = (*minutes as f32 / max_minutes as f32) * bar_max_width;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width.max(2.0), 14.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, egui::CornerRadius::same(1), crate::theme::subject_color(&subjects, *id));
            ui.label(
                RichText::new(format!("{}h {}m", minutes / 60, minutes % 60))
                    .font(theme.small_font())
                    .color(p.text_faint),
            );
        });
    }
}

/// The "Time Per Subject" pie chart for `period`, sized to `visible`
/// (the cell's content box — see `Panel::show_sized`).
fn subject_time_pie(ui: &mut egui::Ui, theme: &Theme, db: &Database, period: crate::stats::HoursPeriod, visible: Vec2) {
    let (subjects, rows) = subject_hours_rows(db, period);
    if rows.is_empty() {
        no_hours_label(ui, theme, period);
        return;
    }
    pie_chart(ui, theme, &subjects, &rows, visible);
}

/// A hand-drawn line chart of logged minutes per week, one line per
/// subject (feature request: "Improved Line Graph" — "instead of the
/// line representing the total time, show a line for each subject,
/// coloured accordingly"), plus the week-over-week comparison message
/// for the combined total (unchanged from before — the request asked
/// to change how the lines are drawn, not to drop the summary). 8
/// weeks — enough to see a real trend without cramming the Dashboard.
/// Each subject's line uses `theme::subject_color` (originally this
/// function's own one-off `subject_line_color`, generalized by the
/// later "Colour" feature request to every chart plus Calendar
/// events), so a subject's line here is the same color as its bar in
/// `subject_hours_chart`, its slice in `pie_chart`, and its events on
/// Calendar.
fn total_time_by_week_chart(ui: &mut egui::Ui, theme: &Theme, db: &Database, visible: Vec2) {
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
    // Feature request: "Week Over Week Change" — compares this week
    // so far against the same span of last week (this Thursday vs.
    // last Thursday), not last week's full total, so a new week
    // doesn't open with "down 90%". Computed by `stats` directly
    // rather than from `totals` above, since the chart's own weekly
    // totals are always whole weeks.
    let message = crate::stats::week_over_week_message(db, chrono::Local::now().date_naive());
    if let Some(message) = &message {
        ui.label(RichText::new(message).font(theme.label_font()).color(p.text));
        ui.add_space(Spacing::XS);
    }

    // Fills the cell (previously a fixed 320).
    let chart_width = ui.available_width().clamp(160.0, 720.0);
    let max_minutes = totals.iter().map(|(_, m)| *m).max().unwrap_or(1).max(1) as f32;

    // Every subject with at least one logged minute somewhere in the
    // window gets its own line — sorted by id purely so the legend
    // and draw order are stable frame to frame, not because id order
    // means anything.
    let mut subject_ids: Vec<i64> =
        weeks.iter().flat_map(|(_, by_subject)| by_subject.keys().copied()).collect::<HashSet<_>>().into_iter().collect();
    subject_ids.sort_unstable();

    let subjects = queries::subjects::list_all_subjects(&db.conn).unwrap_or_default();

    // Sized to what is left of the cell once the message, the date row
    // and the legend have taken theirs (it used to be a fixed 80pt, which
    // overflowed a short cell whenever last week had time logged and the
    // message above appeared).
    let chart_height = week_chart_height(
        visible.y,
        week_message_rows(message.is_some(), visible.x),
        week_legend_rows(subject_ids.len(), visible.x),
    );
    let (rect, _) = ui.allocate_exact_size(egui::vec2(chart_width, chart_height), egui::Sense::hover());
    let painter = ui.painter();
    painter.hline(rect.left()..=rect.right(), rect.bottom(), egui::Stroke::new(1.0, p.border));

    for &subject_id in &subject_ids {
        let values: Vec<f32> = weeks.iter().map(|(_, by_subject)| *by_subject.get(&subject_id).unwrap_or(&0) as f32).collect();
        let color = crate::theme::subject_color(&subjects, subject_id);
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
            // Every other label, to avoid crowding 8 dates into one
            // row.
            let text = if index % 2 == 0 { week_start.format("%d %b").to_string() } else { String::new() };
            ui.add_sized(
                [chart_width / NUM_WEEKS as f32, 0.0],
                egui::Label::new(RichText::new(text).font(theme.small_font()).color(p.text_faint)),
            );
        }
    });

    // Legend — without it the colors alone don't say which subject is
    // which.
    if !subject_ids.is_empty() {
        ui.add_space(Spacing::XS);
        ui.horizontal_wrapped(|ui| {
            for &subject_id in &subject_ids {
                let name = subjects.iter().find(|s| s.id == subject_id).map(|s| s.name.as_str()).unwrap_or("Unknown");
                let color = crate::theme::subject_color(&subjects, subject_id);
                let (swatch_rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().rect_filled(swatch_rect, egui::CornerRadius::same(1), color);
                ui.label(RichText::new(name).font(theme.small_font()).color(p.text_faint));
                ui.add_space(Spacing::SM);
            }
        });
    }
}

/// Measured heights (in a real layout pass, not estimated) of the rows
/// the weekly chart stacks around the chart itself: the week-over-week
/// sentence with its gap, the row of date labels with its gap, and one
/// legend row with its gap. A little slack covers rounding.
const WEEK_MESSAGE_ROW: f32 = 28.0;
const WEEK_DATE_ROW: f32 = 38.0;
const WEEK_LEGEND_ROW: f32 = 30.0;
const WEEK_SLACK: f32 = 4.0;
/// The weekly chart is never drawn shorter or taller than this. When the
/// rows around it crowd it down to the minimum the cell scrolls rather
/// than the chart shrinking to nothing.
const WEEK_CHART_MIN: f32 = 36.0;
const WEEK_CHART_MAX: f32 = 80.0;
/// Roughly how wide one legend entry (swatch, name, gap) is.
const WEEK_LEGEND_ENTRY_WIDTH: f32 = 96.0;
/// Below this width the week-over-week sentence is assumed to wrap onto
/// a second line.
const WEEK_MESSAGE_WRAP_WIDTH: f32 = 700.0;

/// How many text rows the week-over-week message takes: none without a
/// message, one in a wide cell, two where it will wrap. An estimate —
/// erring high only makes the chart a little shorter.
fn week_message_rows(has_message: bool, width: f32) -> usize {
    match (has_message, width >= WEEK_MESSAGE_WRAP_WIDTH) {
        (false, _) => 0,
        (true, true) => 1,
        (true, false) => 2,
    }
}

/// How many rows the wrapped legend takes for `subjects` entries in a
/// cell `width` wide (at least one entry per row).
fn week_legend_rows(subjects: usize, width: f32) -> usize {
    if subjects == 0 {
        return 0;
    }
    let per_row = ((width / WEEK_LEGEND_ENTRY_WIDTH).floor() as usize).max(1);
    subjects.div_ceil(per_row)
}

/// The weekly chart's height in a cell `visible_height` tall, once the
/// message rows, the date-label row and the legend rows have been taken
/// out, kept between the min and max.
fn week_chart_height(visible_height: f32, message_rows: usize, legend_rows: usize) -> f32 {
    let fixed = message_rows as f32 * WEEK_MESSAGE_ROW
        + WEEK_DATE_ROW
        + legend_rows as f32 * WEEK_LEGEND_ROW
        + WEEK_SLACK;
    (visible_height - fixed).clamp(WEEK_CHART_MIN, WEEK_CHART_MAX)
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
/// alongside the existing bar chart, same data). Slices used to be
/// shaded in a gray gradient (`pie_slice_color`, now removed) rather
/// than distinct hues, back when this app's design system was
/// explicitly monochrome throughout; the "Colour" feature request
/// asked for per-subject color on "the rest of the statistics" too,
/// so this now draws each slice (and its legend dot) in that
/// subject's own `theme::subject_color` — the same color the bar
/// chart above and the per-subject line graph below already use for
/// it, so a subject reads as one consistent color across every chart
/// on the Dashboard.
fn pie_chart(ui: &mut egui::Ui, theme: &Theme, subjects: &[Subject], rows: &[(i64, String, i64)], visible: Vec2) {
    let p = &theme.palette;
    let total: i64 = rows.iter().map(|(_, _, m)| *m).sum();
    if total <= 0 {
        return;
    }

    // Sized to the cell it sits in (it used to be a fixed 140pt):
    // as large as fits above its legend, centred, so it reads as part
    // of the page's grid rather than a widget dropped into it.
    let diameter = pie_diameter(visible.x, visible.y, rows.len());
    let radius = diameter / 2.0;

    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(diameter, diameter), egui::Sense::hover());
        let center = rect.center();
        let painter = ui.painter();

        let mut cumulative_frac = 0.0_f32;
        for (id, _, minutes) in rows {
            let slice_frac = *minutes as f32 / total as f32;
            let color = crate::theme::subject_color(subjects, *id);
            let points = pie_slice_points(center, radius, cumulative_frac, cumulative_frac + slice_frac);
            painter.add(egui::Shape::convex_polygon(points, color, egui::Stroke::new(1.5, p.background)));
            cumulative_frac += slice_frac;
        }
    });

    ui.add_space(PIE_LEGEND_GAP);
    // Compact rows: egui's default is an 18pt minimum row plus 3pt of
    // spacing, which would eat the pie's room for no benefit.
    ui.spacing_mut().interact_size.y = PIE_LEGEND_ROW;
    ui.spacing_mut().item_spacing.y = 0.0;
    for (id, name, minutes) in rows {
        let color = crate::theme::subject_color(subjects, *id);
        let percent = (*minutes as f64 / total as f64 * 100.0).round() as i64;
        ui.horizontal(|ui| {
            let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
            ui.painter().circle_filled(dot_rect.center(), 4.0, color);
            ui.label(RichText::new(format!("{name} — {percent}%")).font(theme.small_font()).color(p.text));
        });
    }
}

/// Height of one legend row, and the gap between pie and legend.
const PIE_LEGEND_ROW: f32 = 16.0;
const PIE_LEGEND_GAP: f32 = Spacing::SM;
/// Room for the spacing `egui` inserts between the pie and the legend
/// and around the rows, which the constants above don't count.
const PIE_SLACK: f32 = 8.0;
/// The pie is never drawn smaller or larger than this.
const PIE_MIN_DIAMETER: f32 = 72.0;
const PIE_MAX_DIAMETER: f32 = 280.0;

/// The pie's diameter in a cell with `width` x `height` visible and a
/// legend of `legend_rows` rows beneath it: the largest circle that
/// fits across and above the legend, kept between the min and max.
/// When the legend crowds the pie down to the minimum the cell scrolls
/// rather than the pie shrinking to nothing.
fn pie_diameter(width: f32, height: f32, legend_rows: usize) -> f32 {
    let legend = legend_rows as f32 * PIE_LEGEND_ROW + PIE_LEGEND_GAP + PIE_SLACK;
    width.min(height - legend).clamp(PIE_MIN_DIAMETER, PIE_MAX_DIAMETER)
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
    fn pie_diameter_takes_the_largest_circle_that_fits_above_the_legend() {
        // Width-limited: a narrow, tall cell.
        assert_eq!(pie_diameter(150.0, 600.0, 3), 150.0);
        // Height-limited: a wide, short cell loses the legend's height.
        let d = pie_diameter(600.0, 200.0, 3);
        assert!((d - (200.0 - 3.0 * PIE_LEGEND_ROW - PIE_LEGEND_GAP - PIE_SLACK)).abs() < 1e-4);
        // More legend rows leave less room for the pie.
        assert!(pie_diameter(600.0, 300.0, 6) < pie_diameter(600.0, 300.0, 2));
    }

    #[test]
    fn pie_diameter_stays_within_its_bounds() {
        assert_eq!(pie_diameter(2000.0, 2000.0, 1), PIE_MAX_DIAMETER);
        // A cramped cell (legend taller than the cell) floors at the
        // minimum and lets the cell scroll, never zero or negative.
        assert_eq!(pie_diameter(300.0, 50.0, 8), PIE_MIN_DIAMETER);
        assert_eq!(pie_diameter(10.0, 600.0, 2), PIE_MIN_DIAMETER);
        assert_eq!(pie_diameter(0.0, 0.0, 0), PIE_MIN_DIAMETER);
    }

    #[test]
    fn pie_slice_points_more_steps_for_a_bigger_slice() {
        let center = egui::pos2(0.0, 0.0);
        let small = pie_slice_points(center, 10.0, 0.0, 0.1);
        let big = pie_slice_points(center, 10.0, 0.0, 0.9);
        assert!(big.len() > small.len(), "a near-full circle needs more segments than a sliver to look round");
    }
}

/// Headless render tests for the paged layout: real `egui` layout
/// (no GPU needed — `Context::run_ui` just produces shapes), checking
/// what actually got drawn rather than only the pure arithmetic in
/// `ui::pager`.
#[cfg(test)]
mod paged_tests {
    use super::*;
    use crate::app::view::View;
    use crate::ui::calendar::CalendarState;

    struct Harness {
        ctx: egui::Context,
        theme: Theme,
        db: Database,
        state: DashboardState,
        pomodoro: PomodoroState,
        timer: TimerState,
        revision_session: Option<crate::revision_session::ActiveSession>,
        calendar: CalendarState,
        view: View,
        time: f64,
    }

    impl Harness {
        fn new() -> Self {
            let ctx = egui::Context::default();
            let theme = Theme::new();
            theme.apply(&ctx);
            Self {
                ctx,
                theme,
                db: Database::open_in_memory().unwrap(),
                state: DashboardState::default(),
                pomodoro: PomodoroState::default(),
                timer: TimerState::default(),
                revision_session: None,
                calendar: CalendarState::default(),
                view: View::Dashboard,
                time: 100.0,
            }
        }

        /// One frame `dt` seconds after the last, on a `size` screen.
        fn frame(&mut self, size: Vec2, dt: f64, events: Vec<egui::Event>, pointer: Option<egui::Pos2>) -> egui::FullOutput {
            self.time += dt;
            let mut events = events;
            if let Some(pos) = pointer {
                events.insert(0, egui::Event::PointerMoved(pos));
            }
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let Self { ctx, theme, db, state, pomodoro, timer, revision_session, calendar, view, .. } = self;
            let mut output = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
                    show(ui, theme, Some(db), state, pomodoro, timer, revision_session, calendar, view, None, true);
                });
            });
            // There is no renderer to upload textures to; egui asserts
            // that pending texture updates are handled before the
            // frame's output is dropped.
            output.textures_delta.clear();
            output
        }

        /// Frames until the slide animation has settled.
        fn settle(&mut self, size: Vec2) -> egui::FullOutput {
            let mut out = self.frame(size, 0.016, vec![], None);
            for _ in 0..60 {
                out = self.frame(size, 0.016, vec![], None);
            }
            out
        }

        fn wheel(&mut self, size: Vec2, delta_y: f32, pointer: Option<egui::Pos2>) {
            let event = egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, delta_y),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
            };
            self.frame(size, 0.016, vec![event], pointer);
            // Let egui's wheel smoothing play out.
            for _ in 0..10 {
                self.frame(size, 0.016, vec![], pointer);
            }
        }
    }

    fn flatten<'a>(shape: &'a egui::Shape, out: &mut Vec<&'a egui::Shape>) {
        if let egui::Shape::Vec(inner) = shape {
            for s in inner {
                flatten(s, out);
            }
        } else {
            out.push(shape);
        }
    }

    fn all_text(output: &egui::FullOutput) -> String {
        let mut shapes = Vec::new();
        for clipped in &output.shapes {
            flatten(&clipped.shape, &mut shapes);
        }
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Bounding box of every `Panel` frame drawn (the rects filled with
    /// the surface colour and outlined), excluding the tiny heatmap cells.
    fn panel_bounds(output: &egui::FullOutput, theme: &Theme) -> Option<Rect> {
        let mut shapes = Vec::new();
        for clipped in &output.shapes {
            flatten(&clipped.shape, &mut shapes);
        }
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r)
                    if r.fill == theme.palette.surface && r.stroke.width >= 1.0 && r.rect.width() > 60.0 =>
                {
                    Some(r.rect)
                }
                _ => None,
            })
            .reduce(|a, b| a.union(b))
    }

    const SIZES: [(f32, f32); 3] = [(960.0, 640.0), (640.0, 420.0), (1600.0, 900.0)];

    #[test]
    fn every_page_renders_at_common_window_sizes() {
        for (w, h) in SIZES {
            let size = egui::vec2(w, h);
            for page in 0..PAGE_COUNT {
                let mut harness = Harness::new();
                harness.state.pager.page = page;
                let out = harness.settle(size);
                assert!(!out.shapes.is_empty(), "page {page} drew nothing at {w}x{h}");
            }
        }
    }

    #[test]
    fn only_the_current_page_is_drawn_at_rest() {
        let size = egui::vec2(960.0, 640.0);
        let mut harness = Harness::new();
        for (page, own, others) in [
            (0, "TODAY'S TASKS", ["STOPWATCH", "HOURS PER SUBJECT"]),
            (1, "STOPWATCH", ["TODAY'S TASKS", "HOURS PER SUBJECT"]),
            (2, "HOURS PER SUBJECT", ["TODAY'S TASKS", "STOPWATCH"]),
        ] {
            harness.state.pager.page = page;
            let text = all_text(&harness.settle(size));
            assert!(text.contains(own), "page {page} should show its own label; got:\n{text}");
            for other in others {
                assert!(!text.contains(other), "page {page} must not draw the {other} page; got:\n{text}");
            }
        }
    }

    #[test]
    fn mid_slide_two_pages_are_drawn_and_never_three() {
        let size = egui::vec2(960.0, 640.0);
        let mut harness = Harness::new();
        harness.settle(size);
        harness.state.pager.page = 1;
        // A few frames in, the slide is part-way (labels of a page that
        // is still mostly below the screen aren't painted at all).
        let mut out = harness.frame(size, 0.05, vec![], None);
        for _ in 0..2 {
            out = harness.frame(size, 0.05, vec![], None);
        }
        let text = all_text(&out);
        assert!(
            text.contains("TODAY'S TASKS") && text.contains("TIMER"),
            "mid-slide shows both pages; got:\n{text}"
        );
        assert!(!text.contains("HOURS PER SUBJECT"));
    }

    #[test]
    fn each_page_fills_the_screen_inside_its_margin() {
        for (w, h) in SIZES {
            let size = egui::vec2(w, h);
            for page in 0..PAGE_COUNT {
                let mut harness = Harness::new();
                harness.state.pager.page = page;
                let out = harness.settle(size);
                let bounds = panel_bounds(&out, &harness.theme).expect("a page has panels");
                let m = Spacing::MD;
                // The panels tile the area right to the margins on the
                // left, right and bottom (the top holds the label).
                assert!((bounds.left() - m).abs() < 1.5, "page {page} {w}x{h}: left {}", bounds.left());
                assert!((bounds.right() - (w - m)).abs() < 1.5, "page {page} {w}x{h}: right {}", bounds.right());
                assert!((bounds.bottom() - (h - m)).abs() < 1.5, "page {page} {w}x{h}: bottom {}", bounds.bottom());
            }
        }
    }

    #[test]
    fn a_scroll_gesture_snaps_to_the_next_page_and_back() {
        let size = egui::vec2(960.0, 640.0);
        let mut harness = Harness::new();
        harness.settle(size);
        assert_eq!(harness.state.pager.page, 0);

        // Park the pointer in the page's margin, over no scrolling cell.
        let margin = Some(egui::pos2(4.0, 4.0));
        harness.wheel(size, -50.0, margin);
        assert_eq!(harness.state.pager.page, 1);
        harness.frame(size, 1.0, vec![], margin); // gesture ends
        harness.wheel(size, -50.0, margin);
        assert_eq!(harness.state.pager.page, 2);
        harness.frame(size, 1.0, vec![], margin);
        harness.wheel(size, -50.0, margin);
        assert_eq!(harness.state.pager.page, 2, "the last page stays put");
        harness.frame(size, 1.0, vec![], margin);
        harness.wheel(size, 50.0, margin);
        assert_eq!(harness.state.pager.page, 1);
    }

    #[test]
    fn one_flick_never_skips_a_page() {
        let size = egui::vec2(960.0, 640.0);
        let mut harness = Harness::new();
        harness.settle(size);
        let margin = Some(egui::pos2(4.0, 4.0));
        // A big, fast flick: lots of wheel input in a short time.
        for _ in 0..6 {
            harness.wheel(size, -200.0, margin);
        }
        assert_eq!(harness.state.pager.page, 1);
    }

    #[test]
    fn the_indicator_appears_on_activity_and_fades_away() {
        let size = egui::vec2(960.0, 640.0);
        let mut harness = Harness::new();
        harness.settle(size); // long enough that the start-up flash is over
        assert_eq!(harness.state.pager.indicator_opacity(harness.time), 0.0);
        harness.wheel(size, -50.0, Some(egui::pos2(4.0, 4.0)));
        assert!(harness.state.pager.indicator_opacity(harness.time) > 0.5);
        harness.settle(size); // wheel smoothing finishes, then silence
        harness.frame(size, 5.0, vec![], None);
        assert_eq!(harness.state.pager.indicator_opacity(harness.time), 0.0);
    }

    #[test]
    fn an_overflowing_cell_scrolls_itself_before_the_page_turns() {
        let size = egui::vec2(640.0, 420.0);
        let mut harness = Harness::new();
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        for i in 0..40 {
            queries::tasks::create_task(&harness.db.conn, &format!("Task {i}"), Some(&today), None, None).unwrap();
        }
        harness.settle(size);

        let reports = pager::take_reports(&harness.ctx);
        let overflowing = reports.iter().find(|r| r.can_down).expect("the tasks cell overflows at this size");
        let over_cell = Some(overflowing.rect.center());
        // Re-render once so the reports are back for the next frame.
        harness.frame(size, 0.016, vec![], over_cell);

        harness.wheel(size, -50.0, over_cell);
        assert_eq!(harness.state.pager.page, 0, "the cell took the scroll; the page must not turn");
    }

    /// Four subjects with an hour or more logged each, `days_ago` days ago
    /// (inside the Year window).
    ///
    /// *Which* day matters: the Total-time-by-week cell shows a
    /// week-over-week sentence only when last week's matching span has
    /// time logged. 7 days ago always falls in that span (the same weekday
    /// last week), so it always shows the sentence; 21 days ago never does;
    /// 10 days ago depends on the weekday — which is how this test came to
    /// pass one day and fail the next. Tests that care use 7 and 21.
    fn log_some_study_time_days_ago(harness: &mut Harness, days_ago: i64) {
        let start = Local::now() - chrono::Duration::days(days_ago);
        for (i, name) in ["Maths", "Physics", "Chemistry", "History"].iter().enumerate() {
            let subject = queries::subjects::create_subject(&harness.db.conn, name).unwrap();
            let end = start + chrono::Duration::minutes(60 + 30 * i as i64);
            queries::events::create_event(&harness.db.conn, "Session", "", start, end, Some(subject.id), None).unwrap();
        }
        harness.state.hours_period = crate::stats::HoursPeriod::Year;
    }

    fn log_some_study_time(harness: &mut Harness) {
        log_some_study_time_days_ago(harness, 10);
    }

    #[test]
    fn the_stats_page_fits_its_cells_without_scrolling_with_and_without_the_week_over_week_message() {
        // 7 days ago: the message is always present. 21 days ago: never.
        // Both must fit, at the small and the comfortable window size.
        for days_ago in [7, 21] {
            for (w, h) in [(960.0, 640.0), (1600.0, 900.0)] {
                let size = egui::vec2(w, h);
                let mut harness = Harness::new();
                log_some_study_time_days_ago(&mut harness, days_ago);
                harness.state.pager.page = 2;
                let out = harness.settle(size);
                let text = all_text(&out);
                let has_message = text.contains("last week");
                assert_eq!(has_message, days_ago == 7, "the message is present exactly when expected ({days_ago} days ago)");

                let reports = pager::take_reports(&harness.ctx);
                let overflowing: Vec<_> = reports.iter().filter(|r| r.can_down).collect();
                assert!(
                    overflowing.is_empty(),
                    "{w}x{h}, logged {days_ago} days ago, message: {has_message}: cells that scroll: {overflowing:?}"
                );
            }
        }
    }

    #[test]
    fn week_chart_height_shrinks_for_each_row_taken_around_it_and_stays_in_bounds() {
        let roomy = week_chart_height(400.0, 1, 1);
        assert_eq!(roomy, WEEK_CHART_MAX, "plenty of room: the full height");
        // Every extra row costs height, down to the floor.
        assert!(week_chart_height(161.0, 1, 1) < week_chart_height(161.0, 0, 1));
        assert!(week_chart_height(161.0, 0, 2) < week_chart_height(161.0, 0, 1));
        assert_eq!(week_chart_height(20.0, 2, 3), WEEK_CHART_MIN, "never shrinks to nothing");
        // The case that overflowed: the 161pt cell at 960x640 with the message and a
        // one-row legend (the chart used to be a fixed 80pt, which made 176pt of content).
        let tight = week_chart_height(161.0, 1, 1);
        assert!(tight >= WEEK_CHART_MIN && tight < WEEK_CHART_MAX);
        let stack = tight + WEEK_MESSAGE_ROW + WEEK_DATE_ROW + WEEK_LEGEND_ROW + WEEK_SLACK;
        assert!(stack <= 161.0 + 1e-4, "the whole stack fits the cell: {stack}");
    }

    #[test]
    fn the_message_and_legend_row_estimates_follow_the_width() {
        assert_eq!(week_message_rows(false, 400.0), 0);
        assert_eq!(week_message_rows(true, 900.0), 1);
        assert_eq!(week_message_rows(true, 400.0), 2, "a narrow cell wraps the sentence");
        assert_eq!(week_legend_rows(0, 900.0), 0);
        assert_eq!(week_legend_rows(4, 900.0), 1);
        assert_eq!(week_legend_rows(4, 200.0), 2);
        assert_eq!(week_legend_rows(5, 10.0), 5, "at least one entry per row");
    }

    #[test]
    fn the_pie_chart_fits_its_cell_without_scrolling_at_normal_sizes() {
        for (w, h) in [(960.0, 640.0), (1600.0, 900.0)] {
            let size = egui::vec2(w, h);
            let mut harness = Harness::new();
            log_some_study_time(&mut harness);
            harness.state.pager.page = 2;
            let out = harness.settle(size);
            let text = all_text(&out);
            assert!(text.contains("TIME PER SUBJECT"), "pie panel present at {w}x{h}");
            assert!(text.contains("Maths") && text.contains("History"), "legend lists the subjects at {w}x{h}");

            let reports = pager::take_reports(&harness.ctx);
            let overflowing: Vec<_> = reports.iter().filter(|r| r.can_down).collect();
            assert!(overflowing.is_empty(), "{w}x{h}: cells that scroll: {overflowing:?}");
        }
    }

    #[test]
    fn the_pie_grows_with_the_window() {
        let widest = |w: f32, h: f32| -> f32 {
            let size = egui::vec2(w, h);
            let mut harness = Harness::new();
            log_some_study_time(&mut harness);
            harness.state.pager.page = 2;
            let out = harness.settle(size);
            let mut shapes = Vec::new();
            for clipped in &out.shapes {
                flatten(&clipped.shape, &mut shapes);
            }
            // The pie's slices are the only filled multi-point paths;
            // together they span the pie's full width.
            shapes
                .iter()
                .filter_map(|s| match s {
                    egui::Shape::Path(path) if path.fill != egui::Color32::TRANSPARENT && path.points.len() > 3 => {
                        Some(egui::Rect::from_points(&path.points))
                    }
                    _ => None,
                })
                .reduce(|a, b| a.union(b))
                .map_or(0.0, |r| r.width())
        };
        let small = widest(960.0, 640.0);
        let large = widest(1600.0, 900.0);
        assert!(small >= PIE_MIN_DIAMETER - 1.0, "small window pie is {small}");
        assert!(large > small, "pie should be larger in a larger window ({small} vs {large})");
    }

    #[test]
    fn page_keys_step_through_the_pages() {
        // The keys themselves are handled in `app`; this checks the
        // pager API they use cooperates with the animated position.
        let size = egui::vec2(960.0, 640.0);
        let mut harness = Harness::new();
        harness.settle(size);
        harness.state.pager.step(1, harness.time, PAGE_COUNT);
        let text = all_text(&harness.settle(size));
        assert!(text.contains("STOPWATCH") && !text.contains("TODAY'S TASKS"));
    }
}
