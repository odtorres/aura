//! Interactive AI chat panel for conversational AI interaction.
//!
//! Provides a right-side panel where users can type messages and see
//! streaming AI responses, with full multi-turn conversation support
//! and persistence to SQLite.

use aura_ai::{ContentBlock, Message};
use aura_core::conversation::{ConversationId, ConversationStore, MessageRole};

/// Role of a chat message for display purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    /// Message from the user.
    User,
    /// Message from the AI assistant.
    Assistant,
    /// System or error message.
    System,
}

/// Status of a tool call in the chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCallStatus {
    /// Waiting for user to approve (Y/N).
    PendingApproval,
    /// Tool is currently running.
    Running,
    /// Tool completed successfully.
    Completed,
    /// User denied the tool call.
    Denied,
    /// Tool execution failed.
    Failed(String),
}

/// A displayable item in the chat panel.
#[derive(Debug, Clone)]
pub enum ChatItem {
    /// Plain text message.
    Text {
        /// Who sent this message.
        role: ChatRole,
        /// The message text.
        content: String,
        /// Display timestamp.
        timestamp: String,
    },
    /// A tool call from the AI.
    ToolCall {
        /// Tool use ID (for API result matching).
        id: String,
        /// Tool name.
        name: String,
        /// Tool input parameters.
        input: serde_json::Value,
        /// Current status of the tool call.
        status: ToolCallStatus,
        /// Tool execution result (if completed).
        result: Option<String>,
        /// Display timestamp.
        timestamp: String,
    },
}

impl ChatItem {
    /// Get the role for rendering purposes.
    pub fn role(&self) -> ChatRole {
        match self {
            ChatItem::Text { role, .. } => *role,
            ChatItem::ToolCall { .. } => ChatRole::System,
        }
    }
}

/// A single message in the chat panel (legacy, kept for compatibility).
#[derive(Debug, Clone)]
pub struct ChatMessage {
    /// Who sent this message.
    pub role: ChatRole,
    /// The message text.
    pub content: String,
    /// Display timestamp.
    pub timestamp: String,
}

/// An @-mention autocomplete item.
#[derive(Debug, Clone)]
pub enum MentionItem {
    /// A file in the project.
    File {
        /// Relative file path.
        path: String,
    },
    /// The current editor selection.
    Selection,
    /// The current buffer content.
    Buffer,
    /// Current LSP diagnostics.
    Diagnostics,
}

impl MentionItem {
    /// Display label for the autocomplete dropdown.
    pub fn label(&self) -> String {
        match self {
            MentionItem::File { path } => path.clone(),
            MentionItem::Selection => "@selection".to_string(),
            MentionItem::Buffer => "@buffer".to_string(),
            MentionItem::Diagnostics => "@errors".to_string(),
        }
    }

    /// The text inserted into the input when this item is selected.
    pub fn insert_text(&self) -> String {
        match self {
            MentionItem::File { path } => format!("@{path} "),
            MentionItem::Selection => "@selection ".to_string(),
            MentionItem::Buffer => "@buffer ".to_string(),
            MentionItem::Diagnostics => "@errors ".to_string(),
        }
    }
}

/// Interactive chat panel state.
pub struct ChatPanel {
    /// Whether the panel is visible.
    pub visible: bool,
    /// Panel width in columns.
    pub width: u16,
    /// Display items (rendered in the panel).
    pub items: Vec<ChatItem>,
    /// Legacy display messages (for rendering compatibility).
    pub messages: Vec<ChatMessage>,
    /// Scroll offset for the message area.
    pub scroll: usize,
    /// Current user input text.
    pub input: String,
    /// Cursor position within the input text.
    pub input_cursor: usize,
    /// Whether the AI is currently streaming a response.
    pub streaming: bool,
    /// Accumulated streaming text for the current AI response.
    pub streaming_text: String,
    /// Active conversation ID for persistence.
    pub conversation_id: Option<ConversationId>,
    /// Full message history for multi-turn API calls.
    pub context_messages: Vec<Message>,
    /// Index of tool call currently awaiting user approval.
    pub pending_approval: Option<usize>,
    /// Whether we are in a multi-turn tool loop.
    pub in_tool_loop: bool,
    /// Number of tool iterations in the current loop (for safety limit).
    pub tool_loop_count: usize,
    /// Description of attached selection context (e.g. "12 lines from main.rs").
    pub selection_context: Option<String>,
    /// Maximum context messages to keep for API calls (0 = no limit).
    pub max_context_messages: usize,
    /// Whether the @-mention autocomplete popup is active.
    pub mention_active: bool,
    /// The query text after `@` being typed.
    pub mention_query: String,
    /// Filtered autocomplete results.
    pub mention_matches: Vec<MentionItem>,
    /// Selected item in the autocomplete dropdown.
    pub mention_selected: usize,
    /// Cached list of project files for @-mention completion.
    pub mention_file_cache: Vec<String>,
}

impl ChatPanel {
    /// Create a new chat panel with the given width.
    pub fn new(width: u16) -> Self {
        Self {
            visible: false,
            width,
            items: Vec::new(),
            messages: Vec::new(),
            scroll: 0,
            input: String::new(),
            input_cursor: 0,
            streaming: false,
            streaming_text: String::new(),
            conversation_id: None,
            context_messages: Vec::new(),
            pending_approval: None,
            in_tool_loop: false,
            tool_loop_count: 0,
            selection_context: None,
            max_context_messages: 40,
            mention_active: false,
            mention_query: String::new(),
            mention_matches: Vec::new(),
            mention_selected: 0,
            mention_file_cache: Vec::new(),
        }
    }

    /// Trim context_messages to the configured maximum, preserving the
    /// first message (often system/setup context) and the most recent ones.
    fn trim_context(&mut self) {
        if self.max_context_messages == 0
            || self.context_messages.len() <= self.max_context_messages
        {
            return;
        }
        // Don't trim during active tool loops — the full exchange is needed.
        if self.in_tool_loop {
            return;
        }
        let keep = self.max_context_messages;
        if self.context_messages.len() > keep {
            // Keep the first message + the last (keep - 1) messages.
            let tail_start = self.context_messages.len() - (keep - 1);
            let first = self.context_messages[0].clone();
            let tail: Vec<_> = self.context_messages[tail_start..].to_vec();
            self.context_messages.clear();
            self.context_messages.push(first);
            self.context_messages.extend(tail);
        }
    }

    /// Toggle panel visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Push a user message to the display and context history.
    pub fn push_user_message(&mut self, text: &str) {
        let timestamp = simple_timestamp();
        self.items.push(ChatItem::Text {
            role: ChatRole::User,
            content: text.to_string(),
            timestamp: timestamp.clone(),
        });
        self.messages.push(ChatMessage {
            role: ChatRole::User,
            content: text.to_string(),
            timestamp,
        });
        self.context_messages.push(Message::text("user", text));
        self.trim_context();
        self.scroll_to_bottom();
    }

    /// Begin streaming an AI response.
    pub fn start_streaming(&mut self) {
        self.streaming = true;
        self.streaming_text.clear();
    }

    /// Append a token to the current streaming response.
    pub fn append_token(&mut self, token: &str) {
        self.streaming_text.push_str(token);
        self.scroll_to_bottom();
    }

    /// Finalize the streaming response and add it to messages.
    pub fn finish_streaming(&mut self) {
        self.streaming = false;
        let text = std::mem::take(&mut self.streaming_text);
        if !text.is_empty() {
            let timestamp = simple_timestamp();
            self.items.push(ChatItem::Text {
                role: ChatRole::Assistant,
                content: text.clone(),
                timestamp: timestamp.clone(),
            });
            self.messages.push(ChatMessage {
                role: ChatRole::Assistant,
                content: text.clone(),
                timestamp,
            });
            self.context_messages
                .push(Message::text("assistant", &text));
        }
        self.trim_context();
        self.scroll_to_bottom();
    }

    /// Finalize streaming for a tool-use response (text + tool calls).
    ///
    /// Unlike `finish_streaming`, this does NOT add to context_messages
    /// because the caller will add the full content blocks.
    pub fn finish_streaming_for_tools(&mut self) {
        self.streaming = false;
        let text = std::mem::take(&mut self.streaming_text);
        if !text.is_empty() {
            let timestamp = simple_timestamp();
            self.items.push(ChatItem::Text {
                role: ChatRole::Assistant,
                content: text.clone(),
                timestamp: timestamp.clone(),
            });
            self.messages.push(ChatMessage {
                role: ChatRole::Assistant,
                content: text,
                timestamp,
            });
        }
        self.scroll_to_bottom();
    }

    /// Add a tool call to the display.
    pub fn add_tool_call(
        &mut self,
        id: &str,
        name: &str,
        input: serde_json::Value,
        status: ToolCallStatus,
    ) -> usize {
        let timestamp = simple_timestamp();
        let idx = self.items.len();
        self.items.push(ChatItem::ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            input,
            status,
            result: None,
            timestamp,
        });
        // Also add to legacy messages for rendering.
        self.messages.push(ChatMessage {
            role: ChatRole::System,
            content: format!("[Tool: {}]", name),
            timestamp: simple_timestamp(),
        });
        self.scroll_to_bottom();
        idx
    }

    /// Update the status of a tool call by item index.
    pub fn update_tool_status(&mut self, idx: usize, status: ToolCallStatus) {
        if let Some(ChatItem::ToolCall {
            status: ref mut s, ..
        }) = self.items.get_mut(idx)
        {
            *s = status;
        }
    }

    /// Set the result of a tool call by item index.
    pub fn set_tool_result(&mut self, idx: usize, result: String, success: bool) {
        if let Some(ChatItem::ToolCall {
            status: ref mut s,
            result: ref mut r,
            name,
            ..
        }) = self.items.get_mut(idx)
        {
            *r = Some(result.clone());
            if success {
                *s = ToolCallStatus::Completed;
            } else {
                *s = ToolCallStatus::Failed(result.clone());
            }
            // Update the legacy message to show result summary.
            let summary = if result.len() > 100 {
                format!("{}...", &result[..100])
            } else {
                result
            };
            if let Some(msg) = self.messages.last_mut() {
                msg.content = format!("[Tool: {} → {}]", name, summary);
            }
        }
    }

    /// Add a tool result to the context messages for the API.
    pub fn add_tool_result_to_context(&mut self, tool_use_id: &str, result: &str, is_error: bool) {
        self.context_messages.push(Message::blocks(
            "user",
            vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: result.to_string(),
                is_error: if is_error { Some(true) } else { None },
            }],
        ));
    }

    /// Add the assistant's content blocks (with tool_use) to context messages.
    pub fn add_assistant_blocks_to_context(&mut self, blocks: Vec<ContentBlock>) {
        self.context_messages
            .push(Message::blocks("assistant", blocks));
    }

    /// Add a system/error message.
    pub fn push_system_message(&mut self, text: &str) {
        self.items.push(ChatItem::Text {
            role: ChatRole::System,
            content: text.to_string(),
            timestamp: simple_timestamp(),
        });
        self.messages.push(ChatMessage {
            role: ChatRole::System,
            content: text.to_string(),
            timestamp: simple_timestamp(),
        });
        self.scroll_to_bottom();
    }

    /// Scroll messages up by one line.
    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    /// Scroll messages down by one line.
    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
    }

    /// Scroll up by a page.
    pub fn page_up(&mut self, page_size: usize) {
        self.scroll = self.scroll.saturating_sub(page_size);
    }

    /// Scroll down by a page.
    pub fn page_down(&mut self, page_size: usize) {
        self.scroll = self.scroll.saturating_add(page_size);
    }

    /// Scroll to the bottom of messages.
    pub fn scroll_to_bottom(&mut self) {
        // We set a large value; the renderer will clamp it.
        self.scroll = usize::MAX;
    }

    /// Insert a character at the cursor position.
    pub fn input_char(&mut self, ch: char) {
        let byte_pos = self.byte_offset_of_cursor();
        self.input.insert(byte_pos, ch);
        self.input_cursor = self.input_cursor.saturating_add(1);
    }

    /// Delete the character before the cursor (backspace).
    pub fn input_backspace(&mut self) {
        if self.input_cursor > 0 {
            self.input_cursor = self.input_cursor.saturating_sub(1);
            let byte_pos = self.byte_offset_of_cursor();
            if byte_pos < self.input.len() {
                let ch = self.input[byte_pos..]
                    .chars()
                    .next()
                    .expect("valid UTF-8 boundary");
                self.input
                    .replace_range(byte_pos..byte_pos + ch.len_utf8(), "");
            }
        }
    }

    /// Delete the character at the cursor (delete key).
    pub fn input_delete(&mut self) {
        let byte_pos = self.byte_offset_of_cursor();
        if byte_pos < self.input.len() {
            let ch = self.input[byte_pos..]
                .chars()
                .next()
                .expect("valid UTF-8 boundary");
            self.input
                .replace_range(byte_pos..byte_pos + ch.len_utf8(), "");
        }
    }

    /// Move input cursor left.
    pub fn input_left(&mut self) {
        self.input_cursor = self.input_cursor.saturating_sub(1);
    }

    /// Move input cursor right.
    pub fn input_right(&mut self) {
        let max = self.input.chars().count();
        if self.input_cursor < max {
            self.input_cursor = self.input_cursor.saturating_add(1);
        }
    }

    /// Move input cursor up one visual line (based on wrap width).
    pub fn input_up(&mut self, wrap_width: usize) {
        if wrap_width == 0 {
            return;
        }
        let col = self.input_cursor % wrap_width;
        if self.input_cursor >= wrap_width {
            self.input_cursor = self.input_cursor.saturating_sub(wrap_width);
        } else {
            // Already on first line — move to start.
            self.input_cursor = 0;
        }
        // Clamp column to not exceed text length.
        let max = self.input.chars().count();
        let _ = col; // col preserved by subtraction
        self.input_cursor = self.input_cursor.min(max);
    }

    /// Move input cursor down one visual line (based on wrap width).
    pub fn input_down(&mut self, wrap_width: usize) {
        if wrap_width == 0 {
            return;
        }
        let max = self.input.chars().count();
        let new_pos = self.input_cursor.saturating_add(wrap_width);
        if new_pos <= max {
            self.input_cursor = new_pos;
        } else {
            // Past last line — move to end.
            self.input_cursor = max;
        }
    }

    /// Move input cursor to the start.
    pub fn input_home(&mut self) {
        self.input_cursor = 0;
    }

    /// Move input cursor to the end.
    pub fn input_end(&mut self) {
        self.input_cursor = self.input.chars().count();
    }

    /// Take the current input text and clear the input buffer.
    pub fn take_input(&mut self) -> String {
        self.input_cursor = 0;
        std::mem::take(&mut self.input)
    }

    /// Build the message list for the AI API call.
    pub fn build_messages(&self) -> Vec<Message> {
        self.context_messages.clone()
    }

    /// Clear all messages and reset conversation.
    pub fn clear(&mut self) {
        self.items.clear();
        self.messages.clear();
        self.context_messages.clear();
        self.conversation_id = None;
        self.scroll = 0;
        self.streaming = false;
        self.streaming_text.clear();
        self.pending_approval = None;
        self.in_tool_loop = false;
        self.tool_loop_count = 0;
    }

    // ── @-mention autocomplete ────────────────────────────────────

    /// Start @-mention autocomplete.
    pub fn start_mention(&mut self) {
        self.mention_active = true;
        self.mention_query.clear();
        self.mention_selected = 0;
        self.filter_mentions();
    }

    /// Cancel @-mention autocomplete.
    pub fn cancel_mention(&mut self) {
        self.mention_active = false;
        self.mention_query.clear();
        self.mention_matches.clear();
    }

    /// Add a character to the mention query and re-filter.
    pub fn mention_type_char(&mut self, ch: char) {
        if ch == ' ' || ch == '\n' {
            // Space/enter completes or cancels.
            self.cancel_mention();
            return;
        }
        self.mention_query.push(ch);
        self.mention_selected = 0;
        self.filter_mentions();
    }

    /// Backspace in mention query.
    pub fn mention_backspace(&mut self) {
        if self.mention_query.is_empty() {
            // Backspace past @ cancels the mention.
            self.cancel_mention();
            // Also remove the @ from input.
            if self.input_cursor > 0 {
                self.input_cursor -= 1;
                let byte_pos = self.byte_offset_of_cursor();
                if byte_pos < self.input.len() {
                    self.input.remove(byte_pos);
                }
            }
        } else {
            self.mention_query.pop();
            self.mention_selected = 0;
            self.filter_mentions();
        }
    }

    /// Select the next mention match.
    pub fn mention_next(&mut self) {
        let max = self.mention_matches.len().saturating_sub(1);
        if self.mention_selected < max {
            self.mention_selected += 1;
        }
    }

    /// Select the previous mention match.
    pub fn mention_prev(&mut self) {
        if self.mention_selected > 0 {
            self.mention_selected -= 1;
        }
    }

    /// Complete the selected mention — insert its text into the input.
    pub fn complete_mention(&mut self) {
        let item = match self.mention_matches.get(self.mention_selected) {
            Some(item) => item.clone(),
            None => {
                self.cancel_mention();
                return;
            }
        };
        // Remove the `@` + query we typed so far from input.
        // The @ was already inserted, and we need to replace @+query with the full mention.
        let remove_chars = 1 + self.mention_query.chars().count(); // @ + query
        for _ in 0..remove_chars {
            if self.input_cursor > 0 {
                self.input_cursor -= 1;
                let byte_pos = self.byte_offset_of_cursor();
                if byte_pos < self.input.len() {
                    let ch = self.input[byte_pos..].chars().next().unwrap_or(' ');
                    self.input
                        .replace_range(byte_pos..byte_pos + ch.len_utf8(), "");
                }
            }
        }
        // Insert the completed mention text.
        let insert = item.insert_text();
        let byte_pos = self.byte_offset_of_cursor();
        self.input.insert_str(byte_pos, &insert);
        self.input_cursor += insert.chars().count();
        self.cancel_mention();
    }

    /// Filter mention matches based on current query.
    fn filter_mentions(&mut self) {
        self.mention_matches.clear();
        let query = self.mention_query.to_lowercase();

        // Special mentions first (if query matches).
        let specials = [
            MentionItem::Selection,
            MentionItem::Buffer,
            MentionItem::Diagnostics,
        ];
        for item in &specials {
            let label = item.label().to_lowercase();
            if query.is_empty() || label.contains(&query) {
                self.mention_matches.push(item.clone());
            }
        }

        // File matches.
        for path in &self.mention_file_cache {
            if self.mention_matches.len() >= 10 {
                break;
            }
            let lower = path.to_lowercase();
            if query.is_empty() || lower.contains(&query) || is_fuzzy_match(&lower, &query) {
                self.mention_matches
                    .push(MentionItem::File { path: path.clone() });
            }
        }
    }

    /// Cache the project file list for @-mention autocomplete.
    pub fn cache_project_files(&mut self, root: &std::path::Path) {
        self.mention_file_cache = crate::project_search::search_collect_files(root);
    }

    /// Load an existing conversation from the store.
    pub fn load_conversation(&mut self, store: &ConversationStore, conv_id: &str) {
        match store.messages_for_conversation(conv_id) {
            Ok(msgs) => {
                self.messages.clear();
                self.context_messages.clear();

                // If there's a summary, prepend it as system context so the AI
                // has history without needing hundreds of old messages.
                if let Ok(Some(summary)) = store.get_summary(conv_id) {
                    self.context_messages.push(Message::text(
                        "user",
                        &format!(
                            "[Previous conversation summary]: {summary}\n\n\
                             Continue from where we left off."
                        ),
                    ));
                }

                // Only load the last N messages into context to avoid
                // unbounded growth, but show all in the display.
                let context_start = if self.max_context_messages > 0 {
                    msgs.len().saturating_sub(self.max_context_messages)
                } else {
                    0
                };

                for (i, msg) in msgs.iter().enumerate() {
                    let (role, api_role) = match msg.role {
                        MessageRole::HumanIntent => (ChatRole::User, "user"),
                        MessageRole::AiResponse => (ChatRole::Assistant, "assistant"),
                        MessageRole::System => (ChatRole::System, "system"),
                    };
                    self.messages.push(ChatMessage {
                        role,
                        content: msg.content.clone(),
                        timestamp: msg.created_at.clone(),
                    });
                    // Add to context only recent messages (and skip system).
                    if i >= context_start && role != ChatRole::System {
                        self.context_messages
                            .push(Message::text(api_role, &msg.content));
                    }
                }
                self.conversation_id = Some(conv_id.to_string());
                self.scroll_to_bottom();
            }
            Err(e) => {
                tracing::warn!("Failed to load chat conversation: {e}");
            }
        }
    }

    /// Convert a char-based cursor position to a byte offset in the input string.
    fn byte_offset_of_cursor(&self) -> usize {
        self.input
            .char_indices()
            .nth(self.input_cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.input.len())
    }
}

/// Check if every character of `query` appears in `text` in order (fuzzy match).
fn is_fuzzy_match(text: &str, query: &str) -> bool {
    let mut text_chars = text.chars();
    for qc in query.chars() {
        let mut found = false;
        for tc in text_chars.by_ref() {
            if tc == qc {
                found = true;
                break;
            }
        }
        if !found {
            return false;
        }
    }
    true
}

/// Simple timestamp for display.
fn simple_timestamp() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    let time = secs % 86400;
    let hours = time / 3600;
    let minutes = (time % 3600) / 60;
    let seconds = time % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_panel() {
        let panel = ChatPanel::new(40);
        assert!(!panel.visible);
        assert_eq!(panel.width, 40);
        assert!(panel.messages.is_empty());
        assert!(panel.input.is_empty());
    }

    #[test]
    fn test_toggle() {
        let mut panel = ChatPanel::new(40);
        assert!(!panel.visible);
        panel.toggle();
        assert!(panel.visible);
        panel.toggle();
        assert!(!panel.visible);
    }

    #[test]
    fn test_push_user_message() {
        let mut panel = ChatPanel::new(40);
        panel.push_user_message("hello");
        assert_eq!(panel.messages.len(), 1);
        assert_eq!(panel.messages[0].role, ChatRole::User);
        assert_eq!(panel.messages[0].content, "hello");
        assert_eq!(panel.context_messages.len(), 1);
        assert_eq!(panel.context_messages[0].role, "user");
    }

    #[test]
    fn test_streaming_flow() {
        let mut panel = ChatPanel::new(40);
        panel.start_streaming();
        assert!(panel.streaming);
        panel.append_token("Hello");
        panel.append_token(" world");
        assert_eq!(panel.streaming_text, "Hello world");
        panel.finish_streaming();
        assert!(!panel.streaming);
        assert!(panel.streaming_text.is_empty());
        assert_eq!(panel.messages.len(), 1);
        assert_eq!(panel.messages[0].role, ChatRole::Assistant);
        assert_eq!(panel.messages[0].content, "Hello world");
        assert_eq!(panel.context_messages.len(), 1);
        assert_eq!(panel.context_messages[0].role, "assistant");
    }

    #[test]
    fn test_input_operations() {
        let mut panel = ChatPanel::new(40);
        panel.input_char('h');
        panel.input_char('i');
        assert_eq!(panel.input, "hi");
        assert_eq!(panel.input_cursor, 2);

        panel.input_left();
        assert_eq!(panel.input_cursor, 1);

        panel.input_char('!');
        assert_eq!(panel.input, "h!i");

        panel.input_backspace();
        assert_eq!(panel.input, "hi");
        assert_eq!(panel.input_cursor, 1);

        panel.input_home();
        assert_eq!(panel.input_cursor, 0);

        panel.input_end();
        assert_eq!(panel.input_cursor, 2);
    }

    #[test]
    fn test_take_input() {
        let mut panel = ChatPanel::new(40);
        panel.input_char('t');
        panel.input_char('e');
        panel.input_char('s');
        panel.input_char('t');
        let text = panel.take_input();
        assert_eq!(text, "test");
        assert!(panel.input.is_empty());
        assert_eq!(panel.input_cursor, 0);
    }

    #[test]
    fn test_clear() {
        let mut panel = ChatPanel::new(40);
        panel.push_user_message("hello");
        panel.conversation_id = Some("test-id".to_string());
        panel.clear();
        assert!(panel.messages.is_empty());
        assert!(panel.context_messages.is_empty());
        assert!(panel.conversation_id.is_none());
    }

    #[test]
    fn test_build_messages() {
        let mut panel = ChatPanel::new(40);
        panel.push_user_message("hello");
        panel
            .context_messages
            .push(Message::text("assistant", "hi there"));
        let msgs = panel.build_messages();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
    }

    #[test]
    fn test_scroll() {
        let mut panel = ChatPanel::new(40);
        panel.scroll = 5;
        panel.scroll_up();
        assert_eq!(panel.scroll, 4);
        panel.scroll_down();
        assert_eq!(panel.scroll, 5);
        panel.page_up(3);
        assert_eq!(panel.scroll, 2);
        panel.page_down(10);
        assert_eq!(panel.scroll, 12);
    }

    #[test]
    fn test_input_delete() {
        let mut panel = ChatPanel::new(40);
        panel.input = "abc".to_string();
        panel.input_cursor = 1;
        panel.input_delete();
        assert_eq!(panel.input, "ac");
        assert_eq!(panel.input_cursor, 1);
    }

    #[test]
    fn test_system_message() {
        let mut panel = ChatPanel::new(40);
        panel.push_system_message("Error occurred");
        assert_eq!(panel.messages.len(), 1);
        assert_eq!(panel.messages[0].role, ChatRole::System);
    }

    #[test]
    fn test_load_conversation() {
        let store = ConversationStore::in_memory().unwrap();
        let conv = store
            .create_conversation("__chat__", 0, 0, None, None)
            .unwrap();
        store
            .add_message(&conv.id, MessageRole::HumanIntent, "hi", None)
            .unwrap();
        store
            .add_message(&conv.id, MessageRole::AiResponse, "hello!", Some("claude"))
            .unwrap();

        let mut panel = ChatPanel::new(40);
        panel.load_conversation(&store, &conv.id);
        assert_eq!(panel.messages.len(), 2);
        assert_eq!(panel.context_messages.len(), 2);
        assert_eq!(panel.conversation_id.as_deref(), Some(conv.id.as_str()));
    }
}
