use chrono::{Datelike, NaiveDate, Weekday};
use rusqlite::{params, Connection, OptionalExtension, Result};

use crate::tasks::time_blocking::WorkDay;

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
const KEY_AUTO_ASSIGN_DAILY_MINUTES: &str = "auto_assign_daily_minutes";
const KEY_AUTO_ASSIGN_DEFAULT_TASK_MINUTES: &str = "auto_assign_default_task_minutes";
const KEY_AUTO_ASSIGN_HORIZON_DAYS: &str = "auto_assign_horizon_days";
const KEY_TIME_BLOCKING_ENABLED: &str = "time_blocking_enabled";
const KEY_WORK_START_MINUTES: &str = "work_start_minutes";
const KEY_WORK_END_MINUTES: &str = "work_end_minutes";
const KEY_SLOT_GRANULARITY_MINUTES: &str = "slot_granularity_minutes";
const KEY_MIN_BLOCK_MINUTES: &str = "min_block_minutes";
const KEY_EVENT_BUFFER_MINUTES: &str = "event_buffer_minutes";
const KEY_MAX_CHUNKS: &str = "max_chunks";
const KEY_SPLIT_OVER_MINUTES: &str = "split_over_minutes";
const KEY_MAX_BLOCK_MINUTES: &str = "max_block_minutes";
const KEY_AUTO_REPLAN_BLOCKS: &str = "auto_replan_blocks";
const KEY_BLOCK_REMINDERS_ENABLED: &str = "block_reminders_enabled";
const KEY_BLOCK_START_TOLERANCE_MINUTES: &str = "block_start_tolerance_minutes";
const KEY_CALENDAR_PAST_DIM_PERCENT: &str = "calendar_past_dim_percent";
const KEY_STARTUP_QUIZ_ENABLED: &str = "startup_quiz_enabled";
const KEY_STARTUP_QUIZ_QUESTIONS: &str = "startup_quiz_questions";

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

/// Feature request: "Auto Assigning" — how many minutes of task work
/// one day should hold ("a day should involve 2 hours of task doing
/// {configure in settings}"). Anything unset or unparseable, or not
/// positive, falls back to the default.
pub fn auto_assign_daily_minutes(conn: &Connection) -> Result<i64> {
    positive_i64(conn, KEY_AUTO_ASSIGN_DAILY_MINUTES, crate::tasks::auto_assign::DEFAULT_DAILY_MINUTES)
}

pub fn set_auto_assign_daily_minutes(conn: &Connection, minutes: i64) -> Result<()> {
    set(conn, KEY_AUTO_ASSIGN_DAILY_MINUTES, &minutes.to_string())
}

/// What Auto-assign assumes a task with no estimate takes.
pub fn auto_assign_default_task_minutes(conn: &Connection) -> Result<i64> {
    positive_i64(conn, KEY_AUTO_ASSIGN_DEFAULT_TASK_MINUTES, crate::tasks::auto_assign::DEFAULT_TASK_MINUTES)
}

pub fn set_auto_assign_default_task_minutes(conn: &Connection, minutes: i64) -> Result<()> {
    set(conn, KEY_AUTO_ASSIGN_DEFAULT_TASK_MINUTES, &minutes.to_string())
}

/// How many days ahead (from today) Auto-assign plans.
pub fn auto_assign_horizon_days(conn: &Connection) -> Result<i64> {
    positive_i64(conn, KEY_AUTO_ASSIGN_HORIZON_DAYS, crate::tasks::auto_assign::DEFAULT_HORIZON_DAYS)
}

pub fn set_auto_assign_horizon_days(conn: &Connection, days: i64) -> Result<()> {
    set(conn, KEY_AUTO_ASSIGN_HORIZON_DAYS, &days.to_string())
}

/// Feature request: "Time-blocking" (docs/planning/time-blocking.md
/// §5.4) — whether the app places tasks at times of day at all. Off
/// until someone turns it on, so nothing changes for anyone who hasn't.
pub fn time_blocking_enabled(conn: &Connection) -> Result<bool> {
    Ok(get(conn, KEY_TIME_BLOCKING_ENABLED)?.map(|v| v == "true").unwrap_or(false))
}

pub fn set_time_blocking_enabled(conn: &Connection, enabled: bool) -> Result<()> {
    set(conn, KEY_TIME_BLOCKING_ENABLED, if enabled { "true" } else { "false" })
}

/// The working window, as `(start, end)` minutes after midnight, in
/// which tasks may be placed. Both default (09:00–21:00); a pair that
/// isn't a real window (end past midnight, or not after the start)
/// falls back to the defaults **together**, never half-and-half.
pub fn work_window_minutes(conn: &Connection) -> Result<(i64, i64)> {
    use crate::tasks::time_blocking::{DEFAULT_WORK_END_MINUTES, DEFAULT_WORK_START_MINUTES};
    let start = positive_i64(conn, KEY_WORK_START_MINUTES, DEFAULT_WORK_START_MINUTES)?;
    let end = positive_i64(conn, KEY_WORK_END_MINUTES, DEFAULT_WORK_END_MINUTES)?;
    if end > 24 * 60 || start >= end {
        return Ok((DEFAULT_WORK_START_MINUTES, DEFAULT_WORK_END_MINUTES));
    }
    Ok((start, end))
}

pub fn set_work_window_minutes(conn: &Connection, start: i64, end: i64) -> Result<()> {
    set(conn, KEY_WORK_START_MINUTES, &start.to_string())?;
    set(conn, KEY_WORK_END_MINUTES, &end.to_string())
}

/// Blocks snap to this many minutes since midnight. Anything outside
/// 1–60 is treated as unset.
pub fn slot_granularity_minutes(conn: &Connection) -> Result<i64> {
    use crate::tasks::time_blocking::DEFAULT_GRANULARITY_MINUTES;
    Ok(Some(positive_i64(conn, KEY_SLOT_GRANULARITY_MINUTES, DEFAULT_GRANULARITY_MINUTES)?)
        .filter(|m| *m <= 60)
        .unwrap_or(DEFAULT_GRANULARITY_MINUTES))
}

pub fn set_slot_granularity_minutes(conn: &Connection, minutes: i64) -> Result<()> {
    set(conn, KEY_SLOT_GRANULARITY_MINUTES, &minutes.to_string())
}

/// The smallest block (and free gap) worth using.
pub fn min_block_minutes(conn: &Connection) -> Result<i64> {
    positive_i64(conn, KEY_MIN_BLOCK_MINUTES, crate::tasks::time_blocking::DEFAULT_MIN_BLOCK_MINUTES)
}

pub fn set_min_block_minutes(conn: &Connection, minutes: i64) -> Result<()> {
    set(conn, KEY_MIN_BLOCK_MINUTES, &minutes.to_string())
}

/// The gap kept clear around every calendar event. Unlike the others,
/// `0` is a real value (the default), so only a negative or
/// unparseable one is treated as unset.
pub fn event_buffer_minutes(conn: &Connection) -> Result<i64> {
    use crate::tasks::time_blocking::DEFAULT_BUFFER_MINUTES;
    Ok(get(conn, KEY_EVENT_BUFFER_MINUTES)?
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| *v >= 0)
        .unwrap_or(DEFAULT_BUFFER_MINUTES))
}

pub fn set_event_buffer_minutes(conn: &Connection, minutes: i64) -> Result<()> {
    set(conn, KEY_EVENT_BUFFER_MINUTES, &minutes.to_string())
}

/// The most pieces one task may be split into (`1` = never split).
pub fn max_chunks(conn: &Connection) -> Result<usize> {
    let default = crate::tasks::time_blocking::DEFAULT_MAX_CHUNKS;
    Ok(positive_i64(conn, KEY_MAX_CHUNKS, default as i64)? as usize)
}

pub fn set_max_chunks(conn: &Connection, chunks: usize) -> Result<()> {
    set(conn, KEY_MAX_CHUNKS, &chunks.to_string())
}

/// Feature request: tasks estimated at more than this many minutes are
/// planned as several blocks instead of one (default 30).
pub fn split_over_minutes(conn: &Connection) -> Result<i64> {
    positive_i64(conn, KEY_SPLIT_OVER_MINUTES, crate::tasks::time_blocking::DEFAULT_SPLIT_OVER_MINUTES)
}

pub fn set_split_over_minutes(conn: &Connection, minutes: i64) -> Result<()> {
    set(conn, KEY_SPLIT_OVER_MINUTES, &minutes.to_string())
}

/// Feature request: the longest block made when a task is split (default 60).
pub fn max_block_minutes(conn: &Connection) -> Result<i64> {
    positive_i64(conn, KEY_MAX_BLOCK_MINUTES, crate::tasks::time_blocking::DEFAULT_MAX_BLOCK_MINUTES)
}

pub fn set_max_block_minutes(conn: &Connection, minutes: i64) -> Result<()> {
    set(conn, KEY_MAX_BLOCK_MINUTES, &minutes.to_string())
}

/// Whether planned times are refreshed automatically — at launch and
/// when the day changes — instead of only when asked ("Plan times"
/// or Auto-assign). Off by default: re-planning moves unpinned blocks,
/// and a plan that shifts under someone who didn't ask is worse than a
/// stale one. Only has an effect while time blocking itself is on.
pub fn auto_replan_blocks(conn: &Connection) -> Result<bool> {
    Ok(get(conn, KEY_AUTO_REPLAN_BLOCKS)?.map(|v| v == "true").unwrap_or(false))
}

pub fn set_auto_replan_blocks(conn: &Connection, enabled: bool) -> Result<()> {
    set(conn, KEY_AUTO_REPLAN_BLOCKS, if enabled { "true" } else { "false" })
}

/// Whether to notify shortly before a planned time block starts (the
/// same lead time as session reminders). On by default — a person who
/// turned time blocking on wants the heads-up — but it only ever fires
/// while time blocking is on **and** notifications are enabled, so
/// anyone who hasn't opted into either hears nothing new.
pub fn block_reminders_enabled(conn: &Connection) -> Result<bool> {
    Ok(get(conn, KEY_BLOCK_REMINDERS_ENABLED)?.map(|v| v == "true").unwrap_or(true))
}

pub fn set_block_reminders_enabled(conn: &Connection, enabled: bool) -> Result<()> {
    set(conn, KEY_BLOCK_REMINDERS_ENABLED, if enabled { "true" } else { "false" })
}

/// The setting key holding one weekday's override (`work_hours_0` is
/// Monday … `work_hours_6` is Sunday).
fn weekday_key(weekday: Weekday) -> String {
    format!("work_hours_{}", weekday.num_days_from_monday())
}

/// One weekday's own working hours, if the person set any: `None` means
/// "follow the default window", `Some(WorkDay::Off)` a day off,
/// `Some(WorkDay::Hours(..))` hours of its own. Stored as `off` or
/// `start-end` in minutes; anything that isn't a real window is treated
/// as no override rather than guessed at.
pub fn weekday_override(conn: &Connection, weekday: Weekday) -> Result<Option<WorkDay>> {
    Ok(get(conn, &weekday_key(weekday))?.and_then(|text| parse_work_day(&text)))
}

/// Sets (or, with `None`, clears) one weekday's override.
pub fn set_weekday_override(conn: &Connection, weekday: Weekday, work_day: Option<WorkDay>) -> Result<()> {
    let key = weekday_key(weekday);
    match work_day {
        None => {
            conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
            Ok(())
        }
        Some(WorkDay::Off) => set(conn, &key, "off"),
        Some(WorkDay::Hours(start, end)) => set(conn, &key, &format!("{start}-{end}")),
    }
}

fn parse_work_day(text: &str) -> Option<WorkDay> {
    let text = text.trim();
    if text == "off" {
        return Some(WorkDay::Off);
    }
    let (start, end) = text.split_once('-')?;
    let (start, end) = (start.trim().parse::<i64>().ok()?, end.trim().parse::<i64>().ok()?);
    ((0..end).contains(&start) && end <= 24 * 60).then_some(WorkDay::Hours(start, end))
}

/// What `date` allows: its weekday's override if there is one, otherwise
/// the default window ([`work_window_minutes`]).
pub fn work_day_for(conn: &Connection, date: NaiveDate) -> Result<WorkDay> {
    match weekday_override(conn, date.weekday())? {
        Some(work_day) => Ok(work_day),
        None => work_window_minutes(conn).map(|(start, end)| WorkDay::Hours(start, end)),
    }
}

/// How close to a block's start a stopwatch or timer must begin to count
/// as starting that block, in minutes either side.
pub fn block_start_tolerance_minutes(conn: &Connection) -> Result<i64> {
    positive_i64(conn, KEY_BLOCK_START_TOLERANCE_MINUTES, crate::tasks::block::DEFAULT_START_TOLERANCE_MINUTES)
}

pub fn set_block_start_tolerance_minutes(conn: &Connection, minutes: i64) -> Result<()> {
    set(conn, KEY_BLOCK_START_TOLERANCE_MINUTES, &minutes.to_string())
}

/// How strongly the Day view dims the part of today that has already passed,
/// as a percentage of how far it fades into the background. "Medium" by default.
pub const DEFAULT_PAST_DIM_PERCENT: u8 = 40;
/// The most the dim can be set to, so the past never vanishes entirely.
pub const MAX_PAST_DIM_PERCENT: u8 = 90;

/// 0 turns the dim off. Unset or unparseable falls back to the default; over
/// the cap is held to it.
pub fn calendar_past_dim_percent(conn: &Connection) -> Result<u8> {
    let Some(raw) = get(conn, KEY_CALENDAR_PAST_DIM_PERCENT)? else {
        return Ok(DEFAULT_PAST_DIM_PERCENT);
    };
    Ok(raw.trim().parse::<u8>().map(|n| n.min(MAX_PAST_DIM_PERCENT)).unwrap_or(DEFAULT_PAST_DIM_PERCENT))
}

pub fn set_calendar_past_dim_percent(conn: &Connection, percent: u8) -> Result<()> {
    set(conn, KEY_CALENDAR_PAST_DIM_PERCENT, &percent.min(MAX_PAST_DIM_PERCENT).to_string())
}

/// Feature request: "Startup Questions" — whether launching Revise opens with a
/// short flashcard quiz. On by default, since the feature was asked for; with no
/// flashcards stored there is nothing to ask, so a fresh install is unaffected.
pub fn startup_quiz_enabled(conn: &Connection) -> Result<bool> {
    Ok(get(conn, KEY_STARTUP_QUIZ_ENABLED)?.map(|v| v == "true").unwrap_or(true))
}

pub fn set_startup_quiz_enabled(conn: &Connection, enabled: bool) -> Result<()> {
    set(conn, KEY_STARTUP_QUIZ_ENABLED, if enabled { "true" } else { "false" })
}

/// How many questions the startup quiz asks ("n ... defined in settings,
/// default 5"). Unset, unparseable or not positive falls back to the default;
/// anything over the cap is held to it.
pub fn startup_quiz_questions(conn: &Connection) -> Result<usize> {
    let n = positive_i64(conn, KEY_STARTUP_QUIZ_QUESTIONS, crate::flashcards::quiz::DEFAULT_QUESTIONS as i64)?;
    Ok((n as usize).min(crate::flashcards::quiz::MAX_QUESTIONS))
}

pub fn set_startup_quiz_questions(conn: &Connection, questions: usize) -> Result<()> {
    set(conn, KEY_STARTUP_QUIZ_QUESTIONS, &questions.to_string())
}

fn positive_i64(conn: &Connection, key: &str, default: i64) -> Result<i64> {
    Ok(get(conn, key)?.and_then(|v| v.parse::<i64>().ok()).filter(|v| *v > 0).unwrap_or(default))
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

    #[test]
    fn auto_assign_settings_default_when_untouched() {
        let db = Database::open_in_memory().expect("open in-memory db");
        assert_eq!(auto_assign_daily_minutes(&db.conn).expect("get"), 120);
        assert_eq!(auto_assign_default_task_minutes(&db.conn).expect("get"), 30);
        assert_eq!(auto_assign_horizon_days(&db.conn).expect("get"), 14);
    }

    #[test]
    fn auto_assign_settings_roundtrip_through_storage() {
        let db = Database::open_in_memory().expect("open in-memory db");
        set_auto_assign_daily_minutes(&db.conn, 90).expect("set");
        set_auto_assign_default_task_minutes(&db.conn, 20).expect("set");
        set_auto_assign_horizon_days(&db.conn, 7).expect("set");
        assert_eq!(auto_assign_daily_minutes(&db.conn).expect("get"), 90);
        assert_eq!(auto_assign_default_task_minutes(&db.conn).expect("get"), 20);
        assert_eq!(auto_assign_horizon_days(&db.conn).expect("get"), 7);
    }

    #[test]
    fn auto_assign_settings_ignore_nonsense_stored_values() {
        let db = Database::open_in_memory().expect("open in-memory db");
        set(&db.conn, KEY_AUTO_ASSIGN_DAILY_MINUTES, "0").expect("set");
        set(&db.conn, KEY_AUTO_ASSIGN_HORIZON_DAYS, "-3").expect("set");
        set(&db.conn, KEY_AUTO_ASSIGN_DEFAULT_TASK_MINUTES, "lots").expect("set");
        assert_eq!(auto_assign_daily_minutes(&db.conn).expect("get"), 120);
        assert_eq!(auto_assign_horizon_days(&db.conn).expect("get"), 14);
        assert_eq!(auto_assign_default_task_minutes(&db.conn).expect("get"), 30);
    }

    // --- Time-blocking settings ---------------------------------------------

    #[test]
    fn time_blocking_settings_default_when_untouched() {
        let db = Database::open_in_memory().expect("open in-memory db");
        assert!(!time_blocking_enabled(&db.conn).unwrap(), "off until someone turns it on");
        assert_eq!(work_window_minutes(&db.conn).unwrap(), (540, 1260));
        assert_eq!(slot_granularity_minutes(&db.conn).unwrap(), 15);
        assert_eq!(min_block_minutes(&db.conn).unwrap(), 25);
        assert_eq!(event_buffer_minutes(&db.conn).unwrap(), 0);
        assert_eq!(max_chunks(&db.conn).unwrap(), 6, "split as necessary unless told otherwise");
    }

    #[test]
    fn time_blocking_settings_round_trip() {
        let db = Database::open_in_memory().expect("open in-memory db");
        set_time_blocking_enabled(&db.conn, true).unwrap();
        set_work_window_minutes(&db.conn, 480, 1080).unwrap();
        set_slot_granularity_minutes(&db.conn, 5).unwrap();
        set_min_block_minutes(&db.conn, 20).unwrap();
        set_event_buffer_minutes(&db.conn, 10).unwrap();
        set_max_chunks(&db.conn, 3).unwrap();

        assert!(time_blocking_enabled(&db.conn).unwrap());
        assert_eq!(work_window_minutes(&db.conn).unwrap(), (480, 1080));
        assert_eq!(slot_granularity_minutes(&db.conn).unwrap(), 5);
        assert_eq!(min_block_minutes(&db.conn).unwrap(), 20);
        assert_eq!(event_buffer_minutes(&db.conn).unwrap(), 10);
        assert_eq!(max_chunks(&db.conn).unwrap(), 3);
    }

    #[test]
    fn an_invalid_work_window_falls_back_to_the_defaults_together() {
        let db = Database::open_in_memory().expect("open in-memory db");
        // End before start.
        set_work_window_minutes(&db.conn, 1000, 600).unwrap();
        assert_eq!(work_window_minutes(&db.conn).unwrap(), (540, 1260));
        // End past midnight.
        set_work_window_minutes(&db.conn, 540, 1500).unwrap();
        assert_eq!(work_window_minutes(&db.conn).unwrap(), (540, 1260));
    }

    #[test]
    fn an_unparseable_work_start_falls_back_on_its_own_when_the_result_is_still_a_window() {
        let db = Database::open_in_memory().expect("open in-memory db");
        // Per-key "invalid -> default": the start becomes 09:00, which with the
        // stored 10:00 end is a real window, so the end is kept.
        set(&db.conn, "work_start_minutes", "soon").unwrap();
        set(&db.conn, "work_end_minutes", "600").unwrap();
        assert_eq!(work_window_minutes(&db.conn).unwrap(), (540, 600));

        // But if the default start would not precede the stored end, it is not a window.
        set(&db.conn, "work_end_minutes", "500").unwrap();
        assert_eq!(work_window_minutes(&db.conn).unwrap(), (540, 1260));
    }

    #[test]
    fn invalid_block_numbers_fall_back_to_their_defaults() {
        let db = Database::open_in_memory().expect("open in-memory db");
        set(&db.conn, "slot_granularity_minutes", "0").unwrap();
        assert_eq!(slot_granularity_minutes(&db.conn).unwrap(), 15);
        set(&db.conn, "slot_granularity_minutes", "90").unwrap();
        assert_eq!(slot_granularity_minutes(&db.conn).unwrap(), 15, "coarser than an hour is nonsense");
        set(&db.conn, "min_block_minutes", "-5").unwrap();
        assert_eq!(min_block_minutes(&db.conn).unwrap(), 25);
        set(&db.conn, "event_buffer_minutes", "-1").unwrap();
        assert_eq!(event_buffer_minutes(&db.conn).unwrap(), 0);
        set(&db.conn, "event_buffer_minutes", "lots").unwrap();
        assert_eq!(event_buffer_minutes(&db.conn).unwrap(), 0);
        set(&db.conn, "max_chunks", "0").unwrap();
        assert_eq!(max_chunks(&db.conn).unwrap(), 6);
    }

    #[test]
    fn a_zero_event_buffer_is_a_real_value_not_unset() {
        let db = Database::open_in_memory().expect("open in-memory db");
        set_event_buffer_minutes(&db.conn, 15).unwrap();
        set_event_buffer_minutes(&db.conn, 0).unwrap();
        assert_eq!(event_buffer_minutes(&db.conn).unwrap(), 0);
    }

    #[test]
    fn auto_replan_is_off_until_turned_on_and_round_trips() {
        let db = Database::open_in_memory().expect("open in-memory db");
        assert!(!auto_replan_blocks(&db.conn).unwrap());
        set_auto_replan_blocks(&db.conn, true).unwrap();
        assert!(auto_replan_blocks(&db.conn).unwrap());
        set_auto_replan_blocks(&db.conn, false).unwrap();
        assert!(!auto_replan_blocks(&db.conn).unwrap());
    }

    #[test]
    fn block_reminders_default_on_and_round_trip() {
        let db = Database::open_in_memory().expect("open in-memory db");
        assert!(block_reminders_enabled(&db.conn).unwrap(), "on unless turned off");
        set_block_reminders_enabled(&db.conn, false).unwrap();
        assert!(!block_reminders_enabled(&db.conn).unwrap());
        set_block_reminders_enabled(&db.conn, true).unwrap();
        assert!(block_reminders_enabled(&db.conn).unwrap());
    }

    // --- Per-weekday working hours (Q1) --------------------------------------------

    fn monday() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 12).unwrap()
    }

    #[test]
    fn every_day_follows_the_default_window_until_overridden() {
        let db = Database::open_in_memory().expect("open in-memory db");
        for offset in 0..7 {
            assert_eq!(
                work_day_for(&db.conn, monday() + chrono::Duration::days(offset)).unwrap(),
                WorkDay::Hours(540, 1260)
            );
        }
        assert_eq!(weekday_override(&db.conn, Weekday::Mon).unwrap(), None);
    }

    #[test]
    fn a_weekday_can_be_off_or_have_hours_of_its_own() {
        let db = Database::open_in_memory().expect("open in-memory db");
        set_weekday_override(&db.conn, Weekday::Sun, Some(WorkDay::Off)).unwrap();
        set_weekday_override(&db.conn, Weekday::Fri, Some(WorkDay::Hours(540, 780))).unwrap();

        let sunday = monday() + chrono::Duration::days(6);
        let friday = monday() + chrono::Duration::days(4);
        assert_eq!(work_day_for(&db.conn, sunday).unwrap(), WorkDay::Off);
        assert_eq!(work_day_for(&db.conn, friday).unwrap(), WorkDay::Hours(540, 780));
        assert_eq!(work_day_for(&db.conn, monday()).unwrap(), WorkDay::Hours(540, 1260), "others unchanged");
        // The same weekday a week later is the same.
        assert_eq!(work_day_for(&db.conn, sunday + chrono::Duration::days(7)).unwrap(), WorkDay::Off);
    }

    #[test]
    fn clearing_an_override_returns_the_day_to_the_default() {
        let db = Database::open_in_memory().expect("open in-memory db");
        set_weekday_override(&db.conn, Weekday::Sat, Some(WorkDay::Off)).unwrap();
        set_weekday_override(&db.conn, Weekday::Sat, None).unwrap();
        assert_eq!(weekday_override(&db.conn, Weekday::Sat).unwrap(), None);
    }

    #[test]
    fn an_unreal_stored_override_is_ignored_not_guessed() {
        let db = Database::open_in_memory().expect("open in-memory db");
        for bad in ["", "soon", "900-600", "540-1500", "-5-100", "540", "540-540"] {
            set(&db.conn, "work_hours_0", bad).unwrap();
            assert_eq!(weekday_override(&db.conn, Weekday::Mon).unwrap(), None, "{bad:?}");
            assert_eq!(work_day_for(&db.conn, monday()).unwrap(), WorkDay::Hours(540, 1260), "{bad:?}");
        }
    }

    #[test]
    fn a_weekday_override_ignores_changes_to_the_default_but_other_days_follow_it() {
        let db = Database::open_in_memory().expect("open in-memory db");
        set_weekday_override(&db.conn, Weekday::Fri, Some(WorkDay::Hours(600, 700))).unwrap();
        set_work_window_minutes(&db.conn, 480, 1080).unwrap();

        assert_eq!(work_day_for(&db.conn, monday() + chrono::Duration::days(4)).unwrap(), WorkDay::Hours(600, 700));
        assert_eq!(work_day_for(&db.conn, monday()).unwrap(), WorkDay::Hours(480, 1080));
    }

    #[test]
    fn block_start_tolerance_defaults_to_fifteen_and_rejects_nonsense() {
        let db = Database::open_in_memory().expect("open in-memory db");
        assert_eq!(block_start_tolerance_minutes(&db.conn).unwrap(), 15);
        set_block_start_tolerance_minutes(&db.conn, 30).unwrap();
        assert_eq!(block_start_tolerance_minutes(&db.conn).unwrap(), 30);
        set(&db.conn, "block_start_tolerance_minutes", "0").unwrap();
        assert_eq!(block_start_tolerance_minutes(&db.conn).unwrap(), 15);
        set(&db.conn, "block_start_tolerance_minutes", "soon").unwrap();
        assert_eq!(block_start_tolerance_minutes(&db.conn).unwrap(), 15);
    }

    #[test]
    fn startup_quiz_defaults_to_on_with_five_questions() {
        let db = Database::open_in_memory().expect("open in-memory db");
        assert!(startup_quiz_enabled(&db.conn).expect("get"));
        assert_eq!(startup_quiz_questions(&db.conn).expect("get"), 5);
    }

    #[test]
    fn startup_quiz_settings_round_trip() {
        let db = Database::open_in_memory().expect("open in-memory db");
        set_startup_quiz_enabled(&db.conn, false).expect("set");
        set_startup_quiz_questions(&db.conn, 12).expect("set");
        assert!(!startup_quiz_enabled(&db.conn).expect("get"));
        assert_eq!(startup_quiz_questions(&db.conn).expect("get"), 12);

        set_startup_quiz_enabled(&db.conn, true).expect("set back");
        assert!(startup_quiz_enabled(&db.conn).expect("get"));
    }

    #[test]
    fn a_bad_stored_question_count_falls_back_and_a_huge_one_is_capped() {
        let db = Database::open_in_memory().expect("open in-memory db");
        for bad in ["0", "-3", "lots", ""] {
            set(&db.conn, KEY_STARTUP_QUIZ_QUESTIONS, bad).expect("set raw");
            assert_eq!(startup_quiz_questions(&db.conn).expect("get"), 5, "{bad:?}");
        }
        set_startup_quiz_questions(&db.conn, 10_000).expect("set");
        assert_eq!(startup_quiz_questions(&db.conn).expect("get"), crate::flashcards::quiz::MAX_QUESTIONS);
    }

    #[test]
    fn past_dim_defaults_to_medium_and_round_trips() {
        let db = Database::open_in_memory().expect("open in-memory db");
        assert_eq!(calendar_past_dim_percent(&db.conn).expect("get"), 40);
        set_calendar_past_dim_percent(&db.conn, 0).expect("set");
        assert_eq!(calendar_past_dim_percent(&db.conn).expect("get"), 0, "zero is a real value, not unset");
        set_calendar_past_dim_percent(&db.conn, 75).expect("set");
        assert_eq!(calendar_past_dim_percent(&db.conn).expect("get"), 75);
    }

    #[test]
    fn past_dim_is_capped_and_bad_values_fall_back() {
        let db = Database::open_in_memory().expect("open in-memory db");
        set_calendar_past_dim_percent(&db.conn, 100).expect("set");
        assert_eq!(calendar_past_dim_percent(&db.conn).expect("get"), MAX_PAST_DIM_PERCENT);
        for bad in ["-5", "lots", "", "300"] {
            set(&db.conn, KEY_CALENDAR_PAST_DIM_PERCENT, bad).expect("set raw");
            assert_eq!(calendar_past_dim_percent(&db.conn).expect("get"), 40, "{bad:?}");
        }
    }
}
