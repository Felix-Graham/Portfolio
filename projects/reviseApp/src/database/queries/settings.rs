use rusqlite::{params, Connection, OptionalExtension, Result};

/// Raw get/set over the `settings` key-value table. Typed helpers
/// below (`notifications_enabled`, `reminder_lead_minutes`, ...) are
/// what the rest of the app should actually use — this pair exists
/// mainly to keep those helpers short.
pub fn get(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", params![key], |row| row.get(0)).optional()
}

pub fn set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

const KEY_NOTIFICATIONS_ENABLED: &str = "notifications_enabled";
const KEY_REMINDER_LEAD_MINUTES: &str = "reminder_lead_minutes";
const KEY_TASK_REMINDERS_ENABLED: &str = "task_reminders_enabled";
const KEY_EXEC_COMMANDS_ENABLED: &str = "exec_commands_enabled";
const KEY_NOTIFICATION_BACKEND: &str = "notification_backend";

const DEFAULT_REMINDER_LEAD_MINUTES: i64 = 10;

/// Master switch for all notifications — "notifications can be
/// disabled" (Sprint 10's explicit acceptance criterion). Defaults to
/// on: a setting nobody has touched yet should behave like the
/// feature is simply available, not silently off.
pub fn notifications_enabled(conn: &Connection) -> Result<bool> {
    Ok(get(conn, KEY_NOTIFICATIONS_ENABLED)?.map(|v| v == "true").unwrap_or(true))
}

pub fn set_notifications_enabled(conn: &Connection, enabled: bool) -> Result<()> {
    set(conn, KEY_NOTIFICATIONS_ENABLED, if enabled { "true" } else { "false" })
}

/// How many minutes before a session start its reminder fires.
pub fn reminder_lead_minutes(conn: &Connection) -> Result<i64> {
    Ok(get(conn, KEY_REMINDER_LEAD_MINUTES)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_REMINDER_LEAD_MINUTES))
}

pub fn set_reminder_lead_minutes(conn: &Connection, minutes: i64) -> Result<()> {
    set(conn, KEY_REMINDER_LEAD_MINUTES, &minutes.to_string())
}

/// Separate switch for task due-date reminders ("task reminders if
/// required" — the plan hedges these as optional, so they get their
/// own toggle independent of session reminders).
pub fn task_reminders_enabled(conn: &Connection) -> Result<bool> {
    Ok(get(conn, KEY_TASK_REMINDERS_ENABLED)?.map(|v| v == "true").unwrap_or(true))
}

pub fn set_task_reminders_enabled(conn: &Connection, enabled: bool) -> Result<()> {
    set(conn, KEY_TASK_REMINDERS_ENABLED, if enabled { "true" } else { "false" })
}

/// Feature request: "Startup Scripts/Commands" — master switch for
/// actually *running* any event's `exec_command`. Defaults to
/// **off**, unlike every other toggle in this file: this one lets
/// Revise run an arbitrary shell string unattended at a scheduled
/// time, and a setting nobody has explicitly turned on shouldn't do
/// that silently just because someone typed something into an
/// event's exec-command field at some point.
pub fn exec_commands_enabled(conn: &Connection) -> Result<bool> {
    Ok(get(conn, KEY_EXEC_COMMANDS_ENABLED)?.map(|v| v == "true").unwrap_or(false))
}

pub fn set_exec_commands_enabled(conn: &Connection, enabled: bool) -> Result<()> {
    set(conn, KEY_EXEC_COMMANDS_ENABLED, if enabled { "true" } else { "false" })
}

/// Feature request: "Toggle Switch In App Settings from Hyprctl
/// Notify to Caelestia Notify". Defaults to `Hyprctl` — this app's
/// original, only-ever backend — so a setting nobody has touched
/// behaves exactly as it always has.
pub fn notification_backend(conn: &Connection) -> Result<crate::notifications::NotificationBackend> {
    Ok(get(conn, KEY_NOTIFICATION_BACKEND)?
        .and_then(|v| crate::notifications::NotificationBackend::parse(&v))
        .unwrap_or_default())
}

pub fn set_notification_backend(conn: &Connection, backend: crate::notifications::NotificationBackend) -> Result<()> {
    set(conn, KEY_NOTIFICATION_BACKEND, backend.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::notifications::NotificationBackend;

    #[test]
    fn notification_backend_defaults_to_hyprctl_when_untouched() {
        let db = Database::open_in_memory().expect("open in-memory db");
        assert_eq!(notification_backend(&db.conn).expect("get"), NotificationBackend::Hyprctl);
    }

    #[test]
    fn notification_backend_roundtrips_through_storage() {
        let db = Database::open_in_memory().expect("open in-memory db");
        set_notification_backend(&db.conn, NotificationBackend::Caelestia).expect("set");
        assert_eq!(notification_backend(&db.conn).expect("get"), NotificationBackend::Caelestia);

        set_notification_backend(&db.conn, NotificationBackend::Hyprctl).expect("set back");
        assert_eq!(notification_backend(&db.conn).expect("get"), NotificationBackend::Hyprctl);
    }

    #[test]
    fn notification_backend_as_str_and_parse_round_trip_every_variant() {
        for backend in [NotificationBackend::Hyprctl, NotificationBackend::Caelestia] {
            assert_eq!(NotificationBackend::parse(backend.as_str()), Some(backend));
        }
    }

    #[test]
    fn notification_backend_parse_rejects_unknown_strings() {
        assert_eq!(NotificationBackend::parse("mako"), None);
        assert_eq!(NotificationBackend::parse(""), None);
    }
}
