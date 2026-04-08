//! Bookmark system — named bookmarks across files, persistent across sessions.
//!
//! Bookmarks are saved to `~/.aura/bookmarks.json` and restored on startup.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A single bookmark.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    /// Bookmark name.
    pub name: String,
    /// File path.
    pub file: PathBuf,
    /// Line number (0-indexed).
    pub line: usize,
    /// Column number (0-indexed).
    pub col: usize,
}

/// Bookmark manager — stores and persists named bookmarks.
#[derive(Debug, Default)]
pub struct BookmarkManager {
    /// All bookmarks, keyed by name.
    bookmarks: BTreeMap<String, Bookmark>,
}

impl BookmarkManager {
    /// Create a new bookmark manager, loading from disk.
    pub fn new() -> Self {
        let mut mgr = Self {
            bookmarks: BTreeMap::new(),
        };
        mgr.load();
        mgr
    }

    /// Add or update a bookmark.
    pub fn add(&mut self, name: String, file: PathBuf, line: usize, col: usize) {
        self.bookmarks.insert(
            name.clone(),
            Bookmark {
                name,
                file,
                line,
                col,
            },
        );
        self.save();
    }

    /// Remove a bookmark by name.
    pub fn remove(&mut self, name: &str) -> bool {
        let removed = self.bookmarks.remove(name).is_some();
        if removed {
            self.save();
        }
        removed
    }

    /// Get a bookmark by name.
    pub fn get(&self, name: &str) -> Option<&Bookmark> {
        self.bookmarks.get(name)
    }

    /// List all bookmarks sorted by name.
    pub fn list(&self) -> Vec<&Bookmark> {
        self.bookmarks.values().collect()
    }

    /// Get bookmarks for a specific file (for gutter rendering).
    pub fn for_file(&self, file: &std::path::Path) -> Vec<&Bookmark> {
        self.bookmarks.values().filter(|b| b.file == file).collect()
    }

    /// Persistence path.
    fn storage_path() -> PathBuf {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".aura/bookmarks.json")
    }

    /// Save bookmarks to disk.
    fn save(&self) {
        let path = Self::storage_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let bookmarks: Vec<&Bookmark> = self.bookmarks.values().collect();
        if let Ok(json) = serde_json::to_string_pretty(&bookmarks) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// Load bookmarks from disk.
    fn load(&mut self) {
        let path = Self::storage_path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(bookmarks) = serde_json::from_str::<Vec<Bookmark>>(&content) {
                for bm in bookmarks {
                    self.bookmarks.insert(bm.name.clone(), bm);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_get() {
        let mut mgr = BookmarkManager::default();
        mgr.add("test".to_string(), PathBuf::from("/tmp/test.rs"), 10, 5);
        let bm = mgr.get("test").unwrap();
        assert_eq!(bm.line, 10);
        assert_eq!(bm.col, 5);
    }

    #[test]
    fn test_remove() {
        let mut mgr = BookmarkManager::default();
        mgr.add("x".to_string(), PathBuf::from("/tmp/x.rs"), 0, 0);
        assert!(mgr.remove("x"));
        assert!(!mgr.remove("x"));
        assert!(mgr.get("x").is_none());
    }

    #[test]
    fn test_list() {
        let mut mgr = BookmarkManager::default();
        mgr.add("b".to_string(), PathBuf::from("/b"), 0, 0);
        mgr.add("a".to_string(), PathBuf::from("/a"), 0, 0);
        let list = mgr.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "a"); // BTreeMap sorts by key
    }
}
