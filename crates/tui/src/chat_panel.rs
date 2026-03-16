//! Interactive AI chat panel for conversational AI interaction.
//!
//! Provides a right-side panel where users can type messages and see
//! streaming AI responses, with full multi-turn conversation support
//! and persistence to SQLite.

use aura_ai::Message;
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

/// A single message in the chat panel.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    /// Who sent this message.
    pub role: ChatRole,
    /// The message text.
    pub content: String,
    /// Display timestamp.
    pub timestamp: String,
}

/// Interactive chat panel state.
pub struct ChatPanel {
    /// Whether the panel is visible.
    pub visible: bool,
    /// Panel width in columns.
    pub width: u16,
    /// Display messages (rendered in the panel).
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
}

impl ChatPanel {
    /// Create a new chat panel with the given width.
    pub fn new(width: u16) -> Self {
        Self {
            visible: false,
            width,
            messages: Vec::new(),
            scroll: 0,
            input: String::new(),
            input_cursor: 0,
            streaming: false,
            streaming_text: String::new(),
            conversation_id: None,
            context_messages: Vec::new(),
        }
    }

    /// Toggle panel visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Push a user message to the display and context history.
    pub fn push_user_message(&mut self, text: &str) {
        let timestamp = simple_timestamp();
        self.messages.push(ChatMessage {
            role: ChatRole::User,
            content: text.to_string(),
            timestamp,
        });
        self.context_messages.push(Message {
            role: "user".to_string(),
            content: text.to_string(),
        });
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
            self.messages.push(ChatMessage {
                role: ChatRole::Assistant,
                content: text.clone(),
                timestamp,
            });
            self.context_messages.push(Message {
                role: "assistant".to_string(),
                content: text,
            });
        }
        self.scroll_to_bottom();
    }

    /// Add a system/error message.
    pub fn push_system_message(&mut self, text: &str) {
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
                let ch = self.input[byte_pos..].chars().next().unwrap();
                self.input.replace_range(byte_pos..byte_pos + ch.len_utf8(), "");
            }
        }
    }

    /// Delete the character at the cursor (delete key).
    pub fn input_delete(&mut self) {
        let byte_pos = self.byte_offset_of_cursor();
        if byte_pos < self.input.len() {
            let ch = self.input[byte_pos..].chars().next().unwrap();
            self.input.replace_range(byte_pos..byte_pos + ch.len_utf8(), "");
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
        self.messages.clear();
        self.context_messages.clear();
        self.conversation_id = None;
        self.scroll = 0;
        self.streaming = false;
        self.streaming_text.clear();
    }

    /// Load an existing conversation from the store.
    pub fn load_conversation(&mut self, store: &ConversationStore, conv_id: &str) {
        match store.messages_for_conversation(conv_id) {
            Ok(msgs) => {
                self.messages.clear();
                self.context_messages.clear();
                for msg in &msgs {
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
                    // Only add user/assistant to context (skip system for API).
                    if role != ChatRole::System {
                        self.context_messages.push(Message {
                            role: api_role.to_string(),
                            content: msg.content.clone(),
                        });
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
        panel.context_messages.push(Message {
            role: "assistant".to_string(),
            content: "hi there".to_string(),
        });
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
        let conv = store.create_conversation("__chat__", 0, 0, None, None).unwrap();
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
