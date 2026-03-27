//! Right-side panel listing all AI conversation history.

use aura_core::conversation::{ConversationMessage, ConversationStore};

/// A display-ready conversation entry.
#[derive(Debug, Clone)]
pub struct ConversationEntry {
    /// Unique conversation identifier.
    pub id: String,
    /// AI-generated summary, if available.
    pub summary: Option<String>,
    /// File path attached to this conversation.
    pub file_path: String,
    /// Number of distinct files changed by edit decisions.
    pub files_changed: usize,
    /// Total number of messages in this conversation.
    pub message_count: usize,
    /// ISO-8601 timestamp of the last update.
    pub updated_at: String,
    /// Git commit hash at conversation creation time.
    pub git_commit: Option<String>,
    /// Git branch name at conversation creation time.
    pub branch: Option<String>,
}

/// Persistent right-side panel showing all AI conversations.
pub struct ConversationHistoryPanel {
    /// All conversation entries loaded from the store.
    pub conversations: Vec<ConversationEntry>,
    /// Index of the currently selected conversation.
    pub selected: usize,
    /// Scroll offset for the conversation list.
    pub scroll: usize,
    /// Panel width in columns.
    pub width: u16,
    /// Whether the panel is visible.
    pub visible: bool,
    /// Index of the expanded conversation (showing messages), if any.
    pub expanded: Option<usize>,
    /// Messages of the currently expanded conversation.
    pub expanded_messages: Vec<ConversationMessage>,
    /// Scroll offset within the expanded message view.
    pub message_scroll: usize,
}

impl ConversationHistoryPanel {
    /// Create a new panel with the given width.
    pub fn new(width: u16) -> Self {
        Self {
            conversations: Vec::new(),
            selected: 0,
            scroll: 0,
            width,
            visible: false,
            expanded: None,
            expanded_messages: Vec::new(),
            message_scroll: 0,
        }
    }

    /// Toggle panel visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Refresh the conversation list from the store.
    pub fn refresh(&mut self, store: &ConversationStore) {
        match store.all_conversations_with_stats(100) {
            Ok(rows) => {
                self.conversations = rows
                    .into_iter()
                    .map(|(conv, files_changed, message_count)| ConversationEntry {
                        id: conv.id,
                        summary: conv.summary,
                        file_path: conv.file_path,
                        files_changed,
                        message_count,
                        updated_at: conv.updated_at,
                        git_commit: conv.git_commit,
                        branch: conv.branch,
                    })
                    .collect();
                self.clamp_selected();
            }
            Err(e) => {
                tracing::warn!("Failed to load conversations: {}", e);
            }
        }
    }

    /// Move selection up.
    pub fn select_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.adjust_scroll();
    }

    /// Move selection down.
    pub fn select_down(&mut self) {
        let max = self.conversations.len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
        self.adjust_scroll();
    }

    /// Toggle expand/collapse of the selected conversation.
    pub fn toggle_expand(&mut self, store: &ConversationStore) {
        if self.conversations.is_empty() {
            return;
        }
        if self.expanded == Some(self.selected) {
            // Collapse.
            self.expanded = None;
            self.expanded_messages.clear();
            self.message_scroll = 0;
        } else {
            // Expand.
            let id = &self.conversations[self.selected].id;
            match store.messages_for_conversation(id) {
                Ok(msgs) => {
                    self.expanded = Some(self.selected);
                    self.expanded_messages = msgs;
                    self.message_scroll = 0;
                }
                Err(e) => {
                    tracing::warn!("Failed to load messages: {}", e);
                }
            }
        }
    }

    /// Scroll expanded messages up.
    pub fn scroll_messages_up(&mut self) {
        self.message_scroll = self.message_scroll.saturating_sub(1);
    }

    /// Scroll expanded messages down.
    pub fn scroll_messages_down(&mut self) {
        if !self.expanded_messages.is_empty() {
            self.message_scroll = self
                .message_scroll
                .saturating_add(1)
                .min(self.expanded_messages.len().saturating_sub(1));
        }
    }

    /// Clamp selected index to valid range.
    fn clamp_selected(&mut self) {
        let max = self.conversations.len().saturating_sub(1);
        if self.selected > max {
            self.selected = max;
        }
    }

    /// Keep selected item visible within scroll window.
    fn adjust_scroll(&mut self) {
        if self.selected < self.scroll {
            self.scroll = self.selected;
        }
        // We don't know height here, so we just ensure scroll <= selected.
        // The renderer will handle the rest.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::conversation::{ConversationStore, MessageRole};

    #[test]
    fn test_navigation_clamping() {
        let mut panel = ConversationHistoryPanel::new(30);
        // Empty list — selection should stay at 0.
        panel.select_up();
        assert_eq!(panel.selected, 0);
        panel.select_down();
        assert_eq!(panel.selected, 0);

        // Add some entries manually.
        for i in 0..3 {
            panel.conversations.push(ConversationEntry {
                id: format!("conv-{i}"),
                summary: None,
                file_path: format!("file{i}.rs"),
                files_changed: 0,
                message_count: 0,
                updated_at: String::new(),
                git_commit: None,
                branch: None,
            });
        }

        panel.select_down();
        assert_eq!(panel.selected, 1);
        panel.select_down();
        assert_eq!(panel.selected, 2);
        panel.select_down(); // should clamp
        assert_eq!(panel.selected, 2);
        panel.select_up();
        assert_eq!(panel.selected, 1);
    }

    #[test]
    fn test_expand_collapse() {
        let store = ConversationStore::in_memory().unwrap();
        let conv = store
            .create_conversation("test.rs", 0, 10, None, None)
            .unwrap();
        store
            .add_message(&conv.id, MessageRole::HumanIntent, "hello", None)
            .unwrap();

        let mut panel = ConversationHistoryPanel::new(30);
        panel.refresh(&store);

        assert_eq!(panel.conversations.len(), 1);
        assert!(panel.expanded.is_none());

        // Expand.
        panel.toggle_expand(&store);
        assert_eq!(panel.expanded, Some(0));
        assert_eq!(panel.expanded_messages.len(), 1);

        // Collapse.
        panel.toggle_expand(&store);
        assert!(panel.expanded.is_none());
        assert!(panel.expanded_messages.is_empty());
    }

    #[test]
    fn test_refresh() {
        let store = ConversationStore::in_memory().unwrap();
        store.create_conversation("a.rs", 0, 5, None, None).unwrap();
        store.create_conversation("b.rs", 0, 5, None, None).unwrap();

        let mut panel = ConversationHistoryPanel::new(30);
        panel.refresh(&store);
        assert_eq!(panel.conversations.len(), 2);
    }

    #[test]
    fn test_message_scroll() {
        let mut panel = ConversationHistoryPanel::new(30);
        panel.scroll_messages_up();
        assert_eq!(panel.message_scroll, 0);
        panel.scroll_messages_down(); // no messages
        assert_eq!(panel.message_scroll, 0);
    }
}
