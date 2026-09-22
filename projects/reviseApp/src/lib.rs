//! Revise's library crate. Split out from a plain binary crate
//! specifically so `revise-reminderd` (feature request: "Background
//! Notifications", in `src/bin/`) can reuse `database`,
//! `notifications`, and `gui_heartbeat` directly — no reminder or
//! notification logic is duplicated between the GUI app and the
//! daemon; both call the exact same `notifications::check_reminders`.

pub mod app;
pub mod database;
pub mod events;
pub mod flashcards;
pub mod folders;
pub mod gui_heartbeat;
pub mod natural_date;
pub mod notes;
pub mod notifications;
pub mod pomodoro;
pub mod revision_session;
pub mod stats;
pub mod subjects;
pub mod tasks;
pub mod theme;
pub mod ui;
