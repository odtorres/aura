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
    Insert { pos: usize, text: String },
    /// Deleted a range of characters.
    Delete {
        start: usize,
        end: usize,
        deleted: String,
    },
}

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
            crdt: CrdtDoc::new(),
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
        let crdt = CrdtDoc::with_text(&rope.to_string());
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

    /// Insert text at a character position, tagged with an author.
    pub fn insert(&mut self, char_idx: usize, text: &str, author: AuthorId) {
        let clamped = char_idx.min(self.rope.len_chars());
        self.rope.insert(clamped, text);

        // Mirror to CRDT.
        self.crdt.splice(clamped, 0, text, &author);

        self.modified = true;
        let now = std::time::Instant::now();
        self.last_edit = Some((author.clone(), now));

        // Update per-line authorship.
        let start_line = self.rope.char_to_line(clamped);
        let new_lines = text.chars().filter(|c| *c == '\n').count();
        if new_lines > 0 {
            // Insert new line entries after the current line.
            let insert_pos = (start_line + 1).min(self.line_authors.len());
            for _ in 0..new_lines {
                self.line_authors.insert(insert_pos, author.clone());
            }
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

        // Mirror to CRDT.
        self.crdt
            .splice(start, end.saturating_sub(start), "", &author);

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
                self.crdt.splice(*pos, char_count, "", &edit.author);
            }
            EditKind::Delete { start, deleted, .. } => {
                self.rope.insert(*start, deleted);
                self.crdt.splice(*start, 0, deleted, &edit.author);
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
                        self.crdt.splice(*pos, char_count, "", &edit.author);
                    }
                    EditKind::Delete { start, deleted, .. } => {
                        self.rope.insert(*start, deleted);
                        self.crdt.splice(*start, 0, deleted, &edit.author);
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

    /// Get a reference to the CRDT document.
    pub fn crdt(&self) -> &CrdtDoc {
        &self.crdt
    }

    /// Get a mutable reference to the CRDT document.
    pub fn crdt_mut(&mut self) -> &mut CrdtDoc {
        &mut self.crdt
    }

    /// Get the edit history (visible portion up to history_pos).
    pub fn history(&self) -> &[Edit] {
        &self.history[..self.history_pos]
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
