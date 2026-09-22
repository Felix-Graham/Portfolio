use eframe::egui::{self, RichText};

use crate::database::{queries, Database};
use crate::theme::{Spacing, Theme};
use crate::ui::components::{Input, Modal, RButton};

/// State for the global "Settings" modal. Notification preferences
/// are read fresh from the database each time the modal opens, then
/// held here as plain editable fields (mirroring how the Edit Task/
/// Event/Note modals hold in-progress edits) until Save writes them
/// back — so opening Settings, changing your mind, and closing
/// without saving doesn't touch anything.
pub struct SettingsModalState {
    pub open: bool,
    pub notifications_enabled: bool,
    pub task_reminders_enabled: bool,
    pub lead_minutes_input: String,
    /// Feature request: "Startup Scripts/Commands" — master switch,
    /// off by default (see `queries::settings::exec_commands_enabled`
    /// for why).
    pub exec_commands_enabled: bool,
    /// Feature request: "Toggle Switch In App Settings from Hyprctl
    /// Notify to Caelestia Notify".
    pub notification_backend: crate::notifications::NotificationBackend,
    pub error: Option<String>,
}

impl Default for SettingsModalState {
    fn default() -> Self {
        Self {
            open: false,
            notifications_enabled: true,
            task_reminders_enabled: true,
            lead_minutes_input: "10".to_owned(),
            exec_commands_enabled: false,
            notification_backend: crate::notifications::NotificationBackend::default(),
            error: None,
        }
    }
}

impl SettingsModalState {
    /// Opens the modal, loading current values from the database so
    /// it doesn't show stale defaults if settings were changed
    /// elsewhere (or on a previous run).
    pub fn open_from(&mut self, db: &Database) {
        let conn = &db.conn;
        self.notifications_enabled = queries::settings::notifications_enabled(conn).unwrap_or(true);
        self.task_reminders_enabled = queries::settings::task_reminders_enabled(conn).unwrap_or(true);
        self.lead_minutes_input = queries::settings::reminder_lead_minutes(conn).unwrap_or(10).to_string();
        self.exec_commands_enabled = queries::settings::exec_commands_enabled(conn).unwrap_or(false);
        self.notification_backend = queries::settings::notification_backend(conn).unwrap_or_default();
        self.error = None;
        self.open = true;
    }
}

pub fn show(ctx: &egui::Context, theme: &Theme, db: Option<&Database>, state: &mut SettingsModalState) {
    if !state.open {
        return;
    }
    let Some(db) = db else {
        return;
    };

    let p = &theme.palette;
    let mut modal_open = true;
    let mut save_requested = false;
    let mut close_requested = false;

    Modal::new("settings_modal", "Settings").show(ctx, theme, &mut modal_open, |ui| {
        ui.label(RichText::new("Notifications").font(theme.small_font()).color(p.text_faint));
        ui.add_space(Spacing::XS);

        ui.checkbox(&mut state.notifications_enabled, "Enable session reminders");
        ui.add_space(Spacing::XS);

        ui.add_enabled_ui(state.notifications_enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Remind me").font(theme.small_font()).color(p.text_faint));
                Input::new(&mut state.lead_minutes_input, "10").width(48.0).show(ui, theme);
                ui.label(
                    RichText::new("minutes before a session starts").font(theme.small_font()).color(p.text_faint),
                );
            });
        });
        ui.add_space(Spacing::SM);

        ui.checkbox(&mut state.task_reminders_enabled, "Remind me about tasks due today");

        ui.label(
            RichText::new(
                "Reminders only fire while Revise is open, unless the optional revise-reminderd \
                 background daemon is set up separately (see packaging/README.md) — it isn't \
                 installed or running by default.",
            )
            .font(theme.small_font())
            .color(p.text_faint),
        );
        ui.add_space(Spacing::SM);

        // Feature request: "Toggle Switch In App Settings from
        // Hyprctl Notify to Caelestia Notify".
        ui.label(RichText::new("Notification backend").font(theme.small_font()).color(p.text_faint));
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut state.notification_backend,
                crate::notifications::NotificationBackend::Hyprctl,
                "Hyprctl",
            );
            ui.selectable_value(
                &mut state.notification_backend,
                crate::notifications::NotificationBackend::Caelestia,
                "Caelestia",
            );
        });
        ui.label(
            RichText::new(
                "Hyprctl uses Hyprland's own built-in notification popup. Caelestia sends a \
                 standard desktop notification instead (via notify-send), which caelestia-shell \
                 picks up and renders in its own style — requires libnotify and caelestia-shell \
                 to actually be running.",
            )
            .font(theme.small_font())
            .color(p.text_faint),
        );

        ui.add_space(Spacing::MD);
        ui.separator();
        ui.add_space(Spacing::SM);
        ui.label(RichText::new("Startup Commands").font(theme.small_font()).color(p.text_faint));
        ui.add_space(Spacing::XS);
        ui.checkbox(&mut state.exec_commands_enabled, "Run an event's startup command at its start time");
        ui.label(
            RichText::new(
                "Off by default. When on, Revise runs whatever shell command is set on an \
                 event (e.g. opening a browser) automatically at its start time, with no \
                 further confirmation — only enable this if you trust everything you (or \
                 anyone with access to this database) put in an event's startup command.",
            )
            .font(theme.small_font())
            .color(p.text_faint),
        );

        if let Some(error) = &state.error {
            ui.add_space(Spacing::SM);
            ui.label(RichText::new(error).font(theme.small_font()).color(p.text));
        }

        ui.add_space(Spacing::MD);
        ui.horizontal(|ui| {
            if RButton::new("Save").primary().show(ui, theme).clicked() {
                save_requested = true;
            }
            if RButton::new("Close").show(ui, theme).clicked() {
                close_requested = true;
            }
        });
    });

    if save_requested {
        match state.lead_minutes_input.trim().parse::<i64>() {
            Ok(minutes) if minutes >= 1 => {
                let conn = &db.conn;
                let _ = queries::settings::set_notifications_enabled(conn, state.notifications_enabled);
                let _ = queries::settings::set_task_reminders_enabled(conn, state.task_reminders_enabled);
                let _ = queries::settings::set_reminder_lead_minutes(conn, minutes);
                let _ = queries::settings::set_exec_commands_enabled(conn, state.exec_commands_enabled);
                let _ = queries::settings::set_notification_backend(conn, state.notification_backend);
                state.error = None;
                state.open = false;
            }
            _ => {
                state.error = Some("Lead time must be a positive number of minutes.".to_owned());
            }
        }
        return;
    }

    if !modal_open || close_requested {
        state.open = false;
    }
}
