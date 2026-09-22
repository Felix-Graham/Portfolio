# Architecture

## Layers

- `theme` — the design system (palette, spacing, borders, typography).
  Every visual decision flows from here; no widget hardcodes a color.
- `ui::components` — reusable, theme-aware building blocks: `Panel`,
  `RButton`, `Input`, `checkbox`, `TaskItem`, `EventCard`, `Modal`,
  `NavigationBar`.
- `ui::{dashboard, calendar, tasks, notes, flashcards}` — the five
  view shells. Each exposes a `show(ui, theme, state)` function and a
  `*State` struct that owns its own data (and, from Sprint 2 onward,
  its own `Database` connection).
- `tasks` / `events` / `flashcards` — plain domain structs (`Task`,
  `Event`, `Deck`, `Flashcard`) with no UI or SQL awareness, shared
  across whichever views need them.
- `database` — `Database` (the SQLite connection handle),
  `database::migrations` (schema versioning), and
  `database::queries::{tasks, events, flashcards}` (the only code that
  writes SQL).
- `app` — `ReviseApp` (the `eframe::App` impl / permanent shell),
  `AppState` (aggregates per-view state), and `View` (the five-way
  navigation enum with keyboard shortcut digits).

## Navigation & keyboard

Keyboard handling is vim-esque, gated by whether a text field currently
has focus (`ReviseApp::handle_keyboard_navigation` in `src/app/mod.rs`):

- **Normal mode** (nothing focused): digits `1`-`5` jump directly to a
  section, `Tab` / `Shift+Tab` cycle, and `i` enters **insert mode** by
  focusing the active view's primary input (the new-task field on To
  Do, the search field on Notes — see `View::primary_input_id`).
- **Insert mode** (a text field is focused): every keypress, including
  digits and `i` itself, goes to the field — nothing is read as a
  shortcut. `Escape` returns to normal mode by surrendering focus.

This is why the `1`-`5` shortcuts don't fight with typing a due date
like `2026-09-01`, or the letter `i` in a task title: as soon as a
field has focus, the global shortcuts stop firing entirely. Opening
the "Edit Task" or "New/Edit Event" modal also auto-focuses its title
field, so editing works immediately without an extra `i` press.

The active section is also rendered with a strong focus stroke in
`NavigationBar`, satisfying the "keyboard focus is visible" acceptance
criterion.

## Persistence (Sprint 2+)

- `database::Database` owns a `rusqlite::Connection`. As of Sprint 4
  there is exactly one, held in `AppState` and passed to each view's
  `show` function as `Option<&Database>` (`None` means the DB failed
  to open — views degrade to showing nothing rather than crashing,
  and `ReviseApp` shows one error banner rather than each view
  duplicating it).
- Views hold **no cached query results**. `ToDoState`, `CalendarState`
  are UI-only (which subview is active, in-progress modal edits, text
  inputs) — every `show` call queries SQLite fresh. This is Sprint 4's
  key design decision: it's what makes "completing a task from any
  view updates all views" true by construction rather than by
  remembering to invalidate the right caches in the right places.
  At this app's data volume (tens to low hundreds of rows), querying
  every frame is well within a 16ms frame budget.
- `database::migrations::run` applies numbered SQL migrations, tracked
  via SQLite's `PRAGMA user_version`. Adding a table or column means
  appending to the `MIGRATIONS` array — existing installs pick up only
  the new step, in order, without touching what's already there. This
  is exercised directly in `database::tests::migration_v3_adds_event_id_to_a_pre_existing_v2_database`,
  which hand-seeds a v2 schema and confirms the upgrade is non-destructive.
- `database::queries` is split one submodule per entity
  (`queries::tasks`, `queries::events`, ...). It's the only code that
  writes SQL.

## Sixth `issues.md` pass — in progress

- **Improved Line Graph.** `stats::total_minutes_by_week_per_subject`
  sits right next to `total_minutes_by_week`, reusing the exact same
  `minutes_per_subject_in_range` call each already used —
  `total_minutes_by_week` just additionally collapses it with
  `.values().sum()`. Kept as two functions rather than having the
  chart call the per-subject one and sum it itself, so
  `week_over_week_change`'s existing `&[(NaiveDate, i64)]` signature
  doesn't need touching. A subject's line color is
  `SUBJECT_LINE_COLORS[subject.id % SUBJECT_LINE_COLORS.len()]` —
  keyed by id rather than by position in whatever subject list happens
  to be showing, so a given subject's color can't shift just because
  another subject gained or lost time logged against it. This is a
  deliberate, narrow exception to `theme::Palette`'s own "no color
  accents" framing — distinguishing several overlapping lines by
  grayscale alone isn't really achievable, and the request asked for
  color specifically; every other chart in the app stays monochrome.

- **Tasks Repeating Through A List.** `tasks::repeat_list` is a pure
  module (no `egui`, no SQL) — `RepeatListMode` (`Sequential` /
  `Shuffle` / `SpacedRepetition`; `Smart`, the request's own "Future
  Extension," isn't implemented, since it depends on Subject Sub
  Topics' weakness/strength flags existing first) plus `next_topic`,
  which advances a topic list according to whichever mode, given
  nothing but the topic list and an opaque `state: &str`. Each mode
  encodes `state` differently, and deliberately as plain, simple text
  rather than anything more structured, matching how every other
  small piece of per-row state in this app's `tasks`/`events` tables
  already gets stored:
  - `Sequential`: `state` is just the next index, as decimal digits.
  - `Shuffle`: `state` is a comma-separated queue of remaining
    indices for the current cycle; once empty, a fresh shuffle is
    generated for next time. Reuses `flashcards::shuffled_order`
    rather than a second shuffle implementation — the exact same
    "don't repeat until everyone's had a turn" property Flashcards
    already wanted from its own "Shuffle" button.
  - `SpacedRepetition`: `state` is a comma-separated "times shown"
    count per topic; the least-shown topic always goes next (ties
    toward the earliest). A deliberately simple reading of "spaced
    repetition" that needs no explicit difficulty rating from the
    person — topics spread out evenly over time on their own, rather
    than an SM-2-style scheduler that would need feedback this
    request never described collecting.

  On the `tasks` table side: `repeat_through_list` is a sub-flag of
  `repeat_weekly` (meaningless without it, and `queries::tasks::
  set_repeat_list`/the UI both guard against the two disagreeing).
  `title` becomes a *derived* value once it's on —
  `"{repeat_list_base_title}: {topic}"` — regenerated by
  `reset_due_repeating_tasks` in the same pass that already resets a
  repeating task's due date, so a fresh topic appears at exactly the
  moment the task itself resets, never a stale one sitting alongside a
  fresh due date for a moment. `EditState::from_task` reads
  `repeat_list_base_title` back into the Title field instead of the
  combined `title` for exactly this reason — showing the combined
  string back would let a later save mistake it for a fresh base
  title, silently nesting `": Topic 2: Topic 3"` deeper each edit.
  `set_repeat_list` only picks a fresh first topic when the config is
  a genuine change (wasn't enabled before, or the topics/base title
  changed) — resaving a task for an unrelated edit leaves an
  in-progress cycle exactly where it was.

- **Subject Sub Topics.** `subjects.parent_subject_id` — nullable,
  self-referential, one level of nesting only, same precedent as
  `tasks.parent_task_id` and `folders.parent_id`. Deliberately just
  another row in the same table, not a separate concept: every
  existing subject-picker across the app (`ComboBox`es in
  `ui::{tasks, calendar, notes, dashboard}`) already lists every row
  in `subjects`, so a subtopic shows up wherever a subject can with
  zero changes to any of them — this is what makes it genuinely
  "referenced as its own subject," not just a label nested for
  display purposes only. `queries::subjects::create_subtopic` is
  separate from `create_subject` (mirroring `folders::create_subfolder`
  next to `create_folder`) rather than a new parameter, since every
  other caller only ever creates top-level subjects.

  The roll-up ("time spent on them added to the time spent for the
  parent subject") is `stats::minutes_per_subject_rolled_up`, layered
  on top of the existing `minutes_per_subject_in_range` rather than
  changing it in place — every caller that wants *raw*, ungrouped
  per-subject minutes (there are none outside this module currently,
  but the distinction matters) still can, while both charts that
  aggregate "hours by subject" (the bar chart and the new per-subject
  line graph above) switched to the rolled-up version. Rolling up
  folds a subtopic's minutes into its parent's entry and *removes* the
  subtopic's own key from the result entirely, rather than keeping
  both — showing a subtopic as its own separate slice/bar/line right
  next to its already-inclusive parent would double-count the same
  logged time visually, overstating the total. A subject-level
  "management" view (add/rename/archive a subtopic, inside "Manage
  Subjects") is the one place a subtopic's *own*, non-rolled-up
  contribution might eventually be worth surfacing — not done here,
  since the request only asked to "add and manage" subtopics, not to
  report on them individually.

  Not done, and flagged rather than silently skipped: every existing
  subject-picker `ComboBox` still shows a subtopic by its bare name
  (e.g. "Mechanics"), not qualified by its parent ("Physics:
  Mechanics") — fine as long as names don't collide across different
  parents, but worth a follow-up if that turns out to matter in
  practice. Doing it now would have meant touching all ~8 picker call
  sites the "Optimisation" item below identifies as already
  near-duplicated — better done once, deliberately, alongside that
  consolidation than twice.

- **Optimisation.** Read literally ("perform a code clean, removing
  unnecessary code and simplifying the codebase... one-liners should
  be used when possible") this could mean almost anything on an
  ~11,800-line codebase; the request's own closing line ("do not
  sacrifice the app's functionality without asking") was taken as
  the operating constraint on how far to go unprompted, not just a
  footnote. What was actually checked:
  - No dead code, no accidental double-clones, no leftover debug
    scaffolding turned up on a direct search — six rounds of prior
    feature work already left this in reasonably good shape. The two
    `#[allow(dead_code)]` items that do exist (`ui::components::panel
    ::Panel::tight()`, one `Spacing` constant) are deliberately-kept,
    documented API surface, not oversights — left alone rather than
    deleted on the assumption that "currently unused" and
    "unnecessary" aren't the same thing.
  - The one genuine, sizeable duplication found: essentially the same
    "build a subject `ComboBox`, current selection highlighted,
    `None`/`All` as the first option" shape appears independently
    across `ui::tasks`, `ui::calendar`, `ui::notes`, and
    `ui::dashboard` — roughly 8 call sites, each with small but real
    differences (active-only vs. every subject; "All" vs. "None" vs.
    no blank option at all; filter vs. assignment semantics).
    Consolidating this into one shared helper is a legitimate
    "unnecessary duplication" fix and fits "one-liners should be used
    when possible" well — but this sandbox cannot compile-check any of
    it (same `edition2024` toolchain gap as the `eframe` migration),
    and getting a subtle behavioral difference wrong across 8 call
    sites — showing archived subjects somewhere they shouldn't, say —
    is exactly the kind of thing that wouldn't show up as a compiler
    error at all, only as a quiet behavior regression. Per the
    request's own instruction, this was surfaced rather than done
    outright; picking it up is a matter of the person saying to go
    ahead, accepting that (like the `eframe` migration) it'll likely
    need at least one round of real feedback to land clean.

## Fifth `issues.md` pass — 2 issues fixed, 1 feature request shipped

- **Bug fix: repeating tasks reoccurred immediately after completion.**
  The previous implementation, `spawn_next_repeat_if_needed` (removed
  entirely), ran inside `queries::tasks::set_completed` and eagerly
  inserted a whole new task row — due next week, not yet completed —
  the instant the current one was marked done. That's the literal bug:
  the *next* occurrence was visible in the To Do list immediately,
  rather than only once it was actually due.

  The fix moves the rollover out of `set_completed` entirely and into
  a new `reset_due_repeating_tasks(conn, today)`, run once a day
  alongside the two existing cleanup passes in `AppState::run_cleanup`
  (same "cheap, run unconditionally, gated by `run_daily_cleanup_if_needed`"
  shape as `delete_past_events`/`delete_past_completed_tasks`
  already used). For each completed, `repeat_weekly` task, it checks
  whether `due_date + 7 days` has arrived; if not, nothing happens —
  the row stays completed and untouched. If it has, the *same* row is
  reset in place (`completed = 0`, `completed_at = NULL`, `due_date`
  advanced) rather than a new one being created, so a weekly task is
  always exactly one row for its entire lifetime. If several cycles
  have elapsed since the last check-in (the app wasn't opened for a
  week or more), `due_date` is advanced in 7-day steps until it lands
  on the most recent occurrence that isn't in the future, rather than
  surfacing something weeks stale.

  One knock-on change: `delete_past_completed_tasks` (the existing
  daily pass that deletes old completed tasks, logging their
  completion first) now skips `repeat_weekly` tasks unconditionally.
  Before this fix that didn't matter — a repeating task's completed
  row was always freshly created and would only be a day old by the
  time this ran. After this fix, a repeating task is a genuinely
  long-lived, reused row, so this cleanup pass would otherwise delete
  it out from under `reset_due_repeating_tasks` the very next day it's
  left completed.

- **Toggle Switch In App Settings from Hyprctl Notify to Caelestia
  Notify.** `notifications::NotificationBackend` (`Hyprctl` /
  `Caelestia`, `Default` is `Hyprctl` — no setting change means no
  behavior change) is stored through the existing generic
  `settings`-table key/value store (`queries::settings`), the same
  pattern every other toggle there already uses — no new migration
  needed. `notifications::send` gained a `conn: Option<&Connection>`
  parameter so it can look the setting up itself; every call site
  (the two inside `notifications::check_reminders`, which already had
  `conn`, and the two Pomodoro-completion notifications in
  `AppState::tick_pomodoro`, which didn't) now passes it through,
  falling back to `Hyprctl` if `conn` is `None` (e.g. a database that
  failed to open).

  "Caelestia Notify" was researched rather than guessed at: caelestia
  is a Quickshell-based Hyprland shell whose notification manager is a
  standard freedesktop.org D-Bus notification listener, not something
  with its own bespoke "send" command — so selecting it just means
  sending a standard desktop notification (`notify-send`, part of
  `libnotify`) instead of Hyprland's own `hyprctl notify` overlay,
  and caelestia-shell (if it's running) picks that up and renders it
  in its own style. Implemented as a second subprocess spawn
  (`std::process::Command::new("notify-send")`), exactly like
  `hyprctl` already was — not a linked D-Bus library — preserving the
  portability reasoning `send`'s own doc comment already gave for
  avoiding `notify-rust`/D-Bus in the first place, regardless of which
  backend ends up selected.

- **Issue: Packaging Error, resolved by diagnosis — no code change.**
  The reported terminal transcript shows `cargo run`'s linker failure
  printed *before* the line confirming direnv finished exporting its
  shell environment (`CC`, `LD`, `PATH`, etc.). Confirmed against both
  of this project's `flake.nix` files — this repo's own, plus the
  user's separate outer environment-level one, supplied once
  requested — that neither is missing anything: both use
  `pkgs.mkShell` on the default `stdenv`, which always bundles a C
  compiler (`stdenv.cc`) unless a flake deliberately opts out with
  `stdenvNoCC`; neither does. The actual cause is a well-known direnv
  characteristic: its hook only reloads the environment when the shell
  displays a *new prompt*, not partway through a command line. Piping
  `cd project && cargo run` as one line lets `cd` happen, but `cargo`
  still runs under whatever environment was active *before* entering
  the directory — direnv doesn't reload until that whole line finishes
  and the next prompt draws, which is exactly what the log showed (the
  linker failure, then the environment export lines, in that order).
  Fixed by changing the command invocation — `cd` and `cargo run` as
  two separate commands, or `direnv exec project cargo run ...` for
  scripts/one-liners, both of which force the reload to happen (or
  happen synchronously) before `cargo` starts — not by touching
  `flake.nix` or `packaging/`, since there was nothing wrong in
  either. Per the issue's own explicit instruction to pause and ask
  for the second `flake.nix` before guessing further, that file was
  requested and reviewed before this diagnosis was finalized, rather
  than speculatively editing packaging code first.

## Fourth `issues.md` pass — 1 issue fixed, 3 feature requests shipped

- **Issue: Button Position Fix.** Root cause, confirmed against
  `ui::components::input::Input::show`: it defaults `desired_width` to
  `ui.available_width()`, i.e. "fill the row," so a field followed by
  a button in the same `ui.horizontal` leaves the button nothing to
  lay out into — it renders, just off the visible area. The
  Flashcards import panel was the only form in the app combining a
  text field and its action button on one row; every other
  field-plus-button form already put them on separate lines (see e.g.
  the note edit modal's Title field, or the folder modal's Name
  field, each followed by their own action row below rather than
  beside them). Restructuring the import panel to match fixed it
  without needing `Input::width(...)` to hand-compute a narrower
  size — matching an existing pattern beats inventing a new one.
- **Built-In Flashcard Editor.** `notes::Note` gets one new column,
  `is_flashcard_set` (default `false` — every existing note is
  unaffected), set via a fast-path
  `queries::notes::set_is_flashcard_set` rather than a new parameter
  on `create_note`/`update_note` — per this project's usual rule for a
  field most rows never use, avoiding a ripple through those
  functions' other call sites (in particular the Calendar's
  auto-note-from-event, which has no reason to know about it). A
  flashcard-set note's `content` is parsed with
  `flashcards::import::parse_pairs` — deliberately not
  `parse_deck_text`, which expects a leading title line: a Note
  already has its own `title` column, so requiring one inside
  `content` too would be redundant.

  `folders::Folder` gets `parent_id: Option<i64>`, nullable and
  self-referential — one level of nesting only, the same precedent
  `tasks::Task::parent_task_id` (Sub Tasks) already set. Every
  pre-existing folder keeps `parent_id = NULL`; the Notes tab row
  filters to `parent_id.is_none()` in memory so subject subfolders
  don't clutter it, the same "filter what's already loaded, don't add
  a query" approach the subject filter beside it already used.
  `queries::folders::find_or_create_flashcards_root` lazily creates
  the one reserved top-level "Flashcards" folder the request asked
  for ("stored in a new folder called flashcards, with subfolders for
  each subject/sub-topic") the first time a subject subfolder is
  needed, rather than unconditionally at startup — plenty of installs
  will never use flashcard-set notes at all. It's matched by name
  among top-level folders, so renaming that folder does mean a fresh
  one gets created next time; documented as a caveat rather than
  solved with a dedicated "is this the root" flag, since the request
  didn't call for the root folder to be otherwise protected.

  The Flashcards view treats an imported deck and a flashcard-set Note
  as the same kind of thing to study: `ui::flashcards::SetRef` is
  either `Deck(id)` or `Note(id)`, and `start_study_session` resolves
  either into an identical `PlayerState`. A Note's pairs are parsed
  fresh from its `content` every time it's studied rather than copied
  into the `flashcards` table at edit time — the Note stays the single
  source of truth, so editing it in Notes is immediately reflected
  next time it's studied, with nothing to keep in sync. Deleting a
  flashcard-set Note is a click in Notes, same as any other note;
  there's no separate delete path for it on the Flashcards page (which
  only reads it), unlike an imported deck's own Delete button there.

- **Flashcard Keyboard Shortcuts.** Implemented entirely inside
  `ui::flashcards::show_player`, guarded by
  `ctx.memory(|m| m.focus().is_none())` like `ui::notes`'s own
  arrow-key list navigation. `Tab` and `Esc` are also global shortcuts
  (section-cycle, and — while a modal's open — closing it)
  respectively, so `ReviseApp::handle_keyboard_navigation` now returns
  immediately whenever `state.flashcards.player.is_some()`, the same
  early-return shape it already used for an open command palette —
  otherwise one keypress would fire both the global handler and the
  player's own. `reveal_answer` and `advance` are pulled out as their
  own functions specifically so the "seen" log call happens in exactly
  one place regardless of whether a reveal was triggered by the
  on-screen button, `Enter`/`Space`, or `→`.

- **Navigation Bar Update.** The command palette's `PaletteState` and
  `show` function are unchanged in shape; what changed is that
  `show` now takes `db: Option<&Database>` (same signature style as
  `subjects_modal::show`) and, only once the query's first word is
  exactly "flashcards" (`is_flashcards_set_query`), swaps its list
  from `filter_commands` over `ALL_COMMANDS` to
  `filter_set_options` over `ui::flashcards::available_study_sets(db)`
  — computed only in that branch, not on every keystroke of an
  unrelated command search. Picking a result now returns a
  `PaletteResult` (`Command` or `StudySet`) instead of a bare
  `CommandId`, so `ReviseApp::update` can either dispatch to
  `execute_command` as before or resolve the picked `SetRef` straight
  into a `PlayerState` and switch to the Flashcards view — reusing
  `start_study_session`, the same pipeline the deck list's own
  "Study" button calls, rather than building a second way to start a
  session.

## Flashcards — feature request (outside the `issues.md` cycle)

A fifth view, alongside the original four. `flashcards::{Deck,
Flashcard}` are plain domain structs, same shape as `tasks`/`events`;
`flashcards::import::parse_deck_text` is the pure, unit-tested parser
for the source `.txt` format (title line, then alternating
question/answer lines — matching the uploaded Python vocab tool's own
format), kept entirely separate from `ui::flashcards`'s `egui` code
per this project's usual "pull logic into a tested function" rule.

- **No editor, only import.** Unlike Notes, there's no create/edit
  form for individual cards — a deck's only origin is
  `import_from_path` reading a whole file and handing it to
  `parse_deck_text`, then `queries::flashcards::import_deck` writing
  the deck row and every card row in one transaction. This matches
  the request's own stated scope ("the app need not have its own
  flashcard editor").
- **Import path: drag-and-drop, or a pasted path — no new
  dependency.** `egui`/`eframe` already collects OS drag-and-drop
  events (`ctx.input(|i| i.raw.dropped_files)`); reading a manual path
  from a text field costs nothing extra either. A file-dialog crate
  (`rfd`) was deliberately not added: on Linux it pulls in either a
  GTK or an XDG-portal system dependency, and the one existing open
  issue in this project (`NoGlutinConfigs`, see the HANDOFF/packaging
  notes) is already a graphics-stack compatibility problem on the
  target machine — adding another native system dependency for a
  feature that doesn't need one seemed like an avoidable risk.
- **Cascading delete, not "unlink."** `queries::flashcards::delete_deck`
  deletes a deck's cards before the deck row itself — a flashcard has
  no meaning detached from its deck, unlike a Note losing its Folder
  (`folders::delete_folder` just nulls `folder_id`, since a Note is a
  first-class thing on its own).
- **"Flashcards seen" reuses the Pomodoro stats pattern exactly**:
  `flashcards_seen_log` (migration v18) is an append-only table, same
  shape as `pomodoro_sessions` — one row per answer reveal, counted
  with `COUNT(*)` for the Dashboard's all-time total
  (`queries::flashcards::total_seen`), independent of whether the deck
  or card that produced it still exists. "Seen" is defined as
  *revealing an answer* during study, not just paging past a card,
  since that's the point in the flow where studying has actually
  happened.
- **Shuffle needed a shuffle, not a dependency.** Rather than adding
  the `rand` crate for one button, `flashcards::shuffled_order(len,
  seed)` is a small seeded Fisher-Yates shuffle over a tiny built-in
  xorshift generator — deterministic for a given seed, so (per the
  same "pull logic out to test it" rule) it's directly unit-tested
  without any actual randomness in the test.

## Sprint 7 — calendar views

- `occurrences_on_day` (Sprint 6) is now implemented in terms of a
  more general `occurrences_in_window`, which expands every event —
  recurring or not — across an arbitrary date range in one pass per
  event, bucketed by day in a `HashMap<NaiveDate, Vec<DayOccurrence>>`.
  Day view asks for a single-day window; Week and Month ask for their
  whole visible range. This avoids Week/Month doing their own
  per-cell recurrence expansion (which would mean re-deriving the
  same occurrences up to 42 times for a month grid).
- `week_bounds`, `shift_month`, and `month_grid_weeks` are pure
  date-math functions with no egui dependency, unit-tested directly —
  in particular `month_grid_weeks` returns 4-6 weeks depending on the
  month (not a hardcoded 6), verified by a test that a month starting
  on a Sunday fits in fewer rows than one starting near the end of a
  week.
- `CalendarState.selected_day` stays the single piece of "where am I"
  state across all three subviews — Week and Month derive their
  visible range from it (`week_bounds`, `month_grid_weeks`) rather
  than tracking their own separate cursor, so switching between Day/
  Week/Month never loses your place, and "Today" always means the
  same thing regardless of which view it's pressed from.
- Week and Month are deliberately overview surfaces, not full event
  editors: clicking a day or an event line jumps to Day view for that
  date rather than duplicating Day's Edit/Delete/Skip controls in a
  cramped grid cell. `open_new_event` is the one piece of creation UI
  shared across all three views.

## Sprint 8 — notes

- `notes::Note` mirrors `tasks::Task`/`events::Event`'s shape: a plain
  data struct, no UI or SQL awareness. `Note::preview` is the one bit
  of note-specific logic worth pulling out and testing on its own —
  "first non-empty line, truncated" sounds trivial until a note starts
  with blank lines or the first line is 400 characters long.
- `notes::search_notes` follows the same pattern as
  `app::palette::filter_commands`: a pure, case-insensitive substring
  filter with no database or egui dependency, so the matching logic
  is tested directly rather than through the UI.
- Notes reuse the exact "attach to an event" shape Sprint 4 built for
  tasks — a nullable `event_id` on the row, `list_notes_for_event` for
  the reverse lookup — rather than inventing a different association
  mechanism for the second entity that needed one.
- The Calendar's Edit Event modal now has both a Tasks section
  (Sprint 4) and a Notes section (Sprint 8) side by side. Notes are
  deliberately display-plus-quick-add only there; full editing (and
  the Subject/session dropdown) stays in the Notes view, the same
  division Sprint 7 drew between Day view (full control) and Week/
  Month (overview, click through for detail).
- `Input::multiline` is the first departure from "every Input fills
  its row, submits on Enter" — a note's content needs Enter to mean
  "newline," not "submit." The distinguishing check is
  `response.has_focus()` (still true after pressing Enter in a
  multi-line field) versus `response.lost_focus()` (true after Enter
  in a single-line field, which is how Title/Subject still submit the
  form as expected).

## Sprint 9 — subjects

- `subjects::Subject` is deliberately the simplest possible shared
  entity: id, name, archived, created_at. No color, no icon — "the
  visual interface should remain monochromatic" per the plan.
- Migrating three free-text `subject` columns (events, notes) into a
  real foreign key, while adding one fresh to tasks (which never had
  one), was the actual work of this sprint. The migration is
  non-destructive in two ways: the old `subject` TEXT columns are left
  in place unused rather than dropped (SQLite's column-drop support
  varies by version — not worth the risk for an unused column), and
  every distinct existing free-text value is turned into a real
  `Subject` row with affected rows relinked to it, via
  `INSERT OR IGNORE ... SELECT DISTINCT` plus a correlated-subquery
  `UPDATE`, both running once as part of migration v7. Verified by a
  test that seeds two events and a note all sharing one free-text
  string and confirms they collapse to exactly one `Subject` row, not
  three.
- There is deliberately no `delete_subject` query — only
  `set_archived`. "Archived subjects remain available for historical
  data" is a hard requirement, not just a nice-to-have, so deletion
  was never on the table. `list_active_subjects` (archived excluded)
  feeds "assign this to something new" pickers; `list_all_subjects`
  (archived included, clearly labeled) feeds contexts that need to
  keep showing a subject an existing item already has, like the Edit
  Event/Task/Note modals.
- Subject management (create/rename/archive) lives at the `AppState`
  level (`app::subjects_modal`), not inside any one view — the same
  reasoning as the command palette: it's a cross-cutting concern no
  single screen owns. Reachable via a nav-bar button and a "Manage
  Subjects" palette command, consistent with how "New Task"/"New
  Event"/"New Note" are already reachable both ways.
- The subject filter (`CalendarState.subject_filter`,
  `ToDoState.subject_filter`, `NotesState.subject_filter`) is
  per-view state rather than a single global filter — Calendar's Day/
  Week/Month subviews share one filter (set once at the top of
  `calendar::show`, since switching subviews shouldn't reset it), but
  To Do and Notes each keep their own, since filtering your task list
  to Physics has no obvious bearing on what you'd want your notes
  filtered to.
- `notes::search_notes` (Sprint 8) dropped subject-substring matching
  now that subject is a structured id, not text a search could
  coincidentally match. Narrowing by subject is the filter's job now;
  free-text search stays scoped to title/content.

## Feedback pass — calendar timeline fix, packaging fix, Pomodoro Subject Links

Two reported issues and the first of five feature requests
(`issues.md`), addressed in that order — issues first, since they're
bugs in already-shipped behavior, then the feature requests roughly
in the order they were listed.

- **Calendar Day view timeline rewrite** (issue: "events on the
  calendar should show the time which they take up"). The previous
  layout had two independently-scaled pieces that only looked
  connected: an hour ruler drawn as one fixed-height (20px) row per
  hour, and a separate duration-proportional accent bar
  (`bar_height_for_duration`, `PIXELS_PER_HOUR`-scaled) drawn next to
  whichever hour row the event happened to start in, with manual
  padding afterward attempting to keep later events roughly aligned.
  Nothing tied the bar's scale to the ruler's, so a 2-hour event's bar
  was twice as tall as a 1-hour one's, but neither actually spanned
  two hour-lines on the ruler — the event visually "started and
  finished" at its own row regardless of duration.
  - The fix: a single absolute-positioned timeline. The whole day's
    hour range is reserved up front as one `Rect`
    (`ui.allocate_exact_size`), hour gridlines are painted directly at
    `y = earliest_hour_offset * PIXELS_PER_HOUR`, and each event card
    is placed via `ui.allocate_ui_at_rect` at a `y` computed from its
    *actual* start time on that same scale — `timeline_offset_minutes`,
    pulled out as a small pure function specifically so this alignment
    math is unit-tested directly rather than only visually inspected
    (`calendar_math_tests::timeline_offset_*`). A 2-hour event's card
    now genuinely spans two hour-gridlines, not just a proportionally
    tall bar next to an unrelated ruler.
  - Overlapping events aren't given side-by-side columns — occurrences
    still just stack in the order returned by `occurrences_on_day`, as
    before. That was already a non-goal for the Day view (a
    single-column agenda for one person's schedule rarely needs true
    concurrent-event layout), so the rewrite doesn't newly regress
    anything there, but it's also not attempted.
- **`.desktop` `Exec=revise` doesn't launch** (issue). Root cause: a
  bare `revise` in `Exec=` only resolves if the binary happens to
  already be on whichever `PATH` is launching it, which nothing in the
  repo actually guaranteed — `nix build` alone only produces
  `./result/bin/revise`, never installed anywhere. The fix is a
  `homeManagerModules.default` output added to `flake.nix`:
  `programs.revise.enable = true;` puts the built package on
  `home.packages` (making `revise` a genuinely real command) *and*
  generates the `.desktop` entry with `Exec` pointed directly at that
  package's own Nix store path, so menu/autostart launching doesn't
  depend on `PATH` resolution at all — `programs.revise.autostart`
  additionally drops the same entry into XDG autostart. See
  `packaging/README.md`'s new "Installing `revise` as a real command"
  section for the actual home-manager config snippet. As with the rest
  of `packaging/`, this couldn't be evaluated against a real
  `home-manager switch` in this sandbox (no `nix` here) — see that
  file's "What hasn't been verified."
- **Pomodoro Subject Links** (first feature request): a subject can
  now be picked from the Dashboard's Focus card before starting a
  Pomodoro (`PomodoroState.subject_id: Option<i64>`, deliberately
  *not* cleared by `.reset()` — picking a subject once and running
  several consecutive Pomodoros against it is the expected flow).
  When a Work phase completes with a subject attached,
  `AppState::tick_pomodoro` doesn't add a second, parallel way to
  accumulate subject-time data — it reuses the exact pipeline a
  manually-scheduled revision session already uses: create a one-off
  Calendar `Event` for the completed block (`create_event`) and
  immediately log it `SessionStatus::Completed`
  (`session_log::log_session`). This is what satisfies both halves of
  the request for free: the block "shows up on the calendar" because
  it *is* a calendar event now, and it's "logged on the dashboard's
  subject time page" because `stats::minutes_per_subject` already
  sums exactly this shape of data — no changes needed to that
  function at all. The unconditional, subject-agnostic
  `queries::pomodoro::log_session` call (feeding the separate
  "Pomodoros completed" stat) is untouched and still always runs.

### Feature requests not yet started

Three remain from `issues.md`, in the order requested, roughly
sized by what each would touch:

- **Background Notifications** — a genuine "notify after the app is
  killed" daemon is a materially different shape of feature than
  everything else in `notifications.rs`, which is deliberately
  in-app-only polling (`## Sprint 10`'s "no background process is
  required... unless explicitly needed" — this request *is* that
  explicit need). Likely means a separate small binary/systemd
  user unit reading the same SQLite database, plus a decision on
  what "not too much memory" means concretely (a timer-based poll
  interval, most likely).
- **Pomodoro Stopwatch** — a count-up sibling to the existing
  count-down `PomodoroState`. The Sprint 12 `revision_session.rs`
  (`ActiveSession`) already *is* almost exactly this shape (count up
  from an explicit start, pause/resume, no fixed end) minus the tie to
  a specific Calendar occurrence — worth checking whether this request
  is better served by a "start a Revision Session with no linked
  event" mode than by a third timer type.
- **Notes Folders** — needs a new `folders` table (name, colour),
  a nullable `folder_id` on `notes`, migration, CRUD queries, a
  folder-create/edit UI (name + colour picker), and a way to move a
  note between folders from both the notes list and its edit page, per
  the request.
- **Startup Scripts/Commands** — needs a nullable `exec_command`
  column on `tasks` and/or `events` (the request says "certain tasks,"
  but the browser example ("opening the browser") reads more like an
  event-time trigger), a field in the relevant edit modal(s), and
  actually invoking it — likely from the same reminder-scan path
  `notifications::check_reminders` already polls, given it's the only
  existing "this is due now" detector. Worth deciding whether this
  should require explicit confirmation before running an arbitrary
  shell command, given the security surface of "run whatever string
  the user typed into a text field" at a scheduled time.

## Making it a genuinely launchable desktop app: `install.sh` + `revise-launch.sh`

Follow-up to the `.desktop`-launch-issue fix above: that fix only
covered the Nix/home-manager path. `packaging/install.sh` and
`packaging/revise-launch.sh` do the same job — a working, real
`revise` command plus a correctly-pointed `.desktop` entry — without
requiring Nix at all.

- **`revise-launch.sh`** is a small, dependency-free shell script that
  searches a fixed list of plausible locations for the actual
  `revise` binary (next to itself, `target/{release,debug}/` for
  running from a checkout, `~/.cargo/bin`, `~/.local/bin`,
  `/usr/local/bin`, `/usr/bin`, finally whatever's on `PATH`) and
  `exec`s the first one found. The point of this indirection: once a
  `.desktop` file's `Exec=` points at this script's own fixed,
  absolute install location, it never needs regenerating just because
  Revise was later reinstalled a different way (a manual
  `cargo build --release` today, `cargo install --path .` tomorrow) —
  the script absorbs that variability, not the `.desktop` file.
- **`install.sh`** does the actual work: builds a release binary if
  one isn't already there, installs both it and the launcher to
  `~/.local/bin`, and writes `~/.local/share/applications/
  revise.desktop` with `Exec=` set to the launcher's absolute path —
  eliminating `PATH`-resolution ambiguity entirely, the same fix
  philosophy as the home-manager module's own store-path `Exec=`,
  just for the non-Nix case. Idempotent (safe to re-run after pulling
  changes), and two flags are opt-in, never applied silently:
  - `--autostart`: copies the same `.desktop` entry into XDG
    autostart (`~/.config/autostart`) — compositor-agnostic, works
    under Hyprland too via its XDG autostart support.
  - `--hyprland-exec-once`: idempotently *appends* an `exec-once =`
    line to `~/.config/hypr/hyprland.conf` if one pointing at the
    launcher isn't already present — never overwrites the file,
    never touches anything else in it, and does nothing at all if no
    `hyprland.conf` exists yet (printing the line to add by hand
    instead).
- **A clarification worth stating plainly, since the feature request
  that prompted this named `hyprctl` specifically**: `hyprctl` is a
  *runtime* IPC control tool — it talks to an already-running
  Hyprland, so it has no mechanism to fire "at startup" before that
  socket exists. The actual "run this when Hyprland itself starts"
  hooks are `exec-once` (classic `.conf` syntax, what
  `--hyprland-exec-once` above generates) and, on newer Hyprland
  versions, the Lua config's `hl.on("hyprland.start", ...)`. Where
  `hyprctl` *is* the right tool is launching Revise **on demand**
  afterward — e.g. `hyprctl dispatch exec revise-launch.sh` bound to
  a keybind — which `install.sh`'s closing output shows as a concrete
  example, alongside the startup mechanisms actually being used.
- Verified in this sandbox end to end: built a debug binary (a full
  release build's LTO pass exceeds this sandbox's per-command time
  limit), ran `install.sh --autostart --hyprland-exec-once` against a
  scratch `$HOME` with a fake `cargo` shim standing in for the slow
  release compile, and confirmed the generated `.desktop`'s `Exec=`
  is a correct absolute path, the autostart copy lands correctly, and
  re-running the Hyprland flag against an already-patched
  `hyprland.conf` correctly detects the existing line and leaves the
  file untouched rather than duplicating it. Not verified: any of
  this against a real Hyprland session or real desktop-environment
  application menu, since this sandbox has neither.

### Follow-up: "the app doesn't launch outside of its directory"

A different bug than the `.desktop`/`PATH` issue above, though easy to
conflate with it — reported against a real NixOS machine (its
`configuration.nix`: `programs.hyprland`, `programs.direnv.enable`,
`programs.nix-ld.enable` with a broad library list, `rustc`/`cargo`/
`rustup` all present in `environment.systemPackages`). Root cause: a
plain `cargo build`/`cargo build --release` binary has no embedded
knowledge of where its Wayland/GL/xkbcommon shared libraries live —
it depends entirely on `LD_LIBRARY_PATH` being set correctly at launch
time. This repo's own `flake.nix` `devShell` sets exactly that
(`LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildInputs;`), which is
why the binary works fine from inside a `nix develop` shell (or
wherever `direnv` auto-activates one, given `use flake` +
`programs.direnv.enable`) but fails anywhere else with an "error while
loading shared libraries" — the directory isn't actually what matters,
the *shell environment* is; it just happens that people normally only
have that environment active while sitting in the project directory.

`nix-ld` (also enabled on the reporting machine, with `wayland`,
`libGL`, `libxkbcommon`, `mesa`, and `vulkan-loader` already in its
library list) is NixOS's general answer to this class of problem, but
it only intervenes for binaries whose ELF interpreter is the generic
`/lib64/ld-linux-x86-64.so.2` path — which is what a `rustup`-installed
toolchain's output uses, but *not* a binary compiled with nixpkgs' own
`rustc`/`cargo` (both present here too), whose interpreter is already
a Nix store path that `nix-ld`'s shim never gets a chance to intercept.
Which case applies depends on which `cargo` ends up first on `PATH`
when building — genuinely ambiguous from the config alone, which is
why `packaging/README.md`'s new "Why the binary might only launch from
inside the repo" section leads with a concrete `ldd`/`patchelf
--print-interpreter` diagnostic rather than assuming one or the other.

The actual fix, and the only one that makes the binary genuinely
portable rather than working around wherever it's currently being
launched from: `packaging/install.sh` now builds via `nix build
.#default` whenever `nix` is available, falling back to `cargo build
--release` (with an explicit warning printed either way, so it's
never a silent, easy-to-forget difference) only when it isn't.
`rustPlatform.buildRustPackage`'s fixup phase computes and embeds the
correct RPATH into the binary itself from `buildInputs` — the built
binary carries its own library paths, needing no ambient
`LD_LIBRARY_PATH`, `nix develop`, `direnv`, or `nix-ld` intervention at
all. Since the Nix store path a `nix build` output points to can be
garbage-collected once nothing references it, `install.sh` uses `nix
build --out-link ~/.local/state/revise/result` — a persistent,
named out-link — specifically so the build stays referenced (a
GC root) rather than the binary silently breaking the next time
`nix.settings.auto-optimise-store`-style garbage collection runs.

### Follow-up 2: `NoGlutinConfigs` after the RPATH fix above

A distinct problem one level down from the RPATH issue, reported on
the same NVIDIA + Hyprland + Wayland machine after applying the fix
above: the binary now *launches* (the dynamic linker successfully
resolves `libGL.so`/`libEGL.so`, etc.) but immediately crashes with
`Error: NoGlutinConfigs(...)` from `glutin` (the GL-context-creation
library behind `eframe`'s `glow` backend). `libGL.so`/`libEGL.so`
found via RPATH are only `libglvnd` dispatch stubs — the actual driver
implementation (Mesa's, or NVIDIA's proprietary one here) is resolved
separately, at runtime, through vendor JSON files, which on NixOS live
under `/run/opengl-driver` — a path whose contents depend on which
driver `hardware.graphics`/`hardware.nvidia` activated on a given
machine, so it can't be baked into `buildInputs` the normal way.
Neither plain RPATH, `nix-ld`, nor a `devShell`'s `LD_LIBRARY_PATH`
covers this, since none of those are about driver resolution.

Fixed two ways, kept both since which one actually applies depends on
build method and launch method: `revise-launch.sh` now prepends
`/run/opengl-driver/lib` (and the 32-bit variant) to
`LD_LIBRARY_PATH` before exec'ing — works regardless of how the
binary itself was built, and is a no-op on non-NixOS systems where
that path doesn't exist. `flake.nix`'s package derivation also gained
a `wrapProgram` call (from nixpkgs' `makeWrapper` setup hook) doing
the same `LD_LIBRARY_PATH` prepend directly on the built binary — the
more "proper" fix for the `nix build` path, since it means the
*binary itself* is correct even launched some other way than through
`revise-launch.sh`. (First attempt used `addOpenGLRunpath` directly
instead — caught immediately on the first real build against actual
NixOS, which failed with `attribute 'addOpenGLRunpath' missing` on
that machine's pinned nixpkgs revision; apparently its exact attribute
location isn't stable across nixpkgs versions. `wrapProgram` was
chosen as the replacement specifically for being one of nixpkgs' most
stable, long-standing mechanisms, to avoid the same class of breakage
recurring.) Neither could be verified against a real GPU in this
sandbox — see `packaging/README.md`'s "What hasn't been verified" for
what to check next if this doesn't fully resolve it (driver
version/kernel compatibility and `nvidia-drm.modeset` are outside what
packaging alone can fix).

**This fix wasn't sufficient on its own.** Real diagnostic data from
the reporting machine (`ls /run/opengl-driver/lib | grep nvidia`
showing a complete, modern NVIDIA driver install with full Wayland/
GBM/EGL support already present; the `wrapProgram` wrapper script
inspected directly and confirmed correctly wired) ruled out the
library-path explanation entirely — every fix above was real and
necessary, just not the actual bottleneck for this specific crash.
Forcing X11 (`WAYLAND_DISPLAY=`) failed identically, ruling out a
Wayland/GBM-specific cause too. `RUST_LOG=debug` (previously
inert — nothing in this app installed a `log` consumer; fixed by
adding `env_logger::init()` to `main()`, a small, safe, purely
additive change) showed `glutin` failing on its *first and only*
config-matching attempt, immediately, with the exact same
`ConfigTemplate` every time.

That template looked like a promising clue: `depth_size: 0, stencil_size: 0` —
`eframe`'s own defaults (egui has no use for either), and `glutin`
filters configs by *exact* match on these fields, not "at least" — a
known category of driver-compatibility problem elsewhere in the
ecosystem. `NativeOptions.depth_buffer` was set to `24` in
`src/main.rs` on this theory (24 being the most universally-supported
depth size across GL/EGL implementations). **This did not fix the
issue.** A same-terminal, same-session, back-to-back comparison later
showed `glutin` still reports zero matching configs with `depth_size:
24` — identical failure, just a different number printed in the error.
The setting is harmless and has been left in place (it doesn't hurt
anything), but should not be described as a fix — this was a real
mistake in an earlier pass through this problem, corrected here rather
than left standing.

A further back-to-back test (`cd` into the directory that had once
"worked", run it, `cd /tmp`, run it again, same terminal, same
session) also disproved an apparent directory/`direnv`-shell
dependency — both runs failed identically. The one earlier "it works
from here" report was a one-off that didn't reproduce, not a real
finding.

What the accumulated evidence actually supports: zero EGL configs
match *unconditionally* — under Wayland and X11, regardless of the
requested depth/stencil size, with every required driver library
confirmed present and correctly resolved. That pattern doesn't look
like a filter being too strict; it looks like `glutin`'s EGL
negotiation failing outright against this specific (very current,
595.91.07) NVIDIA driver. `eframe = "0.24.1"`'s vendored `glutin` is
from December 2023.

An attempt to sidestep `glutin` entirely by switching `eframe`'s
renderer to `wgpu` (Vulkan on Linux) was tried and reverted — not
because it was a bad idea (current `eframe`, as of this writing 0.36,
defaults to `wgpu` and no longer uses `glutin`/EGL at all by default,
which is the most promising lead) but because `eframe = "0.24.1"`
specifically pins `wgpu = "^0.18.0"`, and that exact version has been
**yanked** from crates.io by its own maintainers (confirmed directly
against the sparse index, not inferred from a build error alone) —
genuinely unbuildable via a fresh `cargo` resolve at this `eframe`
version, not a bug in this project. Actually adopting `wgpu` requires
a full `eframe`/`egui` version upgrade (0.24 → 0.36 as of this
writing) — a much larger and riskier change, since this app uses the
`egui` API surface extensively throughout `ui/`, and one this sandbox
cannot compile-check at all: its available `rustc` (1.75.0, matching
`eframe 0.24`'s own era) is too old for current `eframe`'s MSRV.

**Status: parked, not resolved**, by deliberate choice — this is a
real, difficult problem, and further progress needs either a real
`eframe`/`egui` upgrade (sized and scoped properly, with a
minimal-reproduction test first — see `packaging/README.md`'s writeup)
or further driver-level debugging on a real machine, neither of which
is a good use of time relative to feature work right now. A minimal,
from-scratch test project (current `eframe`, default features, no
`glow`) was started to de-risk a future migration attempt by
confirming a fresh `eframe`/`wgpu` setup actually opens a window on
the affected machine *before* committing to migrating this app's own
~15k lines of UI code — that test was not finished or verified this
session and is not part of this repository; picking this back up
later should start there.

## Feature requests, continued: Stopwatch, Notes Folders, Startup Scripts, Background Notifications

All five feature requests from `issues.md` are now done. Roughly in
the order implemented:

- **Pomodoro Stopwatch.** Rather than a third timer type,
  `revision_session::ActiveSession` gained an event-less mode:
  `event_id`/`occurrence_date` became `Option`, and a new
  `subject_id: Option<i64>` field was added alongside the existing
  `subject_name` (needed so Finish can attribute a synthesized event
  to the right subject without a second lookup). The Dashboard's new
  "Stopwatch" panel (title + subject picker, only shown when no
  session is already active) constructs one directly with
  `event_id: None`. Because it's the same `ActiveSession` type, it
  reuses `show_revision_session_overlay` completely unchanged — no
  second overlay was needed. On Finish, a subject-linked stopwatch
  session synthesizes a one-off Calendar event exactly like Pomodoro
  Subject Links does for a completed Work phase (same reasoning: reuse
  the pipeline `stats::minutes_per_subject` already sums). An
  unsubjected stopwatch run logs nothing on Finish beyond ending the
  session — there's nothing meaningful to attribute it to.
- **Notes Folders.** New `folders` table (migration v11: `name`,
  `color` as a hex string) and a nullable `notes.folder_id`. Deleting
  a folder (`queries::folders::delete_folder`) detaches its notes
  (`folder_id` back to `NULL`) rather than deleting them — a folder is
  an organizing label, not a container. The Notes view gained a tab
  row (All / Unfiled / each folder, with a small "+ Folder" affordance)
  above the existing subject filter, plus a per-note "move to folder"
  picker and a folder field in the note edit modal. Folder colour is
  deliberately a curated set of six muted swatches
  (`ui::notes::FOLDER_SWATCHES`), not a full RGB picker — `theme.rs`'s
  own stated thesis is "no color accents" in an otherwise
  black/white/gray app, and a handful of desaturated tones lets
  folders stay visually distinguishable without breaking that
  language the way a vivid color wheel would.
- **Startup Scripts/Commands.** A nullable `events.exec_command`
  column (migration v12), kept on `events` rather than `tasks` — the
  request's own examples (opening a browser, a terminal session) are
  both tied to a specific moment in time, which only events (with a
  `start_time`) genuinely have. Set via its own
  `queries::events::set_exec_command`, deliberately *not* folded into
  `create_event`/`update_event`'s signatures, so those two functions'
  many existing call sites (including tests) didn't need to change for
  a field most events will never use. Actually running it is gated
  behind its own Settings toggle, **off by default**
  (`queries::settings::exec_commands_enabled`) — flagged explicitly in
  the feature-request writeup as a security surface worth deciding
  deliberately rather than silently: this runs an arbitrary shell
  string, unattended, at a scheduled time. When enabled, it's
  triggered from the same `notifications::check_reminders` poll
  reminders already use, via a new pure `should_run_exec_command(now,
  event_start)` — due at (or briefly after) start, with a 15-minute
  grace window rather than an unbounded one, so reopening the app
  hours later doesn't fire a now-stale command. Deduplicated
  separately from reminders (`NotificationScanState.
  executed_commands`), since the two fire at different moments (lead
  time before start, vs at start) and shouldn't share one dedup set.
  Implementing this also surfaced and fixed a latent coupling bug:
  `check_reminders` previously gated its *entire* body (including
  startup commands) behind the master "notifications enabled" toggle;
  the two are now independent, since a startup command has nothing to
  do with whether session-start reminders are turned on.
- **Background Notifications.** The crate was split into a library
  (`src/lib.rs`, everything that was previously declared directly in
  `main.rs`) plus two binaries: the existing GUI (`src/main.rs`, now a
  thin `eframe::run_native` wrapper) and a new `revise-reminderd`
  (`src/bin/revise-reminderd.rs`) — a minimal, `eframe`-free daemon
  whose entire `main()` is a sleep loop around the exact same
  `notifications::check_reminders` the GUI already polls with. No
  reminder logic is duplicated between the two. Coordination so they
  don't double-fire around the same due moment: a new
  `gui_heartbeat` module has the GUI touch a timestamped file
  (`$XDG_RUNTIME_DIR/revise-gui.heartbeat`) every time it actually
  performs a scan (`notifications::check_reminders` now returns
  whether it did, vs. short-circuiting on its own internal throttle),
  and the daemon checks that file's freshness before every scan of its
  own, skipping if the GUI looks alive. This means the daemon is safe
  to leave permanently enabled — it's effectively a no-op whenever the
  GUI is already open. Packaged as a systemd user unit: manually via
  `packaging/revise-reminderd.service`, or declaratively via a new
  `programs.revise.reminderDaemon.enable` option on the
  `homeManagerModules.default` module from the `.desktop`-launch-issue
  fix above.

## Feature requests, third round: To Do overhaul (addition, repeats, subtasks, timings) + two new charts

A third `issues.md` batch, six items across Tasks/To Do and
Statistics, in the order listed:

- **Better To Do Addition.** The old flow was a two-step "quick-add
  row (title + due date) then a 'More options' button to reach
  everything else" — the request asked for the full form to be the
  default. `ui::tasks::EditState` is now opened directly by a single
  "Add Task" button (no separate quick-add row at all), with every
  field — title, description, subject, due date, repeat, subtask
  parent, estimated time — available immediately. This is the same
  modal "Edit" already uses, in creation mode, continuing the pattern
  the "Improved Task/Event Addition" round established (unifying
  creation with editing rather than maintaining a second, thinner
  form).
- **Repeating Tasks.** Weekly-only (`tasks.repeat_weekly`, migration
  v15), deliberately *not* the full daily/weekly/monthly/interval
  machinery `recurrence_rules` gives events — a task's lifecycle (one
  checkbox, no occurrence-based logging) doesn't map onto that system,
  and the request's own scope is "repetitive by week." The mechanism
  lives entirely in `queries::tasks::set_completed`, not any one UI
  call site: completing a `repeat_weekly` task spawns its next
  instance (due 7 days after the completed one's own due date) as a
  new row, leaving the completed row alone — so completion history/
  streaks stay intact per instance instead of one row being reset and
  reused. Guarded against a redundant complete→complete call (e.g. a
  double click) spawning twice.
- **Sub Tasks.** `tasks.parent_task_id` (migration v16), self-
  referential and nullable. Nesting is capped at exactly one level
  deep *by construction* — the parent picker in `edit_modal` only
  offers top-level tasks — which rules out cycles entirely rather than
  needing to detect them at arbitrary depth. "A parent may only be
  completed once every child is" is enforced in `set_completed`
  itself (a silent no-op if any child is incomplete, since every
  completion-toggle call site across the app already just fires-and-
  forgets that call) — with an "x/y subtasks" indicator on the parent
  row so the reason a checkbox doesn't "take" is visible before
  someone even tries. "A new task can be added as a child... through
  the `edit` button" is the modal's "+ Add subtask" button, which
  saves the current edit first (so the child has a real parent id to
  point at and nothing typed is lost) before opening a fresh child
  form.
- **Task Timings.** `tasks.estimated_minutes` (migration v17),
  optional. Counted into `stats::minutes_per_subject_in_range` — and
  therefore "Hours Per Subject" and "Total time by Week" both, since
  the latter is built directly on the former — but only for completed,
  *standalone* tasks (`event_id.is_none()`): a task linked to a
  revision session is already covered by that session's own
  scheduled/logged time, and adding the estimate on top would double-
  count the same work, exactly the "if the task is not part of a
  session/block with a contradicting time" caveat in the request.
  Attributed to the task's `completed_at` date for range filtering,
  the same way a session's minutes are anchored to its occurrence
  date.
- **Time Per Subject pie chart.** Alongside the existing bar chart —
  the request said "as well as," not "instead of." Hand-drawn (no
  charting crate in this project), since egui has no built-in pie
  primitive: `ui::dashboard::pie_slice_points` fans out triangles from
  the center across an arc approximated with straight segments (more
  segments for a bigger slice — `pie_chart_tests` checks this
  directly), and `pie_slice_color` shades each slice along a gray
  gradient rather than distinct hues, consistent with `theme.rs`'s
  explicit "no color accents" thesis — the same reasoning Notes
  Folders' curated muted-swatch set already established for this
  codebase.
- **Total time by Week.** `stats::total_minutes_by_week` deliberately
  doesn't re-implement its own aggregation — it calls
  `minutes_per_subject_in_range` once per week and sums the per-
  subject breakdown, so it can never drift out of sync with "Hours Per
  Subject"'s own numbers. `week_over_week_change` produces the "up
  14%..." message the request asked for, with an explicit zero-
  previous-week branch (no division by zero, and a distinct message
  rather than a nonsensical percentage) and returns `None` entirely
  until there are at least two weeks of data to compare. The line
  chart itself (`total_time_by_week_chart`/`line_chart_points`) is
  hand-drawn the same way the pie chart is, for the same reason.

## Feature requests, second round: Task Assignment, Hours Per Subject periods, unified creation dialogs, Event Logging

A second `issues.md` batch, all four items done, worked in the order
listed:

- **Improved Hours Per Subject View.** `stats::minutes_per_subject`
  (unbounded, "all time") became `minutes_per_subject_in_range(db,
  start, end)`, alongside a new pure `period_bounds(HoursPeriod,
  today)` — Week/Month/Year, Week the default, with December's
  year-rollover as an explicit test case since it's the one date-math
  edge this actually has. The Dashboard chart gained a This Week /
  This Month / This Year selector; nothing else about the chart
  changed, since the existing per-subject bar rendering already
  worked from a `HashMap<i64, i64>` regardless of how it was computed.
- **Improved Task/Event Addition.** The bug was real: task creation
  was a two-field inline row (title, due date) while editing had
  five (plus description); event creation from the Dashboard was a
  four-field modal while the Calendar's edit modal had description,
  recurrence, and the startup-command field on top. Fixed by
  *unifying* creation with editing rather than growing a second form
  to match:
  - Tasks: `ui::tasks::EditState.id` became `Option<i64>` (`None` =
    creating). A new "More options…" button next to the quick-add row
    opens the exact same modal "Edit" uses, carrying over whatever was
    already typed rather than discarding it. `save_edit` branches into
    `create_task` + an immediate follow-up `update_task` for the
    description, the same "create, then a fast-path call for the
    rest" shape `set_exec_command` already established.
  - Events: Dashboard's separate `QuickEventState` (title, subject,
    times only) and its ~90 lines of modal/save code were deleted
    outright. "New Event" now switches to the Calendar view and opens
    its existing full `EventEditState` modal in creation mode instead
    — one event-creation form to maintain, not two that could drift
    out of sync. Required threading `&mut CalendarState` and `&mut
    View` into `dashboard::show`, the same direct-mutable-reference
    shape `revision_session` already uses there.
- **Task Assignment.** New `tasks.assigned_date` column (migration
  v13), deliberately separate from `due_date` — "which day I plan to
  work on this" and "when it's actually due" are different questions
  a task can answer differently. The Dashboard's "Today's Tasks"
  filter now treats an explicit assignment as *overriding* the
  due-date default rather than adding to it: a task due today that's
  been assigned to a later day no longer shows up today just because
  its deadline happens to be today — matching the request's own
  framing ("instead of assuming..."). A new "Assign…" button per task
  row opens a small popup reusing the existing natural-language date
  parser (`natural_date::parse_natural_date`). The Calendar half (`ui
  ::calendar::tasks_for_day_section`) adds a "Tasks for this day"
  panel at the top of the Day view, scoped strictly to
  `assigned_date` (not `due_date` — the timeline below already covers
  scheduled events), with its own assign/unassign controls.
- **Event Logging.** New `events.track_session` column (migration
  v14, `NOT NULL DEFAULT 1` so every pre-existing event keeps
  behaving exactly as before — verified with a dedicated
  reopen-a-pre-existing-database test), set via a checkbox right in
  the event edit modal and persisted through a `set_track_session`
  fast-path call, the same shape as `set_exec_command`. Two changes to
  `event_block`'s rendering: (1) the whole Done/Not-Done/Start-Session
  control area is skipped entirely when `track_session` is `false` —
  the request's own target case, a meeting-style block that was never
  a revision session to begin with; (2) for events that *do* track
  sessions, the Done/Not-Done buttons disappear once an occurrence has
  actually been logged one way or the other, replaced by "Edit hours"
  (which now doubles as the general "change what I logged" action)
  and "Clear" — and "Start Session" disappears at that same point too,
  per the request's "the same goes for the Start Session option."

## Sprint 12 — revision session / focus mode

- A **Revision Session** (`revision_session::ActiveSession`) is
  deliberately distinct from the Pomodoro timer (`pomodoro.rs`): a
  Pomodoro is a generic, fixed-length, auto-cycling productivity timer
  unrelated to any particular event, while a session is tied to one
  Calendar occurrence, shows *that* occurrence's title/subject/tasks,
  and counts *up* with no fixed end — it runs until "Finish," at which
  point the elapsed time (rounded to the nearest minute) becomes the
  recorded actual duration. Both share the same overall shape
  (`Instant`-based timing, a `focused`/full-screen overlay overriding
  the entire shell, Escape-to-minimize) because that shape already
  proved itself for Pomodoro — Revision Session mirrors it rather than
  inventing a second pattern for the same problem.
- **"Start Session" threads an `Option<(i64, NaiveDate)>` up through
  `calendar::show`.** The overlay has to live at the `AppState` level
  (same reason `pomodoro` does — it must override the *entire* shell,
  nav bar included, which a single view's `show` function has no way
  to do), so the click itself is surfaced as calendar::show's return
  value — `event_block` → `day_view` → `show` — using the same
  output-parameter pattern already established there for `delete_id`,
  `edit_target`, and `log_action`, just carried one level further out
  to `ReviseApp::update`. Starting a session is allowed from *any*
  occurrence (past, today, or future); the plan doesn't call for
  restricting it, and doing so would add complexity for no clear
  benefit.
- **Recording the result reuses `session_log` outright.** "Finish"
  calls `queries::session_log::log_session(..., SessionStatus::
  Adjusted, Some(session.elapsed_minutes()))` — the exact same
  infrastructure the feature-request pass built for manually reporting
  "this ran for a different duration than scheduled." A revision
  session's elapsed time *is* that case, just measured automatically
  instead of typed in by hand, so no new storage or status variant was
  needed. "Cancel" clears the in-memory session without touching
  `session_log` at all — "I didn't mean to start this" shouldn't leave
  a row behind.
- **The running session lives only in memory** (`AppState.
  revision_session: Option<ActiveSession>`); nothing is written to the
  database until Finish is explicitly clicked. This directly satisfies
  "closing the application does not corrupt session state" by
  construction: if the app closes mid-session, the in-progress session
  is simply lost, not partially or inconsistently written.
- **Escape minimizes, not cancels** — consistent with the Pomodoro
  overlay's existing behavior, and for the same reason: an accidental
  Escape shouldn't silently discard a running session. Since
  `ActiveSession` is a small, fully-tested domain type, the "is the
  overlay currently showing" flag lives beside it instead of inside
  it, as `AppState.revision_session_minimized: bool` — keeping that
  purely-UI concern out of the tested struct. While minimized, the nav
  bar surfaces a "⏱ MM:SS — Resume" button (only rendered when a
  session is active and minimized) as the sole way back into the
  overlay short of Finish or Cancel.
- Toggling a task from inside the overlay calls
  `queries::tasks::set_completed` immediately, exactly like every
  other checkbox in the app, then re-fetches
  `revision_session.tasks` from the database so the on-screen count
  updates without needing to leave focus mode — the session snapshots
  its task list once at start, so nothing else would pick up the
  change otherwise.

## Sprint 11 — keyboard-first interaction

- Most of this sprint's acceptance criteria were already true before
  it started, as a side effect of earlier feedback passes: the vim-
  esque normal/insert mode split, the command palette, and Calendar's
  arrow-key day/week/month navigation. Sprint 11's actual work was
  filling in the plan's specific suggested shortcuts and adding one
  genuinely new interaction pattern (list selection).
- **Single-letter shortcuts (`N`/`T`/`D`/`W`/`M`/`?`) are aliases, not
  separate implementations.** Each maps to an existing (or, for `?`,
  new) `palette::CommandId` and is executed through the same
  `execute_command` the palette itself calls. This matters for
  correctness, not just tidiness: if "New Event" ever needs to change
  (which view it focuses, what it pre-fills), there's exactly one
  place to change it, not two that could quietly drift apart.
- The same `input_mut` + deferred-execution pattern from the Sprint 6
  feedback pass (`pending_focus`, to avoid a same-frame `Key::I` press
  also typing a literal "i") extends naturally to commands:
  `pending_command` holds a `CommandId` raised while scanning input,
  applied once the `input_mut` closure ends — because `execute_command`
  itself needs `ctx.memory_mut`, which can't be called while still
  inside `input_mut` (they share a lock in egui 0.24).
- **List selection** (`ui::list_selection::move_selection`) is the one
  piece of pure, non-trivial logic worth pulling out and testing:
  clamping at the ends rather than wrapping, and — the detail easy to
  get wrong — treating "no selection" as one-before-the-first-item
  only when moving down, so that pressing Up with nothing selected
  still lands on the first item rather than jumping to the last one.
  Both To Do and Notes call the same function rather than each
  re-implementing (and potentially disagreeing on) the rule.
- To Do's `Space` toggles completion using the *current* task's
  `completed` field negated, matching exactly what the mouse-driven
  checkbox path already computes — keyboard and mouse feed the same
  `toggle_id` variable and go through the same one code path
  afterward, so there's no risk of the two diverging on what "toggle"
  means.
- The keyboard shortcuts reference (`app::shortcuts_help`) is
  deliberately just three static tables grouped by where each
  shortcut applies (anywhere / lists / Calendar & palette) — no
  attempt to introspect the app's actual keybinding state and
  generate the list dynamically. At this app's shortcut count, a
  hand-maintained reference is both simpler and less likely to drift
  from reality than a "clever" auto-generated one would be, since
  there's no single registry of bindings to introspect in the first
  place (they're scattered across each view's own `show` function by
  design, so that Calendar's arrow keys, for instance, only exist
  and matter while Calendar is the active view).

## Feature request pass — issues.md (dbus fix, Pomodoro, stats, session log)

- **The dbus/Nix build blocker.** `notify-rust`'s D-Bus path (even
  the pure-Rust `zbus` backend, not just the C `libdbus` binding) was
  removed entirely — not just worked around. `notifications::send`
  now shells out to `hyprctl notify` directly via
  `std::process::Command`, matching the exact syntax already used
  elsewhere in the user's own Hyprland config. This trades broader
  desktop-environment portability (a generic D-Bus notification works
  under GNOME, KDE, any compositor with a notification daemon) for
  zero extra dependencies and guaranteed compatibility with the
  plan's actual target platform. `hyprctl` missing from `PATH` (e.g.
  running under a different compositor) just means the call silently
  no-ops, same "swallow, don't surface" failure handling as before.
- **The Month view header bug.** Root cause: the header row divided
  `ui.available_width()` by 7 *inside* the loop, once per label —
  but each `ui.add_sized` call consumes space, so `available_width()`
  shrinks a little more each iteration and the divisor drifts further
  from "true cell width" with every label. The day grid below never
  had this problem because it uses `ui.columns(7, ...)`, which
  computes all seven column widths once, upfront, from the full row
  width. The fix was simply to make the header use the same
  mechanism instead of hand-rolling an equivalent.
- **Pomodoro (`pomodoro.rs`)** deliberately uses `std::time::Instant`/
  `Duration`, not `chrono`, for the countdown itself — it's a
  short-lived, in-session timer with no calendar semantics, so a
  monotonic clock is the right tool and sidesteps any DST/clock-change
  edge cases a wall-clock comparison could hit. `PomodoroState` lives
  on `AppState`, not inside `DashboardState`, specifically because its
  full-screen focus mode needs to override the *entire* shell — nav
  bar included — which only `ReviseApp::update`'s top-level early
  return can do; a single view's `show` function has no mechanism to
  reach that far up.
- **`stats.rs`** deliberately has no UI code — it's the aggregation
  layer (`minutes_per_subject`, `tasks_done_by_day`) that
  `ui::dashboard`'s stat panel calls into, kept separate so the
  aggregation logic (which occurrences count, what a denied session
  contributes) is unit-tested without needing to render anything.
- **Session logging (`events::session_log`)** follows the exact same
  shape Sprint 6 established for skipping a recurring occurrence
  (`event_exceptions`): keyed by `(event_id, occurrence_date)`, never
  touching the event row or its recurrence rule. `effective_minutes`
  is the one pure function worth isolating: "no log entry" defaults to
  the scheduled duration (the same "assume it happened" default
  free-text due dates have always used), `Denied` is zero, `Adjusted`
  is whatever was logged. `stats::minutes_per_subject` is the only
  caller that actually walks every occurrence of every event
  (recurring ones included, via the same `occurrence::
  occurrences_in_range` the Calendar and reminder-scanner both use)
  to add them up — everywhere else in the app only ever needs a single
  day or a single event's occurrences, not a running total across all
  of history.

## Sprint 10 — notifications & Linux integration

- `notifications::reminder` is the pure part: given "now", a session's
  start time, and a lead time, is this the moment to remind — no
  notification backend, no database, no state about what's already
  fired. That state (`NotificationScanState`: which event-occurrences
  and which tasks have already been reminded about this session) and
  the actual side-effecting call live in `notifications::
  check_reminders`, called once per frame from `AppState` but
  internally throttled to once every 20 seconds — a reminder only
  needs minute-level precision, so there's no reason to hit the
  database 60 times a second. (`notifications::send` itself shells
  out to `hyprctl notify` as of the feature-request pass below — see
  that section for why.)
- `notifications::todays_event_starts` duplicates a *small* slice of
  `ui::calendar::occurrences_in_window`'s occurrence-expansion logic
  (recurring events, exceptions) rather than sharing it directly. The
  UI version returns a `DayOccurrence` with task-checklist and
  rule-label metadata the reminder scanner has no use for; recomputing
  just "event id, title, actual start time" directly against
  `events::occurrence::occurs_on` was less code than threading a
  UI-shaped type through a non-UI consumer.
- Settings are a plain key-value table (`database::queries::settings`)
  rather than fixed columns on some `AppState`-adjacent row, since
  app-wide settings tend to accrete over releases and a key-value
  shape means adding one later doesn't need its own migration.
  Missing keys resolve to sensible defaults (notifications on, 10
  minutes lead, task reminders on) rather than requiring the table be
  pre-seeded.
- Reminders are deliberately in-memory, not persisted: restarting the
  app can reasonably re-remind about something starting imminently,
  and persisting "already notified" state across restarts would be
  meaningfully more complexity (which occurrence, which day, cleanup
  of stale entries) for a marginal benefit.
- No systemd unit, timer, or daemon was written. "No background
  process is required when the application is closed unless
  explicitly needed" is a direct acceptance criterion, and nothing in
  the plan's necessary-features list asks for reminders while the app
  *isn't* running — only in-app polling is implemented, on purpose.
- `packaging/revise.desktop` and `packaging/README.md` are the
  "startup configuration" deliverable: an XDG desktop entry usable
  either via `~/.config/autostart/` or as the target of Hyprland's
  `exec-once`. Neither the notification path nor the desktop entry
  has been exercised against a real compositor or D-Bus session — this
  sandbox has neither — and `packaging/README.md` says so plainly
  rather than implying otherwise.

## Feedback pass 3 — issues.md (auto-cleanup, arrow keys, natural dates)

Four more issues:

1. **Past events and fulfilled to-dos are deleted automatically.**
   Events: only non-recurring ones, once their day has passed — a
   recurring event's row is its *first occurrence*, not "an event
   from one particular day," so deleting it would destroy every
   future occurrence along with whatever past one prompted the
   cleanup. Tasks: only completed ones, once the day they were
   completed has passed; incomplete tasks are never touched no matter
   how old ("carried forwards" is the plan's own phrase for this).
   This surfaced a real design conflict worth documenting: the
   Dashboard's streak (Sprint 5) depends on knowing which past days
   had a completion, read from `tasks.completed_at` — but that column
   disappears the moment its task is deleted. Fixed with a small,
   append-only `completion_log` table (migration v8): right before a
   completed task is deleted, its completion date is written there,
   and `list_completed_dates` (the streak's data source) now unions
   both the log and any still-live tasks' `completed_at`. A dedicated
   test (`cleaned_up_tasks_still_count_toward_the_streak`) confirms a
   completion date survives its own task's deletion. Cleanup runs once
   at app startup and is re-checked every frame thereafter by
   comparing the calendar day against when it last ran — "at the end
   of the day" for a non-daemon desktop app reasonably means "the next
   time anything notices the day changed," whether that's the next
   launch or a session left open across midnight.
2. **Command palette arrow-key navigation.** `ArrowDown`/`ArrowUp` now
   work as well as `Tab`/`Shift+Tab` for moving through results —
   arrow keys are the more natural fit for stepping through a list,
   Tab remains for anyone already used to it.
3. **Calendar arrow-key navigation.** `ArrowLeft`/`ArrowRight` step
   through days, weeks, or months depending on which subview is
   active — the keyboard equivalent of each subview's existing ◀ ▶
   buttons. Gated on nothing being focused, same "normal mode" pattern
   as the rest of the app's keyboard shortcuts, so it doesn't fire
   while typing in a modal field.
4. **Natural-language dates.** `natural_date::parse_natural_date` is a
   new, pure, shared module recognizing `"today"`/`"now"`,
   `"tomorrow"`/`"yesterday"`, bare weekday names, and `"next
   <weekday>"`, on top of literal `YYYY-MM-DD` — used by both
   `tasks::due_date::normalize_due_date` (task due dates) and the
   Calendar's event date field (main date, and the recurrence rule's
   "ends on" date). The two callers treat a parse failure differently,
   deliberately: task due dates fall back to storing whatever was
   typed as free text, matching their loose, never-validated history;
   event dates surface the parser's error message and refuse to save,
   since an event's date is load-bearing for actual scheduling and
   datetime math, not just a label. A bare weekday name resolves to
   the closest occurrence on or after today (today itself, if today
   already is that weekday) — matching how people actually say it
   ("I'll do it Tuesday") — while `"next <weekday>"` is always
   strictly after today even if today matches, since the word "next"
   is doing real work there.

## Feedback pass 2 — issues.md (UI bounding, proportional events)

Two more issues, reported after Sprint 8:

1. **"UI Bounding" (the Dashboard's Add button sitting too far over).**
   The root cause: `Input` defaults to filling `ui.available_width()`,
   and when it's the *first* widget added to a row, "available width"
   is the entire remaining row — nothing has claimed any space yet.
   So a row like `[Input][Add][New Event]` rendered the input at
   nearly the full window width, shoving both buttons out to the far
   right edge. The fix is the standard egui idiom for this: render the
   buttons first, inside a `Layout::right_to_left` block, so they
   claim their (small, fixed) space from the right edge *before* the
   input asks how wide it can be — the input then naturally fills
   whatever's left, snug against the buttons, at any window size.
   Applied to every row with an input and trailing button(s):
   Dashboard's quick-add, To Do's due-date row, and the Calendar Edit
   Event modal's task/note quick-add rows. A fixed-width cap was
   considered and rejected — it would've left a gap between the input
   and the buttons instead of closing it, since the buttons don't
   "shrink-wrap" to however much of their reserved space the input
   actually uses.
2. **Proportional calendar events.** The Day view's event blocks
   previously had no relationship between an event's duration and how
   much vertical space it occupied — a 30-minute session and a 3-hour
   one looked the same size. Rather than literally resizing the
   (fairly dense) event card — title, subject, recurring badge, time
   range, description, task checklist — to a strict pixel height
   (risking clipped content for a short-but-detailed event), each
   event now gets an accent bar on its left edge sized via
   `bar_height_for_duration`: `PIXELS_PER_HOUR` px per hour of
   duration, floored at `MIN_BAR_HEIGHT` so very short events still
   show a visible sliver. `ui.horizontal`'s row sizes to the *taller*
   of {bar, card content} automatically, so a short event with a long
   checklist is never clipped; if the card is shorter than its
   duration calls for, the remaining space is padded in afterward so
   the *next* event still starts at a proportionally correct offset.
   Scoped to Day view — Week's compact one-line-per-event lists and
   Month's session counts aren't really "space" representations in the
   same sense, so proportional sizing wouldn't mean anything there.

## Feedback pass — issues.md

Four issues were reported after Sprint 6 and addressed directly (not
tied to a plan sprint number):

1. **Dialog sizing.** `Modal` now renders as a resizable
   `egui::Window` (draggable edges, `.max_width`/`.max_height` clamped
   to the actual viewport size) with its content in a `ScrollArea`,
   instead of a fixed-size, unscrollable overlay. A dialog can no
   longer exceed the window, and tall content (e.g. the Edit Event
   modal with its Repeat and Tasks sections) scrolls instead of
   overflowing off-screen.
2. **The `i` key and Enter-to-submit.** The `i` shortcut had a real
   bug: focusing a field via `Key::I` didn't stop the *same keystroke*
   also being delivered to that field as typed text, since egui emits
   both a `Key` event and a `Text` event for one press of a letter
   key — so pressing `i` silently left a stray "i" in whatever you
   typed next. Fixed by stripping the matching `Event::Text` from the
   frame's queue (`ctx.input_mut`) right when the shortcut fires. The
   same fix applies to `/` (see item 4). A persistent hint line
   ("1–4 sections · i insert · / command · Enter submit · Esc back")
   now sits under the nav bar so the shortcuts are discoverable
   without reading this file. Every text-entry point (To Do's add-task
   row, the Edit Task/Edit Event/Quick Event modals, Dashboard's quick
   task) now submits on Enter, not just via its button.
3. **"today"/"now" as due dates.** `tasks::due_date::normalize_due_date`
   resolves `"today"`/`"now"` (case-insensitive) to the actual current
   date at save time; every other input still passes through as free
   text, same as before. Due-date placeholders now show today's actual
   date directly (e.g. `"...— today is 2026-08-25"`) so it's visible
   without needing to type anything.
4. **Keyboard access beyond text fields.** Of the three options
   offered, a terminal-style command bar (`/` to open, type to filter,
   Enter to run, Tab/Shift+Tab to move the selection, Escape to close)
   was the most tractable to build correctly in an immediate-mode UI:
   it needed one small, well-defined command list
   (`app::palette::ALL_COMMANDS`) rather than enumerating and
   hint-tagging every widget's on-screen position each frame, or
   indexing all visible text for a Vim-style `/`-search-and-jump. It
   deliberately covers page-level actions — navigate to a section,
   start a new task, start a new event — not per-row actions like
   editing or deleting an arbitrary task or event, which stay
   mouse-driven for now.

## Sprint 6 — recurring events

- `events::recurrence::RecurrenceRule` is a plain data struct
  (frequency, interval, weekdays, end date, occurrence limit) with no
  database or UI dependency, serialized to/from SQLite as simple
  columns (`weekdays` as a comma-separated list of days-from-Monday).
- `events::occurrence::occurrences_in_range` is the whole recurrence
  engine: given an anchor date and a rule, it walks forward computing
  which dates match, bounded by the caller's query range (always just
  `today` in the current UI) rather than the rule's own end condition
  — a rule with no end date anchored a year ago still only costs a
  cheap walk to today, not an unbounded one, which matters since this
  reruns every frame in the immediate-mode UI.
- Recurring events are stored once: `events.recurrence_id` points at
  the rule, and the event row's own `start_time`/`end_time` are the
  *first* occurrence. `ui::calendar::occurrences_on_day` is what turns
  that into "does this event appear today, and if so at what shifted
  time" — nothing about a future occurrence is ever written to disk.
- `event_exceptions` (one row per skipped date) is intentionally the
  simplest possible way to satisfy "individual occurrences can be
  handled without corrupting the parent recurrence" — it doesn't
  support editing a single occurrence's title or time independently
  of the series, only skipping it. Per-occurrence content overrides
  would need a bigger table (essentially caching a modified copy of
  the event per exception date) and aren't implemented.
- A recurring event's tasks live on the parent event row, so they're
  the same tasks on every occurrence — there's no per-occurrence
  completion tracking. The Day timeline only shows the task checklist
  on the literal anchor occurrence to avoid implying otherwise; the
  Edit Event modal manages them regardless of which occurrence opened
  it.

## Sprint 5 — Dashboard as daily home screen

- `Task.completed_at` (nullable RFC3339) is stamped by
  `queries::tasks::set_completed` when a task is completed, and
  cleared when it's un-completed — so the streak only ever reflects
  tasks that are *currently* done, not historically-ever-done.
- `tasks::streak::current_streak` is a pure function over a set of
  dates, deliberately kept free of any database or UI dependency so
  its day-boundary logic (gaps, "yesterday still counts if today's
  quiet yet") is unit-tested directly rather than through the UI.
- `ui::event_time::parse_event_times` factors out start/end time
  parsing and the midnight-crossing rule that Sprint 3 introduced, so
  the Calendar's full Edit Event modal and the Dashboard's smaller
  quick-event form share one implementation instead of two that could
  drift apart.
- "Today's Tasks" is deliberately a different set from the To Do
  list's full view: due-today OR attached to a session scheduled
  today. It's meant to answer "what do I need to do today", which
  isn't the same question as "what's on my list at all".

## Sprint 3 — events / revision sessions

- `events::Event` mirrors `tasks::Task`'s shape: a plain data struct
  with datetime math (`duration`, `crosses_midnight`, `day`), ready
  for Sprint 4 to attach tasks to events.
- Event storage keeps `start_time`/`end_time` as RFC3339 strings, like
  tasks' `created_at`. Filtering "events on day X" happens in Rust
  (`ui::calendar::events_on_day`) rather than in SQL — RFC3339's
  timezone offset suffix doesn't play nicely with SQLite's `date()`.
- The Day view computes its visible hour range from whatever's
  actually scheduled (earliest start minus an hour, to latest end plus
  an hour) rather than a fixed window, so both light and packed days
  render sensibly.
- Midnight-crossing is handled at save time: if the end time is
  earlier than the start time, the event's end date rolls to the next
  day automatically, and the timeline labels it "(+1 day)".
- `subject` on `Event` (and the plan's data model in general) is a
  free-text field for now — Sprint 9 introduces a proper Subject
  entity and this becomes a foreign key.

## What's deliberately not here yet

- Per-occurrence content edits and per-occurrence task completion for
  recurring events (only whole-series edits and skip-this-occurrence
  are supported — see the Sprint 6 note above)
- Drag-and-drop event manipulation in the calendar (the plan is
  explicit this waits until basic event manipulation is stable)
- Full progress/analytics beyond the basic streak (Sprint 13)
- Notifications, Hyprland-specific integration (later milestones)
