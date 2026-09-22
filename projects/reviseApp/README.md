# Revise

A lightweight, native Linux revision-planning app written in Rust,
built primarily for NixOS + Hyprland. See `revise_project_plan.md`
for the full agile plan.

## Status: Six rounds of `issues.md` in progress — 5 issues fixed, 21 feature requests shipped, 2 in progress

Sprint 1 delivered the application shell, design system, and
navigation. Sprint 2 added persistent tasks. Sprint 3 added persistent
events. Sprint 4 connected tasks to events. Sprint 5 made the
Dashboard the daily home screen. Sprint 6 added recurring events, with
a feedback pass fixing dialog sizing, a keyboard-shortcut bug, and
adding the command palette. Sprint 7 expanded the Calendar into Day/
Week/Month views. Sprint 8 added notes, with a second feedback pass
fixing a layout bug and making calendar events duration-aware. Sprint
9 introduced Subjects, with a third feedback pass adding automatic
cleanup of past items, more keyboard navigation, and natural-language
dates. Sprint 10 added desktop notifications and startup integration,
followed by a feature-request pass that replaced the notification
backend, added a Pomodoro timer with focus mode, a stat tracker, and
session logging. Sprint 11 closed the remaining gaps in keyboard-first
interaction. Sprint 12 added Revision Sessions, a full-screen focus
mode started directly from a Calendar event. A first `issues.md` pass
fixed 2 reported issues and shipped 5 feature requests (Pomodoro
Subject Links, Pomodoro Stopwatch, Notes Folders, Startup Scripts/
Commands, Background Notifications) — see below for that batch, and
`packaging/install.sh`/`revise-launch.sh` for the desktop-launch
follow-up after it (a real, still-open graphics-driver compatibility
issue on at least one NVIDIA + Wayland machine was investigated at
length and deliberately parked — see `packaging/README.md`'s "Why the
binary might only launch from inside the repo" for the full, honest
writeup, including a mistake caught and corrected along the way). A
second `issues.md` batch shipped 4 more feature requests (Improved
Hours Per Subject View, Improved Task/Event Addition, Task Assignment,
Event Logging) — see below. **A third `issues.md` batch, all six
items shipped, in the order listed:**

- **Better To Do Addition** — task creation is now a single, fully-
  featured "Add Task" form (title, description, subject, due date,
  repeat, subtask parent, estimated time all available immediately),
  replacing the old two-step quick-add-row-then-"More options" flow.
- **Repeating Tasks** — a task can be marked to repeat weekly;
  completing it automatically creates next week's instance, while the
  completed one keeps its place in streak/completion history.
- **Sub Tasks** — a task can have child tasks (one level of nesting);
  a parent can't be marked complete until every child is, and a new
  "+ Add subtask" button on the edit form creates one directly.
- **Task Timings** — an optional estimated-time field on tasks now
  counts toward "Hours Per Subject" (and "Total time by Week") once
  completed, but only for standalone tasks with no linked revision
  session, to avoid double-counting time a session already accounts
  for.
- **Time Per Subject pie chart** — a hand-drawn pie chart alongside
  the existing bar chart, shaded in a gray gradient rather than
  distinct colors, consistent with the app's monochrome design.
- **Total time by Week** — a hand-drawn line chart of total logged
  time per week, plus a "up 14% from last week"-style comparison
  message.

Covered by `cargo test` (186 tests total, up from 160 at the start of
this third pass) — see `docs/architecture.md` for the full breakdown
of what's tested where, across all three `issues.md` passes.

## Flashcards — feature request (outside the `issues.md` cycle)

A fifth section, **Flashcards**, alongside Dashboard/Calendar/To Do/
Notes (shortcut digit `5`). The app has no flashcard editor of its
own — decks only come in by importing a `.txt` file (drag it onto the
window, or paste its path) in the plain format a hand-written vocab
tool already used: a title line, then question/answer pairs one per
line. Once imported, a deck is stored in SQLite and can be studied
again without re-uploading the file. Studying a deck shows one card
at a time; revealing a card's answer logs it toward a new **"Flashcards
seen"** all-time stat on the Dashboard, next to Focus time and
Pomodoros completed. Deleting a deck removes its cards with it — a
flashcard has no meaning detached from its deck, unlike e.g. a Notes
folder.

Covered by `cargo test` (201 tests total, up from 186) — see
`docs/architecture.md` for what's tested where.

## App icon & desktop packaging — direct request (outside the `issues.md` cycle)

Revise now has a real icon (`packaging/revise.svg`, matching the
app's own monochrome palette) — previously `Icon=revise` in every
`.desktop` entry resolved to nothing on disk anywhere. `packaging/
install.sh` installs it to the standard XDG hicolor location and
refreshes the desktop/icon caches; the Nix package
(`flake.nix`'s `packages.default`) installs the same icon plus its
own generated `.desktop` entry, so `nix build`/`nix profile install`
alone gets full desktop integration too, not just the home-manager
path. `packaging/install.sh` and `packaging/revise-launch.sh` were
run for real in this environment (a genuine `cargo build --release`,
installed binary, generated `.desktop` file validated with
`desktop-file-validate`, `--autostart`/`--hyprland-exec-once` both
exercised including idempotency) — see `packaging/README.md`'s "What
hasn't been verified" section for exactly what that does and doesn't
cover.

## Sixth `issues.md` pass — in progress (3 of 4 requests shipped so far)

- **Improved Line Graph.** The Dashboard's "Total time by Week" chart
  now draws one line per subject (a small fixed color per subject,
  stable across renders) instead of a single combined-total line, with
  a legend underneath. The "up 14% from last week" summary message
  still reflects the combined total, computed by summing the same
  per-subject data the lines are drawn from rather than a second
  query. A deliberate, scoped exception to the rest of the app's
  monochrome design — this is the one chart color was explicitly
  requested for.
- **Tasks Repeating Through A List.** A repeating task can now cycle
  through a list of topics instead of staying the same every week —
  check "Weekly", then "Repeat through a list of topics" to reveal a
  topics field (one per line) and a choice of order: In order,
  Shuffle, or Spaced repetition (always shows whichever topic has come
  up least so far). The task's title becomes `"<title>: <topic>"`,
  advancing to the next one automatically each time it resets. `Smart`
  mode (the request's own "Future Extension," varying frequency by a
  subtopic's flagged weakness/strength) isn't implemented yet.
- **Subject Sub Topics.** Click a subject in "Manage Subjects" to
  expand its list of subtopics, with the same add/rename/archive
  controls as top-level subjects. A subtopic is a completely ordinary
  subject otherwise — it shows up in every existing subject picker
  across the app (tasks, events, notes, Pomodoro) with no changes
  needed there, exactly as "referenced as their own subjects" asked
  for. Its logged time counts toward its parent's total on every chart
  that aggregates "hours by subject" (both the bar chart and the new
  per-subject line graph above), without also double-counting as a
  separate entry of its own.
- **Optimisation** — a scoped, safety-first pass, not the sweeping
  rewrite a full "code clean" could mean. See `docs/architecture.md`'s
  "Sixth `issues.md` pass" section for exactly what was found, what
  was (and wasn't) changed, and why — the short version is that a
  larger cleanup (consolidating ~8 nearly-identical subject-picker
  `ComboBox`es scattered across four files) was identified but not
  done without the person's own go-ahead first, per the request's own
  explicit instruction not to risk functionality without asking.

Not yet covered by a fresh `cargo test` run (the assistant's own
sandbox can no longer compile this project at all post-migration —
see `HANDOFF.md`); ask for one after the remaining two items are in.

## Fifth `issues.md` pass — 2 issues fixed (1 reported ahead of the file), 1 feature request shipped

- **Bug fix (reported directly, ahead of processing the file):
  repeating tasks reoccurred immediately after being marked
  complete.** Completing a `repeat_weekly` task used to spawn a whole
  new row for next week's occurrence right away, so it was already
  sitting in the To Do list well before it was actually due. Fixed:
  completion no longer spawns anything; a repeating task simply stays
  completed, and a daily pass resets that *same* row — uncompleting it
  and advancing its due date — once its next due date actually
  arrives. A weekly task is now always exactly one row, reused
  forever, rather than a new one accumulating every week.
- **Toggle Switch In App Settings from Hyprctl Notify to Caelestia
  Notify.** Settings now has a Notification backend choice: Hyprctl
  (Hyprland's own built-in popup, unchanged default) or Caelestia
  (sends a standard desktop notification via `notify-send`, which
  caelestia-shell — a Quickshell-based Hyprland shell — picks up and
  renders in its own style). Both are just a subprocess spawn, same as
  the app's very first notification mechanism; no D-Bus library
  linked into the binary either way.
- **Issue: Packaging Error — `cargo run` fails with `linker \`cc\`
  not found` outside a working dev shell.** Diagnosed against both of
  this project's `flake.nix` files (this repo's own, plus the user's
  separate outer environment-level one) — neither is missing
  anything; both use `pkgs.mkShell` on the default `stdenv`, which
  always bundles a C compiler. The real cause: direnv only reloads
  its environment when the shell displays a new prompt, not partway
  through one command line, so chaining `cd project && cargo run` in
  a single line runs `cargo` under whatever environment was active
  *before* entering the directory. Fixed by changing how the commands
  are run — `cd` and `cargo run` as two separate commands, or `direnv
  exec` for one-liners — not by changing any packaging code.

Covered by `cargo test` (221 tests total, up from 213) — see
`docs/architecture.md` for what's tested where.

## Fourth `issues.md` pass — 1 issue fixed, 3 feature requests shipped

- **Issue: Button Position Fix.** The Flashcards "Import" button
  rendered ~20% off the right edge of the screen. Cause: `Input`
  fills all available width in its row by default, so a button placed
  right after it in the same `ui.horizontal` had nowhere left to lay
  out. Fixed by stacking the field and the button on separate lines —
  the same layout every other text-field-plus-button form in the app
  already used; the Flashcards import panel was the one place that
  hadn't followed it.
- **Built-In Flashcard Editor.** A note can now be flagged "This is a
  flashcard set" in its edit modal; its `content` is then read as
  plain question/answer pairs (one question per line, its answer on
  the next) instead of freeform text, and it becomes studyable from
  the Flashcards view alongside imported decks — read-only there,
  since it's still edited and deleted from Notes, its actual owner.
  Sets file under subject subfolders inside a single top-level
  "Flashcards" folder (created the first time one's needed), a small,
  additive extension to folders: they can now nest one level, same
  precedent as Sub Tasks.
- **Flashcard Keyboard Shortcuts.** While studying: `Tab` for the next
  card, `→`/`←` reveal-then-advance / un-reveal-then-retreat,
  `Enter`/`Space` flip the current card without moving, `Esc` ends the
  session.
- **Navigation Bar Update.** The command palette (`/`) now has a
  `flashcards [set]` mode: typing "flashcards" switches the palette
  into a picker over every studyable set (imported decks and
  flashcard-set notes together), filterable by a further word, and
  picking one jumps straight into studying it.

Covered by `cargo test` (213 tests total, up from 201) — see
`docs/architecture.md` for what's tested where.

## Second `issues.md` pass — 4 feature requests shipped

- **Improved Hours Per Subject View** — the Dashboard's subject-hours
  chart now has a This Week / This Month / This Year selector
  (defaulting to This Week), instead of always showing an unbounded
  all-time total.
- **Improved Task/Event Addition** — task and event creation both used
  to be thinner forms than editing the same thing. Fixed by unifying
  them: a new "More options…" button opens the exact same modal
  editing uses for tasks; Dashboard's "New Event" now opens the
  Calendar's full Edit Event modal directly (description, recurrence,
  startup command and all) instead of a separate, thinner "quick add"
  modal, which was deleted outright.
- **Task Assignment** — tasks can now be assigned to a specific day,
  independent of their due date, via a new "Assign…" button on the
  Dashboard's Today's Tasks or a new "Tasks for this day" section at
  the top of the Calendar's Day view. An explicit assignment overrides
  the due-date default rather than adding to it — a task due today
  that's been assigned elsewhere no longer clutters today's list.
- **Event Logging** — calendar events can now opt out of session
  tracking entirely at creation (for meeting-style blocks that were
  never a revision session), and for events that do track sessions,
  the Done/Not-Done buttons — and the Start Session button — disappear
  once an occurrence has actually been logged, replaced by "Edit
  hours"/"Clear" instead of sitting there taking up space indefinitely.

Note: the "Better To Do Addition" feature request (third pass, above)
has since superseded the "More options…" button this round introduced
— task creation no longer has a separate quick-add row at all.

## First `issues.md` pass — 2 issues fixed, 5 feature requests shipped

**Issues fixed:**

- **Calendar Day view events now genuinely span the time they take
  up.** Previously the hour ruler and each event's duration-
  proportional accent bar used two different, unrelated scales, so a
  2-hour event's bar was twice as tall as a 1-hour one's but neither
  actually lined up against the hour lines beside it. The Day view is
  now a single proportional timeline — hour gridlines and event cards
  share one `PIXELS_PER_HOUR` scale, computed from real clock time.
- **The `.desktop` file's `Exec=revise` not launching**, because
  nothing previously guaranteed `revise` was actually installed
  anywhere on `PATH`. This flake now exposes a `homeManagerModules.
  default` module — `programs.revise.enable = true;` installs the
  package for real and points the generated `.desktop` entry straight
  at its Nix store path. See `packaging/README.md`.

**Feature requests shipped:**

- **Pomodoro Subject Links** — pick a subject on the Dashboard's Focus
  card before starting a Pomodoro. A completed Work phase with a
  subject attached is recorded exactly like a manually logged revision
  session — a real Calendar event plus a session-log entry.
- **Pomodoro Stopwatch** — a "Stopwatch" panel on the Dashboard starts
  a count-up timer with no Calendar event required up front (title +
  optional subject). It's the same underlying type as a Revision
  Session (just event-less), so it reuses that full-screen overlay
  unchanged; on Finish, a subject-linked run synthesizes a Calendar
  event the same way Pomodoro Subject Links does.
- **Notes Folders** — a new folder tab row above the Notes view's
  subject filter (All / Unfiled / each folder), a "+ Folder" create
  flow with a curated set of muted colour swatches, and a per-note
  folder picker. Deleting a folder detaches its notes rather than
  deleting them.
- **Startup Scripts/Commands** — an optional shell command on any
  Calendar event, run once at its start time. **Off by default** via
  its own Settings toggle — a deliberate gate, given the security
  surface of running an arbitrary shell string unattended.
- **Background Notifications** — a new `revise-reminderd` binary
  (built from the same crate, now split into a library + two
  binaries) keeps polling for due reminders/startup commands after
  the GUI is closed, sharing the exact same reminder logic rather than
  duplicating it. The GUI and daemon coordinate via a small heartbeat
  file so they never double-fire — safe to leave the daemon always
  enabled. Packaged as a systemd user unit, manually or via the
  home-manager module's new `programs.revise.reminderDaemon.enable`.

Covered by `cargo test` (152 tests total, up from 137 at the start of
this pass) — see `docs/architecture.md` for the full breakdown of
what's tested where.

**Also added since:** `packaging/install.sh` + `packaging/
revise-launch.sh` — a working install path that doesn't require Nix.
Builds a release binary, installs it plus a launcher script that
finds it regardless of how it was built, and writes a `.desktop` file
with an absolute `Exec=` path so it's never dependent on `PATH`
resolution. Two opt-in flags: `--autostart` (XDG autostart) and
`--hyprland-exec-once` (idempotently adds Hyprland's actual startup
hook, `exec-once`, to `hyprland.conf`) — see `packaging/README.md` for
the full writeup, including where `hyprctl` itself actually fits in
(runtime dispatch/keybinds, not startup).

### Sprint 12 — Revision Session / Focus Mode, for context

Sprint 12 added **Revision Sessions**: starting a full-screen focus
mode directly from a Calendar event, distinct from the Pomodoro timer:

- **"Start Session"** on any Calendar event occurrence (Day view)
  opens a full-screen overlay showing that occurrence's subject,
  title, a live elapsed-time counter, and its attached tasks
  (toggleable in place, same as everywhere else).
- Unlike the Pomodoro, a session has **no fixed length** — it counts
  up until you click **Finish**, at which point the elapsed time is
  recorded as the occurrence's actual duration via the existing
  session-log feature. **Cancel** discards the session instead,
  recording nothing.
- **Escape minimizes** the overlay (the session keeps running in the
  background); a "Resume" button appears in the nav bar to bring it
  back. The running session lives only in memory, so closing the app
  mid-session simply loses it — no partial or inconsistent data is
  ever written.
- Covered by `cargo test` (137 tests at the time, up from 130): the
  pure `ActiveSession` timing logic in `revision_session.rs` (start/
  pause/resume/tick, elapsed-time formatting, and rounding to the
  nearest minute for the recorded duration). The overlay's UI wiring
  itself isn't separately unit-tested, consistent with how the
  Pomodoro overlay's wiring in `app/mod.rs` isn't either.

### Sprint 11 — Keyboard-First Interaction, for context

Sprint 11 closed the remaining gaps in keyboard-first interaction —
most of the groundwork (vim-esque insert mode, the command palette,
arrow-key calendar navigation) was already in place from earlier
feedback passes:

- **The plan's suggested single-letter shortcuts**: `N`ew event,
  `T`ask, `D`ay/today, `W`eek, `M`onth — each just an alias for a
  command that already existed in the palette, so there's exactly one
  place each action is actually implemented.
- **List keyboard selection** in To Do and Notes: `↑`/`↓` move a
  selection cursor (shown with the same focus-colored border the nav
  bar already uses for the active section), `Space` completes/
  uncompletes the selected task, `Enter` opens the selected item for
  editing.
- **A discoverable shortcuts reference** (`?` or the "Keyboard
  Shortcuts" palette command) lists every shortcut in the app,
  grouped by where it applies — directly answering "shortcuts are
  discoverable" rather than leaving it to the one-line hint bar alone.
- Every new shortcut goes through the same "normal mode" gating
  (nothing focused) the rest of the app's keyboard scheme already
  established, so none of them can fire while typing — "shortcuts do
  not conflict with normal text input" holds by construction, not by
  auditing each new one individually.
- Covered by `cargo test` (130 tests total, up from 123): the list-
  selection movement rule (`ui::list_selection::move_selection`) —
  clamping instead of wrapping, and landing on the first item whether
  the initial press is Up or Down — plus the palette's expanded
  command set.

### Sprint 10 + feature-request pass, for context

Sprint 10 added desktop notifications and startup integration,
followed by a feature-request pass that replaced the notification
backend entirely, and added a Pomodoro timer with focus mode, a stat
tracker, and session logging:

- **Session reminders**: a desktop notification fires a configurable
  number of minutes before a revision session starts, via `hyprctl
  notify` — Hyprland's own built-in notification mechanism — rather
  than D-Bus. This was a deliberate switch after the D-Bus route (via
  `notify-rust`) turned into a real build blocker on Nix, where
  `libdbus`/`dbus-1` aren't always straightforward to locate; since
  the plan's target platform is Hyprland specifically, leaning on
  what it already provides removed the dependency entirely rather
  than chasing it further.
- **Task reminders**, **notification settings**, and **no background
  process when closed** are unchanged from Sprint 10's original
  design — only the delivery mechanism changed.
- **Startup & launcher integration**: `packaging/revise.desktop` plus
  `packaging/README.md`, now covering Hyprland's native Lua config
  (`hl.on("hyprland.start", ...)`), classic `exec-once`, XDG
  autostart, and installing into the application menu
  (`~/.local/share/applications/`).
- **Two bugs fixed**: the Month view's weekday header labels drifted
  out of alignment with the day grid below them by Thursday — a
  hand-rolled `available_width() / 7.0` division recomputed after
  each label had already consumed space, versus the data grid's
  correct, even `ui.columns(7, ...)` split. Now both use the same
  mechanism.
- **Pomodoro timer with focus mode**: a circular countdown on the
  Dashboard: 25-minute focus / 5-minute break, auto-cycling, with a
  notification on each phase change. Starting it enters a full-screen
  focus view that takes over the entire shell (nav bar included); a
  completed focus session is durably logged (survives even if the app
  closes mid-break).
- **Stat tracker** on the Dashboard: total focus time and Pomodoros
  completed (from the Pomodoro log), a GitHub-commit-style heatmap of
  tasks completed per day (reusing Sprint 10's `completion_log`), and
  an hours-per-subject bar chart.
- **Session logging**: any occurrence of an event, once it's started,
  can be marked completed as scheduled, marked as not having
  happened, or logged with a different actual duration — without
  touching the event or its recurrence rule. Denied sessions don't
  count toward "hours per subject"; adjusted ones count their actual
  duration. Un-logged sessions still default to their scheduled
  duration, same as before.

**What's honestly unverified**: this was built in a sandbox with no
display server and no real Hyprland instance to test against.
`hyprctl notify`'s exact syntax matches a working example from the
user's own Hyprland config, and the Lua autostart snippet matches
their config's existing structure, but neither has been confirmed
against a real running Hyprland session — see `packaging/README.md`
for the specific gaps.

## Keyboard

Navigation is vim-esque:

- **Normal mode** (nothing focused): `1`-`5` jump to a section, `Tab` /
  `Shift+Tab` cycle, `i` enters insert mode on the active view's
  primary field (new-task title on To Do, search on Notes, import path
  on Flashcards), `/` opens the command palette, and `?` opens the
  full shortcuts reference.
- **Insert mode** (a field is focused): typing goes entirely to the
  field — digits, letters, everything. `Escape` returns to normal mode.
- **Single-letter shortcuts**: `N` new event, `T` new task, `D` today,
  `W` week, `M` month — each just a keyboard alias for a command also
  reachable through the palette.
- **List selection** in To Do and Notes: `↑`/`↓` move a selection
  cursor (shown with a focus-colored border), `Enter` opens the
  selected item, and in To Do, `Space` completes/uncompletes it.
- **Calendar**: `←`/`→` step through days, weeks, or months depending
  on the active subview.

This means typing a due date like `2026-09-01` no longer gets read as
"jump to section 2" partway through — every shortcut above only fires
in normal mode, so none of them can collide with typing.

## Running

```sh
cargo run
```

## Testing

```sh
cargo test
```

Requires a Rust toolchain (edition 2021) and, on Linux, Wayland or X11
development headers (`libwayland-dev`, `libxkbcommon-dev`, `libx11-dev`,
`libxcb*-dev`) for the windowing backend.

## Layout

See `docs/architecture.md`.
