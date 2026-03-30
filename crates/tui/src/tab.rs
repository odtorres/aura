//! Tab management for multi-buffer editing.
//!
//! Each [`EditorTab`] holds per-buffer state (buffer, cursor, scroll position,
//! highlighting, LSP client, etc.).  [`TabManager`] manages the collection of
//! open tabs and provides navigation helpers.

use crate::config::Theme;
use crate::highlight::{HighlightedLine, Language, SyntaxHighlighter};
use crate::lsp::{self, Diagnostic, LspClient, LspEvent};
use crate::semantic_index::SemanticIndexer;
use aura_core::conversation::ConversationStore;
use aura_core::{Buffer, Cursor};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Instant;

/// Detected indentation style for a buffer.
#[derive(Debug, Clone, Copy)]
pub enum IndentStyle {
    /// Tab characters for indentation.
    Tabs,
    /// Spaces for indentation, with the given width.
    Spaces(u8),
}

impl IndentStyle {
    /// Return the string representation of one indentation level.
    pub fn unit(&self) -> String {
        match self {
            IndentStyle::Tabs => "\t".to_string(),
            IndentStyle::Spaces(w) => " ".repeat(*w as usize),
        }
    }
}

/// Detect the indent style of a buffer by scanning the first 100 non-empty lines.
///
/// Returns `Spaces(4)` as default if the buffer is empty or ambiguous.
pub fn detect_indent_style(buffer: &Buffer) -> IndentStyle {
    let mut tab_count = 0u32;
    let mut space_count = 0u32;
    let mut space_widths = [0u32; 9]; // index 0 unused, 1..=8

    let line_limit = buffer.line_count().min(100);
    let mut prev_indent = 0usize;

    for i in 0..line_limit {
        if let Some(line) = buffer.line_text(i) {
            let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
            if trimmed.is_empty() {
                continue;
            }
            let leading: String = trimmed
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            if leading.is_empty() {
                prev_indent = 0;
                continue;
            }
            if leading.starts_with('\t') {
                tab_count += 1;
            } else {
                space_count += 1;
                let indent = leading.len();
                let delta = indent.saturating_sub(prev_indent);
                if (1..=8).contains(&delta) {
                    space_widths[delta] += 1;
                }
                prev_indent = indent;
            }
        }
    }

    if tab_count > space_count {
        return IndentStyle::Tabs;
    }

    // Find the most common space indent width.
    let best_width = space_widths[1..]
        .iter()
        .enumerate()
        .max_by_key(|(_, &count)| count)
        .map(|(idx, _)| idx + 1)
        .unwrap_or(4);

    IndentStyle::Spaces(best_width as u8)
}

/// Per-buffer state held by a single editor tab.
pub struct EditorTab {
    /// The text buffer.
    pub buffer: Buffer,
    /// Cursor position within the buffer.
    pub cursor: Cursor,
    /// Viewport scroll row offset.
    pub scroll_row: usize,
    /// Viewport scroll column offset.
    pub scroll_col: usize,
    /// Anchor position for visual mode selection.
    pub visual_anchor: Option<Cursor>,
    /// Secondary cursors for multi-cursor editing.
    pub secondary_cursors: Vec<Cursor>,
    /// Snippet engine for Tab-triggered code expansion.
    pub snippet_engine: crate::snippets::SnippetEngine,
    /// Syntax highlighter (None if language not supported).
    pub highlighter: Option<SyntaxHighlighter>,
    /// Cached per-line highlight colours. Regenerated on edits.
    pub highlight_lines: Vec<HighlightedLine>,
    /// Whether highlights are stale and need refreshing.
    pub highlights_dirty: bool,
    /// Active LSP client (None if no server available).
    pub lsp_client: Option<LspClient>,
    /// Current diagnostics for the open file.
    pub diagnostics: Vec<Diagnostic>,
    /// Hover information to display as a popup.
    pub hover_info: Option<String>,
    /// Whether the buffer changed since last didChange notification.
    pub lsp_change_pending: bool,
    /// When the last buffer edit occurred (for debouncing didChange).
    pub lsp_last_change: Instant,
    /// Semantic indexer for code structure analysis.
    pub semantic_indexer: Option<SemanticIndexer>,
    /// Detected language for the current file.
    pub language: Option<Language>,
    /// Whether the semantic index needs refreshing.
    pub semantic_dirty: bool,
    /// Cached semantic context for the symbol at cursor.
    pub semantic_info: Option<String>,
    /// Cached line ranges that have conversation history.
    pub conversation_lines: Vec<(usize, usize)>,
    /// Detected indent style for this buffer.
    pub indent_style: IndentStyle,
    /// Breakpoints set on this file (0-indexed line numbers).
    pub breakpoints: BTreeSet<usize>,
    /// Vim marks: maps register char (a-z) → cursor position.
    pub marks: std::collections::HashMap<char, Cursor>,
    /// Folded line ranges: maps fold start line → fold end line (exclusive).
    pub folded_ranges: std::collections::HashMap<usize, usize>,
    /// Cached foldable ranges from tree-sitter (start_line → end_line).
    pub foldable_ranges: std::collections::HashMap<usize, usize>,
}

impl EditorTab {
    /// Create a new tab from a buffer, optionally starting LSP and highlighter.
    pub fn new(
        buffer: Buffer,
        conversation_store: Option<&ConversationStore>,
        theme: &Theme,
    ) -> Self {
        // Detect language from file extension and set up highlighter.
        let language = buffer
            .file_path()
            .and_then(|p| p.extension())
            .and_then(|ext| ext.to_str())
            .and_then(Language::from_extension);
        let mut highlighter = language.and_then(SyntaxHighlighter::new);
        let semantic_indexer = language.and_then(SemanticIndexer::new);

        let conversation_lines = conversation_store
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

        // Generate initial highlights.
        let highlight_lines = if let Some(hl) = &mut highlighter {
            let source = buffer.rope().to_string();
            hl.highlight(&source, Some(theme))
        } else {
            Vec::new()
        };

        let indent_style = detect_indent_style(&buffer);

        let mut tab = Self {
            buffer,
            cursor: Cursor::origin(),
            scroll_row: 0,
            scroll_col: 0,
            visual_anchor: None,
            secondary_cursors: Vec::new(),
            snippet_engine: crate::snippets::SnippetEngine::new(),
            highlighter,
            highlight_lines,
            highlights_dirty: false,
            lsp_client,
            diagnostics: Vec::new(),
            hover_info: None,
            lsp_change_pending: false,
            lsp_last_change: Instant::now(),
            semantic_indexer,
            language,
            semantic_dirty: true,
            semantic_info: None,
            conversation_lines,
            indent_style,
            breakpoints: BTreeSet::new(),
            marks: std::collections::HashMap::new(),
            folded_ranges: std::collections::HashMap::new(),
            foldable_ranges: std::collections::HashMap::new(),
        };
        tab.refresh_semantic_index();
        tab
    }

    /// Return the file basename, or `"[scratch]"` for unsaved buffers.
    pub fn file_name(&self) -> &str {
        self.buffer
            .file_path()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("[scratch]")
    }

    /// Return a display title: filename + `*` when modified.
    pub fn title(&self) -> String {
        let name = self.file_name();
        if self.buffer.is_modified() {
            format!("{} *", name)
        } else {
            name.to_string()
        }
    }

    /// Whether the buffer has unsaved changes.
    pub fn is_modified(&self) -> bool {
        self.buffer.is_modified()
    }

    /// Mark syntax highlights, semantic index, and LSP as stale.
    pub fn mark_highlights_dirty(&mut self) {
        self.highlights_dirty = true;
        self.semantic_dirty = true;
        self.lsp_change_pending = true;
        self.lsp_last_change = Instant::now();
    }

    /// Regenerate syntax highlights from the current buffer content.
    pub fn refresh_highlights(&mut self, theme: &Theme) {
        if let Some(hl) = &mut self.highlighter {
            let source = self.buffer.rope().to_string();
            self.highlight_lines = hl.highlight(&source, Some(theme));
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

    /// Send a didChange notification with the current buffer content.
    pub fn send_lsp_did_change(&mut self) {
        if let Some(client) = &mut self.lsp_client {
            let text = self.buffer.rope().to_string();
            client.did_change(&text);
        }
        self.lsp_change_pending = false;
    }

    /// Poll the LSP client for events, returning them.
    pub fn poll_lsp_events(&mut self) -> Vec<LspEvent> {
        match &self.lsp_client {
            Some(client) => client.poll_events(),
            None => Vec::new(),
        }
    }

    /// Shutdown the LSP client.
    pub fn shutdown_lsp(&mut self) {
        if let Some(mut client) = self.lsp_client.take() {
            client.shutdown();
        }
    }

    /// Get the canonical file path for dedup comparison.
    pub fn canonical_path(&self) -> Option<PathBuf> {
        self.buffer
            .file_path()
            .and_then(|p| std::fs::canonicalize(p).ok())
    }

    /// Get the file identifier for collaborative editing.
    ///
    /// Returns a deterministic u64 hash of the canonical path, or 0 for
    /// scratch buffers (which are excluded from multi-file collab).
    pub fn file_id(&self) -> u64 {
        self.canonical_path()
            .map(|p| crate::collab::file_id_from_path(&p))
            .unwrap_or(0)
    }
}

/// Manages a collection of open editor tabs.
pub struct TabManager {
    /// The list of open tabs.
    tabs: Vec<EditorTab>,
    /// Index of the currently active tab.
    active: usize,
}

impl TabManager {
    /// Create a new tab manager with a single initial tab.
    pub fn new(tab: EditorTab) -> Self {
        Self {
            tabs: vec![tab],
            active: 0,
        }
    }

    /// Get an immutable reference to the active tab.
    pub fn active(&self) -> &EditorTab {
        &self.tabs[self.active]
    }

    /// Get a mutable reference to the active tab.
    pub fn active_mut(&mut self) -> &mut EditorTab {
        &mut self.tabs[self.active]
    }

    /// Get the index of the active tab.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Get the number of open tabs.
    pub fn count(&self) -> usize {
        self.tabs.len()
    }

    /// Get an iterator over all tabs.
    pub fn tabs(&self) -> &[EditorTab] {
        &self.tabs
    }

    /// Get a mutable iterator over all tabs.
    pub fn tabs_mut(&mut self) -> &mut [EditorTab] {
        &mut self.tabs
    }

    /// Switch to the next tab (wraps around).
    pub fn next(&mut self) {
        if self.tabs.len() > 1 {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }

    /// Switch to the previous tab (wraps around).
    pub fn prev(&mut self) {
        if self.tabs.len() > 1 {
            self.active = if self.active == 0 {
                self.tabs.len() - 1
            } else {
                self.active - 1
            };
        }
    }

    /// Switch to a tab by index (1-based for user-facing commands).
    pub fn switch_to(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.active = idx;
        }
    }

    /// Open a new tab and make it active.
    pub fn open(&mut self, tab: EditorTab) {
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
    }

    /// Open a file in a new tab, or switch to it if already open.
    /// Returns `true` if a new tab was opened, `false` if switched to existing.
    pub fn open_or_switch<F>(&mut self, path: &std::path::Path, factory: F) -> Result<bool, String>
    where
        F: FnOnce() -> Result<EditorTab, String>,
    {
        // Check for existing tab with same canonical path.
        if let Ok(canonical) = std::fs::canonicalize(path) {
            if let Some(idx) = self.find_by_path(&canonical) {
                self.active = idx;
                return Ok(false);
            }
        }
        let tab = factory()?;
        self.open(tab);
        Ok(true)
    }

    /// Close a tab by index. Returns the closed tab, or None if it's the last tab.
    pub fn close(&mut self, idx: usize) -> Option<EditorTab> {
        if self.tabs.len() <= 1 || idx >= self.tabs.len() {
            return None;
        }
        let tab = self.tabs.remove(idx);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        } else if self.active > idx {
            self.active = self.active.saturating_sub(1);
        }
        Some(tab)
    }

    /// Close the active tab. Returns the closed tab, or None if it's the last tab.
    pub fn close_active(&mut self) -> Option<EditorTab> {
        let idx = self.active;
        self.close(idx)
    }

    /// Find a tab by canonical file path.
    pub fn find_by_path(&self, canonical: &std::path::Path) -> Option<usize> {
        self.tabs.iter().position(|tab| {
            tab.canonical_path()
                .map(|p| p == canonical)
                .unwrap_or(false)
        })
    }
}
