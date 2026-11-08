pub mod migrations;
pub mod queries;

use std::path::PathBuf;

use rusqlite::{Connection, Result};

/// Owns the single SQLite connection for the app's lifetime.
/// Sprint 2 added tasks; Sprint 3 adds events (revision sessions).
/// Later sprints add tables for notes and subjects via additional
/// migrations.
pub struct Database {
    pub conn: Connection,
}

impl Database {
    /// Opens (creating if necessary) the on-disk database at the path
    /// specified in the plan: `~/.local/share/revise/revise.db`.
    pub fn open_default() -> Result<Self> {
        let path = default_db_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        Self::open_at(path)
    }

    pub fn open_at(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(path)?;
        migrations::run(&conn)?;
        Ok(Self { conn })
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        migrations::run(&conn)?;
        Ok(Self { conn })
    }
}

fn default_db_path() -> PathBuf {
    let base = home::home_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join(".local/share/revise/revise.db")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_idempotent() {
        let db = Database::open_in_memory().expect("open in-memory db");
        // Running migrations again on the same connection should be a no-op.
        migrations::run(&db.conn).expect("re-run migrations");
    }

    #[test]
    fn create_list_complete_update_delete_roundtrip() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;

        let created = queries::tasks::create_task(conn, "Review networking", Some("2026-09-01"), None, None).expect("create");
        assert_eq!(created.position, 0);
        assert!(!created.completed);

        let second = queries::tasks::create_task(conn, "Past papers", None, None, None).expect("create second");
        assert_eq!(second.position, 1);

        let tasks = queries::tasks::list_tasks(conn).expect("list");
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id, created.id);

        queries::tasks::set_completed(conn, created.id, true).expect("complete");
        let tasks = queries::tasks::list_tasks(conn).expect("list after complete");
        assert!(tasks.iter().find(|t| t.id == created.id).unwrap().completed);

        queries::tasks::update_task(conn, second.id, "Past papers (2020-2023)", "Focus on Q3 style", Some("2026-09-05"), None, None, false, None, None)
            .expect("update");
        let tasks = queries::tasks::list_tasks(conn).expect("list after update");
        let updated = tasks.iter().find(|t| t.id == second.id).unwrap();
        assert_eq!(updated.title, "Past papers (2020-2023)");
        assert_eq!(updated.due_date.as_deref(), Some("2026-09-05"));

        queries::tasks::delete_task(conn, created.id).expect("delete");
        let tasks = queries::tasks::list_tasks(conn).expect("list after delete");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, second.id);
    }

    #[test]
    fn tasks_survive_reopening_the_same_file() {
        // Simulates an application restart: open, write, drop the
        // connection entirely, then reopen the same on-disk file and
        // confirm the data is still there (Sprint 2 acceptance
        // criterion: "Restarting the application does not lose tasks").
        let dir = std::env::temp_dir().join(format!("revise-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("revise.db");

        {
            let db = Database::open_at(db_path.clone()).expect("open db (first run)");
            queries::tasks::create_task(&db.conn, "Survive a restart", None, None, None).expect("create");
        } // `db` dropped here, closing the connection.

        {
            let db = Database::open_at(db_path.clone()).expect("open db (second run)");
            let tasks = queries::tasks::list_tasks(&db.conn).expect("list after reopen");
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks[0].title, "Survive a restart");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn events_roundtrip_and_survive_reopening() {
        use crate::database::queries::events;
        use chrono::{Local, TimeZone};

        let dir = std::env::temp_dir().join(format!("revise-events-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("revise.db");

        let start = Local.with_ymd_and_hms(2026, 8, 24, 14, 0, 0).unwrap();
        let end = Local.with_ymd_and_hms(2026, 8, 24, 15, 0, 0).unwrap();

        let (created_id, subject_id) = {
            let db = Database::open_at(db_path.clone()).expect("open db (first run)");
            let subject = queries::subjects::create_subject(&db.conn, "CS").expect("create subject");
            let created =
                events::create_event(&db.conn, "Revision — CS", "Networking", start, end, Some(subject.id), None)
                    .expect("create event");
            assert_eq!(created.duration(), chrono::Duration::hours(1));
            assert!(!created.crosses_midnight());
            (created.id, subject.id)
        };

        {
            let db = Database::open_at(db_path.clone()).expect("open db (second run)");
            let all = events::list_all_events(&db.conn).expect("list after reopen");
            assert_eq!(all.len(), 1);
            let event = &all[0];
            assert_eq!(event.id, created_id);
            assert_eq!(event.title, "Revision — CS");
            assert_eq!(event.subject_id, Some(subject_id));
            assert_eq!(event.start_time, start);
            assert_eq!(event.end_time, end);

            events::update_event(
                &db.conn,
                event.id,
                "Revision — CS (moved)",
                "Networking",
                start,
                end,
                Some(subject_id),
                None,
            )
            .expect("update");
            let updated = events::list_all_events(&db.conn).expect("list after update");
            assert_eq!(updated[0].title, "Revision — CS (moved)");

            events::delete_event(&db.conn, event.id).expect("delete");
            let after_delete = events::list_all_events(&db.conn).expect("list after delete");
            assert!(after_delete.is_empty());
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn event_crossing_midnight_has_positive_duration() {
        use crate::database::queries::events;
        use chrono::{Local, TimeZone};

        let db = Database::open_in_memory().expect("open in-memory db");
        let start = Local.with_ymd_and_hms(2026, 8, 24, 23, 30, 0).unwrap();
        let end = Local.with_ymd_and_hms(2026, 8, 25, 0, 30, 0).unwrap();

        let event = events::create_event(&db.conn, "Late review", "", start, end, None, None).expect("create");
        assert!(event.crosses_midnight());
        assert_eq!(event.duration(), chrono::Duration::hours(1));
        assert_eq!(event.day(), start.date_naive());
    }

    #[test]
    fn track_session_defaults_true_and_can_be_turned_off() {
        // Feature request: "Event Logging" — new events keep today's
        // behavior by default (Done/Not-Done/Start Session all show),
        // but a meeting-style block can opt out at creation.
        use crate::database::queries::events;
        use chrono::{Local, TimeZone};

        let db = Database::open_in_memory().expect("open in-memory db");
        let start = Local.with_ymd_and_hms(2026, 8, 24, 14, 0, 0).unwrap();
        let end = start + chrono::Duration::hours(1);

        let event = events::create_event(&db.conn, "Team meeting", "", start, end, None, None).expect("create");
        assert!(event.track_session, "new events default to tracked");

        events::set_track_session(&db.conn, event.id, false).expect("opt out");
        let reloaded =
            events::list_all_events(&db.conn).expect("list").into_iter().find(|e| e.id == event.id).expect("found");
        assert!(!reloaded.track_session);
    }

    #[test]
    fn track_session_survives_reopening_a_pre_existing_database() {
        // The migration's DEFAULT 1 must apply to rows that existed
        // before the column did, not just new inserts — otherwise
        // every event created before this feature shipped would
        // silently lose its Done/Not-Done/Start Session controls.
        use crate::database::queries::events;
        use chrono::{Local, TimeZone};

        let dir = std::env::temp_dir().join(format!("revise-track-session-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("revise.db");

        {
            let db = Database::open_at(db_path.clone()).expect("open db (first run)");
            let start = Local.with_ymd_and_hms(2026, 8, 24, 14, 0, 0).unwrap();
            events::create_event(&db.conn, "Old event", "", start, start + chrono::Duration::hours(1), None, None)
                .expect("create");
        }

        {
            let db = Database::open_at(db_path.clone()).expect("open db (second run)");
            let events = events::list_all_events(&db.conn).expect("list");
            assert!(events[0].track_session);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn task_can_attach_to_and_detach_from_an_event() {
        use crate::database::queries::events;
        use chrono::{Local, TimeZone};

        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;

        let start = Local.with_ymd_and_hms(2026, 8, 24, 10, 0, 0).unwrap();
        let end = Local.with_ymd_and_hms(2026, 8, 24, 11, 0, 0).unwrap();
        let event = events::create_event(conn, "Revision CS", "", start, end, None, None).expect("create event");

        // A task can be created already attached to a session...
        let task_a = queries::tasks::create_task(conn, "Review networking", None, Some(event.id), None).expect("create a");
        assert_eq!(task_a.event_id, Some(event.id));

        // ...or attached afterwards.
        let task_b = queries::tasks::create_task(conn, "Complete questions", None, None, None).expect("create b");
        assert_eq!(task_b.event_id, None);
        queries::tasks::set_event(conn, task_b.id, Some(event.id)).expect("attach b");

        let for_event = queries::tasks::list_tasks_for_event(conn, event.id).expect("list for event");
        assert_eq!(for_event.len(), 2);

        // Completing one of them is visible via a completely fresh
        // query — the same guarantee that keeps Dashboard, Calendar,
        // and To Do in sync without any manual cache invalidation.
        queries::tasks::set_completed(conn, task_a.id, true).expect("complete a");
        let for_event = queries::tasks::list_tasks_for_event(conn, event.id).expect("list after complete");
        let completed_count = for_event.iter().filter(|t| t.completed).count();
        assert_eq!(completed_count, 1);
        assert_eq!(for_event.len(), 2);

        // Detaching leaves the task intact, just unlinked.
        queries::tasks::set_event(conn, task_b.id, None).expect("detach b");
        let for_event = queries::tasks::list_tasks_for_event(conn, event.id).expect("list after detach");
        assert_eq!(for_event.len(), 1);
        let all_tasks = queries::tasks::list_tasks(conn).expect("list all");
        assert_eq!(all_tasks.len(), 2, "detaching must not delete the task");
    }

    #[test]
    fn deleting_an_event_unlinks_its_tasks_without_deleting_them() {
        use crate::database::queries::events;
        use chrono::{Local, TimeZone};

        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;

        let start = Local.with_ymd_and_hms(2026, 8, 24, 10, 0, 0).unwrap();
        let end = Local.with_ymd_and_hms(2026, 8, 24, 11, 0, 0).unwrap();
        let event = events::create_event(conn, "Revision CS", "", start, end, None, None).expect("create event");
        let task = queries::tasks::create_task(conn, "Review networking", None, Some(event.id), None).expect("create task");

        events::delete_event(conn, event.id).expect("delete event");

        let all_tasks = queries::tasks::list_tasks(conn).expect("list all");
        assert_eq!(all_tasks.len(), 1, "task must survive the event's deletion");
        assert_eq!(all_tasks[0].id, task.id);
        assert_eq!(all_tasks[0].event_id, None, "task must be unlinked, not left dangling");
    }

    #[test]
    fn migration_v3_adds_event_id_to_a_pre_existing_v2_database() {
        // Simulates an install that already has Sprint 2/3 data on
        // disk (tasks + events, no `event_id` column yet) opening the
        // app after upgrading to Sprint 4's build.
        let dir = std::env::temp_dir().join(format!("revise-migration-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("revise.db");

        {
            // Manually build a v2 schema (no event_id) and seed a task,
            // bypassing `migrations::run` to pin it at v2.
            let conn = Connection::open(&db_path).expect("open v2 db");
            conn.execute_batch(
                r#"
                CREATE TABLE tasks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    completed INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    due_date TEXT,
                    position INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    start_time TEXT NOT NULL,
                    end_time TEXT NOT NULL,
                    subject TEXT,
                    recurrence_id INTEGER
                );
                "#,
            )
            .expect("seed v2 schema");
            conn.execute(
                "INSERT INTO tasks (title, description, completed, created_at, position) VALUES ('Pre-existing task', '', 0, '2026-01-01T00:00:00+00:00', 0)",
                [],
            )
            .expect("seed a pre-existing task");
            conn.pragma_update(None, "user_version", 2).expect("pin at v2");
        }

        {
            // Opening via `Database::open_at` runs migrations, which
            // must apply only the missing v3 step without touching
            // the existing row.
            let db = Database::open_at(db_path.clone()).expect("open and migrate to v3");
            let tasks = queries::tasks::list_tasks(&db.conn).expect("list after migration");
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks[0].title, "Pre-existing task");
            assert_eq!(tasks[0].event_id, None, "new column should default to NULL");

            // And the new functionality works immediately on the
            // migrated table.
            queries::tasks::set_event(&db.conn, tasks[0].id, None).expect("no-op set_event still works");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn completing_a_task_records_its_date_uncompleting_clears_it() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;

        let task = queries::tasks::create_task(conn, "Review networking", None, None, None).expect("create");
        assert!(queries::tasks::list_completed_dates(conn).expect("list dates").is_empty());

        queries::tasks::set_completed(conn, task.id, true).expect("complete");
        let dates = queries::tasks::list_completed_dates(conn).expect("list dates after complete");
        assert_eq!(dates.len(), 1);
        assert_eq!(dates[0], chrono::Local::now().date_naive());

        let tasks = queries::tasks::list_tasks(conn).expect("list tasks");
        assert!(tasks[0].completed_at.is_some());

        queries::tasks::set_completed(conn, task.id, false).expect("uncomplete");
        assert!(queries::tasks::list_completed_dates(conn).expect("list dates after uncomplete").is_empty());
        let tasks = queries::tasks::list_tasks(conn).expect("list tasks after uncomplete");
        assert!(tasks[0].completed_at.is_none());
    }

    #[test]
    fn assigning_a_task_to_a_day_is_independent_of_its_due_date() {
        // The core of "Task Assignment": a task's assigned day and its
        // due date are two separate answers, not one repurposed as
        // the other.
        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;

        let task = queries::tasks::create_task(conn, "Read chapter 4", Some("2026-12-25"), None, None)
            .expect("create");
        assert_eq!(queries::tasks::list_tasks(conn).expect("list")[0].assigned_date, None);

        queries::tasks::set_assigned_date(conn, task.id, Some("2026-09-01")).expect("assign");
        let tasks = queries::tasks::list_tasks(conn).expect("list after assign");
        assert_eq!(tasks[0].assigned_date.as_deref(), Some("2026-09-01"));
        assert_eq!(tasks[0].due_date.as_deref(), Some("2026-12-25"), "assigning a day must not touch due_date");

        queries::tasks::set_assigned_date(conn, task.id, None).expect("unassign");
        assert_eq!(queries::tasks::list_tasks(conn).expect("list after unassign")[0].assigned_date, None);
    }

    #[test]
    fn migration_v4_adds_completed_at_to_a_pre_existing_v3_database() {
        // Same idea as the v3 test: seed a v3-shaped database by hand
        // (tasks + events + event_id, no completed_at) and confirm
        // opening it applies only the missing step.
        let dir = std::env::temp_dir().join(format!("revise-migration-v4-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("revise.db");

        {
            let conn = Connection::open(&db_path).expect("open v3 db");
            conn.execute_batch(
                r#"
                CREATE TABLE tasks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    completed INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    due_date TEXT,
                    position INTEGER NOT NULL DEFAULT 0,
                    event_id INTEGER
                );
                CREATE TABLE events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    start_time TEXT NOT NULL,
                    end_time TEXT NOT NULL,
                    subject TEXT,
                    recurrence_id INTEGER
                );
                "#,
            )
            .expect("seed v3 schema");
            conn.execute(
                "INSERT INTO tasks (title, description, completed, created_at, position) VALUES ('Already completed', '', 1, '2026-01-01T00:00:00+00:00', 0)",
                [],
            )
            .expect("seed a completed task with no completed_at yet");
            conn.pragma_update(None, "user_version", 3).expect("pin at v3");
        }

        let db = Database::open_at(db_path.clone()).expect("open and migrate to v4");
        let tasks = queries::tasks::list_tasks(&db.conn).expect("list after migration");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].completed_at, None, "new column defaults to NULL even for already-completed tasks");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recurrence_rule_and_exception_roundtrip() {
        use crate::database::queries::{events, recurrence};
        use crate::events::{Frequency, RecurrenceRule};
        use chrono::{Local, TimeZone, Weekday};

        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;

        let rule = RecurrenceRule {
            id: None,
            frequency: Frequency::Weekly,
            interval: 1,
            weekdays: vec![Weekday::Tue, Weekday::Thu],
            end_date: None,
            occurrence_limit: Some(10),
        };
        let rule_id = recurrence::create_rule(conn, &rule).expect("create rule");

        let start = Local.with_ymd_and_hms(2026, 8, 25, 10, 0, 0).unwrap(); // a Tuesday
        let end = Local.with_ymd_and_hms(2026, 8, 25, 11, 0, 0).unwrap();
        let event = events::create_event(conn, "Revision CS", "", start, end, None, Some(rule_id))
            .expect("create recurring event");
        assert_eq!(event.recurrence_id, Some(rule_id));

        let fetched = recurrence::get_rule(conn, rule_id).expect("get rule").expect("rule exists");
        assert_eq!(fetched.frequency, Frequency::Weekly);
        assert_eq!(fetched.weekdays, vec![Weekday::Tue, Weekday::Thu]);
        assert_eq!(fetched.occurrence_limit, Some(10));

        // Skip one occurrence without touching the rule.
        let skipped_date = start.date_naive() + chrono::Duration::days(7); // the following Tuesday
        recurrence::add_exception(conn, event.id, skipped_date).expect("add exception");
        let exceptions = recurrence::list_exceptions(conn, event.id).expect("list exceptions");
        assert!(exceptions.contains(&skipped_date));

        let rule_after_skip = recurrence::get_rule(conn, rule_id).expect("get rule after skip").unwrap();
        assert_eq!(rule_after_skip, fetched, "skipping an occurrence must not change the rule");

        // Updating the rule in place keeps the same id.
        let mut updated_rule = rule.clone();
        updated_rule.interval = 2;
        recurrence::update_rule(conn, rule_id, &updated_rule).expect("update rule");
        let refetched = recurrence::get_rule(conn, rule_id).expect("get rule after update").unwrap();
        assert_eq!(refetched.interval, 2);
        assert_eq!(refetched.id, Some(rule_id));
    }

    #[test]
    fn deleting_a_recurring_event_cleans_up_its_rule_and_exceptions() {
        use crate::database::queries::{events, recurrence};
        use crate::events::{Frequency, RecurrenceRule};
        use chrono::{Local, TimeZone};

        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;

        let rule = RecurrenceRule {
            id: None,
            frequency: Frequency::Daily,
            interval: 1,
            weekdays: Vec::new(),
            end_date: None,
            occurrence_limit: None,
        };
        let rule_id = recurrence::create_rule(conn, &rule).expect("create rule");
        let start = Local.with_ymd_and_hms(2026, 8, 25, 9, 0, 0).unwrap();
        let end = Local.with_ymd_and_hms(2026, 8, 25, 10, 0, 0).unwrap();
        let event =
            events::create_event(conn, "Daily review", "", start, end, None, Some(rule_id)).expect("create event");
        recurrence::add_exception(conn, event.id, start.date_naive() + chrono::Duration::days(2))
            .expect("add exception");

        events::delete_event(conn, event.id).expect("delete event");

        assert!(recurrence::get_rule(conn, rule_id).expect("get rule after delete").is_none());
        assert!(recurrence::list_exceptions(conn, event.id).expect("list exceptions after delete").is_empty());
    }

    #[test]
    fn migration_v5_adds_recurrence_tables_to_a_pre_existing_v4_database() {
        let dir = std::env::temp_dir().join(format!("revise-migration-v5-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("revise.db");

        {
            let conn = Connection::open(&db_path).expect("open v4 db");
            conn.execute_batch(
                r#"
                CREATE TABLE tasks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    completed INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    due_date TEXT,
                    position INTEGER NOT NULL DEFAULT 0,
                    event_id INTEGER,
                    completed_at TEXT
                );
                CREATE TABLE events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    start_time TEXT NOT NULL,
                    end_time TEXT NOT NULL,
                    subject TEXT,
                    recurrence_id INTEGER
                );
                "#,
            )
            .expect("seed v4 schema");
            conn.execute(
                "INSERT INTO events (title, description, start_time, end_time) VALUES ('Existing event', '', '2026-01-01T09:00:00+00:00', '2026-01-01T10:00:00+00:00')",
                [],
            )
            .expect("seed a pre-existing event");
            conn.pragma_update(None, "user_version", 4).expect("pin at v4");
        }

        let db = Database::open_at(db_path.clone()).expect("open and migrate to v5");
        let events = queries::events::list_all_events(&db.conn).expect("list after migration");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].recurrence_id, None);

        // The new tables must exist and be usable immediately.
        use crate::events::{Frequency, RecurrenceRule};
        let rule = RecurrenceRule {
            id: None,
            frequency: Frequency::Daily,
            interval: 1,
            weekdays: Vec::new(),
            end_date: None,
            occurrence_limit: None,
        };
        let rule_id = queries::recurrence::create_rule(&db.conn, &rule).expect("create rule post-migration");
        assert!(queries::recurrence::get_rule(&db.conn, rule_id).expect("get rule").is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn notes_roundtrip_and_survive_reopening() {
        use crate::database::queries::notes;

        let dir = std::env::temp_dir().join(format!("revise-notes-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("revise.db");

        let (created_id, subject_id) = {
            let db = Database::open_at(db_path.clone()).expect("open db (first run)");
            let subject = queries::subjects::create_subject(&db.conn, "Physics").expect("create subject");
            let created = notes::create_note(
                &db.conn,
                "Electromagnetism",
                "B = mu0 I / 2 pi r",
                Some(subject.id),
                None,
                None,
            )
            .expect("create note");
            (created.id, subject.id)
        };

        {
            let db = Database::open_at(db_path.clone()).expect("open db (second run)");
            let all = notes::list_all_notes(&db.conn).expect("list after reopen");
            assert_eq!(all.len(), 1);
            let note = &all[0];
            assert_eq!(note.id, created_id);
            assert_eq!(note.title, "Electromagnetism");
            assert_eq!(note.subject_id, Some(subject_id));

            notes::update_note(&db.conn, note.id, "Electromagnetism (revised)", "updated content", Some(subject_id), None, None)
                .expect("update");
            let updated = notes::list_all_notes(&db.conn).expect("list after update");
            assert_eq!(updated[0].title, "Electromagnetism (revised)");
            assert_eq!(updated[0].content, "updated content");

            notes::delete_note(&db.conn, note.id).expect("delete");
            assert!(notes::list_all_notes(&db.conn).expect("list after delete").is_empty());
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn notes_can_attach_to_an_event_and_be_listed_by_it() {
        use crate::database::queries::{events, notes};
        use chrono::{Local, TimeZone};

        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;

        let start = Local.with_ymd_and_hms(2026, 8, 24, 10, 0, 0).unwrap();
        let end = Local.with_ymd_and_hms(2026, 8, 24, 11, 0, 0).unwrap();
        let event = events::create_event(conn, "Revision CS", "", start, end, None, None).expect("create event");

        let note_a = notes::create_note(conn, "Session notes", "", None, Some(event.id), None).expect("create note a");
        let _note_b = notes::create_note(conn, "Unrelated", "", None, None, None).expect("create note b");

        let for_event = notes::list_notes_for_event(conn, event.id).expect("list for event");
        assert_eq!(for_event.len(), 1);
        assert_eq!(for_event[0].id, note_a.id);

        let all = notes::list_all_notes(conn).expect("list all");
        assert_eq!(all.len(), 2, "unrelated note must still exist");
    }

    #[test]
    fn migration_v6_adds_notes_table_to_a_pre_existing_v5_database() {
        let dir = std::env::temp_dir().join(format!("revise-migration-v6-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("revise.db");

        {
            let conn = Connection::open(&db_path).expect("open v5 db");
            conn.execute_batch(
                r#"
                CREATE TABLE tasks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    completed INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    due_date TEXT,
                    position INTEGER NOT NULL DEFAULT 0,
                    event_id INTEGER,
                    completed_at TEXT
                );
                CREATE TABLE events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    start_time TEXT NOT NULL,
                    end_time TEXT NOT NULL,
                    subject TEXT,
                    recurrence_id INTEGER
                );
                CREATE TABLE recurrence_rules (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    frequency TEXT NOT NULL,
                    interval INTEGER NOT NULL DEFAULT 1,
                    weekdays TEXT,
                    end_date TEXT,
                    occurrence_limit INTEGER
                );
                CREATE TABLE event_exceptions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    event_id INTEGER NOT NULL,
                    occurrence_date TEXT NOT NULL,
                    UNIQUE(event_id, occurrence_date)
                );
                "#,
            )
            .expect("seed v5 schema");
            conn.pragma_update(None, "user_version", 5).expect("pin at v5");
        }

        let db = Database::open_at(db_path.clone()).expect("open and migrate to v6");
        use crate::database::queries::notes;
        let created = notes::create_note(&db.conn, "Post-migration note", "", None, None, None).expect("create note post-migration");
        let all = notes::list_all_notes(&db.conn).expect("list");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, created.id);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn subject_create_rename_archive_roundtrip() {
        use crate::database::queries::subjects;

        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;

        let created = subjects::create_subject(conn, "Mathematics").expect("create");
        assert!(!created.archived);

        let all = subjects::list_all_subjects(conn).expect("list all");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "Mathematics");

        subjects::rename_subject(conn, created.id, "Further Mathematics").expect("rename");
        let all = subjects::list_all_subjects(conn).expect("list after rename");
        assert_eq!(all[0].name, "Further Mathematics");

        subjects::set_archived(conn, created.id, true).expect("archive");
        let active = subjects::list_active_subjects(conn).expect("list active");
        assert!(active.is_empty(), "archived subject must not appear in the active list");
        let all = subjects::list_all_subjects(conn).expect("list all after archive");
        assert_eq!(all.len(), 1, "archived subject must still exist — historical data stays intact");
        assert!(all[0].archived);

        subjects::set_archived(conn, created.id, false).expect("unarchive");
        let active = subjects::list_active_subjects(conn).expect("list active after unarchive");
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn events_tasks_and_notes_can_share_a_subject() {
        use crate::database::queries::{events, notes, subjects};
        use chrono::{Local, TimeZone};

        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;

        let subject = subjects::create_subject(conn, "Chemistry").expect("create subject");

        let start = Local.with_ymd_and_hms(2026, 8, 24, 10, 0, 0).unwrap();
        let end = Local.with_ymd_and_hms(2026, 8, 24, 11, 0, 0).unwrap();
        let event =
            events::create_event(conn, "Titration practice", "", start, end, Some(subject.id), None).expect("event");
        let task = queries::tasks::create_task(conn, "Balance equations", None, None, Some(subject.id)).expect("task");
        let note = notes::create_note(conn, "Moles", "n = m / M", Some(subject.id), None, None).expect("note");

        assert_eq!(event.subject_id, Some(subject.id));
        assert_eq!(task.subject_id, Some(subject.id));
        assert_eq!(note.subject_id, Some(subject.id));

        // Archiving the subject doesn't touch anything that already
        // references it.
        subjects::set_archived(conn, subject.id, true).expect("archive");
        let all_events = events::list_all_events(conn).expect("list events");
        let all_tasks = queries::tasks::list_tasks(conn).expect("list tasks");
        let all_notes = notes::list_all_notes(conn).expect("list notes");
        assert_eq!(all_events[0].subject_id, Some(subject.id));
        assert_eq!(all_tasks[0].subject_id, Some(subject.id));
        assert_eq!(all_notes[0].subject_id, Some(subject.id));
    }

    #[test]
    fn migration_v7_backfills_existing_free_text_subjects_into_real_subject_rows() {
        // The riskiest part of the v7 migration: a pre-existing
        // database has events/notes with free-text `subject` values
        // (no `subjects` table, no `subject_id` column yet). Opening
        // it must create a `Subject` row for each distinct value and
        // relink the affected rows to it, without losing anything.
        let dir = std::env::temp_dir().join(format!("revise-migration-v7-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("revise.db");

        {
            let conn = Connection::open(&db_path).expect("open v6 db");
            conn.execute_batch(
                r#"
                CREATE TABLE tasks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    completed INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    due_date TEXT,
                    position INTEGER NOT NULL DEFAULT 0,
                    event_id INTEGER,
                    completed_at TEXT
                );
                CREATE TABLE events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    start_time TEXT NOT NULL,
                    end_time TEXT NOT NULL,
                    subject TEXT,
                    recurrence_id INTEGER
                );
                CREATE TABLE notes (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL DEFAULT '',
                    subject TEXT,
                    event_id INTEGER,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                "#,
            )
            .expect("seed v6 schema");
            conn.execute(
                "INSERT INTO events (title, description, start_time, end_time, subject) VALUES ('Revision CS', '', '2026-01-01T09:00:00+00:00', '2026-01-01T10:00:00+00:00', 'Computer Science')",
                [],
            )
            .expect("seed event with free-text subject");
            conn.execute(
                "INSERT INTO events (title, description, start_time, end_time, subject) VALUES ('Revision CS again', '', '2026-01-02T09:00:00+00:00', '2026-01-02T10:00:00+00:00', 'Computer Science')",
                [],
            )
            .expect("seed second event with the same free-text subject");
            conn.execute(
                "INSERT INTO notes (title, content, subject, created_at, updated_at) VALUES ('Big O notes', '', 'Computer Science', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00')",
                [],
            )
            .expect("seed note with the same free-text subject");
            conn.execute(
                "INSERT INTO events (title, description, start_time, end_time, subject) VALUES ('No subject event', '', '2026-01-03T09:00:00+00:00', '2026-01-03T10:00:00+00:00', NULL)",
                [],
            )
            .expect("seed event with no subject");
            conn.pragma_update(None, "user_version", 6).expect("pin at v6");
        }

        let db = Database::open_at(db_path.clone()).expect("open and migrate to v7");

        // Exactly one Subject row was created for the two distinct
        // events + note sharing "Computer Science" — not one per row.
        let all_subjects = queries::subjects::list_all_subjects(&db.conn).expect("list subjects");
        assert_eq!(all_subjects.len(), 1, "distinct free-text values should collapse to one Subject row");
        let subject = &all_subjects[0];
        assert_eq!(subject.name, "Computer Science");

        let all_events = queries::events::list_all_events(&db.conn).expect("list events");
        assert_eq!(all_events.len(), 3);
        let with_subject: Vec<_> = all_events.iter().filter(|e| e.subject_id == Some(subject.id)).collect();
        assert_eq!(with_subject.len(), 2, "both events with the free-text subject must be relinked");
        let without_subject = all_events.iter().find(|e| e.title == "No subject event").unwrap();
        assert_eq!(without_subject.subject_id, None);

        let all_notes = queries::notes::list_all_notes(&db.conn).expect("list notes");
        assert_eq!(all_notes[0].subject_id, Some(subject.id));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_past_events_removes_only_non_recurring_events_before_today() {
        use crate::database::queries::{events, recurrence};
        use crate::events::{Frequency, RecurrenceRule};
        use chrono::{Local, TimeZone};

        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;
        let today = chrono::NaiveDate::from_ymd_opt(2026, 8, 25).unwrap();

        let past_start = Local.with_ymd_and_hms(2026, 8, 20, 10, 0, 0).unwrap();
        let past_end = Local.with_ymd_and_hms(2026, 8, 20, 11, 0, 0).unwrap();
        let past_event =
            events::create_event(conn, "Old session", "", past_start, past_end, None, None).expect("create past event");

        let today_start = Local.with_ymd_and_hms(2026, 8, 25, 10, 0, 0).unwrap();
        let today_end = Local.with_ymd_and_hms(2026, 8, 25, 11, 0, 0).unwrap();
        let todays_event =
            events::create_event(conn, "Today's session", "", today_start, today_end, None, None).expect("today event");

        let future_start = Local.with_ymd_and_hms(2026, 9, 1, 10, 0, 0).unwrap();
        let future_end = Local.with_ymd_and_hms(2026, 9, 1, 11, 0, 0).unwrap();
        let future_event =
            events::create_event(conn, "Future session", "", future_start, future_end, None, None).expect("future event");

        // A recurring event anchored in the past must survive — its
        // row is the whole series, not just one past day.
        let rule = RecurrenceRule { id: None, frequency: Frequency::Weekly, interval: 1, weekdays: Vec::new(), end_date: None, occurrence_limit: None };
        let rule_id = recurrence::create_rule(conn, &rule).expect("create rule");
        let recurring_event = events::create_event(conn, "Weekly session", "", past_start, past_end, None, Some(rule_id))
            .expect("create recurring event");

        let deleted = events::delete_past_events(conn, today).expect("cleanup");
        assert_eq!(deleted, 1, "only the one past non-recurring event should be deleted");

        let remaining = events::list_all_events(conn).expect("list remaining");
        let remaining_ids: Vec<i64> = remaining.iter().map(|e| e.id).collect();
        assert!(!remaining_ids.contains(&past_event.id), "past non-recurring event should be gone");
        assert!(remaining_ids.contains(&todays_event.id), "today's event must survive");
        assert!(remaining_ids.contains(&future_event.id), "future event must survive");
        assert!(remaining_ids.contains(&recurring_event.id), "a recurring series anchored in the past must survive");
    }

    #[test]
    fn delete_past_completed_tasks_carries_forward_incomplete_ones() {
        use crate::database::queries::tasks;

        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;
        let today = chrono::NaiveDate::from_ymd_opt(2026, 8, 25).unwrap();

        // Completed yesterday — should be cleaned up.
        let completed_past = tasks::create_task(conn, "Old completed", None, None, None).expect("create");
        tasks::set_completed(conn, completed_past.id, true).expect("complete");
        conn.execute(
            "UPDATE tasks SET completed_at = '2026-08-24T12:00:00+00:00' WHERE id = ?1",
            rusqlite::params![completed_past.id],
        )
        .expect("backdate completion");

        // Completed today — should survive (not yet past its day).
        let completed_today = tasks::create_task(conn, "Completed today", None, None, None).expect("create");
        tasks::set_completed(conn, completed_today.id, true).expect("complete");

        // Old but incomplete — must be carried forward regardless of age.
        let old_incomplete = tasks::create_task(conn, "Old incomplete", None, None, None).expect("create");
        conn.execute(
            "UPDATE tasks SET created_at = '2020-01-01T00:00:00+00:00' WHERE id = ?1",
            rusqlite::params![old_incomplete.id],
        )
        .expect("backdate creation");

        let deleted = tasks::delete_past_completed_tasks(conn, today).expect("cleanup");
        assert_eq!(deleted, 1);

        let remaining = tasks::list_tasks(conn).expect("list remaining");
        let remaining_ids: Vec<i64> = remaining.iter().map(|t| t.id).collect();
        assert!(!remaining_ids.contains(&completed_past.id));
        assert!(remaining_ids.contains(&completed_today.id));
        assert!(remaining_ids.contains(&old_incomplete.id), "incomplete tasks must be carried forward regardless of age");
    }

    #[test]
    fn delete_past_completed_tasks_leaves_repeating_tasks_alone() {
        // Bug fix: "repeating events reoccur after marked complete" —
        // a repeating task is a perpetual, reused row now
        // (`reset_due_repeating_tasks` resets it in place), so this
        // cleanup pass must never delete one, no matter how old its
        // completion is.
        use crate::database::queries::tasks;

        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;
        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 22).unwrap();

        let repeating = tasks::create_task(conn, "Water the plants", Some("2026-09-01"), None, None).expect("create");
        tasks::update_task(conn, repeating.id, "Water the plants", "", Some("2026-09-01"), None, None, true, None, None)
            .expect("mark repeating");
        tasks::set_completed(conn, repeating.id, true).expect("complete");
        conn.execute(
            "UPDATE tasks SET completed_at = '2026-09-01T12:00:00+00:00' WHERE id = ?1",
            rusqlite::params![repeating.id],
        )
        .expect("backdate completion");

        let deleted = tasks::delete_past_completed_tasks(conn, today).expect("cleanup");

        assert_eq!(deleted, 0, "a repeating task's completed row must never be deleted");
        let remaining_ids: Vec<i64> = tasks::list_tasks(conn).expect("list remaining").iter().map(|t| t.id).collect();
        assert!(remaining_ids.contains(&repeating.id));
    }

    #[test]
    fn cleaned_up_tasks_still_count_toward_the_streak() {
        // The whole reason for the durable completion log: a
        // completed task's contribution to the streak must survive
        // its own deletion.
        use crate::database::queries::tasks;

        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;
        let today = chrono::NaiveDate::from_ymd_opt(2026, 8, 25).unwrap();
        let yesterday = chrono::NaiveDate::from_ymd_opt(2026, 8, 24).unwrap();

        let task = tasks::create_task(conn, "Yesterday's task", None, None, None).expect("create");
        tasks::set_completed(conn, task.id, true).expect("complete");
        conn.execute(
            "UPDATE tasks SET completed_at = '2026-08-24T12:00:00+00:00' WHERE id = ?1",
            rusqlite::params![task.id],
        )
        .expect("backdate completion");

        assert!(tasks::list_completed_dates(conn).expect("before cleanup").contains(&yesterday));

        tasks::delete_past_completed_tasks(conn, today).expect("cleanup");

        let dates_after = tasks::list_completed_dates(conn).expect("after cleanup");
        assert!(
            dates_after.contains(&yesterday),
            "the completion date must survive in the log even after the task row is deleted"
        );
    }

    #[test]
    fn migration_v8_adds_completion_log_to_a_pre_existing_v7_database() {
        let dir = std::env::temp_dir().join(format!("revise-migration-v8-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("revise.db");

        {
            let conn = Connection::open(&db_path).expect("open v7 db");
            // Bug fix (real `cargo build`/`cargo test` on the user's
            // machine, seventh `issues.md` pass): this fixture claims
            // to already be "at v7" — the version whose own migration
            // (see `migrations::MIGRATIONS[6]`) creates `subjects` —
            // and already had `subject_id` FK columns on
            // tasks/events/notes to match, but never actually created
            // the `subjects` table those columns point at. SQLite
            // doesn't enforce a `REFERENCES` target to exist at
            // `CREATE TABLE` time, so this went unnoticed for a long
            // time; it only broke once a later migration (`ALTER TABLE
            // subjects ...` — first "Subject Sub Topics", now
            // "Colour") needed the table to actually be there. The
            // other two fixtures below (v8, v9) had the exact same
            // gap.
            conn.execute_batch(
                r#"
                CREATE TABLE tasks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    completed INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    due_date TEXT,
                    position INTEGER NOT NULL DEFAULT 0,
                    event_id INTEGER,
                    completed_at TEXT,
                    subject_id INTEGER
                );
                CREATE TABLE events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    start_time TEXT NOT NULL,
                    end_time TEXT NOT NULL,
                    subject_id INTEGER,
                    recurrence_id INTEGER
                );
                CREATE TABLE subjects (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL UNIQUE,
                    archived INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL
                );
                                CREATE TABLE notes (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL DEFAULT '',
                    subject TEXT,
                    event_id INTEGER REFERENCES events(id),
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    subject_id INTEGER REFERENCES subjects(id)
                );
"#,
            )
            .expect("seed v7 schema");
            conn.pragma_update(None, "user_version", 7).expect("pin at v7");
        }

        let db = Database::open_at(db_path.clone()).expect("open and migrate to v8");
        use crate::database::queries::tasks;
        tasks::log_completion(&db.conn, chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap())
            .expect("log_completion works post-migration");
        assert!(!tasks::list_completed_dates(&db.conn).expect("list").is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_default_to_sensible_values_when_unset() {
        use crate::database::queries::settings;

        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;

        assert!(settings::notifications_enabled(conn).expect("default"));
        assert!(settings::task_reminders_enabled(conn).expect("default"));
        assert_eq!(settings::reminder_lead_minutes(conn).expect("default"), 10);
    }

    #[test]
    fn settings_roundtrip_and_survive_reopening() {
        use crate::database::queries::settings;

        let dir = std::env::temp_dir().join(format!("revise-settings-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("revise.db");

        {
            let db = Database::open_at(db_path.clone()).expect("open db (first run)");
            settings::set_notifications_enabled(&db.conn, false).expect("set");
            settings::set_task_reminders_enabled(&db.conn, false).expect("set");
            settings::set_reminder_lead_minutes(&db.conn, 15).expect("set");
        }

        {
            let db = Database::open_at(db_path.clone()).expect("open db (second run)");
            assert!(!settings::notifications_enabled(&db.conn).expect("get"));
            assert!(!settings::task_reminders_enabled(&db.conn).expect("get"));
            assert_eq!(settings::reminder_lead_minutes(&db.conn).expect("get"), 15);

            // Setting again overwrites rather than erroring or duplicating.
            settings::set_reminder_lead_minutes(&db.conn, 5).expect("overwrite");
            assert_eq!(settings::reminder_lead_minutes(&db.conn).expect("get after overwrite"), 5);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migration_v9_adds_settings_table_to_a_pre_existing_v8_database() {
        let dir = std::env::temp_dir().join(format!("revise-migration-v9-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("revise.db");

        {
            let conn = Connection::open(&db_path).expect("open v8 db");
            conn.execute_batch(
                r#"
                CREATE TABLE tasks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    completed INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    due_date TEXT,
                    position INTEGER NOT NULL DEFAULT 0,
                    event_id INTEGER,
                    completed_at TEXT,
                    subject_id INTEGER
                );
                CREATE TABLE events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    start_time TEXT NOT NULL,
                    end_time TEXT NOT NULL,
                    subject_id INTEGER,
                    recurrence_id INTEGER
                );
                CREATE TABLE subjects (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL UNIQUE,
                    archived INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL
                );
                CREATE TABLE completion_log (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    date TEXT NOT NULL
                );
                                CREATE TABLE notes (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL DEFAULT '',
                    subject TEXT,
                    event_id INTEGER REFERENCES events(id),
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    subject_id INTEGER REFERENCES subjects(id)
                );
"#,
            )
            .expect("seed v8 schema");
            conn.pragma_update(None, "user_version", 8).expect("pin at v8");
        }

        let db = Database::open_at(db_path.clone()).expect("open and migrate to v9");
        use crate::database::queries::settings;
        settings::set_notifications_enabled(&db.conn, false).expect("settings table usable post-migration");
        assert!(!settings::notifications_enabled(&db.conn).expect("get"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_reminders_respects_the_notifications_disabled_setting() {
        // Not a full end-to-end test (that needs a real D-Bus session,
        // which this sandbox doesn't have) — but confirms the master
        // switch is actually read and honored: disabling notifications
        // must not panic or error, and must leave `reminded_events` /
        // `reminded_tasks` empty since nothing should be considered
        // "sent".
        use crate::database::queries::{events, settings};
        use crate::notifications::{check_reminders, NotificationScanState};
        use chrono::{Duration, Local};

        let db = Database::open_in_memory().expect("open in-memory db");
        settings::set_notifications_enabled(&db.conn, false).expect("disable notifications");

        let now = Local::now();
        let start = now + Duration::minutes(5);
        let end = start + Duration::hours(1);
        events::create_event(&db.conn, "Imminent session", "", start, end, None, None).expect("create event");

        let mut scan_state = NotificationScanState::default();
        check_reminders(&db, &mut scan_state);

        assert!(scan_state.reminded_events.is_empty(), "disabled notifications must not mark anything as reminded");
    }

    #[test]
    fn session_log_roundtrip_and_clear() {
        use crate::database::queries::{events, session_log};
        use crate::events::SessionStatus;
        use chrono::{Local, TimeZone};

        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;
        let start = Local.with_ymd_and_hms(2026, 8, 24, 10, 0, 0).unwrap();
        let end = Local.with_ymd_and_hms(2026, 8, 24, 11, 0, 0).unwrap();
        let event = events::create_event(conn, "Revision", "", start, end, None, None).expect("event");
        let date = event.day();

        assert!(session_log::get_log(conn, event.id, date).expect("get before log").is_none());

        session_log::log_session(conn, event.id, date, SessionStatus::Completed, None).expect("log completed");
        let entry = session_log::get_log(conn, event.id, date).expect("get").expect("entry exists");
        assert_eq!(entry.status, SessionStatus::Completed);

        // Logging again for the same occurrence overwrites, not duplicates.
        session_log::log_session(conn, event.id, date, SessionStatus::Adjusted, Some(90)).expect("log adjusted");
        let entry = session_log::get_log(conn, event.id, date).expect("get").expect("entry exists");
        assert_eq!(entry.status, SessionStatus::Adjusted);
        assert_eq!(entry.actual_minutes, Some(90));
        assert_eq!(session_log::list_all_logs(conn).expect("list all").len(), 1);

        session_log::clear_log(conn, event.id, date).expect("clear");
        assert!(session_log::get_log(conn, event.id, date).expect("get after clear").is_none());
    }

    #[test]
    fn pomodoro_sessions_accumulate_totals() {
        use crate::database::queries::pomodoro;

        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;
        let today = chrono::NaiveDate::from_ymd_opt(2026, 8, 25).unwrap();

        assert_eq!(pomodoro::count_sessions(conn).expect("count"), 0);
        assert_eq!(pomodoro::total_minutes(conn).expect("total"), 0);

        pomodoro::log_session(conn, today, 25).expect("log 1");
        pomodoro::log_session(conn, today, 25).expect("log 2");
        pomodoro::log_session(conn, today - chrono::Duration::days(1), 25).expect("log 3");

        assert_eq!(pomodoro::count_sessions(conn).expect("count"), 3);
        assert_eq!(pomodoro::total_minutes(conn).expect("total"), 75);
    }

    #[test]
    fn migration_v10_adds_session_log_and_pomodoro_tables_to_a_pre_existing_v9_database() {
        let dir = std::env::temp_dir().join(format!("revise-migration-v10-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("revise.db");

        {
            let conn = Connection::open(&db_path).expect("open v9 db");
            conn.execute_batch(
                r#"
                CREATE TABLE tasks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    completed INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    due_date TEXT,
                    position INTEGER NOT NULL DEFAULT 0,
                    event_id INTEGER,
                    completed_at TEXT,
                    subject_id INTEGER
                );
                CREATE TABLE events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    description TEXT NOT NULL DEFAULT '',
                    start_time TEXT NOT NULL,
                    end_time TEXT NOT NULL,
                    subject_id INTEGER,
                    recurrence_id INTEGER
                );
                CREATE TABLE subjects (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL UNIQUE,
                    archived INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL
                );
                CREATE TABLE settings (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                                CREATE TABLE notes (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL DEFAULT '',
                    subject TEXT,
                    event_id INTEGER REFERENCES events(id),
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    subject_id INTEGER REFERENCES subjects(id)
                );
"#,
            )
            .expect("seed v9 schema");
            conn.pragma_update(None, "user_version", 9).expect("pin at v9");
        }

        let db = Database::open_at(db_path.clone()).expect("open and migrate to v10");
        use crate::database::queries::{events, pomodoro, session_log};
        use crate::events::SessionStatus;
        use chrono::{Local, TimeZone};

        let start = Local.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap();
        let end = Local.with_ymd_and_hms(2026, 1, 1, 10, 0, 0).unwrap();
        let event = events::create_event(&db.conn, "Test", "", start, end, None, None).expect("create event");
        session_log::log_session(&db.conn, event.id, event.day(), SessionStatus::Completed, None)
            .expect("session_log usable post-migration");
        pomodoro::log_session(&db.conn, event.day(), 25).expect("pomodoro_sessions usable post-migration");
        assert_eq!(pomodoro::count_sessions(&db.conn).expect("count"), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn swap_positions_reorders() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let conn = &db.conn;

        let a = queries::tasks::create_task(conn, "A", None, None, None).expect("create a");
        let b = queries::tasks::create_task(conn, "B", None, None, None).expect("create b");

        queries::tasks::swap_positions(conn, a.id, a.position, b.id, b.position).expect("swap");
        let tasks = queries::tasks::list_tasks(conn).expect("list");
        assert_eq!(tasks[0].id, b.id);
        assert_eq!(tasks[1].id, a.id);
    }
}
