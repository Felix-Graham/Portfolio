# Revise

A lightweight, native Linux revision-planning app written in Rust,
built primarily for NixOS + Hyprland. See `revise_project_plan.md`
for the full agile plan.

> **Current state:** time-blocking (Phases 0–4), the calendar "now" pointer and
> the startup-questions quiz are built and tested (854 library tests); the newest
> features are described first below. Start a new session from `HANDOFF.md`
> ("START HERE"). A reusable sandbox build script is in
> `tools/sandbox-toolchain.sh`. Nothing visual has been seen on a real screen yet.

## Status: Eight rounds of `issues.md` — 6 issues fixed, 27 feature requests shipped, plus a CLI/"API", Notes Grid/List view, Auto-assign, a countdown Timer, stopwatch Focus time and a sectioned Dashboard

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

## An "API" (`revise-cli`) — direct request (outside the `issues.md` cycle)

"I would like the app to have something like an API... It should be
callable from other applications and through the terminal from
anywhere. It would add hours/contribute to statistics. This would
allow other revision applications... to be registered and accounted
for."

No network server — this is a single-user local app, and
`revise-reminderd` already proved out the mechanism that actually
matters: a second, GUI-less binary opening the exact same SQLite file
directly. **`revise-cli`** is that same idea applied to writes. It's
installed onto the same `$PATH` as `revise` itself, so it's genuinely
callable from anywhere, by anything that can shell out (which is every
language, including a Python script's `subprocess`):

```
revise-cli register "French Quiz"        # once, prints back a source id
revise-cli log --source "French Quiz" --subject French --minutes 25
revise-cli log --source 1 --subject French --minutes 10 --note "Chapter 4" --date 2026-09-20
revise-cli sources                        # list what's registered, with totals
revise-cli subjects                       # the subjects `--subject` accepts
revise-cli focus --kind timer --minutes 25                       # Focus time, no subject
revise-cli focus --kind pomodoro --minutes 25 --subject French   # ...and French's hours too
```

`log` is for *other* tools: it files time under a registered source and
requires a subject. **`focus`** is for Revise's own timers — it records a
run exactly as the main app does (`--kind` is `pomodoro`, `timer` or
`stopwatch`): the minutes always count toward Focus time, and if a subject
is given they also become that subject's hours. The subject is optional
there, as it is in the app's own Pomodoro, Timer and Stopwatch; a name that
doesn't exist is an error rather than silently unattributed time. `--title`
names the Calendar event a subject run creates, and `--no-notify` skips the
desktop notification. This is what **`revise-applet`** logs through.

Two new tables back this: `sources` (a registered tool — just a unique
name, no token/secret) and `external_contributions` (a subject, a
date, a minute count, an optional note, which source logged it).
`--subject` must already exist in "Manage Subjects" — a typo surfaces
as a clear error rather than silently creating a new subject. Every
contribution counts toward its subject's totals everywhere in the app
(Dashboard charts, Calendar-adjacent stats) via one more small addition
to `stats::minutes_per_subject_in_range`, the same way a completed
task's estimated time already does. A **"Manage Sources"** modal in the
app itself (next to "Subjects" in the nav bar, and in the command
palette) lists registered sources with their all-time totals, and
supports renaming/deleting from the GUI too — registering doesn't
strictly require a terminal. Logging fires a desktop notification on
success, the same free feedback every other "something just happened"
moment in the app already gets.

See `docs/architecture.md`'s "An API (`revise-cli`)" section for the
full design discussion and the trade-offs considered before landing on
this shape.

## `revise-applet` — a small Timer / Pomodoro / Stopwatch window

"A separate applet... a smaller unit for the stopwatch and timer section."
A third binary from the same crate, launched on its own from a menu entry
("Revise Timer"; see `packaging/README.md`). It follows the main app's theme
exactly and opens on the Dashboard Focus page's layout, simplified: **Timer**
and **Pomodoro** side by side, the **Stopwatch** across the bottom, each in its
own section outlined in light grey — and the outline of whichever section the
pointer is over lights up. Choose a subject (optional) in a section and start
it: that unit fills the window until it finishes, then the menu returns.

* **Timer** finishes when the countdown reaches zero; **Pomodoro** is one
  cycle — the focus phase, then its break (skippable); **Stopwatch** finishes
  when you press Finish.
* The time is logged through the API (`revise-cli focus`, called in process):
  Focus time always, and the subject's hours if one was picked. **Stopping
  early** (or closing the window mid-run) logs the time actually spent so far
  rather than discarding it. Space pauses and resumes.
* It sends the same desktop notifications as the main app.

Logic and drawing live in `src/applet/` (`mod.rs` the state and logging,
`ui.rs` the window), unit-tested without a screen; `src/bin/revise-applet.rs`
is only the window options.

## Notes: Grid/List view + Study Now — direct request (outside the `issues.md` cycle)

Notes now offers a choice between the original one-per-row **List**
view and a new **Grid** view (several fixed-width cards per row,
computed from the available width via `ui.columns` — the same
mechanism Calendar's day/week grids and the Dashboard's stat panels
already use). A small two-way toggle next to the subject filter
switches between them; defaults to List, so nobody's existing layout
changes unless they pick Grid. Any note flagged as a flashcard set
(see "Built-In Flashcard Editor" below) now shows a **Study Now**
button on its card in both layouts, which builds a study session for
that note and jumps straight to the Flashcards view to show it — the
same `start_study_session` pipeline the Flashcards view's own "Study"
button and the command palette's `/flashcards [set]` picker already
share.

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

## Startup questions (direct request)

**Request:** a startup quiz — random multiple-choice questions from the stored
flashcard sets, full-screen at launch, a Skip button bottom-right, `n` questions
set in Settings (default 5), and a progress bar along the top that fills
proportionally green for a correct answer and red for an incorrect one.

- **When:** once per launch, before the shell. Nothing appears if the setting is
  off or there's nothing to ask (no flashcards, or no card with a different
  answer to sit beside). Fewer than `n` cards means fewer questions.
- **Questions:** random cards from every imported deck and flashcard-set Note.
  Four answers (the right one plus up to three others, shuffled). Wrong answers
  come from the same set first, then from any set, never repeating or matching the
  right one. The same question text is never asked twice.
- **Answering:** click an answer or press 1–4. The right answer gets a green
  outline and ✓; a wrong pick gets a red outline and ✗. It then waits for
  **Next** (Enter/Space; **Finish** on the last). **Skip** (S) sits bottom-right
  until you answer, then becomes Next/Finish. Each answered question counts
  toward "Flashcards seen" on the Dashboard; a skipped one doesn't.
- **Progress bar:** one equal unit per question, left to right. A unit turns
  green or red when its question is answered, so ten questions with the third
  correct show three units filled. A skipped question is filled grey, so the bar
  still reaches the end.
- **Settings → Startup questions:** an on/off checkbox (on by default) and the
  question count, 1–50 (default 5).

**Interpretation calls worth checking:** the worked example ("10 questions, q3
correct → 3/10 filled") was read as one unit per answered question in order, not
as a running tally of only the green ones. Skipped questions fill grey (the
request only names green and red). The feature is on by default.
**Not verified:** how any of it looks on a real screen. Logic and keyboard play-
through are tested headlessly; clicking the buttons and the colours have not been
seen.

## Calendar "now" pointer (direct request)

**Request:** a visible pointer on the calendar showing the current time (hover
for the time including seconds): a single white line starting left of the hours
and moving down through the day relative to blocks. **Follow-up:** extend it
fully across the calendar (right of the times), with a colour filter applied
behind it over the past. Answers given: the filter covers everything above the
line, as a dark dim, medium by default and configurable in Settings.

- **Day view, today only.** A single white line (the palette's `focus` colour,
  1.5pt) at the current time, on the same scale as the hour lines, events and
  blocks. It starts a few points left of the hour labels and runs to the right
  edge of the timeline. Drawn last, over events and blocks.
- **The dim.** Everything above the line (the whole past part of the timeline,
  hour labels and events included) is covered with the page background at a
  chosen opacity, so the past fades into the background. Once the timeline's
  hours are all over, all of it is dimmed; before they begin, none.
- **Setting:** Settings → Calendar → "Dim the past on today's timeline by N %".
  Default 40 (medium), 0 turns it off, capped at 90 so the past never vanishes.
- **Moves with the seconds**, and the Day view asks egui for a repaint once a
  second while looking at today, so the line and dim creep down by themselves.
- **Hover** the line for `Now — HH:MM:SS`.
- **Hidden** on any other day, on a day that reads "Nothing scheduled", and the
  line (not the dim) when the current time is outside the hours the timeline shows.

**Interpretation calls worth checking:** the dim covers the hour labels as well as
the events (the whole timeline rectangle above the line). The pointer is not drawn
in Week or Month views. Outside the shown hours the line disappears rather than
widening the timeline to fit.

**Tests:** `cargo test` now 854 library tests. Pure tests for the y maths, span,
dim extent and colour; headless render tests that the line and dim are drawn at the
current time on today's timeline, absent on another day, off at 0%, requests a
repaint within a second, and that a block can still be dragged where the line
crosses it. **Not verified:** how it looks on a real screen (in particular whether
40% reads as "medium"), and the hover tooltip itself (tested only by its text).

## Time-blocking: tasks get a time of day, drawn between your events (direct request, implemented from `docs/planning/time-blocking.md`)

Auto-assign decides **which day** a task is on; time-blocking decides
**what time of day**, by placing each day's tasks into the free gaps
between that day's calendar events. Off by default — turn it on under
*Settings → Time blocking*.

- **Plan times** (To Do view, also in the command palette) places every
  assigned task inside the working window (09:00–21:00 by default),
  around events — recurring ones expanded, and one that runs past
  midnight blocks the next morning. A task goes whole in the first gap
  that holds it; if none does it is **split** across that day's gaps,
  and if that is still not enough the rest **carries to the following
  days** up to its deadline (a "maths assignment" becomes several
  sessions). A task is placed in full or not at all; one that can't be is
  reported ("2 didn't fit on their day").
- **Auto-assign** now also leaves room for your events (a day of meetings
  takes less work) and finishes by planning times, with one repair round
  if a task didn't fit the day it was given. With time blocking off
  nothing about Auto-assign changes.
- **Day view:** blocks are drawn on the same timeline as events, as
  outlines in the task's subject colour. Drag a block to move it, drag
  its bottom edge to resize it (this updates the task's estimate), click
  to select, right-click for *Pin / Unpin*, *Remove this block*, *Re-plan
  times*. Moving or resizing **pins** the block: re-planning never moves a
  pinned block, and the task stops being Auto-assign's to move.
  A drop onto an event or another block is refused.
- **Events look like blocks.** In the Calendar's Day view an event is drawn
  as the same kind of card as a block: an outline in its subject's colour
  (grey for a repeating event with no subject), its name, its times, and a few
  small notes (`↻ weekly`, `1 / 2 tasks`, `not done`). Everything that used to
  be a button on the card is in its **right-click menu**: Start session, Mark
  done / not done (or Edit logged hours / Clear log once logged), Edit, the
  attached tasks' checkboxes, and Skip this occurrence + Delete series (or
  Delete). The description, subject name, length and logged outcome are in the
  hover tooltip.
  - **Double border = can't be moved:** a repeating event (an occurrence can't
    be moved on its own) and a block that is in progress or worked.
  - **A finished item is blank** but for its name, its times and its subject
    colour: a worked block (or one whose task is done), or an event whose
    session was logged completed or adjusted. No notes, strike-through or
    dimming.
- **Elsewhere:** the day's task list shows each task's time or
  "unscheduled"; the Dashboard's Today's Tasks lists planned tasks in
  time order with their times; the Week view shows small `~09:00 Title`
  chips. Command palette: *Plan Times*, *Clear Auto Blocks* (keeps pinned
  and finished blocks).
- **Per-weekday hours:** under *Settings → Time blocking → Different hours
  on some days*, each weekday can be a **day off** (nothing planned,
  Auto-assign gives it no work, a task assigned there moves on to the next
  working day when splitting is on) or have **own hours** (e.g. a short
  Friday). Days left alone follow the default window, including when you
  change it later.
- **Settings:** working hours, snap grid (default 15 min), shortest usable
  gap, gap kept around events, most pieces per task (default 6; `1`
  turns splitting off), and an opt-in *Re-plan times automatically at
  launch and each new day* (off by default; re-plans times only and never
  moves a pinned block or a task's day).
- **Reminders:** shortly before a planned block starts (same lead time as
  session reminders, "starts in 7 minutes (14:00–14:45)"). It runs in the
  shared reminder scan, so it works from the app and from the background
  `revise-reminderd` alike, with no duplicate when both are around. On by
  default once time blocking is on; its own checkbox in Settings, and it
  stays silent if notifications as a whole are off.
- **A block follows what actually happens** (direct request):
  - **Started by a timer or stopwatch.** Start one within 15 minutes (a
    Settings value) of a planned block and that block goes *in progress*.
    A session that names a subject only matches a block of that subject; one
    without a subject matches any, and the nearest wins. A free-standing
    stopwatch or a timer links; a Calendar-event session does not (the event
    is already its time). Cancelling gives the block back to the planner.
  - **Adjusts to the time really spent.** When the stopwatch/timer ends the
    block becomes the span actually worked — extended if it ran over, shorter
    if it stopped early — and shows as *worked*. Worked and in-progress blocks
    can't be dragged or resized, and re-planning keeps them and plans around
    them.
  - **Ticking the task off early.** A block under way is cut off at that
    moment; a block still ahead is removed (no time was spent on it); one
    already over is kept.
  - **Unfinished at the end → a new block at least a day ahead.** The task's
    estimate drops by the time worked (a used-up estimate gets one more
    minimum-size session), it moves to tomorrow (or stays on a later day it
    already had), and one block is planned for what is left without disturbing
    any other block. A block that passes with nothing worked is handled the
    same way, with the estimate unchanged, checked about once a minute and at
    launch — so a task you do without a timer and tick off late will already
    have moved to tomorrow.
  - **Kept off the calendar.** The one-off event a finished timer/stopwatch
    creates so its time reaches the stats is **hidden** when the run was linked
    to a block: the block already shows that time, so a second card for the same
    minutes would only clutter the day. It is left out of the Day/Week/Month
    views, the Dashboard's events, the task and note event pickers and the
    reminders, but still counts in Hours Per Subject (and as busy time for the
    planner). A run that wasn't linked to a block behaves exactly as before.
  - **Added to stats.** A linked run counts toward its task's subject (when
    the run has none of its own) through the existing pipeline — the one-off
    event a finished timer/stopwatch already creates — so it is counted once,
    and blocks themselves never add subject time. A task with no subject adds
    Focus time only.

Where it lives: pure planner `src/tasks/time_blocking.rs`; stored block
`src/tasks/block.rs`; storage `src/database/queries/task_blocks.rs`
(migration #27); orchestration `src/database/queries/time_blocking.rs`;
Stage A capacity in `src/tasks/auto_assign.rs` (`plan_with_capacity`);
occurrence expansion moved to `src/events/occurrence.rs`.

Covered by `cargo test` (786 library tests pass): property
tests for the planner (no overlaps, on the grid, all-or-nothing, order
independence, and "splitting off == planning each day alone"), database
tests including every delete cascade, and **headless egui render tests**
that drag, resize and drop-refuse a block through real layout and input.

**Not verified on real hardware:** none of the visuals or the *feel* has
been seen or used on a screen. In particular check: that the 6px resize
strip is easy to grab, the cursor changes, the right-click menu appears,
block outlines line up with the hour ruler, and the wording in Settings.
The block-start **reminder** is covered by tests of its timing, de-duplication
and gating, but no notification has actually been seen on a desktop.

## Topic-list tasks: one task, renamed to each day's topic (direct request)

**Request:** "Tasks which repeat and loop through a list of topics
should only show the parent task (task which currently spawns each topic
task). The parent task should be renamed daily to the corresponding
topic for that day. This will remove clutter in the task section."

**What changed:**
- **"Plan each day's topic ahead" is gone.** It turned a topic-list task
  into a hidden template and made Auto-assign create one task per day,
  which is the clutter. The checkbox, the "◎ plans topics ahead" tag,
  the generator, and "created N topic tasks" in Auto-assign's summary
  are removed (about 20 tests for them with them). A topic-list task is
  now always exactly one task.
- **The one task is renamed to each day's topic.** For a daily task the
  title becomes `<name>: <topic>` for the day it falls on, each day (a
  weekly one: each week). The edit form says so under the list options.
- **It lands on the right topic after skipped days.** The reset pass now
  advances the list by however many days it moved, not by one. A 3-topic
  daily list shown as A on the 1st and not opened until the 4th shows
  that day's topic (A), not the one after the last it displayed (B). A
  weekly list unopened for three weeks does the same.
- **Existing "plan ahead" data is converted automatically** at launch
  (`retire_topic_planning`, run before the reset pass): the template
  becomes an ordinary topic-list task showing a topic and the reset pass
  then moves it to today's; its **unfinished generated per-day tasks
  are deleted**; finished ones are kept until cleaned up like any
  completed task.

**Works with the repeat rules above:** a daily topic task ticked
yesterday comes back unticked today with today's topic; one ticked today
stays ticked until tomorrow; an unticked one is renamed when the next
day arrives (so it is never stuck on an old topic).

**To check:** topics are chosen by *count of days elapsed* from when the
list started, not read off the calendar, so the very first topic is the
first item in your list whatever day you create the task; for Shuffle
and Spaced-repetition lists "that day's topic" is the next one in their
sequence. If you wanted "topic N on the Nth of the month"-style fixed
calendar mapping, say so. Converting an old template **deletes its
unfinished per-day tasks**; that is irreversible. 456 tests pass and the
binaries build.

## Bug fix: repeating tasks now move on their due day if they were ticked (direct report, two rounds)

**Report 1:** "The dates for repeating tasks do not change after a due
date has been reached." **Report 2 (the precise rule):** "On Monday
things SHOULD move unless they were unticked at the end of Sunday. This
goes for all repeating tasks." **Cause:** `reset_due_repeating_tasks`
only acted once the *next* occurrence had arrived (`due_date` + one
interval), so a ticked weekly task kept last Monday's date and tick for
a further week. (My first attempt at this fix moved *everything* the
day after its due date, which was wrong in the other direction —
unticked tasks moved too. Report 2 corrected it.)

**The rule now** (all repeating tasks: weekly, daily, sections, topic
lists), decided by *which day a task was ticked*:

| Task state | What happens |
|---|---|
| Ticked on an earlier day, due today or earlier | Resets to unticked and moves to the next occurrence (a weekly task due Monday → next Monday) — on its due day itself |
| Ticked today | Left alone until tomorrow (the pass runs at every launch; restarting the app Monday afternoon must not un-tick Monday morning's work) |
| Ticked ahead of its due date | Stays ticked until it is due, then resets on the due day |
| Not ticked | Does not move on its due day (it didn't get done). Only when the *next* occurrence arrives is the missed one dropped and the task re-dated, as before, so it is never late for ever |

A repeating **parent whose sections are all ticked counts as ticked**
even if you never ticked the parent, so it moves with its sections
instead of being stranded on last week's date. If only some sections
were ticked, just those move; the unfinished ones and the parent stay.
New dates are never in the past, and a multi-cycle gap is one reset.

**Two things worth knowing:** "ticked at the end of Sunday" is decided
by the tick's timestamp (`completed_at`), not by an actual end-of-day
snapshot; and a task you tick *on* Monday stays ticked all Monday and
moves on Tuesday. The edit form's checkbox now reads "Repeats — if
ticked, resets and moves on when it falls due".

**Tests:** the reset tests were rewritten around ticking a task on a
chosen day (`complete_on`), including the "Maths Assignment" shape in
both cases (all sections ticked by Sunday → all four move Monday; only
A and B ticked → only A and B move). all passed when this was written (456 after the later topic-list change, which removed the plan-ahead tests).

## Bug fix: Auto-assign skipped repeating subtasks (direct report)

**Report:** "Why are subtasks not assigned with the auto assign
feature?" **Cause:** Auto-assign planned subtasks like any other task —
except that it skipped every *repeating* task, and a repeating
parent's subtasks repeat too (the app's own rule, so a weekly multipart
task resets as a unit). So a weekly parent with sections never had any
of them placed (and the parent, being a container, is never placed
either): nothing at all. Ordinary, non-repeating subtasks were already
being assigned. **Fix:** repeating *subtasks* are now planned
(`is_flexible` in `queries/auto_assign.rs`). The old reason for skipping
repeating tasks — a reset moving the due date would strand a fixed
assignment — didn't hold, because the reset already clears
`assigned_date`/`auto_assigned` each cycle, so the next run plans the
new occurrence. Top-level repeating tasks are unchanged (still left on
their due day, counted against it). **Replaced test:**
`repeating_subtasks_are_left_alone` asserted the old behaviour; it is
replaced by tests for the new behaviour, for the plan clearing and
re-planning across a reset, and for top-level repeating tasks staying
put. **Worth checking:** if you saw ordinary (non-repeating) subtasks
unassigned, that is a different cause I couldn't reproduce — tell me the
setup (parent/child due dates, estimates, whether the parent is linked
to a session).

## Dashboard pages: full-screen sections, snap scrolling, momentary position bar (direct request)

The Dashboard's three sections (today / focus / stats) are now
**full-screen pages**. Other views are unchanged.

- **One page at a time.** Each page is exactly the size of the screen;
  only the current page is drawn (mid-slide, the one it is sliding to),
  so the next and previous pages are never visible.
- **Snap scrolling.** One scroll gesture moves one page, with a short
  slide. A trackpad's inertia tail is swallowed, so a flick can't skip
  pages. `PageDown`/`PageUp` also step. Scrolling inside a cell whose
  contents overflow (a long task list on a small window) scrolls that
  cell first and only turns the page once it hits its edge.
- **Items fill the page.** Each page is carved into a grid of panels
  that exactly tile it: *today* = greeting + quick add, streak and
  session cards, then tasks and schedule; *focus* = Timer and Pomodoro
  side by side with the Stopwatch beneath (or the first row alone while
  a session is running); *stats* = three headline figures, then the task
  heatmap beside hours-per-subject, then the weekly line chart. The bar
  and line charts stretch to their cell's width. The pie chart has its
  own *Time per subject* cell beside the bars (following the same
  Week/Month/Year selector) and is drawn as large as fits above its
  legend, centred; the heatmap and scroll wheels keep their natural size.
- **Momentary side indicator.** Three dashes down the right edge
  (top / middle / bottom page): the current page's dash is bright, and
  the highlight slides between dashes during a page change. It appears
  on scroll, key or hover, fades after about a second, and while visible
  pressing a dash jumps to that page.

**Interpretation calls worth checking:** the indicator is per page, not
per scroll distance as the old six-dash bar was; trackpad swipes need a
deliberate flick (threshold in `ui::pager::SCROLL_THRESHOLD`); the
heatmap and scroll wheels are not resized, and the pie moved out of the
bar chart's cell into one of its own.

**Verification:** this round was built and tested against a real
toolchain for the first time. `cargo build` (all three binaries) and
`cargo test` pass: 456 tests at the latest change (469 before the plan-ahead tests were removed), including the 437 that existed before
(so the earlier "never compiled" items are now compiled and tested) and
23 new ones. Of those, the pager and indicator arithmetic are pure unit
tests, and 12 render the real Dashboard headlessly through egui at three
window sizes (checking that the panels tile each page edge to edge, that
only the current page is drawn, that wheel input turns the page and
never skips one, and that an overflowing cell keeps the gesture). **Not
verified:** how it looks and feels on a real screen and trackpad — the
slide timing, gesture threshold and indicator styling are first guesses.
The toolchain used was Rust 1.91 with three egui dependencies locally
patched in a scratch copy (their MSRV is 1.95, which the sandbox
couldn't install); the project's own source was not changed for that, so
build with your normal toolchain.

## Dashboard re-fit: today / focus / stats, and a progress bar for a scrollbar (superseded by the pages above)

- **Three sections**, each opened by a small label and separated from
  the next by a full-width grey-white hairline (the palette's mid grey,
  lighter than card borders):
  - **today** — the welcome message and date, quick add, streak and
    current/next session, Today's Tasks and Today's Schedule;
  - **focus** — Timer, Pomodoro, Stopwatch (in that order; the old
    "Focus" card is now titled "Pomodoro" so it isn't confused with the
    section);
  - **stats** — the statistics panel (its own "Stats" title dropped, the
    section label names it).
- **Side progress bar instead of a scrollbar** (Dashboard only; other
  views keep the normal scrollbar). Modelled on item [1] of the
  reference image — a row of short dashes with the first few filled —
  rotated to run down the right edge: six short, square-cornered dashes,
  filled in the theme's bright text colour from the top as you scroll
  (the dash being passed fills partway), the rest in the hairline grey.
  Hidden when everything fits on screen. Mouse wheel and trackpad scroll
  as before; pressing or dragging on the bar seeks.

**Interpretation calls worth checking:** the dashes represent equal
slices of the scroll distance, not one per section; the bar is
non-colourful on purpose (the theme is monochrome, so no blue); "grey
white lines" became the palette's `text_muted` grey.

**Verification:** as before, the project can't be built here. The bar's
arithmetic (progress, seek offset, per-dash fill) is unit-tested and
passes in the standalone crate (149/149 overall); the drawing, section
layout and scroll-area wiring are uncompiled and have never been seen on
screen.

## Timer, and Focus time from the stopwatch (direct request)

- **Focus time now counts the stopwatch.** Finishing a Stopwatch run
  logs its time (rounded to the nearest minute; under 30 seconds is
  skipped) into the same log Pomodoros use, tagged by source, whether or
  not a subject is attached. "Focus time" on the Dashboard sums
  Pomodoro + stopwatch + timer (hover it to see that); "Pomodoros
  completed" still counts only real Pomodoros. Every existing row counts
  as a Pomodoro (migration v26). Only the free-standing Stopwatch is
  included — Revision Sessions started from a Calendar event are not.
- **Timer panel** on the Dashboard, between Focus and Stopwatch: a
  countdown for a length you pick. The picker is three wheels — hours,
  minutes, seconds — like the iOS Clock app but in the app's square,
  monochrome style: each wheel is a column of bordered square cells
  holding consecutive numbers, the selected value in the middle cell
  (heavier border, larger, brighter), neighbours fading away. Scroll the
  mouse wheel or trackpad over a wheel, drag it, or click any cell to
  jump to it; wheels wrap (59 → 00). Pick an optional subject, press
  Start Timer; while running it shows the Pomodoro's circular countdown
  with Pause/Resume and Cancel. It keeps running on other views, sends a
  notification on start and finish, and a *finished* timer is logged as
  focus time (and as subject time if a subject is picked, via a Calendar
  event, exactly like a Pomodoro). Cancelling logs nothing. Your last
  chosen length is remembered for the next run (until restart).

**Not done / worth knowing:** unlike the Pomodoro, the timer has no
full-screen focus overlay and no nav-bar readout; a running timer does
not survive closing the app; maximum length is 23:59:59.

**Verification:** still no toolchain for the full project (Rust 1.75
only). The timer state machine (15 tests), the wheel's pure helpers
(12), and the focus-source queries plus migration v26 (3) pass in the
standalone SQLite crate (138/138 overall). The `egui` code — the wheel
widget, Dashboard Timer panel, app wiring, stopwatch Finish hook — is
uncompiled; I checked the egui calls it uses against the 0.36.2 source,
but it has never been built or looked at on screen, so expect to
adjust sizes/spacing.

## Bug fix: repeating tasks stayed late forever (after Auto Assigning)

A task due every Monday was "always late the next Mondays." Cause: the
daily reset (`reset_due_repeating_tasks`) only re-dated *completed*
repeating tasks, so one that was missed once kept its old due date for
good. It now rolls *incomplete* repeating tasks forward the same way:
once due date + one interval has arrived (next Monday), the missed
occurrence is dropped and the task is due on the most recent scheduled
date, still incomplete — due today, not a week late. A task less than
one full interval overdue is left alone (that's still the current,
honestly late occurrence). Repeat-through-list tasks also move to their
next topic when rolled. The reset now also clears a stale
`assigned_date` (a day picked for the previous occurrence would keep
the new one off its own due day); this applies to completed tasks too.

Interpretation: "events" was read as repeating *tasks* (`repeat_weekly`),
since that's where a deadline is stored and where the reset lived;
Calendar events already recur by their own rules. Verified in the
standalone SQLite crate (95/95 passing, 8 new/rewritten reset tests).

## Auto Assigning (direct request, after the eighth pass)

A new **Auto-assign** button on the To Do view picks a day for every
task nobody has placed yet.

- **Deadline:** tasks are placed earliest-deadline first, each on the
  earliest day with room, and never after its deadline while a day up
  to it has room. If a deadline leaves nowhere, the deadline wins and
  the task goes over budget (the summary says so).
- **Daily budget:** a day holds up to 120 minutes of tasks by default.
  Settings has the budget, what a task with no estimate is assumed to
  take (30), and how many days ahead to plan (14).
- **Days already set:** tasks fixed to a day are placed first and use
  up that day's budget; everything else flows around them. That covers
  manual assignments, per-day topic tasks, and top-level repeating tasks
  due that day (repeating *subtasks* are planned like any other).
- **Topics per day:** tick "Plan each day's topic ahead" on a
  repeat-through-a-list task and Auto-assign creates one real task per
  scheduled day, each with its own topic (6 topics, daily = 6
  consecutive days), carrying the estimate and subject. The original
  becomes a template: kept in To Do (marked "plans topics ahead") but
  not in Today's Tasks, the Calendar or reminders, and it no longer
  cycles by itself.

On the Calendar's Day view, tasks Auto-assign placed on that day (its
own picks plus generated topic tasks) appear in a separate
**Auto-assigned** section under "Tasks for this day", with a done count
and total planned minutes; the original section keeps only tasks you
assigned by hand. The new section only shows when it has something.

Subtasks are planned like any other task — each gets its own day from
its own deadline and estimate, so a large multipart task can spread
across days. A parent with incomplete subtasks is a *container*: it is
not placed itself and doesn't count toward any day's budget (its
subtasks carry the work, so nothing is counted twice); once all its
subtasks are done it is planned like a normal task. A subtask with no due date of its own inherits its
parent's as its deadline (its own date wins if it has one), so the parts
of a multipart task are planned in time for the whole.

It never moves a task you assigned yourself, and skips completed
tasks, session-linked tasks and repeating tasks (including repeating
subtasks). Tasks it
placed are re-planned on each run.

**Interpretation calls worth checking:** "assigned to a task" was read
as "assigned to a day"; scheduled days for topics come from the task's
due date plus its Day/Week cadence; the planner is a button, not
automatic. Details in `docs/architecture.md`'s "Auto Assigning".

**Verification:** still no toolchain able to build the whole project
(Rust 1.75 only). The planner (`tasks::auto_assign`), the new database
layer (`queries::auto_assign`, the v25 migration, the task and settings
queries) were copied into a standalone crate with real SQLite and
**pass** (88/88, including 21 auto-assign and 3 settings tests). The
`egui` changes (To Do button/badges/checkbox, Settings fields, Dashboard
and Calendar filters, reminder filter) have **not** been compiled.

## Eighth `issues.md` pass — 2 feature requests shipped

- **Stats Screen "Week Over Week Change".** The Dashboard's
  week-over-week message now compares *this week so far* against the
  *same span of last week* — on a Thursday, Mon–Thu vs. last Mon–Thu —
  instead of this week's partial total vs. last week's full total, so
  a new week no longer opens with "down 90%". The message names the
  span it compared, e.g. "up 14% from the same point last week
  (Mon–Thu)". The line chart under it is unchanged (whole weeks).
- **Flashcards As A Grid View.** The Flashcards screen has a
  Grid / List / Mixed toggle. **Grid** (the default): several cards per
  row, each with title, card count, source and a preview of its first
  three questions. **List**: one compact row per set, title and buttons
  only, no preview. **Mixed**: the same compact rows, each with a
  Preview/Hide toggle that opens that set's full card in place. The
  choice isn't persisted (same as Notes' Grid/List).

**Interpretation calls worth checking:** "this Thursday vs. last
Thursday" was read as week-to-date (Mon–Thu vs. Mon–Thu), not a
single day vs. a single day; and "a mix of both" was read as the
expandable list described above. Both are small, isolated changes if
you meant something else. See `docs/architecture.md`'s "Eighth
`issues.md` pass" section.

**Verification:** no compiler capable of building this project was
available (the sandbox only has Rust 1.75; `eframe` 0.36 needs far
newer, and `static.rust-lang.org` is blocked). The new *pure* functions
and their tests (week-to-date bounds/label/message wording,
`grid_columns`, `truncate_chars`, `card_count_label`) were copied into
a standalone crate and pass (15/15). The `egui` rendering, the new
`list_cards_preview` query, and the DB-backed tests have **not** been
compiled — run `cargo build && cargo test`.

## Seventh `issues.md` pass — 1 issue fixed, 3 feature requests shipped

- **Enter closing a dialog while typing (issue).** The repeat-list
  Topics field (a multiline textarea, under "Tasks Repeating Through A
  List") now excludes itself from the task edit modal's Enter-to-save
  check — pressing Enter there inserts a newline, the same
  `has_focus()`-based fix `notes.rs`'s own Content field already used.
  Separately, and more generally: the app's normal-mode keyboard
  shortcuts (digit-jump between views, `n`/`t`/`d`/`w`/`m`, `/`, `i`,
  `?`) can no longer fire while *any* dialog is open — previously they
  were only suppressed while a text field specifically had focus,
  which didn't cover every moment inside a dialog (right after
  clicking a checkbox or a Save/Cancel button, for instance).
- **Colour.** Subjects now have a real, persisted color of their own —
  pick one from a color swatch next to each subject (and subtopic) in
  "Manage Subjects." What used to be the Improved Line Graph's own
  one-off fixed palette is now the *default* every subject starts
  with until explicitly recolored, and that same color now shows up
  everywhere a subject does: the Dashboard's bar chart, pie chart, and
  line graph, Calendar's Day/Week/Month views (an accent bar, the
  subject label, event lines, and small per-day dots respectively),
  and the Dashboard's Current/Next session and Today's Schedule cards.
- **Notifications Extended.** A desktop notification (via whichever
  backend — Hyprctl or Caelestia — is selected in Settings) now fires
  the moment something actually starts: clicking Start/Resume on the
  Pomodoro, starting the Stopwatch, or starting a session from a
  Calendar event. This is new and separate from the pre-existing
  notifications that already fired when a Pomodoro *phase completes*.
- **Tasks Repeating By Day.** A repeating task can now be set to
  repeat daily instead of weekly — a small "Every: Day / Week" picker
  appears once "Repeats" is checked. Every task that already repeated
  before this request keeps its weekly cadence exactly as before (the
  new `repeat_interval_days` column defaults to `7`); picking "Day"
  sets it to `1`, and the reset logic (`reset_due_repeating_tasks`)
  now advances a task's due date by whatever its own interval is,
  rather than always by a week.

See `docs/architecture.md`'s "Seventh `issues.md` pass" section for
the full rationale on all four items. Not yet covered by a fresh
`cargo test` run — the assistant's own sandbox has no Rust toolchain
installed at all right now (see `HANDOFF.md`).

## Sixth `issues.md` pass — 4 of 4 requests shipped

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
sandbox can no longer compile this project at all — see
`HANDOFF.md`).

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
