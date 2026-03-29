//! Visual undo history modal.
//!
//! Shows the complete edit history in a navigable two-panel modal
//! (list + detail). Each entry shows the edit type, author, and
//! relative timestamp. Users can jump to any history point with Enter.

use aura_core::author::AuthorId;
use aura_core::buffer::EditKind;
use ratatui::style::Color;

/// A single entry in the undo tree display.
pub struct UndoEntry {
    /// Index in the buffer's history vector.
    pub index: usize,
    /// Human-readable description of the edit (e.g. "insert 5 chars").
    pub kind_label: String,
    /// Author label (e.g. "human", "ai:claude", "peer:alice").
    pub author_label: String,
    /// Color for the author.
    pub author_color: Color,
    /// Relative time string (e.g. "2s ago", "1m ago").
    pub timestamp: String,
    /// Whether this entry is the current history position.
    pub is_current: bool,
    /// Whether this entry is beyond history_pos (redo territory).
    pub is_redo: bool,
    /// Preview of the edit content (first ~60 chars).
    pub preview: String,
    /// Position in the file where the edit occurred.
    pub position: usize,
}

/// The undo tree modal state.
pub struct UndoTreeModal {
    /// Whether the modal is visible.
    pub visible: bool,
    /// Formatted entries from the buffer history.
    pub entries: Vec<UndoEntry>,
    /// Currently selected entry index.
    pub selected: usize,
    /// Scroll offset.
    pub scroll: usize,
    /// Whether to show the detail panel.
    pub show_detail: bool,
    /// The buffer's current history_pos when the modal was opened.
    pub current_pos: usize,
}

impl UndoTreeModal {
    /// Create a new undo tree modal from buffer history data.
    pub fn new(entries: Vec<UndoEntry>, current_pos: usize) -> Self {
        let selected = if current_pos > 0 { current_pos - 1 } else { 0 };
        Self {
            visible: true,
            entries,
            selected,
            scroll: 0,
            show_detail: true,
            current_pos,
        }
    }

    /// Navigate up.
    pub fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Navigate down.
    pub fn select_down(&mut self) {
        let max = self.entries.len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
    }

    /// Page up (10 entries).
    pub fn page_up(&mut self) {
        self.selected = self.selected.saturating_sub(10);
    }

    /// Page down (10 entries).
    pub fn page_down(&mut self) {
        let max = self.entries.len().saturating_sub(1);
        self.selected = (self.selected + 10).min(max);
    }

    /// Toggle the detail panel.
    pub fn toggle_detail(&mut self) {
        self.show_detail = !self.show_detail;
    }

    /// Get the history index for the selected entry.
    pub fn selected_history_pos(&self) -> Option<usize> {
        self.entries.get(self.selected).map(|e| e.index + 1)
    }
}

/// Build undo entries from buffer history.
pub fn build_entries(
    history: &[aura_core::buffer::Edit],
    history_pos: usize,
    now: std::time::Instant,
) -> Vec<UndoEntry> {
    history
        .iter()
        .enumerate()
        .map(|(i, edit)| {
            let kind_label = match &edit.kind {
                EditKind::Insert { text, .. } => {
                    let chars = text.chars().count();
                    format!("insert {} char{}", chars, if chars == 1 { "" } else { "s" })
                }
                EditKind::Delete { deleted, .. } => {
                    let chars = deleted.chars().count();
                    format!("delete {} char{}", chars, if chars == 1 { "" } else { "s" })
                }
            };

            let (author_label, author_color) = match &edit.author {
                AuthorId::Human => ("human".to_string(), Color::Green),
                AuthorId::Ai(name) => (format!("ai:{name}"), Color::Cyan),
                AuthorId::Peer { name, .. } => (format!("peer:{name}"), Color::Magenta),
            };

            let elapsed = now.duration_since(edit.timestamp);
            let secs = elapsed.as_secs();
            let timestamp = if secs < 60 {
                format!("{}s ago", secs)
            } else if secs < 3600 {
                format!("{}m ago", secs / 60)
            } else {
                format!("{}h ago", secs / 3600)
            };

            let (preview, position) = match &edit.kind {
                EditKind::Insert { pos, text } => {
                    let preview = text.chars().take(60).collect::<String>();
                    let preview = preview.replace('\n', "↵");
                    (format!("+\"{}\"", preview), *pos)
                }
                EditKind::Delete { start, deleted, .. } => {
                    let preview = deleted.chars().take(60).collect::<String>();
                    let preview = preview.replace('\n', "↵");
                    (format!("-\"{}\"", preview), *start)
                }
            };

            UndoEntry {
                index: i,
                kind_label,
                author_label,
                author_color,
                timestamp,
                is_current: i + 1 == history_pos,
                is_redo: i >= history_pos,
                preview,
                position,
            }
        })
        .collect()
}
