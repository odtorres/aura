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

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::buffer::Edit;
    use std::time::{Duration, Instant};

    fn make_entry(idx: usize) -> UndoEntry {
        UndoEntry {
            index: idx,
            kind_label: format!("edit {idx}"),
            author_label: "human".into(),
            author_color: Color::Green,
            timestamp: "0s ago".into(),
            is_current: false,
            is_redo: false,
            preview: "".into(),
            position: 0,
        }
    }

    #[test]
    fn modal_selects_previous_entry_on_open() {
        // With history_pos=3, selection starts at index 2 (the most recently
        // applied edit), so Enter accepts the current state by default.
        let entries = (0..5).map(make_entry).collect();
        let modal = UndoTreeModal::new(entries, 3);
        assert_eq!(modal.selected, 2);
        assert!(modal.visible);
        assert!(modal.show_detail);
    }

    #[test]
    fn navigation_clamps_to_bounds() {
        let mut modal = UndoTreeModal::new((0..3).map(make_entry).collect(), 2);
        assert_eq!(modal.selected, 1);

        modal.select_up();
        modal.select_up();
        modal.select_up(); // Can't go below 0.
        assert_eq!(modal.selected, 0);

        modal.select_down();
        modal.select_down();
        modal.select_down(); // Clamps to max (2).
        assert_eq!(modal.selected, 2);

        modal.page_up();
        assert_eq!(modal.selected, 0);
        modal.page_down();
        assert_eq!(modal.selected, 2);
    }

    #[test]
    fn toggle_detail_flips_flag() {
        let mut modal = UndoTreeModal::new(vec![], 0);
        let before = modal.show_detail;
        modal.toggle_detail();
        assert_ne!(modal.show_detail, before);
    }

    #[test]
    fn selected_history_pos_is_one_based() {
        // history_pos is 1-based (history.len() after a push), so the UI
        // converts the zero-based entry index accordingly.
        let entries = (0..3).map(make_entry).collect();
        let mut modal = UndoTreeModal::new(entries, 2);
        modal.selected = 0;
        assert_eq!(modal.selected_history_pos(), Some(1));
        modal.selected = 2;
        assert_eq!(modal.selected_history_pos(), Some(3));
    }

    #[test]
    fn build_entries_labels_insert_and_delete() {
        let now = Instant::now();
        let history = vec![
            Edit {
                kind: EditKind::Insert {
                    pos: 0,
                    text: "hello".into(),
                },
                author: AuthorId::Human,
                timestamp: now - Duration::from_secs(5),
            },
            Edit {
                kind: EditKind::Delete {
                    start: 0,
                    end: 5,
                    deleted: "hello".into(),
                },
                author: AuthorId::Ai("claude".into()),
                timestamp: now - Duration::from_secs(1),
            },
        ];
        let entries = build_entries(&history, 2, now);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].kind_label.starts_with("insert 5"));
        assert_eq!(entries[0].author_label, "human");
        // is_current flags the entry whose 1-based index equals history_pos.
        assert!(!entries[0].is_current);
        assert!(entries[1].is_current);
        assert_eq!(entries[1].author_label, "ai:claude");
        assert!(entries[1].kind_label.starts_with("delete 5"));
    }
}
