//! Update-check plumbing: poll the background checker, show the notification,
//! run the installer. Extracted from `mod.rs` to keep update state together.

use super::App;
use crate::update::UpdateStatus;

impl App {
    /// Consume a result from the background update checker, if one has
    /// arrived, and update the notification/status accordingly.
    pub(super) fn poll_update_check(&mut self) {
        if let Some(ref rx) = self.update_receiver {
            if let Ok(status) = rx.try_recv() {
                match &status {
                    UpdateStatus::Available { version, .. } => {
                        self.update_notification_visible = true;
                        self.set_status(format!(
                            "Update available: v{} \u{2192} v{version}",
                            crate::update::CURRENT_VERSION
                        ));
                    }
                    UpdateStatus::UpToDate => {
                        self.set_status(format!(
                            "AURA v{} is up to date",
                            crate::update::CURRENT_VERSION
                        ));
                    }
                    UpdateStatus::Error(e) => {
                        self.set_status(format!("Update check failed: {e}"));
                    }
                }
                self.update_status = Some(status);
                self.update_receiver = None;
            }
        }
    }

    /// Trigger a forced update check (bypasses cache). Used by `:update` command.
    pub fn force_update_check(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        crate::update::spawn_forced_update_check(tx);
        self.update_receiver = Some(rx);
        self.update_status = None; // Clear old status while checking.
        self.set_status("Checking for updates...");
    }

    /// Dismiss the update notification toast.
    pub fn dismiss_update_notification(&mut self) {
        self.update_notification_visible = false;
    }

    /// Show the update confirmation modal.
    pub fn show_update_modal(&mut self) {
        self.update_notification_visible = false;
        self.update_modal_visible = true;
    }

    /// Run the platform-appropriate update command in the embedded terminal.
    pub fn run_update(&mut self) {
        self.update_modal_visible = false;
        if let Some(UpdateStatus::Available { ref version, .. }) = self.update_status {
            let cmd = "curl --proto '=https' --tlsv1.2 -LsSf https://github.com/odtorres/aura/releases/latest/download/aura-installer.sh | sh".to_string();
            self.set_status(format!("Updating to v{}...", version));
            // Run the update command in the embedded terminal.
            self.terminal_mut().visible = true;
            self.terminal_focused = true;
            self.terminal_mut()
                .send_bytes(format!("{}\n", cmd).as_bytes());
        }
    }
}
