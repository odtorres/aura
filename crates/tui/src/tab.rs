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
use std::path::PathBuf;
use std::time::{Duration, Instant};

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
    /// Pending LSP client spawn. `Some` while the language server is being
    /// started on a background thread; resolves into `lsp_client` once the
    /// child process exists and its reader/writer threads are up. This lets
    /// opening a tab for a slow-starting server (e.g. rust-analyzer, jdtls)
    /// return control to the UI immediately.
    pub lsp_pending: Option<std::sync::mpsc::Receiver<anyhow::Result<LspClient>>>,
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
    /// Breakpoints set on this file (0-indexed line numbers → optional condition).
    pub breakpoints: std::collections::BTreeMap<usize, Option<String>>,
    /// Vim marks: maps register char (a-z) → cursor position.
    pub marks: std::collections::HashMap<char, Cursor>,
    /// Folded line ranges: maps fold start line → fold end line (exclusive).
    pub folded_ranges: std::collections::HashMap<usize, usize>,
    /// Cached foldable ranges from tree-sitter (start_line → end_line).
    pub foldable_ranges: std::collections::HashMap<usize, usize>,
    /// Whether this tab is pinned (protected from accidental close).
    pub pinned: bool,
    /// Cached inlay hints from LSP for the current viewport.
    pub inlay_hints: Vec<crate::lsp::InlayHint>,
    /// Cached semantic tokens from LSP.
    pub semantic_tokens: Vec<crate::lsp::SemanticToken>,
    /// Cached code lens from LSP.
    pub code_lens: Vec<crate::lsp::CodeLensItem>,
    /// Active signature help (displayed as popup near cursor).
    pub signature_help: Option<crate::lsp::SignatureHelpResult>,
    /// Discovered test function lines (0-indexed).
    pub test_lines: Vec<(usize, String)>,
    /// Side-by-side diff view attached to this tab (None for normal editing).
    pub diff: Option<crate::diff_view::DiffView>,
    /// Cached bracket depths for rainbow colouring, keyed by line index.
    /// `None` means the cache is stale and must be recomputed on next access.
    /// Rebuilt at most once per edit instead of once per frame.
    pub bracket_cache: Option<std::collections::HashMap<usize, Vec<(usize, u8)>>>,
    /// Cache of per-line minimap text, sized to `buffer.line_count()`.
    /// Each entry is the line trimmed of trailing newlines. Refreshed
    /// lazily when `bracket_cache_rev` no longer matches the buffer's
    /// current `revision`. Avoids allocating a fresh `Vec<String>` of
    /// every line on every frame the minimap is visible — that work
    /// scales with file length and used to dominate render time on
    /// 10k-line files.
    pub minimap_lines_cache: Vec<String>,
    /// Buffer revision the `minimap_lines_cache` was built against.
    pub minimap_cache_rev: u64,
    /// When the CRDT history was last compacted during idle time. Used to
    /// rate-limit the idle-compaction check in the main event loop — we never
    /// compact more often than the idle threshold, even if the user alternates
    /// between editing and idling rapidly.
    pub last_idle_compact: Instant,
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
            .and_then(Language::from_extension)
            .or_else(|| {
                buffer
                    .file_path()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .and_then(Language::from_filename)
            });
        let mut highlighter = language.and_then(SyntaxHighlighter::new);
        let semantic_indexer = language.and_then(SemanticIndexer::new);

        let conversation_lines = conversation_store
            .and_then(|store| {
                let fp = buffer.file_path()?.display().to_string();
                store.lines_with_conversations(&fp).ok()
            })
            .unwrap_or_default();

        // Try to start a language server — on a background thread so slow
        // servers (rust-analyzer, jdtls) don't block tab-open.
        let lsp_pending = buffer
            .file_path()
            .and_then(|p| p.extension())
            .and_then(|ext| ext.to_str())
            .and_then(lsp::detect_server)
            .and_then(|config| {
                let file_path = buffer.file_path()?.to_path_buf();
                let workspace_root = file_path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| file_path.clone());
                let content = buffer.rope().to_string();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let result = LspClient::start(&config, &workspace_root, &file_path, &content);
                    // Receiver may be gone if the tab was closed before the
                    // spawn finished — that's fine, drop the result.
                    let _ = tx.send(result);
                });
                Some(rx)
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
            lsp_client: None,
            lsp_pending,
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
            breakpoints: std::collections::BTreeMap::new(),
            marks: std::collections::HashMap::new(),
            folded_ranges: std::collections::HashMap::new(),
            foldable_ranges: std::collections::HashMap::new(),
            pinned: false,
            inlay_hints: Vec::new(),
            semantic_tokens: Vec::new(),
            code_lens: Vec::new(),
            signature_help: None,
            test_lines: Vec::new(),
            diff: None,
            bracket_cache: None,
            minimap_lines_cache: Vec::new(),
            // Sentinel so the very first read forces a rebuild — the
            // buffer starts at revision 0, but we want the cache to
            // populate on first frame even if the buffer is unedited.
            minimap_cache_rev: u64::MAX,
            last_idle_compact: Instant::now(),
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

    /// Return a display title: filename + `*` when modified, `[diff]` for diff tabs.
    pub fn title(&self) -> String {
        if let Some(ref dv) = self.diff {
            return format!("[diff] {}", dv.file_path);
        }
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
        self.bracket_cache = None;
    }

    /// Return the per-line minimap text cache, rebuilding only when the
    /// buffer revision has changed. The minimap renderer used to walk the
    /// rope and allocate a fresh `Vec<String>` of every line on every
    /// frame — this caches that work behind `Buffer::revision`.
    pub fn minimap_lines(&mut self) -> &[String] {
        let rev = self.buffer.revision();
        if self.minimap_cache_rev != rev {
            let total = self.buffer.line_count();
            // Reuse capacity across rebuilds to avoid Vec churn on edits.
            self.minimap_lines_cache.clear();
            self.minimap_lines_cache.reserve(total);
            for i in 0..total {
                let s = self
                    .buffer
                    .rope()
                    .get_line(i)
                    .map(|l| l.to_string().trim_end_matches('\n').to_string())
                    .unwrap_or_default();
                self.minimap_lines_cache.push(s);
            }
            self.minimap_cache_rev = rev;
        }
        &self.minimap_lines_cache
    }

    /// Return the bracket-depth cache, rebuilding it from the buffer if stale.
    ///
    /// The cache is keyed by line index and lists `(column, depth_mod_6)` pairs
    /// for every `(`/`)`/`{`/`}`/`[`/`]` on that line. Depth is cumulative from
    /// the start of the file so rainbow colouring stays consistent across
    /// viewport scrolls.
    pub fn bracket_depths(&mut self) -> &std::collections::HashMap<usize, Vec<(usize, u8)>> {
        if self.bracket_cache.is_none() {
            let mut depths: std::collections::HashMap<usize, Vec<(usize, u8)>> =
                std::collections::HashMap::new();
            let mut depth: i32 = 0;
            let total_lines = self.buffer.line_count();
            for line_idx in 0..total_lines {
                if let Some(rope_line) = self.buffer.line(line_idx) {
                    let mut line_brackets = Vec::new();
                    for (col, ch) in rope_line.chars().enumerate() {
                        if ch == '\n' || ch == '\r' {
                            continue;
                        }
                        match ch {
                            '(' | '{' | '[' => {
                                line_brackets.push((col, (depth.max(0) as u8) % 6));
                                depth += 1;
                            }
                            ')' | '}' | ']' => {
                                depth -= 1;
                                line_brackets.push((col, (depth.max(0) as u8) % 6));
                            }
                            _ => {}
                        }
                    }
                    if !line_brackets.is_empty() {
                        depths.insert(line_idx, line_brackets);
                    }
                }
            }
            self.bracket_cache = Some(depths);
        }
        self.bracket_cache.as_ref().expect("just populated")
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

    /// Compact the CRDT history if the tab has been idle long enough.
    ///
    /// Call every event-loop tick. It's cheap if nothing needs to happen: both
    /// thresholds (idle since last edit, idle since last compact) have to pass
    /// before any real work runs. CRDT history grows during long editing
    /// sessions and is otherwise only compacted on save.
    pub fn maybe_idle_compact_crdt(&mut self) {
        const IDLE_THRESHOLD: Duration = Duration::from_secs(30);
        if self.lsp_last_change.elapsed() >= IDLE_THRESHOLD
            && self.last_idle_compact.elapsed() >= IDLE_THRESHOLD
        {
            self.buffer.crdt_mut().compact();
            self.last_idle_compact = Instant::now();
        }
    }

    /// Send a didChange notification with the current buffer content.
    pub fn send_lsp_did_change(&mut self) {
        if let Some(client) = &mut self.lsp_client {
            let text = self.buffer.rope().to_string();
            client.did_change(&text);
        }
        self.lsp_change_pending = false;
    }

    /// Promote a pending-startup LSP client to the active slot once the
    /// background spawn has completed. No-op when there's nothing pending or
    /// the spawn hasn't finished yet. Errors from the spawn are logged and
    /// the tab continues without an LSP.
    pub fn poll_lsp_startup(&mut self) {
        let Some(rx) = self.lsp_pending.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(client)) => {
                self.lsp_client = Some(client);
                self.lsp_pending = None;
            }
            Ok(Err(e)) => {
                tracing::warn!("LSP startup failed: {e}");
                self.lsp_pending = None;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.lsp_pending = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                // Still starting; check again next tick.
            }
        }
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

    /// Move the active tab to a new position.
    pub fn move_tab(&mut self, new_idx: usize) {
        let idx = self.active;
        let new_idx = new_idx.min(self.tabs.len().saturating_sub(1));
        if idx == new_idx || self.tabs.len() <= 1 {
            return;
        }
        let tab = self.tabs.remove(idx);
        self.tabs.insert(new_idx, tab);
        self.active = new_idx;
    }

    /// Move a tab from one index to another (for drag-to-reorder).
    pub fn move_tab_to(&mut self, from: usize, to: usize) {
        let to = to.min(self.tabs.len().saturating_sub(1));
        if from == to || from >= self.tabs.len() {
            return;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        // Update active index to follow the moved tab if it was active.
        if self.active == from {
            self.active = to;
        } else if from < self.active && to >= self.active {
            self.active = self.active.saturating_sub(1);
        } else if from > self.active && to <= self.active {
            self.active = (self.active + 1).min(self.tabs.len().saturating_sub(1));
        }
    }

    /// Move the active tab one position to the left.
    pub fn move_tab_left(&mut self) {
        if self.active > 0 {
            self.move_tab(self.active - 1);
        }
    }

    /// Move the active tab one position to the right.
    pub fn move_tab_right(&mut self) {
        if self.active + 1 < self.tabs.len() {
            self.move_tab(self.active + 1);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::theme_dark;

    fn make_tab() -> EditorTab {
        // Buffer::new() has no file path → LSP startup is skipped, semantic
        // indexer is None (no language detected), highlighter is None. The
        // tab is cheap to construct and pure for state-machine testing.
        EditorTab::new(Buffer::new(), None, &theme_dark())
    }

    fn make_mgr_with(n: usize) -> TabManager {
        let mut m = TabManager::new(make_tab());
        for _ in 1..n {
            m.open(make_tab());
        }
        m
    }

    #[test]
    fn new_starts_with_one_active_tab() {
        let m = TabManager::new(make_tab());
        assert_eq!(m.count(), 1);
        assert_eq!(m.active_index(), 0);
    }

    #[test]
    fn open_appends_and_activates() {
        let mut m = TabManager::new(make_tab());
        m.open(make_tab());
        assert_eq!(m.count(), 2);
        assert_eq!(m.active_index(), 1);
    }

    #[test]
    fn next_wraps_around() {
        let mut m = make_mgr_with(3);
        m.switch_to(0);
        m.next();
        assert_eq!(m.active_index(), 1);
        m.next();
        assert_eq!(m.active_index(), 2);
        m.next();
        assert_eq!(m.active_index(), 0);
    }

    #[test]
    fn prev_wraps_around() {
        let mut m = make_mgr_with(3);
        m.switch_to(0);
        m.prev();
        assert_eq!(m.active_index(), 2);
        m.prev();
        assert_eq!(m.active_index(), 1);
    }

    #[test]
    fn next_and_prev_are_no_ops_with_single_tab() {
        let mut m = TabManager::new(make_tab());
        m.next();
        assert_eq!(m.active_index(), 0);
        m.prev();
        assert_eq!(m.active_index(), 0);
    }

    #[test]
    fn switch_to_out_of_bounds_is_ignored() {
        let mut m = make_mgr_with(2);
        m.switch_to(99);
        assert_eq!(m.active_index(), 1);
    }

    #[test]
    fn close_returns_none_for_last_tab() {
        let mut m = TabManager::new(make_tab());
        assert!(m.close(0).is_none());
        assert_eq!(m.count(), 1);
    }

    #[test]
    fn close_removes_and_keeps_active_in_bounds() {
        let mut m = make_mgr_with(3);
        // active = 2 (last). Close index 0 → active should shift to 1.
        m.close(0);
        assert_eq!(m.count(), 2);
        assert_eq!(m.active_index(), 1);
    }

    #[test]
    fn close_active_drops_active_and_clamps() {
        let mut m = make_mgr_with(3);
        // active = 2. close_active removes it → active should be 1 (the new last).
        m.close_active();
        assert_eq!(m.count(), 2);
        assert_eq!(m.active_index(), 1);
    }

    #[test]
    fn close_left_of_active_decrements_active() {
        let mut m = make_mgr_with(3);
        m.switch_to(2);
        m.close(0);
        assert_eq!(m.active_index(), 1, "active should follow its content");
    }

    #[test]
    fn close_right_of_active_keeps_active_index() {
        let mut m = make_mgr_with(3);
        m.switch_to(0);
        m.close(2);
        assert_eq!(m.active_index(), 0);
    }

    #[test]
    fn move_tab_to_beginning() {
        let mut m = make_mgr_with(3);
        m.switch_to(2);
        m.move_tab(0);
        assert_eq!(m.active_index(), 0, "active follows the moved tab");
    }

    #[test]
    fn move_tab_no_op_when_same_index() {
        let mut m = make_mgr_with(3);
        m.switch_to(1);
        m.move_tab(1);
        assert_eq!(m.active_index(), 1);
        assert_eq!(m.count(), 3);
    }

    #[test]
    fn move_tab_left_at_position_zero_is_no_op() {
        let mut m = make_mgr_with(3);
        m.switch_to(0);
        m.move_tab_left();
        assert_eq!(m.active_index(), 0);
    }

    #[test]
    fn move_tab_right_at_last_position_is_no_op() {
        let mut m = make_mgr_with(3);
        m.switch_to(2);
        m.move_tab_right();
        assert_eq!(m.active_index(), 2);
    }

    #[test]
    fn move_tab_to_drag_active_forward() {
        // Start: tabs [0, 1, 2], active = 0. Drag 0 → 2. Active should be 2.
        let mut m = make_mgr_with(3);
        m.switch_to(0);
        m.move_tab_to(0, 2);
        assert_eq!(m.active_index(), 2);
    }

    #[test]
    fn move_tab_to_drag_passing_active_shifts_active() {
        // Start: tabs [0, 1, 2], active = 1.
        // Drag tab from 0 to 2 (right of active) — active was at 1 with
        // tab[0] to its left; after removal active item is at 0, then
        // insert at 2 means active is unchanged at 0.
        let mut m = make_mgr_with(3);
        m.switch_to(1);
        m.move_tab_to(0, 2);
        // The tab originally at position 1 (active) is now at position 0.
        assert_eq!(m.active_index(), 0);
    }

    #[test]
    fn find_by_path_returns_none_for_unsaved_buffers() {
        // None of the tabs have a file path, so no canonical path matches.
        let m = make_mgr_with(3);
        let p = std::path::Path::new("/nonexistent/file.rs");
        assert_eq!(m.find_by_path(p), None);
    }

    #[test]
    fn editor_tab_new_buffer_is_unmodified() {
        let tab = make_tab();
        assert!(!tab.is_modified());
    }

    #[test]
    fn editor_tab_title_for_new_buffer_is_untitled() {
        let tab = make_tab();
        // Untitled buffer → title is "[No Name]" or similar non-empty placeholder.
        assert!(!tab.title().is_empty());
    }
}
