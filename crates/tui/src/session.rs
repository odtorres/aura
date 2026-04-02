//! Session persistence — save and restore editor state across restarts.
//!
//! On exit, AURA serialises the open tabs, cursor positions, scroll offsets,
//! active tab index, and UI panel state to `.aura/session.json`.  On the next
//! launch (when no explicit file argument is given) the session is restored so
//! the user picks up exactly where they left off.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    /// Vim marks: char → (row, col).
    #[serde(default)]
    pub marks: HashMap<char, (usize, usize)>,
    /// Code folding: list of (start_line, end_line) pairs.
    #[serde(default)]
    pub folded_ranges: Vec<(usize, usize)>,
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

    // --- Panel sizes (None = use default) ---
    /// File tree / sidebar width.
    #[serde(default)]
    pub file_tree_width: Option<u16>,
    /// Chat panel width.
    #[serde(default)]
    pub chat_panel_width: Option<u16>,
    /// Terminal pane height.
    #[serde(default)]
    pub terminal_height: Option<u16>,
    /// Conversation history panel width.
    #[serde(default)]
    pub conversation_history_width: Option<u16>,

    // --- Additional panel visibility ---
    /// Whether the conversation history panel is visible.
    #[serde(default)]
    pub conversation_history_visible: bool,
    /// Whether the AI Visor panel is visible.
    #[serde(default)]
    pub ai_visor_visible: bool,
    /// Whether the debug panel is visible.
    #[serde(default)]
    pub debug_panel_visible: bool,

    // --- Split pane layout ---
    /// Whether a split pane is active.
    #[serde(default)]
    pub split_active: bool,
    /// Split direction: "vertical" or "horizontal".
    #[serde(default)]
    pub split_direction: Option<String>,
    /// Index of the secondary split tab.
    #[serde(default)]
    pub split_tab_idx: Option<usize>,

    // --- Macro registers ---
    /// Recorded macro key sequences: register char → list of key strings.
    #[serde(default)]
    pub macro_registers: HashMap<char, Vec<String>>,
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

/// Format a KeyEvent into a human-readable string for JSON serialization.
pub fn format_key_event(key: &crossterm::event::KeyEvent) -> String {
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut parts = Vec::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl".to_string());
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("shift".to_string());
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        parts.push("alt".to_string());
    }
    let code = match key.code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::Esc => "esc".to_string(),
        KeyCode::F(n) => format!("f{n}"),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pageup".to_string(),
        KeyCode::PageDown => "pagedown".to_string(),
        _ => format!("{:?}", key.code),
    };
    parts.push(code);
    parts.join("+")
}

/// Parse a human-readable key string back into a KeyEvent.
pub fn parse_key_event(s: &str) -> Option<crossterm::event::KeyEvent> {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let parts: Vec<&str> = s.split('+').collect();
    let mut modifiers = KeyModifiers::empty();
    let mut code_str = "";

    for part in &parts {
        match part.to_lowercase().as_str() {
            "ctrl" => modifiers |= KeyModifiers::CONTROL,
            "shift" => modifiers |= KeyModifiers::SHIFT,
            "alt" => modifiers |= KeyModifiers::ALT,
            _ => code_str = part,
        }
    }

    let code = match code_str.to_lowercase().as_str() {
        "backspace" => KeyCode::Backspace,
        "enter" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "esc" => KeyCode::Esc,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "delete" => KeyCode::Delete,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "space" => KeyCode::Char(' '),
        s if s.starts_with('f') && s.len() <= 3 => {
            let n: u8 = s[1..].parse().ok()?;
            KeyCode::F(n)
        }
        s if s.chars().count() == 1 => KeyCode::Char(s.chars().next()?),
        _ => return None,
    };

    Some(KeyEvent::new(code, modifiers))
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
                    marks: HashMap::from([('a', (5, 0)), ('b', (20, 3))]),
                    folded_ranges: vec![(10, 25), (30, 40)],
                },
                TabState {
                    file_path: None,
                    cursor_row: 0,
                    cursor_col: 0,
                    scroll_row: 0,
                    scroll_col: 0,
                    marks: HashMap::new(),
                    folded_ranges: Vec::new(),
                },
            ],
            active_tab: 0,
            ui: UiState {
                file_tree_visible: true,
                chat_panel_visible: false,
                terminal_visible: false,
                sidebar_view: "files".into(),
                file_tree_width: Some(30),
                chat_panel_width: None,
                terminal_height: Some(12),
                conversation_history_width: None,
                conversation_history_visible: false,
                ai_visor_visible: false,
                debug_panel_visible: false,
                split_active: false,
                split_direction: None,
                split_tab_idx: None,
                macro_registers: HashMap::from([('a', vec!["j".into(), "d".into(), "d".into()])]),
            },
        };

        let json = serde_json::to_string_pretty(&session).unwrap();
        let restored: Session = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.tabs.len(), 2);
        assert_eq!(restored.tabs[0].cursor_row, 10);
        assert_eq!(restored.tabs[0].marks.get(&'a'), Some(&(5, 0)));
        assert_eq!(restored.tabs[0].folded_ranges.len(), 2);
        assert_eq!(restored.active_tab, 0);
        assert!(restored.ui.file_tree_visible);
        assert!(!restored.ui.chat_panel_visible);
        assert_eq!(restored.ui.file_tree_width, Some(30));
        assert_eq!(restored.ui.terminal_height, Some(12));
        assert_eq!(
            restored.ui.macro_registers.get(&'a'),
            Some(&vec!["j".to_string(), "d".to_string(), "d".to_string()])
        );
    }

    #[test]
    fn backward_compatible() {
        // Old session JSON without new fields should still parse.
        let old_json = r#"{
            "working_directory": "/tmp",
            "tabs": [{"file_path": "/tmp/x.rs", "cursor_row": 0, "cursor_col": 0, "scroll_row": 0, "scroll_col": 0}],
            "active_tab": 0,
            "ui": {"file_tree_visible": true, "chat_panel_visible": false, "terminal_visible": false, "sidebar_view": "files"}
        }"#;
        let session: Session = serde_json::from_str(old_json).unwrap();
        assert!(session.tabs[0].marks.is_empty());
        assert!(session.tabs[0].folded_ranges.is_empty());
        assert!(session.ui.file_tree_width.is_none());
        assert!(session.ui.macro_registers.is_empty());
    }

    #[test]
    fn key_event_round_trip() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let events = vec![
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::empty()),
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            KeyEvent::new(KeyCode::F(5), KeyModifiers::empty()),
        ];
        for event in &events {
            let s = format_key_event(event);
            let parsed = parse_key_event(&s).expect(&format!("Failed to parse: {s}"));
            assert_eq!(parsed.code, event.code, "Code mismatch for {s}");
            assert_eq!(
                parsed.modifiers, event.modifiers,
                "Modifiers mismatch for {s}"
            );
        }
    }
}
