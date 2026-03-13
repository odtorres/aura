//! Application state and main event loop.

use crate::highlight::{HighlightedLine, Language, SyntaxHighlighter};
use crate::lsp::{self, Diagnostic, LspClient, LspEvent};
use aura_ai::{AiConfig, AiEvent, AnthropicClient, EditorContext, Message};
use aura_core::{Buffer, Cursor};
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
        };
        app.refresh_highlights();
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

            // Send debounced didChange if needed (300ms delay).
            if self.lsp_change_pending
                && self.lsp_last_change.elapsed() > Duration::from_millis(300)
            {
                self.send_lsp_did_change();
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

        // Build context.
        let selection = self.visual_selection_range();
        let ctx = EditorContext::from_buffer(&self.buffer, &self.cursor, selection);

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
            self.set_status("AI proposal accepted");
        }
        self.mode = Mode::Normal;
    }

    /// Reject the current AI proposal.
    pub fn reject_proposal(&mut self) {
        self.proposal = None;
        self.mode = Mode::Normal;
        self.set_status("AI proposal rejected");
    }

    /// Check if AI is available.
    pub fn has_ai(&self) -> bool {
        self.ai_client.is_some()
    }

    /// Mark syntax highlights as stale (call after any buffer edit).
    pub fn mark_highlights_dirty(&mut self) {
        self.highlights_dirty = true;
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
