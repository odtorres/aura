//! Project-wide search and replace.
//!
//! Scans all files in the project for a query string, displays results
//! grouped by file with match highlighting, and supports batch replace.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Directories to skip when scanning.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".aura",
    ".claude",
    "__pycache__",
    ".next",
    "dist",
    "build",
    ".venv",
    "venv",
];

/// Maximum number of results to return.
const MAX_RESULTS: usize = 1000;

/// Maximum file size to search (skip large binary files).
const MAX_FILE_SIZE: u64 = 5_000_000; // 5 MB

/// A single search match.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Relative file path.
    pub file_path: String,
    /// 1-indexed line number.
    pub line_number: usize,
    /// 0-indexed column where the match starts.
    pub column: usize,
    /// Full text of the line containing the match.
    pub line_text: String,
    /// Char offset of match start within `line_text`.
    pub match_start: usize,
    /// Char offset of match end within `line_text`.
    pub match_end: usize,
}

/// Which input field is focused in the search panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchFocus {
    /// The search query input.
    Query,
    /// The replace text input.
    Replace,
    /// The results list.
    Results,
}

/// State for the project-wide search/replace panel.
pub struct ProjectSearchPanel {
    /// Whether the panel is visible.
    pub visible: bool,
    /// Search query text.
    pub query: String,
    /// Replace text.
    pub replace_text: String,
    /// Whether replace input is shown.
    pub replace_mode: bool,
    /// Search results.
    pub results: Vec<SearchResult>,
    /// Currently selected result index.
    pub selected: usize,
    /// Scroll offset for results.
    pub scroll: usize,
    /// Case-sensitive search toggle.
    pub case_sensitive: bool,
    /// Total number of matches found.
    pub total_matches: usize,
    /// Number of unique files with matches.
    pub file_count: usize,
    /// Which field has focus.
    pub focus: SearchFocus,
    /// Project root path.
    pub root: PathBuf,
}

impl Default for ProjectSearchPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectSearchPanel {
    /// Create a new panel.
    pub fn new() -> Self {
        Self {
            visible: false,
            query: String::new(),
            replace_text: String::new(),
            replace_mode: false,
            results: Vec::new(),
            selected: 0,
            scroll: 0,
            case_sensitive: false,
            total_matches: 0,
            file_count: 0,
            focus: SearchFocus::Query,
            root: PathBuf::new(),
        }
    }

    /// Open the panel.
    pub fn open(&mut self, root: PathBuf) {
        self.visible = true;
        self.root = root;
        self.focus = SearchFocus::Query;
    }

    /// Close the panel.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Navigate up in results.
    pub fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Navigate down in results.
    pub fn select_down(&mut self) {
        let max = self.results.len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
    }

    /// Page up (10 results).
    pub fn page_up(&mut self) {
        self.selected = self.selected.saturating_sub(10);
    }

    /// Page down (10 results).
    pub fn page_down(&mut self) {
        let max = self.results.len().saturating_sub(1);
        self.selected = (self.selected + 10).min(max);
    }

    /// Toggle case sensitivity.
    pub fn toggle_case_sensitive(&mut self) {
        self.case_sensitive = !self.case_sensitive;
    }

    /// Toggle replace mode.
    pub fn toggle_replace_mode(&mut self) {
        self.replace_mode = !self.replace_mode;
        if self.replace_mode {
            self.focus = SearchFocus::Replace;
        } else {
            self.focus = SearchFocus::Query;
        }
    }

    /// Cycle focus: Query → Replace (if shown) → Results → Query.
    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            SearchFocus::Query => {
                if self.replace_mode {
                    SearchFocus::Replace
                } else {
                    SearchFocus::Results
                }
            }
            SearchFocus::Replace => SearchFocus::Results,
            SearchFocus::Results => SearchFocus::Query,
        };
    }

    /// Execute the search.
    pub fn execute(&mut self) {
        if self.query.is_empty() {
            self.results.clear();
            self.total_matches = 0;
            self.file_count = 0;
            return;
        }
        self.results = search_project(&self.root, &self.query, self.case_sensitive);
        self.total_matches = self.results.len();

        let mut files: HashSet<&str> = HashSet::new();
        for r in &self.results {
            files.insert(&r.file_path);
        }
        self.file_count = files.len();
        self.selected = 0;
        self.scroll = 0;
    }

    /// Get the selected result.
    pub fn selected_result(&self) -> Option<&SearchResult> {
        self.results.get(self.selected)
    }

    /// Type a character into the active input field.
    pub fn type_char(&mut self, c: char) {
        match self.focus {
            SearchFocus::Query => self.query.push(c),
            SearchFocus::Replace => self.replace_text.push(c),
            SearchFocus::Results => {}
        }
    }

    /// Backspace in the active input field.
    pub fn backspace(&mut self) {
        match self.focus {
            SearchFocus::Query => {
                self.query.pop();
            }
            SearchFocus::Replace => {
                self.replace_text.pop();
            }
            SearchFocus::Results => {}
        }
    }
}

/// Search all project files for a query string.
pub fn search_project(root: &Path, query: &str, case_sensitive: bool) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let files = collect_files(root);
    let query_lower = if case_sensitive {
        String::new()
    } else {
        query.to_lowercase()
    };

    for file_path in &files {
        if results.len() >= MAX_RESULTS {
            break;
        }

        // Skip large files.
        if let Ok(meta) = std::fs::metadata(file_path) {
            if meta.len() > MAX_FILE_SIZE {
                continue;
            }
        }

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => continue, // Skip binary/unreadable files.
        };

        let rel_path = file_path
            .strip_prefix(root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        for (line_idx, line) in content.lines().enumerate() {
            if results.len() >= MAX_RESULTS {
                break;
            }

            let matches: Vec<usize> = if case_sensitive {
                line.match_indices(query).map(|(pos, _)| pos).collect()
            } else {
                let line_lower = line.to_lowercase();
                line_lower
                    .match_indices(&query_lower)
                    .map(|(pos, _)| pos)
                    .collect()
            };

            for byte_offset in matches {
                if results.len() >= MAX_RESULTS {
                    break;
                }
                // Convert byte offset to char offset.
                let char_start = line[..byte_offset].chars().count();
                let char_end = char_start + query.chars().count();

                results.push(SearchResult {
                    file_path: rel_path.clone(),
                    line_number: line_idx + 1,
                    column: char_start,
                    line_text: line.to_string(),
                    match_start: char_start,
                    match_end: char_end,
                });
            }
        }
    }

    results
}

/// Replace all occurrences of `old` with `new` across project files.
///
/// Returns (files_changed, total_replacements).
pub fn replace_in_files(root: &Path, old: &str, new: &str, case_sensitive: bool) -> (usize, usize) {
    let files = collect_files(root);
    let mut files_changed = 0;
    let mut total_replacements = 0;

    for file_path in &files {
        if let Ok(meta) = std::fs::metadata(file_path) {
            if meta.len() > MAX_FILE_SIZE {
                continue;
            }
        }

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let new_content = if case_sensitive {
            content.replace(old, new)
        } else {
            // Case-insensitive replace.
            let mut result = String::with_capacity(content.len());
            let lower_content = content.to_lowercase();
            let lower_query = old.to_lowercase();
            let mut last_end = 0;
            for (start, _) in lower_content.match_indices(&lower_query) {
                result.push_str(&content[last_end..start]);
                result.push_str(new);
                last_end = start + old.len();
                total_replacements += 1;
            }
            result.push_str(&content[last_end..]);
            if last_end == 0 {
                continue; // No matches in this file.
            }
            result
        };

        if case_sensitive {
            let count = content.matches(old).count();
            if count == 0 {
                continue;
            }
            total_replacements += count;
        }

        if new_content != content && std::fs::write(file_path, &new_content).is_ok() {
            files_changed += 1;
        }
    }

    (files_changed, total_replacements)
}

/// Recursively collect all files in a directory, skipping noise.
fn collect_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files_recursive(root, &mut files);
    files.sort();
    files
}

fn collect_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };

        if path.is_dir() {
            if SKIP_DIRS.contains(&name) {
                continue;
            }
            collect_files_recursive(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_search_finds_matches() {
        let dir = std::env::temp_dir().join("aura_search_test");
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("test.txt"), "hello world\nfoo bar\nhello again").unwrap();

        let results = search_project(&dir, "hello", false);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].line_number, 1);
        assert_eq!(results[1].line_number, 3);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_search_case_insensitive() {
        let dir = std::env::temp_dir().join("aura_search_case_test");
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("test.txt"), "Hello HELLO hello").unwrap();

        let results = search_project(&dir, "hello", false);
        assert_eq!(results.len(), 3);

        let results_cs = search_project(&dir, "hello", true);
        assert_eq!(results_cs.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_replace_in_files() {
        let dir = std::env::temp_dir().join("aura_replace_test");
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("test.txt"), "foo bar foo").unwrap();

        let (files, count) = replace_in_files(&dir, "foo", "baz", true);
        assert_eq!(files, 1);
        assert_eq!(count, 2);

        let content = fs::read_to_string(dir.join("test.txt")).unwrap();
        assert_eq!(content, "baz bar baz");

        let _ = fs::remove_dir_all(&dir);
    }
}
