//! Application state and main event loop.

use crate::highlight::{HighlightedLine, Language, SyntaxHighlighter};
use crate::lsp::{self, Diagnostic, LspClient, LspEvent};
use crate::mcp_client::{McpClientConnection, McpClientEvent};
use crate::mcp_server::{AgentRegistry, McpAction, McpAppResponse, McpServer};
use crate::semantic_index::SemanticIndexer;
use aura_ai::{AiConfig, AiEvent, AnthropicClient, EditorContext, Message};
use aura_core::conversation::{
    ConversationId, ConversationMessage, ConversationStore, Decision, MessageRole,
};
use aura_core::{AuthorId, Buffer, Cursor};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::DefaultTerminal;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The editing mode — vim-inspired but simplified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Command,
    Visual,
    VisualLine,
    /// User is typing a natural-language intent for the AI.
    Intent,
    /// Reviewing an AI-proposed change.
    Review,
}

impl Mode {
    /// Display label for the status bar.
    pub fn label(&self) -> &str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Command => "COMMAND",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "V-LINE",
            Mode::Intent => "INTENT",
            Mode::Review => "REVIEW",
        }
    }
}

/// An AI-proposed edit that the user can accept or reject.
#[derive(Debug, Clone)]
pub struct AiProposal {
    /// The proposed replacement text.
    pub proposed_text: String,
    /// The original text that would be replaced.
    pub original_text: String,
    /// Start char index of the replacement range.
    pub start: usize,
    /// End char index of the replacement range.
    pub end: usize,
    /// Whether the AI is still streaming the proposal.
    pub streaming: bool,
}

/// Conversation history panel data.
#[derive(Debug, Clone)]
pub struct ConversationPanel {
    /// The messages to display.
    pub messages: Vec<ConversationMessage>,
    /// File + line range this conversation covers.
    pub file_info: String,
    /// Scroll offset in the panel.
    pub scroll: usize,
}

/// Top-level application state.
pub struct App {
    pub buffer: Buffer,
    pub cursor: Cursor,
    pub mode: Mode,
    pub should_quit: bool,
    pub command_input: String,
    pub status_message: Option<String>,
    /// Viewport offset for scrolling.
    pub scroll_row: usize,
    pub scroll_col: usize,
    /// Yank register (clipboard).
    pub register: Option<String>,
    /// Anchor position for visual mode selection.
    pub visual_anchor: Option<Cursor>,
    /// Whether to show authorship markers in the gutter.
    pub show_authorship: bool,
    /// Leader key pending (Space was pressed, waiting for next key).
    pub leader_pending: bool,
    /// Intent input buffer (what the user types in Intent mode).
    pub intent_input: String,
    /// Active AI proposal for review.
    pub proposal: Option<AiProposal>,
    /// AI client (None if no API key is configured).
    ai_client: Option<AnthropicClient>,
    /// Receiver for streaming AI events.
    ai_receiver: Option<mpsc::Receiver<AiEvent>>,
    /// Syntax highlighter (None if language not supported).
    highlighter: Option<SyntaxHighlighter>,
    /// Cached per-line highlight colours. Regenerated on edits.
    pub highlight_lines: Vec<HighlightedLine>,
    /// Whether highlights are stale and need refreshing.
    highlights_dirty: bool,
    /// Active LSP client (None if no server available).
    lsp_client: Option<LspClient>,
    /// Current diagnostics for the open file.
    pub diagnostics: Vec<Diagnostic>,
    /// Hover information to display as a popup.
    pub hover_info: Option<String>,
    /// Whether `g` was pressed (waiting for second key: `g`→top, `d`→definition).
    pub g_pending: bool,
    /// Whether the buffer changed since last didChange notification.
    lsp_change_pending: bool,
    /// When the last buffer edit occurred (for debouncing didChange).
    lsp_last_change: Instant,
    /// Semantic indexer for code structure analysis.
    semantic_indexer: Option<SemanticIndexer>,
    /// Detected language for the current file.
    language: Option<Language>,
    /// Whether the semantic index needs refreshing.
    semantic_dirty: bool,
    /// Cached semantic context for the symbol at cursor.
    pub semantic_info: Option<String>,
    /// Conversation storage (None if DB could not be opened).
    conversation_store: Option<ConversationStore>,
    /// Active conversation ID for current intent/review cycle.
    active_conversation: Option<ConversationId>,
    /// Active intent ID for current cycle.
    active_intent_id: Option<String>,
    /// Conversation panel for viewing history.
    pub conversation_panel: Option<ConversationPanel>,
    /// Whether to show conversation markers in the gutter.
    pub show_conversations: bool,
    /// Cached line ranges that have conversation history.
    pub conversation_lines: Vec<(usize, usize)>,
    /// MCP server (None if not started).
    mcp_server: Option<McpServer>,
    /// Connected external MCP servers.
    mcp_clients: Vec<McpClientConnection>,
    /// Registry of connected agents for multi-agent collaboration.
    pub agent_registry: AgentRegistry,
}

impl App {
    /// Create a new app. Attempts to initialise the AI client from env.
    pub fn new(buffer: Buffer) -> Self {
        let ai_client = AiConfig::from_env().and_then(|config| AnthropicClient::new(config).ok());

        // Detect language from file extension and set up highlighter.
        let language = buffer
            .file_path()
            .and_then(|p| p.extension())
            .and_then(|ext| ext.to_str())
            .and_then(Language::from_extension);
        let highlighter = language.and_then(SyntaxHighlighter::new);
        let semantic_indexer = language.and_then(SemanticIndexer::new);

        // Open conversation database.
        let conversation_store = buffer
            .file_path()
            .and_then(|p| p.parent())
            .map(|dir| dir.join(".aura").join("conversations.db"))
            .and_then(|db_path| ConversationStore::open(db_path).ok());

        let conversation_lines = conversation_store
            .as_ref()
            .and_then(|store| {
                let fp = buffer.file_path()?.display().to_string();
                store.lines_with_conversations(&fp).ok()
            })
            .unwrap_or_default();

        // Try to start a language server.
        let lsp_client = buffer
            .file_path()
            .and_then(|p| p.extension())
            .and_then(|ext| ext.to_str())
            .and_then(lsp::detect_server)
            .and_then(|config| {
                let file_path = buffer.file_path()?;
                let workspace_root = file_path.parent().unwrap_or(file_path);
                let content = buffer.rope().to_string();
                LspClient::start(&config, workspace_root, file_path, &content).ok()
            });

        // Start MCP server.
        let mcp_server = match McpServer::start() {
            Ok(server) => {
                tracing::info!("MCP server listening on port {}", server.port);
                Some(server)
            }
            Err(e) => {
                tracing::warn!("Failed to start MCP server: {}", e);
                None
            }
        };

        // Load MCP client connections from aura.toml.
        let mcp_clients = buffer
            .file_path()
            .and_then(|p| p.parent())
            .map(|dir| {
                let config_path = dir.join("aura.toml");
                let configs = crate::mcp_client::load_config(&config_path);
                configs
                    .iter()
                    .filter_map(|config| match McpClientConnection::connect(config) {
                        Ok(conn) => {
                            tracing::info!("Connected to MCP server: {}", config.name);
                            Some(conn)
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to connect to MCP server {}: {}",
                                config.name,
                                e
                            );
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let mut app = Self {
            buffer,
            cursor: Cursor::origin(),
            mode: Mode::Normal,
            should_quit: false,
            command_input: String::new(),
            status_message: None,
            scroll_row: 0,
            scroll_col: 0,
            register: None,
            visual_anchor: None,
            show_authorship: true,
            leader_pending: false,
            intent_input: String::new(),
            proposal: None,
            ai_client,
            ai_receiver: None,
            highlighter,
            highlight_lines: Vec::new(),
            highlights_dirty: true,
            lsp_client,
            diagnostics: Vec::new(),
            hover_info: None,
            g_pending: false,
            lsp_change_pending: false,
            lsp_last_change: Instant::now(),
            semantic_indexer,
            language,
            semantic_dirty: true,
            semantic_info: None,
            conversation_store,
            active_conversation: None,
            active_intent_id: None,
            conversation_panel: None,
            show_conversations: false,
            conversation_lines,
            mcp_server,
            mcp_clients,
            agent_registry: AgentRegistry::default(),
        };
        app.refresh_highlights();
        app.refresh_semantic_index();
        app
    }

    /// Run the main event loop.
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        while !self.should_quit {
            if self.highlights_dirty {
                self.refresh_highlights();
            }
            terminal.draw(|frame| crate::render::draw(frame, self))?;

            // Poll for AI streaming events.
            self.poll_ai_events();

            // Poll for LSP events.
            self.poll_lsp_events();

            // Poll for MCP server requests and external MCP client events.
            self.poll_mcp_requests();
            self.poll_mcp_client_events();

            // Send debounced didChange and re-index if needed (300ms delay).
            if self.lsp_last_change.elapsed() > Duration::from_millis(300) {
                if self.lsp_change_pending {
                    self.send_lsp_did_change();
                }
                if self.semantic_dirty {
                    self.refresh_semantic_index();
                }
            }

            // Poll for terminal events with a small timeout.
            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) => self.handle_key(key.code, key.modifiers),
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }
        }

        // Shutdown MCP server on exit.
        if let Some(server) = &self.mcp_server {
            server.shutdown();
        }

        // Shutdown MCP clients on exit.
        for client in &self.mcp_clients {
            client.shutdown();
        }

        // Shutdown LSP on exit.
        if let Some(mut client) = self.lsp_client.take() {
            client.shutdown();
        }

        Ok(())
    }

    /// Route key events based on the current mode.
    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        match self.mode {
            Mode::Normal => crate::input::handle_normal(self, code, modifiers),
            Mode::Insert => crate::input::handle_insert(self, code, modifiers),
            Mode::Command => crate::input::handle_command(self, code, modifiers),
            Mode::Visual | Mode::VisualLine => crate::input::handle_visual(self, code, modifiers),
            Mode::Intent => crate::input::handle_intent(self, code, modifiers),
            Mode::Review => crate::input::handle_review(self, code, modifiers),
        }
    }

    /// Poll the AI receiver for streaming events.
    fn poll_ai_events(&mut self) {
        let rx = match &self.ai_receiver {
            Some(rx) => rx,
            None => return,
        };

        // Drain all available events.
        loop {
            match rx.try_recv() {
                Ok(AiEvent::Token(text)) => {
                    if let Some(proposal) = &mut self.proposal {
                        proposal.proposed_text.push_str(&text);
                    }
                }
                Ok(AiEvent::Done(full_text)) => {
                    // Log AI response to conversation.
                    if let (Some(store), Some(conv_id)) =
                        (&self.conversation_store, &self.active_conversation)
                    {
                        let _ = store.add_message(
                            conv_id,
                            MessageRole::AiResponse,
                            &full_text,
                            Some("claude"),
                        );
                    }
                    if let Some(proposal) = &mut self.proposal {
                        proposal.proposed_text = full_text;
                        proposal.streaming = false;
                    }
                    self.ai_receiver = None;
                    self.mode = Mode::Review;
                    self.set_status("AI proposal ready — a: accept, r: reject, Esc: cancel");
                    return;
                }
                Ok(AiEvent::Error(err)) => {
                    self.ai_receiver = None;
                    self.proposal = None;
                    self.mode = Mode::Normal;
                    self.set_status(format!("AI error: {err}"));
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.ai_receiver = None;
                    if let Some(proposal) = &mut self.proposal {
                        if !proposal.proposed_text.is_empty() {
                            proposal.streaming = false;
                            self.mode = Mode::Review;
                            self.set_status("AI proposal ready — a: accept, r: reject");
                        } else {
                            self.proposal = None;
                            self.mode = Mode::Normal;
                            self.set_status("AI stream ended unexpectedly");
                        }
                    }
                    return;
                }
            }
        }
    }

    /// Send an intent to the AI.
    pub fn send_intent(&mut self, intent: &str) {
        let client = match &self.ai_client {
            Some(c) => c,
            None => {
                self.set_status("No API key configured. Set ANTHROPIC_API_KEY");
                self.mode = Mode::Normal;
                return;
            }
        };

        // Build context with semantic info.
        let selection = self.visual_selection_range();
        let semantic = self.semantic_context_for_ai();
        let ctx = EditorContext::from_buffer_with_semantic(
            &self.buffer,
            &self.cursor,
            selection,
            semantic,
        );

        // Determine the range to replace.
        let (start, end) = if let Some((s, e)) = selection {
            (s, e)
        } else {
            // Default: the current line.
            let line_start = self
                .buffer
                .cursor_to_char_idx(&Cursor::new(self.cursor.row, 0));
            let line_end = self
                .buffer
                .line(self.cursor.row)
                .map(|l| line_start + l.len_chars())
                .unwrap_or(line_start);
            (line_start, line_end)
        };

        let original_text = if start < end && end <= self.buffer.len_chars() {
            self.buffer.rope().slice(start..end).to_string()
        } else {
            String::new()
        };

        let system = ctx.to_system_prompt();
        let messages = vec![Message {
            role: "user".to_string(),
            content: intent.to_string(),
        }];

        // Log intent to conversation store.
        let file_path_str = self
            .buffer
            .file_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let start_line = self.buffer.char_idx_to_cursor(start).row;
        let end_line = self.buffer.char_idx_to_cursor(end).row;

        if let Some(store) = &self.conversation_store {
            let conv = store
                .create_conversation(&file_path_str, start_line, end_line, None)
                .ok();
            if let Some(ref conv) = conv {
                let _ = store.add_message(&conv.id, MessageRole::HumanIntent, intent, None);
                let intent_rec = store
                    .record_intent(&conv.id, intent, &file_path_str, start_line, end_line)
                    .ok();
                self.active_intent_id = intent_rec.map(|i| i.id);
                self.active_conversation = Some(conv.id.clone());
            }
        }

        let rx = client.stream_completion(&system, messages);
        self.ai_receiver = Some(rx);
        self.proposal = Some(AiProposal {
            proposed_text: String::new(),
            original_text,
            start,
            end,
            streaming: true,
        });
        self.set_status("AI thinking...");
    }

    /// Accept the current AI proposal, applying it to the buffer.
    pub fn accept_proposal(&mut self) {
        if let Some(proposal) = self.proposal.take() {
            // Log decision.
            self.log_edit_decision(
                Decision::Accepted,
                Some(&proposal.original_text),
                Some(&proposal.proposed_text),
                proposal.start,
                proposal.end,
            );

            let author = aura_core::AuthorId::ai("claude");
            // Delete the original range, then insert the proposed text.
            if proposal.start < proposal.end {
                self.buffer
                    .delete(proposal.start, proposal.end, author.clone());
            }
            self.buffer
                .insert(proposal.start, &proposal.proposed_text, author);
            self.cursor = self
                .buffer
                .char_idx_to_cursor(proposal.start + proposal.proposed_text.len());
            self.clamp_cursor();
            self.mark_highlights_dirty();
            self.refresh_conversation_lines();
            self.set_status("AI proposal accepted");
        }
        self.mode = Mode::Normal;
    }

    /// Reject the current AI proposal.
    pub fn reject_proposal(&mut self) {
        if let Some(proposal) = &self.proposal {
            self.log_edit_decision(
                Decision::Rejected,
                Some(&proposal.original_text),
                Some(&proposal.proposed_text),
                proposal.start,
                proposal.end,
            );
        }
        self.proposal = None;
        self.mode = Mode::Normal;
        self.set_status("AI proposal rejected");
    }

    /// Check if AI is available.
    pub fn has_ai(&self) -> bool {
        self.ai_client.is_some()
    }

    /// Mark syntax highlights and semantic index as stale (call after any buffer edit).
    pub fn mark_highlights_dirty(&mut self) {
        self.highlights_dirty = true;
        self.semantic_dirty = true;
        self.lsp_change_pending = true;
        self.lsp_last_change = Instant::now();
    }

    /// Regenerate syntax highlights from the current buffer content.
    pub fn refresh_highlights(&mut self) {
        if let Some(hl) = &self.highlighter {
            let source = self.buffer.rope().to_string();
            self.highlight_lines = hl.highlight(&source);
        }
        self.highlights_dirty = false;
    }

    /// Rebuild the semantic index from the current buffer.
    pub fn refresh_semantic_index(&mut self) {
        if let (Some(indexer), Some(lang)) = (&mut self.semantic_indexer, self.language) {
            let source = self.buffer.rope().to_string();
            let path = self
                .buffer
                .file_path()
                .map(|p| p.to_path_buf())
                .unwrap_or_default();
            indexer.index_file(&path, &source, lang);
        }
        self.semantic_dirty = false;
    }

    /// Update cached semantic info for the symbol at the current cursor.
    pub fn update_semantic_context(&mut self) {
        self.semantic_info = None;
        if let Some(indexer) = &self.semantic_indexer {
            let path = self
                .buffer
                .file_path()
                .map(|p| p.to_path_buf())
                .unwrap_or_default();
            if let Some((id, _)) = indexer.graph().symbol_at(&path, self.cursor.row) {
                self.semantic_info = indexer.graph().context_string(id);
            }
        }
    }

    /// Get impact summary for a range of lines (used during AI proposal review).
    pub fn impact_summary(&self, start_line: usize, end_line: usize) -> Option<String> {
        let indexer = self.semantic_indexer.as_ref()?;
        let path = self.buffer.file_path()?.to_path_buf();
        indexer.graph().impact_summary(&path, start_line, end_line)
    }

    /// Get semantic context string for the AI.
    pub fn semantic_context_for_ai(&self) -> Option<String> {
        let indexer = self.semantic_indexer.as_ref()?;
        let path = self.buffer.file_path()?.to_path_buf();
        let (id, _) = indexer.graph().symbol_at(&path, self.cursor.row)?;
        indexer.graph().context_string(id)
    }

    /// Log an edit decision to the conversation store.
    fn log_edit_decision(
        &self,
        decision: Decision,
        original: Option<&str>,
        proposed: Option<&str>,
        start: usize,
        end: usize,
    ) {
        if let (Some(store), Some(conv_id)) = (&self.conversation_store, &self.active_conversation)
        {
            let file_path = self
                .buffer
                .file_path()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let start_line = self
                .buffer
                .char_idx_to_cursor(start.min(self.buffer.len_chars()))
                .row;
            let end_line = self
                .buffer
                .char_idx_to_cursor(end.min(self.buffer.len_chars()))
                .row;
            let _ = store.log_decision(
                conv_id,
                self.active_intent_id.as_deref(),
                decision,
                original,
                proposed,
                &file_path,
                start_line,
                end_line,
            );
        }
    }

    /// Refresh the cached list of lines with conversation history.
    fn refresh_conversation_lines(&mut self) {
        self.conversation_lines = self
            .conversation_store
            .as_ref()
            .and_then(|store| {
                let fp = self.buffer.file_path()?.display().to_string();
                store.lines_with_conversations(&fp).ok()
            })
            .unwrap_or_default();
    }

    /// Show conversation history for the line at cursor.
    pub fn show_conversation_at_cursor(&mut self) {
        let file_path = self
            .buffer
            .file_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();

        if let Some(store) = &self.conversation_store {
            match store.conversation_for_code(&file_path, self.cursor.row) {
                Ok(Some((conv, messages))) => {
                    let file_info = format!(
                        "{}:{}-{}",
                        conv.file_path,
                        conv.start_line + 1,
                        conv.end_line + 1
                    );
                    self.conversation_panel = Some(ConversationPanel {
                        messages,
                        file_info,
                        scroll: 0,
                    });
                    self.set_status("Conversation history — Esc to close");
                }
                Ok(None) => {
                    self.set_status("No conversation history for this line");
                }
                Err(e) => {
                    self.set_status(format!("Error: {e}"));
                }
            }
        } else {
            self.set_status("Conversation storage not available");
        }
    }

    /// Show recent decisions summary.
    pub fn show_recent_decisions(&mut self) {
        if let Some(store) = &self.conversation_store {
            match store.query_decisions(Some(7), None) {
                Ok(decisions) => {
                    let accepted = decisions
                        .iter()
                        .filter(|d| d.decision == Decision::Accepted)
                        .count();
                    let rejected = decisions
                        .iter()
                        .filter(|d| d.decision == Decision::Rejected)
                        .count();
                    self.set_status(format!(
                        "Last 7 days: {} accepted, {} rejected ({} total)",
                        accepted,
                        rejected,
                        decisions.len()
                    ));
                }
                Err(e) => self.set_status(format!("Error: {e}")),
            }
        }
    }

    /// Search conversation history.
    pub fn search_conversations(&mut self, query: &str) {
        if let Some(store) = &self.conversation_store {
            match store.search(query) {
                Ok(results) => {
                    if results.is_empty() {
                        self.set_status(format!("No results for \"{query}\""));
                    } else {
                        let first = &results[0];
                        self.set_status(format!(
                            "Found {} results. First: {} in {}:{}",
                            results.len(),
                            first.0.role,
                            first.1.file_path,
                            first.1.start_line + 1
                        ));
                    }
                }
                Err(e) => self.set_status(format!("Search error: {e}")),
            }
        }
    }

    /// Check if a line has conversation history.
    pub fn line_has_conversation(&self, line: usize) -> bool {
        self.conversation_lines
            .iter()
            .any(|(start, end)| line >= *start && line <= *end)
    }

    /// Poll the LSP client for events.
    fn poll_lsp_events(&mut self) {
        let events = match &self.lsp_client {
            Some(client) => client.poll_events(),
            None => return,
        };

        for event in events {
            match event {
                LspEvent::Initialized => {
                    self.set_status("LSP server ready");
                }
                LspEvent::Diagnostics(diags) => {
                    self.diagnostics = diags;
                }
                LspEvent::Definition(locations) => {
                    if let Some(loc) = locations.first() {
                        // Jump to the definition location.
                        let target_row = loc.range.start.line as usize;
                        let target_col = loc.range.start.character as usize;

                        // Check if it's in the same file.
                        let current_uri = self
                            .buffer
                            .file_path()
                            .map(|p| format!("file://{}", p.display()))
                            .unwrap_or_default();

                        if loc.uri == current_uri {
                            self.cursor.row = target_row;
                            self.cursor.col = target_col;
                            self.clamp_cursor();
                            self.set_status(format!(
                                "Definition at {}:{}",
                                target_row + 1,
                                target_col + 1
                            ));
                        } else {
                            // Cross-file — show in status bar.
                            let path = loc.uri.strip_prefix("file://").unwrap_or(&loc.uri);
                            self.set_status(format!(
                                "Definition: {}:{}",
                                path,
                                loc.range.start.line + 1
                            ));
                        }
                    } else {
                        self.set_status("No definition found");
                    }
                }
                LspEvent::Hover(result) => {
                    if let Some(hover) = result {
                        let text = hover.to_text();
                        if text.is_empty() {
                            self.hover_info = None;
                        } else {
                            self.hover_info = Some(text);
                        }
                    } else {
                        self.hover_info = None;
                        self.set_status("No hover info");
                    }
                }
                LspEvent::ServerError(e) => {
                    tracing::warn!("LSP server error: {}", e);
                    self.lsp_client = None;
                    self.set_status(format!("LSP error: {e}"));
                }
            }
        }
    }

    /// Send a didChange notification with the current buffer content.
    fn send_lsp_did_change(&mut self) {
        if let Some(client) = &mut self.lsp_client {
            let text = self.buffer.rope().to_string();
            client.did_change(&text);
        }
        self.lsp_change_pending = false;
    }

    /// Request go-to-definition at the current cursor position.
    pub fn lsp_goto_definition(&mut self) {
        if let Some(client) = &mut self.lsp_client {
            client.goto_definition(self.cursor.row as u32, self.cursor.col as u32);
            self.set_status("Looking up definition...");
        } else {
            self.set_status("No LSP server");
        }
    }

    /// Request hover info at the current cursor position.
    pub fn lsp_hover(&mut self) {
        if let Some(client) = &mut self.lsp_client {
            client.hover(self.cursor.row as u32, self.cursor.col as u32);
        } else {
            self.set_status("No LSP server");
        }
    }

    /// Check if an LSP client is active.
    pub fn has_lsp(&self) -> bool {
        self.lsp_client.is_some()
    }

    /// Get diagnostics for a specific line.
    pub fn line_diagnostics(&self, line: usize) -> Option<&Diagnostic> {
        self.diagnostics
            .iter()
            .find(|d| d.range.start.line as usize == line)
    }

    /// Count errors and warnings.
    pub fn diagnostic_counts(&self) -> (usize, usize) {
        let errors = self.diagnostics.iter().filter(|d| d.is_error()).count();
        let warnings = self.diagnostics.iter().filter(|d| d.is_warning()).count();
        (errors, warnings)
    }

    /// Jump to the next diagnostic after the current cursor position.
    pub fn next_diagnostic(&mut self) {
        let target = self
            .diagnostics
            .iter()
            .find(|d| d.range.start.line as usize > self.cursor.row)
            .or_else(|| self.diagnostics.first())
            .map(|d| {
                (
                    d.range.start.line as usize,
                    d.range.start.character as usize,
                    d.message.clone(),
                )
            });

        if let Some((row, col, msg)) = target {
            self.cursor.row = row;
            self.cursor.col = col;
            self.clamp_cursor();
            self.set_status(msg);
        } else {
            self.set_status("No diagnostics");
        }
    }

    /// Jump to the previous diagnostic before the current cursor position.
    pub fn prev_diagnostic(&mut self) {
        let target = self
            .diagnostics
            .iter()
            .rev()
            .find(|d| (d.range.start.line as usize) < self.cursor.row)
            .or_else(|| self.diagnostics.last())
            .map(|d| {
                (
                    d.range.start.line as usize,
                    d.range.start.character as usize,
                    d.message.clone(),
                )
            });

        if let Some((row, col, msg)) = target {
            self.cursor.row = row;
            self.cursor.col = col;
            self.clamp_cursor();
            self.set_status(msg);
        } else {
            self.set_status("No diagnostics");
        }
    }

    /// Clamp cursor to valid buffer positions.
    pub fn clamp_cursor(&mut self) {
        let max_row = self.buffer.line_count().saturating_sub(1);
        self.cursor.row = self.cursor.row.min(max_row);

        if let Some(line) = self.buffer.line(self.cursor.row) {
            let line_len = line.len_chars();
            let max_col = if self.mode == Mode::Insert {
                line_len
            } else {
                line_len.saturating_sub(1)
            };
            self.cursor.col = self.cursor.col.min(max_col);
        }
    }

    /// Ensure the cursor is visible within the viewport.
    pub fn scroll_to_cursor(&mut self, viewport_height: usize, viewport_width: usize) {
        let margin = 5;
        if self.cursor.row < self.scroll_row + margin {
            self.scroll_row = self.cursor.row.saturating_sub(margin);
        }
        if self.cursor.row >= self.scroll_row + viewport_height - margin {
            self.scroll_row = self.cursor.row + margin - viewport_height + 1;
        }
        if self.cursor.col < self.scroll_col {
            self.scroll_col = self.cursor.col;
        }
        if self.cursor.col >= self.scroll_col + viewport_width {
            self.scroll_col = self.cursor.col - viewport_width + 1;
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }

    /// Poll and handle MCP server requests.
    fn poll_mcp_requests(&mut self) {
        let requests = match &self.mcp_server {
            Some(server) => server.poll_requests(),
            None => return,
        };

        for req in requests {
            let response = self.handle_mcp_action(&req.action);
            let _ = req.response_tx.send(response);
        }
    }

    /// Handle a single MCP action and produce a response.
    fn handle_mcp_action(&mut self, action: &McpAction) -> McpAppResponse {
        match action {
            McpAction::ReadBuffer {
                start_line,
                end_line,
            } => {
                let total = self.buffer.line_count();
                let start = start_line.unwrap_or(0).min(total);
                let end = end_line.unwrap_or(total).min(total);

                let mut lines = Vec::new();
                for i in start..end {
                    if let Some(text) = self.buffer.line_text(i) {
                        lines.push(text);
                    }
                }

                McpAppResponse {
                    success: true,
                    data: serde_json::json!({
                        "content": lines.join(""),
                        "start_line": start,
                        "end_line": end,
                        "total_lines": total,
                        "file_path": self.buffer.file_path().map(|p| p.display().to_string()),
                    }),
                }
            }
            McpAction::EditBuffer {
                start_line,
                start_col,
                end_line,
                end_col,
                text,
                agent_id,
            } => {
                // Track the agent edit.
                self.agent_registry.record_edit(agent_id);

                let author = AuthorId::ai(agent_id.clone());
                let start_cursor = Cursor::new(*start_line, *start_col);
                let start_idx = self.buffer.cursor_to_char_idx(&start_cursor);

                if let (Some(el), Some(ec)) = (end_line, end_col) {
                    // Replace range.
                    let end_cursor = Cursor::new(*el, *ec);
                    let end_idx = self.buffer.cursor_to_char_idx(&end_cursor);
                    if start_idx < end_idx && end_idx <= self.buffer.len_chars() {
                        self.buffer.delete(start_idx, end_idx, author.clone());
                    }
                }

                self.buffer.insert(start_idx, text, author);
                self.mark_highlights_dirty();

                McpAppResponse {
                    success: true,
                    data: serde_json::json!({
                        "inserted": text.len(),
                        "position": {
                            "line": start_line,
                            "col": start_col,
                        }
                    }),
                }
            }
            McpAction::GetDiagnostics => {
                let diags: Vec<serde_json::Value> = self
                    .diagnostics
                    .iter()
                    .map(|d| {
                        serde_json::json!({
                            "line": d.range.start.line,
                            "character": d.range.start.character,
                            "severity": d.severity,
                            "message": d.message,
                            "source": d.source,
                        })
                    })
                    .collect();

                McpAppResponse {
                    success: true,
                    data: serde_json::json!({ "diagnostics": diags }),
                }
            }
            McpAction::GetSelection => {
                let selection = self.visual_selection_range().map(|(s, e)| {
                    let text = self.buffer.rope().slice(s..e).to_string();
                    let start_cursor = self.buffer.char_idx_to_cursor(s);
                    let end_cursor = self.buffer.char_idx_to_cursor(e);
                    serde_json::json!({
                        "text": text,
                        "start": { "line": start_cursor.row, "col": start_cursor.col },
                        "end": { "line": end_cursor.row, "col": end_cursor.col },
                    })
                });

                McpAppResponse {
                    success: true,
                    data: selection.unwrap_or(serde_json::json!({ "text": null })),
                }
            }
            McpAction::GetCursorContext => {
                let line_text = self.buffer.line_text(self.cursor.row).unwrap_or_default();
                let semantic = self.semantic_context_for_ai();

                McpAppResponse {
                    success: true,
                    data: serde_json::json!({
                        "cursor": {
                            "line": self.cursor.row,
                            "col": self.cursor.col,
                        },
                        "mode": self.mode.label(),
                        "current_line": line_text,
                        "file_path": self.buffer.file_path().map(|p| p.display().to_string()),
                        "total_lines": self.buffer.line_count(),
                        "semantic_context": semantic,
                    }),
                }
            }
            McpAction::GetConversationHistory {
                start_line,
                end_line,
            } => {
                let store = match &self.conversation_store {
                    Some(s) => s,
                    None => {
                        return McpAppResponse {
                            success: false,
                            data: serde_json::json!({ "error": "No conversation store" }),
                        };
                    }
                };

                let file_path = self
                    .buffer
                    .file_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();

                let start = start_line.unwrap_or(0);
                let end = end_line.unwrap_or(self.buffer.line_count());

                match store.conversations_for_range(&file_path, start, end) {
                    Ok(convs) => {
                        let data: Vec<serde_json::Value> = convs
                            .iter()
                            .map(|c| {
                                let msgs =
                                    store.messages_for_conversation(&c.id).unwrap_or_default();
                                serde_json::json!({
                                    "id": c.id,
                                    "file_path": c.file_path,
                                    "start_line": c.start_line,
                                    "end_line": c.end_line,
                                    "created_at": c.created_at,
                                    "messages": msgs.iter().map(|m| {
                                        serde_json::json!({
                                            "role": format!("{}", m.role),
                                            "content": m.content,
                                            "created_at": m.created_at,
                                        })
                                    }).collect::<Vec<_>>(),
                                })
                            })
                            .collect();

                        McpAppResponse {
                            success: true,
                            data: serde_json::json!({ "conversations": data }),
                        }
                    }
                    Err(e) => McpAppResponse {
                        success: false,
                        data: serde_json::json!({ "error": e.to_string() }),
                    },
                }
            }
            McpAction::RegisterAgent { name } => {
                let registered = self.agent_registry.register(name);
                if registered {
                    self.set_status(format!("Agent '{}' connected", name));
                }
                McpAppResponse {
                    success: registered,
                    data: serde_json::json!({
                        "registered": registered,
                        "agent_id": name,
                        "total_agents": self.agent_registry.count(),
                    }),
                }
            }
            McpAction::UnregisterAgent { name } => {
                let removed = self.agent_registry.unregister(name);
                if removed {
                    self.set_status(format!("Agent '{}' disconnected", name));
                }
                McpAppResponse {
                    success: removed,
                    data: serde_json::json!({
                        "unregistered": removed,
                        "total_agents": self.agent_registry.count(),
                    }),
                }
            }
            McpAction::ListAgents => {
                let agents: Vec<serde_json::Value> = self
                    .agent_registry
                    .agents
                    .values()
                    .map(|a| {
                        serde_json::json!({
                            "name": a.name,
                            "connected_at": a.connected_at,
                            "edit_count": a.edit_count,
                        })
                    })
                    .collect();

                McpAppResponse {
                    success: true,
                    data: serde_json::json!({ "agents": agents }),
                }
            }
            McpAction::GetBufferInfo => McpAppResponse {
                success: true,
                data: serde_json::json!({
                    "file_path": self.buffer.file_path().map(|p| p.display().to_string()),
                    "line_count": self.buffer.line_count(),
                    "char_count": self.buffer.len_chars(),
                    "modified": self.buffer.is_modified(),
                    "language": self.language.map(|l| format!("{:?}", l)),
                    "has_lsp": self.has_lsp(),
                    "connected_agents": self.agent_registry.count(),
                }),
            },
        }
    }

    /// Poll external MCP client connections for events.
    fn poll_mcp_client_events(&mut self) {
        for client in &mut self.mcp_clients {
            let events = client.poll_events();
            for event in events {
                match event {
                    McpClientEvent::Initialized { server_name, tools } => {
                        tracing::info!(
                            "MCP server '{}' initialized with {} tools",
                            server_name,
                            tools.len()
                        );
                    }
                    McpClientEvent::ToolResult { request_id, result } => {
                        tracing::debug!("MCP tool result for request {}: {:?}", request_id, result);
                    }
                    McpClientEvent::Error(e) => {
                        tracing::warn!("MCP client error: {}", e);
                    }
                }
            }
        }
    }

    /// Get the MCP server port (if running).
    pub fn mcp_port(&self) -> Option<u16> {
        self.mcp_server.as_ref().map(|s| s.port)
    }

    /// Get count of connected external MCP servers.
    pub fn mcp_client_count(&self) -> usize {
        self.mcp_clients.len()
    }

    /// Returns the (start, end) character indices of the visual selection, if active.
    pub fn visual_selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.visual_anchor?;
        let a = self.buffer.cursor_to_char_idx(&anchor);
        let b = self.buffer.cursor_to_char_idx(&self.cursor);

        match self.mode {
            Mode::Visual => {
                let (start, end) = if a <= b { (a, b + 1) } else { (b, a + 1) };
                Some((start, end.min(self.buffer.len_chars())))
            }
            Mode::VisualLine => {
                let (start_row, end_row) = if anchor.row <= self.cursor.row {
                    (anchor.row, self.cursor.row)
                } else {
                    (self.cursor.row, anchor.row)
                };
                let start = self.buffer.cursor_to_char_idx(&Cursor::new(start_row, 0));
                let end_cursor = Cursor::new(end_row + 1, 0);
                let end = self.buffer.cursor_to_char_idx(&end_cursor);
                Some((start, end.min(self.buffer.len_chars())))
            }
            _ => None,
        }
    }
}
