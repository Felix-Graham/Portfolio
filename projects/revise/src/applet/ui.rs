//! The applet's window: the menu of three sections, the full-screen view of
//! whichever unit is running, and [`AppletApp`], the `eframe` app around them.
//!
//! It follows the main app's theme exactly — the same `Theme`, palette,
//! monospace fonts, `RButton`, and the Pomodoro ring — and its layout is the
//! Dashboard's Focus page, simplified: Timer and Pomodoro side by side, the
//! Stopwatch across the bottom, carved up by the same `pager::grid`.
//!
//! ## The borders
//!
//! The sections sit edge to edge, each outlined in light grey, so a line
//! between two sections is where their outlines meet. Hovering a section
//! lights its outline up (white, a touch heavier), drawn last so it sits over
//! the grey it shares with its neighbours. Which section is hovered, and what
//! each outline looks like, are plain functions ([`hovered_section`],
//! [`section_stroke`]) so they are tested without a screen.

use std::time::{Duration, Instant};

use eframe::egui::{self, Align, CornerRadius, Frame, Layout, Pos2, Rect, RichText, Stroke, StrokeKind, UiBuilder};

use super::{
    AppletState, ApiLogger, FocusLogger, FocusRun, Notice, Screen, Unit, BREAK_RANGE, POMODORO_STEP, TIMER_PRESETS,
    WORK_RANGE,
};
use crate::database::{queries, Database};
use crate::pomodoro::{self, Phase};
use crate::revision_session;
use crate::subjects::Subject;
use crate::theme::{Border, Palette, Spacing, Theme};
use crate::ui::components::{pomodoro_circle, RButton};
use crate::ui::pager;

/// The window the applet opens at, and the least it can be resized to.
pub const WINDOW_SIZE: [f32; 2] = [760.0, 500.0];
pub const MIN_WINDOW_SIZE: [f32; 2] = [560.0, 400.0];
/// Height of the status line under the sections.
const STATUS_H: f32 = 24.0;
/// How often the subject list is re-read on the menu (so one added in the
/// main app shows up without restarting the applet).
const SUBJECT_REFRESH: Duration = Duration::from_secs(3);

/// The menu's sections, in the order [`section_rects`] returns their areas.
pub const SECTIONS: [Unit; 3] = [Unit::Timer, Unit::Pomodoro, Unit::Stopwatch];

/// The three sections' areas, filling `area` exactly with no gaps: Timer and
/// Pomodoro side by side on top, the Stopwatch across the bottom — the Focus
/// page's arrangement.
pub fn section_rects(area: Rect) -> [Rect; 3] {
    let rows = pager::grid(area, &[1.35, 1.0], &[1.0], 0.0);
    let top = pager::grid(rows[0][0], &[1.0], &[1.0, 1.0], 0.0);
    [top[0][0], top[0][1], rows[1][0]]
}

/// Which section (an index into [`SECTIONS`]) the pointer is over, if any.
pub fn hovered_section(rects: &[Rect; 3], pointer: Option<Pos2>) -> Option<usize> {
    let pointer = pointer?;
    rects.iter().position(|r| r.contains(pointer))
}

/// A section's outline: light grey at rest, lit up (white, a little heavier)
/// while the pointer is over it.
pub fn section_stroke(palette: &Palette, hovered: bool) -> Stroke {
    if hovered {
        Stroke::new(Border::THICK, palette.focus)
    } else {
        Stroke::new(Border::HAIRLINE, palette.text_muted)
    }
}

/// What the full-screen view shows for a running unit.
#[derive(Debug, Clone, PartialEq)]
pub struct Readout {
    /// 0..=1 around the ring.
    pub progress: f32,
    /// The big digits.
    pub text: String,
    /// The small word under them (`focus`, `break`, `timer`, `stopwatch`).
    pub caption: &'static str,
    /// What the stop button is called.
    pub stop_label: &'static str,
    /// A line under the buttons, if one helps.
    pub hint: Option<&'static str>,
}

pub fn readout(state: &AppletState, unit: Unit) -> Readout {
    match unit {
        Unit::Timer => Readout {
            progress: state.timer.progress(),
            text: crate::timer::format_remaining(state.timer.remaining),
            caption: "timer",
            stop_label: "Stop",
            hint: Some("Stop logs the time so far."),
        },
        Unit::Pomodoro => {
            let focus = state.pomodoro.phase == Phase::Work;
            Readout {
                progress: state.pomodoro.progress(),
                text: pomodoro::format_remaining(state.pomodoro.remaining),
                caption: if focus { "focus" } else { "break" },
                stop_label: if focus { "Stop" } else { "Skip break" },
                hint: focus.then_some("Stop logs the time so far."),
            }
        }
        Unit::Stopwatch => {
            let elapsed = state.stopwatch.as_ref().map(|s| s.elapsed).unwrap_or_default();
            Readout {
                // One sweep of the ring a minute: it has no end to count toward.
                progress: (elapsed.as_secs() % 60) as f32 / 60.0,
                text: revision_session::format_elapsed(elapsed),
                caption: "stopwatch",
                stop_label: "Finish",
                hint: None,
            }
        }
    }
}

/// Draws whichever screen is current. `can_log` is whether the database
/// opened; without it nothing can be started, since nothing could be logged.
pub fn show(ui: &mut egui::Ui, theme: &Theme, state: &mut AppletState, logger: &dyn FocusLogger, can_log: bool) {
    match state.screen {
        Screen::Menu => menu(ui, theme, state, can_log),
        Screen::Running(unit) => running(ui, theme, state, unit, logger),
    }
}

fn small(theme: &Theme, text: &str) -> RichText {
    RichText::new(text).font(theme.small_font()).color(theme.palette.text_faint)
}

// --- the menu ----------------------------------------------------------------

fn menu(ui: &mut egui::Ui, theme: &Theme, state: &mut AppletState, can_log: bool) {
    let p = &theme.palette;
    let full = ui.max_rect();
    let body = Rect::from_min_max(full.min, egui::pos2(full.right(), (full.bottom() - STATUS_H).max(full.top())));
    // Inset a hair so the outermost outline isn't clipped by the window edge.
    let rects = section_rects(body.shrink(1.0));

    section(ui, theme, rects[0], "Timer", |ui| timer_section(ui, theme, state, can_log));
    section(ui, theme, rects[1], "Pomodoro", |ui| pomodoro_section(ui, theme, state, can_log));
    section(ui, theme, rects[2], "Stopwatch", |ui| stopwatch_section(ui, theme, state, can_log));

    // Outlines: every section's at rest first, then the hovered one over them.
    let hovered = hovered_section(&rects, ui.input(|i| i.pointer.hover_pos()));
    let painter = ui.painter_at(full);
    let outline = |rect: Rect, lit: bool| {
        painter.rect_stroke(rect, CornerRadius::ZERO, section_stroke(p, lit), StrokeKind::Inside);
    };
    for (i, rect) in rects.iter().enumerate() {
        if Some(i) != hovered {
            outline(*rect, false);
        }
    }
    if let Some(i) = hovered {
        outline(rects[i], true);
    }

    // The outcome of the last log, or why nothing can be started.
    let status_area = Rect::from_min_max(egui::pos2(full.left(), body.bottom()), full.max).shrink2(egui::vec2(Spacing::MD, 0.0));
    ui.scope_builder(UiBuilder::new().max_rect(status_area).layout(Layout::left_to_right(Align::Center)), |ui| {
        if !can_log {
            ui.label(RichText::new("Revise's database couldn't be opened, so nothing can be logged.").font(theme.small_font()).color(p.incorrect));
        } else {
            match &state.status {
                Some(Ok(message)) => {
                    ui.label(RichText::new(format!("✓ {message}")).font(theme.small_font()).color(p.text_muted));
                }
                Some(Err(message)) => {
                    ui.label(RichText::new(format!("✗ {message}")).font(theme.small_font()).color(p.incorrect));
                }
                None => {}
            }
        }
    });
}

/// One section: its label, then its contents, inside the section's outline.
fn section(ui: &mut egui::Ui, theme: &Theme, rect: Rect, label: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    let inner = rect.shrink(Spacing::MD);
    ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)), |ui| {
        ui.set_clip_rect(rect);
        ui.label(small(theme, &label.to_uppercase()));
        ui.add_space(Spacing::SM);
        add_contents(ui);
    });
}

fn subject_picker(ui: &mut egui::Ui, theme: &Theme, id_salt: &str, selected: &mut Option<i64>, subjects: &[Subject]) {
    ui.horizontal(|ui| {
        ui.label(small(theme, "Subject"));
        let label =
            selected.and_then(|id| subjects.iter().find(|s| s.id == id)).map_or_else(|| "No subject".to_owned(), |s| s.name.clone());
        egui::ComboBox::from_id_salt(id_salt).selected_text(label).show_ui(ui, |ui| {
            ui.selectable_value(selected, None, "No subject");
            for subject in subjects {
                ui.selectable_value(selected, Some(subject.id), subject.name.clone());
            }
        });
    });
}

fn start_button(ui: &mut egui::Ui, theme: &Theme, label: &str, enabled: bool) -> bool {
    ui.add_enabled_ui(enabled, |ui| RButton::new(label).primary().show(ui, theme).clicked()).inner
}

fn timer_section(ui: &mut egui::Ui, theme: &Theme, state: &mut AppletState, can_log: bool) {
    let p = &theme.palette;
    ui.label(RichText::new(crate::timer::format_remaining(state.timer.selected())).font(theme.mono(34.0)).color(p.text));
    ui.horizontal(|ui| {
        for (label, delta) in [("−5", -5), ("−1", -1), ("+1", 1), ("+5", 5)] {
            if ui.small_button(label).clicked() {
                state.nudge_timer_minutes(delta);
            }
        }
        ui.label(small(theme, "min"));
    });
    ui.horizontal_wrapped(|ui| {
        let current = state.timer.hours * 60 + state.timer.minutes;
        for preset in TIMER_PRESETS {
            let on = current == preset && state.timer.seconds == 0;
            if ui.selectable_label(on, format!("{preset}m")).clicked() {
                state.set_timer_minutes(preset);
            }
        }
    });
    ui.add_space(Spacing::XS);
    subject_picker(ui, theme, "applet_timer_subject", &mut state.timer.subject_id, &state.subjects);
    ui.add_space(Spacing::SM);
    if start_button(ui, theme, "Start Timer", can_log && state.can_start(Unit::Timer)) {
        state.start(Unit::Timer);
    }
}

/// A `−  value  +` row; returns the change asked for this frame (0 for none).
fn stepper(ui: &mut egui::Ui, theme: &Theme, label: &str, value: u32, step: i32) -> i32 {
    let mut delta = 0;
    ui.horizontal(|ui| {
        ui.label(small(theme, label));
        if ui.small_button("−").clicked() {
            delta = -step;
        }
        ui.label(RichText::new(value.to_string()).font(theme.mono(18.0)).color(theme.palette.text));
        if ui.small_button("+").clicked() {
            delta = step;
        }
        ui.label(small(theme, "min"));
    });
    delta
}

fn pomodoro_section(ui: &mut egui::Ui, theme: &Theme, state: &mut AppletState, can_log: bool) {
    let focus_delta = stepper(ui, theme, "Focus", state.pomodoro.work_minutes, POMODORO_STEP as i32);
    if focus_delta != 0 {
        state.nudge_work_minutes(focus_delta);
    }
    let break_delta = stepper(ui, theme, "Break", state.pomodoro.break_minutes, 1);
    if break_delta != 0 {
        state.nudge_break_minutes(break_delta);
    }
    ui.label(small(theme, &format!("{}–{} min focus, {}–{} min break", WORK_RANGE.0, WORK_RANGE.1, BREAK_RANGE.0, BREAK_RANGE.1)));
    ui.add_space(Spacing::XS);
    subject_picker(ui, theme, "applet_pomodoro_subject", &mut state.pomodoro.subject_id, &state.subjects);
    ui.add_space(Spacing::SM);
    if start_button(ui, theme, "Start Pomodoro", can_log && state.can_start(Unit::Pomodoro)) {
        state.start(Unit::Pomodoro);
    }
}

fn stopwatch_section(ui: &mut egui::Ui, theme: &Theme, state: &mut AppletState, can_log: bool) {
    ui.label(small(theme, "Counts up until you finish it."));
    ui.add_space(Spacing::XS);
    ui.horizontal(|ui| {
        subject_picker(ui, theme, "applet_stopwatch_subject", &mut state.stopwatch_subject, &state.subjects);
        ui.add_space(Spacing::MD);
        if start_button(ui, theme, "Start Stopwatch", can_log && state.can_start(Unit::Stopwatch)) {
            state.start(Unit::Stopwatch);
        }
    });
}

// --- a unit filling the window ---------------------------------------------

fn running(ui: &mut egui::Ui, theme: &Theme, state: &mut AppletState, unit: Unit, logger: &dyn FocusLogger) {
    let p = &theme.palette;
    let area = ui.max_rect();
    if ui.input(|i| i.key_pressed(egui::Key::Space)) {
        state.toggle_pause();
    }

    let readout = readout(state, unit);
    let subject = state.subject_name(state.subject_of(unit));
    let radius = (area.width().min(area.height()) * 0.28).clamp(60.0, 200.0);
    let content_height = radius * 2.0 + 130.0;
    ui.add_space(((area.height() - content_height) / 2.0).max(Spacing::MD));

    let mut pause = false;
    let mut stop = false;
    ui.vertical_centered(|ui| {
        pomodoro_circle::show(ui, theme, readout.progress, &readout.text, readout.caption, radius);
        ui.add_space(Spacing::SM);
        let line = match &subject {
            Some(name) => format!("{} · logging to {name}", unit.label()),
            None => unit.label().to_owned(),
        };
        ui.label(small(theme, &line));
        ui.add_space(Spacing::MD);
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - 240.0) / 2.0).max(0.0));
            let pause_label = if state.is_ticking() { "Pause" } else { "Resume" };
            pause = RButton::new(pause_label).show(ui, theme).clicked();
            stop = RButton::new(readout.stop_label).primary().show(ui, theme).clicked();
        });
        if let Some(hint) = readout.hint {
            ui.add_space(Spacing::SM);
            ui.label(RichText::new(hint).font(theme.small_font()).color(p.text_faint));
        }
    });

    if pause {
        state.toggle_pause();
    }
    if stop {
        if unit == Unit::Pomodoro && state.pomodoro.phase == Phase::Break {
            state.skip_break(logger);
        } else {
            state.stop(logger);
        }
    }
}

// --- the window --------------------------------------------------------------

/// The `eframe` app: owns the database and the state, ticks the running unit
/// each frame, logs through the API, and sends the notifications.
pub struct AppletApp {
    theme: Theme,
    db: Option<Database>,
    state: AppletState,
    subjects_read_at: Option<Instant>,
}

/// Stands in for the real logger when the database couldn't be opened (the
/// menu then refuses to start anything, so this is only a backstop).
struct Unavailable;

impl FocusLogger for Unavailable {
    fn log(&self, _run: &FocusRun) -> Result<String, String> {
        Err("Revise's database isn't available.".to_owned())
    }
}

impl AppletApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let theme = Theme::new();
        theme.apply(&cc.egui_ctx);
        let db = Database::open_default().ok();
        let mut app = Self { theme, db, state: AppletState::default(), subjects_read_at: None };
        app.refresh_subjects();
        app
    }

    fn refresh_subjects(&mut self) {
        if let Some(db) = &self.db {
            if let Ok(subjects) = queries::subjects::list_active_subjects(&db.conn) {
                self.state.set_subjects(subjects);
            }
        }
        self.subjects_read_at = Some(Instant::now());
    }

    fn send(&self, notices: Vec<Notice>) {
        for notice in notices {
            crate::notifications::send(self.db.as_ref().map(|db| &db.conn), &notice.title, &notice.body);
        }
    }

    fn with_logger<R>(&mut self, f: impl FnOnce(&mut AppletState, &dyn FocusLogger) -> R) -> R {
        match &self.db {
            Some(db) => f(&mut self.state, &ApiLogger { db }),
            None => f(&mut self.state, &Unavailable),
        }
    }
}

impl eframe::App for AppletApp {
    // eframe 0.36's `ui` (not `update`), like `ReviseApp` — see its comment.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Closing the window mid-run saves the time so far.
        if ctx.input(|i| i.viewport().close_requested()) {
            self.with_logger(|state, logger| state.close(logger));
        }

        // Keep the list fresh while on the menu.
        if self.state.screen == Screen::Menu && self.subjects_read_at.is_none_or(|at| at.elapsed() > SUBJECT_REFRESH) {
            self.refresh_subjects();
        }

        let notices = self.with_logger(|state, logger| state.tick(logger));
        self.send(notices);
        if self.state.is_running() {
            // Repainting is what keeps the readout moving.
            ctx.request_repaint_after(Duration::from_millis(250));
        } else {
            ctx.request_repaint_after(SUBJECT_REFRESH);
        }

        let can_log = self.db.is_some();
        let background = self.theme.palette.background;
        egui::CentralPanel::default().frame(Frame::NONE.fill(background)).show(ui, |ui| {
            let Self { theme, db, state, .. } = self;
            match db {
                Some(db) => show(ui, theme, state, &ApiLogger { db }, can_log),
                None => show(ui, theme, state, &Unavailable, can_log),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pomodoro::Phase;
    use eframe::egui::{Pos2, Vec2};
    use std::cell::RefCell;

    const SCREEN: Vec2 = Vec2::new(760.0, 500.0);

    #[derive(Default)]
    struct Fake {
        runs: RefCell<Vec<FocusRun>>,
    }

    impl FocusLogger for Fake {
        fn log(&self, run: &FocusRun) -> Result<String, String> {
            self.runs.borrow_mut().push(run.clone());
            Ok("logged".to_owned())
        }
    }

    fn area() -> Rect {
        Rect::from_min_size(Pos2::ZERO, SCREEN)
    }

    // --- layout, hover and outlines ------------------------------------------

    #[test]
    fn the_three_sections_fill_the_window_with_no_gaps_or_overlaps() {
        let rects = section_rects(area());
        let total: f32 = rects.iter().map(|r| r.width() * r.height()).sum();
        assert!((total - area().width() * area().height()).abs() < 1.0, "they tile the area exactly");
        for (i, a) in rects.iter().enumerate() {
            for b in rects.iter().skip(i + 1) {
                let overlap = a.intersect(*b);
                assert!(overlap.width() <= 0.01 || overlap.height() <= 0.01, "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn it_is_the_focus_pages_arrangement_timer_and_pomodoro_over_the_stopwatch() {
        let [timer, pomodoro, stopwatch] = section_rects(area());
        assert_eq!(timer.top(), pomodoro.top());
        assert!(timer.right() <= pomodoro.left() + 0.01, "side by side, timer first");
        assert!(stopwatch.top() >= timer.bottom() - 0.01, "stopwatch beneath");
        assert!((stopwatch.width() - area().width()).abs() < 0.01, "across the whole bottom");
    }

    #[test]
    fn the_hovered_section_is_the_one_under_the_pointer() {
        let rects = section_rects(area());
        assert_eq!(hovered_section(&rects, Some(rects[0].center())), Some(0));
        assert_eq!(hovered_section(&rects, Some(rects[1].center())), Some(1));
        assert_eq!(hovered_section(&rects, Some(rects[2].center())), Some(2));
        assert_eq!(hovered_section(&rects, Some(Pos2::new(-5.0, -5.0))), None);
        assert_eq!(hovered_section(&rects, None), None);
    }

    #[test]
    fn outlines_are_light_grey_and_light_up_on_hover() {
        let p = Palette::dark();
        let rest = section_stroke(&p, false);
        let lit = section_stroke(&p, true);
        assert_eq!(rest.color, p.text_muted, "light grey at rest");
        assert_eq!(lit.color, p.focus, "white when hovered");
        assert!(lit.width > rest.width, "and a little heavier");
        assert!(lit.color.r() > rest.color.r(), "brighter, not just different");
    }

    // --- what the full-screen view shows ---------------------------------------

    #[test]
    fn the_readout_for_each_unit() {
        let mut s = AppletState::default();
        s.start(Unit::Timer);
        let r = readout(&s, Unit::Timer);
        assert_eq!((r.caption, r.stop_label), ("timer", "Stop"));
        assert_eq!(r.text, "5:00");

        let mut s = AppletState::default();
        s.start(Unit::Pomodoro);
        let r = readout(&s, Unit::Pomodoro);
        assert_eq!((r.caption, r.stop_label), ("focus", "Stop"));
        assert_eq!(r.text, "25:00");
        s.pomodoro.phase = Phase::Break;
        let r = readout(&s, Unit::Pomodoro);
        assert_eq!((r.caption, r.stop_label), ("break", "Skip break"));
        assert!(r.hint.is_none(), "nothing to log in a break");

        let mut s = AppletState::default();
        s.start(Unit::Stopwatch);
        s.stopwatch.as_mut().unwrap().elapsed = Duration::from_secs(75);
        let r = readout(&s, Unit::Stopwatch);
        assert_eq!((r.caption, r.stop_label, r.text.as_str()), ("stopwatch", "Finish", "1:15"));
        assert!((r.progress - 15.0 / 60.0).abs() < 1e-6, "the ring sweeps once a minute");
    }

    // --- drawing, headlessly -----------------------------------------------------

    fn render(state: &mut AppletState, logger: &dyn FocusLogger, can_log: bool, frames: usize) {
        let ctx = egui::Context::default();
        let theme = Theme::new();
        theme.apply(&ctx);
        for i in 0..frames {
            let input = egui::RawInput {
                screen_rect: Some(area()),
                time: Some(1.0 + i as f64 * 0.05),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().frame(Frame::NONE).show(ui, |ui| show(ui, &theme, state, logger, can_log));
            });
            out.textures_delta.clear();
        }
    }

    fn subject(id: i64, name: &str) -> Subject {
        Subject { id, name: name.to_owned(), archived: false, created_at: String::new(), parent_subject_id: None, color: None }
    }

    #[test]
    fn the_menu_draws_with_and_without_subjects_and_without_a_database() {
        let log = Fake::default();
        let mut s = AppletState::default();
        render(&mut s, &log, true, 3);
        s.set_subjects(vec![subject(1, "French")]);
        s.timer.subject_id = Some(1);
        s.status = Some(Ok("Logged 25 minutes.".to_owned()));
        render(&mut s, &log, true, 3);
        s.status = Some(Err("locked".to_owned()));
        render(&mut s, &log, false, 3);
    }

    #[test]
    fn every_unit_draws_full_screen_and_a_pomodoro_break_too() {
        let log = Fake::default();
        for unit in [Unit::Timer, Unit::Pomodoro, Unit::Stopwatch] {
            let mut s = AppletState::default();
            s.set_subjects(vec![subject(1, "French")]);
            s.pomodoro.subject_id = Some(1);
            s.start(unit);
            render(&mut s, &log, true, 3);
            assert_eq!(s.screen, Screen::Running(unit), "drawing alone changes nothing");
        }
        let mut s = AppletState::default();
        s.start(Unit::Pomodoro);
        s.pomodoro.phase = Phase::Break;
        render(&mut s, &log, true, 3);
    }

    #[test]
    fn it_draws_in_a_small_window_too() {
        let ctx = egui::Context::default();
        let theme = Theme::new();
        theme.apply(&ctx);
        let log = Fake::default();
        let mut s = AppletState::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::from(MIN_WINDOW_SIZE))),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().frame(Frame::NONE).show(ui, |ui| show(ui, &theme, &mut s, &log, true));
        });
        out.textures_delta.clear();
    }
}
