//! Fuzzy file picker for quickly navigating to files in the workspace.

use std::path::PathBuf;

/// A fuzzy file picker that scans the workspace and allows interactive search.
pub struct FilePicker {
    /// Whether the picker is currently visible.
    pub visible: bool,
    /// The search query typed by the user.
    pub query: String,
    /// All discovered file paths (relative to workspace root).
    entries: Vec<String>,
    /// Filtered entries matching the current query.
    pub filtered: Vec<String>,
    /// Currently selected index in the filtered list.
    pub selected: usize,
    /// The workspace root directory.
    root: PathBuf,
}

/// Maximum number of files to scan to avoid excessive slowness.
const MAX_FILES: usize = 10_000;

/// Directories to skip during recursive scanning.
const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", ".aura"];

impl FilePicker {
    /// Create a new FilePicker rooted at `root`.
    ///
    /// Immediately scans the directory tree (skipping `.git`, `target`,
    /// `node_modules`, and `.aura`), collects relative file paths, and
    /// sorts them alphabetically. The picker starts hidden.
    pub fn new(root: PathBuf) -> Self {
        let mut picker = Self {
            visible: false,
            query: String::new(),
            entries: Vec::new(),
            filtered: Vec::new(),
            selected: 0,
            root,
        };
        picker.scan_entries();
        picker
    }

    /// Open the picker: make it visible, clear the query, reset selection,
    /// and re-run the filter.
    pub fn open(&mut self) {
        self.visible = true;
        self.query.clear();
        self.selected = 0;
        self.filter();
    }

    /// Close the picker.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Append a character to the query and re-filter.
    pub fn type_char(&mut self, c: char) {
        self.query.push(c);
        self.selected = 0;
        self.filter();
    }

    /// Remove the last character from the query and re-filter.
    pub fn backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
        self.filter();
    }

    /// Move the selection up by one entry (wraps around).
    pub fn select_up(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.filtered.len() - 1;
        } else {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    /// Move the selection down by one entry (wraps around).
    pub fn select_down(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.filtered.len();
    }

    /// Return the full path of the currently selected entry, or `None` if
    /// the filtered list is empty or the index is out of range.
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.filtered
            .get(self.selected)
            .map(|rel| self.root.join(rel))
    }

    /// Parse a `filename:line` pattern from the query.
    /// Returns `(file_query, Some(line_number))` if the pattern is detected,
    /// otherwise `(query, None)`.
    pub fn parse_goto_line(&self) -> Option<usize> {
        // Check if query contains `:` followed by digits at the end.
        if let Some(colon_pos) = self.query.rfind(':') {
            let after_colon = &self.query[colon_pos + 1..];
            if let Ok(line) = after_colon.parse::<usize>() {
                if line > 0 {
                    return Some(line);
                }
            }
        }
        None
    }

    /// Return the query part without the `:line` suffix.
    pub fn file_query(&self) -> &str {
        if let Some(colon_pos) = self.query.rfind(':') {
            let after_colon = &self.query[colon_pos + 1..];
            if after_colon.parse::<usize>().is_ok() {
                return &self.query[..colon_pos];
            }
        }
        &self.query
    }

    /// Re-scan the directory and refresh the filtered list.
    pub fn refresh(&mut self) {
        self.scan_entries();
        self.filter();
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    /// Recursively scan `self.root` and populate `self.entries` with relative
    /// paths sorted alphabetically. Caps at [`MAX_FILES`].
    fn scan_entries(&mut self) {
        let mut entries = Vec::new();
        collect_files(&self.root, &self.root, &mut entries);
        entries.sort();
        self.entries = entries;
    }

    /// Fuzzy-filter `self.entries` against `self.query` and update
    /// `self.filtered`.
    ///
    /// A file matches if every character of the query appears in order inside
    /// the file path (case-insensitive). Results are sorted so that:
    /// 1. Exact substring matches come first.
    /// 2. Within each group, shorter paths (closer match) come first.
    fn filter(&mut self) {
        let query_lower = self.query.to_lowercase();

        if query_lower.is_empty() {
            self.filtered = self.entries.clone();
            self.selected = 0;
            return;
        }

        let mut exact: Vec<&str> = Vec::new();
        let mut fuzzy: Vec<&str> = Vec::new();

        for entry in &self.entries {
            let entry_lower = entry.to_lowercase();
            if is_fuzzy_match(&entry_lower, &query_lower) {
                if entry_lower.contains(query_lower.as_str()) {
                    exact.push(entry);
                } else {
                    fuzzy.push(entry);
                }
            }
        }

        // Within each group sort by path length (shorter = better match).
        exact.sort_by_key(|s| s.len());
        fuzzy.sort_by_key(|s| s.len());

        self.filtered = exact
            .into_iter()
            .chain(fuzzy)
            .map(|s| s.to_string())
            .collect();

        // Clamp the selection so it stays valid after filtering.
        if self.filtered.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.filtered.len() - 1);
        }
    }
}

/// Returns `true` if every character of `query` appears in `text` in order
/// (both should already be lowercase).
fn is_fuzzy_match(text: &str, query: &str) -> bool {
    let mut text_chars = text.chars();
    'outer: for qc in query.chars() {
        for tc in text_chars.by_ref() {
            if tc == qc {
                continue 'outer;
            }
        }
        return false;
    }
    true
}

/// Recursively walk `dir`, appending paths relative to `root` into `out`.
/// Stops once `out` reaches [`MAX_FILES`].
fn collect_files(root: &PathBuf, dir: &PathBuf, out: &mut Vec<String>) {
    if out.len() >= MAX_FILES {
        return;
    }
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in read {
        if out.len() >= MAX_FILES {
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if path.is_dir() {
            if SKIP_DIRS.contains(&file_name.as_str()) {
                continue;
            }
            collect_files(root, &path, out);
        } else {
            let relative = path
                .strip_prefix(root)
                .ok()
                .and_then(|p| p.to_str())
                .map(|s| s.to_string());
            if let Some(rel) = relative {
                out.push(rel);
            }
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build a temporary directory tree under `std::env::temp_dir()` for
    /// testing and return the path. The caller is responsible for cleanup via
    /// `fs::remove_dir_all`.
    fn make_test_dir(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(name);
        // Clean up any leftover directory from a previous run.
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test dir");
        fs::write(root.join("main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("lib.rs"), "pub mod foo;").unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src").join("buffer.rs"), "// buffer").unwrap();
        fs::write(root.join("src").join("cursor.rs"), "// cursor").unwrap();
        root
    }

    #[test]
    fn test_filter_empty_query() {
        let root = make_test_dir("aura_fp_test_empty_query");
        let mut picker = FilePicker::new(root.clone());
        picker.open();
        // With an empty query all entries should be returned.
        assert_eq!(picker.filtered.len(), picker.entries.len());
        assert!(!picker.filtered.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_filter_exact_match() {
        let root = make_test_dir("aura_fp_test_exact_match");
        let mut picker = FilePicker::new(root.clone());
        picker.open();
        picker.type_char('m');
        picker.type_char('a');
        picker.type_char('i');
        picker.type_char('n');
        // "main.rs" should be in results.
        assert!(
            picker.filtered.iter().any(|f| f.contains("main")),
            "expected main.rs in filtered, got {:?}",
            picker.filtered
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_filter_fuzzy_match() {
        let root = make_test_dir("aura_fp_test_fuzzy_match");
        let mut picker = FilePicker::new(root.clone());
        picker.open();
        // "sr" should fuzzy-match "src/buffer.rs" and "src/cursor.rs".
        picker.type_char('s');
        picker.type_char('r');
        let src_matches: Vec<_> = picker
            .filtered
            .iter()
            .filter(|f| f.contains("src"))
            .collect();
        assert!(
            !src_matches.is_empty(),
            "expected src/ files in filtered, got {:?}",
            picker.filtered
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_select_navigation() {
        let root = make_test_dir("aura_fp_test_navigation");
        let mut picker = FilePicker::new(root.clone());
        picker.open();
        assert_eq!(picker.selected, 0);
        picker.select_down();
        assert_eq!(picker.selected, 1);
        picker.select_up();
        assert_eq!(picker.selected, 0);
        // Wrapping: select_up from 0 should wrap to last entry.
        picker.select_up();
        assert_eq!(picker.selected, picker.filtered.len() - 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn test_no_matches() {
        let root = make_test_dir("aura_fp_test_no_matches");
        let mut picker = FilePicker::new(root.clone());
        picker.open();
        // Type a query that won't match anything.
        for c in "zzzzzzzzzzzzz".chars() {
            picker.type_char(c);
        }
        assert!(picker.filtered.is_empty());
        // Navigation on empty list should not panic.
        picker.select_down();
        picker.select_up();
        assert_eq!(picker.selected, 0);
        assert!(picker.selected_path().is_none());
        let _ = fs::remove_dir_all(&root);
    }
}
