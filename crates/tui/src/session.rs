//! Session persistence — save and restore editor state across restarts.
//!
//! On exit, AURA serialises the open tabs, cursor positions, scroll offsets,
//! active tab index, and UI panel state to `.aura/session.json`.  On the next
//! launch (when no explicit file argument is given) the session is restored so
//! the user picks up exactly where they left off.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Persisted state for a single editor tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabState {
    /// Absolute path to the file (None for scratch buffers).
    pub file_path: Option<PathBuf>,
    /// Cursor row.
    pub cursor_row: usize,
    /// Cursor column.
    pub cursor_col: usize,
    /// Viewport scroll row offset.
    pub scroll_row: usize,
    /// Viewport scroll column offset.
    pub scroll_col: usize,
}

/// Persisted UI layout state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiState {
    /// Whether the file tree sidebar is visible.
    pub file_tree_visible: bool,
    /// Whether the chat panel is visible.
    pub chat_panel_visible: bool,
    /// Whether the terminal pane is visible.
    pub terminal_visible: bool,
    /// Active sidebar view ("files" or "git").
    pub sidebar_view: String,
}

/// Top-level session data written to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// The working directory the session belongs to.
    pub working_directory: PathBuf,
    /// Open tabs in order.
    pub tabs: Vec<TabState>,
    /// Index of the active tab.
    pub active_tab: usize,
    /// UI panel state.
    pub ui: UiState,
}

/// Default session file location: `<project_root>/.aura/session.json`.
pub fn session_path(project_root: &Path) -> PathBuf {
    project_root.join(".aura").join("session.json")
}

/// Save a session to disk.  Creates the `.aura/` directory if needed.
pub fn save_session(session: &Session, path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(session)?;
    std::fs::write(path, json)?;
    tracing::info!("Session saved to {:?}", path);
    Ok(())
}

/// Load a session from disk.  Returns `None` if the file doesn't exist or
/// can't be parsed.
pub fn load_session(path: &Path) -> Option<Session> {
    let data = std::fs::read_to_string(path).ok()?;
    let session: Session = serde_json::from_str(&data)
        .inspect_err(|e| tracing::warn!("Failed to parse session file {:?}: {}", path, e))
        .ok()?;
    tracing::info!(
        "Restored session from {:?} ({} tabs)",
        path,
        session.tabs.len()
    );
    Some(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let session = Session {
            working_directory: PathBuf::from("/tmp/test"),
            tabs: vec![
                TabState {
                    file_path: Some(PathBuf::from("/tmp/test/foo.rs")),
                    cursor_row: 10,
                    cursor_col: 5,
                    scroll_row: 3,
                    scroll_col: 0,
                },
                TabState {
                    file_path: None,
                    cursor_row: 0,
                    cursor_col: 0,
                    scroll_row: 0,
                    scroll_col: 0,
                },
            ],
            active_tab: 0,
            ui: UiState {
                file_tree_visible: true,
                chat_panel_visible: false,
                terminal_visible: false,
                sidebar_view: "files".into(),
            },
        };

        let json = serde_json::to_string_pretty(&session).unwrap();
        let restored: Session = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.tabs.len(), 2);
        assert_eq!(restored.tabs[0].cursor_row, 10);
        assert_eq!(restored.active_tab, 0);
        assert!(restored.ui.file_tree_visible);
        assert!(!restored.ui.chat_panel_visible);
    }
}
