//! File tree sidebar plugin for AURA.
//!
//! Provides a collapsible file tree panel rendered on the left side of the editor.
//! Supports keyboard navigation (select up/down) and directory expand/collapse.

use std::path::{Path, PathBuf};

/// A single entry in the file tree — either a file or a directory.
#[derive(Debug, Clone)]
pub struct FileTreeEntry {
    /// Display name (file/dir name only).
    pub name: String,
    /// Full path.
    pub path: PathBuf,
    /// Whether this is a directory.
    pub is_dir: bool,
    /// Nesting depth for indentation.
    pub depth: usize,
    /// Whether this directory is expanded (only relevant for dirs).
    pub expanded: bool,
}

/// The file tree sidebar state.
pub struct FileTree {
    /// Whether the tree is visible.
    pub visible: bool,
    /// Flat list of visible entries (expanded tree).
    pub entries: Vec<FileTreeEntry>,
    /// Currently selected index.
    pub selected: usize,
    /// Root directory.
    root: PathBuf,
    /// Width of the tree panel in columns.
    pub width: u16,
}

impl FileTree {
    /// Create a new file tree rooted at `root`, scanning one level deep.
    ///
    /// Directories are listed before files, both sorted alphabetically.
    /// Hidden directories and common noise directories (`.git`, `target`,
    /// `node_modules`, `.aura`) are omitted.
    pub fn new(root: PathBuf) -> Self {
        let mut tree = Self {
            visible: false,
            entries: Vec::new(),
            selected: 0,
            root,
            width: 30,
        };
        tree.refresh();
        tree
    }

    /// Toggle tree visibility. When becoming visible, refreshes entries.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible {
            self.refresh();
        }
    }

    /// Move selection up by one entry (clamped at 0).
    pub fn select_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Move selection down by one entry (clamped at last index).
    pub fn select_down(&mut self) {
        if !self.entries.is_empty() {
            self.selected = (self.selected + 1).min(self.entries.len() - 1);
        }
    }

    /// Toggle expansion of the currently selected directory entry.
    ///
    /// If the selected entry is a collapsed directory, its children are
    /// inserted directly after it. If it is an expanded directory, all
    /// descendant entries are removed.
    pub fn toggle_expand(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let idx = self.selected;
        if !self.entries[idx].is_dir {
            return;
        }

        if self.entries[idx].expanded {
            // Collapse: remove all descendants (entries with depth > this one's depth).
            self.entries[idx].expanded = false;
            let depth = self.entries[idx].depth;
            let mut end = idx + 1;
            while end < self.entries.len() && self.entries[end].depth > depth {
                end += 1;
            }
            self.entries.drain(idx + 1..end);
        } else {
            // Expand: scan one level and insert children after this entry.
            self.entries[idx].expanded = true;
            let path = self.entries[idx].path.clone();
            let child_depth = self.entries[idx].depth + 1;
            let children = self.scan_dir(&path, child_depth);
            let insert_pos = idx + 1;
            // Insert in reverse order so they land in the right sequence.
            for (i, child) in children.into_iter().enumerate() {
                self.entries.insert(insert_pos + i, child);
            }
        }
    }

    /// Return the path of the selected entry if it is a file (not a dir).
    pub fn selected_path(&self) -> Option<&Path> {
        self.entries
            .get(self.selected)
            .filter(|e| !e.is_dir)
            .map(|e| e.path.as_path())
    }

    /// Scan one level of `path`, returning entries sorted dirs-first, then
    /// files, each group in alphabetical order. Hidden entries and common
    /// noise directories are skipped.
    fn scan_dir(&self, path: &Path, depth: usize) -> Vec<FileTreeEntry> {
        let read_dir = match std::fs::read_dir(path) {
            Ok(rd) => rd,
            Err(_) => return Vec::new(),
        };

        let mut dirs: Vec<FileTreeEntry> = Vec::new();
        let mut files: Vec<FileTreeEntry> = Vec::new();

        for entry in read_dir.flatten() {
            let entry_path = entry.path();
            let name = match entry_path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            // Skip hidden entries and common noise directories.
            if name.starts_with('.') {
                continue;
            }
            if name == "target" || name == "node_modules" {
                continue;
            }

            let is_dir = entry_path.is_dir();
            let tree_entry = FileTreeEntry {
                name,
                path: entry_path,
                is_dir,
                depth,
                expanded: false,
            };

            if is_dir {
                dirs.push(tree_entry);
            } else {
                files.push(tree_entry);
            }
        }

        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        dirs.into_iter().chain(files).collect()
    }

    /// Rescan the full tree, preserving expansion state where possible.
    ///
    /// After refreshing, the selected index is clamped so it remains valid.
    pub fn refresh(&mut self) {
        // Collect currently expanded paths so we can re-expand them.
        let expanded_paths: std::collections::HashSet<PathBuf> = self
            .entries
            .iter()
            .filter(|e| e.is_dir && e.expanded)
            .map(|e| e.path.clone())
            .collect();

        self.entries = self.build_entries(&self.root.clone(), 0, &expanded_paths);

        // Clamp selected index.
        if !self.entries.is_empty() {
            self.selected = self.selected.min(self.entries.len() - 1);
        } else {
            self.selected = 0;
        }
    }

    /// Recursively build entries for `path`, re-expanding previously expanded dirs.
    fn build_entries(
        &self,
        path: &Path,
        depth: usize,
        expanded_paths: &std::collections::HashSet<PathBuf>,
    ) -> Vec<FileTreeEntry> {
        let top_level = self.scan_dir(path, depth);
        let mut result = Vec::new();

        for mut entry in top_level {
            let should_expand = entry.is_dir && expanded_paths.contains(&entry.path);
            if should_expand {
                entry.expanded = true;
                let children = self.build_entries(&entry.path.clone(), depth + 1, expanded_paths);
                result.push(entry);
                result.extend(children);
            } else {
                result.push(entry);
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Create a uniquely-named temp directory under the system temp path.
    /// Caller is responsible for cleaning up with `fs::remove_dir_all`.
    fn make_temp_dir() -> PathBuf {
        let base = std::env::temp_dir();
        // Use process ID + a counter so parallel tests don't collide.
        let unique = format!(
            "aura_file_tree_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
        );
        let path = base.join(unique);
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    #[test]
    fn test_new_empty_dir() {
        let dir = make_temp_dir();
        let tree = FileTree::new(dir.clone());
        // Empty directory means no entries.
        assert!(tree.entries.is_empty());
        assert_eq!(tree.selected, 0);
        assert_eq!(tree.width, 30);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_toggle_visibility() {
        let dir = make_temp_dir();
        let mut tree = FileTree::new(dir.clone());
        assert!(!tree.visible);

        tree.toggle();
        assert!(tree.visible);

        tree.toggle();
        assert!(!tree.visible);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_select_navigation() {
        let dir = make_temp_dir();
        // Create a few files.
        fs::write(dir.join("aaa.txt"), "").unwrap();
        fs::write(dir.join("bbb.txt"), "").unwrap();
        fs::write(dir.join("ccc.txt"), "").unwrap();

        let mut tree = FileTree::new(dir.clone());
        assert_eq!(tree.selected, 0);

        tree.select_down();
        assert_eq!(tree.selected, 1);

        tree.select_down();
        assert_eq!(tree.selected, 2);

        // Clamp at end.
        tree.select_down();
        assert_eq!(tree.selected, 2);

        tree.select_up();
        assert_eq!(tree.selected, 1);

        // Clamp at beginning.
        tree.select_up();
        tree.select_up();
        assert_eq!(tree.selected, 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
