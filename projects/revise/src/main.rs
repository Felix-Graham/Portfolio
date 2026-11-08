use revise::app::ReviseApp;

fn main() -> eframe::Result<()> {
    // Installs a `log`-crate subscriber so `RUST_LOG=debug` actually
    // shows something — `eframe`/`glutin`'s own detailed
    // backend-selection tracing (which display/config backend was
    // tried, what it found) was previously going nowhere, since
    // nothing was consuming it. Genuinely useful for diagnosing
    // graphics-backend issues like `NoGlutinConfigs` on a specific
    // machine, and otherwise silent/inert by default (no output
    // unless `RUST_LOG` is set).
    env_logger::init();

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Revise")
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([640.0, 420.0]),
        // No `depth_buffer` setting here anymore — that was a
        // `glow`/`glutin` (OpenGL/EGL)-specific workaround for a
        // `NoGlutinConfigs` startup crash on a real NVIDIA + Hyprland
        // + Wayland machine (glutin's own EGL config matching
        // rejected a literal `depth_size: 0`, egui's own default).
        // Confirmed on that same machine: a from-scratch app using
        // eframe's current *default* backend — `wgpu`, not
        // `glow`/`glutin` at all — opens a window with no such issue
        // (see `diagnostics/eframe-wgpu-smoke-test/`), which is why
        // this app is migrating to it rather than continuing to work
        // around `glutin`. `NativeOptions` has no `depth_buffer` field
        // to set even if it were still wanted — that concept doesn't
        // apply to `wgpu`'s own surface configuration.
        ..Default::default()
    };

    eframe::run_native("revise", native_options, Box::new(|cc| Ok(Box::new(ReviseApp::new(cc)))))
}
