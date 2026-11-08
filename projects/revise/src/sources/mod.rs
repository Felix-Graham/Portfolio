//! Direct request: "an API... callable from other applications and
//! through the terminal from anywhere... other revision applications
//! (my own french quiz built in Python for example) to be registered
//! and accounted for."
//!
//! No network server, no auth token — this is a single-user local
//! app, and `revise-reminderd` (feature request: "Background
//! Notifications") already proves out the actual mechanism that
//! matters here: a second, GUI-less binary opening the exact same
//! SQLite file directly. `revise-cli` (`src/bin/revise-cli.rs`, logic
//! in `crate::cli`) is that same idea applied to writes instead of
//! reads — `register` inserts a `Source`, `log` inserts an
//! `ExternalContribution` against one. Every view in this app already
//! queries the database fresh on each render, so a contribution
//! written by an outside process shows up in the Dashboard's charts on
//! its very next repaint, with nothing needed in between.

pub mod contribution;
pub mod source;

pub use contribution::ExternalContribution;
pub use source::Source;
