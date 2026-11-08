pub mod reminder;

pub use reminder::{should_remind_for_event, should_remind_for_task, should_run_exec_command};

use std::collections::HashSet;

use chrono::{DateTime, Local, NaiveDate};

use crate::database::{queries, Database};

use rusqlite::Connection;

/// Feature request: "Toggle Switch In App Settings from Hyprctl
/// Notify to Caelestia Notify" — which external command `send`
/// shells out to. `Hyprctl` (the original, and still the default) is
/// Hyprland's own built-in overlay; `Caelestia` instead sends a
/// standard freedesktop.org desktop notification via `notify-send`,
/// which caelestia-shell (a Quickshell-based Hyprland shell) picks up
/// and renders in its own style, in place of Hyprland's overlay.
/// `notify-send` is invoked as a subprocess exactly like `hyprctl`
/// already is — not linked as a D-Bus library — for the same
/// portability reason `send`'s own doc comment below gives for
/// choosing `hyprctl` over `notify-rust` originally: no system D-Bus
/// library needed in the binary itself, regardless of which backend
/// is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotificationBackend {
    #[default]
    Hyprctl,
    Caelestia,
}

impl NotificationBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            NotificationBackend::Hyprctl => "hyprctl",
            NotificationBackend::Caelestia => "caelestia",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "hyprctl" => Some(NotificationBackend::Hyprctl),
            "caelestia" => Some(NotificationBackend::Caelestia),
            _ => None,
        }
    }
}

/// Sends a desktop notification via whichever backend
/// `queries::settings::notification_backend` currently has selected
/// (feature request: "Toggle Switch In App Settings from Hyprctl
/// Notify to Caelestia Notify") — `Hyprctl` if `conn` is unavailable
/// (e.g. a database error at startup) or the setting hasn't been
/// touched, preserving this app's original, only-ever behavior for
/// anyone who hasn't opted into the new one.
///
/// # Hyprctl
/// `hyprctl notify` — Hyprland's own built-in notification mechanism
/// (the same one used for `hl.notification.create` in a Lua config,
/// or `hyprctl notify` directly from a keybind). Chosen originally
/// over the freedesktop.org D-Bus notification spec (the more
/// portable option, and what an earlier version of this used via
/// `notify-rust`) because it needs no system D-Bus library at all —
/// a real, hard-to-satisfy dependency on some setups (Nix in
/// particular, where `libdbus`/`dbus-1` aren't always straightforward
/// to locate) — and the plan's target platform is specifically
/// Hyprland, so leaning on what it already provides is a reasonable
/// trade against broader desktop-environment portability.
///
/// # Caelestia
/// `notify-send` — the standard freedesktop.org desktop notification
/// command (part of `libnotify`), which caelestia-shell's own
/// notification manager listens for over D-Bus and renders in its
/// own style instead of Hyprland's overlay. Still just a subprocess
/// call, not a linked D-Bus library, for the exact portability reason
/// above.
///
/// Errors are swallowed rather than surfaced either way: the chosen
/// command not being on `PATH` (e.g. running outside Hyprland, or
/// without `libnotify` installed) or failing for any other reason
/// shouldn't crash or otherwise interrupt the app, the same way a
/// failed non-critical DB read elsewhere falls back to an empty
/// result instead of panicking.
pub(crate) fn send(conn: Option<&Connection>, summary: &str, body: &str) {
    let backend = conn.and_then(|c| queries::settings::notification_backend(c).ok()).unwrap_or_default();
    match backend {
        NotificationBackend::Hyprctl => {
            let message = format!("{summary}: {body}");
            let _ = std::process::Command::new("hyprctl").args(["notify", "-1", "8000", "rgb(F2F2F2)", &message]).spawn();
        }
        NotificationBackend::Caelestia => {
            let _ = std::process::Command::new("notify-send").args([summary, body]).spawn();
        }
    }
}

/// Tracks which reminders have already fired this session, so
/// polling repeatedly doesn't repeatedly notify. Deliberately
/// in-memory, not persisted: restarting the app and potentially
/// getting reminded again about something starting imminently is a
/// perfectly reasonable outcome, and persisting "already notified"
/// state would be considerably more complexity for a marginal benefit.
#[derive(Default)]
pub struct NotificationScanState {
    pub(crate) reminded_events: HashSet<(i64, NaiveDate)>,
    pub(crate) reminded_tasks: HashSet<i64>,
    /// Feature request: "Time-blocking" — blocks already announced,
    /// keyed by `(task id, start as a unix timestamp)` rather than the
    /// block's row id: re-planning replaces auto blocks with new rows,
    /// and an unchanged block shouldn't announce itself a second time,
    /// while one that moved (a new start) should. Pruned of old entries
    /// on every scan so a long-running daemon doesn't accumulate them.
    pub(crate) reminded_blocks: HashSet<(i64, i64)>,
    /// Feature request: "Startup Scripts/Commands" — separate from
    /// `reminded_events` since a reminder and running the startup
    /// command fire at different moments (lead time before start, vs
    /// at start) and shouldn't share one dedup set.
    pub(crate) executed_commands: HashSet<(i64, NaiveDate)>,
    reminded_tasks_day: Option<NaiveDate>,
    last_check: Option<DateTime<Local>>,
}

/// How often to actually hit the database and check for due
/// reminders. Checking every frame (up to 60 times a second in an
/// immediate-mode UI) would be wasteful for something that only needs
/// minute-level precision.
const CHECK_INTERVAL_SECONDS: i64 = 20;

/// Polls for due reminders and sends any that are newly due. Cheap to
/// call every frame — it no-ops until `CHECK_INTERVAL_SECONDS` has
/// passed since the last real check. Returns whether a real scan
/// actually ran this call (`false` if it short-circuited on the
/// throttle) — used by the GUI to know when to refresh its "I'm
/// alive" heartbeat for `revise-reminderd` (feature request:
/// "Background Notifications"), independent of whether reminders
/// themselves are currently enabled.
pub fn check_reminders(db: &Database, state: &mut NotificationScanState) -> bool {
    let now = Local::now();
    if let Some(last) = state.last_check {
        if (now - last).num_seconds() < CHECK_INTERVAL_SECONDS {
            return false;
        }
    }
    state.last_check = Some(now);

    let conn = &db.conn;
    let today = now.date_naive();
    if state.reminded_tasks_day != Some(today) {
        state.reminded_tasks.clear();
        state.reminded_tasks_day = Some(today);
    }

    // Reminders and startup commands are gated independently — an
    // event's startup command is a distinct feature (and its own
    // opt-in setting) from "notify me before this starts", so turning
    // reminders off shouldn't silently also stop startup commands
    // from running, or vice versa.
    let notifications_enabled = queries::settings::notifications_enabled(conn).unwrap_or(true);
    let exec_commands_enabled = queries::settings::exec_commands_enabled(conn).unwrap_or(false);

    if notifications_enabled || exec_commands_enabled {
        let lead_minutes = queries::settings::reminder_lead_minutes(conn).unwrap_or(10);
        for (event_id, title, start, exec_command) in todays_event_starts(conn, today) {
            let key = (event_id, start.date_naive());
            if notifications_enabled
                && !state.reminded_events.contains(&key)
                && should_remind_for_event(now, start, lead_minutes)
            {
                send(Some(conn), "Revision session", &format!("{title} starts in {lead_minutes} minutes."));
                state.reminded_events.insert(key);
            }

            // Feature request: "Startup Scripts/Commands". A separate
            // dedup key/condition from the reminder above — this fires
            // once, at (or shortly after) the actual start time, not
            // during the lead-time window.
            if exec_commands_enabled {
                if let Some(command) = exec_command {
                    if !state.executed_commands.contains(&key) && should_run_exec_command(now, start) {
                        let _ = std::process::Command::new("sh").arg("-c").arg(&command).spawn();
                        state.executed_commands.insert(key);
                    }
                }
            }
        }
    }

    // Feature request: "Time-blocking" — "your planned block starts soon".
    // Lives here, not in a separate loop, so the GUI and `revise-reminderd`
    // both get it from the one shared scan (and the GUI-heartbeat rule that
    // stops them double-notifying covers it too).
    state.reminded_blocks.retain(|(_, start)| *start >= (now - chrono::Duration::days(1)).timestamp());
    if notifications_enabled
        && queries::settings::time_blocking_enabled(conn).unwrap_or(false)
        && queries::settings::block_reminders_enabled(conn).unwrap_or(true)
    {
        let lead_minutes = queries::settings::reminder_lead_minutes(conn).unwrap_or(10);
        for due in due_block_reminders(conn, now, lead_minutes, &state.reminded_blocks) {
            send(Some(conn), "Planned task", &due.message(now));
            state.reminded_blocks.insert(due.key);
        }
    }

    if notifications_enabled && queries::settings::task_reminders_enabled(conn).unwrap_or(true) {
        let tasks = queries::tasks::list_tasks(conn).unwrap_or_default();
        for task in tasks {
            // Feature request: "Auto Assigning" — a topic template's
            // own due date is just its start date, not a deadline to
            // remind about; its generated per-day tasks are reminded
            // on their own days.
            if task.plan_topics_ahead || state.reminded_tasks.contains(&task.id) {
                continue;
            }
            let due = task.due_date.as_deref().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
            if should_remind_for_task(today, due, task.completed) {
                send(Some(conn), "Task due today", &task.title);
                state.reminded_tasks.insert(task.id);
            }
        }
    }

    true
}

/// A planned block that is about to start and hasn't been announced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueBlockReminder {
    /// `(task id, start timestamp)` — what `reminded_blocks` records.
    pub key: (i64, i64),
    pub title: String,
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
}

impl DueBlockReminder {
    /// The notification body, e.g. `Maths assignment starts in 7 minutes
    /// (14:00–14:45).` The countdown is the time actually left when the
    /// reminder fires (the scan only runs every 20–60 seconds, so it is
    /// rarely exactly the lead time), rounded up and never below one.
    pub fn message(&self, now: DateTime<Local>) -> String {
        let minutes = ((self.start - now).num_seconds() + 59).div_euclid(60).max(1);
        format!(
            "{} starts in {} ({}–{}).",
            self.title,
            if minutes == 1 { "1 minute".to_owned() } else { format!("{minutes} minutes") },
            self.start.format("%H:%M"),
            self.end.format("%H:%M"),
        )
    }
}

/// Planned time blocks whose start is inside the reminder window
/// (`lead_minutes` before the start, up to the start itself — the same
/// rule as session reminders, `should_remind_for_event`) and that are
/// not in `already`. Looks at today **and tomorrow**, so a block at
/// 00:05 is announced from 23:55 the night before. Skips blocks of
/// finished tasks. Pure with respect to the clock and to sending, so
/// the timing and the de-duplication are tested directly.
pub fn due_block_reminders(
    conn: &Connection,
    now: DateTime<Local>,
    lead_minutes: i64,
    already: &HashSet<(i64, i64)>,
) -> Vec<DueBlockReminder> {
    let today = now.date_naive();
    let tomorrow = today + chrono::Duration::days(1);
    let blocks = queries::task_blocks::list_blocks_between(conn, today, tomorrow).unwrap_or_default();
    if blocks.is_empty() {
        return Vec::new();
    }
    let tasks = queries::tasks::list_tasks(conn).unwrap_or_default();

    blocks
        .into_iter()
        .filter_map(|block| {
            let key = (block.task_id, block.start_time.timestamp());
            if block.is_settled() || already.contains(&key) || !should_remind_for_event(now, block.start_time, lead_minutes) {
                return None;
            }
            let task = tasks.iter().find(|t| t.id == block.task_id)?;
            if task.completed {
                return None;
            }
            Some(DueBlockReminder { key, title: task.title.clone(), start: block.start_time, end: block.end_time })
        })
        .collect()
}

/// Every event occurrence (recurring or not) that starts today, as
/// `(event_id, title, start, exec_command)`. Mirrors the
/// occurrence-expansion in `ui::calendar::occurrences_in_window` at a
/// smaller scope — just enough to know when today's sessions start
/// (and what, if anything, should run at that moment), not the full
/// `DayOccurrence` shape the UI needs for editing/exceptions/tasks.
/// `exec_command` is carried from the parent event's own field —
/// every occurrence of a recurring event shares it, the same way they
/// already share its task list.
fn todays_event_starts(
    conn: &rusqlite::Connection,
    today: NaiveDate,
) -> Vec<(i64, String, DateTime<Local>, Option<String>)> {
    let events = queries::events::list_visible_events(conn).unwrap_or_default();
    let mut starts = Vec::new();

    for event in &events {
        let anchor_date = event.day();
        match event.recurrence_id {
            None => {
                if anchor_date == today {
                    starts.push((event.id, event.title.clone(), event.start_time, event.exec_command.clone()));
                }
            }
            Some(rule_id) => {
                let Ok(Some(rule)) = queries::recurrence::get_rule(conn, rule_id) else { continue };
                if !crate::events::occurrence::occurs_on(anchor_date, &rule, today) {
                    continue;
                }
                let exceptions = queries::recurrence::list_exceptions(conn, event.id).unwrap_or_default();
                if exceptions.contains(&today) {
                    continue;
                }
                let start = if today == anchor_date {
                    event.start_time
                } else {
                    event.start_time + (today - anchor_date)
                };
                starts.push((event.id, event.title.clone(), start, event.exec_command.clone()));
            }
        }
    }

    starts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::queries::{events, recurrence};
    use crate::events::{Frequency, RecurrenceRule};
    use chrono::TimeZone;

    #[test]
    fn todays_event_starts_carries_the_events_exec_command() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let today = Local::now().date_naive();
        let start = today.and_hms_opt(14, 0, 0).unwrap();
        let start = Local.from_local_datetime(&start).unwrap();
        let end = start + chrono::Duration::hours(1);
        let event = events::create_event(&db.conn, "Today's session", "", start, end, None, None).expect("create");
        events::set_exec_command(&db.conn, event.id, Some("firefox")).expect("set exec command");

        let starts = todays_event_starts(&db.conn, today);
        assert_eq!(starts[0].3.as_deref(), Some("firefox"));
    }

    #[test]
    fn todays_event_starts_includes_non_recurring_events_today() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let today = Local::now().date_naive();
        let start = today.and_hms_opt(14, 0, 0).unwrap();
        let start = Local.from_local_datetime(&start).unwrap();
        let end = start + chrono::Duration::hours(1);
        events::create_event(&db.conn, "Today's session", "", start, end, None, None).expect("create");

        let starts = todays_event_starts(&db.conn, today);
        assert_eq!(starts.len(), 1);
        assert_eq!(starts[0].1, "Today's session");
        assert_eq!(starts[0].2, start);
    }

    #[test]
    fn todays_event_starts_expands_a_recurring_event_shifted_to_todays_time() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let today = Local::now().date_naive();
        let anchor_day = today - chrono::Duration::days(7);
        let anchor_start_naive = anchor_day.and_hms_opt(9, 0, 0).unwrap();
        let anchor_start = Local.from_local_datetime(&anchor_start_naive).unwrap();
        let anchor_end = anchor_start + chrono::Duration::hours(1);

        let rule = RecurrenceRule {
            id: None,
            frequency: Frequency::Weekly,
            interval: 1,
            weekdays: Vec::new(),
            end_date: None,
            occurrence_limit: None,
        };
        let rule_id = recurrence::create_rule(&db.conn, &rule).expect("create rule");
        events::create_event(&db.conn, "Weekly session", "", anchor_start, anchor_end, None, Some(rule_id))
            .expect("create recurring event");

        let starts = todays_event_starts(&db.conn, today);
        assert_eq!(starts.len(), 1);
        assert_eq!(starts[0].2.date_naive(), today);
        assert_eq!(starts[0].2.format("%H:%M").to_string(), "09:00");
    }

    #[test]
    fn skipped_occurrence_does_not_appear_in_todays_starts() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let today = Local::now().date_naive();
        let anchor_start_naive = today.and_hms_opt(9, 0, 0).unwrap();
        let anchor_start = Local.from_local_datetime(&anchor_start_naive).unwrap();
        let anchor_end = anchor_start + chrono::Duration::hours(1);

        let rule = RecurrenceRule {
            id: None,
            frequency: Frequency::Daily,
            interval: 1,
            weekdays: Vec::new(),
            end_date: None,
            occurrence_limit: None,
        };
        let rule_id = recurrence::create_rule(&db.conn, &rule).expect("create rule");
        let event = events::create_event(&db.conn, "Daily session", "", anchor_start, anchor_end, None, Some(rule_id))
            .expect("create recurring event");
        recurrence::add_exception(&db.conn, event.id, today).expect("skip today");

        assert!(todays_event_starts(&db.conn, today).is_empty());
    }

    // --- Time-blocking: block-start reminders ------------------------------------------

    use crate::database::queries::{settings as settings_q, task_blocks, tasks as tasks_q};

    fn task_with_block_starting_in(db: &Database, title: &str, minutes_from_now: i64) -> (i64, DateTime<Local>) {
        let task = tasks_q::create_task(&db.conn, title, None, None, None).expect("task").id;
        let start = Local::now() + chrono::Duration::minutes(minutes_from_now);
        task_blocks::insert_locked_block(&db.conn, task, start, start + chrono::Duration::minutes(45), 0).expect("block");
        (task, start)
    }

    #[test]
    fn a_block_inside_the_lead_window_is_due_and_one_outside_it_is_not() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let (soon, _) = task_with_block_starting_in(&db, "Soon", 5);
        task_with_block_starting_in(&db, "Later", 45);

        let due = due_block_reminders(&db.conn, Local::now(), 10, &HashSet::new());

        assert_eq!(due.len(), 1);
        assert_eq!(due[0].key.0, soon);
        assert_eq!(due[0].title, "Soon");
    }

    #[test]
    fn a_block_that_has_already_started_is_no_longer_due() {
        let db = Database::open_in_memory().expect("open in-memory db");
        task_with_block_starting_in(&db, "Started", -3);
        assert!(due_block_reminders(&db.conn, Local::now(), 10, &HashSet::new()).is_empty());
    }

    #[test]
    fn an_announced_block_is_not_due_again_but_a_moved_one_is() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let (task, start) = task_with_block_starting_in(&db, "Essay", 5);

        let first = due_block_reminders(&db.conn, Local::now(), 10, &HashSet::new());
        let announced: HashSet<(i64, i64)> = first.iter().map(|d| d.key).collect();
        assert!(due_block_reminders(&db.conn, Local::now(), 10, &announced).is_empty(), "already announced");

        // Moving the block to a new start is a new key, so it announces itself again.
        let block = task_blocks::list_blocks_for_task(&db.conn, task).expect("blocks").remove(0);
        let moved = start + chrono::Duration::minutes(2);
        task_blocks::update_block_times(&db.conn, block.id, moved, moved + chrono::Duration::minutes(45)).expect("move");
        assert_eq!(due_block_reminders(&db.conn, Local::now(), 10, &announced).len(), 1);
    }

    #[test]
    fn a_finished_tasks_block_is_not_announced() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let (task, _) = task_with_block_starting_in(&db, "Done", 5);
        tasks_q::set_completed(&db.conn, task, true).expect("complete");
        assert!(due_block_reminders(&db.conn, Local::now(), 10, &HashSet::new()).is_empty());
    }

    #[test]
    fn a_zero_lead_time_announces_nothing() {
        let db = Database::open_in_memory().expect("open in-memory db");
        task_with_block_starting_in(&db, "Soon", 1);
        assert!(due_block_reminders(&db.conn, Local::now(), 0, &HashSet::new()).is_empty());
    }

    #[test]
    fn a_block_just_after_midnight_is_announced_from_the_evening_before() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let task = tasks_q::create_task(&db.conn, "Night owl", None, None, None).expect("task").id;
        let tomorrow = Local::now().date_naive() + chrono::Duration::days(1);
        let start = Local.from_local_datetime(&tomorrow.and_hms_opt(0, 5, 0).unwrap()).earliest().unwrap();
        task_blocks::insert_locked_block(&db.conn, task, start, start + chrono::Duration::minutes(30), 0).expect("block");

        let at_2355 = start - chrono::Duration::minutes(10);
        assert_eq!(due_block_reminders(&db.conn, at_2355, 10, &HashSet::new()).len(), 1);
    }

    #[test]
    fn the_message_counts_down_the_time_actually_left() {
        let start = Local.with_ymd_and_hms(2026, 10, 14, 14, 0, 0).unwrap();
        let reminder = DueBlockReminder {
            key: (1, start.timestamp()),
            title: "Maths assignment".to_owned(),
            start,
            end: start + chrono::Duration::minutes(45),
        };
        let at = |h, m, s| Local.with_ymd_and_hms(2026, 10, 14, h, m, s).unwrap();
        assert_eq!(reminder.message(at(13, 53, 0)), "Maths assignment starts in 7 minutes (14:00–14:45).");
        assert_eq!(reminder.message(at(13, 53, 30)), "Maths assignment starts in 7 minutes (14:00–14:45).", "rounds up");
        assert_eq!(reminder.message(at(13, 59, 30)), "Maths assignment starts in 1 minute (14:00–14:45).");
        assert_eq!(reminder.message(at(13, 59, 59)), "Maths assignment starts in 1 minute (14:00–14:45).", "never below one");
    }

    #[test]
    fn check_reminders_announces_a_due_block_once_when_time_blocking_is_on() {
        let db = Database::open_in_memory().expect("open in-memory db");
        settings_q::set_time_blocking_enabled(&db.conn, true).expect("enable");
        task_with_block_starting_in(&db, "Essay", 5);
        let mut state = NotificationScanState::default();

        assert!(check_reminders(&db, &mut state));
        assert_eq!(state.reminded_blocks.len(), 1, "announced");

        // A second real scan (past the throttle) does not announce it again.
        state.last_check = Some(Local::now() - chrono::Duration::seconds(60));
        check_reminders(&db, &mut state);
        assert_eq!(state.reminded_blocks.len(), 1);
    }

    #[test]
    fn check_reminders_stays_silent_about_blocks_unless_everything_needed_is_on() {
        let db = Database::open_in_memory().expect("open in-memory db");
        task_with_block_starting_in(&db, "Essay", 5);

        // Time blocking off (the default): nothing.
        let mut state = NotificationScanState::default();
        check_reminders(&db, &mut state);
        assert!(state.reminded_blocks.is_empty());

        // Time blocking on but block reminders off.
        settings_q::set_time_blocking_enabled(&db.conn, true).expect("enable");
        settings_q::set_block_reminders_enabled(&db.conn, false).expect("off");
        let mut state = NotificationScanState::default();
        check_reminders(&db, &mut state);
        assert!(state.reminded_blocks.is_empty());

        // Block reminders on but notifications as a whole off.
        settings_q::set_block_reminders_enabled(&db.conn, true).expect("on");
        settings_q::set_notifications_enabled(&db.conn, false).expect("notifications off");
        let mut state = NotificationScanState::default();
        check_reminders(&db, &mut state);
        assert!(state.reminded_blocks.is_empty());
    }

    #[test]
    fn old_announcements_are_pruned_so_a_long_running_daemon_does_not_grow() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let mut state = NotificationScanState::default();
        let long_ago = (Local::now() - chrono::Duration::days(3)).timestamp();
        let recent = (Local::now() - chrono::Duration::hours(2)).timestamp();
        state.reminded_blocks.insert((1, long_ago));
        state.reminded_blocks.insert((2, recent));

        check_reminders(&db, &mut state);

        assert!(!state.reminded_blocks.contains(&(1, long_ago)));
        assert!(state.reminded_blocks.contains(&(2, recent)));
    }
}
