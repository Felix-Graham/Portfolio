//! A lightweight liveness signal the GUI app leaves behind, so
//! `revise-reminderd` (feature request: "Background Notifications")
//! can tell whether the GUI is already running — and therefore
//! already responsible for reminders and startup commands — without
//! a real IPC mechanism. The only question that needs answering is
//! "has the GUI checked in recently", and a file's modified time
//! already answers that, so that's all this is: a file, touched
//! periodically, with nothing meaningful in its contents.
//!
//! Exactly one of {the GUI app, the daemon} is meant to be
//! responsible for reminders/startup commands at any given moment —
//! see `revise-reminderd`'s own doc comment for the other half of
//! this handshake.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// Comfortably longer than the GUI's own poll interval
/// (`notifications::CHECK_INTERVAL_SECONDS`, 20s), so one missed
/// frame — a slow redraw, the window being backgrounded, etc. —
/// doesn't make the daemon think the GUI has gone away when it
/// hasn't.
const FRESHNESS_WINDOW: Duration = Duration::from_secs(45);

fn heartbeat_path() -> PathBuf {
    // `$XDG_RUNTIME_DIR` is the correct place for this kind of
    // ephemeral, session-scoped marker (it's cleared on logout,
    // unlike a config or data directory) — falling back to the
    // system temp dir only if it isn't set, e.g. in this sandbox.
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("revise-gui.heartbeat")
}

/// Called by the GUI app each time it actually runs a reminder scan
/// (see `AppState::check_reminders`) — not gated on any setting,
/// since the daemon needs to know the GUI is alive regardless of
/// whether reminders themselves happen to be turned on right now.
pub fn touch() {
    let _ = fs::write(heartbeat_path(), []);
}

/// Called by `revise-reminderd` before it runs its own scan.
pub fn gui_is_alive() -> bool {
    let Ok(metadata) = fs::metadata(heartbeat_path()) else { return false };
    let Ok(modified) = metadata.modified() else { return false };
    SystemTime::now().duration_since(modified).map(|age| age < FRESHNESS_WINDOW).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Both assertions share one test function rather than being split
    // across two `#[test]`s: they read/write the same on-disk path,
    // and cargo runs tests in parallel by default, so two separate
    // tests touching/removing that file concurrently would be
    // genuinely racy.
    #[test]
    fn heartbeat_freshness_reflects_whether_its_been_touched_recently() {
        let _ = fs::remove_file(heartbeat_path());
        assert!(!gui_is_alive());

        touch();
        assert!(gui_is_alive());
    }
}
