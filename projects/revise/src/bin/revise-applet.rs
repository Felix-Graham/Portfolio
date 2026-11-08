//! `revise-applet` — feature request: "a separate applet... a smaller unit
//! for the stopwatch and timer section", following the main app's theme,
//! launched from its own `.desktop` file, and logging through Revise's "API".
//!
//! Like `revise-reminderd` and `revise-cli` it is a thin binary over the
//! `revise` library crate: it opens the same SQLite file as the main app and
//! shares its theme and timers. All the logic and drawing live in
//! `revise::applet`, where `cargo test` reaches them.

use revise::applet::ui::{AppletApp, MIN_WINDOW_SIZE, WINDOW_SIZE};

fn main() -> eframe::Result<()> {
    // `RUST_LOG=debug` shows eframe's backend selection, as for `revise`.
    env_logger::init();

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Revise Timer")
            // The Wayland app id / X11 class: what `revise-applet.desktop`'s
            // `StartupWMClass` names, and what a Hyprland window rule matches
            // (e.g. `windowrulev2 = float, class:^(revise-applet)$`).
            .with_app_id("revise-applet")
            .with_inner_size(WINDOW_SIZE)
            .with_min_inner_size(MIN_WINDOW_SIZE),
        ..Default::default()
    };

    eframe::run_native("revise-applet", native_options, Box::new(|cc| Ok(Box::new(AppletApp::new(cc)))))
}
