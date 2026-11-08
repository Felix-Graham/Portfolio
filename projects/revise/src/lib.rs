//! Revise's library crate. Split out from a plain binary crate
//! specifically so `revise-reminderd` (feature request: "Background
//! Notifications", in `src/bin/`) can reuse `database`,
//! `notifications`, and `gui_heartbeat` directly — no reminder or
//! notification logic is duplicated between the GUI app and the
//! daemon; both call the exact same `notifications::check_reminders`.
//! `revise-cli` (direct request: "an API... callable from other
//! applications and through the terminal from anywhere") reuses the
//! same split for the same reason — `cli::run` is real, tested logic
//! that both the actual `revise-cli` binary and `cli`'s own unit tests
//! call, rather than command-line parsing and DB writes living
//! untested inside a `fn main()` only `cargo run --bin revise-cli`
//! could ever exercise.

pub mod app;
pub mod applet;
pub mod cli;
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
pub mod sources;
pub mod stats;
pub mod subjects;
pub mod tasks;
pub mod timer;
pub mod theme;
pub mod ui;
