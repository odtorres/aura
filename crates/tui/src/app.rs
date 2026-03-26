//! Application state and main event loop.

use crate::chat_panel::ChatPanel;
use crate::config::{AuraConfig, Theme};
use crate::diff_view::DiffView;
use crate::embedded_terminal::EmbeddedTerminal;
use crate::file_picker::FilePicker;
use crate::file_tree::FileTree;
use crate::help::HelpOverlay;
use crate::git::{GitRepo, LineStatus};
use crate::lsp::{Diagnostic, LspEvent};
use crate::conversation_history::ConversationHistoryPanel;
use crate::session::{self, Session, TabState, UiState};
use crate::source_control::{SidebarView, SourceControlPanel};
use crate::mcp_client::{McpClientConnection, McpClientEvent};
use crate::mcp_server::{AgentRegistry, McpAction, McpAppResponse, McpServer};
use crate::plugin::PluginManager;
use crate::speculative::{Aggressiveness, GhostSuggestion, SpeculativeEngine};
use crate::tab::{EditorTab, TabManager};
use crate::chat_panel::ToolCallStatus;
use crate::chat_tools;
use crate::update::{self, UpdateStatus};
use aura_ai::{
    estimate_tokens, editor_tools, tool_permission, AiBackend, AiConfig, AiEvent, ContentBlock,
    EditorContext, Message, ToolPermission,
};
use aura_core::conversation::{
    ConversationId, ConversationMessage, ConversationStore, Decision, MessageRole,
};
use aura_core::{AuthorId, Buffer, Cursor};
use crossterm::event::{self, Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::DefaultTerminal;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

/// Get the user's home directory.
fn dirs_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Simple ISO-8601 timestamp without pulling in the `chrono` crate.
fn chrono_now() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", d.as_secs())
}

/// The editing mode — vim-inspired but simplified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Default mode for navigation and commands.
    Normal,
    /// Text insertion mode.
    Insert,
    /// Command-line input mode (`:` commands).
    Command,
    /// Character-wise visual selection mode.
    Visual,
    /// Line-wise visual selection mode.
    VisualLine,
    /// User is typing a natural-language intent for the AI.
    Intent,
    /// Reviewing an AI-proposed change.
    Review,
    /// Viewing a side-by-side git diff (read-only).
    Diff,
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
            Mode::Diff => "DIFF",
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
    /// Tab manager holding all open editor buffers.
    pub tabs: TabManager,
    /// Current editing mode.
    pub mode: Mode,
    /// When true, the event loop will exit.
    pub should_quit: bool,
    /// Text buffer for command-mode input.
    pub command_input: String,
    /// Transient message shown in the status bar.
    pub status_message: Option<String>,
    /// Yank register (clipboard).
    pub register: Option<String>,
    /// Whether to show authorship markers in the gutter.
    pub show_authorship: bool,
    /// Leader key pending (Space was pressed, waiting for next key).
    pub leader_pending: bool,
    /// Intent input buffer (what the user types in Intent mode).
    pub intent_input: String,
    /// Active AI proposal for review.
    pub proposal: Option<AiProposal>,
    /// AI backend (None if neither API key nor Claude Code CLI is available).
    ai_client: Option<AiBackend>,
    /// Receiver for streaming AI events.
    ai_receiver: Option<mpsc::Receiver<AiEvent>>,
    /// Whether `g` was pressed (waiting for second key: `g`→top, `d`→definition).
    pub g_pending: bool,
    /// Conversation storage (None if DB could not be opened).
    pub(crate) conversation_store: Option<ConversationStore>,
    /// Active conversation ID for current intent/review cycle.
    active_conversation: Option<ConversationId>,
    /// Active intent ID for current cycle.
    active_intent_id: Option<String>,
    /// Conversation panel for viewing history.
    pub conversation_panel: Option<ConversationPanel>,
    /// Whether to show conversation markers in the gutter.
    pub show_conversations: bool,
    /// MCP server (None if not started).
    mcp_server: Option<McpServer>,
    /// Connected external MCP servers.
    mcp_clients: Vec<McpClientConnection>,
    /// Registry of connected agents for multi-agent collaboration.
    pub agent_registry: AgentRegistry,
    /// Speculative execution engine for background AI analysis.
    speculative: Option<SpeculativeEngine>,
    /// Git repository handle (None if not in a git repo).
    pub(crate) git_repo: Option<GitRepo>,
    /// Whether to show inline blame annotations.
    pub show_blame: bool,
    /// Loaded configuration from aura.toml.
    pub config: AuraConfig,
    /// Resolved color theme.
    pub theme: Theme,
    /// When true, the intent_input is editing an existing proposal rather than
    /// sending a new request to the AI. Pressing Enter in Intent mode will
    /// update the proposal text and return to Review mode.
    pub editing_proposal: bool,
    /// When true, the intent_input is a revision request for the current
    /// proposal. Pressing Enter will send a new AI request that includes the
    /// current proposed text plus the user's revision instructions.
    pub revising_proposal: bool,
    /// Whether experimental mode is active. In this mode AI suggestions are
    /// auto-accepted without requiring user review.
    pub experimental_mode: bool,
    /// Plugin system — holds all registered plugins and routes events to them.
    pub plugin_manager: PluginManager,
    /// Embedded terminal / command runner pane.
    pub terminal: EmbeddedTerminal,
    /// When `true`, keystrokes are routed to the terminal input instead of the
    /// editor.
    pub terminal_focused: bool,
    /// Whether the file tree sidebar has keyboard focus.
    pub file_tree_focused: bool,
    /// Fuzzy file picker overlay.
    pub file_picker: FilePicker,
    /// In-editor help overlay.
    pub help: HelpOverlay,
    /// File tree sidebar.
    pub file_tree: FileTree,
    /// Which sidebar view is active (Files or Git).
    pub sidebar_view: SidebarView,
    /// Source control panel state.
    pub source_control: SourceControlPanel,
    /// Whether the source control panel has keyboard focus.
    pub source_control_focused: bool,
    /// Side-by-side diff view (None when not active).
    pub diff_view: Option<DiffView>,
    /// Last time the source control panel was refreshed.
    last_sc_refresh: std::time::Instant,
    /// Right-side AI conversation history panel.
    pub conversation_history: ConversationHistoryPanel,
    /// Whether the conversation history panel has keyboard focus.
    pub conversation_history_focused: bool,
    /// Interactive AI chat panel.
    pub chat_panel: ChatPanel,
    /// Whether the chat panel has keyboard focus.
    pub chat_panel_focused: bool,
    /// Receiver for streaming chat AI events.
    chat_receiver: Option<mpsc::Receiver<AiEvent>>,
    /// Cached chat panel rect from the last render frame.
    pub chat_panel_rect: Rect,
    /// Pending tool calls awaiting execution or approval.
    pending_tool_calls: Vec<PendingToolCall>,
    /// Content blocks from the current assistant turn (for multi-turn tool use).
    current_assistant_blocks: Vec<ContentBlock>,
    /// Cached system prompt for continuing tool loops.
    tool_loop_system_prompt: String,
    /// Cached panel rects from the last render frame (used for mouse click-to-focus).
    pub editor_rect: Rect,
    /// Cached terminal panel rect from the last render frame.
    pub terminal_rect: Rect,
    /// Cached file tree / source control sidebar rect from the last render frame.
    pub file_tree_rect: Rect,
    /// Cached conversation history panel rect from the last render frame.
    pub conv_history_rect: Rect,
    /// Cached tab bar rect from the last render frame (used for mouse click-to-switch).
    pub tab_bar_rect: Rect,

    // --- Bracket matching ---
    /// Position (row, col) of the matching bracket for the char under cursor.
    pub matching_bracket: Option<(usize, usize)>,

    // --- Find & Replace ---
    /// Confirmed search query (after pressing Enter).
    pub search_query: Option<String>,
    /// Text being typed in the search bar (while search_active is true).
    pub search_input: String,
    /// Whether we are currently typing a search query (/ has been pressed).
    pub search_active: bool,
    /// Whether search direction is forward (/) or backward (?).
    pub search_forward: bool,
    /// All match positions as (start_char_idx, end_char_idx) pairs.
    pub search_matches: Vec<(usize, usize)>,
    /// Index into search_matches for the current/focused match.
    pub search_current: usize,

    // --- Update checker ---
    /// Receiver for background update check results.
    update_receiver: Option<mpsc::Receiver<UpdateStatus>>,
    /// Latest update status (displayed in status bar).
    pub update_status: Option<UpdateStatus>,
}

/// A tool call pending execution or approval.
#[derive(Debug, Clone)]
struct PendingToolCall {
    /// Tool use ID from the API.
    id: String,
    /// Tool name.
    name: String,
    /// Tool input parameters.
    input: serde_json::Value,
    /// Index in chat_panel.items for status updates.
    item_index: usize,
}

impl App {
    /// Create a new app. Attempts to initialise the AI client from env.
    pub fn new(buffer: Buffer) -> Self {
        // Load configuration from aura.toml.
        let config_path = buffer
            .file_path()
            .and_then(|p| p.parent())
            .map(|dir| dir.join("aura.toml"))
            .unwrap_or_else(|| std::path::PathBuf::from("aura.toml"));
        let config = crate::config::load_config(&config_path);
        let config_table = crate::config::load_config_table(&config_path);
        let theme = crate::config::resolve_theme(&config.theme, config_table.as_ref());
        tracing::info!("Loaded config, theme: {}", theme.name);

        let ai_client = AiBackend::auto_detect();


        // Start MCP server.
        let mcp_server = match McpServer::start() {
            Ok(server) => {
                tracing::info!("MCP server listening on port {}", server.port);
                // Write discovery file so external tools (Claude Code, etc.)
                // can auto-discover the running AURA instance.
                Self::write_mcp_discovery(server.port, buffer.file_path());
                Some(server)
            }
            Err(e) => {
                tracing::warn!("Failed to start MCP server: {}", e);
                None
            }
        };

        // Open git repository — try from file path first, then from current directory.
        let git_repo = buffer
            .file_path()
            .and_then(|p| GitRepo::discover(p).ok())
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .and_then(|cwd| GitRepo::discover(&cwd).ok())
            });

        if let Some(ref repo) = git_repo {
            tracing::info!(
                "Git repo at {:?}, branch: {}",
                repo.workdir(),
                repo.current_branch().unwrap_or_else(|| "detached".into())
            );
        }

        // Open conversation database.
        // Priority: git workdir .aura/ → cwd .aura/ → ~/.aura/ (global fallback).
        let conversation_store = git_repo
            .as_ref()
            .map(|r| r.workdir().join(".aura").join("conversations.db"))
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|cwd| cwd.join(".aura").join("conversations.db"))
            })
            .and_then(|db_path| {
                tracing::debug!("Trying project-local conversation store at {:?}", db_path);
                ConversationStore::open(&db_path)
                    .inspect_err(|e| tracing::warn!("Failed to open conversation store at {:?}: {}", db_path, e))
                    .ok()
            })
            .or_else(|| {
                let fallback = dirs_path()?.join(".aura").join("conversations.db");
                tracing::debug!("Trying global fallback conversation store at {:?}", fallback);
                ConversationStore::open(&fallback)
                    .inspect_err(|e| tracing::warn!("Failed to open fallback conversation store: {}", e))
                    .ok()
            });
        if let Some(ref store) = conversation_store {
            match store.all_conversations_with_stats(1) {
                Ok(existing) => tracing::info!(
                    "Conversation store initialized ({} existing conversations)",
                    existing.len()
                ),
                Err(e) => tracing::warn!("Conversation store opened but query failed: {e}"),
            }
        } else {
            tracing::warn!("No conversation store available — AI history will not be recorded");
        }

        // Initialize speculative engine (reuses AI config).
        let speculative =
            AiConfig::from_env().and_then(|config| SpeculativeEngine::new(config).ok());

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

        // Extract MCP port for the embedded terminal environment.
        let mcp_port = mcp_server.as_ref().map(|s| s.port);

        // Determine the working directory for the embedded terminal.
        let terminal_cwd = buffer
            .file_path()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            });

        // Create the initial editor tab.
        let tab = EditorTab::new(buffer, conversation_store.as_ref(), &theme);

        let mut app = Self {
            tabs: TabManager::new(tab),
            mode: Mode::Normal,
            should_quit: false,
            command_input: String::new(),
            status_message: None,
            register: None,
            show_authorship: true,
            leader_pending: false,
            intent_input: String::new(),
            proposal: None,
            ai_client,
            ai_receiver: None,
            g_pending: false,
            conversation_store,
            active_conversation: None,
            active_intent_id: None,
            conversation_panel: None,
            show_conversations: false,
            mcp_server,
            mcp_clients,
            agent_registry: AgentRegistry::default(),
            speculative,
            git_repo,
            show_blame: false,
            config: config.clone(),
            theme,
            editing_proposal: false,
            revising_proposal: false,
            experimental_mode: false,
            plugin_manager: PluginManager::new(),
            terminal: {
                let port_str = mcp_port.map(|p| p.to_string());
                let mut env_vars: Vec<(&str, &str)> = Vec::new();
                if let Some(ref ps) = port_str {
                    env_vars.push(("AURA_MCP_PORT", ps.as_str()));
                }
                EmbeddedTerminal::with_env(terminal_cwd.clone(), &env_vars)
            },
            terminal_focused: false,
            file_tree_focused: false,
            file_picker: FilePicker::new(terminal_cwd.clone()),
            help: HelpOverlay::new(),
            file_tree: FileTree::new(terminal_cwd),
            sidebar_view: SidebarView::Files,
            source_control: SourceControlPanel::new(30),
            source_control_focused: false,
            diff_view: None,
            last_sc_refresh: std::time::Instant::now(),
            conversation_history: ConversationHistoryPanel::new(30),
            conversation_history_focused: false,
            chat_panel: ChatPanel::new(40),
            chat_panel_focused: false,
            chat_receiver: None,
            chat_panel_rect: Rect::default(),
            pending_tool_calls: Vec::new(),
            current_assistant_blocks: Vec::new(),
            tool_loop_system_prompt: String::new(),
            editor_rect: Rect::default(),
            terminal_rect: Rect::default(),
            file_tree_rect: Rect::default(),
            conv_history_rect: Rect::default(),
            tab_bar_rect: Rect::default(),
            matching_bracket: None,
            search_query: None,
            search_input: String::new(),
            search_active: false,
            search_forward: true,
            search_matches: Vec::new(),
            search_current: 0,
            update_receiver: None,
            update_status: None,
        };
        // Apply config settings.
        app.show_authorship = config.editor.show_authorship;

        // Spawn background update checker.
        if config.update.check_for_updates {
            let (tx, rx) = mpsc::channel();
            update::spawn_update_check(tx, config.update.check_interval_hours);
            app.update_receiver = Some(rx);
        }

        // Show AI backend status on startup.
        if let Some(ref backend) = app.ai_client {
            app.status_message = Some(format!("AI ready ({})", backend.label()));
        }

        app
    }

    /// Convenience: immutable reference to the active tab.
    #[inline]
    pub fn tab(&self) -> &EditorTab {
        self.tabs.active()
    }

    /// Convenience: mutable reference to the active tab.
    #[inline]
    pub fn tab_mut(&mut self) -> &mut EditorTab {
        self.tabs.active_mut()
    }

    // ---- Compatibility accessors ----
    // These delegate to the active tab so the rest of the codebase can continue
    // using `app.buffer`, `app.cursor`, etc. via method calls while we
    // transition.  Gradually these will be removed in favour of `app.tab()`.

    /// Reference to the active buffer.
    #[inline]
    pub fn buffer(&self) -> &Buffer {
        &self.tab().buffer
    }

    /// Mutable reference to the active buffer.
    #[inline]
    pub fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.tab_mut().buffer
    }

    /// Reference to the active cursor.
    #[inline]
    pub fn cursor(&self) -> &Cursor {
        &self.tab().cursor
    }

    /// Mutable reference to the active cursor.
    #[inline]
    pub fn cursor_mut(&mut self) -> &mut Cursor {
        &mut self.tab_mut().cursor
    }

    /// Run the main event loop.
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        while !self.should_quit {
            if self.tab().highlights_dirty {
                self.refresh_highlights();
            }
            // Update live selection context indicator for the chat panel.
            if self.chat_panel.visible {
                self.chat_panel.selection_context =
                    self.visual_selection_range().map(|(sel_start, sel_end)| {
                        let tab = self.tab();
                        let start_cur = tab.buffer.char_idx_to_cursor(sel_start);
                        let end_cur = tab.buffer.char_idx_to_cursor(sel_end);
                        let lines = end_cur.row.saturating_sub(start_cur.row) + 1;
                        let file_name = tab.file_name().to_string();
                        format!(
                            "{lines} line{} from {file_name}",
                            if lines == 1 { "" } else { "s" }
                        )
                    });
            }
            self.update_matching_bracket();
            terminal.draw(|frame| crate::render::draw(frame, self))?;

            // Set blinking bar cursor when chat input is focused, block otherwise.
            if self.chat_panel_focused && !self.chat_panel.streaming {
                crossterm::execute!(
                    std::io::stdout(),
                    crossterm::cursor::SetCursorStyle::BlinkingBar
                )?;
            } else if self.mode == Mode::Insert {
                crossterm::execute!(
                    std::io::stdout(),
                    crossterm::cursor::SetCursorStyle::BlinkingBar
                )?;
            } else {
                crossterm::execute!(
                    std::io::stdout(),
                    crossterm::cursor::SetCursorStyle::SteadyBlock
                )?;
            }

            // Poll for AI streaming events.
            self.poll_ai_events();

            // Poll for chat panel streaming events.
            self.poll_chat_events();

            // Poll for LSP events.
            self.poll_lsp_events();

            // Poll for MCP server requests and external MCP client events.
            self.poll_mcp_requests();
            self.poll_mcp_client_events();

            // Poll for update check result.
            self.poll_update_check();

            // Poll speculative engine and trigger analysis if idle.
            self.poll_speculative();
            self.maybe_trigger_analysis();

            // Send debounced didChange and re-index if needed (300ms delay).
            if self.tab().lsp_last_change.elapsed() > Duration::from_millis(300) {
                if self.tab().lsp_change_pending {
                    self.send_lsp_did_change();
                }
                if self.tab().semantic_dirty {
                    self.refresh_semantic_index();
                }
            }

            // Auto-refresh source control panel every 2 seconds when visible.
            if self.sidebar_view == SidebarView::Git
                && self.last_sc_refresh.elapsed() > Duration::from_secs(2)
            {
                self.refresh_source_control();
            }

            // Poll for terminal events with a small timeout.
            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) => self.handle_key(key.code, key.modifiers),
                    Event::Mouse(mouse) => match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            self.handle_mouse_click(mouse.column, mouse.row);
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            self.handle_mouse_drag(mouse.column, mouse.row);
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            // Clear the drag anchor if no selection was made.
                            if self.mode != Mode::Visual && self.mode != Mode::VisualLine {
                                self.tab_mut().visual_anchor = None;
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            self.handle_mouse_scroll(mouse.column, mouse.row, true);
                        }
                        MouseEventKind::ScrollDown => {
                            self.handle_mouse_scroll(mouse.column, mouse.row, false);
                        }
                        _ => {}
                    },
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }
        }

        // Save session before shutting down.
        self.save_session();

        // Shutdown MCP server on exit.
        if let Some(server) = &self.mcp_server {
            server.shutdown();
        }

        // Clean up MCP discovery file.
        Self::remove_mcp_discovery();

        // Shutdown MCP clients on exit.
        for client in &self.mcp_clients {
            client.shutdown();
        }

        // Shutdown LSP on exit — all tabs.
        for tab in self.tabs.tabs_mut() {
            tab.shutdown_lsp();
        }

        Ok(())
    }

    /// Determine the project root for session storage.
    ///
    /// Uses the git workdir if available, otherwise the current directory.
    fn session_project_root(&self) -> Option<PathBuf> {
        self.git_repo
            .as_ref()
            .map(|r| r.workdir().to_path_buf())
            .or_else(|| std::env::current_dir().ok())
    }

    /// Save the current session state to disk.
    fn save_session(&self) {
        let root = match self.session_project_root() {
            Some(r) => r,
            None => return,
        };
        let path = session::session_path(&root);

        let tabs: Vec<TabState> = self
            .tabs
            .tabs()
            .iter()
            .map(|tab| TabState {
                file_path: tab.buffer.file_path().map(|p| p.to_path_buf()),
                cursor_row: tab.cursor.row,
                cursor_col: tab.cursor.col,
                scroll_row: tab.scroll_row,
                scroll_col: tab.scroll_col,
            })
            .collect();

        let sidebar_view = match self.sidebar_view {
            SidebarView::Files => "files",
            SidebarView::Git => "git",
        };

        let session = Session {
            working_directory: root.clone(),
            tabs,
            active_tab: self.tabs.active_index(),
            ui: UiState {
                file_tree_visible: self.file_tree.visible,
                chat_panel_visible: self.chat_panel.visible,
                terminal_visible: self.terminal.visible,
                sidebar_view: sidebar_view.into(),
            },
        };

        if let Err(e) = session::save_session(&session, &path) {
            tracing::warn!("Failed to save session: {}", e);
        }
    }

    /// Restore a previously saved session.
    ///
    /// Opens all tabs from the session file, restoring cursor and scroll
    /// positions.  Called from `main.rs` after `App::new` when no explicit
    /// file argument was given.
    pub fn restore_session(&mut self) {
        let root = match self.session_project_root() {
            Some(r) => r,
            None => return,
        };
        let path = session::session_path(&root);
        let session = match session::load_session(&path) {
            Some(s) => s,
            None => return,
        };

        // Only restore if there are file-backed tabs to reopen.
        let file_tabs: Vec<&TabState> = session
            .tabs
            .iter()
            .filter(|t| t.file_path.is_some())
            .collect();
        if file_tabs.is_empty() {
            return;
        }

        // Open each saved tab, skipping files that no longer exist.
        let mut opened = false;
        for tab_state in &session.tabs {
            let file_path = match &tab_state.file_path {
                Some(p) if p.exists() => p,
                _ => continue,
            };
            let buffer = match Buffer::from_file(file_path.to_str().unwrap_or_default()) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!("Session restore: could not open {:?}: {}", file_path, e);
                    continue;
                }
            };
            let mut tab = EditorTab::new(
                buffer,
                self.conversation_store.as_ref(),
                &self.theme,
            );
            // Restore cursor and scroll, clamped to buffer bounds.
            let max_row = tab.buffer.line_count().saturating_sub(1);
            tab.cursor.row = tab_state.cursor_row.min(max_row);
            let line_len = tab
                .buffer
                .line_text(tab.cursor.row)
                .map(|l| l.trim_end_matches('\n').len())
                .unwrap_or(0);
            tab.cursor.col = tab_state.cursor_col.min(line_len);
            tab.scroll_row = tab_state.scroll_row.min(max_row);
            tab.scroll_col = tab_state.scroll_col;

            if !opened {
                // Replace the initial scratch tab with the first restored tab.
                self.tabs = TabManager::new(tab);
                opened = true;
            } else {
                self.tabs.open(tab);
            }
        }

        if !opened {
            return;
        }

        // Restore active tab index.
        let max_idx = self.tabs.count().saturating_sub(1);
        self.tabs.switch_to(session.active_tab.min(max_idx));

        // Restore UI state.
        self.file_tree.visible = session.ui.file_tree_visible;
        self.chat_panel.visible = session.ui.chat_panel_visible;
        self.terminal.visible = session.ui.terminal_visible;
        self.sidebar_view = match session.ui.sidebar_view.as_str() {
            "git" => SidebarView::Git,
            _ => SidebarView::Files,
        };

        self.set_status(format!(
            "Session restored ({} tab{})",
            self.tabs.count(),
            if self.tabs.count() == 1 { "" } else { "s" }
        ));
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
            Mode::Diff => crate::input::handle_diff(self, code, modifiers),
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
                        if let Err(e) = store.add_message(
                            conv_id,
                            MessageRole::AiResponse,
                            &full_text,
                            Some("claude"),
                        ) {
                            tracing::warn!("Failed to log AI response: {e}");
                        }
                    } else {
                        tracing::warn!(
                            "Cannot log AI response: store={} conv={}",
                            self.conversation_store.is_some(),
                            self.active_conversation.is_some()
                        );
                    }
                    if let Some(proposal) = &mut self.proposal {
                        proposal.proposed_text = full_text;
                        proposal.streaming = false;
                    }
                    self.ai_receiver = None;
                    // Refresh history panel so the new conversation appears.
                    self.refresh_conversation_history();
                    // In experimental mode, auto-accept without user review.
                    if self.experimental_mode {
                        self.mode = Mode::Review;
                        self.accept_proposal();
                        self.set_status("[EXPERIMENT] AI proposal auto-accepted");
                    } else {
                        self.mode = Mode::Review;
                        self.set_status("AI proposal ready — a: accept, r: reject, Esc: cancel");
                    }
                    return;
                }
                Ok(AiEvent::Error(err)) => {
                    self.ai_receiver = None;
                    self.proposal = None;
                    self.mode = Mode::Normal;
                    self.set_status(format!("AI error: {err}"));
                    return;
                }
                // Tool use / activity events are only relevant for chat; ignore here.
                Ok(AiEvent::ToolUse { .. })
                | Ok(AiEvent::ToolUseComplete { .. })
                | Ok(AiEvent::Activity(_)) => {}
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.ai_receiver = None;
                    if let Some(proposal) = &mut self.proposal {
                        if !proposal.proposed_text.is_empty() {
                            // Save the AI response that was accumulated via Token
                            // events — the Done event may not have been delivered
                            // before the sender dropped.
                            if let (Some(store), Some(conv_id)) =
                                (&self.conversation_store, &self.active_conversation)
                            {
                                if let Err(e) = store.add_message(
                                    conv_id,
                                    MessageRole::AiResponse,
                                    &proposal.proposed_text,
                                    Some("claude"),
                                ) {
                                    tracing::warn!("Failed to log AI response on disconnect: {e}");
                                }
                            }
                            proposal.streaming = false;
                            self.mode = Mode::Review;
                            self.refresh_conversation_history();
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
        if self.ai_client.is_none() {
            self.set_status("No AI backend available. Set ANTHROPIC_API_KEY or install Claude Code CLI");
            self.mode = Mode::Normal;
            return;
        }

        // Ensure conversation store is ready before we borrow ai_client.
        self.ensure_conversation_store();

        let client = self.ai_client.as_ref().unwrap();

        // Build context with semantic info and LSP diagnostics.
        let selection = self.visual_selection_range();
        let semantic = self.semantic_context_for_ai();
        let tab = self.tab();
        let mut ctx =
            EditorContext::from_buffer_with_semantic(&tab.buffer, &tab.cursor, selection, semantic);
        // Propagate the context window limit from the client config.
        ctx.max_tokens = client.max_context_tokens();

        // Attach current diagnostics so the AI is aware of errors/warnings.
        ctx.diagnostics = tab
            .diagnostics
            .iter()
            .map(|d| aura_ai::DiagnosticSummary {
                line: d.range.start.line as usize + 1,
                severity: match d.severity {
                    Some(1) => "error".to_string(),
                    Some(2) => "warning".to_string(),
                    Some(3) => "info".to_string(),
                    _ => "hint".to_string(),
                },
                message: d.message.clone(),
            })
            .collect();

        // Determine the range to replace.
        let (start, end) = if let Some((s, e)) = selection {
            (s, e)
        } else {
            // Default: the current line.
            let line_start = tab
                .buffer
                .cursor_to_char_idx(&Cursor::new(tab.cursor.row, 0));
            let line_end = tab
                .buffer
                .line(tab.cursor.row)
                .map(|l| line_start + l.len_chars())
                .unwrap_or(line_start);
            (line_start, line_end)
        };

        let original_text = if start < end && end <= tab.buffer.len_chars() {
            tab.buffer.rope().slice(start..end).to_string()
        } else {
            String::new()
        };

        let system = ctx.to_system_prompt();
        let messages = vec![Message::text("user", intent)];

        // Log intent to conversation store.
        let file_path_str = tab
            .buffer
            .file_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let start_line = tab.buffer.char_idx_to_cursor(start).row;
        let end_line = tab.buffer.char_idx_to_cursor(end).row;

        let (branch, commit) = self.git_context();
        if let Some(store) = &self.conversation_store {
            match store.create_conversation(&file_path_str, start_line, end_line, commit.as_deref(), branch.as_deref()) {
                Ok(conv) => {
                    if let Err(e) =
                        store.add_message(&conv.id, MessageRole::HumanIntent, intent, None)
                    {
                        tracing::warn!("Failed to log human intent message: {e}");
                    }
                    let intent_rec = store
                        .record_intent(&conv.id, intent, &file_path_str, start_line, end_line)
                        .ok();
                    self.active_intent_id = intent_rec.map(|i| i.id);
                    self.active_conversation = Some(conv.id.clone());
                    tracing::info!(
                        "Created conversation {} for '{}'",
                        conv.id,
                        file_path_str
                    );
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to create conversation for '{}': {e}",
                        file_path_str
                    );
                }
            }
        } else {
            tracing::warn!("No conversation store — AI history will not be saved");
        }

        // Compute token estimate for the status bar display.
        let token_count = estimate_tokens(&system);
        let token_display = if token_count >= 1_000 {
            format!("{:.1}K", token_count as f64 / 1_000.0)
        } else {
            token_count.to_string()
        };

        let rx = client.stream_completion(&system, messages);
        self.ai_receiver = Some(rx);
        self.proposal = Some(AiProposal {
            proposed_text: String::new(),
            original_text,
            start,
            end,
            streaming: true,
        });
        self.set_status(format!("AI thinking... (~{token_display} tokens)"));
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
            let tab = self.tab_mut();
            if proposal.start < proposal.end {
                tab.buffer
                    .delete(proposal.start, proposal.end, author.clone());
            }
            tab.buffer
                .insert(proposal.start, &proposal.proposed_text, author);
            tab.cursor = tab
                .buffer
                .char_idx_to_cursor(proposal.start + proposal.proposed_text.len());
            self.clamp_cursor();
            self.mark_highlights_dirty();
            self.refresh_conversation_lines();
            self.set_status("AI proposal accepted");
        }
        self.mode = Mode::Normal;
        self.refresh_conversation_history();
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
        self.refresh_conversation_history();
        self.set_status("AI proposal rejected");
    }

    /// Check if AI is available.
    pub fn has_ai(&self) -> bool {
        self.ai_client.is_some()
    }

    /// Mark syntax highlights and semantic index as stale (call after any buffer edit).
    pub fn mark_highlights_dirty(&mut self) {
        self.tab_mut().mark_highlights_dirty();
        let cursor_row = self.tab().cursor.row;
        if let Some(engine) = &mut self.speculative {
            engine.buffer_edited(cursor_row);
        }
        if let Some(repo) = &mut self.git_repo {
            repo.invalidate_status();
        }
    }

    /// Regenerate syntax highlights from the current buffer content.
    pub fn refresh_highlights(&mut self) {
        let theme = self.theme.clone();
        self.tab_mut().refresh_highlights(&theme);
    }

    /// Rebuild the semantic index from the current buffer.
    pub fn refresh_semantic_index(&mut self) {
        self.tab_mut().refresh_semantic_index();
    }

    /// Update cached semantic info for the symbol at the current cursor.
    pub fn update_semantic_context(&mut self) {
        let tab = self.tab_mut();
        tab.semantic_info = None;
        if let Some(indexer) = &tab.semantic_indexer {
            let path = tab
                .buffer
                .file_path()
                .map(|p| p.to_path_buf())
                .unwrap_or_default();
            if let Some((id, _)) = indexer.graph().symbol_at(&path, tab.cursor.row) {
                tab.semantic_info = indexer.graph().context_string(id);
            }
        }
    }

    /// Get impact summary for a range of lines (used during AI proposal review).
    pub fn impact_summary(&self, start_line: usize, end_line: usize) -> Option<String> {
        let tab = self.tab();
        let indexer = tab.semantic_indexer.as_ref()?;
        let path = tab.buffer.file_path()?.to_path_buf();
        indexer.graph().impact_summary(&path, start_line, end_line)
    }

    /// Get semantic context string for the AI, optionally including the
    /// Tree-sitter node kind at the cursor position.
    pub fn semantic_context_for_ai(&self) -> Option<String> {
        let tab = self.tab();
        // Collect symbol-level context from the semantic graph.
        let symbol_ctx = tab.semantic_indexer.as_ref().and_then(|indexer| {
            let path = tab.buffer.file_path()?.to_path_buf();
            let (id, _) = indexer.graph().symbol_at(&path, tab.cursor.row)?;
            indexer.graph().context_string(id)
        });

        // Include the Tree-sitter node kind at cursor if a highlighter is available.
        let node_type_ctx = tab.highlighter.as_ref().and_then(|hl| {
            // Compute byte offset for current cursor position.
            let source = tab.buffer.rope().to_string();
            let char_idx = tab.buffer.cursor_to_char_idx(&tab.cursor);
            // Convert char index to byte offset.
            let byte_off = source
                .char_indices()
                .nth(char_idx)
                .map(|(b, _)| b)
                .unwrap_or(source.len());
            let kind = hl.node_type_at_byte(byte_off)?;
            Some(format!("Syntax node at cursor: {kind}"))
        });

        match (symbol_ctx, node_type_ctx) {
            (Some(sym), Some(node)) => Some(format!("{sym}\n{node}")),
            (Some(sym), None) => Some(sym),
            (None, Some(node)) => Some(node),
            (None, None) => None,
        }
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
            let tab = self.tab();
            let file_path = tab
                .buffer
                .file_path()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let start_line = tab
                .buffer
                .char_idx_to_cursor(start.min(tab.buffer.len_chars()))
                .row;
            let end_line = tab
                .buffer
                .char_idx_to_cursor(end.min(tab.buffer.len_chars()))
                .row;
            let (branch, commit) = self.git_context();
            if let Err(e) = store.log_decision(
                conv_id,
                self.active_intent_id.as_deref(),
                decision,
                original,
                proposed,
                &file_path,
                start_line,
                end_line,
                commit.as_deref(),
                branch.as_deref(),
            ) {
                tracing::warn!("Failed to log edit decision: {e}");
            }
        }
    }

    /// Refresh the cached list of lines with conversation history.
    fn refresh_conversation_lines(&mut self) {
        let conv_lines = self
            .conversation_store
            .as_ref()
            .and_then(|store| {
                let fp = self.tab().buffer.file_path()?.display().to_string();
                store.lines_with_conversations(&fp).ok()
            })
            .unwrap_or_default();
        self.tab_mut().conversation_lines = conv_lines;
    }

    /// Show conversation history for the line at cursor.
    pub fn show_conversation_at_cursor(&mut self) {
        let file_path = self
            .tab()
            .buffer
            .file_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let cursor_row = self.tab().cursor.row;

        if let Some(store) = &self.conversation_store {
            match store.conversation_for_code(&file_path, cursor_row) {
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

    /// Show the undo history tree in the conversation panel.
    ///
    /// Builds a text representation of the full edit history (including redo
    /// entries) and displays it using the existing [`ConversationPanel`]
    /// mechanism so the user can scroll through it.  The panel is closed with
    /// `q` or `Esc` just like the conversation history panel.
    pub fn show_undo_tree(&mut self) {
        use aura_core::conversation::{ConversationMessage, MessageRole};
        let tree_text = self.tab().buffer.undo_tree_text();
        let history_len = self.tab().buffer.history().len();
        let total = tree_text.lines().count();
        // Wrap the tree text as a single System message so the renderer can
        // display it verbatim inside the ConversationPanel.
        let message = ConversationMessage {
            id: "undo-tree".to_string(),
            conversation_id: "undo-tree".to_string(),
            role: MessageRole::System,
            content: tree_text,
            created_at: String::new(),
            model: None,
        };
        self.conversation_panel = Some(ConversationPanel {
            messages: vec![message],
            file_info: format!(
                "Undo tree — {} active edit{}, {} total",
                history_len,
                if history_len == 1 { "" } else { "s" },
                total,
            ),
            scroll: 0,
        });
        self.set_status("Undo tree — Esc/q to close, j/k to scroll");
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
        self.tab()
            .conversation_lines
            .iter()
            .any(|(start, end)| line >= *start && line <= *end)
    }

    /// Poll the LSP client for events.
    fn poll_lsp_events(&mut self) {
        let events = self.tab_mut().poll_lsp_events();
        if events.is_empty() {
            return;
        }

        for event in events {
            match event {
                LspEvent::Initialized => {
                    self.set_status("LSP server ready");
                }
                LspEvent::Diagnostics(diags) => {
                    self.tab_mut().diagnostics = diags;
                }
                LspEvent::Definition(locations) => {
                    if let Some(loc) = locations.first() {
                        // Jump to the definition location.
                        let target_row = loc.range.start.line as usize;
                        let target_col = loc.range.start.character as usize;

                        // Check if it's in the same file.
                        let current_uri = self
                            .tab()
                            .buffer
                            .file_path()
                            .map(|p| format!("file://{}", p.display()))
                            .unwrap_or_default();

                        if loc.uri == current_uri {
                            let tab = self.tab_mut();
                            tab.cursor.row = target_row;
                            tab.cursor.col = target_col;
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
                            self.tab_mut().hover_info = None;
                        } else {
                            self.tab_mut().hover_info = Some(text);
                        }
                    } else {
                        self.tab_mut().hover_info = None;
                        self.set_status("No hover info");
                    }
                }
                LspEvent::CodeActions(actions) => {
                    if actions.is_empty() {
                        self.set_status("No code actions available");
                    } else {
                        // Display up to three actions in the status bar and,
                        // if the AI is available, feed them as context for a
                        // quick-fix intent.
                        let titles: Vec<&str> =
                            actions.iter().take(3).map(|a| a.title.as_str()).collect();
                        let summary = titles.join(" | ");
                        self.set_status(format!("Code actions: {summary}"));

                        if self.has_ai() {
                            let action_list = actions
                                .iter()
                                .map(|a| format!("- {}", a.title))
                                .collect::<Vec<_>>()
                                .join("\n");
                            let prompt = format!(
                                "The following code actions are available at the cursor:\n\
                                 {action_list}\n\n\
                                 Apply the most appropriate action to fix or improve the code. \
                                 Output only the corrected code."
                            );
                            self.send_intent(&prompt);
                        }
                    }
                }
                LspEvent::ServerError(e) => {
                    tracing::warn!("LSP server error: {}", e);
                    self.tab_mut().lsp_client = None;
                    self.set_status(format!("LSP error: {e}"));
                }
            }
        }
    }

    /// Send a didChange notification with the current buffer content.
    fn send_lsp_did_change(&mut self) {
        self.tab_mut().send_lsp_did_change();
    }

    /// Request go-to-definition at the current cursor position.
    pub fn lsp_goto_definition(&mut self) {
        let tab = self.tab_mut();
        let row = tab.cursor.row as u32;
        let col = tab.cursor.col as u32;
        if let Some(client) = &mut tab.lsp_client {
            client.goto_definition(row, col);
            self.set_status("Looking up definition...");
        } else {
            self.set_status("No LSP server");
        }
    }

    /// Request hover info at the current cursor position.
    pub fn lsp_hover(&mut self) {
        let row = self.tab().cursor.row as u32;
        let col = self.tab().cursor.col as u32;
        if let Some(client) = &mut self.tab_mut().lsp_client {
            client.hover(row, col);
        } else {
            self.set_status("No LSP server");
        }
    }

    /// Request code actions from the LSP server at the cursor position.
    ///
    /// Diagnostics on the current line are forwarded as context so the server
    /// can offer targeted quick-fixes. The result is handled asynchronously
    /// in [`poll_lsp_events`] via [`lsp::LspEvent::CodeActions`].
    pub fn lsp_request_code_actions(&mut self) {
        let tab = self.tab();
        let row = tab.cursor.row as u32;
        let col = tab.cursor.col as u32;

        // Serialise diagnostics on this line into the LSP JSON format.
        let diag_json: Vec<serde_json::Value> = tab
            .diagnostics
            .iter()
            .filter(|d| d.range.start.line == row)
            .map(|d| {
                serde_json::json!({
                    "range": {
                        "start": { "line": d.range.start.line, "character": d.range.start.character },
                        "end":   { "line": d.range.end.line,   "character": d.range.end.character }
                    },
                    "severity": d.severity,
                    "message": d.message
                })
            })
            .collect();

        if let Some(client) = &mut self.tab_mut().lsp_client {
            client.request_code_actions(row, col, &diag_json);
            self.set_status("Requesting code actions...");
        } else {
            self.set_status("No LSP server");
        }
    }

    /// Check if an LSP client is active.
    pub fn has_lsp(&self) -> bool {
        self.tab().lsp_client.is_some()
    }

    /// Get diagnostics for a specific line.
    pub fn line_diagnostics(&self, line: usize) -> Option<&Diagnostic> {
        self.tab()
            .diagnostics
            .iter()
            .find(|d| d.range.start.line as usize == line)
    }

    /// Count errors and warnings.
    pub fn diagnostic_counts(&self) -> (usize, usize) {
        let tab = self.tab();
        let errors = tab.diagnostics.iter().filter(|d| d.is_error()).count();
        let warnings = tab.diagnostics.iter().filter(|d| d.is_warning()).count();
        (errors, warnings)
    }

    /// Jump to the next diagnostic after the current cursor position.
    pub fn next_diagnostic(&mut self) {
        let tab = self.tab();
        let cursor_row = tab.cursor.row;
        let target = tab
            .diagnostics
            .iter()
            .find(|d| d.range.start.line as usize > cursor_row)
            .or_else(|| tab.diagnostics.first())
            .map(|d| {
                (
                    d.range.start.line as usize,
                    d.range.start.character as usize,
                    d.message.clone(),
                )
            });

        if let Some((row, col, msg)) = target {
            let tab = self.tab_mut();
            tab.cursor.row = row;
            tab.cursor.col = col;
            self.clamp_cursor();
            self.set_status(msg);
        } else {
            self.set_status("No diagnostics");
        }
    }

    /// Jump to the previous diagnostic before the current cursor position.
    pub fn prev_diagnostic(&mut self) {
        let tab = self.tab();
        let cursor_row = tab.cursor.row;
        let target = tab
            .diagnostics
            .iter()
            .rev()
            .find(|d| (d.range.start.line as usize) < cursor_row)
            .or_else(|| tab.diagnostics.last())
            .map(|d| {
                (
                    d.range.start.line as usize,
                    d.range.start.character as usize,
                    d.message.clone(),
                )
            });

        if let Some((row, col, msg)) = target {
            let tab = self.tab_mut();
            tab.cursor.row = row;
            tab.cursor.col = col;
            self.clamp_cursor();
            self.set_status(msg);
        } else {
            self.set_status("No diagnostics");
        }
    }

    /// Clamp cursor to valid buffer positions.
    pub fn clamp_cursor(&mut self) {
        let mode = self.mode;
        let tab = self.tab_mut();
        let max_row = tab.buffer.line_count().saturating_sub(1);
        tab.cursor.row = tab.cursor.row.min(max_row);

        if let Some(line) = tab.buffer.line(tab.cursor.row) {
            let line_len = line.len_chars();
            let max_col = if mode == Mode::Insert {
                line_len
            } else {
                line_len.saturating_sub(1)
            };
            tab.cursor.col = tab.cursor.col.min(max_col);
        }
    }

    /// Update the matching bracket position for the character under the cursor.
    pub fn update_matching_bracket(&mut self) {
        let tab = self.tab();
        let char_idx = tab.buffer.cursor_to_char_idx(&tab.cursor);
        self.matching_bracket = tab
            .buffer
            .find_matching_bracket(char_idx)
            .map(|match_idx| {
                let cursor = self.tab().buffer.char_idx_to_cursor(match_idx);
                (cursor.row, cursor.col)
            });
    }

    /// Populate search_matches from the current buffer using search_query.
    pub fn execute_search(&mut self) {
        if let Some(ref query) = self.search_query {
            let matches = self.tab().buffer.find_all(query);
            self.search_matches = matches;
            // Find the match nearest to (and at or after) the cursor.
            if !self.search_matches.is_empty() {
                let cursor_char = self.tab().buffer.cursor_to_char_idx(&self.tab().cursor);
                self.search_current = self
                    .search_matches
                    .iter()
                    .position(|&(s, _)| s >= cursor_char)
                    .unwrap_or(0);
            }
        } else {
            self.search_matches.clear();
            self.search_current = 0;
        }
    }

    /// Jump to the next search match from the current position.
    pub fn search_next(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        // Find the next match after the cursor.
        let cursor_char = self.tab().buffer.cursor_to_char_idx(&self.tab().cursor);
        if let Some(pos) = self
            .search_matches
            .iter()
            .position(|&(s, _)| s > cursor_char)
        {
            self.search_current = pos;
        } else {
            // Wrap around.
            self.search_current = 0;
        }
        let (start, _) = self.search_matches[self.search_current];
        let new_cursor = self.tab().buffer.char_idx_to_cursor(start);
        self.tab_mut().cursor = new_cursor;
        self.clamp_cursor();
    }

    /// Jump to the previous search match from the current position.
    pub fn search_prev(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        let cursor_char = self.tab().buffer.cursor_to_char_idx(&self.tab().cursor);
        if let Some(pos) = self
            .search_matches
            .iter()
            .rposition(|&(s, _)| s < cursor_char)
        {
            self.search_current = pos;
        } else {
            // Wrap around.
            self.search_current = self.search_matches.len().saturating_sub(1);
        }
        let (start, _) = self.search_matches[self.search_current];
        let new_cursor = self.tab().buffer.char_idx_to_cursor(start);
        self.tab_mut().cursor = new_cursor;
        self.clamp_cursor();
    }

    /// Clear all search state.
    pub fn clear_search(&mut self) {
        self.search_query = None;
        self.search_input.clear();
        self.search_active = false;
        self.search_matches.clear();
        self.search_current = 0;
    }

    /// Ensure the cursor is visible within the viewport.
    pub fn scroll_to_cursor(&mut self, viewport_height: usize, viewport_width: usize) {
        let margin = self.config.editor.scroll_margin;
        let tab = self.tab_mut();
        if tab.cursor.row < tab.scroll_row + margin {
            tab.scroll_row = tab.cursor.row.saturating_sub(margin);
        }
        if tab.cursor.row >= tab.scroll_row + viewport_height - margin {
            tab.scroll_row = tab.cursor.row + margin - viewport_height + 1;
        }
        if tab.cursor.col < tab.scroll_col {
            tab.scroll_col = tab.cursor.col;
        }
        if tab.cursor.col >= tab.scroll_col + viewport_width {
            tab.scroll_col = tab.cursor.col - viewport_width + 1;
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

        let mut needs_history_refresh = false;
        for req in requests {
            // Track whether this action may have modified conversation history.
            let modifies_history = matches!(
                &req.action,
                McpAction::ReadBuffer { .. }
                    | McpAction::EditBuffer { .. }
                    | McpAction::LogConversation { .. }
                    | McpAction::RegisterAgent { .. }
                    | McpAction::RegisterAgentWithRole { .. }
            );
            let response = self.handle_mcp_action(&req.action);
            if modifies_history && response.success {
                needs_history_refresh = true;
            }
            let _ = req.response_tx.send(response);
        }
        if needs_history_refresh {
            self.refresh_conversation_history();
        }
    }

    /// Handle a single MCP action and produce a response.
    fn handle_mcp_action(&mut self, action: &McpAction) -> McpAppResponse {
        match action {
            McpAction::ReadBuffer {
                start_line,
                end_line,
            } => {
                let tab = self.tab();
                let total = tab.buffer.line_count();
                let start = start_line.unwrap_or(0).min(total);
                let end = end_line.unwrap_or(total).min(total);

                let mut lines = Vec::new();
                for i in start..end {
                    if let Some(text) = tab.buffer.line_text(i) {
                        lines.push(text);
                    }
                }

                let file_path = tab.buffer.file_path().map(|p| p.display().to_string());

                // Automatically include selection context when present.
                let selection = self.visual_selection_range().map(|(s, e)| {
                    let tab = self.tab();
                    let text = tab.buffer.rope().slice(s..e).to_string();
                    let start_cursor = tab.buffer.char_idx_to_cursor(s);
                    let end_cursor = tab.buffer.char_idx_to_cursor(e);
                    serde_json::json!({
                        "text": text,
                        "start": { "line": start_cursor.row, "col": start_cursor.col },
                        "end": { "line": end_cursor.row, "col": end_cursor.col },
                    })
                });

                let response = McpAppResponse {
                    success: true,
                    data: serde_json::json!({
                        "content": lines.join(""),
                        "start_line": start,
                        "end_line": end,
                        "total_lines": total,
                        "file_path": file_path,
                        "selection": selection,
                    }),
                };

                // Auto-create a conversation if none is active, so MCP reads
                // are tracked in the AI History panel.
                if self.active_conversation.is_none() {
                    self.ensure_mcp_conversation(start, end.saturating_sub(1));
                }

                response
            }
            McpAction::EditBuffer {
                start_line,
                start_col,
                end_line,
                end_col,
                text,
                agent_id,
            } => {
                // Track the agent edit and include role in the status message.
                self.agent_registry.record_edit(agent_id);
                let agent_role = self
                    .agent_registry
                    .agent_role(agent_id)
                    .map(|r| r.to_string());

                let author = AuthorId::ai(agent_id.clone());
                let tab = self.tab_mut();
                let start_cursor = Cursor::new(*start_line, *start_col);
                let start_idx = tab.buffer.cursor_to_char_idx(&start_cursor);

                if let (Some(el), Some(ec)) = (end_line, end_col) {
                    // Replace range.
                    let end_cursor = Cursor::new(*el, *ec);
                    let end_idx = tab.buffer.cursor_to_char_idx(&end_cursor);
                    if start_idx < end_idx && end_idx <= tab.buffer.len_chars() {
                        tab.buffer.delete(start_idx, end_idx, author.clone());
                    }
                }

                tab.buffer.insert(start_idx, text, author);
                self.mark_highlights_dirty();

                // Auto-log this edit as a conversation entry.
                if let Some(store) = &self.conversation_store {
                    let file_path = self
                        .tab()
                        .buffer
                        .file_path()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "<scratch>".to_string());
                    let end_l = end_line.unwrap_or(*start_line);
                    let conv = store
                        .conversations_for_range(&file_path, *start_line, end_l)
                        .inspect_err(|e| {
                            tracing::warn!("Failed to query conversations for range: {e}")
                        })
                        .ok()
                        .and_then(|v| v.into_iter().next())
                        .or_else(|| {
                            let (branch, commit) = self.git_context();
                            store
                                .create_conversation(&file_path, *start_line, end_l, commit.as_deref(), branch.as_deref())
                                .inspect_err(|e| {
                                    tracing::error!("Failed to create MCP conversation: {e}")
                                })
                                .ok()
                        });
                    if let Some(c) = conv {
                        let preview = if text.len() > 200 {
                            format!("{}...", &text[..200])
                        } else {
                            text.to_string()
                        };
                        let content = format!(
                            "[{}] Edited lines {}-{}: {}",
                            agent_id, start_line, end_l, preview
                        );
                        if let Err(e) = store.add_message(
                            &c.id,
                            MessageRole::AiResponse,
                            &content,
                            Some(agent_id),
                        ) {
                            tracing::warn!("Failed to log MCP edit message: {e}");
                        }
                    }
                } else {
                    tracing::warn!("No conversation store for MCP edit logging");
                }

                // Show agent edit in status bar, including role if available.
                let status_msg = if let Some(role) = &agent_role {
                    format!("Agent '{}' [{}] edited buffer", agent_id, role)
                } else {
                    format!("Agent '{}' edited buffer", agent_id)
                };
                self.set_status(status_msg);

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
                    .tab()
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
                    let tab = self.tab();
                    let text = tab.buffer.rope().slice(s..e).to_string();
                    let start_cursor = tab.buffer.char_idx_to_cursor(s);
                    let end_cursor = tab.buffer.char_idx_to_cursor(e);
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
                let tab = self.tab();
                let line_text = tab.buffer.line_text(tab.cursor.row).unwrap_or_default();
                let cursor_row = tab.cursor.row;
                let cursor_col = tab.cursor.col;
                let file_path = tab.buffer.file_path().map(|p| p.display().to_string());
                let total_lines = tab.buffer.line_count();
                let semantic = self.semantic_context_for_ai();

                // Automatically include selection context when present.
                let selection = self.visual_selection_range().map(|(s, e)| {
                    let tab = self.tab();
                    let text = tab.buffer.rope().slice(s..e).to_string();
                    let start_cursor = tab.buffer.char_idx_to_cursor(s);
                    let end_cursor = tab.buffer.char_idx_to_cursor(e);
                    serde_json::json!({
                        "text": text,
                        "start": { "line": start_cursor.row, "col": start_cursor.col },
                        "end": { "line": end_cursor.row, "col": end_cursor.col },
                    })
                });

                McpAppResponse {
                    success: true,
                    data: serde_json::json!({
                        "cursor": {
                            "line": cursor_row,
                            "col": cursor_col,
                        },
                        "mode": self.mode.label(),
                        "current_line": line_text,
                        "file_path": file_path,
                        "total_lines": total_lines,
                        "semantic_context": semantic,
                        "selection": selection,
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

                let tab = self.tab();
                let file_path = tab
                    .buffer
                    .file_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();

                let start = start_line.unwrap_or(0);
                let end = end_line.unwrap_or(tab.buffer.line_count());

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
                    self.log_agent_session(name, None);
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
            McpAction::RegisterAgentWithRole { name, role } => {
                let registered = self.agent_registry.register_with_role(name, role.clone());
                if registered {
                    let role_str = role.as_deref().unwrap_or("unassigned");
                    self.set_status(format!("Agent '{}' connected (role: {})", name, role_str));
                    self.log_agent_session(name, role.as_deref());
                }
                McpAppResponse {
                    success: registered,
                    data: serde_json::json!({
                        "registered": registered,
                        "agent_id": name,
                        "role": role,
                        "total_agents": self.agent_registry.count(),
                    }),
                }
            }
            McpAction::AssignRole { name, role } => {
                let assigned = self.agent_registry.assign_role(name, role.clone());
                if assigned {
                    self.set_status(format!("Agent '{}' assigned role: {}", name, role));
                }
                McpAppResponse {
                    success: assigned,
                    data: serde_json::json!({
                        "assigned": assigned,
                        "agent_id": name,
                        "role": role,
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
                            "role": a.role,
                        })
                    })
                    .collect();

                McpAppResponse {
                    success: true,
                    data: serde_json::json!({
                        "agents": agents,
                        "total": agents.len(),
                    }),
                }
            }
            McpAction::GetBufferInfo => {
                let tab = self.tab();
                McpAppResponse {
                    success: true,
                    data: serde_json::json!({
                        "file_path": tab.buffer.file_path().map(|p| p.display().to_string()),
                        "line_count": tab.buffer.line_count(),
                        "char_count": tab.buffer.len_chars(),
                        "modified": tab.buffer.is_modified(),
                        "language": tab.language.map(|l| format!("{:?}", l)),
                        "has_lsp": self.has_lsp(),
                        "connected_agents": self.agent_registry.count(),
                    }),
                }
            }
            McpAction::LogConversation {
                agent_id,
                message,
                role,
                context,
                line_start,
                line_end,
            } => {
                self.ensure_conversation_store();
                let store = match &self.conversation_store {
                    Some(s) => s,
                    None => {
                        return McpAppResponse {
                            success: false,
                            data: serde_json::json!({ "error": "No conversation store available" }),
                        };
                    }
                };

                let file_path = self
                    .tab()
                    .buffer
                    .file_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<scratch>".to_string());

                let start = line_start.unwrap_or(0);
                let end = line_end.unwrap_or(self.tab().buffer.line_count().saturating_sub(1));

                let msg_role = match role.as_str() {
                    "human_intent" => MessageRole::HumanIntent,
                    "system" => MessageRole::System,
                    _ => MessageRole::AiResponse,
                };

                // Create or find a conversation for this range.
                let conv = store
                    .conversations_for_range(&file_path, start, end)
                    .ok()
                    .and_then(|v| v.into_iter().next())
                    .or_else(|| {
                        let (branch, commit) = self.git_context();
                        store.create_conversation(&file_path, start, end, commit.as_deref(), branch.as_deref()).ok()
                    });

                match conv {
                    Some(c) => {
                        let content = if let Some(ctx) = context {
                            format!("[{}] {}\n\nContext: {}", agent_id, message, ctx)
                        } else {
                            format!("[{}] {}", agent_id, message)
                        };
                        match store.add_message(&c.id, msg_role, &content, Some(agent_id)) {
                            Ok(msg) => {
                                self.set_status(format!("Logged conversation from '{}'", agent_id));
                                McpAppResponse {
                                    success: true,
                                    data: serde_json::json!({
                                        "conversation_id": c.id,
                                        "message_id": msg.id,
                                    }),
                                }
                            }
                            Err(e) => McpAppResponse {
                                success: false,
                                data: serde_json::json!({ "error": e.to_string() }),
                            },
                        }
                    }
                    None => McpAppResponse {
                        success: false,
                        data: serde_json::json!({ "error": "Failed to create conversation" }),
                    },
                }
            }
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

    /// Poll the background update checker for results.
    fn poll_update_check(&mut self) {
        if let Some(ref rx) = self.update_receiver {
            if let Ok(status) = rx.try_recv() {
                if let UpdateStatus::Available { ref version, .. } = status {
                    self.set_status(format!(
                        "AURA v{} available \u{2014} :update for details",
                        version
                    ));
                }
                self.update_status = Some(status);
                // Drop the receiver — we only need one result per session.
                self.update_receiver = None;
            }
        }
    }

    /// Poll the speculative engine for completed analyses.
    fn poll_speculative(&mut self) {
        if let Some(engine) = &mut self.speculative {
            engine.poll_events();
        }
    }

    /// Trigger background analysis if the cursor has been idle long enough.
    fn maybe_trigger_analysis(&mut self) {
        let should = self
            .speculative
            .as_ref()
            .map(|e| e.should_analyze())
            .unwrap_or(false);

        if !should {
            return;
        }

        // Gather diagnostic messages for context.
        let tab = self.tab();
        let diag_msgs: Vec<String> = tab
            .diagnostics
            .iter()
            .map(|d| format!("line {}: {}", d.range.start.line + 1, d.message))
            .collect();
        let cursor = tab.cursor;

        let semantic = self.semantic_context_for_ai();

        // We need to borrow speculative mutably and tab immutably at the same
        // time.  Since they live in different fields of `self`, we split the
        // borrows by accessing `self.tabs` and `self.speculative` directly.
        if let Some(engine) = &mut self.speculative {
            engine.analyze(&self.tabs.active().buffer, &cursor, semantic, &diag_msgs);
        }
    }

    /// Notify the speculative engine that the cursor moved.
    pub fn notify_cursor_moved(&mut self) {
        let cursor = self.tab().cursor;
        if let Some(engine) = &mut self.speculative {
            engine.cursor_moved(&cursor);
        }
    }

    /// Get the current ghost suggestion for rendering.
    pub fn current_ghost_suggestion(&self) -> Option<&GhostSuggestion> {
        self.speculative.as_ref()?.current_suggestion()
    }

    /// Cycle to the next ghost suggestion.
    pub fn next_ghost_suggestion(&mut self) {
        if let Some(engine) = &mut self.speculative {
            engine.next_suggestion();
        }
    }

    /// Cycle to the previous ghost suggestion.
    pub fn prev_ghost_suggestion(&mut self) {
        if let Some(engine) = &mut self.speculative {
            engine.prev_suggestion();
        }
    }

    /// Accept the current ghost suggestion and apply it to the buffer.
    pub fn accept_ghost_suggestion(&mut self) {
        let suggestion = self
            .speculative
            .as_mut()
            .and_then(|e| e.accept_suggestion());

        if let Some(suggestion) = suggestion {
            let author = AuthorId::ai("speculative");

            // Replace the region with the suggested text.
            let tab = self.tab_mut();
            let start_cursor = Cursor::new(suggestion.start_line, 0);
            let start_idx = tab.buffer.cursor_to_char_idx(&start_cursor);

            let end_cursor = Cursor::new(suggestion.end_line, 0);
            let end_idx = tab
                .buffer
                .cursor_to_char_idx(&end_cursor)
                .min(tab.buffer.len_chars());

            if start_idx < end_idx {
                tab.buffer.delete(start_idx, end_idx, author.clone());
            }
            tab.buffer.insert(start_idx, &suggestion.text, author);
            self.mark_highlights_dirty();

            self.set_status(format!(
                "Applied {} suggestion: {}",
                suggestion.category.label(),
                suggestion.explanation
            ));

            // Check for cross-file changes.
            self.trigger_cross_file_check();
        }
    }

    /// Dismiss current ghost suggestions.
    pub fn dismiss_ghost_suggestions(&mut self) {
        if let Some(engine) = &mut self.speculative {
            engine.dismiss_suggestions();
        }
    }

    /// Get ghost suggestion status text for the status bar.
    pub fn ghost_suggestion_status(&self) -> Option<String> {
        let engine = self.speculative.as_ref()?;
        if engine.is_analyzing() {
            return Some("analyzing...".to_string());
        }
        let suggestion = engine.current_suggestion()?;
        let total = engine.active_suggestions.len();
        let idx = engine.suggestion_index + 1;
        Some(format!(
            "[{}/{}] {}: {} — Tab: accept, Alt+]: next, Esc: dismiss",
            idx,
            total,
            suggestion.category.label(),
            suggestion.explanation
        ))
    }

    /// Toggle speculative aggressiveness.
    pub fn cycle_aggressiveness(&mut self) {
        if let Some(engine) = &mut self.speculative {
            engine.aggressiveness = engine.aggressiveness.next();
            let label = engine.aggressiveness.label().to_string();
            self.set_status(format!("AI suggestion level: {label}"));
        } else {
            self.set_status("Speculative AI not available (no API key)");
        }
    }

    /// Get current aggressiveness level.
    pub fn aggressiveness(&self) -> Option<Aggressiveness> {
        self.speculative.as_ref().map(|e| e.aggressiveness)
    }

    /// Trigger cross-file change check after accepting an edit.
    fn trigger_cross_file_check(&mut self) {
        // Find related files via semantic graph.
        let tab = self.tab();
        let related = tab
            .semantic_indexer
            .as_ref()
            .and_then(|indexer| {
                let path = tab.buffer.file_path()?.to_path_buf();
                let (id, _) = indexer.graph().symbol_at(&path, tab.cursor.row)?;
                let impact = indexer.graph().impact_of(id);
                // Collect unique file paths from callers and tests.
                let mut files: Vec<String> = Vec::new();
                let current = tab.buffer.file_path()?.display().to_string();
                for caller_id in &impact.direct_callers {
                    if let Some(sym) = indexer.graph().symbol(*caller_id) {
                        let fp = sym.file_path.display().to_string();
                        if fp != current && !files.contains(&fp) {
                            files.push(fp);
                        }
                    }
                }
                for test_id in &impact.affected_tests {
                    if let Some(sym) = indexer.graph().symbol(*test_id) {
                        let fp = sym.file_path.display().to_string();
                        if fp != current && !files.contains(&fp) {
                            files.push(fp);
                        }
                    }
                }
                if files.is_empty() {
                    None
                } else {
                    Some(files)
                }
            })
            .unwrap_or_default();

        if !related.is_empty() {
            let semantic = self.semantic_context_for_ai();
            let cursor = self.tabs.active().cursor;
            if let Some(engine) = &mut self.speculative {
                engine.propose_cross_file_changes(
                    &self.tabs.active().buffer,
                    &cursor,
                    semantic,
                    related,
                );
            }
        }
    }

    /// Check if a cross-file changeset is pending.
    pub fn has_pending_changeset(&self) -> bool {
        self.speculative
            .as_ref()
            .and_then(|e| e.pending_changeset.as_ref())
            .is_some()
    }

    /// Get pending changeset summary.
    pub fn changeset_summary(&self) -> Option<String> {
        let cs = self.speculative.as_ref()?.pending_changeset.as_ref()?;
        let files: Vec<&str> = cs.changes.iter().map(|c| c.file_path.as_str()).collect();
        Some(format!(
            "Cross-file changes: {} file(s) — {}",
            cs.changes.len(),
            files.join(", ")
        ))
    }

    /// Dismiss the pending changeset.
    pub fn dismiss_changeset(&mut self) {
        if let Some(engine) = &mut self.speculative {
            engine.pending_changeset = None;
        }
    }

    // --- Git integration ---

    /// Get line-level git diff status for the current file.
    pub fn git_line_status(&mut self, line: usize) -> Option<LineStatus> {
        let file_path = self.tab().buffer.file_path()?.to_path_buf();
        let repo = self.git_repo.as_mut()?;
        let status = repo.line_status(&file_path);
        status.get(&line).copied()
    }

    /// Get blame info for a specific line.
    pub fn git_blame_for_line(&mut self, line: usize) -> Option<crate::git::BlameEntry> {
        let file_path = self.tab().buffer.file_path()?.to_path_buf();
        let repo = self.git_repo.as_mut()?;
        let blame = repo.blame(&file_path);
        blame.get(line).and_then(|e| e.clone())
    }

    /// Check if git is available.
    pub fn has_git(&self) -> bool {
        self.git_repo.is_some()
    }

    /// Get the current branch name.
    pub fn git_branch(&self) -> Option<String> {
        self.git_repo.as_ref()?.current_branch()
    }

    /// Get the current git branch and commit hash for conversation context.
    fn git_context(&self) -> (Option<String>, Option<String>) {
        match &self.git_repo {
            Some(repo) => (repo.current_branch(), repo.head_short()),
            None => (None, None),
        }
    }

    /// Commit the current file with a message.
    pub fn git_commit(&self, message: &str) {
        let file_path = match self.tab().buffer.file_path() {
            Some(p) => p.to_path_buf(),
            None => {
                return;
            }
        };

        let repo = match &self.git_repo {
            Some(r) => r,
            None => return,
        };

        // Get conversation summary if available.
        let conv_summary = self.active_conversation.as_ref().and_then(|conv_id| {
            let store = self.conversation_store.as_ref()?;
            let msgs = store.messages_for_conversation(conv_id).ok()?;
            msgs.first()
                .map(|m| m.content.chars().take(100).collect::<String>())
        });

        match repo.commit_with_conversation(&file_path, message, conv_summary.as_deref()) {
            Ok(hash) => {
                tracing::info!("Committed: {hash}");
            }
            Err(e) => {
                tracing::warn!("Commit failed: {e}");
            }
        }
    }

    /// Generate an AI commit message for the current changes.
    pub fn generate_commit_message(&mut self) {
        let client = match &self.ai_client {
            Some(c) => c,
            None => {
                self.set_status("No API key for commit message generation");
                return;
            }
        };

        let diff_summary = self
            .git_repo
            .as_ref()
            .and_then(|repo| {
                let fp = self.tab().buffer.file_path()?;
                repo.diff_summary(fp).ok()
            })
            .unwrap_or_default();

        if diff_summary.trim().is_empty() {
            self.set_status("No staged changes to describe");
            return;
        }

        let file_path_str = self
            .tab()
            .buffer
            .file_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let system = "You are generating a git commit message. \
                      Write a concise, conventional commit message (type: description). \
                      Output ONLY the commit message, no explanations."
            .to_string();
        let messages = vec![Message::text(
            "user",
            &format!(
                "Generate a commit message for these changes:\n\n{}\n\nFile: {}",
                diff_summary, file_path_str
            ),
        )];

        let rx = client.stream_completion(&system, messages);
        // Collect synchronously in a background thread, then apply.
        let (result_tx, result_rx) = mpsc::channel::<String>();
        std::thread::Builder::new()
            .name("commit-msg".to_string())
            .spawn(move || {
                let mut msg = String::new();
                loop {
                    match rx.recv() {
                        Ok(AiEvent::Token(t)) => msg.push_str(&t),
                        Ok(AiEvent::Done(t)) => {
                            msg = t;
                            break;
                        }
                        Ok(AiEvent::ToolUse { .. })
                        | Ok(AiEvent::ToolUseComplete { .. })
                        | Ok(AiEvent::Activity(_)) => {}
                        Ok(AiEvent::Error(_)) | Err(_) => break,
                    }
                }
                let _ = result_tx.send(msg);
            })
            .ok();

        // Store the receiver; we'll poll it. For now, use blocking with timeout.
        match result_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(msg) if !msg.is_empty() => {
                let trimmed = msg.trim().to_string();
                self.set_status(format!("Commit msg: {trimmed}"));
                // Auto-commit with the generated message.
                self.git_commit(&trimmed);
                self.set_status(format!("Committed: {trimmed}"));
            }
            _ => {
                self.set_status("Failed to generate commit message");
            }
        }
    }

    /// List git branches.
    pub fn git_list_branches(&self) -> Vec<crate::git::BranchInfo> {
        self.git_repo
            .as_ref()
            .and_then(|r| r.list_branches().ok())
            .unwrap_or_default()
    }

    /// Switch to a git branch.
    pub fn git_checkout(&mut self, branch: &str) {
        if let Some(repo) = &mut self.git_repo {
            match repo.checkout_branch(branch) {
                Ok(()) => {
                    self.set_status(format!("Switched to branch '{branch}'"));
                    self.mark_highlights_dirty();
                }
                Err(e) => {
                    self.set_status(format!("Checkout failed: {e}"));
                }
            }
        }
    }

    /// Create a new git branch.
    pub fn git_create_branch(&mut self, branch: &str) {
        if let Some(repo) = &mut self.git_repo {
            match repo.create_branch(branch) {
                Ok(()) => {
                    self.set_status(format!("Created and switched to branch '{branch}'"));
                }
                Err(e) => {
                    self.set_status(format!("Branch creation failed: {e}"));
                }
            }
        }
    }

    /// Toggle blame display.
    pub fn toggle_blame(&mut self) {
        self.show_blame = !self.show_blame;
        let state = if self.show_blame { "on" } else { "off" };
        self.set_status(format!("Inline blame: {state}"));
    }

    /// Toggle the sidebar between Files and Git views.
    pub fn toggle_sidebar_view(&mut self) {
        self.sidebar_view = match self.sidebar_view {
            SidebarView::Files => {
                // Switching to Git: refresh and transfer focus.
                self.refresh_source_control();
                self.file_tree_focused = false;
                self.source_control_focused = self.file_tree.visible;
                SidebarView::Git
            }
            SidebarView::Git => {
                self.source_control_focused = false;
                self.file_tree_focused = self.file_tree.visible;
                SidebarView::Files
            }
        };
    }

    /// Refresh the source control panel from git status.
    pub fn refresh_source_control(&mut self) {
        if let Some(repo) = &self.git_repo {
            self.source_control.refresh(repo);
        }
        self.last_sc_refresh = std::time::Instant::now();
    }

    /// Stage the selected file in the source control panel.
    pub fn sc_stage_selected(&mut self) {
        if let Some(repo) = &self.git_repo {
            self.source_control.stage_selected(repo);
        }
    }

    /// Unstage the selected file in the source control panel.
    pub fn sc_unstage_selected(&mut self) {
        if let Some(repo) = &self.git_repo {
            self.source_control.unstage_selected(repo);
        }
    }

    /// Discard changes for the selected file in the source control panel.
    pub fn sc_discard_selected(&mut self) {
        if let Some(repo) = &self.git_repo {
            self.source_control.discard_selected(repo);
        }
    }

    /// Commit staged changes from the source control panel.
    pub fn sc_commit(&mut self) {
        if let Some(repo) = &self.git_repo {
            match self.source_control.commit(repo) {
                Ok(hash) => self.set_status(format!("Committed: {hash}")),
                Err(msg) => self.set_status(msg),
            }
        } else {
            self.set_status("Not in a git repository");
        }
    }

    /// Toggle the AI conversation history panel.
    ///
    /// If visible and focused → close. If visible but unfocused → focus.
    /// If hidden → open, refresh, and focus.
    pub fn toggle_conversation_history(&mut self) {
        if self.conversation_history.visible {
            if self.conversation_history_focused {
                self.conversation_history.visible = false;
                self.conversation_history_focused = false;
            } else {
                self.conversation_history_focused = true;
                self.file_tree_focused = false;
                self.source_control_focused = false;
                self.terminal_focused = false;
            }
        } else {
            // Hide chat panel — same right-side area.
            self.chat_panel.visible = false;
            self.chat_panel_focused = false;
            self.conversation_history.visible = true;
            self.refresh_conversation_history();
            self.conversation_history_focused = true;
            self.file_tree_focused = false;
            self.source_control_focused = false;
            self.terminal_focused = false;
        }
    }

    /// Handle a mouse click by focusing the panel under the cursor.
    fn handle_mouse_click(&mut self, col: u16, row: u16) {
        let point_in = |r: Rect| {
            r.width > 0 && r.height > 0
                && col >= r.x && col < r.x + r.width
                && row >= r.y && row < r.y + r.height
        };

        // Tab bar: clicking a tab switches to it.
        if self.tabs.count() > 1 && point_in(self.tab_bar_rect) {
            let click_x = (col - self.tab_bar_rect.x) as usize;
            let max_width = self.tab_bar_rect.width as usize;
            let mut x = 0usize;
            for (i, tab) in self.tabs.tabs().iter().enumerate() {
                let label = if i < 9 {
                    format!(" {}:{} ", i + 1, tab.title())
                } else {
                    format!(" {} ", tab.title())
                };
                let label_len = label.len();
                if x + label_len + 1 > max_width {
                    break;
                }
                if click_x >= x && click_x < x + label_len {
                    self.tabs.switch_to(i);
                    return;
                }
                x += label_len;
                // Separator character.
                if i + 1 < self.tabs.count() {
                    x += 1;
                }
            }
            return;
        }

        if self.terminal.visible && point_in(self.terminal_rect) {
            self.terminal_focused = true;
            self.file_tree_focused = false;
            self.source_control_focused = false;
            self.conversation_history_focused = false;
        } else if self.file_tree.visible && point_in(self.file_tree_rect) {
            self.terminal_focused = false;
            self.conversation_history_focused = false;
            self.chat_panel_focused = false;
            if self.sidebar_view == SidebarView::Git {
                self.source_control_focused = true;
                self.file_tree_focused = false;
            } else {
                self.file_tree_focused = true;
                self.source_control_focused = false;
                // Map the click row to a file tree entry.
                // Layout: file_tree_rect has a 1-cell border on all sides,
                // then a 1-row tab header ("Files | Git").
                // Entries start at file_tree_rect.y + 1 (border) + 1 (tab header) = +2.
                let entries_start_y = self.file_tree_rect.y + 2;
                if row >= entries_start_y {
                    let visible_height = self
                        .file_tree_rect
                        .height
                        .saturating_sub(3) as usize; // border top + tab header + border bottom
                    let selected = self.file_tree.selected;
                    let scroll_offset = if selected >= visible_height && visible_height > 0 {
                        selected.saturating_sub(visible_height - 1)
                    } else {
                        0
                    };
                    let clicked_row = (row - entries_start_y) as usize;
                    let clicked_idx = scroll_offset + clicked_row;
                    if clicked_idx < self.file_tree.entries.len() {
                        self.file_tree.selected = clicked_idx;
                        // Open the entry (file or toggle dir).
                        self.open_file_tree_selection();
                    }
                }
            }
        } else if self.conversation_history.visible && point_in(self.conv_history_rect) {
            self.conversation_history_focused = true;
            self.chat_panel_focused = false;
            self.terminal_focused = false;
            self.file_tree_focused = false;
            self.source_control_focused = false;
        } else if self.chat_panel.visible && point_in(self.chat_panel_rect) {
            self.chat_panel_focused = true;
            self.conversation_history_focused = false;
            self.terminal_focused = false;
            self.file_tree_focused = false;
            self.source_control_focused = false;
        } else if point_in(self.editor_rect) {
            self.terminal_focused = false;
            self.file_tree_focused = false;
            self.source_control_focused = false;
            self.conversation_history_focused = false;
            self.chat_panel_focused = false;

            // Move cursor and set visual anchor for potential drag selection.
            if self.screen_to_cursor(col, row) {
                // Clear any existing selection and record anchor for drag.
                self.mode = Mode::Normal;
                let cursor = self.tab().cursor;
                self.tab_mut().visual_anchor = Some(cursor);
            }
        }
    }

    /// Handle mouse drag — extend visual selection while dragging.
    fn handle_mouse_drag(&mut self, col: u16, row: u16) {
        // Only start/extend selection if the drag is within the editor area.
        let r = self.editor_rect;
        let in_editor = r.width > 0
            && r.height > 0
            && col >= r.x
            && col < r.x + r.width
            && row >= r.y
            && row < r.y + r.height;
        if !in_editor {
            return;
        }

        if self.screen_to_cursor(col, row) {
            // Enter Visual mode on the first drag event if not already there.
            if self.mode != Mode::Visual {
                self.mode = Mode::Visual;
            }
        }
    }

    /// Translate screen coordinates to a buffer position and move the cursor.
    ///
    /// Returns `true` if the coordinates mapped to a valid editor text area
    /// position and the cursor was moved.
    fn screen_to_cursor(&mut self, col: u16, row: u16) -> bool {
        let content_x = self.editor_rect.x + 1; // border
        let content_y = self.editor_rect.y + 1; // border
        let gutter_width: u16 = 6;
        let text_start_x = content_x + gutter_width;

        if col < text_start_x || row < content_y {
            return false;
        }

        let clicked_row = (row - content_y) as usize;
        let clicked_col = (col - text_start_x) as usize;
        let is_insert = self.mode == Mode::Insert;
        let tab = self.tab_mut();
        let target_row = tab.scroll_row + clicked_row;
        let target_col = tab.scroll_col + clicked_col;

        let max_row = tab.buffer.line_count().saturating_sub(1);
        tab.cursor.row = target_row.min(max_row);

        let line_len = tab
            .buffer
            .line_text(tab.cursor.row)
            .map(|l| {
                let trimmed = l.trim_end_matches('\n').trim_end_matches('\r');
                trimmed.len()
            })
            .unwrap_or(0);
        let max_col = if is_insert {
            line_len
        } else {
            line_len.saturating_sub(1)
        };
        tab.cursor.col = target_col.min(max_col);
        true
    }

    /// Handle mouse scroll by scrolling the panel under the cursor.
    ///
    /// `up` is `true` for scroll-up (content moves down), `false` for scroll-down.
    fn handle_mouse_scroll(&mut self, col: u16, row: u16, up: bool) {
        let point_in = |r: Rect| {
            r.width > 0
                && r.height > 0
                && col >= r.x
                && col < r.x + r.width
                && row >= r.y
                && row < r.y + r.height
        };

        let scroll_lines: usize = 3;

        if point_in(self.editor_rect) {
            // Scroll the editor viewport and keep cursor within the visible area
            // so that scroll_to_cursor (called every frame) does not reset the scroll.
            let viewport_h = self.editor_rect.height.saturating_sub(2) as usize; // borders
            let margin = self.config.editor.scroll_margin;
            {
                let tab = self.tab_mut();
                let max_scroll = tab.buffer.line_count().saturating_sub(1);
                if up {
                    tab.scroll_row = tab.scroll_row.saturating_sub(scroll_lines);
                } else {
                    tab.scroll_row = (tab.scroll_row + scroll_lines).min(max_scroll);
                }
                // Clamp cursor row to stay inside the visible viewport, accounting
                // for the scroll margin so that scroll_to_cursor does not undo
                // the scroll on the next frame.
                if viewport_h > margin * 2 {
                    let safe_start = tab.scroll_row + margin;
                    let safe_end = tab.scroll_row + viewport_h.saturating_sub(1).saturating_sub(margin);
                    if tab.cursor.row < safe_start {
                        tab.cursor.row = safe_start;
                    } else if tab.cursor.row > safe_end {
                        tab.cursor.row = safe_end;
                    }
                }
            }
            self.clamp_cursor();
        } else if self.file_tree.visible && point_in(self.file_tree_rect) {
            // Scroll the file tree / source control sidebar.
            if self.sidebar_view == SidebarView::Files {
                for _ in 0..scroll_lines {
                    if up {
                        self.file_tree.select_up();
                    } else {
                        self.file_tree.select_down();
                    }
                }
            } else {
                for _ in 0..scroll_lines {
                    if up {
                        self.source_control.select_up();
                    } else {
                        self.source_control.select_down();
                    }
                }
            }
        } else if self.terminal.visible && point_in(self.terminal_rect) {
            // Scroll the terminal scrollback.
            for _ in 0..scroll_lines {
                if up {
                    self.terminal.scroll_up();
                } else {
                    self.terminal.scroll_down();
                }
            }
        } else if self.chat_panel.visible && point_in(self.chat_panel_rect) {
            // Scroll the chat panel.
            if up {
                for _ in 0..scroll_lines {
                    self.chat_panel.scroll_up();
                }
            } else {
                for _ in 0..scroll_lines {
                    self.chat_panel.scroll_down();
                }
            }
        } else if self.conversation_history.visible && point_in(self.conv_history_rect) {
            // Scroll the conversation history panel.
            for _ in 0..scroll_lines {
                if up {
                    self.conversation_history.select_up();
                } else {
                    self.conversation_history.select_down();
                }
            }
        }
    }

    /// Refresh the conversation history panel from the database.
    pub fn refresh_conversation_history(&mut self) {
        if let Some(store) = &self.conversation_store {
            self.conversation_history.refresh(store);
        }
    }

    // ── Chat panel ───────────────────────────────────────────────

    /// Toggle the chat panel.
    ///
    /// If visible and focused → close. If visible but unfocused → focus.
    /// If hidden → open and focus. Mutually exclusive with conversation history.
    pub fn toggle_chat_panel(&mut self) {
        if self.chat_panel.visible {
            if self.chat_panel_focused {
                self.chat_panel.visible = false;
                self.chat_panel_focused = false;
            } else {
                self.chat_panel_focused = true;
                self.conversation_history_focused = false;
                self.file_tree_focused = false;
                self.source_control_focused = false;
                self.terminal_focused = false;
            }
        } else {
            // Hide conversation history — same right-side area.
            self.conversation_history.visible = false;
            self.conversation_history_focused = false;
            self.chat_panel.visible = true;
            self.chat_panel_focused = true;
            self.file_tree_focused = false;
            self.source_control_focused = false;
            self.terminal_focused = false;
            // Load existing chat conversation if we have one.
            self.load_chat_conversation();
        }
    }

    /// Send the current chat input as a message to the AI.
    pub fn send_chat_message(&mut self) {
        let text = self.chat_panel.take_input();
        if text.trim().is_empty() {
            return;
        }

        // Handle slash commands.
        if text.starts_with('/') {
            match text.trim() {
                "/clear" => {
                    self.chat_panel.clear();
                    self.set_status("Chat cleared");
                    return;
                }
                "/help" => {
                    self.chat_panel.push_system_message(
                        "Commands: /clear — clear chat, /help — show help\n\
                         Enter — send message, Esc — unfocus, Ctrl+Up/Down — scroll\n\n\
                         AI Tools: The AI can read files, edit files, search code,\n\
                         list files, and run commands. Safe tools (read/search/list)\n\
                         auto-approve. Destructive tools (edit/run) ask for approval:\n\
                         press Y to allow, N to deny.",
                    );
                    return;
                }
                _ => {
                    self.chat_panel.push_system_message(
                        &format!("Unknown command: {}", text.trim()),
                    );
                    return;
                }
            }
        }

        if self.ai_client.is_none() {
            self.chat_panel.push_system_message(
                "No AI backend available. Set ANTHROPIC_API_KEY or install Claude Code CLI.",
            );
            return;
        }

        // Ensure we have a chat conversation in the database.
        self.ensure_chat_conversation();

        // Add user message to panel.
        self.chat_panel.push_user_message(&text);

        // Persist user message.
        if let (Some(store), Some(conv_id)) =
            (&self.conversation_store, &self.chat_panel.conversation_id)
        {
            if let Err(e) = store.add_message(conv_id, MessageRole::HumanIntent, &text, None) {
                tracing::warn!("Failed to persist chat user message: {e}");
            }
        }

        // Capture selection context for the chat indicator.
        if let Some((sel_start, sel_end)) = self.visual_selection_range() {
            let tab = self.tab();
            let start_cur = tab.buffer.char_idx_to_cursor(sel_start);
            let end_cur = tab.buffer.char_idx_to_cursor(sel_end);
            let lines = end_cur.row.saturating_sub(start_cur.row) + 1;
            let file_name = tab.file_name();
            self.chat_panel.selection_context =
                Some(format!("{lines} line{} from {file_name}", if lines == 1 { "" } else { "s" }));
        } else {
            self.chat_panel.selection_context = None;
        }

        // Build system prompt with editor context.
        let system = self.build_chat_system_prompt();
        let messages = self.chat_panel.build_messages();

        let client = self.ai_client.as_ref().unwrap();

        // Use tools if the backend supports them.
        if client.supports_tools() {
            let tools = editor_tools();
            let rx = client.stream_completion_with_tools(&system, messages, tools);
            self.chat_receiver = Some(rx);
            self.chat_panel.start_streaming();
            self.chat_panel.in_tool_loop = false;
            self.chat_panel.tool_loop_count = 0;
            self.tool_loop_system_prompt = system;
        } else {
            let rx = client.stream_completion(&system, messages);
            self.chat_receiver = Some(rx);
            self.chat_panel.start_streaming();
        }
    }

    /// Poll the chat receiver for streaming events.
    fn poll_chat_events(&mut self) {
        // If we have pending tool calls awaiting approval, don't poll.
        if self.chat_panel.pending_approval.is_some() {
            return;
        }

        let rx = match &self.chat_receiver {
            Some(rx) => rx,
            None => return,
        };

        loop {
            match rx.try_recv() {
                Ok(AiEvent::Token(text)) => {
                    self.chat_panel.append_token(&text);
                }
                Ok(AiEvent::Done(full_text)) => {
                    // Persist AI response.
                    if let (Some(store), Some(conv_id)) =
                        (&self.conversation_store, &self.chat_panel.conversation_id)
                    {
                        if let Err(e) = store.add_message(
                            conv_id,
                            MessageRole::AiResponse,
                            &full_text,
                            Some("claude"),
                        ) {
                            tracing::warn!("Failed to persist chat AI response: {e}");
                        }
                    }
                    self.chat_panel.finish_streaming();
                    self.chat_panel.in_tool_loop = false;
                    self.chat_receiver = None;
                    return;
                }
                Ok(AiEvent::ToolUse { id, name, input }) => {
                    // A tool call is being streamed — track it.
                    let permission = tool_permission(&name);
                    let status = match permission {
                        ToolPermission::AutoApprove => ToolCallStatus::Running,
                        ToolPermission::RequiresApproval => ToolCallStatus::PendingApproval,
                    };
                    let idx = self.chat_panel.add_tool_call(&id, &name, input.clone(), status);
                    self.pending_tool_calls.push(PendingToolCall {
                        id,
                        name,
                        input,
                        item_index: idx,
                    });
                }
                Ok(AiEvent::ToolUseComplete { text, content_blocks }) => {
                    // The assistant turn is complete with tool calls.
                    self.chat_panel.finish_streaming_for_tools();
                    self.current_assistant_blocks = content_blocks.clone();
                    self.chat_panel
                        .add_assistant_blocks_to_context(content_blocks);
                    self.chat_panel.in_tool_loop = true;
                    self.chat_receiver = None;

                    // Persist the text portion.
                    if !text.is_empty() {
                        if let (Some(store), Some(conv_id)) =
                            (&self.conversation_store, &self.chat_panel.conversation_id)
                        {
                            let _ = store.add_message(
                                conv_id,
                                MessageRole::AiResponse,
                                &text,
                                Some("claude"),
                            );
                        }
                    }

                    // Process pending tool calls.
                    self.process_pending_tools();
                    return;
                }
                Ok(AiEvent::Activity(msg)) => {
                    // Show activity/status from the backend in the chat panel.
                    self.chat_panel.push_system_message(&msg);
                }
                Ok(AiEvent::Error(err)) => {
                    self.chat_panel
                        .push_system_message(&format!("Error: {err}"));
                    self.chat_panel.streaming = false;
                    self.chat_panel.streaming_text.clear();
                    self.chat_panel.in_tool_loop = false;
                    self.chat_receiver = None;
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    // Finalize whatever was accumulated.
                    if !self.chat_panel.streaming_text.is_empty() {
                        let text = self.chat_panel.streaming_text.clone();
                        if let (Some(store), Some(conv_id)) =
                            (&self.conversation_store, &self.chat_panel.conversation_id)
                        {
                            let _ = store.add_message(
                                conv_id,
                                MessageRole::AiResponse,
                                &text,
                                Some("claude"),
                            );
                        }
                    }
                    self.chat_panel.finish_streaming();
                    self.chat_receiver = None;
                    return;
                }
            }
        }
    }

    /// Process pending tool calls — auto-approve safe ones, prompt for others.
    fn process_pending_tools(&mut self) {
        if self.pending_tool_calls.is_empty() {
            return;
        }

        // Process all auto-approve tools first.
        let mut needs_approval = Vec::new();
        let calls: Vec<PendingToolCall> = std::mem::take(&mut self.pending_tool_calls);

        for call in calls {
            let permission = tool_permission(&call.name);
            if permission == ToolPermission::AutoApprove {
                self.execute_tool_call(&call);
            } else {
                needs_approval.push(call);
            }
        }

        if let Some(call) = needs_approval.first() {
            // Set pending approval for the first tool that needs it.
            self.chat_panel.pending_approval = Some(call.item_index);
            self.pending_tool_calls = needs_approval;
            // Auto-focus the chat panel so the user can press Y/N.
            self.chat_panel_focused = true;
        } else {
            // All tools were auto-approved — continue the loop.
            self.continue_tool_loop();
        }
    }

    /// Execute a single tool call and update the chat panel.
    fn execute_tool_call(&mut self, call: &PendingToolCall) {
        self.chat_panel
            .update_tool_status(call.item_index, ToolCallStatus::Running);

        let project_root = std::env::current_dir().unwrap_or_default();
        let result = chat_tools::execute_tool(&call.name, &call.input, &project_root);

        match result {
            Ok(output) => {
                self.chat_panel
                    .set_tool_result(call.item_index, output.clone(), true);
                self.chat_panel
                    .add_tool_result_to_context(&call.id, &output, false);
            }
            Err(err) => {
                self.chat_panel
                    .set_tool_result(call.item_index, err.clone(), false);
                self.chat_panel
                    .add_tool_result_to_context(&call.id, &err, true);
            }
        }
    }

    /// Approve the pending tool call and execute it.
    pub fn approve_pending_tool(&mut self) {
        if self.pending_tool_calls.is_empty() {
            return;
        }

        let call = self.pending_tool_calls.remove(0);
        self.chat_panel.pending_approval = None;
        self.execute_tool_call(&call);

        // Check if there are more pending tools that need approval.
        if let Some(next) = self.pending_tool_calls.first() {
            let permission = tool_permission(&next.name);
            if permission == ToolPermission::RequiresApproval {
                self.chat_panel.pending_approval = Some(next.item_index);
            } else {
                // Auto-approve remaining.
                let remaining: Vec<PendingToolCall> =
                    std::mem::take(&mut self.pending_tool_calls);
                for c in &remaining {
                    self.execute_tool_call(c);
                }
                self.continue_tool_loop();
            }
        } else {
            self.continue_tool_loop();
        }
    }

    /// Deny the pending tool call.
    pub fn deny_pending_tool(&mut self) {
        if self.pending_tool_calls.is_empty() {
            return;
        }

        let call = self.pending_tool_calls.remove(0);
        self.chat_panel
            .update_tool_status(call.item_index, ToolCallStatus::Denied);
        self.chat_panel.add_tool_result_to_context(
            &call.id,
            "User denied this tool call.",
            true,
        );

        // Deny all remaining pending tools too.
        let remaining: Vec<PendingToolCall> = std::mem::take(&mut self.pending_tool_calls);
        for c in &remaining {
            self.chat_panel
                .update_tool_status(c.item_index, ToolCallStatus::Denied);
            self.chat_panel.add_tool_result_to_context(
                &c.id,
                "User denied this tool call.",
                true,
            );
        }
        self.chat_panel.pending_approval = None;
        self.continue_tool_loop();
    }

    /// Continue the tool loop by sending tool results back to the API.
    fn continue_tool_loop(&mut self) {
        self.chat_panel.tool_loop_count = self.chat_panel.tool_loop_count.saturating_add(1);

        if self.chat_panel.tool_loop_count >= chat_tools::MAX_TOOL_ITERATIONS {
            self.chat_panel.push_system_message(
                "Tool loop limit reached. Stopping automatic tool use.",
            );
            self.chat_panel.in_tool_loop = false;
            return;
        }

        let client = match &self.ai_client {
            Some(c) => c,
            None => return,
        };

        let messages = self.chat_panel.build_messages();
        let tools = editor_tools();
        let system = self.tool_loop_system_prompt.clone();
        let rx = client.stream_completion_with_tools(&system, messages, tools);
        self.chat_receiver = Some(rx);
        self.chat_panel.start_streaming();
    }

    /// Build a system prompt for chat that includes editor context.
    fn build_chat_system_prompt(&self) -> String {
        let tab = self.tab();
        let file_path = tab
            .buffer
            .file_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<scratch>".to_string());
        let line_count = tab.buffer.line_count();
        let cursor_row = tab.cursor.row + 1;
        let cursor_col = tab.cursor.col + 1;

        let mut prompt = String::from(
            "You are an AI assistant integrated into the AURA text editor. \
             Help the user with their coding questions and tasks. \
             Be concise and helpful.\n\n\
             You have access to tools for interacting with the codebase. \
             Use them when the user asks you to read, edit, or search files, \
             or run commands. Always prefer using tools over guessing about \
             file contents. When editing files, show the user what you plan \
             to change.\n\n",
        );
        prompt.push_str(&format!("Current file: {file_path}\n"));
        prompt.push_str(&format!("Lines: {line_count}, Cursor: {cursor_row}:{cursor_col}\n"));

        // Include selected text if there is an active visual selection.
        if let Some((sel_start, sel_end)) = self.visual_selection_range() {
            let selected_text = tab.buffer.rope().slice(sel_start..sel_end).to_string();
            let start_cursor = tab.buffer.char_idx_to_cursor(sel_start);
            let end_cursor = tab.buffer.char_idx_to_cursor(sel_end);
            let sel_lines = end_cursor.row.saturating_sub(start_cursor.row) + 1;
            prompt.push_str(&format!(
                "\nThe user has selected {} line{} (L{}:{} to L{}:{}):\n```\n{}\n```\n",
                sel_lines,
                if sel_lines == 1 { "" } else { "s" },
                start_cursor.row + 1,
                start_cursor.col + 1,
                end_cursor.row + 1,
                end_cursor.col + 1,
                selected_text,
            ));
        } else {
            // No selection — include a snippet of surrounding code for context.
            let start = cursor_row.saturating_sub(6);
            let end = (cursor_row + 5).min(line_count);
            if start < end {
                prompt.push_str("\nCode around cursor:\n```\n");
                for i in start..end {
                    if let Some(line) = tab.buffer.line(i) {
                        let marker = if i + 1 == cursor_row { ">" } else { " " };
                        prompt.push_str(&format!("{marker}{:4} | {}\n", i + 1, line));
                    }
                }
                prompt.push_str("```\n");
            }
        }

        // Include diagnostics if any.
        if !tab.diagnostics.is_empty() {
            prompt.push_str("\nActive diagnostics:\n");
            for d in tab.diagnostics.iter().take(5) {
                let sev = if d.is_error() { "error" } else { "warning" };
                prompt.push_str(&format!(
                    "- L{}: [{}] {}\n",
                    d.range.start.line + 1,
                    sev,
                    d.message
                ));
            }
        }

        prompt
    }

    /// Ensure a chat conversation exists in the database.
    fn ensure_chat_conversation(&mut self) {
        if self.chat_panel.conversation_id.is_some() {
            return;
        }
        self.ensure_conversation_store();
        let (branch, _) = self.git_context();
        if let Some(store) = &self.conversation_store {
            match store.find_or_create_chat_conversation(branch.as_deref()) {
                Ok(conv) => {
                    self.chat_panel.conversation_id = Some(conv.id);
                }
                Err(e) => {
                    tracing::warn!("Failed to create chat conversation: {e}");
                }
            }
        }
    }

    /// Load an existing chat conversation from the database.
    fn load_chat_conversation(&mut self) {
        self.ensure_conversation_store();
        let (branch, _) = self.git_context();
        if let Some(store) = &self.conversation_store {
            match store.find_or_create_chat_conversation(branch.as_deref()) {
                Ok(conv) => {
                    self.chat_panel.load_conversation(store, &conv.id);
                }
                Err(e) => {
                    tracing::warn!("Failed to load chat conversation: {e}");
                }
            }
        }
    }

    /// Seed sample conversations into the database for testing the history panel.
    ///
    /// Creates 3 conversations with realistic data (messages, summaries, decisions)
    /// so the AI History panel can be developed without a live AI backend.
    pub fn seed_conversation_history(&mut self) {
        self.ensure_conversation_store();
        let (branch, commit) = self.git_context();

        let store = match &self.conversation_store {
            Some(s) => s,
            None => {
                self.set_status("Failed to open conversation store");
                return;
            }
        };

        let samples: &[(&str, usize, usize, &str, &[(&str, &str)])] = &[
            (
                "src/main.rs",
                10,
                25,
                "Refactor main entry point to use async runtime",
                &[
                    ("human_intent", "Refactor the main function to use tokio async runtime instead of blocking calls"),
                    ("ai_response", "I'll restructure main() to use #[tokio::main] and convert the blocking I/O calls to their async equivalents. The key changes are:\n1. Add tokio::main attribute\n2. Convert file reads to tokio::fs\n3. Wrap the event loop in a select! macro"),
                ],
            ),
            (
                "src/lib.rs",
                42,
                68,
                "Add error handling for buffer operations",
                &[
                    ("human_intent", "Add proper error handling to the buffer insert and delete operations"),
                    ("ai_response", "I'll replace the unwrap() calls with proper Result propagation using the ? operator and add context via anyhow::Context. This ensures buffer operations never panic in production."),
                    ("human_intent", "Also add a custom error type for out-of-bounds access"),
                    ("ai_response", "Added BufferError::OutOfBounds with the range information. All index-based operations now validate bounds before accessing the rope and return this error type."),
                ],
            ),
            (
                "src/utils.rs",
                1,
                15,
                "Implement word-boundary detection for cursor movement",
                &[
                    ("human_intent", "Write a utility function that detects word boundaries for vim-style w/b cursor movement"),
                    ("ai_response", "Here's a word_boundary_forward() function that handles three categories: whitespace, punctuation, and word characters. It follows vim's definition where a word boundary is a transition between character categories."),
                ],
            ),
        ];

        let mut count = 0usize;
        for (file_path, start, end, summary, messages) in samples {
            let conv = match store.create_conversation(
                file_path,
                *start,
                *end,
                commit.as_deref(),
                branch.as_deref(),
            ) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("seed: failed to create conversation: {e}");
                    continue;
                }
            };

            let _ = store.update_summary(&conv.id, summary);

            for (role_str, content) in *messages {
                let role = match *role_str {
                    "human_intent" => MessageRole::HumanIntent,
                    "ai_response" => MessageRole::AiResponse,
                    _ => MessageRole::System,
                };
                let model = if role == MessageRole::AiResponse {
                    Some("claude-sonnet-4-20250514")
                } else {
                    None
                };
                let _ = store.add_message(&conv.id, role, content, model);
            }

            // Log a sample decision on the first conversation.
            if count == 0 {
                let _ = store.log_decision(
                    &conv.id,
                    None,
                    Decision::Accepted,
                    Some("fn main() {"),
                    Some("#[tokio::main]\nasync fn main() {"),
                    file_path,
                    *start,
                    *end,
                    commit.as_deref(),
                    branch.as_deref(),
                );
            } else if count == 1 {
                let _ = store.log_decision(
                    &conv.id,
                    None,
                    Decision::Rejected,
                    Some("buffer.insert(pos, text)"),
                    Some("buffer.try_insert(pos, text)?"),
                    file_path,
                    *start,
                    *end,
                    commit.as_deref(),
                    branch.as_deref(),
                );
            }

            count += 1;
        }

        self.refresh_conversation_history();

        // Open the history panel if not already visible.
        if !self.conversation_history.visible {
            self.conversation_history.visible = true;
            self.conversation_history_focused = true;
            self.file_tree_focused = false;
            self.source_control_focused = false;
            self.terminal_focused = false;
        }

        self.set_status(format!("Seeded {count} sample conversations"));
    }

    /// Lazily initialize the conversation store if not already set.
    ///
    /// Uses the same priority as initial construction:
    /// git workdir `.aura/` → cwd `.aura/` → `~/.aura/` (global fallback).
    fn ensure_conversation_store(&mut self) {
        if self.conversation_store.is_some() {
            return;
        }
        let db_path = self
            .git_repo
            .as_ref()
            .map(|r| r.workdir().join(".aura").join("conversations.db"))
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|cwd| cwd.join(".aura").join("conversations.db"))
            })
            .or_else(|| dirs_path().map(|d| d.join(".aura").join("conversations.db")));
        if let Some(path) = db_path {
            match ConversationStore::open(&path) {
                Ok(s) => {
                    tracing::info!("Lazily initialized conversation store at {:?}", path);
                    self.conversation_store = Some(s);
                }
                Err(e) => {
                    tracing::warn!("Failed to lazily open conversation store at {:?}: {e}", path);
                }
            }
        }
    }

    /// Toggle-expand the selected conversation in the history panel.
    pub fn conversation_history_toggle_expand(&mut self) {
        if let Some(store) = &self.conversation_store {
            self.conversation_history.toggle_expand(store);
        }
    }

    /// Ensure an active MCP conversation exists, creating one if needed.
    fn ensure_mcp_conversation(&mut self, start_line: usize, end_line: usize) {
        self.ensure_conversation_store();

        if let Some(store) = &self.conversation_store {
            let file_path = self
                .tab()
                .buffer
                .file_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<scratch>".to_string());

            // Reuse an existing conversation for this range, or create a new one.
            let conv = store
                .conversations_for_range(&file_path, start_line, end_line)
                .ok()
                .and_then(|v| v.into_iter().next())
                .or_else(|| {
                    let (branch, commit) = self.git_context();
                    store
                        .create_conversation(&file_path, start_line, end_line, commit.as_deref(), branch.as_deref())
                        .ok()
                });

            if let Some(c) = conv {
                self.active_conversation = Some(c.id);
                self.refresh_conversation_history();
            }
        }
    }

    /// Log an agent session start as a conversation entry.
    fn log_agent_session(&mut self, agent_name: &str, role: Option<&str>) {
        self.ensure_conversation_store();

        if let Some(store) = &self.conversation_store {
            let file_path = self
                .tab()
                .buffer
                .file_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<scratch>".to_string());
            let end_line = self.tab().buffer.line_count().saturating_sub(1);

            let (branch, commit) = self.git_context();
            if let Ok(conv) = store.create_conversation(&file_path, 0, end_line, commit.as_deref(), branch.as_deref()) {
                let msg = if let Some(r) = role {
                    format!("Agent '{}' connected (role: {})", agent_name, r)
                } else {
                    format!("Agent '{}' connected", agent_name)
                };
                let _ = store.add_message(
                    &conv.id,
                    MessageRole::System,
                    &msg,
                    Some(agent_name),
                );
                self.active_conversation = Some(conv.id);
            }
        }

        self.refresh_conversation_history();
    }

    /// Read the git aura log and display it in the conversation panel.
    ///
    /// Shows up to `limit` commits with their `Aura-Conversation` trailers.
    pub fn show_aura_log(&mut self, limit: usize) {
        use aura_core::conversation::{ConversationMessage, MessageRole};

        let entries = match &self.git_repo {
            Some(repo) => repo.aura_log(limit),
            None => {
                self.set_status("Not in a git repository");
                return;
            }
        };

        if entries.is_empty() {
            self.set_status("No commits found");
            return;
        }

        let mut lines = Vec::new();
        for entry in &entries {
            let conv_tag = entry
                .conversation_id
                .as_deref()
                .map(|id| format!(" [conv: {}]", id))
                .unwrap_or_default();
            lines.push(format!(
                "{} {} — {}{}",
                entry.commit_short, entry.author, entry.summary, conv_tag
            ));
        }

        let content = lines.join("\n");
        let message = ConversationMessage {
            id: "aura-log".to_string(),
            conversation_id: "aura-log".to_string(),
            role: MessageRole::System,
            content,
            created_at: String::new(),
            model: None,
        };

        let aura_count = entries
            .iter()
            .filter(|e| e.conversation_id.is_some())
            .count();

        self.conversation_panel = Some(ConversationPanel {
            messages: vec![message],
            file_info: format!(
                "git log --aura: {} commits, {} with Aura conversations",
                entries.len(),
                aura_count
            ),
            scroll: 0,
        });
        self.set_status("Aura log — Esc/q to close, j/k to scroll");
    }

    /// Enter experimental mode by creating a new branch `aura-experiment/<name>`.
    ///
    /// While experimental mode is active, AI suggestions are automatically accepted
    /// without requiring user review.
    pub fn enter_experiment_mode(&mut self, name: &str) {
        if name.is_empty() {
            self.set_status("Usage: experiment <name>");
            return;
        }

        let branch_name = format!("aura-experiment/{}", name);

        if let Some(repo) = &mut self.git_repo {
            match repo.create_branch(&branch_name) {
                Ok(()) => {
                    self.experimental_mode = true;
                    self.set_status(format!(
                        "[EXPERIMENT] Branch '{}' created — AI suggestions will be auto-accepted",
                        branch_name
                    ));
                }
                Err(e) => {
                    self.set_status(format!("Failed to create experiment branch: {e}"));
                }
            }
        } else {
            // No git repo — still enable experimental mode without creating a branch.
            self.experimental_mode = true;
            self.set_status(format!(
                "[EXPERIMENT] '{}' started (no git repo — branch not created) — AI suggestions will be auto-accepted",
                name
            ));
        }
    }

    /// Open the fuzzy file picker overlay.
    pub fn open_file_picker(&mut self) {
        self.file_picker.open();
    }

    /// Open a file by path in a new tab, or switch to an existing tab.
    pub fn open_file(&mut self, path: PathBuf) -> Result<(), String> {
        let conversation_store = self.conversation_store.as_ref();
        let theme = self.theme.clone();
        let was_new = self.tabs.open_or_switch(&path, || {
            let buf = Buffer::from_file(&path).map_err(|e| format!("Error opening file: {}", e))?;
            Ok(EditorTab::new(buf, conversation_store, &theme))
        })?;
        if was_new {
            self.set_status(format!("Opened {}", path.display()));
        } else {
            self.set_status(format!("Switched to {}", path.display()));
        }
        Ok(())
    }

    /// Open a side-by-side diff view for the given file (relative to repo root).
    ///
    /// Compares the HEAD version against the working tree version.
    pub fn open_diff_view(&mut self, rel_path: &str) {
        let workdir = match self.git_repo.as_ref().map(|r| r.workdir().to_path_buf()) {
            Some(wd) => wd,
            None => {
                self.set_status("No git repository");
                return;
            }
        };

        let rel = std::path::Path::new(rel_path);
        let full_path = workdir.join(rel);

        // Read working tree content.
        let new_content = match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(e) => {
                self.set_status(format!("Cannot read file: {}", e));
                return;
            }
        };

        // Read HEAD content.
        let old_content = match self.git_repo.as_ref().and_then(|r| r.head_file_content(rel).ok()) {
            Some(Some(c)) => c,
            _ => String::new(), // New file — empty old side.
        };

        let lines = crate::git::aligned_diff_lines(&old_content, &new_content);
        self.diff_view = Some(DiffView::new(rel_path.to_string(), lines));
        self.mode = Mode::Diff;
    }

    /// Load the file currently selected in the file picker into a tab,
    /// then close the picker. If no file is selected, or loading fails, a
    /// status message is shown instead.
    pub fn open_selected_file(&mut self) {
        let path = match self.file_picker.selected_path() {
            Some(p) => p,
            None => {
                self.set_status("No file selected");
                return;
            }
        };
        self.file_picker.close();
        if let Err(e) = self.open_file(path) {
            self.set_status(e);
        }
    }

    /// Open the file currently selected in the file tree into the buffer.
    /// If the selected entry is a directory, toggles its expansion instead.
    /// Write the MCP discovery file to `~/.aura/mcp.json`.
    ///
    /// This file allows external tools like Claude Code to auto-discover
    /// the running AURA MCP server without manual port configuration.
    fn write_mcp_discovery(port: u16, file_path: Option<&std::path::Path>) {
        let Some(home) = dirs_path() else { return };
        let aura_dir = home.join(".aura");
        if std::fs::create_dir_all(&aura_dir).is_err() {
            tracing::warn!("Could not create ~/.aura directory");
            return;
        }
        let discovery = serde_json::json!({
            "host": "127.0.0.1",
            "port": port,
            "pid": std::process::id(),
            "file": file_path.map(|p| p.display().to_string()),
            "started": chrono_now(),
        });
        let path = aura_dir.join("mcp.json");
        match std::fs::write(
            &path,
            serde_json::to_string_pretty(&discovery).unwrap_or_default(),
        ) {
            Ok(()) => tracing::info!("MCP discovery file written to {}", path.display()),
            Err(e) => tracing::warn!("Failed to write MCP discovery file: {}", e),
        }
    }

    /// Remove the MCP discovery file on shutdown.
    fn remove_mcp_discovery() {
        let Some(home) = dirs_path() else { return };
        let path = home.join(".aura").join("mcp.json");
        let _ = std::fs::remove_file(&path);
    }

    pub fn open_file_tree_selection(&mut self) {
        if self.file_tree.entries.is_empty() {
            return;
        }
        let entry = &self.file_tree.entries[self.file_tree.selected];
        if entry.is_dir {
            self.file_tree.toggle_expand();
            return;
        }
        let path = entry.path.clone();
        self.file_tree_focused = false;
        if let Err(e) = self.open_file(path) {
            self.set_status(e);
        }
    }

    /// Returns the (start, end) character indices of the visual selection, if active.
    pub fn visual_selection_range(&self) -> Option<(usize, usize)> {
        let tab = self.tab();
        let anchor = tab.visual_anchor?;
        let a = tab.buffer.cursor_to_char_idx(&anchor);
        let b = tab.buffer.cursor_to_char_idx(&tab.cursor);

        match self.mode {
            Mode::Visual => {
                let (start, end) = if a <= b { (a, b + 1) } else { (b, a + 1) };
                Some((start, end.min(tab.buffer.len_chars())))
            }
            Mode::VisualLine => {
                let (start_row, end_row) = if anchor.row <= tab.cursor.row {
                    (anchor.row, tab.cursor.row)
                } else {
                    (tab.cursor.row, anchor.row)
                };
                let start = tab.buffer.cursor_to_char_idx(&Cursor::new(start_row, 0));
                let end_cursor = Cursor::new(end_row + 1, 0);
                let end = tab.buffer.cursor_to_char_idx(&end_cursor);
                Some((start, end.min(tab.buffer.len_chars())))
            }
            _ => None,
        }
    }

    /// Close the current tab. Returns `true` if the app should quit.
    pub fn close_current_tab(&mut self) -> bool {
        if self.tab().is_modified() {
            self.set_status("Unsaved changes! Use :tabclose! to force or :w first");
            return false;
        }
        self.close_current_tab_force()
    }

    /// Force-close the current tab without checking for modifications.
    /// Returns `true` if there are no more tabs (app should quit).
    pub fn close_current_tab_force(&mut self) -> bool {
        // Shutdown LSP for this tab.
        self.tab_mut().shutdown_lsp();
        if self.tabs.close_active().is_none() {
            // Last tab — signal quit.
            return true;
        }
        false
    }
}
