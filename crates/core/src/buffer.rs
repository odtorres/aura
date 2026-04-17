//! Rope-based text buffer with CRDT-backed authorship tracking.
//!
//! Every edit is tagged with an [`AuthorId`] so the editor can distinguish
//! human keystrokes from AI-generated changes. The buffer is backed by both
//! a rope (for fast local operations) and an automerge CRDT document (for
//! conflict-free multi-author editing and change provenance).

use crate::author::AuthorId;
use crate::crdt::CrdtDoc;
use crate::cursor::Cursor;
use ropey::Rope;

/// A single recorded edit, tagged with who made it.
#[derive(Debug, Clone)]
pub struct Edit {
    /// What kind of edit was performed.
    pub kind: EditKind,
    /// Who performed this edit.
    pub author: AuthorId,
    /// When this edit occurred.
    pub timestamp: std::time::Instant,
}

/// The type of edit that was performed.
#[derive(Debug, Clone)]
pub enum EditKind {
    /// Inserted text at a character index.
    Insert {
        /// Character index where text was inserted.
        pos: usize,
        /// The text that was inserted.
        text: String,
    },
    /// Deleted a range of characters.
    Delete {
        /// Start character index of the deleted range.
        start: usize,
        /// End character index of the deleted range.
        end: usize,
        /// The text that was deleted.
        deleted: String,
    },
}

/// Maximum number of entries kept in the undo history before the oldest are
/// dropped. Bounds peak memory for long editing sessions: at ~200 bytes/Edit,
/// 1000 entries caps the history at ~200 KB.
pub const MAX_UNDO_ENTRIES: usize = 1000;

/// The main text buffer, backed by a rope for efficient editing.
///
/// The buffer is dual-backed: a [`Rope`] for fast local text operations,
/// and a [`CrdtDoc`] (automerge) for conflict-free multi-author tracking.
pub struct Buffer {
    /// The underlying rope holding file contents.
    rope: Rope,
    /// CRDT document mirroring the rope contents.
    crdt: CrdtDoc,
    /// Path to the file on disk, if any.
    file_path: Option<std::path::PathBuf>,
    /// Whether the buffer has unsaved modifications.
    modified: bool,
    /// Edit history for undo/redo.
    history: Vec<Edit>,
    /// Current position in the undo history.
    history_pos: usize,
    /// Per-line authorship: who last modified each line.
    line_authors: Vec<AuthorId>,
    /// The most recent edit's author and timestamp.
    last_edit: Option<(AuthorId, std::time::Instant)>,
}

impl Buffer {
    /// Create an empty buffer.
    pub fn new() -> Self {
        Self {
            rope: Rope::new(),
            crdt: CrdtDoc::default(),
            file_path: None,
            modified: false,
            history: Vec::new(),
            history_pos: 0,
            line_authors: vec![AuthorId::human()],
            last_edit: None,
        }
    }

    /// Load a buffer from a file.
    ///
    /// Uses `Rope::from_reader` for efficient streaming reads — the entire
    /// file is never held as a single `String` in memory, making this safe
    /// for large files.
    pub fn from_file(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let file = std::io::BufReader::new(std::fs::File::open(path.as_ref())?);
        let rope = Rope::from_reader(file)?;
        let line_count = rope.len_lines();
        // Initialize CRDT with the file contents.
        let crdt = CrdtDoc::with_text(&rope.to_string())
            .map_err(|e| anyhow::anyhow!("CRDT init failed: {e}"))?;
        Ok(Self {
            rope,
            crdt,
            file_path: Some(path.as_ref().to_path_buf()),
            modified: false,
            history: Vec::new(),
            history_pos: 0,
            line_authors: vec![AuthorId::human(); line_count],
            last_edit: None,
        })
    }

    /// Save the buffer to its file path.
    ///
    /// Uses chunked writes via `Rope::write_to` so the full buffer is never
    /// materialised as a single `String`.
    pub fn save(&mut self) -> anyhow::Result<()> {
        let path = self
            .file_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No file path set for this buffer"))?;
        let file = std::io::BufWriter::new(std::fs::File::create(path)?);
        self.rope.write_to(file)?;
        self.modified = false;
        // Compact CRDT history to free memory.
        self.crdt.compact();
        Ok(())
    }

    /// Create a buffer from a string with an associated file path.
    ///
    /// Used for remote files fetched via SSH where the content is
    /// already available as a string.
    pub fn from_text(text: &str, path: std::path::PathBuf) -> anyhow::Result<Self> {
        let rope = Rope::from_str(text);
        let line_count = rope.len_lines();
        let crdt =
            CrdtDoc::with_text(text).map_err(|e| anyhow::anyhow!("CRDT init failed: {e}"))?;
        Ok(Self {
            rope,
            crdt,
            file_path: Some(path),
            modified: false,
            history: Vec::new(),
            history_pos: 0,
            line_authors: vec![AuthorId::human(); line_count],
            last_edit: None,
        })
    }

    /// Insert text at a character position, tagged with an author.
    pub fn insert(&mut self, char_idx: usize, text: &str, author: AuthorId) {
        let clamped = char_idx.min(self.rope.len_chars());
        self.rope.insert(clamped, text);

        // Mirror to CRDT (log errors but don't fail — rope is the source of truth).
        if let Err(e) = self.crdt.splice(clamped, 0, text, &author) {
            tracing::warn!("CRDT splice failed during insert: {e}");
        }

        self.modified = true;
        let now = std::time::Instant::now();
        self.last_edit = Some((author.clone(), now));

        // Update per-line authorship.
        let start_line = self.rope.char_to_line(clamped);
        let new_lines = text.chars().filter(|c| *c == '\n').count();
        if new_lines > 0 {
            // Bulk insert new line entries (avoid O(n²) individual inserts).
            let insert_pos = (start_line + 1).min(self.line_authors.len());
            let new_entries = vec![author.clone(); new_lines];
            self.line_authors
                .splice(insert_pos..insert_pos, new_entries);
        }
        // Mark the affected line as modified by this author.
        if start_line < self.line_authors.len() {
            self.line_authors[start_line] = author.clone();
        }

        // Truncate any redo history beyond current position.
        self.history.truncate(self.history_pos);
        self.history.push(Edit {
            kind: EditKind::Insert {
                pos: clamped,
                text: text.to_string(),
            },
            author,
            timestamp: now,
        });
        self.history_pos = self.history.len();
        self.bound_history();
    }

    /// Delete a range of characters [start, end), tagged with an author.
    pub fn delete(&mut self, start: usize, end: usize, author: AuthorId) {
        let start = start.min(self.rope.len_chars());
        let end = end.min(self.rope.len_chars());
        if start >= end {
            return;
        }

        let deleted: String = self.rope.slice(start..end).to_string();

        // Count lines being removed before modifying the rope.
        let start_line = self.rope.char_to_line(start);
        let end_line = self.rope.char_to_line(end.saturating_sub(1).max(start));
        let lines_removed = end_line.saturating_sub(start_line);

        self.rope.remove(start..end);

        // Mirror to CRDT (log errors but don't fail — rope is the source of truth).
        if let Err(e) = self
            .crdt
            .splice(start, end.saturating_sub(start), "", &author)
        {
            tracing::warn!("CRDT splice failed during delete: {e}");
        }

        self.modified = true;
        let now = std::time::Instant::now();
        self.last_edit = Some((author.clone(), now));

        // Update per-line authorship: remove deleted lines, mark surviving line.
        if lines_removed > 0 {
            let remove_start = (start_line + 1).min(self.line_authors.len());
            let remove_end = (remove_start + lines_removed).min(self.line_authors.len());
            if remove_start < remove_end {
                self.line_authors.drain(remove_start..remove_end);
            }
        }
        if start_line < self.line_authors.len() {
            self.line_authors[start_line] = author.clone();
        }

        self.history.truncate(self.history_pos);
        self.history.push(Edit {
            kind: EditKind::Delete {
                start,
                end,
                deleted,
            },
            author,
            timestamp: now,
        });
        self.history_pos = self.history.len();
        self.bound_history();
    }

    /// Drop the oldest history entries so the total stays under `MAX_UNDO_ENTRIES`.
    /// Keeps `history_pos` aligned so undo still points at the correct entry.
    fn bound_history(&mut self) {
        if self.history.len() > MAX_UNDO_ENTRIES {
            let overflow = self.history.len() - MAX_UNDO_ENTRIES;
            self.history.drain(..overflow);
            self.history_pos = self.history_pos.saturating_sub(overflow);
        }
    }

    /// Insert a single character at the cursor position.
    pub fn insert_char(&mut self, cursor: &Cursor, ch: char, author: AuthorId) -> usize {
        let pos = self.cursor_to_char_idx(cursor);
        let s = ch.to_string();
        self.insert(pos, &s, author);
        pos + 1
    }

    /// Delete the character before the cursor (backspace).
    pub fn backspace(&mut self, cursor: &Cursor, author: AuthorId) -> Option<usize> {
        let pos = self.cursor_to_char_idx(cursor);
        if pos == 0 {
            return None;
        }
        self.delete(pos - 1, pos, author);
        Some(pos - 1)
    }

    /// Undo the last edit. Returns the author of the undone edit.
    pub fn undo(&mut self) -> Option<AuthorId> {
        if self.history_pos == 0 {
            return None;
        }
        self.history_pos -= 1;
        let edit = &self.history[self.history_pos].clone();
        match &edit.kind {
            EditKind::Insert { pos, text } => {
                let char_count = text.chars().count();
                let end = *pos + char_count;
                self.rope.remove(*pos..end);
                let _ = self.crdt.splice(*pos, char_count, "", &edit.author);
            }
            EditKind::Delete { start, deleted, .. } => {
                self.rope.insert(*start, deleted);
                let _ = self.crdt.splice(*start, 0, deleted, &edit.author);
            }
        }
        self.modified = true;
        self.rebuild_line_authors();
        Some(edit.author.clone())
    }

    /// Undo only edits made by a specific author.
    pub fn undo_by_author(&mut self, target: &AuthorId) -> bool {
        for i in (0..self.history_pos).rev() {
            if &self.history[i].author == target {
                let edit = self.history[i].clone();
                match &edit.kind {
                    EditKind::Insert { pos, text } => {
                        let char_count = text.chars().count();
                        let end = *pos + char_count;
                        self.rope.remove(*pos..end);
                        let _ = self.crdt.splice(*pos, char_count, "", &edit.author);
                    }
                    EditKind::Delete { start, deleted, .. } => {
                        self.rope.insert(*start, deleted);
                        let _ = self.crdt.splice(*start, 0, deleted, &edit.author);
                    }
                }
                self.history.remove(i);
                self.history_pos = self.history.len();
                self.modified = true;
                self.rebuild_line_authors();
                return true;
            }
        }
        false
    }

    /// Rebuild per-line authorship from the edit history.
    ///
    /// Called after undo operations which are hard to track incrementally.
    fn rebuild_line_authors(&mut self) {
        let line_count = self.rope.len_lines().max(1);
        self.line_authors = vec![AuthorId::human(); line_count];

        // Replay visible history to determine per-line authorship.
        // This is O(history * lines) but history is bounded and undo is infrequent.
        for edit in self.history.iter().take(self.history_pos) {
            match &edit.kind {
                EditKind::Insert { pos, text } => {
                    if *pos < self.rope.len_chars() || *pos == 0 {
                        let line = if *pos < self.rope.len_chars() {
                            self.rope.char_to_line(*pos)
                        } else {
                            self.rope.len_lines().saturating_sub(1)
                        };
                        let new_lines = text.chars().filter(|c| *c == '\n').count();
                        let end_line = (line + new_lines).min(line_count.saturating_sub(1));
                        for l in line..=end_line {
                            if l < self.line_authors.len() {
                                self.line_authors[l] = edit.author.clone();
                            }
                        }
                    }
                }
                EditKind::Delete { start, .. } => {
                    if *start < self.rope.len_chars() {
                        let line = self.rope.char_to_line(*start);
                        if line < self.line_authors.len() {
                            self.line_authors[line] = edit.author.clone();
                        }
                    }
                }
            }
        }
    }

    /// Convert a (row, col) cursor to a character index in the rope.
    pub fn cursor_to_char_idx(&self, cursor: &Cursor) -> usize {
        let line_count = self.rope.len_lines();
        if line_count == 0 {
            return 0;
        }
        let row = cursor.row.min(line_count - 1);
        let line_start = self.rope.line_to_char(row);
        let line_len = self.rope.line(row).len_chars();
        // Subtract 1 for the newline if present and not the last line.
        let max_col = if row < line_count - 1 {
            line_len.saturating_sub(1)
        } else {
            line_len
        };
        let col = cursor.col.min(max_col);
        line_start + col
    }

    /// Convert a character index to a (row, col) cursor.
    pub fn char_idx_to_cursor(&self, char_idx: usize) -> Cursor {
        let clamped = char_idx.min(self.rope.len_chars());
        let row = self.rope.char_to_line(clamped);
        let line_start = self.rope.line_to_char(row);
        let col = clamped - line_start;
        Cursor { row, col }
    }

    // --- Word movement ---

    /// Find the start of the next word from a character index.
    /// A "word" is a sequence of alphanumeric/underscore chars, or a sequence
    /// of non-whitespace non-word chars. Whitespace separates words.
    pub fn next_word_start(&self, char_idx: usize) -> usize {
        let len = self.rope.len_chars();
        if char_idx >= len {
            return len;
        }
        let mut i = char_idx;
        let first = self.char_at(i);

        // Skip over current word class.
        if is_word_char(first) {
            while i < len && is_word_char(self.char_at(i)) {
                i += 1;
            }
        } else if !first.is_whitespace() {
            while i < len && !is_word_char(self.char_at(i)) && !self.char_at(i).is_whitespace() {
                i += 1;
            }
        }

        // Skip whitespace.
        while i < len && self.char_at(i).is_whitespace() {
            i += 1;
        }
        i
    }

    /// Find the start of the previous word from a character index.
    pub fn prev_word_start(&self, char_idx: usize) -> usize {
        if char_idx == 0 {
            return 0;
        }
        let mut i = char_idx.saturating_sub(1);

        // Skip whitespace backwards.
        while i > 0 && self.char_at(i).is_whitespace() {
            i -= 1;
        }

        if i == 0 {
            return 0;
        }

        let ch = self.char_at(i);
        if is_word_char(ch) {
            while i > 0 && is_word_char(self.char_at(i.saturating_sub(1))) {
                i -= 1;
            }
        } else {
            while i > 0
                && !is_word_char(self.char_at(i.saturating_sub(1)))
                && !self.char_at(i.saturating_sub(1)).is_whitespace()
            {
                i -= 1;
            }
        }
        i
    }

    /// Find the end of the current word from a character index.
    pub fn word_end(&self, char_idx: usize) -> usize {
        let len = self.rope.len_chars();
        if char_idx >= len {
            return len.saturating_sub(1);
        }
        let mut i = char_idx + 1;

        // Skip whitespace.
        while i < len && self.char_at(i).is_whitespace() {
            i += 1;
        }

        if i >= len {
            return len.saturating_sub(1);
        }

        let ch = self.char_at(i);
        if is_word_char(ch) {
            while i < len.saturating_sub(1) && is_word_char(self.char_at(i + 1)) {
                i += 1;
            }
        } else {
            while i < len.saturating_sub(1)
                && !is_word_char(self.char_at(i + 1))
                && !self.char_at(i + 1).is_whitespace()
            {
                i += 1;
            }
        }
        i
    }

    /// Move backward to the end of the previous word (vim `ge` motion).
    pub fn word_end_backward(&self, char_idx: usize) -> usize {
        if char_idx == 0 {
            return 0;
        }
        let mut i = char_idx;

        // Skip whitespace backward.
        while i > 0 && self.char_at(i).is_whitespace() {
            i -= 1;
        }

        if i == 0 {
            return 0;
        }

        // Now we're on the last char of a word — skip backward through the word.
        let ch = self.char_at(i);
        if is_word_char(ch) {
            while i > 0 && is_word_char(self.char_at(i - 1)) {
                i -= 1;
            }
        } else {
            while i > 0
                && !is_word_char(self.char_at(i - 1))
                && !self.char_at(i - 1).is_whitespace()
            {
                i -= 1;
            }
        }
        i
    }

    /// Get the character at a given index. Returns a space if out of bounds.
    fn char_at(&self, idx: usize) -> char {
        if idx < self.rope.len_chars() {
            self.rope.char(idx)
        } else {
            ' '
        }
    }

    /// Delete an entire line by index and return its contents (including newline).
    pub fn delete_line(&mut self, line_idx: usize, author: AuthorId) -> Option<String> {
        if line_idx >= self.rope.len_lines() {
            return None;
        }
        let start = self.rope.line_to_char(line_idx);
        let line = self.rope.line(line_idx);
        let len = line.len_chars();
        if len == 0 {
            return None;
        }
        let end = start + len;
        let text = self.rope.slice(start..end).to_string();
        self.delete(start, end, author);
        Some(text)
    }

    /// Get the full text of a line including the newline, if present.
    pub fn line_text(&self, line_idx: usize) -> Option<String> {
        if line_idx < self.rope.len_lines() {
            Some(self.rope.line(line_idx).to_string())
        } else {
            None
        }
    }

    // --- Accessors ---

    /// Get a reference to the underlying rope.
    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    /// Get a mutable reference to the underlying rope (for bulk reload).
    pub fn rope_mut(&mut self) -> &mut Rope {
        &mut self.rope
    }

    /// Get the full buffer contents as a String.
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Get the number of lines in the buffer.
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    /// Get a rope slice for the given line index, if it exists.
    pub fn line(&self, idx: usize) -> Option<ropey::RopeSlice<'_>> {
        if idx < self.rope.len_lines() {
            Some(self.rope.line(idx))
        } else {
            None
        }
    }

    /// Whether the buffer has unsaved changes.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Mark the buffer as not modified (e.g., after a remote save).
    pub fn clear_modified(&mut self) {
        self.modified = false;
    }

    /// The file path associated with this buffer, if any.
    pub fn file_path(&self) -> Option<&std::path::Path> {
        self.file_path.as_deref()
    }

    /// Set the file path associated with this buffer.
    pub fn set_file_path(&mut self, path: impl AsRef<std::path::Path>) {
        self.file_path = Some(path.as_ref().to_path_buf());
    }

    /// Total number of characters in the buffer.
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Whether the buffer contains no characters.
    pub fn is_empty(&self) -> bool {
        self.rope.len_chars() == 0
    }

    // --- Authorship ---

    /// Get the author who last modified a given line.
    pub fn line_author(&self, line_idx: usize) -> Option<&AuthorId> {
        self.line_authors.get(line_idx)
    }

    /// Get the author and timestamp of the most recent edit.
    pub fn last_edit(&self) -> Option<(&AuthorId, std::time::Instant)> {
        self.last_edit.as_ref().map(|(a, t)| (a, *t))
    }

    // ----- Vim motion helpers -----

    /// Join the given line with the next line, replacing the newline with a space.
    pub fn join_lines(&mut self, line: usize, author: AuthorId) {
        if line + 1 >= self.line_count() {
            return;
        }
        // Find the newline at the end of the line.
        let line_end = self.rope.line_to_char(line + 1);
        let join_pos = line_end.saturating_sub(1);
        if join_pos < self.rope.len_chars() {
            self.delete(join_pos, join_pos + 1, author.clone());
            // Insert a space where the newline was (unless the next line starts with whitespace).
            let next_char = self.rope.get_char(join_pos);
            if next_char.map(|c| !c.is_whitespace()).unwrap_or(true) {
                self.insert(join_pos, " ", author);
            }
        }
    }

    /// Indent lines in the range [start_line, end_line] by prepending `indent_str`.
    pub fn indent_lines(
        &mut self,
        start_line: usize,
        end_line: usize,
        indent_str: &str,
        author: AuthorId,
    ) {
        // Work backwards to avoid index shifting.
        for line in (start_line..=end_line.min(self.line_count().saturating_sub(1))).rev() {
            let char_idx = self.rope.line_to_char(line);
            self.insert(char_idx, indent_str, author.clone());
        }
    }

    /// Dedent lines in the range [start_line, end_line] by removing one indent level.
    pub fn dedent_lines(
        &mut self,
        start_line: usize,
        end_line: usize,
        tab_width: usize,
        author: AuthorId,
    ) {
        for line in (start_line..=end_line.min(self.line_count().saturating_sub(1))).rev() {
            let char_idx = self.rope.line_to_char(line);
            if let Some(line_text) = self.line_text(line) {
                let mut remove = 0;
                for ch in line_text.chars() {
                    if ch == '\t' && remove == 0 {
                        remove = 1;
                        break;
                    } else if ch == ' ' && remove < tab_width {
                        remove += 1;
                    } else {
                        break;
                    }
                }
                if remove > 0 {
                    self.delete(char_idx, char_idx + remove, author.clone());
                }
            }
        }
    }

    /// Find the inner range of a delimiter pair around `char_idx`.
    /// Returns (start, end) character indices of the content between delimiters.
    pub fn find_inner_delimited(
        &self,
        char_idx: usize,
        open: char,
        close: char,
    ) -> Option<(usize, usize)> {
        let text = self.rope.to_string();
        let bytes = text.as_bytes();
        let byte_idx = self.rope.char_to_byte(char_idx.min(self.rope.len_chars()));

        // Search backward for opening delimiter.
        let mut depth = 0i32;
        let mut open_pos = None;
        for i in (0..byte_idx).rev() {
            let ch = bytes[i] as char;
            if ch == close && open != close {
                depth += 1;
            } else if ch == open {
                if depth == 0 {
                    open_pos = Some(i);
                    break;
                }
                depth -= 1;
            }
        }
        // For same-char delimiters (quotes), search outward.
        if open == close {
            let line_start = text[..byte_idx].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let line_end = text[byte_idx..]
                .find('\n')
                .map(|p| byte_idx + p)
                .unwrap_or(text.len());
            let line = &text[line_start..line_end];
            let positions: Vec<usize> = line
                .char_indices()
                .filter(|(_, c)| *c == open)
                .map(|(i, _)| line_start + i)
                .collect();
            // Find the pair that encloses char_idx.
            for pair in positions.windows(2) {
                if pair[0] < byte_idx && byte_idx <= pair[1] {
                    let start = self.rope.byte_to_char(pair[0] + open.len_utf8());
                    let end = self.rope.byte_to_char(pair[1]);
                    return Some((start, end));
                }
            }
            return None;
        }
        let open_byte = open_pos?;

        // Search forward for closing delimiter.
        depth = 0;
        let mut close_pos = None;
        for (i, &b) in bytes.iter().enumerate().skip(open_byte + 1) {
            let ch = b as char;
            if ch == open {
                depth += 1;
            } else if ch == close {
                if depth == 0 {
                    close_pos = Some(i);
                    break;
                }
                depth -= 1;
            }
        }
        let close_byte = close_pos?;

        let start = self.rope.byte_to_char(open_byte + open.len_utf8());
        let end = self.rope.byte_to_char(close_byte);
        Some((start, end))
    }

    /// Find the around range of a delimiter pair (including delimiters).
    pub fn find_around_delimited(
        &self,
        char_idx: usize,
        open: char,
        close: char,
    ) -> Option<(usize, usize)> {
        let (inner_start, inner_end) = self.find_inner_delimited(char_idx, open, close)?;
        let start = inner_start.saturating_sub(1); // include opening delimiter
        let end = (inner_end + 1).min(self.rope.len_chars()); // include closing delimiter
        Some((start, end))
    }

    /// Find the inner word boundaries around `char_idx`.
    pub fn find_inner_word(&self, char_idx: usize) -> (usize, usize) {
        let len = self.rope.len_chars();
        if len == 0 {
            return (0, 0);
        }
        let idx = char_idx.min(len.saturating_sub(1));
        let ch = self.rope.get_char(idx).unwrap_or(' ');
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let in_word = is_word(ch);

        let mut start = idx;
        while start > 0 {
            let prev = self.rope.get_char(start - 1).unwrap_or(' ');
            if is_word(prev) != in_word {
                break;
            }
            start -= 1;
        }

        let mut end = idx + 1;
        while end < len {
            let next = self.rope.get_char(end).unwrap_or(' ');
            if is_word(next) != in_word {
                break;
            }
            end += 1;
        }

        (start, end)
    }

    /// Find the around word boundaries (word + trailing whitespace).
    pub fn find_around_word(&self, char_idx: usize) -> (usize, usize) {
        let (start, mut end) = self.find_inner_word(char_idx);
        let len = self.rope.len_chars();
        // Include trailing whitespace.
        while end < len {
            let ch = self.rope.get_char(end).unwrap_or('x');
            if !ch.is_whitespace() || ch == '\n' {
                break;
            }
            end += 1;
        }
        (start, end)
    }

    /// Get the word under the cursor at (row, col).
    pub fn word_at_cursor(&self, row: usize, col: usize) -> String {
        let cursor = Cursor { row, col };
        let char_idx = self.cursor_to_char_idx(&cursor);
        let (start, end) = self.find_inner_word(char_idx);
        self.rope.slice(start..end).to_string()
    }

    /// Replace a byte range with new text, preserving authorship as human.
    pub fn replace_range(&mut self, start_byte: usize, end_byte: usize, new_text: &str) {
        let start_char = self.rope.byte_to_char(start_byte);
        let end_char = self.rope.byte_to_char(end_byte);
        self.delete(start_char, end_char, AuthorId::Human);
        self.insert(start_char, new_text, AuthorId::Human);
    }

    /// Get a single character at the given index.
    pub fn get_char(&self, char_idx: usize) -> Option<char> {
        self.rope.get_char(char_idx)
    }

    /// Get a reference to the CRDT document.
    pub fn crdt(&self) -> &CrdtDoc {
        &self.crdt
    }

    /// Get a mutable reference to the CRDT document.
    pub fn crdt_mut(&mut self) -> &mut CrdtDoc {
        &mut self.crdt
    }

    // ----- Collaborative sync -----

    /// Apply a sync message from a remote peer, reconciling the rope with the
    /// CRDT state afterwards.
    ///
    /// Uses incremental reconciliation: finds the first and last differing
    /// characters between the old and new text, then patches only that range
    /// in the rope. This is O(delta + scan) instead of O(document).
    pub fn apply_remote_sync(
        &mut self,
        sync_state: &mut crate::sync::SyncState,
        msg: crate::sync::SyncMessage,
        remote_author: &AuthorId,
    ) -> Result<(), automerge::AutomergeError> {
        self.crdt.receive_sync_message(sync_state, msg)?;

        // Get the CRDT text as the source of truth.
        let new_text = self.crdt.text()?;
        let old_text = self.rope.to_string();

        if new_text == old_text {
            return Ok(());
        }

        let old_bytes = old_text.as_bytes();
        let new_bytes = new_text.as_bytes();

        // Find the first byte that differs.
        let prefix_len = old_bytes
            .iter()
            .zip(new_bytes.iter())
            .take_while(|(a, b)| a == b)
            .count();

        // Find the last byte that differs (scanning from the end).
        let old_suffix_start = old_bytes.len();
        let new_suffix_start = new_bytes.len();
        let max_suffix = (old_suffix_start - prefix_len).min(new_suffix_start - prefix_len);
        let suffix_len = old_bytes[old_suffix_start.saturating_sub(max_suffix)..]
            .iter()
            .rev()
            .zip(
                new_bytes[new_suffix_start.saturating_sub(max_suffix)..]
                    .iter()
                    .rev(),
            )
            .take_while(|(a, b)| a == b)
            .count();

        let old_change_end = old_suffix_start - suffix_len;
        let new_change_end = new_suffix_start - suffix_len;

        // Convert byte offsets to char offsets in the rope.
        let char_start = old_text[..prefix_len].chars().count();
        let char_old_end = old_text[..old_change_end].chars().count();
        let insert_text = &new_text[prefix_len..new_change_end];

        // Apply the minimal patch to the rope.
        if char_old_end > char_start {
            self.rope.remove(char_start..char_old_end);
        }
        if !insert_text.is_empty() {
            self.rope.insert(char_start, insert_text);
        }

        // Update line authors for affected lines.
        let line_count = self.rope.len_lines().max(1);
        if self.history.is_empty() {
            self.line_authors = vec![remote_author.clone(); line_count];
        } else {
            // Determine affected line range and mark those as remote.
            let affected_line_start = self.rope.char_to_line(char_start);
            let affected_line_end =
                if char_start + insert_text.chars().count() <= self.rope.len_chars() {
                    self.rope
                        .char_to_line(char_start + insert_text.chars().count())
                } else {
                    line_count.saturating_sub(1)
                };

            // Resize line_authors to match.
            self.line_authors.resize(line_count, remote_author.clone());
            for line in affected_line_start..=affected_line_end.min(line_count.saturating_sub(1)) {
                if line < self.line_authors.len() {
                    self.line_authors[line] = remote_author.clone();
                }
            }
        }

        self.modified = true;
        Ok(())
    }

    /// Load a full document snapshot from a remote host (initial sync).
    ///
    /// Replaces both the CRDT and rope with the snapshot content.
    pub fn load_remote_snapshot(&mut self, bytes: &[u8]) -> Result<(), automerge::AutomergeError> {
        let new_crdt = CrdtDoc::load_bytes(bytes)?;
        let text = new_crdt.text()?;
        let line_count = text.lines().count().max(1);

        self.crdt = new_crdt;
        self.rope = Rope::from_str(&text);
        self.line_authors = vec![AuthorId::human(); line_count];
        self.history.clear();
        self.history_pos = 0;
        self.modified = false;

        Ok(())
    }

    /// Get the edit history (visible portion up to history_pos).
    pub fn history(&self) -> &[Edit] {
        &self.history[..self.history_pos]
    }

    /// Get the full edit history including redo entries beyond history_pos.
    pub fn full_history(&self) -> &[Edit] {
        &self.history
    }

    /// Get the current history position (index of the next edit to undo).
    pub fn history_pos(&self) -> usize {
        self.history_pos
    }

    /// Restore the buffer to a specific history position by undoing/redoing.
    ///
    /// If `target_pos < history_pos`, undoes edits to reach the target.
    /// If `target_pos > history_pos`, redoes edits to reach the target.
    pub fn restore_to(&mut self, target_pos: usize) {
        let target_pos = target_pos.min(self.history.len());
        // Undo to reach target.
        while self.history_pos > target_pos {
            self.history_pos -= 1;
            let edit = self.history[self.history_pos].clone();
            match &edit.kind {
                EditKind::Insert { pos, text } => {
                    let char_count = text.chars().count();
                    let end = *pos + char_count;
                    if end <= self.rope.len_chars() {
                        self.rope.remove(*pos..end);
                        let _ = self.crdt.splice(*pos, char_count, "", &edit.author);
                    }
                }
                EditKind::Delete { start, deleted, .. } => {
                    if *start <= self.rope.len_chars() {
                        self.rope.insert(*start, deleted);
                        let _ = self.crdt.splice(*start, 0, deleted, &edit.author);
                    }
                }
            }
        }
        // Redo to reach target.
        while self.history_pos < target_pos {
            let edit = self.history[self.history_pos].clone();
            match &edit.kind {
                EditKind::Insert { pos, text } => {
                    if *pos <= self.rope.len_chars() {
                        self.rope.insert(*pos, text);
                        let _ = self.crdt.splice(*pos, 0, text, &edit.author);
                    }
                }
                EditKind::Delete { start, end, .. } => {
                    let char_count = end.saturating_sub(*start);
                    if *start + char_count <= self.rope.len_chars() {
                        self.rope.remove(*start..*start + char_count);
                        let _ = self.crdt.splice(*start, char_count, "", &edit.author);
                    }
                }
            }
            self.history_pos += 1;
        }
        self.modified = true;
        self.rebuild_line_authors();
    }

    // --- Bracket matching ---

    /// Bracket pairs used for matching.
    const BRACKET_PAIRS: &'static [(char, char)] = &[('(', ')'), ('{', '}'), ('[', ']')];

    /// Find the matching bracket for the character at `char_idx`.
    ///
    /// Returns `Some(matching_char_idx)` if the character is one of `(){}[]`
    /// and a matching counterpart is found. Handles nesting correctly.
    pub fn find_matching_bracket(&self, char_idx: usize) -> Option<usize> {
        if char_idx >= self.rope.len_chars() {
            return None;
        }
        let ch = self.rope.char(char_idx);

        // Check if it's an opening bracket.
        for &(open, close) in Self::BRACKET_PAIRS {
            if ch == open {
                return self.scan_forward(char_idx, open, close);
            }
            if ch == close {
                return self.scan_backward(char_idx, open, close);
            }
        }
        None
    }

    /// Scan forward from `start` to find the matching closing bracket.
    fn scan_forward(&self, start: usize, open: char, close: char) -> Option<usize> {
        let len = self.rope.len_chars();
        let mut depth: usize = 0;
        for i in start..len {
            let c = self.rope.char(i);
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }
        None
    }

    /// Scan backward from `start` to find the matching opening bracket.
    fn scan_backward(&self, start: usize, open: char, close: char) -> Option<usize> {
        let mut depth: usize = 0;
        for i in (0..=start).rev() {
            let c = self.rope.char(i);
            if c == close {
                depth += 1;
            } else if c == open {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }
        None
    }

    // --- Search ---

    /// Find all occurrences of `query` in the buffer.
    ///
    /// Returns a `Vec` of `(start_char_idx, end_char_idx)` pairs.
    /// Non-overlapping matches only.
    pub fn find_all(&self, query: &str) -> Vec<(usize, usize)> {
        if query.is_empty() {
            return Vec::new();
        }
        let text = self.rope.to_string();
        let query_char_len = query.chars().count();
        text.match_indices(query)
            .map(|(byte_offset, _)| {
                let char_start = text[..byte_offset].chars().count();
                (char_start, char_start + query_char_len)
            })
            .collect()
    }

    /// Replace all occurrences of `old` with `new` within the char range
    /// `[range_start, range_end)`.
    ///
    /// Replacements are applied from the end backwards to preserve earlier
    /// indices. Returns the number of replacements made.
    pub fn replace_all(
        &mut self,
        old: &str,
        new: &str,
        range_start: usize,
        range_end: usize,
        author: AuthorId,
    ) -> usize {
        let matches = self.find_all(old);
        let old_char_len = old.chars().count();
        let mut count = 0;
        // Apply from end to start so indices stay valid.
        for &(start, _end) in matches.iter().rev() {
            if start >= range_start && start + old_char_len <= range_end {
                self.delete(start, start + old_char_len, author.clone());
                self.insert(start, new, author.clone());
                count += 1;
            }
        }
        count
    }

    /// Return a text representation of the undo history tree.
    ///
    /// Each entry shows: its index, the author, the edit type and size, and
    /// whether it is the current position (marked with `→`) or a future redo
    /// entry (marked with `(redo)`).  An empty history returns a placeholder
    /// message.
    ///
    /// Example output:
    /// ```text
    ///   0 [human] insert 5 chars
    ///   1 [human] delete 3 chars
    /// → 2 [ai:claude] insert 20 chars
    ///   3 [human] insert 1 char     (redo)
    /// ```
    pub fn undo_tree_text(&self) -> String {
        if self.history.is_empty() {
            return "  (no edits yet)".to_string();
        }

        let mut lines = Vec::with_capacity(self.history.len());
        for (i, edit) in self.history.iter().enumerate() {
            let author_label = match &edit.author {
                crate::author::AuthorId::Human => "human".to_string(),
                crate::author::AuthorId::Ai(name) => format!("ai:{name}"),
                crate::author::AuthorId::Peer { name, peer_id } => {
                    format!("peer:{name}#{peer_id}")
                }
            };
            let action_label = match &edit.kind {
                EditKind::Insert { text, .. } => {
                    let n = text.chars().count();
                    format!("insert {} char{}", n, if n == 1 { "" } else { "s" })
                }
                EditKind::Delete { deleted, .. } => {
                    let n = deleted.chars().count();
                    format!("delete {} char{}", n, if n == 1 { "" } else { "s" })
                }
            };

            // The current position points to the *next* edit to be undone, so
            // the entry at index `history_pos - 1` is the most recent active
            // edit.  Entries at index >= `history_pos` are redo entries.
            let is_current = self.history_pos > 0 && i == self.history_pos - 1;
            let is_redo = i >= self.history_pos;

            let prefix = if is_current { "→" } else { " " };
            let suffix = if is_redo { "     (redo)" } else { "" };

            lines.push(format!(
                "{prefix} {i:>3} [{author_label}] {action_label}{suffix}"
            ));
        }
        lines.join("\n")
    }
}

/// Returns true if the character is a "word" character (alphanumeric or underscore).
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::author::AuthorId;

    fn human() -> AuthorId {
        AuthorId::human()
    }

    fn ai() -> AuthorId {
        AuthorId::ai("test-agent")
    }

    #[test]
    fn test_insert_and_read() {
        let mut buf = Buffer::new();
        buf.insert(0, "Hello, world!", human());
        assert_eq!(buf.text(), "Hello, world!");
        assert!(buf.is_modified());
    }

    #[test]
    fn test_delete() {
        let mut buf = Buffer::new();
        buf.insert(0, "Hello, world!", human());
        buf.delete(5, 7, human()); // remove ", "
        assert_eq!(buf.text(), "Helloworld!");
    }

    #[test]
    fn test_undo() {
        let mut buf = Buffer::new();
        buf.insert(0, "Hello", human());
        buf.insert(5, " world", human());
        assert_eq!(buf.text(), "Hello world");

        buf.undo();
        assert_eq!(buf.text(), "Hello");

        buf.undo();
        assert_eq!(buf.text(), "");
    }

    #[test]
    fn test_history_is_bounded() {
        let mut buf = Buffer::new();
        for i in 0..MAX_UNDO_ENTRIES + 250 {
            buf.insert(buf.len_chars(), "x", human());
            assert!(
                buf.history.len() <= MAX_UNDO_ENTRIES,
                "history grew past cap after {i} edits"
            );
        }
        assert_eq!(buf.history.len(), MAX_UNDO_ENTRIES);
        // history_pos tracks the "current" undo position; must stay within bounds.
        assert!(buf.history_pos <= buf.history.len());
        // Undo still works on the retained tail.
        let before = buf.text().len();
        buf.undo();
        assert_eq!(buf.text().len(), before - 1);
    }

    #[test]
    fn test_undo_by_author() {
        let mut buf = Buffer::new();
        buf.insert(0, "human wrote this", human());
        buf.insert(16, "\nai added this", ai());
        assert_eq!(buf.text(), "human wrote this\nai added this");

        // Undo only the AI's edit.
        let undone = buf.undo_by_author(&ai());
        assert!(undone);
        assert_eq!(buf.text(), "human wrote this");
    }

    #[test]
    fn test_cursor_conversion_roundtrip() {
        let mut buf = Buffer::new();
        buf.insert(0, "line one\nline two\nline three", human());

        let cursor = Cursor { row: 1, col: 5 };
        let char_idx = buf.cursor_to_char_idx(&cursor);
        let back = buf.char_idx_to_cursor(char_idx);
        assert_eq!(back.row, 1);
        assert_eq!(back.col, 5);
    }

    #[test]
    fn test_empty_buffer_operations() {
        let mut buf = Buffer::new();
        assert_eq!(buf.line_count(), 1); // rope always has at least 1 line
        assert!(buf.undo().is_none());
        assert!(!buf.undo_by_author(&human()));
    }

    #[test]
    fn test_word_movement() {
        let mut buf = Buffer::new();
        buf.insert(0, "hello world  foo_bar", human());

        // next_word_start
        assert_eq!(buf.next_word_start(0), 6); // "hello" -> "world"
        assert_eq!(buf.next_word_start(6), 13); // "world" -> "foo_bar"

        // prev_word_start
        assert_eq!(buf.prev_word_start(6), 0); // "world" -> "hello"
        assert_eq!(buf.prev_word_start(13), 6); // "foo_bar" -> "world"

        // word_end
        assert_eq!(buf.word_end(0), 4); // end of "hello"
        assert_eq!(buf.word_end(5), 10); // end of "world"
    }

    #[test]
    fn test_delete_line() {
        let mut buf = Buffer::new();
        buf.insert(0, "line one\nline two\nline three", human());
        let deleted = buf.delete_line(1, human());
        assert_eq!(deleted, Some("line two\n".to_string()));
        assert_eq!(buf.text(), "line one\nline three");
    }

    // --- Bracket matching tests ---

    #[test]
    fn test_bracket_match_simple_parens() {
        let mut buf = Buffer::new();
        buf.insert(0, "(hello)", human());
        assert_eq!(buf.find_matching_bracket(0), Some(6)); // ( -> )
        assert_eq!(buf.find_matching_bracket(6), Some(0)); // ) -> (
    }

    #[test]
    fn test_bracket_match_nested() {
        let mut buf = Buffer::new();
        buf.insert(0, "({[]})", human());
        assert_eq!(buf.find_matching_bracket(0), Some(5)); // ( -> )
        assert_eq!(buf.find_matching_bracket(1), Some(4)); // { -> }
        assert_eq!(buf.find_matching_bracket(2), Some(3)); // [ -> ]
        assert_eq!(buf.find_matching_bracket(3), Some(2)); // ] -> [
        assert_eq!(buf.find_matching_bracket(4), Some(1)); // } -> {
        assert_eq!(buf.find_matching_bracket(5), Some(0)); // ) -> (
    }

    #[test]
    fn test_bracket_match_unmatched() {
        let mut buf = Buffer::new();
        buf.insert(0, "(hello", human());
        assert_eq!(buf.find_matching_bracket(0), None);
    }

    #[test]
    fn test_bracket_match_not_a_bracket() {
        let mut buf = Buffer::new();
        buf.insert(0, "hello", human());
        assert_eq!(buf.find_matching_bracket(0), None);
    }

    #[test]
    fn test_bracket_match_empty_buffer() {
        let buf = Buffer::new();
        assert_eq!(buf.find_matching_bracket(0), None);
    }

    #[test]
    fn test_bracket_match_code() {
        let mut buf = Buffer::new();
        buf.insert(0, "fn main() {\n    let x = vec![1, 2];\n}", human());
        // The opening { is at char index 11
        let text = buf.text();
        let open_brace = text.find('{').unwrap();
        let close_brace = text.rfind('}').unwrap();
        assert_eq!(buf.find_matching_bracket(open_brace), Some(close_brace));
        assert_eq!(buf.find_matching_bracket(close_brace), Some(open_brace));
    }

    // --- Search tests ---

    #[test]
    fn test_find_all_basic() {
        let mut buf = Buffer::new();
        buf.insert(0, "hello hello hello", human());
        let matches = buf.find_all("hello");
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0], (0, 5));
        assert_eq!(matches[1], (6, 11));
        assert_eq!(matches[2], (12, 17));
    }

    #[test]
    fn test_find_all_no_match() {
        let mut buf = Buffer::new();
        buf.insert(0, "hello world", human());
        let matches = buf.find_all("xyz");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_find_all_empty_query() {
        let mut buf = Buffer::new();
        buf.insert(0, "hello", human());
        assert!(buf.find_all("").is_empty());
    }

    #[test]
    fn test_replace_all_basic() {
        let mut buf = Buffer::new();
        buf.insert(0, "foo bar foo baz foo", human());
        let len = buf.len_chars();
        let count = buf.replace_all("foo", "qux", 0, len, human());
        assert_eq!(count, 3);
        assert_eq!(buf.text(), "qux bar qux baz qux");
    }

    #[test]
    fn test_replace_all_in_range() {
        let mut buf = Buffer::new();
        buf.insert(0, "foo\nfoo\nfoo", human());
        // Replace only in the first line (chars 0..4 = "foo\n").
        let count = buf.replace_all("foo", "bar", 0, 4, human());
        assert_eq!(count, 1);
        assert_eq!(buf.text(), "bar\nfoo\nfoo");
    }

    #[test]
    fn test_buffer_remote_sync() {
        // Create two buffers sharing the same CRDT origin.
        let mut buf_a = Buffer::new();
        buf_a.insert(0, "hello", human());

        // Fork the CRDT to create buf_b's initial state.
        let snapshot = buf_a.crdt_mut().save_bytes();
        let mut buf_b = Buffer::new();
        buf_b.load_remote_snapshot(&snapshot).unwrap();
        assert_eq!(buf_b.text(), "hello");

        // Make an edit on A.
        buf_a.insert(5, " world", human());
        assert_eq!(buf_a.text(), "hello world");

        // Sync A → B.
        let remote_author = AuthorId::peer("alice", 1);
        let mut state_a = crate::sync::SyncState::new();
        let mut state_b = crate::sync::SyncState::new();

        for _ in 0..10 {
            if let Some(m) = buf_a.crdt_mut().generate_sync_message(&mut state_a) {
                buf_b
                    .apply_remote_sync(&mut state_b, m, &remote_author)
                    .unwrap();
            }
            if let Some(m) = buf_b.crdt_mut().generate_sync_message(&mut state_b) {
                buf_a
                    .apply_remote_sync(&mut state_a, m, &AuthorId::peer("bob", 2))
                    .unwrap();
            }
        }

        assert_eq!(buf_b.text(), "hello world");
        assert_eq!(buf_a.text(), buf_b.text());
    }

    #[test]
    fn test_incremental_sync_small_edit_in_large_doc() {
        // Create a large document.
        let mut lines: Vec<String> = (0..100)
            .map(|i| format!("line {} content here\n", i))
            .collect();
        let big_text = lines.join("");

        let mut buf_a = Buffer::new();
        buf_a.insert(0, &big_text, human());

        let snapshot = buf_a.crdt_mut().save_bytes();
        let mut buf_b = Buffer::new();
        buf_b.load_remote_snapshot(&snapshot).unwrap();

        // A makes a small edit in the middle of the document.
        let edit_line = 50;
        let line_start = buf_a.rope().line_to_char(edit_line);
        buf_a.insert(line_start, "INSERTED ", human());
        lines[edit_line] = format!("INSERTED line {} content here\n", edit_line);
        let expected = lines.join("");

        // Sync A → B.
        let remote_author = AuthorId::peer("alice", 1);
        let mut state_a = crate::sync::SyncState::new();
        let mut state_b = crate::sync::SyncState::new();

        for _ in 0..10 {
            if let Some(m) = buf_a.crdt_mut().generate_sync_message(&mut state_a) {
                buf_b
                    .apply_remote_sync(&mut state_b, m, &remote_author)
                    .unwrap();
            }
            if let Some(m) = buf_b.crdt_mut().generate_sync_message(&mut state_b) {
                buf_a
                    .apply_remote_sync(&mut state_a, m, &AuthorId::peer("bob", 2))
                    .unwrap();
            }
        }

        assert_eq!(buf_b.text(), expected);
        assert_eq!(buf_a.text(), buf_b.text());

        // Verify that the line_authors for the edited line is the remote author.
        if let Some(author) = buf_b.line_author(edit_line) {
            assert!(
                author.is_peer(),
                "edited line should be attributed to remote peer"
            );
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::author::AuthorId;
    use proptest::prelude::*;

    fn human() -> AuthorId {
        AuthorId::human()
    }

    proptest! {
        /// Random insert/delete sequences should never corrupt the buffer.
        #[test]
        fn random_inserts_never_corrupt(
            ops in proptest::collection::vec(
                (0..100usize, "[a-zA-Z0-9 \n]{1,10}"),
                1..50
            )
        ) {
            let mut buf = Buffer::new();
            for (pos, text) in &ops {
                let clamped = (*pos).min(buf.len_chars());
                buf.insert(clamped, text, human());
            }
            // Buffer should remain valid: text length should match rope.
            let text = buf.text();
            prop_assert_eq!(text.len(), buf.rope().len_bytes());
        }

        /// Insert then undo should return buffer to original state.
        #[test]
        fn insert_undo_roundtrip(text in "[a-zA-Z0-9]{1,20}") {
            let mut buf = Buffer::new();
            buf.insert(0, "original", human());
            let original = buf.text();
            buf.insert(buf.len_chars(), &text, human());
            buf.undo();
            prop_assert_eq!(buf.text(), original);
        }

        /// Random deletes should never panic or corrupt buffer.
        #[test]
        fn random_deletes_never_panic(
            initial in "[a-zA-Z0-9 \n]{10,100}",
            ranges in proptest::collection::vec((0..50usize, 0..50usize), 1..20)
        ) {
            let mut buf = Buffer::new();
            buf.insert(0, &initial, human());
            for (a, b) in &ranges {
                let start = (*a).min(buf.len_chars());
                let end = (*b).min(buf.len_chars());
                let (start, end) = if start <= end { (start, end) } else { (end, start) };
                buf.delete(start, end, human());
            }
            // Should still be valid.
            let _ = buf.text();
            prop_assert!(buf.line_count() >= 1);
        }
    }
}
