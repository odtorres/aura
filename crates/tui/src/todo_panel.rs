// `TodoItem.text` is captured during the workspace scan and shown in
// detail tooltips; the panel currently only renders title + tag, so the
// raw text field reads as unused at the type-system level.
#![allow(dead_code)]

//! TODO/FIXME aggregation panel.
//!
//! Scans the workspace for TODO, FIXME, HACK, XXX tags and displays
//! them in a navigable list.

use std::path::{Path, PathBuf};

/// A TODO/FIXME/HACK item found in the codebase.
#[derive(Debug, Clone)]
pub struct TodoItem {
    /// File path.
    pub file: PathBuf,
    /// Line number (0-indexed).
    pub line: usize,
    /// Tag type (TODO, FIXME, HACK, XXX).
    pub tag: String,
    /// The comment text after the tag.
    pub text: String,
}

/// Tags to scan for.
const TODO_TAGS: &[&str] = &["TODO", "FIXME", "HACK", "XXX", "BUG", "NOTE"];

/// Scan a directory for TODO items.
pub fn scan_directory(root: &Path) -> Vec<TodoItem> {
    let mut items = Vec::new();
    scan_recursive(root, root, &mut items);
    items.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    items
}

fn scan_recursive(root: &Path, dir: &Path, items: &mut Vec<TodoItem>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if path.is_dir() {
            if matches!(
                name,
                ".git" | "target" | "node_modules" | ".next" | "dist" | "build" | "__pycache__"
            ) {
                continue;
            }
            scan_recursive(root, &path, items);
        } else if is_source_file(&path) {
            scan_file(root, &path, items);
        }
    }
}

fn scan_file(root: &Path, file: &Path, items: &mut Vec<TodoItem>) {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return,
    };
    let rel = file.strip_prefix(root).unwrap_or(file).to_path_buf();

    for (line_num, line) in content.lines().enumerate() {
        for tag in TODO_TAGS {
            // Match patterns like: // TODO: text, # FIXME text, /* HACK: text
            if let Some(pos) = line.find(tag) {
                // Check it's in a comment context (preceded by //, #, /*, --, etc.).
                let before = &line[..pos];
                if before.contains("//")
                    || before.contains('#')
                    || before.contains("/*")
                    || before.contains("--")
                    || before.trim().is_empty()
                {
                    let after = &line[pos + tag.len()..];
                    let text = after
                        .trim_start_matches(':')
                        .trim_start_matches(' ')
                        .trim()
                        .to_string();
                    items.push(TodoItem {
                        file: rel.clone(),
                        line: line_num,
                        tag: tag.to_string(),
                        text,
                    });
                    break; // Only match first tag per line.
                }
            }
        }
    }
}

fn is_source_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            matches!(
                ext,
                "rs" | "py"
                    | "js"
                    | "ts"
                    | "tsx"
                    | "jsx"
                    | "go"
                    | "rb"
                    | "lua"
                    | "c"
                    | "cpp"
                    | "h"
                    | "java"
                    | "kt"
                    | "swift"
                    | "zig"
                    | "ex"
                    | "hs"
                    | "sh"
                    | "html"
                    | "css"
                    | "vue"
                    | "svelte"
                    | "php"
                    | "sql"
                    | "md"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_line() {
        let mut items = Vec::new();
        let tmp = std::env::temp_dir().join("aura_test_todo.rs");
        std::fs::write(
            &tmp,
            "// TODO: fix this\nlet x = 1;\n// FIXME: urgent\n# HACK: workaround\n",
        )
        .unwrap();
        scan_file(std::env::temp_dir().as_path(), &tmp, &mut items);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].tag, "TODO");
        assert_eq!(items[1].tag, "FIXME");
        assert_eq!(items[2].tag, "HACK");
        let _ = std::fs::remove_file(&tmp);
    }
}
