use chrono::{DateTime, Local, NaiveDate};

/// True if `now` falls inside the reminder window for a session
/// starting at `event_start`: from `lead_minutes` before start, up to
/// (not including) the start itself. `lead_minutes <= 0` disables
/// reminders entirely (there's no "negative warning").
///
/// Pure and independent of D-Bus, the database, or which occurrence
/// this is — `app`'s polling loop is what turns "is now in the
/// window" into "have I already shown this one", since that requires
/// state this function deliberately doesn't have.
pub fn should_remind_for_event(now: DateTime<Local>, event_start: DateTime<Local>, lead_minutes: i64) -> bool {
    if lead_minutes <= 0 {
        return false;
    }
    let window_start = event_start - chrono::Duration::minutes(lead_minutes);
    now >= window_start && now < event_start
}

/// True if a task due today, not yet completed, should get its
/// once-a-day due-date reminder. Unlike event reminders (a precise
/// minutes-before window), a task reminder just needs "today, and not
/// done yet" — there's no meaningful "starts at" time to count down
/// from.
pub fn should_remind_for_task(today: NaiveDate, due_date: Option<NaiveDate>, completed: bool) -> bool {
    !completed && due_date == Some(today)
}

/// True if `event_start`'s startup command is due to run: at or after
/// its start time, but within a short grace window afterward — not
/// unboundedly, so reopening the app hours after a session's start
/// doesn't fire a now-stale "open the browser" command well after the
/// moment it made sense. Independent of whether it's *allowed* to run
/// at all (`queries::settings::exec_commands_enabled`) — that's a
/// separate, coarser gate checked by the caller.
pub fn should_run_exec_command(now: DateTime<Local>, event_start: DateTime<Local>) -> bool {
    const GRACE_MINUTES: i64 = 15;
    now >= event_start && now < event_start + chrono::Duration::minutes(GRACE_MINUTES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn fires_inside_the_lead_window() {
        let start = at(2026, 8, 25, 16, 0);
        assert!(should_remind_for_event(at(2026, 8, 25, 15, 55), start, 10));
        assert!(should_remind_for_event(at(2026, 8, 25, 15, 50), start, 10));
    }

    #[test]
    fn does_not_fire_before_the_window_opens() {
        let start = at(2026, 8, 25, 16, 0);
        assert!(!should_remind_for_event(at(2026, 8, 25, 15, 49), start, 10));
    }

    #[test]
    fn does_not_fire_once_the_session_has_started() {
        let start = at(2026, 8, 25, 16, 0);
        assert!(!should_remind_for_event(at(2026, 8, 25, 16, 0), start, 10));
        assert!(!should_remind_for_event(at(2026, 8, 25, 16, 30), start, 10));
    }

    #[test]
    fn zero_or_negative_lead_time_disables_reminders() {
        let start = at(2026, 8, 25, 16, 0);
        assert!(!should_remind_for_event(at(2026, 8, 25, 15, 59), start, 0));
        assert!(!should_remind_for_event(at(2026, 8, 25, 15, 59), start, -5));
    }

    #[test]
    fn task_due_today_and_incomplete_gets_reminded() {
        let today = NaiveDate::from_ymd_opt(2026, 8, 25).unwrap();
        assert!(should_remind_for_task(today, Some(today), false));
    }

    #[test]
    fn completed_task_never_gets_reminded() {
        let today = NaiveDate::from_ymd_opt(2026, 8, 25).unwrap();
        assert!(!should_remind_for_task(today, Some(today), true));
    }

    #[test]
    fn task_due_a_different_day_is_not_reminded() {
        let today = NaiveDate::from_ymd_opt(2026, 8, 25).unwrap();
        let tomorrow = NaiveDate::from_ymd_opt(2026, 8, 26).unwrap();
        let yesterday = NaiveDate::from_ymd_opt(2026, 8, 24).unwrap();
        assert!(!should_remind_for_task(today, Some(tomorrow), false));
        assert!(!should_remind_for_task(today, Some(yesterday), false));
    }

    #[test]
    fn task_with_no_due_date_is_never_reminded() {
        let today = NaiveDate::from_ymd_opt(2026, 8, 25).unwrap();
        assert!(!should_remind_for_task(today, None, false));
    }

    #[test]
    fn exec_command_is_due_right_at_start() {
        let start = at(2026, 8, 25, 9, 0);
        assert!(should_run_exec_command(start, start));
        assert!(should_run_exec_command(at(2026, 8, 25, 9, 10), start));
    }

    #[test]
    fn exec_command_is_not_due_before_start() {
        let start = at(2026, 8, 25, 9, 0);
        assert!(!should_run_exec_command(at(2026, 8, 25, 8, 59), start));
    }

    #[test]
    fn exec_command_is_no_longer_due_well_after_start() {
        // Reopening the app hours after a session's start shouldn't
        // fire a now-stale "open the browser" command.
        let start = at(2026, 8, 25, 9, 0);
        assert!(!should_run_exec_command(at(2026, 8, 25, 9, 30), start));
    }
}
