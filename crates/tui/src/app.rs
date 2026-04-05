//! Application state and main event loop.

use crate::chat_panel::ChatPanel;
use crate::chat_panel::ToolCallStatus;
use crate::chat_tools;
use crate::config::{AuraConfig, Theme};
use crate::conversation_history::ConversationHistoryPanel;
use crate::diff_view::DiffView;
use crate::embedded_terminal::EmbeddedTerminal;
use crate::file_picker::FilePicker;
use crate::file_tree::FileTree;
use crate::git::{GitRepo, LineStatus};
use crate::help::HelpOverlay;
use crate::lsp::{Diagnostic, LspEvent};
use crate::mcp_client::{McpClientConnection, McpClientEvent};
use crate::mcp_server::{AgentRegistry, McpAction, McpAppResponse, McpServer};
use crate::plugin::PluginManager;
use crate::session::{self, Session, TabState, UiState};
use crate::source_control::{SidebarView, SourceControlPanel};
use crate::speculative::{Aggressiveness, GhostSuggestion, SpeculativeEngine};
use crate::tab::{EditorTab, TabManager};
use crate::update::{self, UpdateStatus};
use aura_ai::{
    editor_tools, estimate_tokens, tool_permission, AiBackend, AiConfig, AiEvent, ContentBlock,
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

/// Direction for split panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    /// Left/right split (vertical divider).
    Vertical,
    /// Top/bottom split (horizontal divider).
    Horizontal,
}

/// Trust level controlling which tools are auto-approved in agent mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustLevel {
    /// Only read tools (read_file, list_files, search_files) are auto-approved.
    ReadOnly,
    /// Read + write tools (edit_file, create_directory, rename_file) are auto-approved.
    WriteAllowed,
    /// Everything is auto-approved including run_command (original behavior).
    FullAuto,
}

impl TrustLevel {
    /// Parse from a string (for `:agent trust` command).
    pub fn parse_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "read" | "readonly" | "read_only" => Some(Self::ReadOnly),
            "write" | "writeallowed" | "write_allowed" => Some(Self::WriteAllowed),
            "full" | "fullauto" | "full_auto" | "auto" => Some(Self::FullAuto),
            _ => None,
        }
    }

    /// Whether a tool is auto-approved at this trust level.
    pub fn auto_approves(&self, tool_name: &str) -> bool {
        match self {
            Self::ReadOnly => matches!(tool_name, "read_file" | "list_files" | "search_files"),
            Self::WriteAllowed => matches!(
                tool_name,
                "read_file"
                    | "list_files"
                    | "search_files"
                    | "edit_file"
                    | "create_directory"
                    | "rename_file"
            ),
            Self::FullAuto => true,
        }
    }

    /// Display label.
    pub fn label(&self) -> &str {
        match self {
            Self::ReadOnly => "ReadOnly",
            Self::WriteAllowed => "Write",
            Self::FullAuto => "FullAuto",
        }
    }
}

/// Autonomous agent session state.
pub struct AgentSession {
    /// What the agent is working on.
    pub task: String,
    /// Maximum tool iterations before stopping.
    pub max_iterations: usize,
    /// Current iteration count.
    pub iteration: usize,
    /// Files modified by the agent.
    pub files_changed: Vec<String>,
    /// Number of commands executed.
    pub commands_run: usize,
    /// When the session started.
    pub started_at: std::time::Instant,
    /// Trust level controlling auto-approval of tools.
    pub trust_level: TrustLevel,
    /// Whether the agent is paused.
    pub paused: bool,
    /// Unique session ID for persistence.
    pub session_id: String,
    /// File contents captured before agent started (for diff review).
    pub file_snapshots: std::collections::HashMap<String, String>,
    /// Execution plan (if agent was started with `:agent plan`).
    pub plan: Option<crate::agent_plan::AgentPlan>,
    /// Activity timeline.
    pub timeline: crate::agent_timeline::AgentTimeline,
    /// Subagent orchestrator.
    pub subagent_manager: crate::subagent::SubagentManager,
}

/// Surround editing state machine.
#[derive(Debug, Clone)]
pub enum SurroundState {
    /// `cs` pressed — waiting for the old delimiter char.
    ChangeWaitOld,
    /// `cs<old>` pressed — waiting for the new delimiter char.
    ChangeWaitNew(char),
    /// `ds` pressed — waiting for the delimiter char to delete.
    DeleteWait,
    /// `ys` pressed — waiting for motion to define range.
    AddWaitMotion,
    /// `ys` + motion resolved — waiting for the delimiter char to wrap.
    AddWaitDelimiter(usize, usize), // (start, end) char indices
}

/// Vim operator waiting for a motion (operator-pending mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    /// Delete (d).
    Delete,
    /// Change — delete and enter Insert mode (c).
    Change,
    /// Yank (y).
    Yank,
    /// Indent (>).
    Indent,
    /// Dedent (<).
    Dedent,
}

/// Character search mode (f/F/t/T).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindCharMode {
    /// Forward to char (f).
    Forward,
    /// Backward to char (F).
    Backward,
    /// Forward till before char (t).
    ForwardTill,
    /// Backward till after char (T).
    BackwardTill,
}

/// Which panel border is being resized by mouse drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelResizeDrag {
    /// Left sidebar right border (dragging changes sidebar width).
    LeftSidebar,
    /// Right panel left border (dragging changes chat/history/visor width).
    RightPanel,
    /// Terminal top border (dragging changes terminal height).
    Terminal,
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
    /// Block (column) visual selection mode.
    VisualBlock,
    /// User is typing a natural-language intent for the AI.
    Intent,
    /// Reviewing an AI-proposed change.
    Review,
    /// Viewing a side-by-side git diff (read-only).
    Diff,
    /// Merge conflict editor (3-panel: incoming | current | result).
    MergeConflict,
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
            Mode::VisualBlock => "V-BLOCK",
            Mode::Intent => "INTENT",
            Mode::Review => "REVIEW",
            Mode::Diff => "DIFF",
            Mode::MergeConflict => "MERGE",
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

/// An inline conflict detected in the buffer (for in-editor resolution).
pub struct InlineConflict {
    /// Line number of the `<<<<<<<` marker (0-indexed).
    pub marker_start: usize,
    /// Line number of the `=======` separator.
    pub separator: usize,
    /// Line number of the `>>>>>>>` marker.
    pub marker_end: usize,
}

/// Floating panel showing all references to a symbol.
pub struct ReferencesPanel {
    /// All reference locations.
    pub locations: Vec<crate::lsp::LspLocation>,
    /// Currently selected index.
    pub selected: usize,
}

impl ReferencesPanel {
    /// Create a new references panel.
    pub fn new(locations: Vec<crate::lsp::LspLocation>) -> Self {
        Self {
            locations,
            selected: 0,
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
        let max = self.locations.len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
    }

    /// Get the selected location.
    pub fn selected_location(&self) -> Option<&crate::lsp::LspLocation> {
        self.locations.get(self.selected)
    }
}

/// Inline peek definition popup showing a symbol's definition without navigating.
pub struct PeekDefinition {
    /// Resolved file path of the definition.
    pub file_path: std::path::PathBuf,
    /// Line number (0-indexed) of the definition in the target file.
    pub target_line: usize,
    /// Column (0-indexed) of the definition.
    pub target_col: usize,
    /// Lines of source content to display.
    pub lines: Vec<String>,
    /// First line number in the original file (for line number display).
    pub first_line: usize,
    /// Scroll offset within the popup.
    pub scroll_offset: usize,
    /// Syntax-highlighted colour per character per line.
    pub highlighted: Vec<crate::highlight::HighlightedLine>,
}

/// Format a `KeyEvent` as a human-readable string (e.g. "a", "Ctrl+s", "Esc").
pub fn format_key_event(key: &crossterm::event::KeyEvent) -> String {
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut parts = Vec::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("C");
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        parts.push("A");
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("S");
    }
    let code_str = match key.code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Backspace => "BS".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::Delete => "Del".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PgUp".to_string(),
        KeyCode::PageDown => "PgDn".to_string(),
        KeyCode::F(n) => format!("F{n}"),
        _ => "?".to_string(),
    };
    if parts.is_empty() {
        code_str
    } else {
        let prefix = parts.join("-");
        format!("{}-{}", prefix, code_str)
    }
}

/// Format a sequence of key events as a compact string.
pub fn format_key_sequence(keys: &[crossterm::event::KeyEvent]) -> String {
    keys.iter()
        .map(format_key_event)
        .collect::<Vec<_>>()
        .join(" ")
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
    /// Filtered command completions: (command, description, shortcut).
    pub command_completions: Vec<(String, String, String)>,
    /// Currently selected completion index.
    pub command_completion_idx: Option<usize>,
    /// Transient message shown in the status bar.
    pub status_message: Option<String>,
    /// Yank register (clipboard).
    pub register: Option<String>,
    /// Whether to show authorship markers in the gutter.
    pub show_authorship: bool,
    /// Leader key pending (Space was pressed, waiting for next key).
    pub leader_pending: bool,
    /// Pending operator waiting for a motion (operator-pending mode).
    pub pending_operator: Option<Operator>,
    /// Count prefix accumulator (e.g., "3" in "3dw").
    pub count_prefix: Option<usize>,
    /// Character search pending (f/F/t/T pressed, waiting for char).
    pub find_char_pending: Option<FindCharMode>,
    /// Last character search for ; and , repeat.
    pub last_find_char: Option<(FindCharMode, char)>,
    /// Replace char pending (r pressed, waiting for char).
    pub replace_char_pending: bool,
    /// Text object pending: true = inner (i), false = around (a). Waiting for delimiter.
    pub text_object_pending: Option<bool>,
    /// Last edit keys for dot repeat (Normal mode key sequence that performed an edit).
    pub last_edit_keys: Vec<crossterm::event::KeyEvent>,
    /// Whether we are currently recording keys for dot repeat.
    pub recording_edit: bool,
    /// Current edit key accumulator (reset on mode change back to Normal).
    pub current_edit_keys: Vec<crossterm::event::KeyEvent>,
    /// Macro recording: which register (a-z) is being recorded, if any.
    pub macro_recording: Option<char>,
    /// Macro registers: named key sequences (a-z).
    pub macro_registers: std::collections::HashMap<char, Vec<crossterm::event::KeyEvent>>,
    /// Whether we are currently playing back a macro (prevents recursive recording).
    pub macro_playing: bool,
    /// Waiting for register key to start recording (q was pressed).
    pub macro_record_pending: bool,
    /// Waiting for register key to play macro (@ was pressed).
    pub macro_play_pending: bool,
    /// Intent input buffer (what the user types in Intent mode).
    pub intent_input: String,
    /// Cached project rules (from .aura/rules.md or .aura/rules/*.md).
    pub project_rules: Option<String>,
    /// Document outline modal visible.
    pub outline_visible: bool,
    /// Document outline items: (line_number, label_text).
    pub outline_items: Vec<(usize, String)>,
    /// Filtered indices into outline_items.
    pub outline_filtered: Vec<usize>,
    /// Outline search query.
    pub outline_query: String,
    /// Outline selected index in filtered list.
    pub outline_selected: usize,
    /// Active AI proposal for review.
    pub proposal: Option<AiProposal>,
    /// AI backend (None if neither API key nor Claude Code CLI is available).
    ai_client: Option<AiBackend>,
    /// Receiver for streaming AI events.
    ai_receiver: Option<mpsc::Receiver<AiEvent>>,
    /// Whether `g` was pressed (waiting for second key: `g`→top, `d`→definition).
    pub g_pending: bool,
    /// Whether the z-prefix key has been pressed (fold commands).
    pub z_pending: bool,
    /// Whether `m` was pressed (setting a mark).
    pub mark_pending: bool,
    /// Whether `'` was pressed (jumping to a mark).
    pub jump_mark_pending: bool,
    /// Surround action pending state.
    pub surround_pending: Option<SurroundState>,
    /// Active autonomous agent session (None when not in agent mode).
    pub agent_mode: Option<AgentSession>,
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
    /// ACP (Agent Client Protocol) server for external agent integration.
    acp_server: Option<crate::acp_server::AcpServer>,
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
    /// Path to the config file for hot-reload.
    config_path: std::path::PathBuf,
    /// Last known modification time of the config file.
    config_mtime: Option<std::time::SystemTime>,
    /// Last time we checked the config file for changes.
    config_check_last: std::time::Instant,
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
    /// Embedded terminal tabs (multiple PTY instances).
    pub terminals: Vec<EmbeddedTerminal>,
    /// Index of the active terminal tab.
    pub active_terminal: usize,
    /// When `true`, keystrokes are routed to the terminal input instead of the
    /// editor.
    pub terminal_focused: bool,
    /// Whether the file tree sidebar has keyboard focus.
    pub file_tree_focused: bool,
    /// Fuzzy file picker overlay.
    pub file_picker: FilePicker,
    /// In-editor help overlay.
    pub help: HelpOverlay,
    /// Settings modal overlay.
    pub settings_modal: crate::settings_modal::SettingsModal,
    /// Command palette overlay.
    pub command_palette: crate::command_palette::CommandPalette,
    /// Branch picker modal.
    pub branch_picker: crate::branch_picker::BranchPicker,
    /// Git graph modal.
    pub git_graph: crate::git_graph::GitGraphModal,
    /// Claude Code activity watcher.
    pub claude_watcher: Option<crate::claude_watcher::ClaudeWatcher>,

    // --- Split panes ---
    /// Whether a split pane is active.
    pub split_active: bool,
    /// Direction of the split.
    pub split_direction: SplitDirection,
    /// Tab index shown in the secondary (right/bottom) pane.
    pub split_tab_idx: usize,
    /// Whether the secondary pane has focus (false = primary has focus).
    pub split_focus_secondary: bool,
    /// Whether split panes synchronize scroll position.
    pub split_scroll_sync: bool,
    /// Pending block insert/append: (start_row, end_row, column, is_append).
    /// Set when entering insert mode from visual block I/A.
    /// When exiting insert mode, the typed text is replicated to all rows.
    pub block_insert_pending: Option<(usize, usize, usize, bool)>,
    /// Cursor position when block insert started (to compute inserted text).
    pub block_insert_start_char: usize,
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
    /// Agent diff: list of (path, old_content, new_content) for multi-file review.
    pub agent_diff_files: Vec<(String, String, String)>,
    /// Current index in agent_diff_files.
    pub agent_diff_idx: usize,
    /// Active merge conflict editor (3-panel).
    pub merge_view: Option<crate::merge_view::MergeConflictView>,
    /// References panel (floating list of symbol references).
    pub references_panel: Option<ReferencesPanel>,
    /// Undo tree visualization modal.
    pub undo_tree: Option<crate::undo_tree::UndoTreeModal>,
    /// AI Visor panel (Claude Code config browser).
    pub ai_visor: crate::ai_visor::AiVisorPanel,
    /// Whether the AI Visor panel has keyboard focus.
    pub ai_visor_focused: bool,
    /// Project-wide search/replace panel.
    pub project_search: crate::project_search::ProjectSearchPanel,
    /// Inline conflict markers detected in the current buffer.
    pub inline_conflicts: Vec<InlineConflict>,
    /// Whether rename mode is active (typing new name in command bar).
    pub rename_active: bool,
    /// The rename input text.
    pub rename_input: String,
    /// Last time the source control panel was refreshed.
    last_sc_refresh: std::time::Instant,
    /// Last time auto-save was performed.
    auto_save_last: std::time::Instant,
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
    /// Cached rect for the "stage all" button in the git panel (for mouse clicks).
    pub stage_all_btn_rect: Rect,
    /// Cached rect for the AI commit message button (for mouse clicks).
    pub ai_commit_btn_rect: Rect,
    /// Receiver for streaming AI-generated commit message tokens.
    commit_msg_receiver: Option<mpsc::Receiver<AiEvent>>,
    /// Receiver for background conversation summarization (conversation_id, receiver).
    summarize_receiver: Option<(String, mpsc::Receiver<AiEvent>)>,
    /// Cached conversation history panel rect from the last render frame.
    pub conv_history_rect: Rect,
    /// Cached rect for the AI visor panel (mouse detection).
    pub ai_visor_rect: Rect,
    /// Cached tab bar rect from the last render frame (used for mouse click-to-switch).
    pub tab_bar_rect: Rect,
    /// Cached rect for the status bar (for mouse click detection).
    pub status_bar_rect: Rect,
    /// Close button hit areas: (tab_index, x_start, x_end) in absolute screen coords.
    pub tab_close_btn_ranges: Vec<(usize, u16, u16)>,
    /// Tab index pending close confirmation (unsaved changes dialog).
    pub tab_close_confirm: Option<usize>,

    // --- Collaboration ---
    /// Active collaboration session (None when not collaborating).
    pub collab: Option<crate::collab::CollabSession>,
    /// Last cursor position sent in an awareness update (for change detection).
    collab_last_cursor: Option<(usize, usize)>,
    /// Last selection sent in an awareness update.
    collab_last_selection: Option<((usize, usize), (usize, usize))>,
    /// Timestamp of last awareness broadcast (for throttling).
    collab_last_awareness: std::time::Instant,
    /// Last scroll position sent in an awareness update (for change detection).
    collab_last_scroll: Option<(usize, usize)>,
    /// Peer ID we are currently following in follow mode (None = not following).
    pub collab_follow_peer: Option<u64>,
    /// Last scroll position applied from the followed peer (for break detection).
    collab_follow_last_applied: Option<(usize, usize)>,
    /// Whether terminal sharing is active (host only).
    pub collab_sharing_terminal: bool,
    /// Timestamp of the last terminal snapshot broadcast.
    collab_last_terminal_broadcast: std::time::Instant,
    /// Hash of the last broadcast terminal snapshot (change detection).
    collab_last_terminal_hash: u64,
    /// Shared terminal snapshot received from the host (client only).
    pub collab_shared_terminal: Option<crate::embedded_terminal::TerminalSnapshot>,
    /// Whether to show the shared terminal instead of local terminal (client only).
    pub viewing_shared_terminal: bool,

    // --- Debugger ---
    /// Active DAP debug adapter client (None when not debugging).
    pub dap_client: Option<crate::dap::DapClient>,
    /// Debug panel (bottom panel: call stack, variables, output).
    pub debug_panel: crate::debug_panel::DebugPanel,
    /// Whether the debug panel has keyboard focus.
    pub debug_panel_focused: bool,
    /// Cached debug panel rect from the last render frame.
    pub debug_panel_rect: ratatui::layout::Rect,

    // --- Panel resize drag ---
    /// Which panel border is being dragged (if any).
    panel_resize_drag: Option<PanelResizeDrag>,

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
    /// History of previous search queries (most recent last).
    pub search_history: Vec<String>,
    /// Index into search_history when browsing with Up/Down (None = not browsing).
    pub search_history_idx: Option<usize>,
    /// All match positions as (start_char_idx, end_char_idx) pairs.
    pub search_matches: Vec<(usize, usize)>,
    /// Index into search_matches for the current/focused match.
    pub search_current: usize,

    // --- Peek definition ---
    /// Inline peek definition popup (shows definition without navigating).
    pub peek_definition: Option<PeekDefinition>,
    /// Flag: the next LSP Definition response should open peek, not navigate.
    pub peek_definition_pending: bool,

    // --- Terminal AI suggestions ---
    /// Current AI suggestion for the terminal prompt.
    pub terminal_suggestion: Option<String>,
    /// Receiver for terminal suggestion AI response.
    terminal_suggestion_rx: Option<mpsc::Receiver<AiEvent>>,
    /// Timestamp of last terminal keypress (for idle detection).
    pub terminal_last_key: std::time::Instant,
    /// Whether a terminal suggestion request is in flight.
    terminal_suggestion_pending: bool,

    // --- Registers modal ---
    /// Whether the registers modal is visible.
    pub registers_visible: bool,
    /// Selected index in the registers list.
    pub registers_selected: usize,
    /// Which macro register is being edited (None = not editing).
    pub macro_editing: Option<char>,
    /// Selected key index within the macro being edited.
    pub macro_edit_selected: usize,

    // --- Update checker ---
    /// Receiver for background update check results.
    update_receiver: Option<mpsc::Receiver<UpdateStatus>>,
    /// Latest update status (displayed in status bar).
    pub update_status: Option<UpdateStatus>,
    /// Whether the floating update notification toast is visible.
    pub update_notification_visible: bool,
    /// Whether the update confirmation modal is visible.
    pub update_modal_visible: bool,
    /// Cached rect for the update notification (for mouse click detection).
    pub update_notification_rect: Rect,
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
        let config_mtime = std::fs::metadata(&config_path)
            .and_then(|m| m.modified())
            .ok();
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

        // Start ACP server for external agent integration.
        let acp_server = match crate::acp_server::AcpServer::start() {
            Ok(server) => {
                tracing::info!("ACP server listening on port {}", server.port);
                Self::write_acp_discovery(server.port, buffer.file_path());
                Some(server)
            }
            Err(e) => {
                tracing::warn!("Failed to start ACP server: {}", e);
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
                    .inspect_err(|e| {
                        tracing::warn!("Failed to open conversation store at {:?}: {}", db_path, e)
                    })
                    .ok()
            })
            .or_else(|| {
                let fallback = dirs_path()?.join(".aura").join("conversations.db");
                tracing::debug!(
                    "Trying global fallback conversation store at {:?}",
                    fallback
                );
                ConversationStore::open(&fallback)
                    .inspect_err(|e| {
                        tracing::warn!("Failed to open fallback conversation store: {}", e)
                    })
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
            // Auto-compact on startup if enabled.
            if config.conversations.auto_compact {
                let compact_config = aura_core::CompactConfig {
                    max_message_age_days: config.conversations.max_message_age_days,
                    max_messages_per_conversation: config
                        .conversations
                        .max_messages_per_conversation,
                    max_conversations: config.conversations.max_conversations,
                    keep_recent_messages: config.conversations.keep_recent_messages,
                };
                match store.compact(&compact_config) {
                    Ok(stats) => {
                        if stats.messages_deleted > 0 || stats.conversations_deleted > 0 {
                            tracing::info!(
                                "Auto-compact: deleted {} messages, {} conversations",
                                stats.messages_deleted,
                                stats.conversations_deleted
                            );
                        }
                    }
                    Err(e) => tracing::warn!("Auto-compact failed: {e}"),
                }
            }
        } else {
            tracing::warn!("No conversation store available — AI history will not be recorded");
        }

        // Initialize speculative engine (reuses AI config with optional model override).
        let speculative = AiConfig::from_env().and_then(|ai_config| {
            let mut engine = SpeculativeEngine::new(ai_config).ok()?;
            engine.model_override = config.ai.model_for("speculative").to_string();
            // Clear override if it matches the default (no need to override).
            if engine.model_override == config.ai.model {
                engine.model_override.clear();
            }
            Some(engine)
        });

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
            command_completions: Vec::new(),
            command_completion_idx: None,
            status_message: None,
            register: None,
            pending_operator: None,
            count_prefix: None,
            find_char_pending: None,
            last_find_char: None,
            replace_char_pending: false,
            text_object_pending: None,
            last_edit_keys: Vec::new(),
            recording_edit: false,
            current_edit_keys: Vec::new(),
            macro_recording: None,
            macro_registers: std::collections::HashMap::new(),
            macro_playing: false,
            macro_record_pending: false,
            macro_play_pending: false,
            show_authorship: true,
            leader_pending: false,
            intent_input: String::new(),
            project_rules: None,
            outline_visible: false,
            outline_items: Vec::new(),
            outline_filtered: Vec::new(),
            outline_query: String::new(),
            outline_selected: 0,
            proposal: None,
            ai_client,
            ai_receiver: None,
            g_pending: false,
            z_pending: false,
            mark_pending: false,
            jump_mark_pending: false,
            surround_pending: None,
            agent_mode: None,
            conversation_store,
            active_conversation: None,
            active_intent_id: None,
            conversation_panel: None,
            show_conversations: false,
            mcp_server,
            acp_server,
            mcp_clients,
            agent_registry: AgentRegistry::default(),
            speculative,
            git_repo,
            show_blame: false,
            config: config.clone(),
            config_path,
            config_mtime,
            config_check_last: std::time::Instant::now(),
            theme,
            editing_proposal: false,
            revising_proposal: false,
            experimental_mode: false,
            plugin_manager: PluginManager::new(),
            terminals: {
                let port_str = mcp_port.map(|p| p.to_string());
                let mut env_vars: Vec<(&str, &str)> = Vec::new();
                if let Some(ref ps) = port_str {
                    env_vars.push(("AURA_MCP_PORT", ps.as_str()));
                }
                let mut t = EmbeddedTerminal::with_env(terminal_cwd.clone(), &env_vars);
                t.label = "Terminal 1".to_string();
                t.inject_shell_integration();
                vec![t]
            },
            active_terminal: 0,
            terminal_focused: false,
            file_tree_focused: false,
            file_picker: FilePicker::new(terminal_cwd.clone()),
            help: HelpOverlay::new(),
            settings_modal: crate::settings_modal::SettingsModal::new(),
            command_palette: crate::command_palette::CommandPalette::new(),
            branch_picker: crate::branch_picker::BranchPicker::new(),
            git_graph: crate::git_graph::GitGraphModal::new(),
            claude_watcher: crate::claude_watcher::ClaudeWatcher::start(&terminal_cwd),
            split_active: false,
            split_direction: SplitDirection::Vertical,
            split_tab_idx: 0,
            split_focus_secondary: false,
            split_scroll_sync: false,
            block_insert_pending: None,
            block_insert_start_char: 0,
            file_tree: FileTree::new(terminal_cwd),
            sidebar_view: SidebarView::Files,
            source_control: SourceControlPanel::new(30),
            source_control_focused: false,
            diff_view: None,
            agent_diff_files: Vec::new(),
            agent_diff_idx: 0,
            merge_view: None,
            references_panel: None,
            undo_tree: None,
            ai_visor: crate::ai_visor::AiVisorPanel::new(40),
            project_search: crate::project_search::ProjectSearchPanel::new(),
            inline_conflicts: Vec::new(),
            ai_visor_focused: false,
            rename_active: false,
            rename_input: String::new(),
            last_sc_refresh: std::time::Instant::now(),
            auto_save_last: std::time::Instant::now(),
            conversation_history: ConversationHistoryPanel::new(30),
            conversation_history_focused: false,
            chat_panel: {
                let mut cp = ChatPanel::new(40);
                cp.max_context_messages = config.ai.max_context_messages;
                cp
            },
            chat_panel_focused: false,
            chat_receiver: None,
            chat_panel_rect: Rect::default(),
            pending_tool_calls: Vec::new(),
            current_assistant_blocks: Vec::new(),
            tool_loop_system_prompt: String::new(),
            editor_rect: Rect::default(),
            terminal_rect: Rect::default(),
            file_tree_rect: Rect::default(),
            stage_all_btn_rect: Rect::default(),
            ai_commit_btn_rect: Rect::default(),
            commit_msg_receiver: None,
            summarize_receiver: None,
            conv_history_rect: Rect::default(),
            ai_visor_rect: Rect::default(),
            tab_bar_rect: Rect::default(),
            status_bar_rect: Rect::default(),
            tab_close_btn_ranges: Vec::new(),
            tab_close_confirm: None,
            collab: None,
            collab_last_cursor: None,
            collab_last_selection: None,
            collab_last_awareness: std::time::Instant::now(),
            collab_last_scroll: None,
            collab_follow_peer: None,
            collab_follow_last_applied: None,
            collab_sharing_terminal: false,
            collab_last_terminal_broadcast: std::time::Instant::now(),
            collab_last_terminal_hash: 0,
            collab_shared_terminal: None,
            viewing_shared_terminal: false,
            dap_client: None,
            debug_panel: crate::debug_panel::DebugPanel::new(),
            debug_panel_focused: false,
            debug_panel_rect: Rect::default(),
            panel_resize_drag: None,
            matching_bracket: None,
            search_query: None,
            search_input: String::new(),
            search_active: false,
            search_forward: true,
            search_history: Vec::new(),
            search_history_idx: None,
            search_matches: Vec::new(),
            search_current: 0,
            peek_definition: None,
            peek_definition_pending: false,
            terminal_suggestion: None,
            terminal_suggestion_rx: None,
            terminal_last_key: std::time::Instant::now(),
            terminal_suggestion_pending: false,
            registers_visible: false,
            registers_selected: 0,
            macro_editing: None,
            macro_edit_selected: 0,
            update_receiver: None,
            update_status: None,
            update_notification_visible: false,
            update_modal_visible: false,
            update_notification_rect: Rect::default(),
        };
        // Apply config settings.
        app.show_authorship = config.editor.show_authorship;
        app.chat_panel.max_context_messages = config.conversations.max_context_messages;

        // Kick off background summarization for eligible conversations.
        app.maybe_summarize_next();

        // Load project rules from .aura/rules.md or .aura/rules/*.md.
        app.load_project_rules();

        // Discover and load Lua plugins from ~/.aura/plugins/.
        for plugin in crate::plugin::discover_lua_plugins() {
            app.plugin_manager.register(plugin);
        }
        let plugin_count = app.plugin_manager.plugin_names().len();
        if plugin_count > 0 {
            tracing::info!("Loaded {plugin_count} plugin(s)");
        }

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

    /// Convenience: reference to the active terminal tab.
    #[inline]
    pub fn terminal(&self) -> &EmbeddedTerminal {
        &self.terminals[self.active_terminal]
    }

    /// Convenience: mutable reference to the active terminal tab.
    #[inline]
    pub fn terminal_mut(&mut self) -> &mut EmbeddedTerminal {
        &mut self.terminals[self.active_terminal]
    }

    /// Spawn a new terminal tab and make it active.
    pub fn new_terminal_tab(&mut self) {
        let cwd = self
            .tab()
            .buffer
            .file_path()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let idx = self.terminals.len() + 1;
        let mut t = EmbeddedTerminal::new(cwd);
        t.label = format!("Terminal {idx}");
        // Match height of existing terminals.
        t.height = self.terminal().height;
        t.visible = true;
        t.inject_shell_integration();
        self.terminals.push(t);
        self.active_terminal = self.terminals.len() - 1;
        self.terminal_focused = true;
    }

    /// Close the active terminal tab.
    pub fn close_terminal_tab(&mut self) {
        if self.terminals.len() <= 1 {
            // Last terminal — just hide it.
            self.terminal_mut().visible = false;
            self.terminal_focused = false;
            return;
        }
        self.terminals.remove(self.active_terminal);
        if self.active_terminal >= self.terminals.len() {
            self.active_terminal = self.terminals.len() - 1;
        }
    }

    /// Switch to the next terminal tab.
    pub fn next_terminal_tab(&mut self) {
        if self.terminals.len() > 1 {
            self.active_terminal = (self.active_terminal + 1) % self.terminals.len();
        }
    }

    /// Switch to the previous terminal tab.
    pub fn prev_terminal_tab(&mut self) {
        if self.terminals.len() > 1 {
            self.active_terminal = if self.active_terminal == 0 {
                self.terminals.len() - 1
            } else {
                self.active_terminal - 1
            };
        }
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

            // Poll for subagent streaming events.
            self.poll_subagent_events();

            // Poll for LSP events.
            self.poll_lsp_events();

            // Poll for MCP server requests and external MCP client events.
            self.poll_mcp_requests();
            self.poll_mcp_client_events();

            // Poll for ACP server requests.
            self.poll_acp_requests();

            // Poll for AI commit message tokens and conversation summarization.
            self.poll_commit_msg();
            self.poll_summarize();

            // Poll for terminal AI suggestion.
            self.poll_terminal_suggestion();

            // Request terminal suggestion after 2s idle when terminal is focused.
            if self.terminal_focused
                && self.terminal_suggestion.is_none()
                && !self.terminal_suggestion_pending
                && !self.terminal().command_running()
                && self.terminal_last_key.elapsed() > std::time::Duration::from_secs(2)
            {
                self.request_terminal_suggestion();
            }

            // Poll for DAP debugger events.
            self.poll_dap_events();

            // Poll for Claude Code activity.
            if let Some(watcher) = &mut self.claude_watcher {
                let events = watcher.poll_events();
                if !events.is_empty() {
                    self.persist_claude_code_activity(&events);
                }
            }

            // Poll for collaboration events, apply follow viewport, broadcast awareness.
            self.poll_collab_events();
            self.apply_follow_viewport();
            self.maybe_send_awareness();
            self.maybe_broadcast_terminal_snapshot();

            // Poll for update check result.
            self.poll_update_check();

            // Poll speculative engine and trigger analysis if idle.
            self.poll_speculative();
            self.maybe_trigger_analysis();
            self.update_edit_predictions();

            // Send debounced didChange and re-index if needed (300ms delay).
            if self.tab().lsp_last_change.elapsed() > Duration::from_millis(300) {
                if self.tab().lsp_change_pending {
                    self.send_lsp_did_change();
                    // Request inlay hints for the visible range after changes settle.
                    let scroll = self.tab().scroll_row as u32;
                    let viewport = 50u32; // request a bit more than visible
                    if let Some(ref mut lsp) = self.tab_mut().lsp_client {
                        lsp.request_inlay_hints(scroll, scroll + viewport);
                        lsp.request_semantic_tokens();
                        lsp.request_code_lens();
                    }
                }
                if self.tab().semantic_dirty {
                    self.refresh_semantic_index();
                }
            }

            // Hot-reload config: check aura.toml every 2 seconds.
            if self.config_check_last.elapsed() > Duration::from_secs(2) {
                self.config_check_last = std::time::Instant::now();
                let new_mtime = std::fs::metadata(&self.config_path)
                    .and_then(|m| m.modified())
                    .ok();
                if new_mtime != self.config_mtime && new_mtime.is_some() {
                    self.config_mtime = new_mtime;
                    let new_config = crate::config::load_config(&self.config_path);
                    let config_table = crate::config::load_config_table(&self.config_path);
                    let new_theme =
                        crate::config::resolve_theme(&new_config.theme, config_table.as_ref());
                    self.config = new_config;
                    self.theme = new_theme;
                    self.set_status("Config reloaded from aura.toml");
                }
            }

            // Auto-refresh source control panel every 2 seconds when visible.
            if self.sidebar_view == SidebarView::Git
                && self.last_sc_refresh.elapsed() > Duration::from_secs(2)
            {
                self.refresh_source_control();
            }

            // Auto-save modified buffers on interval.
            let auto_secs = self.config.editor.auto_save_seconds;
            if auto_secs > 0 && self.auto_save_last.elapsed() > Duration::from_secs(auto_secs) {
                self.auto_save_last = std::time::Instant::now();
                for tab in self.tabs.tabs_mut() {
                    if tab.buffer.is_modified() && tab.buffer.file_path().is_some() {
                        let _ = tab.buffer.save();
                    }
                }
            }

            // Poll for terminal events with a small timeout.
            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) => self.handle_key(key.code, key.modifiers),
                    Event::Mouse(mouse) => match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            // Check if click is on a panel border for resizing.
                            if let Some(drag) = self.detect_panel_border(mouse.column, mouse.row) {
                                self.panel_resize_drag = Some(drag);
                            } else {
                                self.panel_resize_drag = None;
                                self.handle_mouse_click(mouse.column, mouse.row);
                            }
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            if let Some(drag) = self.panel_resize_drag {
                                self.apply_panel_resize(drag, mouse.column, mouse.row);
                            } else {
                                self.handle_mouse_drag(mouse.column, mouse.row);
                            }
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            self.panel_resize_drag = None;
                            // Clear the drag anchor if no selection was made.
                            if !matches!(
                                self.mode,
                                Mode::Visual | Mode::VisualLine | Mode::VisualBlock
                            ) {
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

        // Clean up discovery files.
        Self::remove_mcp_discovery();
        Self::remove_acp_discovery();

        // Shutdown ACP server.
        if let Some(server) = &self.acp_server {
            server.shutdown();
        }

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
    pub fn save_session(&self) {
        let root = match self.session_project_root() {
            Some(r) => r,
            None => return,
        };
        let path = session::session_path(&root);

        let tabs: Vec<TabState> = self
            .tabs
            .tabs()
            .iter()
            .map(|tab| {
                let marks = tab
                    .marks
                    .iter()
                    .map(|(&c, cur)| (c, (cur.row, cur.col)))
                    .collect();
                let folded_ranges = tab
                    .folded_ranges
                    .iter()
                    .map(|(&start, &end)| (start, end))
                    .collect();
                TabState {
                    file_path: tab.buffer.file_path().map(|p| p.to_path_buf()),
                    cursor_row: tab.cursor.row,
                    cursor_col: tab.cursor.col,
                    scroll_row: tab.scroll_row,
                    scroll_col: tab.scroll_col,
                    marks,
                    folded_ranges,
                    pinned: tab.pinned,
                }
            })
            .collect();

        let sidebar_view = match self.sidebar_view {
            SidebarView::Files => "files",
            SidebarView::Git => "git",
        };

        // Serialize macro registers.
        let macro_registers: std::collections::HashMap<char, Vec<String>> = self
            .macro_registers
            .iter()
            .map(|(&c, keys)| {
                let strs = keys.iter().map(session::format_key_event).collect();
                (c, strs)
            })
            .collect();

        let session = Session {
            working_directory: root.clone(),
            tabs,
            active_tab: self.tabs.active_index(),
            ui: UiState {
                file_tree_visible: self.file_tree.visible,
                chat_panel_visible: self.chat_panel.visible,
                terminal_visible: self.terminal().visible,
                sidebar_view: sidebar_view.into(),
                file_tree_width: Some(self.file_tree.width),
                chat_panel_width: Some(self.chat_panel.width),
                terminal_height: Some(self.terminal().height),
                conversation_history_width: Some(self.conversation_history.width),
                conversation_history_visible: self.conversation_history.visible,
                ai_visor_width: Some(self.ai_visor.width),
                ai_visor_visible: self.ai_visor.visible,
                debug_panel_visible: self.debug_panel.visible,
                split_active: self.split_active,
                split_direction: if self.split_active {
                    Some(
                        match self.split_direction {
                            crate::app::SplitDirection::Vertical => "vertical",
                            crate::app::SplitDirection::Horizontal => "horizontal",
                        }
                        .into(),
                    )
                } else {
                    None
                },
                split_tab_idx: if self.split_active {
                    Some(self.split_tab_idx)
                } else {
                    None
                },
                macro_registers,
                search_history: self
                    .search_history
                    .iter()
                    .rev()
                    .take(50)
                    .rev()
                    .cloned()
                    .collect(),
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
    /// Restore the default session from `.aura/session.json`.
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
        self.apply_session(session);
    }

    /// Apply a loaded session (restore tabs, UI state, etc.).
    pub fn apply_session(&mut self, session: Session) {
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
            let mut tab = EditorTab::new(buffer, self.conversation_store.as_ref(), &self.theme);
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

            // Restore marks.
            for (&c, &(row, col)) in &tab_state.marks {
                tab.marks
                    .insert(c, aura_core::Cursor::new(row.min(max_row), col));
            }

            // Restore folded ranges.
            for &(start, end) in &tab_state.folded_ranges {
                if start < tab.buffer.line_count() && end <= tab.buffer.line_count() {
                    tab.folded_ranges.insert(start, end);
                }
            }

            // Restore pinned state.
            tab.pinned = tab_state.pinned;

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
        self.terminal_mut().visible = session.ui.terminal_visible;
        self.sidebar_view = match session.ui.sidebar_view.as_str() {
            "git" => SidebarView::Git,
            _ => SidebarView::Files,
        };

        // Restore panel sizes.
        if let Some(w) = session.ui.file_tree_width {
            self.file_tree.width = w.max(15);
        }
        if let Some(w) = session.ui.chat_panel_width {
            self.chat_panel.width = w.max(20);
        }
        if let Some(h) = session.ui.terminal_height {
            self.terminal_mut().height = h.clamp(3, 50);
        }
        if let Some(w) = session.ui.conversation_history_width {
            self.conversation_history.width = w.max(20);
        }
        if let Some(w) = session.ui.ai_visor_width {
            self.ai_visor.width = w.max(20);
        }

        // Restore additional panel visibility.
        self.conversation_history.visible = session.ui.conversation_history_visible;
        self.ai_visor.visible = session.ui.ai_visor_visible;
        self.debug_panel.visible = session.ui.debug_panel_visible;

        // Restore split pane layout.
        if session.ui.split_active {
            if let Some(idx) = session.ui.split_tab_idx {
                if idx < self.tabs.count() {
                    self.split_active = true;
                    self.split_tab_idx = idx;
                    self.split_direction =
                        match session.ui.split_direction.as_deref().unwrap_or("vertical") {
                            "horizontal" => SplitDirection::Horizontal,
                            _ => SplitDirection::Vertical,
                        };
                }
            }
        }

        // Restore macro registers.
        for (c, key_strs) in &session.ui.macro_registers {
            let keys: Vec<crossterm::event::KeyEvent> = key_strs
                .iter()
                .filter_map(|s| session::parse_key_event(s))
                .collect();
            if !keys.is_empty() {
                self.macro_registers.insert(*c, keys);
            }
        }

        // Restore search history.
        self.search_history = session.ui.search_history.clone();

        self.set_status(format!(
            "Session restored ({} tab{})",
            self.tabs.count(),
            if self.tabs.count() == 1 { "" } else { "s" }
        ));
    }

    /// Route key events based on the current mode.
    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let key_event = crossterm::event::KeyEvent::new(code, modifiers);
        let prev_mode = self.mode;

        // Record key for macro if recording (and not playing back).
        if self.macro_recording.is_some() && !self.macro_playing {
            // Don't record the 'q' that stops recording.
            let is_stop_q =
                self.mode == Mode::Normal && code == KeyCode::Char('q') && modifiers.is_empty();
            if !is_stop_q {
                if let Some(reg) = self.macro_recording {
                    self.macro_registers.entry(reg).or_default().push(key_event);
                }
            }
        }

        // Record keys for dot repeat (edits in Normal/Insert).
        if self.recording_edit {
            self.current_edit_keys.push(key_event);
        }

        match self.mode {
            Mode::Normal => crate::input::handle_normal(self, code, modifiers),
            Mode::Insert => crate::input::handle_insert(self, code, modifiers),
            Mode::Command => crate::input::handle_command(self, code, modifiers),
            Mode::Visual | Mode::VisualLine | Mode::VisualBlock => {
                crate::input::handle_visual(self, code, modifiers)
            }
            Mode::Intent => crate::input::handle_intent(self, code, modifiers),
            Mode::Review => crate::input::handle_review(self, code, modifiers),
            Mode::Diff => crate::input::handle_diff(self, code, modifiers),
            Mode::MergeConflict => crate::input::handle_merge_conflict(self, code, modifiers),
        }

        // Detect when an edit starts (Normal → Insert via c, s, o, etc.)
        // and when it ends (Insert → Normal via Esc).
        if prev_mode == Mode::Normal && self.mode == Mode::Insert && !self.recording_edit {
            self.recording_edit = true;
            // The key(s) that triggered the mode change are already in current_edit_keys
            // if recording started this frame.
        }
        if prev_mode == Mode::Insert && self.mode == Mode::Normal && self.recording_edit {
            self.recording_edit = false;
            if !self.current_edit_keys.is_empty() {
                self.last_edit_keys = std::mem::take(&mut self.current_edit_keys);
            }
        }
        // For single-key edits in Normal mode (x, dd, J, ~, r, etc.),
        // the edit is complete immediately — no Insert mode involved.
        // These are captured by start_edit_record/stop_edit_record calls
        // in the input handler.
    }

    /// Start recording keys for dot repeat (called by input handlers for
    /// single-key edits like x, dd, J, ~).
    pub fn start_dot_record(&mut self) {
        if !self.recording_edit && !self.macro_playing {
            self.current_edit_keys.clear();
            self.recording_edit = true;
        }
    }

    /// Stop recording and save as last edit (for single-key edits).
    pub fn stop_dot_record(&mut self) {
        if self.recording_edit {
            self.recording_edit = false;
            if !self.current_edit_keys.is_empty() {
                self.last_edit_keys = std::mem::take(&mut self.current_edit_keys);
            }
        }
    }

    /// Replay the last edit (dot repeat).
    pub fn dot_repeat(&mut self) {
        if self.last_edit_keys.is_empty() {
            return;
        }
        let keys = self.last_edit_keys.clone();
        self.macro_playing = true; // Prevent re-recording.
        for key in &keys {
            self.handle_key(key.code, key.modifiers);
        }
        self.macro_playing = false;
    }

    /// Start recording a macro into register.
    pub fn start_macro_recording(&mut self, register: char) {
        self.macro_recording = Some(register);
        self.macro_registers.insert(register, Vec::new());
        self.set_status(format!("Recording @{register}..."));
    }

    /// Stop recording the current macro.
    pub fn stop_macro_recording(&mut self) {
        if let Some(reg) = self.macro_recording.take() {
            let count = self.macro_registers.get(&reg).map(|v| v.len()).unwrap_or(0);
            self.set_status(format!("Recorded @{reg} ({count} keys)"));
        }
    }

    /// Play back a macro from register.
    pub fn play_macro(&mut self, register: char) {
        let keys = match self.macro_registers.get(&register) {
            Some(k) if !k.is_empty() => k.clone(),
            _ => {
                self.set_status(format!("Empty macro @{register}"));
                return;
            }
        };
        self.macro_playing = true;
        for key in &keys {
            self.handle_key(key.code, key.modifiers);
        }
        self.macro_playing = false;
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
            self.set_status(
                "No AI backend available. Set ANTHROPIC_API_KEY or install Claude Code CLI",
            );
            self.mode = Mode::Normal;
            return;
        }

        // Ensure conversation store is ready before we borrow ai_client.
        self.ensure_conversation_store();

        let client = match self.ai_client.as_ref() {
            Some(c) => c,
            None => return, // Already guarded above, but be safe.
        };

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
            match store.create_conversation(
                &file_path_str,
                start_line,
                end_line,
                commit.as_deref(),
                branch.as_deref(),
            ) {
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
                    tracing::info!("Created conversation {} for '{}'", conv.id, file_path_str);
                }
                Err(e) => {
                    tracing::error!("Failed to create conversation for '{}': {e}", file_path_str);
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
        // Broadcast CRDT changes to collab peers.
        self.broadcast_collab_sync();
    }

    /// Regenerate syntax highlights from the current buffer content.
    pub fn refresh_highlights(&mut self) {
        let theme = self.theme.clone();
        self.tab_mut().refresh_highlights(&theme);
        self.refresh_foldable_ranges();
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
        let history = self.tab().buffer.full_history();
        let history_pos = self.tab().buffer.history_pos();
        let now = std::time::Instant::now();
        let entries = crate::undo_tree::build_entries(history, history_pos, now);
        if entries.is_empty() {
            self.set_status("No edits in history");
            return;
        }
        let modal = crate::undo_tree::UndoTreeModal::new(entries, history_pos);
        self.undo_tree = Some(modal);
        self.set_status("Undo tree — j/k navigate, Enter restore, t toggle detail, Esc close");
    }

    /// Restore the buffer to the selected undo tree position.
    pub fn restore_to_undo_pos(&mut self) {
        let target_pos = match &self.undo_tree {
            Some(modal) => modal.selected_history_pos(),
            None => return,
        };
        let target_pos = match target_pos {
            Some(p) => p,
            None => return,
        };
        self.tab_mut().buffer.restore_to(target_pos);
        self.tab_mut().mark_highlights_dirty();
        self.undo_tree = None;
        self.set_status(format!("Restored to history position {target_pos}"));
    }

    // ── Git Stash & PR ───────────────────────────────────────────

    /// Push current changes to a stash.
    pub fn sc_stash_push(&mut self) {
        if let Some(repo) = &self.git_repo {
            match repo.stash_push("WIP") {
                Ok(()) => {
                    self.set_status("Changes stashed");
                    self.refresh_source_control();
                }
                Err(e) => self.set_status(format!("Stash failed: {e}")),
            }
        }
    }

    /// Pop the selected stash (or the top one).
    pub fn sc_stash_pop(&mut self) {
        let name = self
            .source_control
            .selected_stash()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "stash@{0}".to_string());

        if let Some(repo) = &self.git_repo {
            match repo.stash_pop(&name) {
                Ok(()) => {
                    self.set_status(format!("Popped stash: {name}"));
                    self.refresh_source_control();
                }
                Err(e) => self.set_status(format!("Stash pop failed: {e}")),
            }
        }
    }

    /// Drop the selected stash.
    pub fn sc_stash_drop(&mut self) {
        let name = match self.source_control.selected_stash() {
            Some(s) => s.name.clone(),
            None => {
                self.set_status("No stash selected");
                return;
            }
        };

        if let Some(repo) = &self.git_repo {
            match repo.stash_drop(&name) {
                Ok(()) => {
                    self.set_status(format!("Dropped stash: {name}"));
                    self.refresh_source_control();
                }
                Err(e) => self.set_status(format!("Stash drop failed: {e}")),
            }
        }
    }

    /// Send the last failed terminal command to the AI chat for diagnosis and fix.
    pub fn fix_last_failed_command(&mut self) {
        if let Some(cmd_record) = self.terminal().last_failed_command() {
            let exit = cmd_record.exit_code.unwrap_or(-1);
            let msg = format!(
                "The command `{}` failed with exit code {}. What went wrong and how can I fix it?",
                cmd_record.command, exit
            );
            // Open chat panel and inject the message.
            if !self.chat_panel.visible {
                self.chat_panel.visible = true;
                self.chat_panel_focused = true;
                self.load_chat_conversation();
            }
            self.chat_panel.input = msg;
            self.set_status(format!(
                "Fix: sent failed command '{}' to chat",
                cmd_record.command
            ));
        } else {
            self.set_status("No failed command to fix");
        }
    }

    /// Create a pull request using `gh` CLI.
    pub fn create_pr(&mut self) {
        let branch = match self.git_branch() {
            Some(b) => b,
            None => {
                self.set_status("Not on a branch");
                return;
            }
        };

        if branch == "main" || branch == "master" {
            self.set_status("Cannot create PR from main/master branch");
            return;
        }

        // Open terminal and run gh pr create interactively.
        self.terminal_mut().visible = true;
        self.terminal_focused = true;
        let cmd = format!("gh pr create --head {branch}");
        self.terminal_mut().send_bytes(cmd.as_bytes());
        self.terminal_mut().send_enter();
        self.set_status(format!("Creating PR for branch '{branch}'..."));
    }

    // ── Task Runner ──────────────────────────────────────────────

    /// Run a named task in the embedded terminal.
    pub fn run_task(&mut self, name: &str) {
        let tasks = self.get_tasks();
        let task = match tasks.get(name) {
            Some(t) => t.clone(),
            None => {
                let available: Vec<&str> = tasks.keys().map(|s| s.as_str()).collect();
                if available.is_empty() {
                    self.set_status("No tasks configured. Add [tasks] to aura.toml");
                } else {
                    self.set_status(format!(
                        "Unknown task '{}'. Available: {}",
                        name,
                        available.join(", ")
                    ));
                }
                return;
            }
        };

        // Show terminal and send the command.
        self.terminal_mut().visible = true;
        self.terminal_focused = true;
        self.terminal_mut().send_bytes(task.command.as_bytes());
        self.terminal_mut().send_enter();
        self.set_status(format!("Running task: {} ({})", name, task.command));
    }

    /// Get all available tasks (configured + auto-detected).
    pub fn get_tasks(&self) -> std::collections::HashMap<String, crate::config::TaskConfig> {
        if !self.config.tasks.is_empty() {
            return self.config.tasks.clone();
        }
        // Auto-detect from project type.
        self.auto_detect_tasks()
    }

    /// Auto-detect tasks based on project files.
    fn auto_detect_tasks(&self) -> std::collections::HashMap<String, crate::config::TaskConfig> {
        use crate::config::TaskConfig;
        let mut tasks = std::collections::HashMap::new();
        let root = self
            .tab()
            .buffer
            .file_path()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        if root.join("Cargo.toml").exists() {
            tasks.insert(
                "build".into(),
                TaskConfig {
                    command: "cargo build".into(),
                    description: "Build the project".into(),
                },
            );
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: "cargo test".into(),
                    description: "Run tests".into(),
                },
            );
            tasks.insert(
                "clippy".into(),
                TaskConfig {
                    command: "cargo clippy -- -D warnings".into(),
                    description: "Run lints".into(),
                },
            );
            tasks.insert(
                "fmt".into(),
                TaskConfig {
                    command: "cargo fmt --all".into(),
                    description: "Format code".into(),
                },
            );
        } else if root.join("package.json").exists() {
            tasks.insert(
                "build".into(),
                TaskConfig {
                    command: "npm run build".into(),
                    description: "Build the project".into(),
                },
            );
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: "npm test".into(),
                    description: "Run tests".into(),
                },
            );
            tasks.insert(
                "lint".into(),
                TaskConfig {
                    command: "npm run lint".into(),
                    description: "Run lints".into(),
                },
            );
            tasks.insert(
                "dev".into(),
                TaskConfig {
                    command: "npm run dev".into(),
                    description: "Start dev server".into(),
                },
            );
        } else if root.join("Makefile").exists() || root.join("makefile").exists() {
            tasks.insert(
                "build".into(),
                TaskConfig {
                    command: "make".into(),
                    description: "Build (default target)".into(),
                },
            );
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: "make test".into(),
                    description: "Run tests".into(),
                },
            );
            tasks.insert(
                "clean".into(),
                TaskConfig {
                    command: "make clean".into(),
                    description: "Clean build artifacts".into(),
                },
            );
        } else if root.join("go.mod").exists() {
            tasks.insert(
                "build".into(),
                TaskConfig {
                    command: "go build ./...".into(),
                    description: "Build the project".into(),
                },
            );
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: "go test ./...".into(),
                    description: "Run tests".into(),
                },
            );
            tasks.insert(
                "fmt".into(),
                TaskConfig {
                    command: "gofmt -w .".into(),
                    description: "Format code".into(),
                },
            );
        } else if root.join("mix.exs").exists() {
            tasks.insert(
                "build".into(),
                TaskConfig {
                    command: "mix compile".into(),
                    description: "Compile the project".into(),
                },
            );
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: "mix test".into(),
                    description: "Run tests".into(),
                },
            );
            tasks.insert(
                "fmt".into(),
                TaskConfig {
                    command: "mix format".into(),
                    description: "Format code".into(),
                },
            );
            tasks.insert(
                "deps".into(),
                TaskConfig {
                    command: "mix deps.get".into(),
                    description: "Fetch dependencies".into(),
                },
            );
            tasks.insert(
                "server".into(),
                TaskConfig {
                    command: "mix phx.server".into(),
                    description: "Start Phoenix server".into(),
                },
            );
        } else if root.join("pubspec.yaml").exists() {
            // Dart / Flutter
            tasks.insert(
                "build".into(),
                TaskConfig {
                    command: "dart compile exe lib/main.dart".into(),
                    description: "Compile Dart".into(),
                },
            );
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: "dart test".into(),
                    description: "Run tests".into(),
                },
            );
            tasks.insert(
                "fmt".into(),
                TaskConfig {
                    command: "dart format .".into(),
                    description: "Format code".into(),
                },
            );
            tasks.insert(
                "deps".into(),
                TaskConfig {
                    command: "dart pub get".into(),
                    description: "Fetch dependencies".into(),
                },
            );
            if root.join("lib").join("main.dart").exists() {
                tasks.insert(
                    "run".into(),
                    TaskConfig {
                        command: "flutter run".into(),
                        description: "Run Flutter app".into(),
                    },
                );
            }
        } else if root.join("build.zig").exists() {
            // Zig
            tasks.insert(
                "build".into(),
                TaskConfig {
                    command: "zig build".into(),
                    description: "Build the project".into(),
                },
            );
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: "zig build test".into(),
                    description: "Run tests".into(),
                },
            );
        } else if root.join("build.sbt").exists() {
            // Scala / sbt
            tasks.insert(
                "build".into(),
                TaskConfig {
                    command: "sbt compile".into(),
                    description: "Compile the project".into(),
                },
            );
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: "sbt test".into(),
                    description: "Run tests".into(),
                },
            );
            tasks.insert(
                "run".into(),
                TaskConfig {
                    command: "sbt run".into(),
                    description: "Run the project".into(),
                },
            );
        } else if root.join("stack.yaml").exists() || root.join("cabal.project").exists() {
            // Haskell
            let tool = if root.join("stack.yaml").exists() {
                "stack"
            } else {
                "cabal"
            };
            tasks.insert(
                "build".into(),
                TaskConfig {
                    command: format!("{tool} build"),
                    description: "Build the project".into(),
                },
            );
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: format!("{tool} test"),
                    description: "Run tests".into(),
                },
            );
            tasks.insert(
                "run".into(),
                TaskConfig {
                    command: format!("{tool} run"),
                    description: "Run the project".into(),
                },
            );
        } else if root.join("requirements.txt").exists() || root.join("pyproject.toml").exists() {
            tasks.insert(
                "test".into(),
                TaskConfig {
                    command: "pytest".into(),
                    description: "Run tests".into(),
                },
            );
            tasks.insert(
                "lint".into(),
                TaskConfig {
                    command: "ruff check .".into(),
                    description: "Run lints".into(),
                },
            );
            tasks.insert(
                "fmt".into(),
                TaskConfig {
                    command: "black .".into(),
                    description: "Format code".into(),
                },
            );
        }

        tasks
    }

    // ── Document Outline ─────────────────────────────────────────

    /// Open the document outline modal.
    pub fn open_outline(&mut self) {
        let mut items = Vec::new();
        for &start_line in self.tab().foldable_ranges.keys() {
            if let Some(text) = self.tab().buffer.line_text(start_line) {
                let label = text.trim().to_string();
                if !label.is_empty() {
                    items.push((start_line, label));
                }
            }
        }
        items.sort_by_key(|(line, _)| *line);
        self.outline_items = items;
        self.outline_filtered = (0..self.outline_items.len()).collect();
        self.outline_query.clear();
        self.outline_selected = 0;
        self.outline_visible = true;
    }

    /// Filter outline items by query.
    pub fn filter_outline(&mut self) {
        let query = self.outline_query.to_lowercase();
        self.outline_selected = 0;
        if query.is_empty() {
            self.outline_filtered = (0..self.outline_items.len()).collect();
            return;
        }
        self.outline_filtered = self
            .outline_items
            .iter()
            .enumerate()
            .filter(|(_, (_, label))| label.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect();
    }

    /// Jump to the selected outline item.
    pub fn goto_outline_selection(&mut self) {
        if let Some(&idx) = self.outline_filtered.get(self.outline_selected) {
            if let Some(&(line, _)) = self.outline_items.get(idx) {
                self.tab_mut().cursor.row = line;
                self.tab_mut().cursor.col = 0;
                self.tab_mut().scroll_row = line.saturating_sub(10);
            }
        }
        self.outline_visible = false;
    }

    // ── Registers Modal ────────────────────────────────────────

    /// Build the list of register entries for display.
    /// Returns: Vec of (register_name, content_preview).
    pub fn register_entries(&self) -> Vec<(String, String)> {
        let mut entries = Vec::new();

        // Yank register (").
        if let Some(ref text) = self.register {
            let preview: String = text.chars().take(60).collect();
            entries.push(("\"".to_string(), preview));
        }

        // Macro registers (a-z), sorted.
        let mut keys: Vec<char> = self.macro_registers.keys().copied().collect();
        keys.sort();
        for ch in keys {
            if let Some(keys_vec) = self.macro_registers.get(&ch) {
                let preview = format_key_sequence(keys_vec);
                entries.push((ch.to_string(), preview));
            }
        }

        entries
    }

    /// Open the registers modal.
    pub fn open_registers_modal(&mut self) {
        self.registers_selected = 0;
        self.macro_editing = None;
        self.registers_visible = true;
    }

    /// Enter macro editing mode for the selected register.
    pub fn edit_selected_macro(&mut self) {
        let entries = self.register_entries();
        if let Some((name, _)) = entries.get(self.registers_selected) {
            if let Some(ch) = name.chars().next() {
                // Only macro registers (a-z) are editable.
                if ch.is_ascii_lowercase() && self.macro_registers.contains_key(&ch) {
                    self.macro_editing = Some(ch);
                    self.macro_edit_selected = 0;
                } else {
                    self.set_status("Only macro registers (a-z) can be edited");
                }
            }
        }
    }

    /// Delete the selected key from the macro being edited.
    pub fn delete_macro_key(&mut self) {
        if let Some(ch) = self.macro_editing {
            if let Some(keys) = self.macro_registers.get_mut(&ch) {
                if !keys.is_empty() && self.macro_edit_selected < keys.len() {
                    keys.remove(self.macro_edit_selected);
                    if self.macro_edit_selected >= keys.len() && self.macro_edit_selected > 0 {
                        self.macro_edit_selected -= 1;
                    }
                }
                if keys.is_empty() {
                    self.macro_registers.remove(&ch);
                    self.macro_editing = None;
                }
            }
        }
    }

    // ── Project Rules ────────────────────────────────────────────

    /// Load project rules from `.aura/rules.md` or `.aura/rules/*.md`.
    pub fn load_project_rules(&mut self) {
        let project_root = self
            .tab()
            .buffer
            .file_path()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        let mut rules = String::new();

        // Check single-file rules first: .aura/rules.md
        let single_file = project_root.join(".aura/rules.md");
        if single_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&single_file) {
                rules.push_str(&content);
            }
        }

        // Check rules directory: .aura/rules/*.md
        let rules_dir = project_root.join(".aura/rules");
        if rules_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&rules_dir) {
                let mut rule_files: Vec<_> = entries
                    .flatten()
                    .filter(|e| {
                        e.path()
                            .extension()
                            .is_some_and(|ext| ext == "md" || ext == "txt")
                    })
                    .collect();
                rule_files.sort_by_key(|e| e.file_name());

                for entry in rule_files {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if !rules.is_empty() {
                            rules.push('\n');
                        }
                        rules.push_str(&content);
                    }
                }
            }
        }

        self.project_rules = if rules.is_empty() { None } else { Some(rules) };
    }

    // ── Autonomous Agent Mode ────────────────────────────────────

    /// Start an autonomous agent session with the given task.
    /// Start an autonomous agent session.
    pub fn start_agent(&mut self, task: &str, max_iterations: usize) {
        self.start_agent_with_options(task, max_iterations, TrustLevel::FullAuto, false);
    }

    /// Start an agent session with explicit options.
    pub fn start_agent_with_options(
        &mut self,
        task: &str,
        max_iterations: usize,
        trust_level: TrustLevel,
        planning: bool,
    ) {
        if self.ai_client.is_none() {
            self.set_status("No AI backend available for agent mode");
            return;
        }

        // Ensure chat panel is open.
        if !self.chat_panel.visible {
            self.chat_panel.visible = true;
            self.chat_panel_focused = true;
        }

        // Snapshot all open files for diff review later.
        let mut file_snapshots = std::collections::HashMap::new();
        for tab in self.tabs.tabs() {
            if let Some(path) = tab.buffer.file_path() {
                if let Ok(content) = std::fs::read_to_string(path) {
                    file_snapshots.insert(path.display().to_string(), content);
                }
            }
        }

        let session_id = format!(
            "{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );

        self.agent_mode = Some(AgentSession {
            task: task.to_string(),
            max_iterations,
            iteration: 0,
            files_changed: Vec::new(),
            commands_run: 0,
            started_at: std::time::Instant::now(),
            trust_level,
            paused: false,
            session_id,
            file_snapshots,
            plan: None,
            timeline: crate::agent_timeline::AgentTimeline::default(),
            subagent_manager: crate::subagent::SubagentManager::default(),
        });

        // Build the agent prompt.
        let trust_note = match trust_level {
            TrustLevel::ReadOnly => {
                "Read-only tools are auto-approved. Write/command tools require user approval."
            }
            TrustLevel::WriteAllowed => {
                "Read and write tools are auto-approved. Shell commands require user approval."
            }
            TrustLevel::FullAuto => "All tools are auto-approved.",
        };

        let agent_prompt = if planning {
            format!(
                "You are in AUTONOMOUS AGENT MODE with PLANNING.\n\n\
                 Trust level: {trust_note}\n\n\
                 Your task: {task}\n\n\
                 FIRST: Create a numbered execution plan. Format each step as:\n\
                 1. Description of what to do\n\
                 2. Next step\n\
                 ...\n\n\
                 Present the plan and wait for approval before executing."
            )
        } else {
            format!(
                "You are in AUTONOMOUS AGENT MODE.\n\n\
                 Trust level: {trust_note}\n\n\
                 Your task: {task}\n\n\
                 Work autonomously to complete this task:\n\
                 1. Analyze the codebase to understand what needs to change\n\
                 2. Make the necessary edits\n\
                 3. Run tests/checks to verify your changes work\n\
                 4. Fix any errors that arise\n\
                 5. When done, respond with a summary of what you did\n\n\
                 Be thorough but efficient. You have up to {max_iterations} tool iterations."
            )
        };

        // Inject as user message and send.
        self.chat_panel.input = agent_prompt;
        self.chat_panel.input_cursor = self.chat_panel.input.chars().count();
        self.send_chat_message();

        self.set_status(format!("Agent started: {task}"));
    }

    /// Stop the agent and show summary.
    pub fn stop_agent(&mut self, reason: &str) {
        if let Some(mut session) = self.agent_mode.take() {
            // Cancel any running subagents.
            session.subagent_manager.cancel_all();

            let elapsed = session.started_at.elapsed();
            let secs = elapsed.as_secs();
            let files = session.files_changed.len();
            let cmds = session.commands_run;
            let iters = session.iteration;
            let subagents = session.subagent_manager.total_count();

            // Compute diffs from snapshots for review.
            let mut diffs: Vec<(String, String, String)> = Vec::new();
            for (path, old_content) in &session.file_snapshots {
                let new_content = std::fs::read_to_string(path).unwrap_or_default();
                if *old_content != new_content {
                    diffs.push((path.clone(), old_content.clone(), new_content));
                }
            }
            // Include files changed that weren't snapshotted.
            for path in &session.files_changed {
                if !session.file_snapshots.contains_key(path) {
                    if let Ok(content) = std::fs::read_to_string(path) {
                        diffs.push((path.clone(), String::new(), content));
                    }
                }
            }
            self.agent_diff_files = diffs;
            self.agent_diff_idx = 0;

            let sub_info = if subagents > 0 {
                format!(", {subagents} subagent(s)")
            } else {
                String::new()
            };

            let diff_hint = if !self.agent_diff_files.is_empty() {
                let n = self.agent_diff_files.len();
                format!(" — :agent diff to review {n} file(s)")
            } else {
                String::new()
            };

            self.set_status(format!(
                "Agent stopped ({reason}): {iters} iterations, {files} file(s), {cmds} cmd(s){sub_info}, {secs}s{diff_hint}"
            ));
            // Stop any ongoing streaming.
            self.chat_panel.streaming = false;
            self.chat_panel.in_tool_loop = false;
            self.pending_tool_calls.clear();
        }
    }

    /// Pause a running agent.
    pub fn pause_agent(&mut self) {
        if let Some(ref mut session) = self.agent_mode {
            if session.paused {
                return;
            }
            session.paused = true;
            // Drop the receiver to stop consuming streaming events.
            self.chat_receiver = None;
            self.chat_panel.streaming = false;
            self.set_status("Agent paused (Ctrl+P or :agent resume to continue)");
        }
    }

    /// Resume a paused agent.
    pub fn resume_agent(&mut self) {
        let should_resume = self.agent_mode.as_ref().map(|s| s.paused).unwrap_or(false);
        if !should_resume {
            return;
        }
        let task = self.agent_mode.as_ref().unwrap().task.clone();
        self.agent_mode.as_mut().unwrap().paused = false;
        self.set_status(format!("Agent resumed: {task}"));
        // Re-send to continue the tool loop.
        self.continue_tool_loop();
    }

    /// Approve the agent's execution plan and start executing.
    pub fn approve_agent_plan(&mut self) {
        self.chat_panel.plan_pending_approval = false;

        if let Some(ref mut session) = self.agent_mode {
            if let Some(ref mut plan) = session.plan {
                plan.approved = true;
            }
        }

        // Build a message telling the AI to execute the plan.
        let plan_summary = self
            .agent_mode
            .as_ref()
            .and_then(|s| s.plan.as_ref())
            .map(|p| {
                p.steps
                    .iter()
                    .map(|s| format!("{}. {}", s.index, s.description))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();

        self.chat_panel
            .push_system_message("✓ Plan approved — executing...");

        let exec_prompt =
            format!("Your plan has been approved. Execute it now, step by step:\n{plan_summary}");
        self.chat_panel.input = exec_prompt;
        self.chat_panel.input_cursor = self.chat_panel.input.chars().count();
        self.send_chat_message();
    }

    /// Deny the agent's execution plan and stop the agent.
    pub fn deny_agent_plan(&mut self) {
        self.chat_panel.plan_pending_approval = false;
        self.chat_panel
            .push_system_message("✗ Plan rejected — agent stopped.");
        self.stop_agent("plan rejected");
    }

    // ── Code Folding ─────────────────────────────────────────────

    /// Refresh foldable ranges from tree-sitter.
    pub fn refresh_foldable_ranges(&mut self) {
        let ranges = self
            .tab()
            .highlighter
            .as_ref()
            .map(|h| h.foldable_ranges())
            .unwrap_or_default();
        self.tab_mut().foldable_ranges = ranges;
    }

    /// Toggle fold at the cursor line.
    pub fn toggle_fold(&mut self) {
        let line = self.tab().cursor.row;
        if self.tab().folded_ranges.contains_key(&line) {
            self.tab_mut().folded_ranges.remove(&line);
        } else if let Some(&end) = self.tab().foldable_ranges.get(&line) {
            self.tab_mut().folded_ranges.insert(line, end);
        }
    }

    /// Close fold at cursor line.
    pub fn close_fold(&mut self) {
        let line = self.tab().cursor.row;
        if let Some(&end) = self.tab().foldable_ranges.get(&line) {
            self.tab_mut().folded_ranges.insert(line, end);
        }
    }

    /// Open fold at cursor line.
    pub fn open_fold(&mut self) {
        let line = self.tab().cursor.row;
        self.tab_mut().folded_ranges.remove(&line);
    }

    /// Close all folds.
    pub fn close_all_folds(&mut self) {
        let ranges = self.tab().foldable_ranges.clone();
        self.tab_mut().folded_ranges = ranges;
    }

    /// Open all folds.
    pub fn open_all_folds(&mut self) {
        self.tab_mut().folded_ranges.clear();
    }

    /// Check if a line is inside a folded range (not the fold start itself).
    pub fn is_line_folded(&self, line: usize) -> bool {
        for (&start, &end) in &self.tab().folded_ranges {
            if line > start && line <= end {
                return true;
            }
        }
        false
    }

    // ── Project-wide search ───────────────────────────────────────

    /// Open the project-wide search panel.
    pub fn open_project_search(&mut self) {
        let root = self
            .tab()
            .buffer
            .file_path()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        self.project_search.open(root);
    }

    /// Execute the project search with current query.
    pub fn execute_project_search(&mut self) {
        self.project_search.execute();
        let count = self.project_search.total_matches;
        let files = self.project_search.file_count;
        if count == 0 {
            self.set_status("No matches found");
        } else {
            self.set_status(format!("{count} match(es) in {files} file(s)"));
        }
        self.project_search.focus = crate::project_search::SearchFocus::Results;
    }

    /// Jump to the selected search result.
    pub fn goto_search_result(&mut self) {
        let result = match self.project_search.selected_result() {
            Some(r) => r.clone(),
            None => return,
        };

        let root = self.project_search.root.clone();
        let full_path = root.join(&result.file_path);
        if let Err(e) = self.open_file(full_path) {
            self.set_status(e);
            return;
        }
        self.tab_mut().cursor.row = result.line_number.saturating_sub(1);
        self.tab_mut().cursor.col = result.column;
        self.tab_mut().scroll_row = self.tab().cursor.row.saturating_sub(10);
        self.project_search.close();
    }

    /// Replace all matches across the project.
    pub fn replace_all_project(&mut self) {
        let old = self.project_search.query.clone();
        let new = self.project_search.replace_text.clone();
        let root = self.project_search.root.clone();
        let case_sensitive = self.project_search.case_sensitive;

        if old.is_empty() || new.is_empty() {
            self.set_status("Search and replace text required");
            return;
        }

        let (files_changed, total) =
            crate::project_search::replace_in_files(&root, &old, &new, case_sensitive);
        self.set_status(format!(
            "Replaced {total} occurrence(s) in {files_changed} file(s)"
        ));

        // Reload the current buffer if it was modified on disk.
        if let Some(path) = self.tab().buffer.file_path().map(|p| p.to_path_buf()) {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let current = self.tab().buffer.text();
                if content != current {
                    // Replace buffer content with disk version.
                    let len = self.tab().buffer.len_chars();
                    self.tab_mut()
                        .buffer
                        .delete(0, len, aura_core::AuthorId::Human);
                    self.tab_mut()
                        .buffer
                        .insert(0, &content, aura_core::AuthorId::Human);
                    self.tab_mut().mark_highlights_dirty();
                }
            }
        }

        // Re-run search to update results.
        self.project_search.execute();
    }

    /// Detect inline conflict markers in the current buffer.
    pub fn detect_inline_conflicts(&mut self) {
        self.inline_conflicts.clear();
        let line_count = self.tab().buffer.line_count();
        let mut i = 0;
        while i < line_count {
            if let Some(line) = self.tab().buffer.line_text(i) {
                if line.starts_with("<<<<<<<") {
                    // Find ======= and >>>>>>>
                    let mut separator = None;
                    let mut end = None;
                    let mut j = i + 1;
                    while j < line_count {
                        if let Some(l) = self.tab().buffer.line_text(j) {
                            if l.starts_with("=======") && separator.is_none() {
                                separator = Some(j);
                            } else if l.starts_with(">>>>>>>") {
                                end = Some(j);
                                break;
                            }
                        }
                        j += 1;
                    }
                    if let (Some(sep), Some(marker_end)) = (separator, end) {
                        self.inline_conflicts.push(InlineConflict {
                            marker_start: i,
                            separator: sep,
                            marker_end,
                        });
                        i = marker_end + 1;
                        continue;
                    }
                }
            }
            i += 1;
        }
    }

    /// Resolve an inline conflict at the cursor position.
    pub fn resolve_inline_conflict(&mut self, resolution: crate::merge_view::Resolution) {
        use aura_core::AuthorId;

        // Find which conflict the cursor is in.
        let cursor_line = self.tab().cursor.row;
        let conflict_idx = self
            .inline_conflicts
            .iter()
            .position(|c| cursor_line >= c.marker_start && cursor_line <= c.marker_end);
        let conflict_idx = match conflict_idx {
            Some(idx) => idx,
            None => {
                self.set_status("Cursor is not on a conflict block");
                return;
            }
        };

        let conflict = &self.inline_conflicts[conflict_idx];
        let marker_start = conflict.marker_start;
        let separator = conflict.separator;
        let marker_end = conflict.marker_end;

        // Extract ours and theirs lines.
        let mut ours = Vec::new();
        for line_idx in (marker_start + 1)..separator {
            if let Some(text) = self.tab().buffer.line_text(line_idx) {
                ours.push(text.trim_end_matches('\n').to_string());
            }
        }
        let mut theirs = Vec::new();
        for line_idx in (separator + 1)..marker_end {
            if let Some(text) = self.tab().buffer.line_text(line_idx) {
                theirs.push(text.trim_end_matches('\n').to_string());
            }
        }

        // Build replacement text based on resolution.
        let replacement = match resolution {
            crate::merge_view::Resolution::AcceptCurrent => ours.join("\n"),
            crate::merge_view::Resolution::AcceptIncoming => theirs.join("\n"),
            crate::merge_view::Resolution::AcceptBothCurrentFirst => {
                let mut lines = ours;
                lines.extend(theirs);
                lines.join("\n")
            }
            crate::merge_view::Resolution::AcceptBothIncomingFirst => {
                let mut lines = theirs;
                lines.extend(ours);
                lines.join("\n")
            }
            _ => return,
        };

        // Calculate the char range to delete (from start of marker_start line to end of marker_end line).
        let rope = self.tab().buffer.rope();
        let start_char = rope.line_to_char(marker_start);
        let end_char = if marker_end + 1 < rope.len_lines() {
            rope.line_to_char(marker_end + 1)
        } else {
            rope.len_chars()
        };

        // Delete the conflict block and insert the replacement.
        let replacement_with_newline = if replacement.is_empty() {
            String::new()
        } else if marker_end + 1 < self.tab().buffer.line_count() {
            format!("{}\n", replacement)
        } else {
            replacement
        };

        self.tab_mut()
            .buffer
            .delete(start_char, end_char, AuthorId::Human);
        if !replacement_with_newline.is_empty() {
            self.tab_mut()
                .buffer
                .insert(start_char, &replacement_with_newline, AuthorId::Human);
        }

        // Move cursor to the start of where the conflict was.
        self.tab_mut().cursor.row = marker_start;
        self.tab_mut().cursor.col = 0;
        self.tab_mut().mark_highlights_dirty();

        // Re-detect remaining conflicts.
        self.detect_inline_conflicts();

        let remaining = self.inline_conflicts.len();
        let label = match resolution {
            crate::merge_view::Resolution::AcceptCurrent => "current",
            crate::merge_view::Resolution::AcceptIncoming => "incoming",
            crate::merge_view::Resolution::AcceptBothCurrentFirst => "both (current first)",
            crate::merge_view::Resolution::AcceptBothIncomingFirst => "both (incoming first)",
            _ => "resolved",
        };
        self.set_status(format!(
            "Accepted {label} — {remaining} conflict(s) remaining"
        ));
    }

    /// Toggle the AI Visor panel.
    pub fn toggle_ai_visor(&mut self) {
        if self.ai_visor.visible {
            self.ai_visor.visible = false;
            self.ai_visor_focused = false;
        } else {
            // Load data from project root.
            let project_root = self
                .tab()
                .buffer
                .file_path()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf())
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            self.ai_visor.sections = crate::ai_visor::load_visor_data(&project_root);
            self.ai_visor.visible = true;
            self.ai_visor_focused = true;
            // Close other right-side panels.
            self.chat_panel.visible = false;
            self.chat_panel_focused = false;
            self.conversation_history.visible = false;
            self.conversation_history_focused = false;
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
                    let is_peek = self.peek_definition_pending;
                    self.peek_definition_pending = false;

                    if let Some(loc) = locations.first() {
                        let target_row = loc.range.start.line as usize;
                        let target_col = loc.range.start.character as usize;

                        let current_uri = self
                            .tab()
                            .buffer
                            .file_path()
                            .map(|p| format!("file://{}", p.display()))
                            .unwrap_or_default();

                        if is_peek {
                            // Peek mode: show inline popup without navigating.
                            self.open_peek_definition(loc, &current_uri);
                        } else if loc.uri == current_uri {
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
                LspEvent::References(locations) => {
                    if locations.is_empty() {
                        self.set_status("No references found");
                    } else {
                        let count = locations.len();
                        self.set_status(format!("{count} reference(s) found"));
                        self.references_panel = Some(ReferencesPanel::new(locations));
                    }
                }
                LspEvent::RenameApplied(edits) => {
                    self.apply_rename_edits(edits);
                }
                LspEvent::InlayHints(hints) => {
                    self.tab_mut().inlay_hints = hints;
                }
                LspEvent::SemanticTokens(tokens) => {
                    self.tab_mut().semantic_tokens = tokens;
                }
                LspEvent::CodeLens(items) => {
                    self.tab_mut().code_lens = items;
                }
                LspEvent::SignatureHelp(result) => {
                    self.tab_mut().signature_help = result;
                }
                LspEvent::CallHierarchy(items) => {
                    if items.is_empty() {
                        self.set_status("No callers found");
                    } else {
                        let msg = items
                            .iter()
                            .map(|i| {
                                format!(
                                    "  {} ({}:{})",
                                    i.name,
                                    i.uri.rsplit('/').next().unwrap_or(&i.uri),
                                    i.line + 1
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        self.chat_panel.push_system_message(&format!(
                            "Incoming calls ({}):\n{}",
                            items.len(),
                            msg
                        ));
                        if !self.chat_panel.visible {
                            self.chat_panel.visible = true;
                        }
                        self.set_status(format!("{} caller(s) found", items.len()));
                    }
                }
                LspEvent::ServerError(e) => {
                    tracing::warn!("LSP server error: {}", e);
                    self.tab_mut().lsp_client = None;
                    // Attempt to restart the LSP server.
                    if let Some(path) = self.tab().buffer.file_path() {
                        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                            if let Some(config) = crate::lsp::detect_server(ext) {
                                let file_path = path.to_path_buf();
                                let workspace =
                                    file_path.parent().unwrap_or(&file_path).to_path_buf();
                                let content = self.tab().buffer.rope().to_string();
                                match crate::lsp::LspClient::start(
                                    &config, &workspace, &file_path, &content,
                                ) {
                                    Ok(client) => {
                                        self.tab_mut().lsp_client = Some(client);
                                        self.set_status("LSP server restarted");
                                        return;
                                    }
                                    Err(restart_err) => {
                                        tracing::warn!("LSP restart failed: {restart_err}");
                                    }
                                }
                            }
                        }
                    }
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

    /// Request peek definition at the current cursor position (shows inline popup).
    pub fn lsp_peek_definition(&mut self) {
        let row = self.tab().cursor.row as u32;
        let col = self.tab().cursor.col as u32;
        let has_lsp = self.tab().lsp_client.is_some();
        if has_lsp {
            self.peek_definition_pending = true;
            if let Some(lsp) = self.tab_mut().lsp_client.as_mut() {
                lsp.goto_definition(row, col);
            }
            self.set_status("Peeking definition...");
        } else {
            self.set_status("No LSP server");
        }
    }

    /// Open the peek definition popup for the given LSP location.
    fn open_peek_definition(&mut self, loc: &crate::lsp::LspLocation, current_uri: &str) {
        use std::path::PathBuf;

        let target_row = loc.range.start.line as usize;
        let target_col = loc.range.start.character as usize;
        let context_lines: usize = 20;

        // Resolve file path and read content.
        let (file_path, content) = if loc.uri == current_uri {
            // Same file — read from buffer.
            let path = self
                .tab()
                .buffer
                .file_path()
                .map(|p| p.to_path_buf())
                .unwrap_or_default();
            let text = self.tab().buffer.text();
            (path, text)
        } else {
            // Cross-file — read from disk.
            let path_str = loc.uri.strip_prefix("file://").unwrap_or(&loc.uri);
            let path = PathBuf::from(path_str);
            match std::fs::read_to_string(&path) {
                Ok(text) => (path, text),
                Err(_) => {
                    self.set_status("Could not read definition file");
                    return;
                }
            }
        };

        let all_lines: Vec<&str> = content.lines().collect();
        let total = all_lines.len();
        let start = target_row.saturating_sub(2); // 2 lines of context above
        let end = total.min(start + context_lines);
        let lines: Vec<String> = all_lines[start..end]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let slice_text = lines.join("\n");

        // Syntax-highlight the slice.
        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let highlighted = if let Some(lang) = crate::highlight::Language::from_extension(ext) {
            if let Some(mut hl) = crate::highlight::SyntaxHighlighter::new(lang) {
                hl.highlight(&slice_text, Some(&self.theme))
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let display_path = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        self.set_status(format!("Peek: {}:{}", display_path, target_row + 1));

        self.peek_definition = Some(PeekDefinition {
            file_path,
            target_line: target_row,
            target_col,
            lines,
            first_line: start,
            scroll_offset: 0,
            highlighted,
        });
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
    /// in `poll_lsp_events` via `LspEvent::CodeActions`.
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

    /// Request all references to the symbol at the cursor.
    pub fn lsp_references(&mut self) {
        let row = self.tab().cursor.row as u32;
        let col = self.tab().cursor.col as u32;
        if let Some(client) = &mut self.tab_mut().lsp_client {
            client.references(row, col);
            self.set_status("Finding references...");
        } else {
            self.set_status("No LSP server");
        }
    }

    /// Start rename mode: show input prompt with current word.
    pub fn lsp_rename_start(&mut self) {
        if self.tab().lsp_client.is_none() {
            self.set_status("No LSP server");
            return;
        }
        // Get the word under cursor as default input.
        let word = self
            .tab()
            .buffer
            .word_at_cursor(self.tab().cursor.row, self.tab().cursor.col);
        self.rename_input = word;
        self.rename_active = true;
        self.set_status("Rename: type new name and press Enter");
    }

    /// Execute a rename request with the given new name.
    pub fn lsp_rename_execute(&mut self) {
        let new_name = self.rename_input.clone();
        self.rename_active = false;
        if new_name.is_empty() {
            self.set_status("Rename cancelled (empty name)");
            return;
        }
        let row = self.tab().cursor.row as u32;
        let col = self.tab().cursor.col as u32;
        if let Some(client) = &mut self.tab_mut().lsp_client {
            client.rename(row, col, &new_name);
            self.set_status(format!("Renaming to '{new_name}'..."));
        }
    }

    /// Apply rename edits from the LSP server.
    fn apply_rename_edits(
        &mut self,
        edits: std::collections::HashMap<String, Vec<crate::lsp::TextEdit>>,
    ) {
        let mut total_edits = 0usize;
        let mut files_changed = 0usize;

        for (uri, file_edits) in &edits {
            // Convert URI to file path.
            let path_str = uri.strip_prefix("file://").unwrap_or(uri);
            let path = std::path::PathBuf::from(path_str);

            // Check if this file is the current tab.
            let is_current = self
                .tab()
                .buffer
                .file_path()
                .is_some_and(|p| p.canonicalize().ok() == path.canonicalize().ok());

            if is_current {
                // Apply edits to the current buffer (in reverse order to preserve positions).
                let mut sorted_edits = file_edits.clone();
                sorted_edits.sort_by(|a, b| {
                    b.range
                        .start
                        .line
                        .cmp(&a.range.start.line)
                        .then(b.range.start.character.cmp(&a.range.start.character))
                });

                for edit in &sorted_edits {
                    let start_line = edit.range.start.line as usize;
                    let start_col = edit.range.start.character as usize;
                    let end_line = edit.range.end.line as usize;
                    let end_col = edit.range.end.character as usize;

                    // Calculate byte offsets.
                    let rope = self.tab().buffer.rope();
                    if start_line < rope.len_lines() && end_line < rope.len_lines() {
                        let start_byte = rope.line_to_byte(start_line) + start_col;
                        let end_byte = rope.line_to_byte(end_line) + end_col;
                        if end_byte <= rope.len_bytes() && start_byte <= end_byte {
                            self.tab_mut().buffer.replace_range(
                                start_byte,
                                end_byte,
                                &edit.new_text,
                            );
                            total_edits += 1;
                        }
                    }
                }
                self.tab_mut().mark_highlights_dirty();
                files_changed += 1;
            } else {
                // Check if file is open in another tab — apply in-memory if so.
                let open_tab_idx = self.tabs.tabs().iter().position(|t| {
                    t.buffer.file_path().and_then(|p| p.canonicalize().ok())
                        == path.canonicalize().ok()
                });

                if let Some(tab_idx) = open_tab_idx {
                    // Apply edits to the open tab's buffer (in reverse order).
                    let mut sorted_edits = file_edits.clone();
                    sorted_edits.sort_by(|a, b| {
                        b.range
                            .start
                            .line
                            .cmp(&a.range.start.line)
                            .then(b.range.start.character.cmp(&a.range.start.character))
                    });
                    for edit in &sorted_edits {
                        let start_line = edit.range.start.line as usize;
                        let start_col = edit.range.start.character as usize;
                        let end_line = edit.range.end.line as usize;
                        let end_col = edit.range.end.character as usize;
                        let rope = self.tabs.tabs()[tab_idx].buffer.rope();
                        if start_line < rope.len_lines() && end_line < rope.len_lines() {
                            let start_byte = rope.line_to_byte(start_line) + start_col;
                            let end_byte = rope.line_to_byte(end_line) + end_col;
                            if end_byte <= rope.len_bytes() && start_byte <= end_byte {
                                self.tabs.tabs_mut()[tab_idx].buffer.replace_range(
                                    start_byte,
                                    end_byte,
                                    &edit.new_text,
                                );
                                total_edits += 1;
                            }
                        }
                    }
                    self.tabs.tabs_mut()[tab_idx].mark_highlights_dirty();
                    files_changed += 1;
                } else {
                    // File not open — read-modify-write on disk.
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let mut lines: Vec<String> = content.lines().map(String::from).collect();
                        let mut sorted_edits = file_edits.clone();
                        sorted_edits.sort_by(|a, b| {
                            b.range
                                .start
                                .line
                                .cmp(&a.range.start.line)
                                .then(b.range.start.character.cmp(&a.range.start.character))
                        });

                        for edit in &sorted_edits {
                            let sl = edit.range.start.line as usize;
                            let sc = edit.range.start.character as usize;
                            let el = edit.range.end.line as usize;
                            let ec = edit.range.end.character as usize;

                            if sl < lines.len() && el < lines.len() {
                                if sl == el {
                                    let line = &lines[sl];
                                    let before = &line[..sc.min(line.len())];
                                    let after = &line[ec.min(line.len())..];
                                    lines[sl] = format!("{before}{}{after}", edit.new_text);
                                }
                                total_edits += 1;
                            }
                        }

                        let new_content = lines.join("\n");
                        if let Err(e) = std::fs::write(&path, &new_content) {
                            tracing::warn!("Failed to write {}: {e}", path.display());
                        }
                        files_changed += 1;
                    }
                }
            }
        }

        self.set_status(format!(
            "Renamed: {total_edits} edit(s) in {files_changed} file(s)"
        ));
    }

    /// Navigate to a reference location (open file, jump to line).
    pub fn goto_reference(&mut self) {
        let location = match &self.references_panel {
            Some(panel) => panel.selected_location().cloned(),
            None => return,
        };
        let location = match location {
            Some(l) => l,
            None => return,
        };

        self.references_panel = None;

        let path_str = location
            .uri
            .strip_prefix("file://")
            .unwrap_or(&location.uri);
        let line = location.range.start.line as usize;
        let col = location.range.start.character as usize;

        // Check if it's the current file.
        let is_current = self.tab().buffer.file_path().is_some_and(|p| {
            let abs = std::path::Path::new(path_str);
            p.canonicalize().ok() == abs.canonicalize().ok()
        });

        if is_current {
            self.tab_mut().cursor.row = line;
            self.tab_mut().cursor.col = col;
            self.tab_mut().scroll_row = self.tab().cursor.row.saturating_sub(10);
        } else {
            // Open file in new tab.
            let path = std::path::PathBuf::from(path_str);
            if let Err(e) = self.open_file(path) {
                self.set_status(e);
                return;
            }
            self.tab_mut().cursor.row = line;
            self.tab_mut().cursor.col = col;
            self.tab_mut().scroll_row = self.tab().cursor.row.saturating_sub(10);
        }
    }

    /// Check if an LSP client is active.
    pub fn has_lsp(&self) -> bool {
        self.tab().lsp_client.is_some()
    }

    /// Check if an AI commit message is currently being generated.
    pub fn is_generating_commit_msg(&self) -> bool {
        self.commit_msg_receiver.is_some()
    }

    /// Get diagnostics for a specific line.
    pub fn line_diagnostics(&self, line: usize) -> Option<&Diagnostic> {
        self.tab().diagnostics.iter().find(|d| {
            let start = d.range.start.line as usize;
            let end = d.range.end.line as usize;
            line >= start && line <= end
        })
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
        self.matching_bracket = tab.buffer.find_matching_bracket(char_idx).map(|match_idx| {
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

    /// Jump to the nearest search match (used during incremental search).
    pub fn jump_to_nearest_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        let (start, _) = self.search_matches[self.search_current];
        let new_cursor = self.tab().buffer.char_idx_to_cursor(start);
        self.tab_mut().cursor = new_cursor;
        self.clamp_cursor();
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

    /// Set a transient status message shown in the command bar.
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }

    /// Poll and handle ACP server requests.
    fn poll_acp_requests(&mut self) {
        let requests = match &self.acp_server {
            Some(server) => server.poll_requests(),
            None => return,
        };

        for req in requests {
            let response = self.handle_acp_action(&req.action);
            let _ = req.response_tx.send(response);
        }
    }

    /// Handle a single ACP action.
    fn handle_acp_action(
        &mut self,
        action: &crate::acp_server::AcpAction,
    ) -> crate::acp_server::AcpAppResponse {
        use crate::acp_server::{AcpAction, AcpAppResponse};

        match action {
            AcpAction::GetEditorInfo => AcpAppResponse {
                success: true,
                data: serde_json::json!({
                    "name": "AURA Editor",
                    "version": env!("CARGO_PKG_VERSION"),
                    "protocol": "acp",
                    "language": self.tab().language.map(|l| format!("{:?}", l)),
                    "file": self.tab().buffer.file_path().map(|p| p.display().to_string()),
                }),
            },
            AcpAction::ReadDocument => {
                let content = self.tab().buffer.text();
                let path = self
                    .tab()
                    .buffer
                    .file_path()
                    .map(|p| p.display().to_string());
                AcpAppResponse {
                    success: true,
                    data: serde_json::json!({
                        "content": content,
                        "path": path,
                        "lines": self.tab().buffer.line_count(),
                    }),
                }
            }
            AcpAction::ReadFile { path } => match std::fs::read_to_string(path) {
                Ok(content) => AcpAppResponse {
                    success: true,
                    data: serde_json::json!({ "content": content, "path": path }),
                },
                Err(e) => AcpAppResponse {
                    success: false,
                    data: serde_json::json!({ "error": format!("{e}") }),
                },
            },
            AcpAction::ApplyEdit {
                start_line,
                start_col,
                end_line,
                end_col,
                text,
            } => {
                let rope = self.tab().buffer.rope();
                let start_char = rope.line_to_char(*start_line) + start_col;
                let end_char = rope.line_to_char(*end_line) + end_col;
                if end_char > start_char {
                    self.tab_mut().buffer.delete(
                        start_char,
                        end_char,
                        aura_core::AuthorId::ai("acp-agent"),
                    );
                }
                if !text.is_empty() {
                    self.tab_mut().buffer.insert(
                        start_char,
                        text,
                        aura_core::AuthorId::ai("acp-agent"),
                    );
                }
                self.tab_mut().mark_highlights_dirty();
                AcpAppResponse {
                    success: true,
                    data: serde_json::json!({ "applied": true }),
                }
            }
            AcpAction::GetCursorContext => {
                let row = self.tab().cursor.row;
                let col = self.tab().cursor.col;
                let line_text = self.tab().buffer.line_text(row).unwrap_or_default();
                AcpAppResponse {
                    success: true,
                    data: serde_json::json!({
                        "line": row + 1,
                        "column": col,
                        "lineText": line_text.trim_end(),
                    }),
                }
            }
            AcpAction::GetDiagnostics => {
                let diags: Vec<serde_json::Value> = self
                    .tab()
                    .diagnostics
                    .iter()
                    .map(|d| {
                        serde_json::json!({
                            "line": d.range.start.line + 1,
                            "column": d.range.start.character,
                            "severity": d.severity,
                            "message": d.message,
                            "source": d.source,
                        })
                    })
                    .collect();
                AcpAppResponse {
                    success: true,
                    data: serde_json::json!({ "diagnostics": diags }),
                }
            }
            AcpAction::GetSelection => {
                let selection = self.visual_selection_range().map(|(start, end)| {
                    let text = self.tab().buffer.rope().slice(start..end).to_string();
                    let start_cur = self.tab().buffer.char_idx_to_cursor(start);
                    let end_cur = self.tab().buffer.char_idx_to_cursor(end);
                    serde_json::json!({
                        "text": text,
                        "startLine": start_cur.row + 1,
                        "startColumn": start_cur.col,
                        "endLine": end_cur.row + 1,
                        "endColumn": end_cur.col,
                    })
                });
                AcpAppResponse {
                    success: true,
                    data: selection.unwrap_or(serde_json::json!({ "text": null })),
                }
            }
            AcpAction::ListOpenFiles => {
                let files: Vec<String> = self
                    .tabs
                    .tabs()
                    .iter()
                    .filter_map(|t| t.buffer.file_path().map(|p| p.display().to_string()))
                    .collect();
                AcpAppResponse {
                    success: true,
                    data: serde_json::json!({ "files": files }),
                }
            }
            AcpAction::OpenFile { path } => {
                let p = std::path::PathBuf::from(path);
                match self.open_file(p) {
                    Ok(()) => AcpAppResponse {
                        success: true,
                        data: serde_json::json!({ "opened": path }),
                    },
                    Err(e) => AcpAppResponse {
                        success: false,
                        data: serde_json::json!({ "error": e }),
                    },
                }
            }
            AcpAction::RunCommand { command } => {
                self.terminal_mut().visible = true;
                self.terminal_mut().send_bytes(command.as_bytes());
                self.terminal_mut().send_enter();
                AcpAppResponse {
                    success: true,
                    data: serde_json::json!({ "command": command, "status": "sent_to_terminal" }),
                }
            }
            AcpAction::GetProjectStructure => {
                let files: Vec<String> = self
                    .file_tree
                    .entries
                    .iter()
                    .filter(|e| !e.is_dir)
                    .take(500)
                    .map(|e| e.path.display().to_string())
                    .collect();
                AcpAppResponse {
                    success: true,
                    data: serde_json::json!({ "files": files, "count": files.len() }),
                }
            }
        }
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
                                .create_conversation(
                                    &file_path,
                                    *start_line,
                                    end_l,
                                    commit.as_deref(),
                                    branch.as_deref(),
                                )
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
                        store
                            .create_conversation(
                                &file_path,
                                start,
                                end,
                                commit.as_deref(),
                                branch.as_deref(),
                            )
                            .ok()
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
            McpAction::ReportActivity {
                agent_id,
                activity_type,
                description,
            } => {
                if let Some(agent) = self.agent_registry.agents.get_mut(agent_id) {
                    agent.last_activity = Some(format!("{activity_type}: {description}"));
                    agent.current_task = Some(description.clone());
                    agent.activity_count += 1;
                }
                tracing::debug!("Agent {agent_id} activity: {activity_type}: {description}");
                McpAppResponse {
                    success: true,
                    data: serde_json::json!({ "recorded": true }),
                }
            }
            McpAction::GetEditorState => {
                let tab = self.tab();
                let file_path = tab
                    .buffer
                    .file_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                let open_files: Vec<String> = self
                    .tabs
                    .tabs()
                    .iter()
                    .map(|t| t.file_name().to_string())
                    .collect();
                let diagnostics_count = tab.diagnostics.len();
                McpAppResponse {
                    success: true,
                    data: serde_json::json!({
                        "mode": self.mode.label(),
                        "current_file": file_path,
                        "cursor_row": tab.cursor.row,
                        "cursor_col": tab.cursor.col,
                        "open_files": open_files,
                        "tab_count": self.tabs.count(),
                        "diagnostics_count": diagnostics_count,
                        "modified": tab.buffer.is_modified(),
                        "line_count": tab.buffer.line_count(),
                    }),
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

    /// Get the ACP server port (if running).
    pub fn acp_port(&self) -> Option<u16> {
        self.acp_server.as_ref().map(|s| s.port)
    }

    /// Get count of connected external MCP servers.
    pub fn mcp_client_count(&self) -> usize {
        self.mcp_clients.len()
    }

    /// Poll the background update checker for results.
    fn poll_update_check(&mut self) {
        if let Some(ref rx) = self.update_receiver {
            if let Ok(status) = rx.try_recv() {
                match &status {
                    UpdateStatus::Available { version, .. } => {
                        self.update_notification_visible = true;
                        self.set_status(format!(
                            "Update available: v{} \u{2192} v{version}",
                            crate::update::CURRENT_VERSION
                        ));
                    }
                    UpdateStatus::UpToDate => {
                        self.set_status(format!(
                            "AURA v{} is up to date",
                            crate::update::CURRENT_VERSION
                        ));
                    }
                    UpdateStatus::Error(e) => {
                        self.set_status(format!("Update check failed: {e}"));
                    }
                }
                self.update_status = Some(status);
                self.update_receiver = None;
            }
        }
    }

    /// Trigger a forced update check (bypasses cache). Used by `:update` command.
    pub fn force_update_check(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        crate::update::spawn_forced_update_check(tx);
        self.update_receiver = Some(rx);
        self.update_status = None; // Clear old status while checking.
        self.set_status("Checking for updates...");
    }

    /// Dismiss the update notification toast.
    pub fn dismiss_update_notification(&mut self) {
        self.update_notification_visible = false;
    }

    /// Show the update confirmation modal.
    pub fn show_update_modal(&mut self) {
        self.update_notification_visible = false;
        self.update_modal_visible = true;
    }

    /// Run the platform-appropriate update command in the embedded terminal.
    pub fn run_update(&mut self) {
        self.update_modal_visible = false;
        if let Some(UpdateStatus::Available { ref version, .. }) = self.update_status {
            let cmd = "curl --proto '=https' --tlsv1.2 -LsSf https://github.com/odtorres/aura/releases/latest/download/aura-installer.sh | sh".to_string();
            self.set_status(format!("Updating to v{}...", version));
            // Run the update command in the embedded terminal.
            self.terminal_mut().visible = true;
            self.terminal_focused = true;
            self.terminal_mut()
                .send_bytes(format!("{}\n", cmd).as_bytes());
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

    /// Get current edit predictions.
    pub fn edit_predictions(&self) -> &[crate::speculative::NextEditPrediction] {
        match &self.speculative {
            Some(engine) => &engine.edit_predictions,
            None => &[],
        }
    }

    /// Jump cursor to the next (or previous) edit prediction.
    pub fn jump_to_prediction(&mut self, forward: bool) {
        if let Some(engine) = &mut self.speculative {
            if engine.edit_predictions.is_empty() {
                return;
            }
            if forward {
                engine.next_prediction();
            } else {
                engine.prev_prediction();
            }
            if let Some(pred) = engine.current_prediction() {
                let line = pred.line;
                let reason = pred.reason.label().to_string();
                self.tab_mut().cursor.row = line;
                self.tab_mut().cursor.col = 0;
                self.set_status(format!("Prediction: {reason} (line {})", line + 1));
            }
        }
    }

    /// Update next-edit predictions based on recent edit history and diagnostics.
    fn update_edit_predictions(&mut self) {
        let has_engine = self.speculative.is_some();
        if !has_engine {
            return;
        }

        // Extract recent edit line numbers from buffer history.
        let buffer = &self.tabs.active().buffer;
        let history = buffer.history();
        let recent_edit_lines: Vec<usize> = history
            .iter()
            .rev()
            .take(20)
            .filter_map(|edit| {
                let pos = match &edit.kind {
                    aura_core::buffer::EditKind::Insert { pos, .. } => *pos,
                    aura_core::buffer::EditKind::Delete { start, .. } => *start,
                };
                // Convert char position to line number safely.
                if pos <= buffer.len_chars() {
                    Some(buffer.char_idx_to_cursor(pos).row)
                } else {
                    None
                }
            })
            .collect();

        let cursor_line = self.tabs.active().cursor.row;
        let diagnostics: Vec<(usize, String)> = self
            .tabs
            .active()
            .diagnostics
            .iter()
            .map(|d| (d.range.start.line as usize, d.message.clone()))
            .collect();
        let line_count = buffer.line_count();

        if let Some(engine) = &mut self.speculative {
            engine.update_predictions(&recent_edit_lines, cursor_line, &diagnostics, line_count);
        }
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

        // Get the full staged diff for better AI context.
        let diff_stat = self
            .git_repo
            .as_ref()
            .and_then(|repo| repo.staged_diff_summary().ok())
            .unwrap_or_default();

        if diff_stat.trim().is_empty() {
            self.set_status("No staged changes to describe");
            return;
        }

        let diff_patch = self
            .git_repo
            .as_ref()
            .and_then(|repo| repo.staged_diff_patch(4000).ok())
            .unwrap_or_default();

        let system = "You are generating a git commit message. \
                      Write a concise, conventional commit message \
                      (type: description, e.g. 'feat: add login page'). \
                      If there are multiple changes, add bullet points in the body. \
                      Output ONLY the commit message, no explanations or markdown fences."
            .to_string();
        let messages = vec![Message::text(
            "user",
            &format!(
                "Generate a commit message for these staged changes:\n\n\
                 Summary:\n{diff_stat}\n\nDiff:\n{diff_patch}"
            ),
        )];

        let commit_model = self.config.ai.model_for("commit").to_string();
        let rx = client.stream_completion_with_model(&system, messages, &commit_model);

        // Clear the commit message and start streaming into it.
        self.source_control.commit_message.clear();
        self.source_control.editing_commit_message = false;
        self.source_control.focused_section = crate::source_control::GitPanelSection::CommitMessage;
        self.commit_msg_receiver = Some(rx);
        self.set_status(format!("Generating commit message ({commit_model})..."));
    }

    /// Poll for streaming AI commit message tokens.
    fn poll_commit_msg(&mut self) {
        let rx = match &self.commit_msg_receiver {
            Some(r) => r,
            None => return,
        };

        let mut done = false;
        loop {
            match rx.try_recv() {
                Ok(AiEvent::Token(t)) => {
                    self.source_control.commit_message.push_str(&t);
                }
                Ok(AiEvent::Done(full)) => {
                    self.source_control.commit_message = full.trim().to_string();
                    done = true;
                    break;
                }
                Ok(AiEvent::Error(e)) => {
                    self.set_status(format!("AI error: {e}"));
                    done = true;
                    break;
                }
                Ok(_) => {} // Ignore tool use events.
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    done = true;
                    break;
                }
            }
        }

        if done {
            self.commit_msg_receiver = None;
            if self.source_control.commit_message.is_empty() {
                self.set_status("Failed to generate commit message");
            } else {
                self.set_status("Commit message ready — review and press c to commit");
            }
        }
    }

    /// Request an AI suggestion for the terminal prompt.
    pub fn request_terminal_suggestion(&mut self) {
        if self.terminal_suggestion_pending {
            return;
        }
        let client = match &self.ai_client {
            Some(c) => c,
            None => return,
        };

        // Only suggest when at a prompt (not while a command is running).
        if self.terminal().command_running() {
            return;
        }

        // Build context from recent commands.
        let cmds = self.terminal().commands();
        let recent: Vec<String> = cmds
            .iter()
            .rev()
            .take(5)
            .map(|c| {
                let exit = c
                    .exit_code
                    .map(|e| format!(" (exit {e})"))
                    .unwrap_or_default();
                format!("$ {}{}", c.command, exit)
            })
            .collect();
        let history = recent.into_iter().rev().collect::<Vec<_>>().join("\n");

        // Detect project type from cwd.
        let project_hint = self
            .tab()
            .buffer
            .file_path()
            .and_then(|p| p.parent())
            .map(|dir| {
                if dir.join("Cargo.toml").exists() {
                    "Rust (Cargo)"
                } else if dir.join("package.json").exists() {
                    "Node.js"
                } else if dir.join("go.mod").exists() {
                    "Go"
                } else if dir.join("mix.exs").exists() {
                    "Elixir"
                } else if dir.join("pubspec.yaml").exists() {
                    "Dart/Flutter"
                } else if dir.join("build.zig").exists() {
                    "Zig"
                } else if dir.join("build.sbt").exists() {
                    "Scala"
                } else if dir.join("stack.yaml").exists() || dir.join("cabal.project").exists() {
                    "Haskell"
                } else if dir.join("composer.json").exists() {
                    "PHP"
                } else if dir.join("pyproject.toml").exists() || dir.join("setup.py").exists() {
                    "Python"
                } else {
                    "Unknown"
                }
            })
            .unwrap_or("Unknown");

        let system = "You are a terminal command assistant. Suggest the single most likely \
                      next command the user would run. Output ONLY the command, nothing else. \
                      No explanation, no markdown, no quotes."
            .to_string();
        let prompt = format!(
            "Project type: {project_hint}\n\
             Recent commands:\n{history}\n\n\
             Suggest the next command:"
        );
        let messages = vec![aura_ai::Message::text("user", &prompt)];
        let rx = client.stream_completion(&system, messages);
        self.terminal_suggestion_rx = Some(rx);
        self.terminal_suggestion_pending = true;
    }

    /// Poll for terminal suggestion AI response.
    fn poll_terminal_suggestion(&mut self) {
        let rx = match &self.terminal_suggestion_rx {
            Some(r) => r,
            None => return,
        };

        let mut done = false;
        loop {
            match rx.try_recv() {
                Ok(AiEvent::Token(t)) => {
                    let suggestion = self.terminal_suggestion.get_or_insert_with(String::new);
                    suggestion.push_str(&t);
                }
                Ok(AiEvent::Done(full)) => {
                    let trimmed = full.trim().to_string();
                    self.terminal_suggestion = if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed)
                    };
                    done = true;
                    break;
                }
                Ok(AiEvent::Error(_)) | Err(mpsc::TryRecvError::Disconnected) => {
                    done = true;
                    break;
                }
                Ok(_) => {}
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }

        if done {
            self.terminal_suggestion_rx = None;
            self.terminal_suggestion_pending = false;
        }
    }

    /// Summarize a long conversation using AI.
    fn summarize_conversation(&mut self, conversation_id: &str) {
        let client = match &self.ai_client {
            Some(c) => c,
            None => return,
        };
        let store = match &self.conversation_store {
            Some(s) => s,
            None => return,
        };
        let messages = match store.messages_for_conversation(conversation_id) {
            Ok(m) => m,
            Err(_) => return,
        };

        let threshold = self.config.conversations.keep_recent_messages;
        if messages.len() <= threshold {
            return;
        }

        // Build transcript from messages (truncated to ~4000 chars for the AI).
        let mut transcript = String::new();
        for msg in &messages {
            let role_label = match msg.role {
                aura_core::conversation::MessageRole::HumanIntent => "Human",
                aura_core::conversation::MessageRole::AiResponse => "AI",
                aura_core::conversation::MessageRole::System => "System",
            };
            transcript.push_str(&format!("{role_label}: {}\n\n", msg.content));
            if transcript.len() > 4000 {
                transcript.push_str("... (truncated)");
                break;
            }
        }

        let system = "Summarize this conversation between a developer and AI assistant. \
                      Focus on: what was discussed, what decisions were made, what code was \
                      changed. Be concise (2-4 sentences). Output ONLY the summary."
            .to_string();
        let msgs = vec![Message::text("user", &transcript)];
        let summarize_model = self.config.ai.model_for("summarize").to_string();
        let rx = client.stream_completion_with_model(&system, msgs, &summarize_model);
        self.summarize_receiver = Some((conversation_id.to_string(), rx));
    }

    /// Poll for background conversation summarization results.
    fn poll_summarize(&mut self) {
        let (conv_id, rx) = match &self.summarize_receiver {
            Some((id, rx)) => (id.clone(), rx),
            None => return,
        };

        let mut summary = String::new();
        let mut done = false;
        loop {
            match rx.try_recv() {
                Ok(AiEvent::Token(t)) => summary.push_str(&t),
                Ok(AiEvent::Done(full)) => {
                    summary = full.trim().to_string();
                    done = true;
                    break;
                }
                Ok(AiEvent::Error(_)) => {
                    done = true;
                    break;
                }
                Ok(_) => {}
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    done = true;
                    break;
                }
            }
        }

        if done {
            self.summarize_receiver = None;
            if !summary.is_empty() {
                if let Some(store) = &self.conversation_store {
                    if let Err(e) = store.update_summary(&conv_id, &summary) {
                        tracing::warn!("Failed to update conversation summary: {e}");
                    }
                    // Insert the summary as a system message for future context.
                    let _ = store.add_message(
                        &conv_id,
                        aura_core::conversation::MessageRole::System,
                        &format!("[Summary] {summary}"),
                        None,
                    );
                    // Thin old messages, keeping recent ones + the new summary.
                    let keep = self.config.conversations.keep_recent_messages;
                    let _ = store.delete_messages_except_recent(&conv_id, keep + 1);
                    tracing::info!("Summarized conversation {conv_id}");
                }
            }

            // Check for more conversations needing summarization.
            self.maybe_summarize_next();
        }
    }

    /// Find the next conversation needing summarization and start it.
    fn maybe_summarize_next(&mut self) {
        if self.summarize_receiver.is_some() || self.ai_client.is_none() {
            return;
        }
        if let Some(store) = &self.conversation_store {
            let threshold = self.config.conversations.keep_recent_messages;
            if let Ok(ids) = store.conversations_needing_summary(threshold) {
                if let Some(id) = ids.first() {
                    let id = id.clone();
                    self.summarize_conversation(&id);
                }
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

    /// Open the git graph modal.
    pub fn open_git_graph(&mut self) {
        if let Some(repo) = &self.git_repo {
            match repo.graph_log(100) {
                Ok(commits) => {
                    self.git_graph.open(commits);
                    // Mark commits that have linked AI conversations.
                    if let Some(store) = &self.conversation_store {
                        for (i, commit) in self.git_graph.commits.iter().enumerate() {
                            if store.has_conversations_for_commit(&commit.hash) {
                                self.git_graph.commits_with_conversations.insert(i);
                            }
                        }
                    }
                    // Load files for first commit.
                    self.load_graph_commit_files();
                }
                Err(e) => self.set_status(format!("Git graph failed: {e}")),
            }
        } else {
            self.set_status("Not a git repository");
        }
    }

    /// Open the AI conversation linked to the selected git graph commit.
    pub fn open_graph_commit_conversation(&mut self) {
        let hash = match self.git_graph.selected_hash() {
            Some(h) => h.to_string(),
            None => return,
        };
        let store = match &self.conversation_store {
            Some(s) => s,
            None => {
                self.set_status("No conversation store");
                return;
            }
        };
        match store.conversations_for_commit(&hash) {
            Ok(convs) if !convs.is_empty() => {
                let conv_id = convs[0].id.clone();
                // Close the graph, open history panel, and expand the conversation.
                self.git_graph.close();
                self.conversation_history.visible = true;
                self.conversation_history_focused = true;
                self.refresh_conversation_history();
                // Find and select the conversation in the history panel.
                if let Some(idx) = self
                    .conversation_history
                    .conversations
                    .iter()
                    .position(|c| c.id == conv_id)
                {
                    self.conversation_history.selected = idx;
                    if let Some(store) = &self.conversation_store {
                        self.conversation_history.toggle_expand(store);
                    }
                }
                self.set_status(format!(
                    "Conversation for commit {}",
                    &hash[..hash.len().min(7)]
                ));
            }
            Ok(_) => {
                self.set_status(format!(
                    "No conversations for commit {}",
                    &hash[..hash.len().min(7)]
                ));
            }
            Err(e) => {
                self.set_status(format!("Failed to query conversations: {e}"));
            }
        }
    }

    /// Load changed files for the selected commit in the git graph.
    pub fn load_graph_commit_files(&mut self) {
        let hash = match self.git_graph.selected_hash() {
            Some(h) => h.to_string(),
            None => return,
        };
        if let Some(repo) = &self.git_repo {
            self.git_graph.detail_files = repo.commit_files(&hash).unwrap_or_default();
        }
    }

    /// Open the branch picker modal.
    pub fn open_branch_picker(&mut self) {
        let branches = self.git_list_branches();
        if branches.is_empty() {
            self.set_status("No branches found");
            return;
        }
        self.branch_picker.open(branches);
    }

    /// Execute the branch picker selection.
    pub fn execute_branch_pick(&mut self) {
        let branch = match self.branch_picker.selected_branch() {
            Some(b) => b.to_string(),
            None => return,
        };
        self.branch_picker.close();
        self.git_checkout(&branch);
        self.refresh_source_control();
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

    // ── Debugger ──────────────────────────────────────────────────

    /// Toggle a breakpoint on the current cursor line.
    /// Toggle a breakpoint on the current line (no condition).
    pub fn toggle_breakpoint(&mut self) {
        let line = self.tab().cursor.row;
        let tab = self.tab_mut();
        use std::collections::btree_map::Entry;
        match tab.breakpoints.entry(line) {
            Entry::Occupied(e) => {
                e.remove();
            }
            Entry::Vacant(e) => {
                e.insert(None);
            }
        }
        // If a debug session is active, resend breakpoints for this file.
        self.sync_breakpoints_to_adapter();
    }

    /// Set a conditional breakpoint on the current line.
    pub fn set_conditional_breakpoint(&mut self, condition: &str) {
        let line = self.tab().cursor.row;
        self.tab_mut()
            .breakpoints
            .insert(line, Some(condition.to_string()));
        self.sync_breakpoints_to_adapter();
        self.set_status(format!(
            "Conditional breakpoint at line {}: {condition}",
            line + 1
        ));
    }

    /// Start a debug session for the current file.
    pub fn start_debug_session(&mut self) {
        if self.dap_client.is_some() {
            self.set_status("Debug session already active");
            return;
        }

        let ext = self
            .tab()
            .buffer
            .file_path()
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .map(String::from);

        let ext = match ext {
            Some(e) => e,
            None => {
                self.set_status("Cannot detect file type for debugging");
                return;
            }
        };

        // Check user-configured debuggers first, then auto-detect.
        let adapter_config = self
            .config
            .debuggers
            .values()
            .find(|d| d.extensions.iter().any(|e| e == &ext))
            .map(|d| crate::dap::DapAdapterConfig {
                command: d.command.clone(),
                args: d.args.clone(),
            })
            .or_else(|| crate::dap::detect_debug_adapter(&ext));

        let adapter_config = match adapter_config {
            Some(c) => c,
            None => {
                self.set_status(format!("No debug adapter found for .{ext} files"));
                return;
            }
        };

        let workspace_root = self
            .tab()
            .buffer
            .file_path()
            .and_then(|p| p.parent())
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();

        match crate::dap::DapClient::start(&adapter_config, &workspace_root) {
            Ok(client) => {
                self.dap_client = Some(client);
                self.debug_panel.open();
                self.debug_panel.state.status = crate::debug_panel::SessionStatus::Running;
                self.set_status(format!(
                    "Debug session started ({})",
                    adapter_config.command
                ));
            }
            Err(e) => {
                self.set_status(format!("Failed to start debugger: {e}"));
            }
        }
    }

    /// Launch the debuggee after initialization.
    pub fn debug_launch(&mut self, program: &str) {
        let args: Vec<String> = Vec::new();
        let cwd = self
            .tab()
            .buffer
            .file_path()
            .and_then(|p| p.parent())
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();

        if let Some(client) = &mut self.dap_client {
            client.launch(program, &args, &cwd);
            // Send breakpoints for all open tabs.
            self.sync_all_breakpoints_to_adapter();
            if let Some(client) = &mut self.dap_client {
                client.configuration_done();
            }
        }
    }

    /// Send breakpoints for the current file to the adapter.
    fn sync_breakpoints_to_adapter(&mut self) {
        let file_path = self.tab().buffer.file_path().map(|p| p.to_path_buf());
        let breakpoints: Vec<(usize, Option<String>)> = self
            .tab()
            .breakpoints
            .iter()
            .map(|(&line, cond)| (line, cond.clone()))
            .collect();

        if let (Some(path), Some(client)) = (file_path, &mut self.dap_client) {
            client.set_breakpoints_with_conditions(&path, &breakpoints);
        }
    }

    /// Send breakpoints for all open tabs to the adapter.
    #[allow(clippy::type_complexity)]
    fn sync_all_breakpoints_to_adapter(&mut self) {
        let tab_info: Vec<(std::path::PathBuf, Vec<(usize, Option<String>)>)> = self
            .tabs
            .tabs()
            .iter()
            .filter_map(|tab| {
                let path = tab.buffer.file_path()?.to_path_buf();
                if tab.breakpoints.is_empty() {
                    return None;
                }
                let bps: Vec<(usize, Option<String>)> = tab
                    .breakpoints
                    .iter()
                    .map(|(&line, cond)| (line, cond.clone()))
                    .collect();
                Some((path, bps))
            })
            .collect();

        if let Some(client) = &mut self.dap_client {
            for (path, bps) in &tab_info {
                client.set_breakpoints_with_conditions(path, bps);
            }
        }
    }

    /// Continue execution in the debug session.
    pub fn debug_continue(&mut self) {
        let thread_id = self.debug_panel.state.stopped_thread_id.unwrap_or(1);
        if let Some(client) = &mut self.dap_client {
            client.continue_exec(thread_id);
            self.debug_panel.state.status = crate::debug_panel::SessionStatus::Running;
            self.debug_panel.state.clear_stopped();
        }
    }

    /// Step over in the debug session.
    pub fn debug_step_over(&mut self) {
        let thread_id = self.debug_panel.state.stopped_thread_id.unwrap_or(1);
        if let Some(client) = &mut self.dap_client {
            client.next(thread_id);
            self.debug_panel.state.status = crate::debug_panel::SessionStatus::Running;
        }
    }

    /// Step in in the debug session.
    pub fn debug_step_in(&mut self) {
        let thread_id = self.debug_panel.state.stopped_thread_id.unwrap_or(1);
        if let Some(client) = &mut self.dap_client {
            client.step_in(thread_id);
            self.debug_panel.state.status = crate::debug_panel::SessionStatus::Running;
        }
    }

    /// Step out in the debug session.
    pub fn debug_step_out(&mut self) {
        let thread_id = self.debug_panel.state.stopped_thread_id.unwrap_or(1);
        if let Some(client) = &mut self.dap_client {
            client.step_out(thread_id);
            self.debug_panel.state.status = crate::debug_panel::SessionStatus::Running;
        }
    }

    /// Stop the debug session.
    pub fn debug_stop(&mut self) {
        if let Some(mut client) = self.dap_client.take() {
            client.disconnect();
        }
        self.debug_panel.state.reset();
        self.debug_panel_focused = false;
        self.set_status("Debug session ended");
    }

    /// Poll DAP events and update debug state.
    pub fn poll_dap_events(&mut self) {
        let events = match &mut self.dap_client {
            Some(client) => client.poll_events(),
            None => return,
        };

        for event in events {
            match event {
                crate::dap::DapEvent::Initialized => {
                    // Adapter is ready — send launch if we have a program.
                    // For now, users must use :debug <program> to launch.
                }
                crate::dap::DapEvent::Stopped { thread_id, reason } => {
                    self.debug_panel.state.status =
                        crate::debug_panel::SessionStatus::Stopped(reason);
                    self.debug_panel.state.stopped_thread_id = Some(thread_id);
                    self.debug_panel.open();
                    // Auto-request stack trace.
                    if let Some(client) = &mut self.dap_client {
                        client.request_stack_trace(thread_id);
                    }
                }
                crate::dap::DapEvent::Continued { .. } => {
                    self.debug_panel.state.status = crate::debug_panel::SessionStatus::Running;
                    self.debug_panel.state.clear_stopped();
                }
                crate::dap::DapEvent::Terminated => {
                    self.debug_panel.state.status = crate::debug_panel::SessionStatus::Terminated;
                    self.dap_client = None;
                    self.set_status("Debug session terminated");
                }
                crate::dap::DapEvent::Output { output, .. } => {
                    // Split output into lines and append.
                    for line in output.lines() {
                        self.debug_panel.state.output_lines.push(line.to_string());
                    }
                }
                crate::dap::DapEvent::StackTrace(frames) => {
                    // Navigate to the top frame location.
                    if let Some(frame) = frames.first() {
                        if let Some(ref path) = frame.source_path {
                            let line = frame.line.saturating_sub(1) as usize; // DAP is 1-indexed
                            self.debug_panel.state.stopped_file = Some(path.clone());
                            self.debug_panel.state.stopped_line = Some(line);
                            // TODO: open file if not already open and scroll to line
                        }
                    }
                    self.debug_panel.state.stack_frames = frames;
                    self.debug_panel.state.selected_frame = 0;
                    // Auto-request scopes for the top frame.
                    if let Some(frame) = self.debug_panel.state.stack_frames.first() {
                        let frame_id = frame.id;
                        if let Some(client) = &mut self.dap_client {
                            client.request_scopes(frame_id);
                        }
                    }
                }
                crate::dap::DapEvent::Scopes(scopes) => {
                    self.debug_panel.state.scopes = scopes;
                    // Auto-request variables for the first non-expensive scope.
                    if let Some(scope) = self.debug_panel.state.scopes.iter().find(|s| !s.expensive)
                    {
                        let var_ref = scope.variables_reference;
                        if let Some(client) = &mut self.dap_client {
                            client.request_variables(var_ref);
                        }
                    }
                }
                crate::dap::DapEvent::Variables { reference, vars } => {
                    let nodes: Vec<crate::debug_panel::VariableNode> = vars
                        .iter()
                        .map(|v| crate::debug_panel::VariableNode {
                            name: v.name.clone(),
                            value: v.value.clone(),
                            type_name: v.type_name.clone(),
                            indent: 0,
                            expandable: v.variables_reference > 0,
                            expanded: false,
                            variables_reference: v.variables_reference,
                        })
                        .collect();

                    if reference == 0 {
                        // Top-level variables (scope response).
                        self.debug_panel.state.variables = nodes;
                        self.debug_panel.state.selected_var = 0;
                    } else {
                        // Child variables: insert after the parent node.
                        let parent_idx = self
                            .debug_panel
                            .state
                            .variables
                            .iter()
                            .position(|n| n.variables_reference == reference);
                        if let Some(idx) = parent_idx {
                            let parent_indent = self.debug_panel.state.variables[idx].indent;
                            self.debug_panel.state.variables[idx].expanded = true;
                            // Remove any existing children first.
                            let mut remove_end = idx + 1;
                            while remove_end < self.debug_panel.state.variables.len()
                                && self.debug_panel.state.variables[remove_end].indent
                                    > parent_indent
                            {
                                remove_end += 1;
                            }
                            if remove_end > idx + 1 {
                                self.debug_panel.state.variables.drain(idx + 1..remove_end);
                            }
                            // Insert children with incremented indent.
                            let children: Vec<crate::debug_panel::VariableNode> = nodes
                                .into_iter()
                                .map(|mut n| {
                                    n.indent = parent_indent + 1;
                                    n
                                })
                                .collect();
                            let insert_pos = (idx + 1).min(self.debug_panel.state.variables.len());
                            for (i, child) in children.into_iter().enumerate() {
                                self.debug_panel
                                    .state
                                    .variables
                                    .insert(insert_pos + i, child);
                            }
                        }
                    }
                }
                crate::dap::DapEvent::BreakpointsSet(_results) => {
                    // Could update verified status in the future.
                }
                crate::dap::DapEvent::Error(msg) => {
                    self.set_status(format!("Debug error: {msg}"));
                }
            }
        }
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

    /// Stage all changed files in the source control panel.
    pub fn sc_stage_all(&mut self) {
        if let Some(repo) = &self.git_repo {
            self.source_control.stage_all(repo);
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
            self.conversation_history.visible = false;
            self.conversation_history_focused = false;
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
            r.width > 0
                && r.height > 0
                && col >= r.x
                && col < r.x + r.width
                && row >= r.y
                && row < r.y + r.height
        };

        // Click on the update notification toast → open the update modal.
        if self.update_notification_visible && point_in(self.update_notification_rect) {
            self.show_update_modal();
            return;
        }

        // If the close-tab confirm modal is visible, clicks outside dismiss it.
        if self.tab_close_confirm.is_some() {
            self.tab_close_confirm = None;
            return;
        }

        // If the update modal is visible, clicks outside dismiss it.
        if self.update_modal_visible {
            self.update_modal_visible = false;
            return;
        }

        // Status bar: click to open relevant features.
        if point_in(self.status_bar_rect) {
            // Open command palette as the default action for status bar clicks.
            self.open_command_palette();
            return;
        }

        // Tab bar: check close buttons first, then tab switching.
        if point_in(self.tab_bar_rect) {
            // Check if click is on a close button.
            for &(tab_idx, x_start, x_end) in &self.tab_close_btn_ranges {
                if col >= x_start && col < x_end && row == self.tab_bar_rect.y {
                    // Close button clicked — check for unsaved changes.
                    if tab_idx < self.tabs.count() && self.tabs.tabs()[tab_idx].is_modified() {
                        self.tab_close_confirm = Some(tab_idx);
                    } else if self.close_tab_by_index(tab_idx) {
                        self.should_quit = true;
                    }
                    return;
                }
            }

            // Otherwise, switch to the clicked tab.
            let click_x = (col - self.tab_bar_rect.x) as usize;
            let max_width = self.tab_bar_rect.width as usize;
            let close_btn_len = 2; // "× " display width
            let mut x = 0usize;
            for (i, tab) in self.tabs.tabs().iter().enumerate() {
                let label = if i < 9 {
                    format!(" {}:{} ", i + 1, tab.title())
                } else {
                    format!(" {} ", tab.title())
                };
                let label_len = label.chars().count();
                let total_len = label_len + close_btn_len;
                if x + total_len + 1 > max_width {
                    break;
                }
                if click_x >= x && click_x < x + label_len {
                    self.tabs.switch_to(i);
                    return;
                }
                x += total_len;
                // Separator character.
                if i + 1 < self.tabs.count() {
                    x += 1;
                }
            }
            return;
        }

        if self.terminal().visible && point_in(self.terminal_rect) {
            self.terminal_focused = true;
            self.file_tree_focused = false;
            self.source_control_focused = false;
            self.conversation_history_focused = false;
        } else if self.file_tree.visible && point_in(self.file_tree_rect) {
            self.terminal_focused = false;
            self.conversation_history_focused = false;
            self.chat_panel_focused = false;

            // Layout: border (1) + tab header row (1) + entries.
            let tab_header_y = self.file_tree_rect.y + 1; // border
            let entries_start_y = tab_header_y + 1;

            // Click on the "Files | Git" tab header → switch sidebar view.
            if row == tab_header_y {
                let local_x = col.saturating_sub(self.file_tree_rect.x + 1);
                // " Files | Git " — "Files" spans roughly cols 0..6, "Git" from ~9.
                if local_x < 7 {
                    // Clicked "Files".
                    self.sidebar_view = SidebarView::Files;
                    self.file_tree_focused = true;
                    self.source_control_focused = false;
                } else {
                    // Clicked "Git" (or the separator area).
                    self.sidebar_view = SidebarView::Git;
                    self.source_control_focused = true;
                    self.file_tree_focused = false;
                    self.refresh_source_control();
                }
            } else if self.sidebar_view == SidebarView::Git {
                self.source_control_focused = true;
                self.file_tree_focused = false;
                // Check if click is on the AI commit message button.
                let ai_btn = self.ai_commit_btn_rect;
                let on_ai_btn = ai_btn.width > 0
                    && col >= ai_btn.x
                    && col < ai_btn.x + ai_btn.width
                    && row >= ai_btn.y
                    && row < ai_btn.y + ai_btn.height;
                // Check if click is on the "stage all" button.
                let btn = self.stage_all_btn_rect;
                let on_stage_btn = btn.width > 0
                    && col >= btn.x
                    && col < btn.x + btn.width
                    && row >= btn.y
                    && row < btn.y + btn.height;

                if on_ai_btn {
                    self.generate_commit_message();
                } else if on_stage_btn {
                    self.sc_stage_all();
                } else if row >= entries_start_y {
                    // Map click to a git panel entry.
                    self.handle_git_panel_click(row, entries_start_y);
                }
            } else {
                self.file_tree_focused = true;
                self.source_control_focused = false;
                // Map the click row to a file tree entry.
                if row >= entries_start_y {
                    let visible_height = self.file_tree_rect.height.saturating_sub(3) as usize; // border top + tab header + border bottom
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

            // Check if click is on the gutter fold indicator.
            let content_x = self.editor_rect.x + 1; // border
            let content_y = self.editor_rect.y + 1; // border
            let gutter_width: u16 = 6;
            let in_gutter = col >= content_x && col < content_x + gutter_width && row >= content_y;
            if in_gutter {
                let clicked_row = (row - content_y) as usize;
                let target_line = self.tab().scroll_row + clicked_row;
                let is_folded = self.tab().folded_ranges.contains_key(&target_line);
                let is_foldable = self.tab().foldable_ranges.contains_key(&target_line);
                if is_folded {
                    // Unfold.
                    self.tab_mut().folded_ranges.remove(&target_line);
                    return;
                } else if is_foldable {
                    // Fold.
                    if let Some(&end) = self.tab().foldable_ranges.get(&target_line) {
                        self.tab_mut().folded_ranges.insert(target_line, end);
                    }
                    return;
                }
            }

            // Move cursor and set visual anchor for potential drag selection.
            if self.screen_to_cursor(col, row) {
                // Clear any existing selection and record anchor for drag.
                self.mode = Mode::Normal;
                let cursor = self.tab().cursor;
                self.tab_mut().visual_anchor = Some(cursor);
            }
        }
    }

    /// Map a click in the git source-control panel to entry selection.
    ///
    /// The git panel layout (below the tab header) is:
    ///   branch line, blank, commit header, message lines (1-3), blank,
    ///   staged header, staged entries, blank, changed header, changed entries.
    fn handle_git_panel_click(&mut self, row: u16, entries_start_y: u16) {
        use crate::source_control::GitPanelSection;

        let mut y = entries_start_y as usize;

        // Branch line.
        y += 1;
        // Blank separator.
        y += 1;
        // Commit message header.
        y += 1;
        // Commit message lines (1-3).
        let msg_lines = if self.source_control.commit_message.is_empty() {
            1
        } else {
            self.source_control.commit_message.lines().count().min(3)
        };
        y += msg_lines;
        // Blank separator.
        y += 1;

        let click = row as usize;

        // Staged header.
        let staged_header_y = y;
        y += 1;
        let staged_start = y;
        let staged_count = self.source_control.staged.len();
        y += staged_count;

        // Check if click is in staged entries.
        if click >= staged_start && click < staged_start + staged_count {
            let idx = click - staged_start;
            self.source_control.focused_section = GitPanelSection::StagedFiles;
            self.source_control.selected = idx;
            return;
        }
        // Click on staged header → focus staged section.
        if click == staged_header_y {
            self.source_control.focused_section = GitPanelSection::StagedFiles;
            self.source_control.selected = 0;
            return;
        }

        // Blank separator.
        y += 1;

        // Changed header.
        let changed_header_y = y;
        y += 1;
        let changed_start = y;
        let changed_count = self.source_control.changed.len();

        // Check if click is in changed entries.
        if click >= changed_start && click < changed_start + changed_count {
            let idx = click - changed_start;
            self.source_control.focused_section = GitPanelSection::ChangedFiles;
            self.source_control.selected = idx;
            return;
        }
        // Click on changed header → focus changed section.
        if click == changed_header_y {
            self.source_control.focused_section = GitPanelSection::ChangedFiles;
            self.source_control.selected = 0;
        }
    }

    /// Detect if a mouse click is on a panel border (for resize dragging).
    ///
    /// Returns the drag type if the click is within 1 pixel of a border.
    fn detect_panel_border(&self, col: u16, row: u16) -> Option<PanelResizeDrag> {
        // Left sidebar right border.
        if self.file_tree.visible && self.file_tree_rect.width > 0 {
            let border_x = self.file_tree_rect.x + self.file_tree_rect.width;
            if col == border_x || col == border_x.saturating_sub(1) {
                return Some(PanelResizeDrag::LeftSidebar);
            }
        }

        // Right panel left border (chat, history, visor).
        let right_rect = if self.chat_panel.visible {
            self.chat_panel_rect
        } else if self.conversation_history.visible {
            self.conv_history_rect
        } else if self.ai_visor.visible {
            self.ai_visor_rect
        } else {
            Rect::default()
        };
        if right_rect.width > 0 {
            let border_x = right_rect.x;
            if col == border_x || col == border_x.saturating_add(1) {
                return Some(PanelResizeDrag::RightPanel);
            }
        }

        // Terminal top border.
        if self.terminal().visible && self.terminal_rect.height > 0 {
            let border_y = self.terminal_rect.y;
            if row == border_y || row == border_y.saturating_sub(1) {
                // Only if within the horizontal range.
                if col >= self.terminal_rect.x
                    && col < self.terminal_rect.x + self.terminal_rect.width
                {
                    return Some(PanelResizeDrag::Terminal);
                }
            }
        }

        None
    }

    /// Apply a panel resize based on mouse drag position.
    fn apply_panel_resize(&mut self, drag: PanelResizeDrag, col: u16, _row: u16) {
        match drag {
            PanelResizeDrag::LeftSidebar => {
                // New width = mouse column position (min 15, max half of screen).
                let max_w = (self.editor_rect.x + self.editor_rect.width) / 2;
                let new_width = col.max(15).min(max_w.max(15));
                self.file_tree.width = new_width;
            }
            PanelResizeDrag::RightPanel => {
                // New width = distance from mouse to right edge.
                let total = self.editor_rect.x + self.editor_rect.width;
                let right_rect = if self.chat_panel.visible {
                    self.chat_panel_rect
                } else if self.conversation_history.visible {
                    self.conv_history_rect
                } else if self.ai_visor.visible {
                    self.ai_visor_rect
                } else {
                    return;
                };
                let right_edge = right_rect.x + right_rect.width;
                let new_width = right_edge.saturating_sub(col).max(20).min(total / 2);
                if self.chat_panel.visible {
                    self.chat_panel.width = new_width;
                } else if self.conversation_history.visible {
                    self.conversation_history.width = new_width;
                } else if self.ai_visor.visible {
                    self.ai_visor.width = new_width;
                }
            }
            PanelResizeDrag::Terminal => {
                // New height = distance from mouse to bottom (minus status/command bars).
                let total_h = self.terminal_rect.y + self.terminal_rect.height;
                let new_height = total_h.saturating_sub(_row).clamp(3, 50);
                self.terminal_mut().height = new_height;
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
                    let safe_end =
                        tab.scroll_row + viewport_h.saturating_sub(1).saturating_sub(margin);
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
        } else if self.terminal().visible && point_in(self.terminal_rect) {
            // Scroll the terminal scrollback.
            for _ in 0..scroll_lines {
                if up {
                    self.terminal_mut().scroll_up();
                } else {
                    self.terminal_mut().scroll_down();
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
            self.chat_panel.visible = false;
            self.chat_panel_focused = false;
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
            // Cache project files for @-mention autocomplete.
            let root = self
                .tab()
                .buffer
                .file_path()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf())
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            self.chat_panel.cache_project_files(&root);
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
                    self.chat_panel
                        .push_system_message(&format!("Unknown command: {}", text.trim()));
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
        self.ensure_conversation_store();
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
        // Refresh history panel so the chat interaction appears immediately.
        self.refresh_conversation_history();

        // Capture selection context for the chat indicator.
        if let Some((sel_start, sel_end)) = self.visual_selection_range() {
            let tab = self.tab();
            let start_cur = tab.buffer.char_idx_to_cursor(sel_start);
            let end_cur = tab.buffer.char_idx_to_cursor(sel_end);
            let lines = end_cur.row.saturating_sub(start_cur.row) + 1;
            let file_name = tab.file_name();
            self.chat_panel.selection_context = Some(format!(
                "{lines} line{} from {file_name}",
                if lines == 1 { "" } else { "s" }
            ));
        } else {
            self.chat_panel.selection_context = None;
        }

        // Expand @-mentions in the last user message.
        let mention_context = self.expand_mentions(&text);

        // Build system prompt with editor context + mention context.
        let mut system = self.build_chat_system_prompt();
        if !mention_context.is_empty() {
            system.push_str("\n\n--- Referenced content (@-mentions) ---\n");
            system.push_str(&mention_context);
        }
        let messages = self.chat_panel.build_messages();

        let client = match self.ai_client.as_ref() {
            Some(c) => c,
            None => {
                self.set_status("No AI client configured (set ANTHROPIC_API_KEY)");
                return;
            }
        };

        // Use tools if the backend supports them.
        // Select model based on feature: agent mode uses agent_model, otherwise chat_model.
        let feature = if self.agent_mode.is_some() {
            "agent"
        } else {
            "chat"
        };
        let model = self.config.ai.model_for(feature).to_string();

        if client.supports_tools() {
            let mut tools = editor_tools();
            // Include subagent tools when in agent mode.
            if self.agent_mode.is_some() {
                tools.extend(aura_ai::agent_tools());
            }
            let rx =
                client.stream_completion_with_tools_and_model(&system, messages, tools, &model);
            self.chat_receiver = Some(rx);
            self.chat_panel.start_streaming();
            self.chat_panel.in_tool_loop = false;
            self.chat_panel.tool_loop_count = 0;
            self.tool_loop_system_prompt = system;
        } else {
            let rx = client.stream_completion_with_model(&system, messages, &model);
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
                    self.ensure_conversation_store();
                    if self.chat_panel.conversation_id.is_none() {
                        self.ensure_chat_conversation();
                    }
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

                    // If in agent planning mode and no plan yet, try to parse one.
                    let needs_plan_approval = if let Some(ref mut session) = self.agent_mode {
                        if session.plan.is_none() {
                            if let Some(plan) = crate::agent_plan::parse_plan_from_response(
                                &full_text,
                                &session.task.clone(),
                            ) {
                                session.plan = Some(plan);
                                session
                                    .timeline
                                    .add(crate::agent_timeline::TimelineEntry::new(
                                        crate::agent_timeline::TimelineActionType::PlanCreated,
                                        "Agent plan created — awaiting approval",
                                    ));
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    if needs_plan_approval {
                        self.chat_panel.push_system_message(
                            "📋 Plan ready — press Y to approve and execute, N to cancel.",
                        );
                        self.chat_panel.plan_pending_approval = true;
                        self.chat_panel_focused = true;
                    }

                    // Refresh history so new interactions appear in the AI History panel.
                    self.refresh_conversation_history();
                    return;
                }
                Ok(AiEvent::ToolUse { id, name, input }) => {
                    // A tool call is being streamed — track it.
                    let permission = tool_permission(&name);
                    let status = match permission {
                        ToolPermission::AutoApprove => ToolCallStatus::Running,
                        ToolPermission::RequiresApproval => ToolCallStatus::PendingApproval,
                    };
                    let idx = self
                        .chat_panel
                        .add_tool_call(&id, &name, input.clone(), status);
                    self.pending_tool_calls.push(PendingToolCall {
                        id,
                        name,
                        input,
                        item_index: idx,
                    });
                }
                Ok(AiEvent::ToolUseComplete {
                    text,
                    content_blocks,
                }) => {
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

        // Determine auto-approve logic: trust level in agent mode, else base permission.
        let agent_trust = self.agent_mode.as_ref().map(|s| s.trust_level);

        // Process all auto-approve tools first.
        let mut needs_approval = Vec::new();
        let calls: Vec<PendingToolCall> = std::mem::take(&mut self.pending_tool_calls);

        for call in calls {
            let permission = tool_permission(&call.name);
            let should_auto = match agent_trust {
                Some(trust) => trust.auto_approves(&call.name),
                None => permission == ToolPermission::AutoApprove,
            };

            if should_auto {
                // Track agent metrics.
                let mut limit_reached = false;
                if let Some(ref mut session) = self.agent_mode {
                    session.iteration += 1;
                    if call.name == "edit_file" {
                        if let Some(path) = call.input.get("path").and_then(|v| v.as_str()) {
                            if !session.files_changed.contains(&path.to_string()) {
                                session.files_changed.push(path.to_string());
                            }
                            session.timeline.record_file_change(path);
                        }
                    }
                    if call.name == "run_command" {
                        session.commands_run += 1;
                        if let Some(cmd) = call.input.get("command").and_then(|v| v.as_str()) {
                            session.timeline.record_command(cmd, 0);
                        }
                    }
                    session.timeline.record_tool(&call.name, true);
                    // Check iteration limit.
                    if session.iteration > session.max_iterations {
                        limit_reached = true;
                    }
                }
                if limit_reached {
                    self.stop_agent("Iteration limit reached");
                    return;
                }
                // Handle subagent tools directly (they need access to App state).
                if matches!(
                    call.name.as_str(),
                    "spawn_subagent" | "check_subagent" | "cancel_subagent"
                ) {
                    let result = match call.name.as_str() {
                        "spawn_subagent" => self.spawn_subagent_from_tool_call(&call.input),
                        "check_subagent" => self.check_subagent(&call.input),
                        "cancel_subagent" => self.cancel_subagent(&call.input),
                        _ => unreachable!(),
                    };
                    self.chat_panel
                        .set_tool_result(call.item_index, result.clone(), true);
                    self.chat_panel
                        .add_tool_result_to_context(&call.id, &result, false);
                } else {
                    self.execute_tool_call(&call);
                }
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
                let remaining: Vec<PendingToolCall> = std::mem::take(&mut self.pending_tool_calls);
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
        self.chat_panel
            .add_tool_result_to_context(&call.id, "User denied this tool call.", true);

        // Deny all remaining pending tools too.
        let remaining: Vec<PendingToolCall> = std::mem::take(&mut self.pending_tool_calls);
        for c in &remaining {
            self.chat_panel
                .update_tool_status(c.item_index, ToolCallStatus::Denied);
            self.chat_panel
                .add_tool_result_to_context(&c.id, "User denied this tool call.", true);
        }
        self.chat_panel.pending_approval = None;
        self.continue_tool_loop();
    }

    /// Continue the tool loop by sending tool results back to the API.
    fn continue_tool_loop(&mut self) {
        self.chat_panel.tool_loop_count = self.chat_panel.tool_loop_count.saturating_add(1);

        if self.chat_panel.tool_loop_count >= chat_tools::MAX_TOOL_ITERATIONS {
            self.chat_panel
                .push_system_message("Tool loop limit reached. Stopping automatic tool use.");
            self.chat_panel.in_tool_loop = false;
            return;
        }

        let client = match &self.ai_client {
            Some(c) => c,
            None => return,
        };

        let messages = self.chat_panel.build_messages();
        let mut tools = editor_tools();
        // Include subagent tools when in agent mode.
        if self.agent_mode.is_some() {
            tools.extend(aura_ai::agent_tools());
        }
        let system = self.tool_loop_system_prompt.clone();
        let feature = if self.agent_mode.is_some() {
            "agent"
        } else {
            "chat"
        };
        let model = self.config.ai.model_for(feature).to_string();
        let rx = client.stream_completion_with_tools_and_model(&system, messages, tools, &model);
        self.chat_receiver = Some(rx);
        self.chat_panel.start_streaming();
    }

    /// Poll all active subagents for streaming events.
    fn poll_subagent_events(&mut self) {
        let session = match self.agent_mode.as_mut() {
            Some(s) => s,
            None => return,
        };

        let active_ids = session.subagent_manager.active_ids();
        let project_root = std::env::current_dir().unwrap_or_default();

        for id in active_ids {
            let subagent = match session.subagent_manager.get_mut(&id) {
                Some(s) => s,
                None => continue,
            };

            let rx = match subagent.receiver.as_ref() {
                Some(rx) => rx,
                None => continue,
            };

            loop {
                match rx.try_recv() {
                    Ok(AiEvent::Token(text)) => {
                        subagent.streaming_text.push_str(&text);
                    }
                    Ok(AiEvent::Done(full_text)) => {
                        subagent.status =
                            crate::subagent::SubagentStatus::Completed(full_text.clone());
                        subagent.receiver = None;
                        let label = subagent.role.label().to_string();
                        session
                            .timeline
                            .add(crate::agent_timeline::TimelineEntry::for_subagent(
                                &id,
                                crate::agent_timeline::TimelineActionType::SubagentCompleted {
                                    summary: full_text.chars().take(100).collect::<String>(),
                                },
                                &format!("[{label}] completed"),
                            ));
                        break;
                    }
                    Ok(AiEvent::ToolUse {
                        id: tool_id,
                        name,
                        input,
                    }) => {
                        // Check tool restrictions.
                        if subagent.tool_restrictions.allows(&name) {
                            subagent.iteration += 1;
                            if subagent.iteration > subagent.max_iterations {
                                subagent.status = crate::subagent::SubagentStatus::Failed(
                                    "Iteration limit reached".into(),
                                );
                                subagent.receiver = None;
                                break;
                            }
                            // Execute the tool.
                            let result = chat_tools::execute_tool(&name, &input, &project_root);
                            let (content, is_error) = match result {
                                Ok(output) => (output, false),
                                Err(err) => (err, true),
                            };
                            // Add tool_use and tool_result to subagent context.
                            subagent.current_assistant_blocks.push(
                                aura_ai::ContentBlock::ToolUse {
                                    id: tool_id.clone(),
                                    name: name.clone(),
                                    input: input.clone(),
                                },
                            );
                            // Build the assistant message from accumulated blocks.
                            let assistant_blocks =
                                std::mem::take(&mut subagent.current_assistant_blocks);
                            if !assistant_blocks.is_empty() {
                                subagent.context_messages.push(aura_ai::Message {
                                    role: "assistant".to_string(),
                                    content: aura_ai::MessageContent::Blocks(assistant_blocks),
                                });
                            }
                            // Add tool result.
                            subagent.context_messages.push(aura_ai::Message {
                                role: "user".to_string(),
                                content: aura_ai::MessageContent::Blocks(vec![
                                    aura_ai::ContentBlock::ToolResult {
                                        tool_use_id: tool_id,
                                        content,
                                        is_error: if is_error { Some(true) } else { None },
                                    },
                                ]),
                            });
                        } else {
                            // Tool not allowed — send denial.
                            subagent.current_assistant_blocks.push(
                                aura_ai::ContentBlock::ToolUse {
                                    id: tool_id.clone(),
                                    name: name.clone(),
                                    input,
                                },
                            );
                            let assistant_blocks =
                                std::mem::take(&mut subagent.current_assistant_blocks);
                            if !assistant_blocks.is_empty() {
                                subagent.context_messages.push(aura_ai::Message {
                                    role: "assistant".to_string(),
                                    content: aura_ai::MessageContent::Blocks(assistant_blocks),
                                });
                            }
                            subagent.context_messages.push(aura_ai::Message {
                                role: "user".to_string(),
                                content: aura_ai::MessageContent::Blocks(vec![
                                    aura_ai::ContentBlock::ToolResult {
                                        tool_use_id: tool_id,
                                        content: format!(
                                            "Tool '{name}' is not allowed for this subagent role."
                                        ),
                                        is_error: Some(true),
                                    },
                                ]),
                            });
                        }
                    }
                    Ok(AiEvent::ToolUseComplete {
                        text: _,
                        content_blocks,
                    }) => {
                        // Store assistant blocks and continue subagent loop.
                        subagent.current_assistant_blocks = content_blocks;
                        subagent.status = crate::subagent::SubagentStatus::WaitingTool;
                        // Continue the subagent's tool loop.
                        if let Some(client) = &self.ai_client {
                            let messages = subagent.context_messages.clone();
                            let tools = editor_tools();
                            let system = subagent.system_prompt.clone();
                            let rx = client.stream_completion_with_tools(&system, messages, tools);
                            subagent.receiver = Some(rx);
                            subagent.status = crate::subagent::SubagentStatus::Running;
                        }
                        break;
                    }
                    Ok(AiEvent::Activity(_)) => {}
                    Ok(AiEvent::Error(err)) => {
                        subagent.status = crate::subagent::SubagentStatus::Failed(err);
                        subagent.receiver = None;
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        if !subagent.streaming_text.is_empty() {
                            let text = subagent.streaming_text.clone();
                            subagent.status = crate::subagent::SubagentStatus::Completed(text);
                        } else {
                            subagent.status =
                                crate::subagent::SubagentStatus::Failed("Connection lost".into());
                        }
                        subagent.receiver = None;
                        break;
                    }
                }
            }
        }
    }

    /// Spawn a subagent from a tool call and return the result string.
    fn spawn_subagent_from_tool_call(&mut self, input: &serde_json::Value) -> String {
        let task = input
            .get("task")
            .and_then(|v| v.as_str())
            .unwrap_or("unspecified task");
        let role_str = input
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("custom");
        let role = crate::subagent::SubagentRole::parse_str(role_str);

        // Build tool restrictions.
        let tool_restrictions =
            if let Some(tools_arr) = input.get("tools").and_then(|v| v.as_array()) {
                let allowed: Vec<String> = tools_arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                if allowed.is_empty() {
                    role.default_tool_restrictions()
                } else {
                    crate::subagent::ToolRestrictions {
                        allowed_tools: allowed,
                    }
                }
            } else {
                role.default_tool_restrictions()
            };

        // Generate a short ID.
        let id = format!(
            "sub_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                % 100_000
        );

        let mut subagent = crate::subagent::Subagent::new(
            id.clone(),
            role.clone(),
            task.to_string(),
            tool_restrictions,
            25, // max iterations per subagent
        );

        // Send the initial request.
        if let Some(client) = &self.ai_client {
            let system_prompt = subagent.system_prompt.clone();
            let messages = vec![aura_ai::Message {
                role: "user".to_string(),
                content: aura_ai::MessageContent::Text(task.to_string()),
            }];
            subagent.context_messages = messages.clone();
            let tools = editor_tools();
            let rx = client.stream_completion_with_tools(&system_prompt, messages, tools);
            subagent.receiver = Some(rx);
        }

        let role_label = role.label().to_string();
        let can_add = self
            .agent_mode
            .as_ref()
            .map(|s| s.subagent_manager.can_spawn())
            .unwrap_or(false);

        if !can_add {
            return "Cannot spawn subagent: maximum concurrent subagents reached".to_string();
        }

        if let Some(ref mut session) = self.agent_mode {
            session.subagent_manager.add(subagent);
            session
                .timeline
                .add(crate::agent_timeline::TimelineEntry::new(
                    crate::agent_timeline::TimelineActionType::SubagentSpawned {
                        role: role_label.clone(),
                    },
                    &format!("Spawned [{role_label}] subagent: {task}"),
                ));
        }

        format!("Subagent spawned with ID: {id} (role: {role_label}). Use check_subagent to get results.")
    }

    /// Check the status of a subagent.
    fn check_subagent(&self, input: &serde_json::Value) -> String {
        let id = input.get("id").and_then(|v| v.as_str()).unwrap_or("");

        let session = match &self.agent_mode {
            Some(s) => s,
            None => return "No active agent session".to_string(),
        };

        match session.subagent_manager.get(id) {
            Some(subagent) => {
                let status = subagent.status.label();
                let iters = subagent.iteration;
                match &subagent.status {
                    crate::subagent::SubagentStatus::Completed(summary) => {
                        format!("Status: {status} ({iters} iterations)\n\nResult:\n{summary}")
                    }
                    crate::subagent::SubagentStatus::Failed(err) => {
                        format!("Status: {status} ({iters} iterations)\n\nError: {err}")
                    }
                    _ => {
                        let partial = if subagent.streaming_text.is_empty() {
                            "No output yet.".to_string()
                        } else {
                            let preview: String =
                                subagent.streaming_text.chars().take(200).collect();
                            format!("Partial output:\n{preview}...")
                        };
                        format!("Status: {status} ({iters} iterations)\n{partial}")
                    }
                }
            }
            None => format!("Subagent '{id}' not found"),
        }
    }

    /// Cancel a subagent.
    fn cancel_subagent(&mut self, input: &serde_json::Value) -> String {
        let id = input.get("id").and_then(|v| v.as_str()).unwrap_or("");

        if let Some(ref mut session) = self.agent_mode {
            if session.subagent_manager.cancel(id) {
                format!("Subagent '{id}' cancelled")
            } else {
                format!("Subagent '{id}' not found")
            }
        } else {
            "No active agent session".to_string()
        }
    }

    /// Build a system prompt for chat that includes editor context.
    /// Expand @-mentions in a chat message into file/context content.
    fn expand_mentions(&self, text: &str) -> String {
        let mut context = String::new();
        let project_root = self
            .tab()
            .buffer
            .file_path()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        // Find all @-mentions in the text.
        for word in text.split_whitespace() {
            if !word.starts_with('@') || word.len() <= 1 {
                continue;
            }
            let mention = &word[1..]; // Strip leading @.

            match mention {
                "selection" => {
                    if let Some((sel_start, sel_end)) = self.visual_selection_range() {
                        let selected = self
                            .tab()
                            .buffer
                            .rope()
                            .slice(sel_start..sel_end)
                            .to_string();
                        context.push_str(&format!(
                            "\n--- @selection ---\n{}\n--- end @selection ---\n",
                            selected
                        ));
                    }
                }
                "buffer" => {
                    let content = self.tab().buffer.text();
                    let file_name = self.tab().file_name();
                    // Truncate very large buffers.
                    let truncated: String = content.chars().take(30000).collect();
                    context.push_str(&format!(
                        "\n--- @buffer ({}) ---\n{}\n--- end @buffer ---\n",
                        file_name, truncated
                    ));
                }
                "errors" => {
                    let diagnostics: Vec<String> = self
                        .tab()
                        .diagnostics
                        .iter()
                        .map(|d| {
                            format!(
                                "L{}:{} [{}] {}",
                                d.range.start.line + 1,
                                d.range.start.character + 1,
                                if d.is_error() {
                                    "error"
                                } else if d.is_warning() {
                                    "warning"
                                } else {
                                    "info"
                                },
                                d.message
                            )
                        })
                        .collect();
                    if diagnostics.is_empty() {
                        context
                            .push_str("\n--- @errors ---\nNo diagnostics.\n--- end @errors ---\n");
                    } else {
                        context.push_str(&format!(
                            "\n--- @errors ({} diagnostics) ---\n{}\n--- end @errors ---\n",
                            diagnostics.len(),
                            diagnostics.join("\n")
                        ));
                    }
                }
                mention if mention.starts_with("docs:") => {
                    // @docs:name — read from .aura/docs/<name>.md
                    let doc_name = &mention[5..];
                    let doc_path = project_root.join(format!(".aura/docs/{doc_name}.md"));
                    match std::fs::read_to_string(&doc_path) {
                        Ok(content) => {
                            let truncated: String = content.chars().take(30000).collect();
                            context.push_str(&format!(
                                "\n--- @docs:{doc_name} ---\n{truncated}\n--- end @docs:{doc_name} ---\n"
                            ));
                        }
                        Err(_) => {
                            // Try .txt extension.
                            let txt_path = project_root.join(format!(".aura/docs/{doc_name}.txt"));
                            match std::fs::read_to_string(&txt_path) {
                                Ok(content) => {
                                    let truncated: String = content.chars().take(30000).collect();
                                    context.push_str(&format!(
                                        "\n--- @docs:{doc_name} ---\n{truncated}\n--- end @docs:{doc_name} ---\n"
                                    ));
                                }
                                Err(_) => {
                                    context.push_str(&format!(
                                        "\n--- @docs:{doc_name} ---\n(doc not found in .aura/docs/)\n--- end @docs:{doc_name} ---\n"
                                    ));
                                }
                            }
                        }
                    }
                }
                "docs" => {
                    // @docs — list available docs.
                    let docs_dir = project_root.join(".aura/docs");
                    if docs_dir.is_dir() {
                        if let Ok(entries) = std::fs::read_dir(&docs_dir) {
                            let names: Vec<String> = entries
                                .flatten()
                                .filter_map(|e| {
                                    let p = e.path();
                                    if p.extension().is_some_and(|ext| ext == "md" || ext == "txt")
                                    {
                                        p.file_stem().and_then(|s| s.to_str()).map(String::from)
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            context.push_str(&format!(
                                "\n--- @docs ---\nAvailable docs: {}\nUse @docs:<name> to include.\n--- end @docs ---\n",
                                names.join(", ")
                            ));
                        }
                    } else {
                        context.push_str(
                            "\n--- @docs ---\nNo .aura/docs/ directory found.\n--- end @docs ---\n",
                        );
                    }
                }
                file_path => {
                    // Try to read the file from the project.
                    let full_path = project_root.join(file_path);
                    match std::fs::read_to_string(&full_path) {
                        Ok(content) => {
                            let truncated: String = content.chars().take(30000).collect();
                            context.push_str(&format!(
                                "\n--- @{file_path} ---\n{truncated}\n--- end @{file_path} ---\n"
                            ));
                        }
                        Err(_) => {
                            context.push_str(&format!(
                                "\n--- @{file_path} ---\n(file not found)\n--- end @{file_path} ---\n"
                            ));
                        }
                    }
                }
            }
        }

        context
    }

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
        // Inject project rules if loaded.
        if let Some(ref rules) = self.project_rules {
            prompt.push_str("\n--- Project Rules ---\n");
            prompt.push_str(rules);
            prompt.push_str("\n--- End Project Rules ---\n\n");
        }

        prompt.push_str(&format!("Current file: {file_path}\n"));
        prompt.push_str(&format!(
            "Lines: {line_count}, Cursor: {cursor_row}:{cursor_col}\n"
        ));

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

    /// Persist Claude Code activity events to the conversation store.
    fn persist_claude_code_activity(&mut self, events: &[crate::claude_watcher::ClaudeActivity]) {
        use crate::claude_watcher::ClaudeActivity;
        use aura_core::conversation::MessageRole;

        self.ensure_conversation_store();
        let store = match &self.conversation_store {
            Some(s) => s,
            None => return,
        };
        let (branch, commit) = self.git_context();

        let mut needs_refresh = false;
        for event in events {
            let (session_id, role, content, model) = match event {
                ClaudeActivity::UserMessage { text, session_id } => (
                    session_id.as_str(),
                    MessageRole::HumanIntent,
                    text.as_str(),
                    None,
                ),
                ClaudeActivity::AssistantMessage {
                    text,
                    model,
                    session_id,
                } => (
                    session_id.as_str(),
                    MessageRole::AiResponse,
                    text.as_str(),
                    Some(model.as_str()),
                ),
                ClaudeActivity::ToolCall {
                    name,
                    input_summary,
                    session_id,
                } => {
                    tracing::debug!("Claude Code: {name}({input_summary})");
                    // Skip tool calls — only persist user/assistant messages.
                    let _ = session_id;
                    continue;
                }
                ClaudeActivity::Progress { .. } => continue,
            };

            let conv = match store.find_or_create_claude_code_conversation(
                session_id,
                commit.as_deref(),
                branch.as_deref(),
            ) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("Failed to create Claude Code conversation: {e}");
                    continue;
                }
            };

            if let Err(e) = store.add_message(&conv.id, role, content, model) {
                tracing::warn!("Failed to persist Claude Code message: {e}");
            } else {
                // Update git_commit to current HEAD so the graph links to the latest commit.
                if let Some(ref c) = commit {
                    let _ = store.update_git_commit(&conv.id, c);
                }
                needs_refresh = true;
            }
        }

        if needs_refresh {
            self.refresh_conversation_history();
        }
    }

    /// Ensure a chat conversation exists in the database.
    fn ensure_chat_conversation(&mut self) {
        if self.chat_panel.conversation_id.is_some() {
            return;
        }
        self.ensure_conversation_store();
        let (branch, commit) = self.git_context();
        if let Some(store) = &self.conversation_store {
            match store.find_or_create_chat_conversation(commit.as_deref(), branch.as_deref()) {
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
        let (branch, commit) = self.git_context();
        if let Some(store) = &self.conversation_store {
            match store.find_or_create_chat_conversation(commit.as_deref(), branch.as_deref()) {
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

        type Sample<'a> = (&'a str, usize, usize, &'a str, &'a [(&'a str, &'a str)]);
        let samples: &[Sample<'_>] = &[
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
                    tracing::warn!(
                        "Failed to lazily open conversation store at {:?}: {e}",
                        path
                    );
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
                        .create_conversation(
                            &file_path,
                            start_line,
                            end_line,
                            commit.as_deref(),
                            branch.as_deref(),
                        )
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
            if let Ok(conv) = store.create_conversation(
                &file_path,
                0,
                end_line,
                commit.as_deref(),
                branch.as_deref(),
            ) {
                let msg = if let Some(r) = role {
                    format!("Agent '{}' connected (role: {})", agent_name, r)
                } else {
                    format!("Agent '{}' connected", agent_name)
                };
                let _ = store.add_message(&conv.id, MessageRole::System, &msg, Some(agent_name));
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
            // Apply .editorconfig settings for this file.
            let ec = crate::config::lookup_editorconfig(&path);
            if let Some(ref style) = ec.indent_style {
                self.config.editor.spaces_for_tabs = style == "space";
            }
            if let Some(size) = ec.indent_size {
                self.config.editor.tab_width = size;
            }
            self.set_status(format!("Opened {}", path.display()));
        } else {
            self.set_status(format!("Switched to {}", path.display()));
        }
        // Detect inline conflict markers.
        self.detect_inline_conflicts();
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
        let old_content = match self
            .git_repo
            .as_ref()
            .and_then(|r| r.head_file_content(rel).ok())
        {
            Some(Some(c)) => c,
            _ => String::new(), // New file — empty old side.
        };

        let lines = crate::git::aligned_diff_lines(&old_content, &new_content);
        self.diff_view = Some(DiffView::new(rel_path.to_string(), lines));
        self.mode = Mode::Diff;
    }

    /// Open the 3-panel merge conflict editor for a file with conflicts.
    pub fn open_merge_view(&mut self, rel_path: &str) {
        let workdir = match self.git_repo.as_ref().map(|r| r.workdir().to_path_buf()) {
            Some(wd) => wd,
            None => {
                self.set_status("No git repository");
                return;
            }
        };

        let full_path = workdir.join(rel_path);
        let content = match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(e) => {
                self.set_status(format!("Cannot read file: {e}"));
                return;
            }
        };

        let segments = crate::merge_view::parse_conflict_markers(&content);
        let has_conflicts = segments
            .iter()
            .any(|s| matches!(s, crate::merge_view::MergeSegment::Conflict(_)));

        if !has_conflicts {
            self.set_status("No conflicts found in file");
            return;
        }

        let view = crate::merge_view::MergeConflictView::new(rel_path.to_string(), segments);
        self.set_status(format!(
            "Merge editor: {} conflict(s)",
            view.total_conflicts
        ));
        self.merge_view = Some(view);
        self.mode = Mode::MergeConflict;
    }

    /// Complete the merge: write resolved content and stage the file.
    pub fn complete_merge(&mut self) {
        let (file_path, result) = match &self.merge_view {
            Some(view) if view.all_resolved() => (view.file_path.clone(), view.build_result()),
            Some(view) => {
                let remaining = view.total_conflicts - view.resolved_count;
                self.set_status(format!("{remaining} conflict(s) remaining"));
                return;
            }
            None => return,
        };

        let workdir = match self.git_repo.as_ref().map(|r| r.workdir().to_path_buf()) {
            Some(wd) => wd,
            None => return,
        };

        let full_path = workdir.join(&file_path);
        if let Err(e) = std::fs::write(&full_path, &result) {
            self.set_status(format!("Failed to write: {e}"));
            return;
        }

        if let Some(ref repo) = self.git_repo {
            if let Err(e) = repo.stage_file(&file_path) {
                tracing::warn!("Failed to stage {file_path}: {e}");
            }
        }

        self.merge_view = None;
        self.mode = Mode::Normal;
        self.source_control_focused = true;
        self.refresh_source_control();
        self.set_status(format!("Merge complete: {file_path}"));
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

    /// Write the ACP discovery file so external agents can auto-discover AURA.
    fn write_acp_discovery(port: u16, file_path: Option<&std::path::Path>) {
        let Some(home) = dirs_path() else { return };
        let aura_dir = home.join(".aura");
        if std::fs::create_dir_all(&aura_dir).is_err() {
            return;
        }
        let discovery = serde_json::json!({
            "protocol": "acp",
            "host": "127.0.0.1",
            "port": port,
            "pid": std::process::id(),
            "editor": "AURA",
            "version": env!("CARGO_PKG_VERSION"),
            "file": file_path.map(|p| p.display().to_string()),
            "started": chrono_now(),
            "capabilities": [
                "document/read", "document/edit",
                "cursor/context", "selection/get",
                "diagnostics/get",
                "file/read", "file/list", "file/open",
                "editor/info", "terminal/run", "project/structure"
            ]
        });
        let path = aura_dir.join("acp.json");
        match std::fs::write(
            &path,
            serde_json::to_string_pretty(&discovery).unwrap_or_default(),
        ) {
            Ok(()) => tracing::info!("ACP discovery file written to {}", path.display()),
            Err(e) => tracing::warn!("Failed to write ACP discovery file: {}", e),
        }
    }

    /// Remove the ACP discovery file on shutdown.
    fn remove_acp_discovery() {
        let Some(home) = dirs_path() else { return };
        let path = home.join(".aura").join("acp.json");
        let _ = std::fs::remove_file(&path);
    }

    /// Open the file currently selected in the file-tree sidebar.
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
            Mode::VisualBlock => {
                // Block selection: rectangular area from anchor to cursor.
                // Return the full range covering all lines in the block.
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

    /// Get the visual block selection rectangle (start_row, end_row, start_col, end_col).
    pub fn visual_block_rect(&self) -> Option<(usize, usize, usize, usize)> {
        if self.mode != Mode::VisualBlock {
            return None;
        }
        let tab = self.tab();
        let anchor = tab.visual_anchor?;
        let (start_row, end_row) = if anchor.row <= tab.cursor.row {
            (anchor.row, tab.cursor.row)
        } else {
            (tab.cursor.row, anchor.row)
        };
        let (start_col, end_col) = if anchor.col <= tab.cursor.col {
            (anchor.col, tab.cursor.col)
        } else {
            (tab.cursor.col, anchor.col)
        };
        Some((start_row, end_row, start_col, end_col))
    }

    // ----- Collaboration -----

    /// Start hosting a collaboration session.
    pub fn start_collab_host(&mut self) {
        let name = self.config.collab.display_name.clone();
        let port = self.config.collab.default_port;

        // Collect snapshots for all open files (skip scratch buffers).
        let mut files = Vec::new();
        for tab in self.tabs.tabs_mut() {
            if let Some(path) = tab.canonical_path() {
                let file_id = crate::collab::file_id_from_path(&path);
                let snapshot = tab.buffer.crdt_mut().save_bytes();
                files.push((file_id, path.display().to_string(), snapshot));
            }
        }
        if files.is_empty() {
            // Fallback: share active buffer even if scratch.
            let snapshot = self.tab_mut().buffer.crdt_mut().save_bytes();
            files.push((0, String::new(), snapshot));
        }

        // Build TLS/auth config.
        let tls_auth = crate::collab::TlsAuthConfig {
            use_tls: self.config.collab.use_tls,
            bind_address: self.config.collab.bind_address.clone(),
            auth_token: if self.config.collab.require_auth {
                Some(crate::collab::generate_auth_token())
            } else {
                None
            },
        };

        match crate::collab::CollabSession::host(&name, port, files, &tls_auth) {
            Ok(session) => {
                let port = session.port.unwrap_or(0);
                let token_msg = session
                    .auth_token
                    .as_ref()
                    .map(|t| format!(" | Token: {t}"))
                    .unwrap_or_default();
                self.set_status(format!("Hosting on port {port}{token_msg}"));
                self.collab = Some(session);
            }
            Err(e) => self.set_status(format!("Failed to host: {e}")),
        }
    }

    /// Join an existing collaboration session.
    pub fn join_collab_session(&mut self, addr: &str) {
        self.join_collab_with_token(addr, None);
    }

    /// Join a collaboration session with an optional auth token.
    pub fn join_collab_with_token(&mut self, addr: &str, token: Option<&str>) {
        let name = self.config.collab.display_name.clone();

        let use_tls = self.config.collab.use_tls;
        match crate::collab::CollabSession::join(&name, addr, token, use_tls) {
            Ok(session) => {
                self.collab = Some(session);
                self.set_status(format!("Joining collab session at {addr}"));
            }
            Err(e) => self.set_status(format!("Failed to join: {e}")),
        }
    }

    // ----- Split panes -----

    /// Open a vertical split showing the current tab in both panes.
    pub fn split_vertical(&mut self) {
        self.split_active = true;
        self.split_direction = SplitDirection::Vertical;
        self.split_tab_idx = self.tabs.active_index();
        self.split_focus_secondary = false;
        self.set_status("Vertical split — Ctrl+W to switch panes");
    }

    /// Open a horizontal split showing the current tab in both panes.
    pub fn split_horizontal(&mut self) {
        self.split_active = true;
        self.split_direction = SplitDirection::Horizontal;
        self.split_tab_idx = self.tabs.active_index();
        self.split_focus_secondary = false;
        self.set_status("Horizontal split — Ctrl+W to switch panes");
    }

    /// Close the split pane.
    pub fn split_close(&mut self) {
        self.split_active = false;
        self.split_focus_secondary = false;
    }

    /// Toggle focus between primary and secondary panes.
    pub fn split_toggle_focus(&mut self) {
        if self.split_active {
            self.split_focus_secondary = !self.split_focus_secondary;
        }
    }

    /// Get the tab index of the focused pane.
    pub fn focused_tab_idx(&self) -> usize {
        if self.split_active && self.split_focus_secondary {
            self.split_tab_idx
        } else {
            self.tabs.active_index()
        }
    }

    /// Open the command palette.
    pub fn open_command_palette(&mut self) {
        let commands = crate::command_palette::editor_commands();
        let settings = crate::command_palette::settings_items(&self.config);

        // Collect workspace files.
        let files: Vec<crate::command_palette::PaletteItem> = self
            .file_tree
            .entries
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| crate::command_palette::PaletteItem::File {
                path: e.path.display().to_string(),
            })
            .collect();

        // Add project tasks to the palette.
        let mut task_items: Vec<crate::command_palette::PaletteItem> = self
            .get_tasks()
            .iter()
            .map(
                |(name, task)| crate::command_palette::PaletteItem::Command {
                    id: format!("task {name}"),
                    label: format!("Task: {} — {}", name, task.description),
                    shortcut: String::new(),
                },
            )
            .collect();
        let mut all_commands = commands;
        all_commands.append(&mut task_items);

        self.command_palette.open(all_commands, files, settings);
    }

    /// Execute the currently selected palette item.
    pub fn execute_palette_selection(&mut self) {
        let item = match self.command_palette.selected_item() {
            Some(i) => i.clone(),
            None => return,
        };
        self.command_palette.close();

        match item {
            crate::command_palette::PaletteItem::Command { id, .. } => {
                // Execute as a command-mode command.
                self.mode = Mode::Normal;
                crate::input::execute_command_from_palette(self, &id);
            }
            crate::command_palette::PaletteItem::File { path } => {
                if let Err(e) = self.open_file(std::path::PathBuf::from(&path)) {
                    self.set_status(e);
                }
            }
            crate::command_palette::PaletteItem::Setting { .. } => {
                self.open_settings();
            }
        }
    }

    /// Open the settings modal.
    pub fn open_settings(&mut self) {
        self.settings_modal.open(&self.config);
    }

    /// Close the settings modal and apply changes.
    pub fn close_settings(&mut self) {
        self.settings_modal.apply_to_config(&mut self.config);
        // Apply live settings that need immediate effect.
        self.show_authorship = self.config.editor.show_authorship;
        self.chat_panel.max_context_messages = self.config.conversations.max_context_messages;
        self.settings_modal.close();
    }

    /// Manually compact the conversation database.
    pub fn compact_conversations(&mut self) {
        let store = match &self.conversation_store {
            Some(s) => s,
            None => {
                self.set_status("No conversation store available");
                return;
            }
        };
        let compact_config = aura_core::CompactConfig {
            max_message_age_days: self.config.conversations.max_message_age_days,
            max_messages_per_conversation: self.config.conversations.max_messages_per_conversation,
            max_conversations: self.config.conversations.max_conversations,
            keep_recent_messages: self.config.conversations.keep_recent_messages,
        };
        match store.compact(&compact_config) {
            Ok(stats) => {
                self.set_status(format!(
                    "Compacted: {} messages, {} conversations deleted",
                    stats.messages_deleted, stats.conversations_deleted
                ));
            }
            Err(e) => self.set_status(format!("Compact failed: {e}")),
        }
        // Also trigger AI summarization for eligible conversations.
        self.maybe_summarize_next();
    }

    /// Stop the current collaboration session.
    pub fn stop_collab(&mut self) {
        if let Some(session) = self.collab.take() {
            session.shutdown();
            self.collab_follow_peer = None;
            self.collab_follow_last_applied = None;
            self.collab_sharing_terminal = false;
            self.collab_shared_terminal = None;
            self.viewing_shared_terminal = false;
            self.set_status("Collab session ended");
        }
    }

    /// Start following a peer's viewport by display name.
    pub fn start_follow(&mut self, name: &str) {
        let session = match &self.collab {
            Some(s) => s,
            None => {
                self.set_status("Not in a collab session");
                return;
            }
        };

        // Reject following self.
        if session.local_name.eq_ignore_ascii_case(name) {
            self.set_status("Cannot follow yourself");
            return;
        }

        // Find peer by name (case-insensitive).
        let found = session
            .peers
            .values()
            .find(|p| p.name.eq_ignore_ascii_case(name));
        match found {
            Some(peer) => {
                let peer_id = peer.peer_id;
                let peer_name = peer.name.clone();
                self.collab_follow_peer = Some(peer_id);
                self.collab_follow_last_applied = None;
                self.set_status(format!("Following {peer_name}"));
            }
            None => {
                let names: Vec<String> = session.peers.values().map(|p| p.name.clone()).collect();
                if names.is_empty() {
                    self.set_status("No peers connected");
                } else {
                    self.set_status(format!(
                        "Peer '{}' not found. Available: {}",
                        name,
                        names.join(", ")
                    ));
                }
            }
        }
    }

    /// Stop following a peer.
    pub fn stop_follow(&mut self) {
        if self.collab_follow_peer.is_some() {
            self.collab_follow_peer = None;
            self.collab_follow_last_applied = None;
            self.set_status("Stopped following");
        } else {
            self.set_status("Not following anyone");
        }
    }

    /// If following a peer, sync local viewport to theirs. Auto-breaks on local navigation.
    fn apply_follow_viewport(&mut self) {
        let peer_id = match self.collab_follow_peer {
            Some(id) => id,
            None => return,
        };

        // Detect local scroll that breaks follow mode.
        if let Some((last_sr, last_sc)) = self.collab_follow_last_applied {
            let tab = self.tab();
            if tab.scroll_row != last_sr || tab.scroll_col != last_sc {
                self.collab_follow_peer = None;
                self.collab_follow_last_applied = None;
                self.set_status("Follow ended: local navigation");
                return;
            }
        }

        // Check peer still exists.
        let (awareness_file_id, scroll_row, scroll_col) = {
            let session = match &self.collab {
                Some(s) => s,
                None => {
                    self.collab_follow_peer = None;
                    self.collab_follow_last_applied = None;
                    return;
                }
            };
            let peer = match session.peers.get(&peer_id) {
                Some(p) => p,
                None => {
                    self.collab_follow_peer = None;
                    self.collab_follow_last_applied = None;
                    self.set_status("Follow ended: peer disconnected");
                    return;
                }
            };

            // Get the peer's latest awareness (prefer the one with scroll data).
            let awareness = peer
                .awareness
                .values()
                .filter(|a| a.scroll_row.is_some())
                .max_by_key(|a| a.scroll_row.unwrap_or(0));
            match awareness {
                Some(a) => (a.file_id, a.scroll_row, a.scroll_col),
                None => return,
            }
        };

        // File switching: if the peer is on a different file, switch to it.
        let current_file_id = self.tab().file_id();
        if awareness_file_id != 0 && awareness_file_id != current_file_id {
            if let Some(idx) = self.find_tab_by_file_id(awareness_file_id) {
                self.tabs.switch_to(idx);
            }
        }

        // Apply viewport.
        if let (Some(sr), Some(sc)) = (scroll_row, scroll_col) {
            let tab = self.tab_mut();
            tab.scroll_row = sr;
            tab.scroll_col = sc;
            self.collab_follow_last_applied = Some((sr, sc));
        }
    }

    /// Poll for collaboration events (called from the main event loop).
    fn poll_collab_events(&mut self) {
        let events = match &self.collab {
            Some(session) => session.poll_events(),
            None => return,
        };

        for event in events {
            match event {
                crate::collab::CollabEvent::SyncMessage {
                    peer_id,
                    file_id,
                    data,
                } => {
                    // Decode the automerge sync message and apply it.
                    let msg = match aura_core::sync::SyncMessage::decode(&data) {
                        Ok(m) => m,
                        Err(e) => {
                            tracing::warn!("Failed to decode sync message: {e:?}");
                            continue;
                        }
                    };

                    // Find the tab for this file_id.
                    let tab_idx = self.find_tab_by_file_id(file_id);

                    // Take the sync state out to avoid borrow conflict.
                    let mut sync_state = if let Some(session) = &mut self.collab {
                        if !session.peers.contains_key(&peer_id) {
                            session.add_peer(peer_id, format!("peer-{peer_id}"));
                        }
                        if let Some(peer) = session.peers.get_mut(&peer_id) {
                            std::mem::take(peer.sync_states.entry(file_id).or_default())
                        } else {
                            aura_core::sync::SyncState::new()
                        }
                    } else {
                        continue;
                    };

                    let remote_author =
                        aura_core::AuthorId::peer(format!("peer-{peer_id}"), peer_id);

                    if let Some(idx) = tab_idx {
                        let tab = &mut self.tabs.tabs_mut()[idx];
                        if let Err(e) =
                            tab.buffer
                                .apply_remote_sync(&mut sync_state, msg, &remote_author)
                        {
                            tracing::warn!("Failed to apply sync message: {e}");
                        } else {
                            tab.mark_highlights_dirty();
                        }
                        let reply = self.tabs.tabs_mut()[idx]
                            .buffer
                            .crdt_mut()
                            .generate_sync_message(&mut sync_state);
                        if let Some(session) = &mut self.collab {
                            if let Some(peer) = session.peers.get_mut(&peer_id) {
                                peer.sync_states.insert(file_id, sync_state);
                            }
                            if let Some(reply_msg) = reply {
                                session.broadcast_sync(file_id, reply_msg.encode());
                            }
                        }
                    } else if let Some(session) = &mut self.collab {
                        if let Some(peer) = session.peers.get_mut(&peer_id) {
                            peer.sync_states.insert(file_id, sync_state);
                        }
                    }
                }
                crate::collab::CollabEvent::DocSnapshot {
                    file_id,
                    path,
                    data,
                } => {
                    // Find or create a tab for this file.
                    let tab_idx = if !path.is_empty() {
                        let p = std::path::PathBuf::from(&path);
                        if let Some(idx) = self.tabs.find_by_path(&p) {
                            let tab = &mut self.tabs.tabs_mut()[idx];
                            if let Err(e) = tab.buffer.load_remote_snapshot(&data) {
                                tracing::warn!("Failed to load snapshot for {path}: {e}");
                                continue;
                            }
                            Some(idx)
                        } else {
                            let mut buf = aura_core::Buffer::new();
                            if let Err(e) = buf.load_remote_snapshot(&data) {
                                tracing::warn!("Failed to load snapshot for {path}: {e}");
                                continue;
                            }
                            let tab = crate::tab::EditorTab::new(
                                buf,
                                self.conversation_store.as_ref(),
                                &self.theme,
                            );
                            self.tabs.open(tab);
                            Some(self.tabs.count() - 1)
                        }
                    } else {
                        if let Err(e) = self.tab_mut().buffer.load_remote_snapshot(&data) {
                            tracing::warn!("Failed to load snapshot: {e}");
                            continue;
                        }
                        Some(self.tabs.active_index())
                    };

                    if let Some(idx) = tab_idx {
                        self.tabs.tabs_mut()[idx].mark_highlights_dirty();
                        let tab_fid = self.tabs.tabs()[idx].file_id();
                        let actual_fid = if file_id != 0 { file_id } else { tab_fid };

                        let mut state = aura_core::sync::SyncState::new();
                        let msg = self.tabs.tabs_mut()[idx]
                            .buffer
                            .crdt_mut()
                            .generate_sync_message(&mut state);

                        if let Some(session) = &mut self.collab {
                            if !session.peers.contains_key(&0) {
                                session.add_peer(0, "host".to_string());
                            }
                            if let Some(peer) = session.peers.get_mut(&0) {
                                peer.sync_states.insert(actual_fid, state);
                            }
                            if let Some(m) = msg {
                                session.broadcast_sync(actual_fid, m.encode());
                            }
                        }
                    }
                    self.set_status(if path.is_empty() {
                        "Document synced from host".to_string()
                    } else {
                        format!("Synced: {path}")
                    });
                }
                crate::collab::CollabEvent::FileOpened { file_id: _, path } => {
                    self.set_status(format!("Host opened: {path}"));
                }
                crate::collab::CollabEvent::FileClosed { file_id } => {
                    if let Some(idx) = self.find_tab_by_file_id(file_id) {
                        let name = self.tabs.tabs()[idx].file_name().to_string();
                        if !self.tabs.tabs()[idx].is_modified() {
                            self.tabs.close(idx);
                            self.set_status(format!("Host closed: {name}"));
                        }
                    }
                }
                crate::collab::CollabEvent::PeerJoined { peer_id, name } => {
                    if let Some(session) = &mut self.collab {
                        session.add_peer(peer_id, name.clone());
                    }
                    self.set_status(format!("Peer joined: {name}"));
                }
                crate::collab::CollabEvent::PeerLeft { peer_id } => {
                    let name = self
                        .collab
                        .as_ref()
                        .and_then(|s| s.peers.get(&peer_id))
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| format!("peer-{peer_id}"));
                    if let Some(session) = &mut self.collab {
                        session.remove_peer(peer_id);
                    }
                    // Break follow mode if we were following this peer.
                    if self.collab_follow_peer == Some(peer_id) {
                        self.collab_follow_peer = None;
                        self.collab_follow_last_applied = None;
                    }
                    self.set_status(format!("Peer left: {name}"));
                }
                crate::collab::CollabEvent::Awareness(update) => {
                    if let Some(session) = &mut self.collab {
                        session.update_peer_awareness(update);
                    }
                }
                crate::collab::CollabEvent::TerminalSnapshot { data } => {
                    match serde_json::from_slice::<crate::embedded_terminal::TerminalSnapshot>(
                        &data,
                    ) {
                        Ok(snapshot) => {
                            self.collab_shared_terminal = Some(snapshot);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to decode terminal snapshot: {e}");
                        }
                    }
                }
                crate::collab::CollabEvent::Reconnecting { attempt } => {
                    if let Some(session) = &mut self.collab {
                        session.reconnecting = true;
                        session.reconnect_attempt = attempt;
                    }
                    self.set_status(format!("Collab: reconnecting (attempt {attempt})..."));
                }
                crate::collab::CollabEvent::Reconnected => {
                    if let Some(session) = &mut self.collab {
                        session.reconnecting = false;
                        session.reconnect_attempt = 0;
                    }
                    self.set_status("Collab: reconnected!");
                }
                crate::collab::CollabEvent::Error(msg) => {
                    tracing::warn!("Collab error: {msg}");
                    self.set_status(format!("Collab: {msg}"));
                }
            }
        }

        // Periodically clean up expired disconnected peer states (host only).
        if let Some(session) = &mut self.collab {
            if session.is_host {
                session.cleanup_disconnected_peers();
            }
        }
    }

    /// Broadcast local CRDT changes for the active tab to all peers.
    fn broadcast_collab_sync(&mut self) {
        if self.collab.is_none() {
            return;
        }

        let file_id = self.tab().file_id();
        let peer_ids: Vec<u64> = self
            .collab
            .as_ref()
            .map(|s| s.peers.keys().copied().collect())
            .unwrap_or_default();

        for peer_id in peer_ids {
            let mut sync_state = if let Some(session) = &mut self.collab {
                if let Some(peer) = session.peers.get_mut(&peer_id) {
                    std::mem::take(peer.sync_states.entry(file_id).or_default())
                } else {
                    continue;
                }
            } else {
                continue;
            };

            let msg = self
                .tabs
                .active_mut()
                .buffer
                .crdt_mut()
                .generate_sync_message(&mut sync_state);

            if let Some(session) = &mut self.collab {
                if let Some(peer) = session.peers.get_mut(&peer_id) {
                    peer.sync_states.insert(file_id, sync_state);
                }
                if let Some(m) = msg {
                    session.broadcast_sync(file_id, m.encode());
                }
            }
        }
    }

    /// Find a tab index by file_id.
    fn find_tab_by_file_id(&self, file_id: u64) -> Option<usize> {
        if file_id == 0 {
            return Some(self.tabs.active_index());
        }
        self.tabs
            .tabs()
            .iter()
            .position(|tab| tab.file_id() == file_id)
    }

    /// Send an awareness update if cursor/selection changed (throttled to 50ms).
    fn maybe_send_awareness(&mut self) {
        if self.collab.is_none() {
            return;
        }

        let now = std::time::Instant::now();
        if now.duration_since(self.collab_last_awareness) < Duration::from_millis(50) {
            return;
        }

        let tab = self.tab();
        let cursor = (tab.cursor.row, tab.cursor.col);
        let selection = tab
            .visual_anchor
            .as_ref()
            .map(|anchor| ((anchor.row, anchor.col), (tab.cursor.row, tab.cursor.col)));

        let current_scroll = (tab.scroll_row, tab.scroll_col);

        // Only send if something changed.
        if self.collab_last_cursor == Some(cursor)
            && self.collab_last_selection == selection
            && self.collab_last_scroll == Some(current_scroll)
        {
            return;
        }

        self.collab_last_cursor = Some(cursor);
        self.collab_last_selection = selection;
        self.collab_last_scroll = Some(current_scroll);
        self.collab_last_awareness = now;

        if let Some(session) = &self.collab {
            let file_id = self.tab().file_id();
            let update = crate::collab::AwarenessUpdate {
                peer_id: session.local_peer_id,
                name: session.local_name.clone(),
                file_id,
                cursor: Some(cursor),
                selection,
                scroll_row: Some(current_scroll.0),
                scroll_col: Some(current_scroll.1),
            };
            session.broadcast_awareness(update);
        }
    }

    /// Broadcast terminal screen snapshot to collab peers (host only, throttled).
    fn maybe_broadcast_terminal_snapshot(&mut self) {
        if !self.collab_sharing_terminal || self.collab.is_none() {
            return;
        }
        if let Some(session) = &self.collab {
            if !session.is_host {
                return;
            }
        }

        let now = std::time::Instant::now();
        if now.duration_since(self.collab_last_terminal_broadcast) < Duration::from_millis(150) {
            return;
        }

        let snapshot = self.terminal().terminal_snapshot();

        // Quick change detection via hash.
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            snapshot.cursor_row.hash(&mut hasher);
            snapshot.cursor_col.hash(&mut hasher);
            snapshot.rows.hash(&mut hasher);
            snapshot.cols.hash(&mut hasher);
            if let Some(first_row) = snapshot.cells.first() {
                for cell in first_row {
                    cell.ch.hash(&mut hasher);
                }
            }
            if let Some(last_row) = snapshot.cells.last() {
                for cell in last_row {
                    cell.ch.hash(&mut hasher);
                }
            }
            hasher.finish()
        };

        if hash == self.collab_last_terminal_hash {
            return;
        }

        if let Ok(data) = serde_json::to_vec(&snapshot) {
            if let Some(session) = &self.collab {
                session.broadcast_terminal_snapshot(data);
            }
            self.collab_last_terminal_hash = hash;
            self.collab_last_terminal_broadcast = now;
        }
    }

    /// Get peer awareness states for the currently active file.
    pub fn collab_peer_awareness(&self) -> Vec<&crate::collab::AwarenessUpdate> {
        let file_id = self.tab().file_id();
        match &self.collab {
            Some(session) => session
                .peers
                .values()
                .filter_map(|p| p.awareness.get(&file_id))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Get peer color by peer_id.
    pub fn collab_peer_color(&self, peer_id: u64) -> ratatui::style::Color {
        use aura_core::AuthorColor;
        match AuthorColor::for_peer(peer_id) {
            AuthorColor::Cyan => ratatui::style::Color::Cyan,
            AuthorColor::Magenta => ratatui::style::Color::Magenta,
            AuthorColor::Orange => ratatui::style::Color::Indexed(208),
            AuthorColor::Teal => ratatui::style::Color::Indexed(30),
            AuthorColor::Purple => ratatui::style::Color::Indexed(141),
            AuthorColor::Yellow => ratatui::style::Color::Yellow,
            _ => ratatui::style::Color::Gray,
        }
    }

    /// Get the collab status for UI display.
    pub fn collab_status(&self) -> crate::collab::CollabStatus {
        self.collab
            .as_ref()
            .map(|s| s.status())
            .unwrap_or(crate::collab::CollabStatus::Inactive)
    }

    /// Close the current tab. Returns `true` if the app should quit.
    pub fn close_current_tab(&mut self) -> bool {
        if self.tab().pinned {
            self.set_status("Tab is pinned. Use :unpin first or :tabc! to force close");
            return false;
        }
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

    /// Force-close a tab by index. Returns `true` if the app should quit.
    pub fn close_tab_by_index(&mut self, idx: usize) -> bool {
        if idx >= self.tabs.count() {
            return false;
        }
        // Shutdown LSP for the target tab.
        self.tabs.tabs_mut()[idx].shutdown_lsp();
        if self.tabs.close(idx).is_none() {
            // Last tab — signal quit.
            return true;
        }
        false
    }

    /// Save a tab's buffer by index. Returns Ok(()) on success.
    pub fn save_tab_by_index(&mut self, idx: usize) -> Result<(), String> {
        if idx >= self.tabs.count() {
            return Err("Invalid tab index".to_string());
        }
        self.tabs.tabs_mut()[idx]
            .buffer
            .save()
            .map_err(|e| format!("{}", e))
    }

    /// Handle the user's response to the close-tab confirmation dialog.
    pub fn handle_close_confirm_save(&mut self) {
        if let Some(idx) = self.tab_close_confirm.take() {
            if idx < self.tabs.count() {
                match self.save_tab_by_index(idx) {
                    Ok(_) => {
                        if self.close_tab_by_index(idx) {
                            self.should_quit = true;
                        }
                    }
                    Err(e) => self.set_status(format!("Save failed: {}", e)),
                }
            }
        }
    }

    /// Handle the user choosing to discard changes and close the tab.
    pub fn handle_close_confirm_discard(&mut self) {
        if let Some(idx) = self.tab_close_confirm.take() {
            if self.close_tab_by_index(idx) {
                self.should_quit = true;
            }
        }
    }

    /// Cancel the close-tab confirmation dialog.
    pub fn handle_close_confirm_cancel(&mut self) {
        self.tab_close_confirm = None;
    }

    /// Show the agent diff review: compare file snapshots from agent start with current disk.
    pub fn show_agent_diff(&mut self) {
        // Collect snapshots from the active agent session, or a recently-stopped one.
        let snapshots = if let Some(ref session) = self.agent_mode {
            session.file_snapshots.clone()
        } else {
            // No active session — check if we have leftover diff files.
            if !self.agent_diff_files.is_empty() {
                // Show existing diffs.
                self.agent_diff_idx = 0;
                if let Some((path, old, new)) = self.agent_diff_files.first() {
                    let lines = crate::git::aligned_diff_lines(old, new);
                    if lines.is_empty() {
                        self.set_status("No changes to review");
                    } else {
                        let total = self.agent_diff_files.len();
                        self.diff_view = Some(DiffView::new(format!("{path} [1/{total}]"), lines));
                        self.set_status(format!("Agent diff: {total} file(s) changed"));
                    }
                }
                return;
            }
            self.set_status("No agent session with file snapshots");
            return;
        };

        if snapshots.is_empty() {
            self.set_status("No file snapshots captured — agent has no changes to review");
            return;
        }

        // Compare each snapshot with current disk content.
        let mut diffs: Vec<(String, String, String)> = Vec::new();
        for (path, old_content) in &snapshots {
            let new_content = std::fs::read_to_string(path).unwrap_or_default();
            if *old_content != new_content {
                diffs.push((path.clone(), old_content.clone(), new_content));
            }
        }

        // Also check files_changed that might not have been in snapshots.
        if let Some(ref session) = self.agent_mode {
            for path in &session.files_changed {
                if !snapshots.contains_key(path) {
                    if let Ok(content) = std::fs::read_to_string(path) {
                        diffs.push((path.clone(), String::new(), content));
                    }
                }
            }
        }

        if diffs.is_empty() {
            self.set_status("No changes detected");
            return;
        }

        self.agent_diff_files = diffs;
        self.agent_diff_idx = 0;

        // Show the first file's diff.
        let (path, old, new) = &self.agent_diff_files[0];
        let lines = crate::git::aligned_diff_lines(old, new);
        let total = self.agent_diff_files.len();
        self.diff_view = Some(DiffView::new(format!("{path} [1/{total}]"), lines));
        self.set_status(format!(
            "Agent diff: {total} file(s) changed — use :agent diff to cycle"
        ));
    }

    /// Cycle to the next file in the agent diff review.
    pub fn next_agent_diff(&mut self) {
        if self.agent_diff_files.is_empty() {
            return;
        }
        self.agent_diff_idx = (self.agent_diff_idx + 1) % self.agent_diff_files.len();
        let (path, old, new) = &self.agent_diff_files[self.agent_diff_idx];
        let lines = crate::git::aligned_diff_lines(old, new);
        let idx = self.agent_diff_idx + 1;
        let total = self.agent_diff_files.len();
        self.diff_view = Some(DiffView::new(format!("{path} [{idx}/{total}]"), lines));
    }

    /// Revert the current file in agent diff review to its snapshot state.
    pub fn revert_agent_diff_file(&mut self) {
        if self.agent_diff_files.is_empty() {
            return;
        }
        let (path, old_content, _) = &self.agent_diff_files[self.agent_diff_idx];
        let path = path.clone();
        let old_content = old_content.clone();
        if std::fs::write(&path, &old_content).is_ok() {
            self.set_status(format!("Reverted: {path}"));
            self.agent_diff_files.remove(self.agent_diff_idx);
            if self.agent_diff_files.is_empty() {
                self.diff_view = None;
                self.set_status("All agent changes reviewed");
            } else {
                self.agent_diff_idx = self
                    .agent_diff_idx
                    .min(self.agent_diff_files.len().saturating_sub(1));
                self.next_agent_diff();
            }
        } else {
            self.set_status(format!("Failed to revert: {path}"));
        }
    }

    /// Set the yank register and optionally sync to system clipboard.
    pub fn set_yank(&mut self, text: String) {
        if self.config.editor.clipboard_sync && !text.is_empty() {
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                let _ = clipboard.set_text(&text);
            }
        }
        self.register = Some(text);
    }

    /// Trim trailing whitespace from every line in the current buffer.
    pub fn trim_trailing_whitespace(&mut self) {
        let line_count = self.tab().buffer.line_count();
        // Process in reverse to avoid index shifts.
        for line_idx in (0..line_count).rev() {
            let line = self.tab().buffer.rope().line(line_idx).to_string();
            let trimmed = line.trim_end_matches([' ', '\t']);
            if trimmed.len() < line.trim_end_matches('\n').len() {
                let line_start = self.tab().buffer.rope().line_to_char(line_idx);
                let trim_start = line_start + trimmed.len();
                let trim_end = line_start + line.trim_end_matches('\n').len();
                if trim_end > trim_start {
                    self.tab_mut()
                        .buffer
                        .delete(trim_start, trim_end, aura_core::AuthorId::Human);
                }
            }
        }
    }

    /// Ensure the buffer ends with a newline character.
    pub fn ensure_final_newline(&mut self) {
        let len = self.tab().buffer.rope().len_chars();
        if len == 0 {
            return;
        }
        let last_char = self.tab().buffer.rope().char(len - 1);
        if last_char != '\n' {
            self.tab_mut()
                .buffer
                .insert(len, "\n", aura_core::AuthorId::Human);
        }
    }

    /// Run the appropriate language formatter on the current buffer.
    ///
    /// Detects the formatter from the file extension and runs it in-place.
    /// The buffer content is replaced with the formatted output.
    pub fn format_current_buffer(&mut self) {
        let path = match self.tab().buffer.file_path() {
            Some(p) => p.to_path_buf(),
            None => return, // scratch buffer, nothing to format
        };

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Map extensions to formatter commands.
        let formatter: Option<(&str, Vec<&str>)> = match ext.as_str() {
            "rs" => Some(("rustfmt", vec!["--edition", "2021"])),
            "py" => Some(("black", vec!["-q", "-"])),
            "js" | "jsx" | "ts" | "tsx" | "css" | "json" | "md" | "html" | "yaml" | "yml" => {
                Some(("prettier", vec!["--write"]))
            }
            "go" => Some(("gofmt", vec!["-w"])),
            "zig" => Some(("zig", vec!["fmt"])),
            "c" | "cpp" | "cc" | "h" | "hpp" => Some(("clang-format", vec!["-i"])),
            "lua" => Some(("stylua", vec![])),
            "sh" | "bash" => Some(("shfmt", vec!["-w"])),
            _ => None,
        };

        let (cmd, args) = match formatter {
            Some(f) => f,
            None => return, // no formatter for this file type
        };

        // Save the buffer to disk first so the formatter can read it.
        if self.tab_mut().buffer.save().is_err() {
            return;
        }

        let path_str = path.display().to_string();

        // Special case: black reads from stdin with `-` flag, but we want file mode.
        let mut full_args: Vec<&str> = args;
        // For formatters that take the file as an argument (most of them).
        if cmd != "black" {
            full_args.push(&path_str);
        }

        // For black, use the file path instead of stdin.
        let final_args = if cmd == "black" {
            vec!["-q", &path_str]
        } else {
            full_args
        };

        let result = std::process::Command::new(cmd).args(&final_args).output();

        match result {
            Ok(output) if output.status.success() => {
                // Reload the buffer from disk (formatter wrote in-place).
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let tab = self.tab_mut();
                    let current = tab.buffer.rope().to_string();
                    if content != current {
                        // Replace buffer content with formatted version.
                        let len = tab.buffer.rope().len_chars();
                        tab.buffer.delete(0, len, aura_core::AuthorId::Human);
                        tab.buffer.insert(0, &content, aura_core::AuthorId::Human);
                    }
                }
            }
            Ok(_output) => {
                // Formatter failed silently — don't block save.
                tracing::debug!("Formatter {cmd} returned non-zero exit code");
            }
            Err(_) => {
                // Formatter not installed — silently skip.
                tracing::debug!("Formatter {cmd} not found, skipping format on save");
            }
        }
    }
}
