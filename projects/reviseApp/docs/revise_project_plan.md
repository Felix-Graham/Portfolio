# Revise --- Agile Project Plan

## 1. Project Overview

**Revise** is a lightweight, native Linux revision-planning application
written in Rust and designed primarily for NixOS and Hyprland users.

The application combines:

-   A daily revision dashboard
-   A calendar and revision schedule
-   A task/to-do system
-   Notes
-   Revision sessions containing tasks
-   Recurring events
-   Lightweight progress tracking
-   Linux/Wayland notifications
-   Keyboard-first interaction

The application should feel like a **daily revision workspace**, rather
than a generic calendar or productivity application.

The central idea is:

> **The Dashboard tells the user what their day looks like. The Calendar
> tells them when. Tasks tell them what needs doing. Notes hold
> supporting information.**

The visual direction is minimalist and monochromatic, inspired by the
design language of Nothing OS and Nothing's web design: black and white,
thin borders, generous whitespace, geometric/technical typography,
restrained animation, and very little visual clutter.

------------------------------------------------------------------------

## 2. Project Goals

### Primary goals

1.  Build a fast native Linux application using Rust.
2.  Make the application pleasant to use every day for revision.
3.  Make scheduling revision sessions extremely quick.
4.  Connect calendar events and tasks into one coherent system.
5.  Provide a useful dashboard that answers "What should I do today?"
6.  Support recurring revision sessions.
7.  Integrate naturally with NixOS and Hyprland.
8.  Keep all core data local and usable offline.
9.  Establish a strong, recognisable minimalist visual identity.

### Secondary goals

-   Keyboard-first operation
-   Notifications
-   Revision progress statistics
-   Search
-   Focus/session mode
-   Nix/Home Manager packaging
-   Import/export and backups

### Non-goals for the initial release

The project should avoid becoming:

-   A full Google Calendar replacement
-   A collaboration platform
-   A cloud-dependent application
-   A complicated project-management system
-   A highly gamified productivity application
-   A feature-heavy note-taking application

The initial product should remain focused on **personal revision
planning**.

------------------------------------------------------------------------

# 3. Product Model

The application should revolve around four primary views.

``` text
┌─────────────┬─────────────┬─────────────┬─────────────┐
│  Dashboard  │  Calendar   │  To Do List │    Notes    │
└─────────────┴─────────────┴─────────────┴─────────────┘
```

## Dashboard

The home screen and primary daily view.

It should show:

-   Greeting
-   Current date
-   Current/next revision session
-   Today's schedule
-   Outstanding tasks
-   Recently completed tasks
-   Revision streak
-   Lightweight progress information

The dashboard should answer:

> **What do I need to do today?**

## Calendar

A visual representation of scheduled revision sessions and events.

Initial views:

-   Day
-   Week
-   Month

The day view should prioritise a vertical timeline.

## To Do List

A dedicated task-management view.

Tasks should support:

-   Creation
-   Completion
-   Editing
-   Deletion
-   Due dates
-   Priority
-   Association with a revision session
-   Optional subject/category

## Notes

A lightweight notes system for revision-related information.

The first version can remain intentionally simple:

-   Create note
-   Edit note
-   Delete note
-   Search notes
-   Associate notes with subjects or revision sessions

------------------------------------------------------------------------

# 4. Core Domain Model

The application should treat a **revision session** as the central
object connecting scheduling and tasks.

``` text
Day
 └── Revision Session
      ├── Subject
      ├── Time
      ├── Duration
      ├── Tasks
      │    ├── Task
      │    ├── Task
      │    └── Task
      └── Notes
```

This allows the same information to appear in multiple views without
duplicating data.

For example:

``` text
Calendar:
14:00 — Revision CS

Dashboard:
14:00 — Revision CS
2 tasks remaining

To Do:
□ Review networking
□ Complete past-paper questions
```

Completing the task from any of these views should update the same
underlying task.

------------------------------------------------------------------------

# 5. Technical Direction

## Language

**Rust**

Rust should be used for:

-   Application logic
-   Data model
-   Persistence
-   Calendar calculations
-   Recurrence calculations
-   Notifications
-   Configuration
-   Search
-   UI integration

## GUI

The project should evaluate:

-   Slint
-   Iced
-   egui/eframe
-   GTK4 + gtk-rs

The first sprint should produce a minimal prototype with the preferred
framework before committing heavily to the final UI architecture.

The primary criteria should be:

-   Wayland support
-   Performance
-   Ease of custom UI design
-   Keyboard input
-   Animation support
-   Maintainability
-   NixOS compatibility

## Database

Use a local SQLite database.

Suggested location:

``` text
~/.local/share/revise/revise.db
```

Configuration can initially live in:

``` text
~/.config/revise/config.toml
```

## Packaging

Use Nix Flakes from the beginning.

The repository should eventually provide:

``` text
flake.nix
├── package
├── devShell
└── formatter
```

The application should ultimately be usable through NixOS/Home Manager.

------------------------------------------------------------------------

# 6. Agile Development Method

Development will use short, focused sprints.

Each sprint should have:

1.  **Sprint Goal**
2.  **Necessary Features**
3.  **Possible Executions**
4.  **Acceptance Criteria**
5.  **Review**
6.  **Potential Backlog Items**

A sprint should ideally produce something demonstrable, even if the
feature is not production-ready.

The project should favour **vertical slices** over implementing an
entire technical layer before anything works.

For example, instead of:

``` text
Database → Calendar engine → UI → Tasks
```

prefer:

``` text
Create event → Save event → Display event → Edit event
```

This allows the product to become usable progressively.

------------------------------------------------------------------------

# 7. Sprint 0 --- Project Discovery & Technical Spike

## Goal

Establish the project structure, development environment, visual
direction, and GUI framework.

## Necessary Features

-   Git repository
-   Rust project
-   Nix Flake
-   Development shell
-   Formatter/linter configuration
-   Basic application window
-   Initial UI framework evaluation
-   Initial architecture documentation

## Possible Execution

Create:

``` text
revise/
├── Cargo.toml
├── flake.nix
├── flake.lock
├── README.md
├── LICENSE
├── docs/
└── src/
```

Evaluate at least two GUI frameworks by implementing the same small
prototype:

``` text
┌─────────────────────────────────────┐
│ Dashboard   Calendar   Tasks Notes  │
├─────────────────────────────────────┤
│                                     │
│       GOOD MORNING                  │
│                                     │
│       23 AUGUST                     │
│                                     │
└─────────────────────────────────────┘
```

## Acceptance Criteria

-   `nix develop` works.
-   `cargo build` works inside the development shell.
-   Application launches on NixOS.
-   GUI framework decision is documented.
-   Basic navigation prototype exists.
-   Project architecture is documented.

## Deliverable

A running empty Revise application with the intended high-level layout.

------------------------------------------------------------------------

# 8. Sprint 1 --- Application Shell & Visual Foundation

## Goal

Build the application's permanent shell and establish the design system.

## Necessary Features

-   Main application window
-   Four navigation sections
-   Dashboard shell
-   Calendar shell
-   To Do shell
-   Notes shell
-   Typography system
-   Border system
-   Spacing system
-   Basic keyboard navigation

## Visual Direction

The initial design should establish:

-   Black/white palette
-   Thin borders
-   Minimal rounded corners or square corners
-   Generous whitespace
-   Geometric/monospace typography
-   Minimal iconography
-   Restrained animations

The UI should feel technical without becoming a terminal emulator.

## Possible Execution

Create reusable components:

``` text
AppShell
NavigationBar
Panel
Button
Input
Checkbox
Timeline
EventCard
TaskItem
Modal
```

## Acceptance Criteria

-   All four views can be opened.
-   Navigation feels consistent.
-   UI components share a coherent visual system.
-   Keyboard focus is visible.
-   Window works correctly under Hyprland.

------------------------------------------------------------------------

# 9. Sprint 2 --- Tasks / To Do System

## Goal

Implement the first persistent user data.

## Necessary Features

-   Create task
-   Edit task
-   Delete task
-   Complete task
-   Uncomplete task
-   Task persistence
-   Task ordering
-   Optional due date

## Data Model

Initial task:

``` text
Task
├── id
├── title
├── description
├── completed
├── created_at
└── due_date
```

## Possible Execution

Build SQLite persistence before making the UI complex.

Example workflow:

``` text
Create Task
    ↓
Validate
    ↓
Insert into SQLite
    ↓
Update application state
    ↓
Render TaskItem
```

## Acceptance Criteria

-   A task can be created from the UI.
-   Restarting the application does not lose tasks.
-   Tasks can be completed.
-   Completed tasks have a clear visual state.
-   Editing and deletion work.
-   Database migrations are established.

------------------------------------------------------------------------

# 10. Sprint 3 --- Revision Sessions & Calendar Events

## Goal

Introduce the central concept of a scheduled revision session.

## Necessary Features

-   Create event
-   Edit event
-   Delete event
-   Start date/time
-   End date/time or duration
-   Title
-   Description
-   Subject/category
-   Event persistence
-   Day timeline

## Data Model

``` text
Event
├── id
├── title
├── description
├── start_time
├── end_time
├── subject_id?
└── recurrence_id?
```

## Possible Execution

Create a simple day timeline:

``` text
08:00
│
├── Mathematics
│   08:00–09:00
│
10:00
│
├── Revision CS
│   10:00–11:00
│
12:00
│
└────────────────
```

Events should be clickable and editable.

## Acceptance Criteria

-   Events can be created.
-   Events appear at the correct time.
-   Events persist across application restarts.
-   Events can be edited/deleted.
-   The dashboard can display today's events.
-   The calendar correctly handles events crossing normal hour
    boundaries.

------------------------------------------------------------------------

# 11. Sprint 4 --- Connect Tasks to Revision Sessions

## Goal

Turn the calendar and task manager into one system.

## Necessary Features

-   Attach tasks to events
-   Display event tasks
-   Complete event tasks
-   Show task progress on events
-   Display event task progress on Dashboard

## Example

``` text
REVISION CS
10:00–11:00

□ Review networking
□ Complete questions
✓ Review flashcards

2 / 3 complete
```

The same task should appear in:

-   Dashboard
-   Calendar event
-   To Do List

## Acceptance Criteria

-   A task can belong to a revision session.
-   Completing a task from any view updates all views.
-   Events display completion progress.
-   Tasks can exist independently of events.

------------------------------------------------------------------------

# 12. Sprint 5 --- Dashboard

## Goal

Make the Dashboard the central daily experience.

## Necessary Features

-   Greeting
-   Date
-   Today's events
-   Today's tasks
-   Next revision session
-   Task completion summary
-   Basic streak
-   Quick task creation
-   Quick event creation

## Proposed Layout

``` text
┌───────────────────────────────────────────────────────┐
│ Dashboard   Calendar   To Do List   Notes             │
├───────────────────────────────────────────────────────┤
│                                                       │
│ GOOD MORNING                                          │
│                                                       │
│ 23 AUGUST                              MONDAY         │
│                                                       │
│ ┌────────────────────────────┐ ┌───────────────────┐ │
│ │ TASKS                      │ │ TODAY             │ │
│ │                            │ │                   │ │
│ │ □ Integration questions   │ │ 08:00             │ │
│ │ □ Physics flashcards      │ │ BLOCK A           │ │
│ │ ✓ Formula review           │ │                   │ │
│ │                            │ │ 10:00             │ │
│ │                            │ │ REVISION CS       │ │
│ └────────────────────────────┘ └───────────────────┘ │
│                                                       │
└───────────────────────────────────────────────────────┘
```

## Acceptance Criteria

When the application launches, the user should be able to understand:

-   What day it is.
-   What they need to do.
-   What revision sessions are scheduled.
-   What is currently in progress.
-   What remains unfinished.

The Dashboard should require little or no configuration before becoming
useful.

------------------------------------------------------------------------

# 13. Sprint 6 --- Recurring Events

## Goal

Support repeatable revision schedules.

## Necessary Features

-   Never repeat
-   Daily recurrence
-   Every N days
-   Weekly recurrence
-   Every N weeks
-   Selected weekdays
-   Monthly recurrence
-   Optional end date
-   Optional occurrence limit

## Example

``` text
Every 1 week

☐ Monday
☑ Tuesday
☐ Wednesday
☑ Thursday
☐ Friday
☐ Saturday
☐ Sunday
```

## Possible Execution

Implement recurrence logic as a separate domain module.

Example:

``` text
calendar/
├── event.rs
├── recurrence.rs
└── occurrence.rs
```

The recurrence engine should generate occurrences without duplicating
the underlying event definition unnecessarily.

## Acceptance Criteria

-   Recurring events appear correctly in day/week/month views.
-   Editing recurrence rules behaves predictably.
-   Individual occurrences can be handled without corrupting the parent
    recurrence.
-   Recurrence calculations have automated tests.

------------------------------------------------------------------------

# 14. Sprint 7 --- Calendar Views

## Goal

Expand the calendar from a day timeline into a complete scheduling
interface.

## Necessary Features

-   Day view
-   Week view
-   Month view
-   Date navigation
-   Today button
-   Event creation from calendar
-   Drag/resize if practical

## Possible Execution

Start with:

``` text
DAY
```

then:

``` text
WEEK
```

then:

``` text
MONTH
```

Do not implement drag-and-drop until basic event manipulation is stable.

## Acceptance Criteria

-   User can navigate dates.
-   User can switch between views.
-   Events appear consistently in all views.
-   Creating an event from the calendar works.
-   Current date is visually clear.

------------------------------------------------------------------------

# 15. Sprint 8 --- Notes

## Goal

Introduce lightweight revision notes without turning the project into a
full note-taking application.

## Necessary Features

-   Create note
-   Edit note
-   Delete note
-   Note title
-   Note content
-   Search
-   Subject association
-   Optional event association

## Possible Execution

Start with plain text or simple Markdown.

Example:

``` text
ELECTROMAGNETISM

Magnetic field strength:
B = μ₀I / 2πr

Remember:
- Direction follows right-hand rule.
- Field decreases with distance.
```

## Acceptance Criteria

-   Notes persist locally.
-   Notes can be associated with a subject/session.
-   Notes can be searched.
-   Opening a revision session can show related notes.

------------------------------------------------------------------------

# 16. Sprint 9 --- Subjects & Revision Organisation

## Goal

Introduce subjects as a first-class organisational concept.

## Necessary Features

-   Create subject
-   Rename subject
-   Archive subject
-   Assign event to subject
-   Assign task to subject
-   Assign note to subject
-   Filter by subject

## Example

``` text
MATHEMATICS
PHYSICS
CHEMISTRY
COMPUTER SCIENCE
```

The visual interface should remain monochromatic. Subjects do not need
bright colours.

## Acceptance Criteria

-   Events, tasks and notes can share a subject.
-   Subject filtering works consistently.
-   Archived subjects remain available for historical data.

------------------------------------------------------------------------

# 17. Sprint 10 --- Notifications & Linux Integration

## Goal

Make Revise behave like a native Linux desktop application.

## Necessary Features

-   Session reminders
-   Task reminders if required
-   Notification configuration
-   Startup configuration
-   Wayland compatibility
-   Hyprland testing

## Possible Execution

Example notification:

``` text
REVISION SESSION

Physics starts in 10 minutes.

Electromagnetism
16:00–17:00
```

Potential startup configuration:

``` ini
exec-once = revise
```

The application should not assume that it owns the entire desktop.

## Acceptance Criteria

-   Notifications appear through the desktop notification system.
-   Application behaves correctly under Hyprland.
-   Application can launch at startup.
-   Notifications can be disabled.
-   No background process is required when the application is closed
    unless explicitly needed.

------------------------------------------------------------------------

# 18. Sprint 11 --- Keyboard-First Interaction

## Goal

Make the application exceptionally efficient for keyboard-oriented Linux
users.

## Necessary Features

Suggested shortcuts:

``` text
N       New event
T       New task
D       Today
W       Week
M       Month
← →     Previous / next day
SPACE   Complete task
ENTER   Open selected item
ESC     Close modal
/       Search
```

A command palette can eventually support:

``` text
> new task
> new session
> go to today
> search integration
> open calendar
```

## Acceptance Criteria

-   Core workflows can be completed without a mouse.
-   Shortcuts are discoverable.
-   Keyboard focus is clear.
-   Shortcuts do not conflict with normal text input.

------------------------------------------------------------------------

# 19. Sprint 12 --- Revision Session / Focus Mode

## Goal

Add an optional focused mode for actually completing a revision session.

## Necessary Features

-   Start session
-   Session timer
-   Display session tasks
-   Complete tasks
-   Finish session
-   Record actual duration

## Example

``` text
────────────────────────────────

          MATHEMATICS

           INTEGRATION

             47:32

────────────────────────────────

          2 / 3 TASKS

────────────────────────────────
```

## Acceptance Criteria

-   A session can be started from an event.
-   Timer is accurate.
-   Tasks remain accessible.
-   Completing a session records useful history.
-   Closing the application does not corrupt session state.

------------------------------------------------------------------------

# 20. Sprint 13 --- Progress & Statistics

## Goal

Provide useful revision feedback without excessive gamification.

## Necessary Features

-   Revision time
-   Completed sessions
-   Planned sessions
-   Tasks completed
-   Subject breakdown
-   Current streak
-   Weekly activity

## Example

``` text
THIS WEEK

MON   ███████
TUE   █████
WED   █████████
THU   ███
FRI   ████████

TOTAL

12h 40m
18 sessions
42 tasks
```

## Acceptance Criteria

-   Statistics are based on actual stored data.
-   Time calculations are reliable.
-   Empty states are handled gracefully.
-   Statistics do not overwhelm the Dashboard.

------------------------------------------------------------------------

# 21. Sprint 14 --- Search & Command Palette

## Goal

Make accumulated revision history easy to navigate.

## Necessary Features

-   Global search
-   Search tasks
-   Search events
-   Search notes
-   Search subjects
-   Command palette

## Possible Execution

Press `/`:

``` text
┌──────────────────────────────────────────┐
│ > integration                            │
├──────────────────────────────────────────┤
│ EVENT    Mathematics — Integration       │
│ TASK     Integration questions           │
│ NOTE     Integration formulas             │
└──────────────────────────────────────────┘
```

## Acceptance Criteria

-   Search returns relevant results quickly.
-   Results link to the correct view.
-   Search works entirely offline.

------------------------------------------------------------------------

# 22. Sprint 15 --- Data Safety & Import/Export

## Goal

Ensure users can safely retain and move their revision data.

## Necessary Features

-   Database backup
-   Manual export
-   Import
-   JSON or similar portable format
-   Database integrity checks

## Possible Execution

Provide:

``` text
revise export backup.json
revise import backup.json
```

or equivalent GUI functionality.

## Acceptance Criteria

-   A complete backup can be exported.
-   A backup can be restored.
-   Import errors do not destroy existing data.
-   Database migrations are versioned.

------------------------------------------------------------------------

# 23. Sprint 16 --- NixOS / Home Manager Packaging

## Goal

Make Revise easy to install and configure in a Nix environment.

## Necessary Features

-   Nix package
-   Flake output
-   Development shell
-   Optional Home Manager module
-   Desktop entry
-   Application icon
-   Configuration documentation

Potential interface:

``` nix
programs.revise = {
  enable = true;

  settings = {
    default-view = "dashboard";
    notifications = true;
    startup = true;
  };
};
```

## Acceptance Criteria

-   Package can be built reproducibly.
-   Application can be installed through Nix.
-   Desktop entry works.
-   Startup configuration is documented.
-   Configuration survives upgrades.

------------------------------------------------------------------------

# 24. Sprint 17 --- UI Polish & Nothing-Inspired Design Pass

## Goal

Bring the application from functional prototype to cohesive product.

## Necessary Features

-   Typography refinement
-   Spacing refinement
-   Borders
-   Icons
-   Transitions
-   Hover states
-   Focus states
-   Empty states
-   Loading states
-   Error states
-   Responsive sizing

## Design principles

### Monochrome

Primarily:

``` text
BLACK
WHITE
GREY
```

Use colour only when it communicates something important.

### Borders

Thin rectangular borders should define the interface.

### Whitespace

Do not fill every available area.

### Typography

Large technical typography for dates and headings.

Smaller monospace or geometric typography for metadata.

### Animation

Animations should be:

-   Fast
-   Subtle
-   Functional

Avoid decorative animation.

## Acceptance Criteria

The application should visually resemble a coherent product rather than
a collection of UI components.

------------------------------------------------------------------------

# 25. Sprint 18 --- Performance, Stability & Refactoring

## Goal

Prepare the application for release.

## Necessary Features

-   Performance profiling
-   Memory checks
-   Database query optimisation
-   Startup optimisation
-   Error handling audit
-   Logging
-   Crash resilience
-   Automated tests
-   Code cleanup

## Performance targets

The application should:

-   Launch quickly.
-   Remain responsive with thousands of tasks/events.
-   Avoid unnecessary background work.
-   Avoid excessive memory usage.
-   Perform calendar calculations without blocking the UI.

## Acceptance Criteria

-   No known critical crashes.
-   Database operations are tested.
-   Recurrence calculations are tested.
-   Application remains responsive with a large dataset.
-   Errors are recoverable where possible.

------------------------------------------------------------------------

# 26. Sprint 19 --- Release Candidate

## Goal

Create a complete, stable first release.

## Necessary Features

-   Final documentation
-   Installation instructions
-   NixOS instructions
-   Hyprland instructions
-   Keyboard shortcut reference
-   Configuration documentation
-   Backup documentation
-   Screenshots
-   Release notes

## Testing

Test:

-   Fresh installation
-   Existing database
-   Empty database
-   Large database
-   Recurring events
-   Event deletion
-   Event modification
-   Task completion
-   Application restart
-   System reboot
-   Notification behaviour
-   Hyprland startup
-   Different window sizes

## Acceptance Criteria

A new NixOS/Hyprland user should be able to install Revise, create a
revision schedule, restart their computer, and continue using it without
manual recovery or configuration.

------------------------------------------------------------------------

# 27. Post-MVP Backlog

Features that should remain outside the initial critical path:

-   Calendar sync
-   Google Calendar integration
-   iCalendar import/export
-   Mobile companion
-   Cloud synchronisation
-   Multi-device synchronisation
-   Shared calendars
-   Advanced Markdown editor
-   Flashcard integration
-   Spaced repetition
-   AI-assisted planning
-   Advanced analytics
-   Custom themes
-   Plugin system

These should only be considered after the core workflow is stable.

------------------------------------------------------------------------

# 28. Testing Strategy

Testing should happen continuously rather than being left until the
final sprint.

## Unit tests

Prioritise pure domain logic:

-   Recurrence calculations
-   Date calculations
-   Event duration
-   Task state
-   Streak calculation
-   Progress calculation

Example:

``` text
Given:
  Event starts Monday
  Repeat every 2 weeks

Expected:
  Monday week 1
  Monday week 3
  Monday week 5
```

## Integration tests

Test:

-   Database creation
-   Database migrations
-   CRUD operations
-   Event/task relationships
-   Import/export

## UI tests

Where practical:

-   Navigation
-   Creating tasks
-   Completing tasks
-   Creating events
-   Editing events
-   Keyboard shortcuts

------------------------------------------------------------------------

# 29. Suggested Repository Structure

``` text
revise/
├── Cargo.toml
├── Cargo.lock
├── flake.nix
├── flake.lock
├── README.md
├── LICENSE
│
├── docs/
│   ├── architecture.md
│   ├── design.md
│   ├── database.md
│   └── roadmap.md
│
├── migrations/
│
├── assets/
│   ├── fonts/
│   └── icons/
│
└── src/
    ├── main.rs
    │
    ├── app/
    │   ├── mod.rs
    │   ├── state.rs
    │   └── commands.rs
    │
    ├── calendar/
    │   ├── mod.rs
    │   ├── event.rs
    │   ├── recurrence.rs
    │   └── timeline.rs
    │
    ├── tasks/
    │   ├── mod.rs
    │   └── task.rs
    │
    ├── notes/
    │   ├── mod.rs
    │   └── note.rs
    │
    ├── subjects/
    │   ├── mod.rs
    │   └── subject.rs
    │
    ├── database/
    │   ├── mod.rs
    │   ├── migrations.rs
    │   └── queries.rs
    │
    ├── notifications/
    │   └── mod.rs
    │
    ├── statistics/
    │   └── mod.rs
    │
    ├── config/
    │   └── mod.rs
    │
    └── ui/
        ├── mod.rs
        ├── dashboard.rs
        ├── calendar.rs
        ├── tasks.rs
        ├── notes.rs
        ├── components/
        └── theme.rs
```

The exact structure can change as implementation progresses. The
important architectural boundary is between **domain logic**,
**persistence**, and **UI**.

------------------------------------------------------------------------

# 30. Definition of Done

A feature should not be considered complete merely because it compiles.

A sprint feature is "Done" when:

-   It works from the user interface.
-   Data persists correctly where appropriate.
-   Errors are handled.
-   Relevant tests exist.
-   The feature works under Wayland/Hyprland.
-   Keyboard interaction has been considered.
-   The visual design is consistent.
-   Existing features have not regressed.
-   The implementation is documented where necessary.

------------------------------------------------------------------------

# 31. Product Milestones

## Milestone 1 --- Prototype

**Sprints 0--2**

Result:

-   Running Rust application
-   Nix development environment
-   UI shell
-   Persistent tasks

## Milestone 2 --- Core Product

**Sprints 3--5**

Result:

-   Calendar
-   Revision sessions
-   Connected tasks
-   Functional Dashboard

At this point Revise should already be usable for basic personal
revision planning.

## Milestone 3 --- Scheduling

**Sprints 6--9**

Result:

-   Recurring events
-   Week/month calendar
-   Notes
-   Subjects

## Milestone 4 --- Linux Integration

**Sprints 10--12**

Result:

-   Notifications
-   Hyprland integration
-   Keyboard-first operation
-   Focus mode

## Milestone 5 --- Productivity & Polish

**Sprints 13--17**

Result:

-   Statistics
-   Search
-   Command palette
-   Import/export
-   NixOS packaging
-   Final visual design

## Milestone 6 --- Release

**Sprints 18--19**

Result:

-   Stable
-   Tested
-   Packaged
-   Documented
-   Ready for daily use

------------------------------------------------------------------------

# 32. Initial Backlog

The first backlog should be kept deliberately small.

### Must Have

-   [ ] Rust application
-   [ ] Nix Flake
-   [ ] Main window
-   [ ] Dashboard
-   [ ] Calendar
-   [ ] To Do List
-   [ ] Notes
-   [ ] SQLite persistence
-   [ ] Tasks
-   [ ] Revision sessions
-   [ ] Event/task relationship
-   [ ] Day timeline
-   [ ] Recurring events
-   [ ] Basic notifications
-   [ ] Hyprland compatibility

### Should Have

-   [ ] Week calendar
-   [ ] Month calendar
-   [ ] Subjects
-   [ ] Keyboard shortcuts
-   [ ] Command palette
-   [ ] Search
-   [ ] Revision statistics
-   [ ] Focus mode
-   [ ] Import/export
-   [ ] Home Manager module

### Could Have

-   [ ] iCalendar support
-   [ ] Calendar synchronisation
-   [ ] Flashcard integration
-   [ ] Spaced repetition
-   [ ] Advanced Markdown
-   [ ] Custom themes
-   [ ] Plugins

------------------------------------------------------------------------

# 33. Guiding Principle

The project should always prioritise this question:

> **Does this help the user understand what they need to revise and
> actually get it done?**

If a feature adds complexity without improving that workflow, it should
probably remain in the backlog.

The ideal Revise experience is:

``` text
Open laptop
      ↓
Revise launches
      ↓
GOOD MORNING
      ↓
See today's sessions
      ↓
See outstanding tasks
      ↓
Start revision
      ↓
Complete tasks
      ↓
Finish session
      ↓
Progress is recorded
```

The application should feel like a **quiet, persistent revision
companion** rather than a productivity suite.
