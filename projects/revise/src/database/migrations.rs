use rusqlite::{Connection, Result};

/// Ordered list of forward-only migrations. Each entry's index + 1 is
/// its schema version; `run` applies whichever the connection hasn't
/// seen yet, tracked via SQLite's built-in `user_version` pragma.
/// Sprint 2 acceptance criterion: "Database migrations are established."
const MIGRATIONS: &[&str] = &[
    // v1: tasks table (Section 9 data model).
    r#"
    CREATE TABLE IF NOT EXISTS tasks (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        title       TEXT NOT NULL,
        description TEXT NOT NULL DEFAULT '',
        completed   INTEGER NOT NULL DEFAULT 0,
        created_at  TEXT NOT NULL,
        due_date    TEXT,
        position    INTEGER NOT NULL DEFAULT 0
    );
    "#,
// v2: events table (Sprint 3 — Revision Sessions & Calendar Events).
    // `subject` is a free-text label for now; Sprint 9 introduces a
    // proper Subject entity and this becomes a foreign key.
    // `recurrence_id` is reserved for Sprint 6 and unused until then.
    r#"
    CREATE TABLE IF NOT EXISTS events (
        id             INTEGER PRIMARY KEY AUTOINCREMENT,
        title          TEXT NOT NULL,
        description    TEXT NOT NULL DEFAULT '',
        start_time     TEXT NOT NULL,
        end_time       TEXT NOT NULL,
        subject        TEXT,
        recurrence_id  INTEGER
    );
    CREATE INDEX IF NOT EXISTS idx_events_start_time ON events(start_time);
    "#,
    // v3: link tasks to events (Sprint 4). `event_id` is nullable so
    // tasks can keep existing independently — the acceptance
    // criterion is "attached", not "owned".
    r#"
    ALTER TABLE tasks ADD COLUMN event_id INTEGER REFERENCES events(id);
    CREATE INDEX IF NOT EXISTS idx_tasks_event_id ON tasks(event_id);
    "#,
    // v4: track when a task was completed (Sprint 5 — Dashboard basic
    // streak). Cleared back to NULL when a task is un-completed, so
    // the streak only reflects tasks currently marked done.
    r#"
    ALTER TABLE tasks ADD COLUMN completed_at TEXT;
    "#,
    // v5: recurring events (Sprint 6). `recurrence_rules` is
    // deliberately separate from `events` — an event's own row still
    // represents just its first occurrence (anchor date + time-of-day
    // + duration); the rule projects the rest forward without
    // duplicating the event definition, per the plan's Section 13
    // guidance. `event_exceptions` lets a single occurrence be
    // skipped (e.g. "not this Tuesday") without touching the rule or
    // any other occurrence.
    r#"
    CREATE TABLE IF NOT EXISTS recurrence_rules (
        id                INTEGER PRIMARY KEY AUTOINCREMENT,
        frequency         TEXT NOT NULL,
        interval          INTEGER NOT NULL DEFAULT 1,
        weekdays          TEXT,
        end_date          TEXT,
        occurrence_limit  INTEGER
    );
    CREATE TABLE IF NOT EXISTS event_exceptions (
        id               INTEGER PRIMARY KEY AUTOINCREMENT,
        event_id         INTEGER NOT NULL REFERENCES events(id),
        occurrence_date  TEXT NOT NULL,
        UNIQUE(event_id, occurrence_date)
    );
    CREATE INDEX IF NOT EXISTS idx_event_exceptions_event_id ON event_exceptions(event_id);
    "#,
    // v6: notes (Sprint 8). `subject` mirrors `events.subject` — free
    // text for now, becomes a Subject foreign key in Sprint 9.
    // `event_id` is nullable: a note can stand alone or attach to the
    // revision session it was taken during.
    r#"
    CREATE TABLE IF NOT EXISTS notes (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        title       TEXT NOT NULL,
        content     TEXT NOT NULL DEFAULT '',
        subject     TEXT,
        event_id    INTEGER REFERENCES events(id),
        created_at  TEXT NOT NULL,
        updated_at  TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_notes_event_id ON notes(event_id);
    "#,
    // v7: subjects (Sprint 9). A real, shared entity — events, tasks,
    // and notes get a `subject_id` foreign key each, replacing the
    // free-text `subject` columns on events/notes (which are left in
    // place, unused, rather than dropped — SQLite's column-drop
    // support varies by version, and there's no benefit worth the
    // risk here). Existing free-text values are migrated forward
    // automatically: every distinct non-empty `subject` string in
    // `events`/`notes` becomes a real `Subject` row, and the affected
    // rows are relinked to it — so upgrading doesn't silently lose
    // anyone's existing subject labels.
    r#"
    CREATE TABLE IF NOT EXISTS subjects (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        name        TEXT NOT NULL UNIQUE,
        archived    INTEGER NOT NULL DEFAULT 0,
        created_at  TEXT NOT NULL
    );

    ALTER TABLE events ADD COLUMN subject_id INTEGER REFERENCES subjects(id);
    ALTER TABLE tasks  ADD COLUMN subject_id INTEGER REFERENCES subjects(id);
    ALTER TABLE notes  ADD COLUMN subject_id INTEGER REFERENCES subjects(id);

    INSERT OR IGNORE INTO subjects (name, archived, created_at)
    SELECT DISTINCT subject, 0, datetime('now') FROM events
    WHERE subject IS NOT NULL AND trim(subject) <> '';

    INSERT OR IGNORE INTO subjects (name, archived, created_at)
    SELECT DISTINCT subject, 0, datetime('now') FROM notes
    WHERE subject IS NOT NULL AND trim(subject) <> '';

    UPDATE events SET subject_id = (SELECT id FROM subjects WHERE subjects.name = events.subject)
    WHERE subject IS NOT NULL AND trim(subject) <> '';

    UPDATE notes SET subject_id = (SELECT id FROM subjects WHERE subjects.name = notes.subject)
    WHERE subject IS NOT NULL AND trim(subject) <> '';

    CREATE INDEX IF NOT EXISTS idx_events_subject_id ON events(subject_id);
    CREATE INDEX IF NOT EXISTS idx_tasks_subject_id ON tasks(subject_id);
    CREATE INDEX IF NOT EXISTS idx_notes_subject_id ON notes(subject_id);
    "#,
    // v8: a durable completion log (feedback pass). Past events and
    // fulfilled to-do items now get cleaned up automatically (see
    // `queries::events::delete_past_events` and
    // `queries::tasks::delete_past_completed_tasks`) — but the
    // Dashboard's streak (Sprint 5) depends on knowing which past
    // days had a completion, and that can't keep living solely on
    // `tasks.completed_at` once the task itself might be deleted.
    // This table is append-only and outlives the tasks that fed it.
    r#"
    CREATE TABLE IF NOT EXISTS completion_log (
        id    INTEGER PRIMARY KEY AUTOINCREMENT,
        date  TEXT NOT NULL
    );
    "#,
    // v9: settings (Sprint 10 — Notifications & Linux Integration). A
    // plain key-value table rather than fixed columns, since app-wide
    // settings tend to accrete over time and a key-value shape avoids
    // a migration for every new one.
    r#"
    CREATE TABLE IF NOT EXISTS settings (
        key    TEXT PRIMARY KEY,
        value  TEXT NOT NULL
    );
    "#,
    // v10: session logging + Pomodoro tracking (feature request pass).
    // `session_log` lets a specific occurrence of an event be marked
    // completed-as-scheduled, denied (didn't happen — excluded from
    // "hours per subject"), or adjusted (a different actual duration)
    // — without that touching the event/recurrence rule itself, the
    // same "don't corrupt the parent" principle Sprint 6's exceptions
    // already follow. `pomodoro_sessions` is a durable, append-only
    // log of completed focus sessions, in the same spirit as
    // `completion_log` (Sprint 10 feedback pass): it's the source of
    // truth for "focus time" and "Pomodoros completed" stats,
    // independent of anything else's lifecycle.
    r#"
    CREATE TABLE IF NOT EXISTS session_log (
        id               INTEGER PRIMARY KEY AUTOINCREMENT,
        event_id         INTEGER NOT NULL REFERENCES events(id),
        occurrence_date  TEXT NOT NULL,
        status           TEXT NOT NULL,
        actual_minutes   INTEGER,
        UNIQUE(event_id, occurrence_date)
    );
    CREATE INDEX IF NOT EXISTS idx_session_log_event_id ON session_log(event_id);

    CREATE TABLE IF NOT EXISTS pomodoro_sessions (
        id       INTEGER PRIMARY KEY AUTOINCREMENT,
        date     TEXT NOT NULL,
        minutes  INTEGER NOT NULL
    );
    "#,
    // v11: Notes Folders (feature request). A folder is a name + a
    // colour (stored as a hex string, same representation
    // `theme::Palette` already uses) that notes can optionally belong
    // to. `folder_id` is nullable on `notes` for the same reason
    // `event_id`/`subject_id` already are there — "a note can stand
    // alone" was already true and stays true; folders are additive.
    // Deleting a folder doesn't delete its notes — `queries::folders::
    // delete_folder` clears `folder_id` back to NULL on them first, in
    // the same spirit as how subjects handle removal (there, by never
    // hard-deleting at all — `set_archived` instead — since a Subject
    // is referenced from three tables; a Folder only from one, so an
    // explicit "detach then delete" is simple enough here).
    r#"
    CREATE TABLE IF NOT EXISTS folders (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        name        TEXT NOT NULL UNIQUE,
        color       TEXT NOT NULL,
        created_at  TEXT NOT NULL
    );

    ALTER TABLE notes ADD COLUMN folder_id INTEGER REFERENCES folders(id);
    CREATE INDEX IF NOT EXISTS idx_notes_folder_id ON notes(folder_id);
    "#,
    // v12: Startup Scripts/Commands (feature request). An optional
    // shell command an `Event` runs at its own start time — a
    // nullable column, following the same "additive, opt-in" shape as
    // every other feature-request column above. Deliberately on
    // `events`, not `tasks`: the request's own examples (opening a
    // browser, starting a terminal session) are both tied to a
    // specific moment in time, which only events (with a
    // `start_time`) genuinely have — see `docs/architecture.md` for
    // the full reasoning.
    r#"
    ALTER TABLE events ADD COLUMN exec_command TEXT;
    "#,
    // v13: Task Assignment (feature request). `assigned_date` is
    // deliberately a separate column from `due_date`, not a
    // repurposing of it — the whole point of the request is that
    // "which day I plan to work on this" and "when it's actually
    // due" are two different questions a task can have two different
    // answers to. `NULL` (the default) means "not assigned to a
    // specific day", same opt-in shape as every other feature-request
    // column above.
    r#"
    ALTER TABLE tasks ADD COLUMN assigned_date TEXT;
    "#,
    // v14: Event Logging (feature request). Whether an event's
    // Done/Not-Done/Start-Session controls apply to it at all —
    // declared at creation (defaulting to on, so every pre-existing
    // event keeps behaving exactly as before) so a meeting-style
    // block that was never going to be "completed" or "started as a
    // session" can opt out and skip rendering that whole row, per the
    // request's own stated goal: "stop calendar blocks such as
    // meetings from taking up excess space."
    r#"
    ALTER TABLE events ADD COLUMN track_session INTEGER NOT NULL DEFAULT 1;
    "#,
    // v15: Repeating Tasks (feature request). Weekly-only, matching
    // the request's own scope ("some tasks are repetitive by week") —
    // not the full daily/weekly/monthly/interval machinery
    // `recurrence_rules` gives events, since tasks have a completely
    // different lifecycle (a single checkbox, not occurrence-based
    // logging) that doesn't map onto that system. See
    // `queries::tasks::set_completed` for how this is actually
    // applied: completing a repeating task spawns its next week's
    // instance as a new row, rather than "un-completing" and reusing
    // the same one — so completion history (streaks, stats) stays
    // intact per instance.
    r#"
    ALTER TABLE tasks ADD COLUMN repeat_weekly INTEGER NOT NULL DEFAULT 0;
    "#,
    // v16: Sub Tasks (feature request). Self-referential, nullable —
    // `NULL` means "top-level task" (every existing task, and the
    // overwhelming majority of new ones). A parent can only be
    // completed once every child is — enforced in
    // `queries::tasks::set_completed`, not just the UI, so it holds
    // regardless of which of the several completion-toggle call sites
    // (Dashboard, To Do, Calendar, Revision Session overlay) is used.
    r#"
    ALTER TABLE tasks ADD COLUMN parent_task_id INTEGER REFERENCES tasks(id);
    CREATE INDEX IF NOT EXISTS idx_tasks_parent_task_id ON tasks(parent_task_id);
    "#,
    // v17: Task Timings (feature request). An optional estimate,
    // independent of any linked event's own duration — see
    // `stats::minutes_per_subject_in_range` for exactly when this
    // counts toward subject-time totals (only for standalone,
    // completed tasks with no linked event, to avoid double-counting
    // time a session/block already accounts for).
    r#"
    ALTER TABLE tasks ADD COLUMN estimated_minutes INTEGER;
    "#,
    // v18: Flashcards (feature request). A deck is a title plus an
    // ordered set of question/answer pairs, imported wholesale from a
    // `.txt` file in the format the user's own vocab-quiz tool used
    // (see `flashcards::import::parse_deck_text`) — the app has no
    // flashcard editor of its own, only import, per the request's own
    // scope. `source_name` is purely informational; nothing re-reads
    // the file after import. `flashcards_seen_log` is append-only,
    // same shape and purpose as `pomodoro_sessions` above: the
    // durable source of truth for the Dashboard's "Flashcards seen"
    // stat, independent of a deck (or its cards) ever being deleted
    // later.
    r#"
    CREATE TABLE IF NOT EXISTS flashcard_decks (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        title        TEXT NOT NULL,
        source_name  TEXT,
        created_at   TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS flashcards (
        id       INTEGER PRIMARY KEY AUTOINCREMENT,
        deck_id  INTEGER NOT NULL REFERENCES flashcard_decks(id),
        question TEXT NOT NULL,
        answer   TEXT NOT NULL,
        position INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX IF NOT EXISTS idx_flashcards_deck_id ON flashcards(deck_id);
    CREATE TABLE IF NOT EXISTS flashcards_seen_log (
        id    INTEGER PRIMARY KEY AUTOINCREMENT,
        date  TEXT NOT NULL
    );
    "#,
    // v19: Built-In Flashcard Editor (feature request). A note can
    // now be flagged as a flashcard set (`is_flashcard_set`, default
    // 0 — every existing note keeps behaving exactly as before) and
    // uses the plain question/answer-per-line format
    // `flashcards::import::parse_pairs` reads, minus the title line
    // `parse_deck_text` expects for imported files: a Note already
    // has its own `title` column, so repeating it inside `content`
    // would be redundant.
    //
    // `folders.parent_id` is a nullable, self-referential column —
    // same one-level-only nesting precedent as `tasks.parent_task_id`
    // (Sub Tasks) — so a flashcard-set note's folder can be a subject
    // subfolder underneath a single top-level "Flashcards" folder,
    // per the request's own wording ("stored in a new folder called
    // flashcards, with subfolders for each subject/sub-topic"). Every
    // pre-existing folder keeps `parent_id = NULL`, i.e. exactly the
    // flat, top-level folder it already was.
    r#"
    ALTER TABLE notes ADD COLUMN is_flashcard_set INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE folders ADD COLUMN parent_id INTEGER REFERENCES folders(id);
    CREATE INDEX IF NOT EXISTS idx_folders_parent_id ON folders(parent_id);
    "#,
    // v20: Tasks Repeating Through A List (feature request). A
    // sub-option of `repeat_weekly` (default 0 — no existing
    // repeating task is affected): instead of the same title every
    // cycle, the task's displayed title becomes
    // "{repeat_list_base_title}: {topic}", advancing to the next
    // topic in `repeat_list_topics` (newline-delimited) each time
    // `queries::tasks::reset_due_repeating_tasks` resets it.
    // `repeat_list_mode` picks how "next" is chosen (sequential,
    // shuffle, or spaced repetition — see
    // `tasks::repeat_list::next_topic`); `repeat_list_state` is that
    // mode's own small encoding of "where things are right now" (an
    // index, a shuffled queue, or per-topic show-counts — never
    // meant to be read except by that module's own functions).
    r#"
    ALTER TABLE tasks ADD COLUMN repeat_through_list INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE tasks ADD COLUMN repeat_list_base_title TEXT;
    ALTER TABLE tasks ADD COLUMN repeat_list_topics TEXT;
    ALTER TABLE tasks ADD COLUMN repeat_list_mode TEXT;
    ALTER TABLE tasks ADD COLUMN repeat_list_state TEXT;
    "#,
    // v21: Subject Sub Topics (feature request). One level of
    // nesting only — same precedent as `tasks.parent_task_id` and
    // `folders.parent_id` — so a subtopic can't itself have
    // subtopics. Every pre-existing subject keeps
    // `parent_subject_id = NULL`, i.e. exactly the top-level subject
    // it already was. A subtopic is a real row in the same `subjects`
    // table, not a separate concept, which is what lets it be
    // "referenced as its own subject" everywhere a subject can be
    // (tasks, events, notes, Pomodoro) with no further changes to any
    // of those — they already just list every row in this table.
    r#"
    ALTER TABLE subjects ADD COLUMN parent_subject_id INTEGER REFERENCES subjects(id);
    "#,
    // v22: Colour (feature request). Every pre-existing subject keeps
    // `color = NULL`, meaning "no explicit choice yet" — read via
    // `theme::subject_color`, which falls back to this id's entry in
    // `theme::DEFAULT_SUBJECT_COLORS` (the same fixed palette the
    // Improved Line Graph already used, generalized from that one
    // chart to every chart plus Calendar events, now that a subject's
    // color is a real stored choice and not just something derived
    // from its id). Stored as `"#rrggbb"` text, the simplest
    // representation that round-trips through `egui::Color32` losslessly
    // and matches how every other small per-row setting in this app
    // (e.g. `repeat_list_mode`) is already stored — no separate r/g/b
    // columns needed for a value that's never queried or filtered on,
    // only ever read back whole.
    r#"
    ALTER TABLE subjects ADD COLUMN color TEXT;
    "#,
    // v23: Tasks Repeating By Day (feature request) — "like repeating
    // events, tasks should have the option to repeat by day." Every
    // pre-existing repeating task gets `repeat_interval_days = 7` via
    // the column's own `DEFAULT`, exactly the cadence `repeat_weekly`
    // alone already meant for it — nothing about how an existing
    // weekly task behaves changes. See `Task::repeat_interval_days`
    // for why this is a plain day-count rather than reusing
    // `events::Frequency`.
    r#"
    ALTER TABLE tasks ADD COLUMN repeat_interval_days INTEGER NOT NULL DEFAULT 7;
    "#,
    // v24: External Sources / Contributions (direct request — "an API
    // ... other revision applications ... registered and accounted
    // for"). Two tables, introduced together since neither means
    // anything without the other, same precedent as v5's
    // `recurrence_rules`/`event_exceptions` and v10's `session_log`/
    // pomodoro tables. `sources` is deliberately minimal — just a name
    // someone picks at `revise-cli register`, no token/secret; this is
    // a single-user local app, and a name is enough to tell one
    // registered tool's contributed time apart from another's in the
    // UI, which is all "accounted for" actually requires here. `name`
    // is `UNIQUE` so the CLI can resolve `--source <name>` without
    // ambiguity. `external_contributions` mirrors `session_log`'s own
    // shape (a subject, a date, a minute count) but *without*
    // `session_log`'s `event_id` — a contribution from an outside tool
    // was never scheduled as a Calendar event in the first place, so
    // requiring one would force every external tool to also model
    // events it has no reason to know about. `subject_id` is `NOT
    // NULL`: unlike a session_log entry (which always has an event,
    // and therefore an implied subject already), a contribution has no
    // other way to reach a subject, so there's nothing to roll up if
    // this is missing — the CLI requires an existing subject up front
    // rather than silently dropping ones that would end up
    // unattributed. `note` is nullable free text (e.g. "Chapter 4
    // quiz") for context beyond the bare minute count.
    r#"
    CREATE TABLE sources (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL UNIQUE,
        created_at TEXT NOT NULL
    );
    CREATE TABLE external_contributions (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        source_id INTEGER NOT NULL REFERENCES sources(id),
        subject_id INTEGER NOT NULL REFERENCES subjects(id),
        occurred_on TEXT NOT NULL,
        minutes INTEGER NOT NULL,
        note TEXT,
        created_at TEXT NOT NULL
    );
    "#,
    // v25: Auto Assigning (direct request — "a task should be
    // automatically assigned ... based on proximity to deadline, task
    // time for a day, and if a task is set for that day"). Four
    // columns on `tasks`, all additive with defaults that leave every
    // existing task behaving exactly as before:
    // - `plan_topics_ahead`: on a repeat-through-a-list task, makes it a
    //   *template* — Auto-assign creates one real task per scheduled
    //   day for its topics instead of the one row cycling.
    // - `generated_from`: on those generated per-day tasks, the id of
    //   the template they came from (`NULL` for every ordinary task).
    // - `auto_assigned`: the planner (not a person) chose this task's
    //   `assigned_date`, so a later run may move it. A manual
    //   assignment clears it and is never moved.
    // - `plan_signature`: a fingerprint of a template's topic
    //   configuration when its per-day tasks were generated, so
    //   editing the topics/order/cadence/start date regenerates them
    //   (see `queries::tasks::set_plan_topics_ahead`). Deliberately
    //   not on the `Task` struct — nothing outside that one function
    //   reads it.
    r#"
    ALTER TABLE tasks ADD COLUMN plan_topics_ahead INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE tasks ADD COLUMN generated_from INTEGER;
    ALTER TABLE tasks ADD COLUMN auto_assigned INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE tasks ADD COLUMN plan_signature TEXT;
    "#,
    // v26: Focus time from more than Pomodoros (feature request:
    // "Focus time should accept data from the stopwatch not just
    // pomodoro. There should also be a timer."). `pomodoro_sessions`
    // is already the append-only log behind the Dashboard's "Focus
    // time", so rather than a second table, each row now records where
    // it came from: 'pomodoro' (every existing row, via the default),
    // 'stopwatch' or 'timer'. "Focus time" sums every source;
    // "Pomodoros completed" still counts only 'pomodoro' rows.
    r#"
    ALTER TABLE pomodoro_sessions ADD COLUMN source TEXT NOT NULL DEFAULT 'pomodoro';
    "#,
    // v27: Time-blocking (docs/planning/time-blocking.md, §5.1) — a
    // task's day (`assigned_date`) stays the source of truth; a row
    // here refines it to a time of day, placed in the free gap between
    // that day's calendar events. Many rows per task so a task can be
    // split across gaps. The app never issues `PRAGMA foreign_keys`,
    // but the bundled SQLite is built with foreign keys ON by default,
    // so `task_id` IS enforced: a block can't point at a missing task,
    // and a task can't be deleted while it still has blocks. Every
    // delete path therefore removes a task's blocks FIRST, by hand —
    // see `queries::task_blocks` and the cascades in `queries::tasks`.
    // Times are RFC3339 local, like `events`.
    // `locked = 1` means the person placed or moved the block, so
    // re-planning leaves it alone. An existing database gets an empty
    // table, so behaviour is unchanged until blocks are planned.
    r#"
    CREATE TABLE task_blocks (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        task_id     INTEGER NOT NULL REFERENCES tasks(id),
        start_time  TEXT NOT NULL,
        end_time    TEXT NOT NULL,
        locked      INTEGER NOT NULL DEFAULT 0,
        chunk_index INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX idx_task_blocks_task  ON task_blocks(task_id);
    CREATE INDEX idx_task_blocks_start ON task_blocks(start_time);
    "#,
    // v28: A block's life (feature request: a block is started by a
    // stopwatch or timer begun near it, adjusts to the time really spent,
    // and carries unfinished work forward). 0 = planned (the planner's to
    // move), 1 = in progress (a timer/stopwatch is running against it),
    // 2 = done (its times are what actually happened). Blocks in progress
    // or done are history: re-planning keeps them and plans around them.
    // Existing rows are all planned, so nothing changes until a session
    // is linked to a block.
    r#"
    ALTER TABLE task_blocks ADD COLUMN status INTEGER NOT NULL DEFAULT 0;
    "#,
    // v29: Hidden events. The one-off event a finished timer/stopwatch
    // creates so its time reaches the per-subject stats is, when the run was
    // started against a planned time block, redundant on the calendar — the
    // block itself already records that time. Such an event is stored with
    // `hidden = 1`: every calendar view and picker leaves it out, while the
    // stats (and the planner's busy time) still count it. Existing events
    // are all visible.
    r#"
    ALTER TABLE events ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;
    "#,
    // v30: Time spent per task (feature request: task "bars"). A stopwatch,
    // timer or Pomodoro started with a `task` picked logs its minutes here,
    // one row per run, and the bar next to the task's name is the sum of
    // them against the task's *budget*. `tasks.estimated_minutes` can't be
    // that budget: it means "what is still to do" and the planner shrinks it
    // as work is done. `time_budget_minutes` is the total the bar measures
    // against, taken the first time time is logged (NULL = follow the
    // estimate, as for a task nobody has worked on). `task_time_log` has no
    // foreign key, like the other append-only logs; the task's delete paths
    // remove its rows by hand. Existing tasks have no time logged, so every
    // bar starts empty.
    r#"
    ALTER TABLE tasks ADD COLUMN time_budget_minutes INTEGER;
    CREATE TABLE task_time_log (
        id       INTEGER PRIMARY KEY AUTOINCREMENT,
        task_id  INTEGER NOT NULL,
        date     TEXT NOT NULL,
        minutes  INTEGER NOT NULL,
        source   TEXT NOT NULL
    );
    CREATE INDEX idx_task_time_log_task ON task_time_log(task_id);
    "#,
    // v31: Movable events. A one-off event marked movable can be dragged and
    // resized in the Day view like a task's time block. Off for every
    // existing event, so nothing about them changes.
    r#"
    ALTER TABLE events ADD COLUMN movable INTEGER NOT NULL DEFAULT 0;
    "#,
];

pub fn run(conn: &Connection) -> Result<()> {
    let current_version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    for (index, migration_sql) in MIGRATIONS.iter().enumerate() {
        let version = (index + 1) as i32;
        if version > current_version {
            conn.execute_batch(migration_sql)?;
            conn.pragma_update(None, "user_version", version)?;
        }
    }

    Ok(())
}
