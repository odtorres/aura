//! Right-side panel listing all AI conversation history.
//!
//! Shows conversations grouped by git branch with human-friendly timestamps,
//! intent-based titles, search/filter, and acceptance rate indicators.

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
    /// Latest intent text (what the user asked).
    pub intent: Option<String>,
    /// Accepted edit count.
    pub accepted: usize,
    /// Rejected edit count.
    pub rejected: usize,
    /// First user message (for chat conversations that lack intents).
    pub first_user_message: Option<String>,
}

impl ConversationEntry {
    /// Get the best display title: intent > summary > file path basename.
    pub fn display_title(&self) -> String {
        if let Some(ref intent) = self.intent {
            if !intent.is_empty() {
                return smart_truncate(intent, 60);
            }
        }
        if let Some(ref summary) = self.summary {
            if !summary.is_empty() {
                return smart_truncate(summary, 60);
            }
        }
        // Friendly label for chat conversations.
        if self.file_path == "__chat__" {
            return if let Some(ref first_msg) = self.first_user_message {
                format!("Chat: {}", smart_truncate(first_msg, 55))
            } else {
                "Chat session".to_string()
            };
        }
        // Friendly label for Claude Code observed conversations.
        if self.file_path.starts_with("__claude_code__") {
            return if let Some(ref first_msg) = self.first_user_message {
                format!("CC: {}", smart_truncate(first_msg, 57))
            } else {
                "Claude Code session".to_string()
            };
        }
        // Fall back to file basename.
        self.file_path
            .rsplit('/')
            .next()
            .unwrap_or(&self.file_path)
            .to_string()
    }

    /// Get the branch name or "no branch".
    pub fn branch_name(&self) -> &str {
        self.branch.as_deref().unwrap_or("no branch")
    }

    /// Get a human-friendly relative timestamp.
    pub fn relative_time(&self) -> String {
        relative_timestamp(&self.updated_at)
    }

    /// Get acceptance rate string (e.g., "2/3" or empty).
    pub fn acceptance_badge(&self) -> Option<String> {
        let total = self.accepted + self.rejected;
        if total > 0 {
            Some(format!("{}/{}", self.accepted, total))
        } else {
            None
        }
    }
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
    /// Search query for filtering conversations.
    pub search_query: String,
    /// Whether the search input is active.
    pub search_active: bool,
    /// Filtered indices into `conversations` (None = show all).
    pub filtered: Option<Vec<usize>>,
    /// Whether the detail modal is open (full-screen view of a conversation).
    pub detail_view: bool,
    /// Scroll offset within the detail modal.
    pub detail_scroll: usize,
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
            search_query: String::new(),
            search_active: false,
            filtered: None,
            detail_view: false,
            detail_scroll: 0,
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
                    .map(|(conv, files_changed, message_count)| {
                        // Load latest intent for this conversation.
                        let intent = store
                            .latest_intent(&conv.id)
                            .ok()
                            .flatten()
                            .map(|i| i.intent_text);
                        // Load decision stats.
                        let (accepted, rejected) = store.decision_stats(&conv.id).unwrap_or((0, 0));
                        // For chat/Claude Code conversations, load first user message as title.
                        let first_user_message = if intent.is_none()
                            && (conv.file_path == "__chat__"
                                || conv.file_path.starts_with("__claude_code__"))
                        {
                            store.first_user_message(&conv.id).ok().flatten()
                        } else {
                            None
                        };
                        ConversationEntry {
                            id: conv.id,
                            summary: conv.summary,
                            file_path: conv.file_path,
                            files_changed,
                            message_count,
                            updated_at: conv.updated_at,
                            git_commit: conv.git_commit,
                            branch: conv.branch,
                            intent,
                            accepted,
                            rejected,
                            first_user_message,
                        }
                    })
                    .collect();
                self.clamp_selected();
                if self.search_active {
                    self.apply_filter();
                }
            }
            Err(e) => {
                tracing::warn!("Failed to load conversations: {}", e);
            }
        }
    }

    /// Get the visible entries (filtered or all).
    pub fn visible_entries(&self) -> Vec<usize> {
        self.filtered
            .clone()
            .unwrap_or_else(|| (0..self.conversations.len()).collect())
    }

    /// Get conversations grouped by branch.
    pub fn grouped_by_branch(&self) -> Vec<(String, Vec<usize>)> {
        let indices = self.visible_entries();
        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
        let mut branch_map: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        for idx in indices {
            let branch = self.conversations[idx].branch_name().to_string();
            if let Some(&group_idx) = branch_map.get(&branch) {
                groups[group_idx].1.push(idx);
            } else {
                branch_map.insert(branch.clone(), groups.len());
                groups.push((branch, vec![idx]));
            }
        }

        groups
    }

    /// Move selection up.
    pub fn select_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.adjust_scroll();
    }

    /// Move selection down.
    pub fn select_down(&mut self) {
        let max = self.visible_entries().len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
        self.adjust_scroll();
    }

    /// Toggle expand/collapse of the selected conversation.
    pub fn toggle_expand(&mut self, store: &ConversationStore) {
        let entries = self.visible_entries();
        if entries.is_empty() {
            return;
        }
        let actual_idx = entries.get(self.selected).copied().unwrap_or(0);

        if self.expanded == Some(actual_idx) {
            self.expanded = None;
            self.expanded_messages.clear();
            self.message_scroll = 0;
        } else {
            let id = &self.conversations[actual_idx].id;
            match store.messages_for_conversation(id) {
                Ok(msgs) => {
                    self.expanded = Some(actual_idx);
                    self.expanded_messages = msgs;
                    self.message_scroll = 0;
                }
                Err(e) => {
                    tracing::warn!("Failed to load messages: {}", e);
                }
            }
        }
    }

    /// Start search mode.
    pub fn start_search(&mut self) {
        self.search_active = true;
        self.search_query.clear();
    }

    /// Type a character into the search query.
    pub fn search_type_char(&mut self, c: char) {
        self.search_query.push(c);
        self.apply_filter();
    }

    /// Delete last character from search query.
    pub fn search_backspace(&mut self) {
        self.search_query.pop();
        if self.search_query.is_empty() {
            self.search_active = false;
            self.filtered = None;
        } else {
            self.apply_filter();
        }
    }

    /// Open the detail modal for the currently expanded conversation.
    pub fn open_detail(&mut self) {
        if self.expanded.is_some() {
            self.detail_view = true;
            self.detail_scroll = 0;
        }
    }

    /// Close the detail modal.
    pub fn close_detail(&mut self) {
        self.detail_view = false;
        self.detail_scroll = 0;
    }

    /// Scroll the detail view up.
    pub fn detail_scroll_up(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(1);
    }

    /// Scroll the detail view down.
    pub fn detail_scroll_down(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(1);
    }

    /// Page up in the detail view.
    pub fn detail_page_up(&mut self, lines: usize) {
        self.detail_scroll = self.detail_scroll.saturating_sub(lines);
    }

    /// Page down in the detail view.
    pub fn detail_page_down(&mut self, lines: usize) {
        self.detail_scroll = self.detail_scroll.saturating_add(lines);
    }

    /// Cancel search.
    pub fn cancel_search(&mut self) {
        self.search_active = false;
        self.search_query.clear();
        self.filtered = None;
        self.selected = 0;
    }

    /// Apply the current search filter.
    fn apply_filter(&mut self) {
        let query = self.search_query.to_lowercase();
        if query.is_empty() {
            self.filtered = None;
            return;
        }
        self.filtered = Some(
            self.conversations
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    entry.display_title().to_lowercase().contains(&query)
                        || entry.file_path.to_lowercase().contains(&query)
                        || entry.branch_name().to_lowercase().contains(&query)
                })
                .map(|(i, _)| i)
                .collect(),
        );
        self.selected = 0;
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
    }
}

/// Convert an ISO-8601 timestamp to a human-friendly relative time.
fn relative_timestamp(iso: &str) -> String {
    // Parse "2026-03-28T01:33:46Z" or similar.
    // Simple approach: extract date components and compare to now.
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Try to parse the ISO timestamp to Unix seconds.
    let ts_secs = parse_iso_to_unix(iso).unwrap_or(now_secs);
    let diff = now_secs.saturating_sub(ts_secs);

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        let mins = diff / 60;
        format!("{mins}m ago")
    } else if diff < 86400 {
        let hours = diff / 3600;
        format!("{hours}h ago")
    } else if diff < 604800 {
        let days = diff / 86400;
        format!("{days}d ago")
    } else {
        // Show the date portion only.
        iso.split('T').next().unwrap_or(iso).to_string()
    }
}

/// Parse an ISO-8601 timestamp to Unix seconds (simple implementation).
fn parse_iso_to_unix(iso: &str) -> Option<u64> {
    // Expected format: "2026-03-28T01:33:46Z" or "2026-03-28 01:33:46"
    let s = iso.replace('T', " ").replace('Z', "");
    let parts: Vec<&str> = s.split(' ').collect();
    let date_parts: Vec<u64> = parts
        .first()?
        .split('-')
        .filter_map(|p| p.parse().ok())
        .collect();
    if date_parts.len() != 3 {
        return None;
    }
    let (year, month, day) = (date_parts[0], date_parts[1], date_parts[2]);

    let time_parts: Vec<u64> = parts
        .get(1)
        .unwrap_or(&"00:00:00")
        .split(':')
        .filter_map(|p| p.parse().ok())
        .collect();
    let (hour, min, sec) = (
        *time_parts.first().unwrap_or(&0),
        *time_parts.get(1).unwrap_or(&0),
        *time_parts.get(2).unwrap_or(&0),
    );

    // Rough days-since-epoch (not accounting for leap years perfectly).
    let days_in_year = 365u64;
    let days = (year - 1970) * days_in_year
        + (year - 1970) / 4 // leap years (approximate)
        + (month - 1) * 30 // approximate days per month
        + day
        - 1;
    Some(days * 86400 + hour * 3600 + min * 60 + sec)
}

/// Truncate a string at a word boundary.
fn smart_truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let truncated = &s[..max.min(s.len())];
    // Find the last space to break at a word boundary.
    if let Some(last_space) = truncated.rfind(' ') {
        if last_space > max / 2 {
            return format!("{}...", &truncated[..last_space]);
        }
    }
    format!("{truncated}...")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::conversation::{ConversationStore, MessageRole};

    #[test]
    fn test_navigation_clamping() {
        let mut panel = ConversationHistoryPanel::new(30);
        panel.select_up();
        assert_eq!(panel.selected, 0);
        panel.select_down();
        assert_eq!(panel.selected, 0);

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
                intent: None,
                accepted: 0,
                rejected: 0,
                first_user_message: None,
            });
        }

        panel.select_down();
        assert_eq!(panel.selected, 1);
        panel.select_down();
        assert_eq!(panel.selected, 2);
        panel.select_down();
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

        panel.toggle_expand(&store);
        assert_eq!(panel.expanded, Some(0));
        assert_eq!(panel.expanded_messages.len(), 1);

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
        panel.scroll_messages_down();
        assert_eq!(panel.message_scroll, 0);
    }

    #[test]
    fn test_smart_truncate() {
        assert_eq!(smart_truncate("hello world", 20), "hello world");
        let result = smart_truncate("hello beautiful world today", 15);
        assert!(result.ends_with("..."), "got: {result}");
        assert!(result.len() <= 20);
    }

    #[test]
    fn test_relative_timestamp() {
        // Just verify it doesn't panic.
        let result = relative_timestamp("2026-03-28T01:33:46Z");
        assert!(!result.is_empty());
    }

    #[test]
    fn test_display_title() {
        let entry = ConversationEntry {
            id: "test".into(),
            summary: None,
            file_path: "/Users/test/project/src/main.rs".into(),
            files_changed: 0,
            message_count: 0,
            updated_at: String::new(),
            git_commit: None,
            branch: None,
            intent: Some("Fix the null pointer bug in parser".into()),
            accepted: 0,
            rejected: 0,
            first_user_message: None,
        };
        assert_eq!(entry.display_title(), "Fix the null pointer bug in parser");

        let entry2 = ConversationEntry {
            intent: None,
            summary: Some("Refactored error handling".into()),
            ..entry.clone()
        };
        assert_eq!(entry2.display_title(), "Refactored error handling");

        let entry3 = ConversationEntry {
            intent: None,
            summary: None,
            ..entry
        };
        assert_eq!(entry3.display_title(), "main.rs");
    }

    #[test]
    fn test_branch_grouping() {
        let mut panel = ConversationHistoryPanel::new(30);
        panel.conversations = vec![
            ConversationEntry {
                id: "1".into(),
                summary: None,
                file_path: "a.rs".into(),
                files_changed: 0,
                message_count: 0,
                updated_at: String::new(),
                git_commit: None,
                branch: Some("main".into()),
                intent: None,
                accepted: 0,
                rejected: 0,
                first_user_message: None,
            },
            ConversationEntry {
                id: "2".into(),
                summary: None,
                file_path: "b.rs".into(),
                files_changed: 0,
                message_count: 0,
                updated_at: String::new(),
                git_commit: None,
                branch: Some("feature".into()),
                intent: None,
                accepted: 0,
                rejected: 0,
                first_user_message: None,
            },
            ConversationEntry {
                id: "3".into(),
                summary: None,
                file_path: "c.rs".into(),
                files_changed: 0,
                message_count: 0,
                updated_at: String::new(),
                git_commit: None,
                branch: Some("main".into()),
                intent: None,
                accepted: 0,
                rejected: 0,
                first_user_message: None,
            },
        ];

        let groups = panel.grouped_by_branch();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, "main");
        assert_eq!(groups[0].1.len(), 2);
        assert_eq!(groups[1].0, "feature");
        assert_eq!(groups[1].1.len(), 1);
    }
}
