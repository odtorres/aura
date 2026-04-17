//! App lifecycle helpers: filesystem watching, RAG index readiness, config hot-reload.
//!
//! These were extracted from `app/mod.rs` so the monolith file doesn't keep
//! growing. Anything here is tick-scoped: runs every event-loop iteration,
//! cheap when nothing needs to happen, applies background-computed results
//! when they resolve.

use super::App;

impl App {
    /// Register the initial tab's file path and the config path with the
    /// filesystem watcher. Called once at the end of construction — subsequent
    /// tab opens hook in via [`watch_tab_path`].
    pub(super) fn register_initial_watched_paths(&mut self) {
        let Some(watcher) = self.file_watcher.as_mut() else {
            return;
        };
        watcher.watch(&self.config_path);
        for tab in self.tabs.tabs() {
            if let Some(path) = tab.buffer.file_path() {
                watcher.watch(path);
            }
        }
    }

    /// Register a single file path with the watcher. Safe no-op if the
    /// watcher couldn't start or the path is already watched.
    pub fn watch_tab_path(&mut self, path: &std::path::Path) {
        if let Some(watcher) = self.file_watcher.as_mut() {
            watcher.watch(path);
        }
    }

    /// Drain pending filesystem events and dispatch to the appropriate
    /// handler (buffer reload vs. config reload). Returns true if at least
    /// one event was processed — callers can use this to skip the
    /// fallback mtime poll for that tick.
    pub(super) fn drain_file_watcher_events(&mut self) -> bool {
        let Some(watcher) = self.file_watcher.as_mut() else {
            return false;
        };
        let changed = watcher.drain();
        if changed.is_empty() {
            return false;
        }

        let config_path = &self.config_path;
        let config_canon = config_path.canonicalize().ok();
        let mut config_changed = false;
        let mut buffer_changed = false;
        for path in &changed {
            let is_config = path == config_path
                || path.file_name() == config_path.file_name()
                    && path.canonicalize().ok() == config_canon;
            if is_config {
                config_changed = true;
            } else {
                buffer_changed = true;
            }
        }

        if config_changed {
            self.reload_config_from_disk();
        }
        if buffer_changed {
            // Delegate to the existing check which handles dirty-buffer
            // warnings and cursor preservation across reloads.
            self.check_external_file_changes();
        }
        true
    }

    /// Promote a background-built RAG index into the active slot once its
    /// builder thread finishes. Called every tick; the first completed build
    /// wins and the receiver is dropped afterwards.
    pub(super) fn poll_rag_index_ready(&mut self) {
        let Some(rx) = self.rag_index_pending.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(index) => {
                if !index.chunks.is_empty() {
                    tracing::info!(
                        "RAG index: {} chunks from {} files",
                        index.chunks.len(),
                        index.file_count
                    );
                    self.rag_index = Some(index);
                }
                self.rag_index_pending = None;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.rag_index_pending = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                // Still indexing.
            }
        }
    }

    /// Reload `aura.toml` from disk and apply it immediately. Extracted so
    /// both the watcher path and the fallback poll can share the logic.
    pub(super) fn reload_config_from_disk(&mut self) {
        let new_mtime = std::fs::metadata(&self.config_path)
            .and_then(|m| m.modified())
            .ok();
        if new_mtime == self.config_mtime {
            return;
        }
        self.config_mtime = new_mtime;
        let new_config = crate::config::load_config(&self.config_path);
        let config_table = crate::config::load_config_table(&self.config_path);
        let new_theme = crate::config::resolve_theme(&new_config.theme, config_table.as_ref());
        self.config = new_config;
        self.theme = new_theme;
        self.set_status("Config reloaded from aura.toml");
    }
}
