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

    if notifications_enabled && queries::settings::task_reminders_enabled(conn).unwrap_or(true) {
        let tasks = queries::tasks::list_tasks(conn).unwrap_or_default();
        for task in tasks {
            if state.reminded_tasks.contains(&task.id) {
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
    let events = queries::events::list_all_events(conn).unwrap_or_default();
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
}
