//! `revise-reminderd` — feature request: "Background Notifications".
//!
//! A minimal, GUI-less daemon that keeps polling for due
//! reminders/startup commands after the main Revise app has been
//! closed. Deliberately thin: every bit of actual reminder logic
//! (`should_remind_for_event`, `todays_event_starts`, sending via
//! `hyprctl notify`, running an event's startup command) lives in
//! `revise::notifications` and is shared verbatim with the GUI app —
//! this binary is just a different, non-`eframe` `main()` wrapped
//! around the same `notifications::check_reminders` the app already
//! polls with on every frame.
//!
//! Exactly one of {the GUI app, this daemon} is meant to be
//! responsible for reminders/startup commands at any given moment —
//! running both at once around the same due moment risks a duplicate
//! notification or, worse, a startup command running twice. The GUI
//! leaves a small heartbeat file behind each time it scans
//! (`revise::gui_heartbeat`); this daemon checks it before every scan
//! of its own and defers to the GUI whenever it looks like the GUI is
//! already running.

use std::time::Duration;

use revise::database::Database;
use revise::gui_heartbeat;
use revise::notifications::{self, NotificationScanState};

/// Coarser than the GUI's own poll interval (20s,
/// `notifications::CHECK_INTERVAL_SECONDS`) — a background daemon has
/// no redraw loop already running for free, so every wakeup is a
/// deliberate cost, and reminders/startup commands only need
/// minute-level precision anyway.
const POLL_INTERVAL: Duration = Duration::from_secs(60);

fn main() {
    let db = match Database::open_default() {
        Ok(db) => db,
        Err(err) => {
            eprintln!("revise-reminderd: could not open the Revise database ({err}), exiting");
            std::process::exit(1);
        }
    };

    // A fresh `NotificationScanState` means this process's own
    // internal 20-second scan throttle starts cold — harmless: the
    // first scan after startup just runs immediately instead of
    // waiting out a throttle window nothing has actually elapsed on
    // yet.
    let mut state = NotificationScanState::default();

    eprintln!("revise-reminderd: watching for due reminders and startup commands (Ctrl+C to stop)");

    loop {
        if !gui_heartbeat::gui_is_alive() {
            notifications::check_reminders(&db, &mut state);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}
