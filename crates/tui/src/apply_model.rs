// The SEARCH/REPLACE block parser is built and fully tested but not yet
// wired into the AI proposal flow — that integration is gated behind the
// "apply model" feature work and uses the existing diff-based flow in
// the meantime. Allow dead code at module scope so the parser stays in
// the tree without warnings until it ships.
#![allow(dead_code)]

//! Apply model — parse and apply structured AI edits to source files.
//!
//! The apply model takes AI-proposed changes as search/replace blocks
//! and merges them into source files reliably. This separates the
//! "thinking" model (which proposes changes) from the "apply" model
//! (which patches files), following the Cursor two-model pattern.

use std::path::{Path, PathBuf};

/// A single search/replace edit operation.
#[derive(Debug, Clone)]
pub struct EditBlock {
    /// Target file path.
    pub file: PathBuf,
    /// The text to search for (exact match).
    pub search: String,
    /// The replacement text.
    pub replace: String,
}

/// Parse AI output for search/replace blocks.
///
/// Expected format:
/// ```text
/// <<<< SEARCH file.rs
/// old code here
/// ====
/// new code here
/// >>>> REPLACE
/// ```
///
/// Also supports the simpler format:
/// ```text
/// --- file.rs
/// - old line
/// + new line
/// ```
pub fn parse_edit_blocks(ai_output: &str) -> Vec<EditBlock> {
    let mut blocks = Vec::new();

    // Parse <<<< SEARCH / ==== / >>>> REPLACE format.
    let mut lines = ai_output.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();

        // Format 1: <<<< SEARCH file.rs
        if let Some(rest) = trimmed.strip_prefix("<<<< SEARCH") {
            let file = rest.trim().to_string();
            let mut search = Vec::new();
            let mut replace = Vec::new();
            let mut in_replace = false;

            for inner_line in lines.by_ref() {
                let inner = inner_line.trim();
                if inner == "====" {
                    in_replace = true;
                } else if inner.starts_with(">>>> REPLACE") {
                    break;
                } else if in_replace {
                    replace.push(inner_line);
                } else {
                    search.push(inner_line);
                }
            }

            if !search.is_empty() {
                blocks.push(EditBlock {
                    file: PathBuf::from(file),
                    search: search.join("\n"),
                    replace: replace.join("\n"),
                });
            }
        }

        // Format 2: ```diff or unified diff style
        if trimmed.starts_with("--- a/") || trimmed.starts_with("--- ") {
            let file = trimmed
                .strip_prefix("--- a/")
                .or_else(|| trimmed.strip_prefix("--- "))
                .unwrap_or("")
                .trim()
                .to_string();

            // Skip the +++ line.
            if let Some(next) = lines.peek() {
                if next.starts_with("+++ ") {
                    lines.next();
                }
            }

            let mut search_lines = Vec::new();
            let mut replace_lines = Vec::new();

            for inner_line in lines.by_ref() {
                if inner_line.starts_with("--- ") || inner_line.starts_with("diff ") {
                    break;
                }
                if let Some(removed) = inner_line.strip_prefix('-') {
                    search_lines.push(removed);
                } else if let Some(added) = inner_line.strip_prefix('+') {
                    replace_lines.push(added);
                } else if let Some(context) = inner_line.strip_prefix(' ') {
                    search_lines.push(context);
                    replace_lines.push(context);
                }
            }

            if !search_lines.is_empty() && !file.is_empty() {
                blocks.push(EditBlock {
                    file: PathBuf::from(file),
                    search: search_lines.join("\n"),
                    replace: replace_lines.join("\n"),
                });
            }
        }
    }

    blocks
}

/// Apply edit blocks to files on disk.
///
/// Returns a list of (file, success, message) for each edit.
pub fn apply_edits(project_root: &Path, edits: &[EditBlock]) -> Vec<(PathBuf, bool, String)> {
    let mut results = Vec::new();

    for edit in edits {
        let full_path = project_root.join(&edit.file);
        match std::fs::read_to_string(&full_path) {
            Ok(content) => {
                if let Some(new_content) = content.replacen(&edit.search, &edit.replace, 1).into() {
                    // Check that the search text was actually found.
                    if new_content == content && !edit.search.is_empty() {
                        results.push((
                            edit.file.clone(),
                            false,
                            "Search text not found in file".to_string(),
                        ));
                    } else {
                        match std::fs::write(&full_path, &new_content) {
                            Ok(()) => {
                                results.push((edit.file.clone(), true, "Applied".to_string()))
                            }
                            Err(e) => results.push((
                                edit.file.clone(),
                                false,
                                format!("Write failed: {e}"),
                            )),
                        }
                    }
                }
            }
            Err(e) => {
                results.push((edit.file.clone(), false, format!("Read failed: {e}")));
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_search_replace() {
        let input = r#"
<<<< SEARCH main.rs
fn old_function() {
    println!("old");
}
====
fn new_function() {
    println!("new");
}
>>>> REPLACE
"#;
        let blocks = parse_edit_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].file, PathBuf::from("main.rs"));
        assert!(blocks[0].search.contains("old_function"));
        assert!(blocks[0].replace.contains("new_function"));
    }

    #[test]
    fn test_parse_multiple_blocks() {
        let input = r#"
<<<< SEARCH a.rs
old_a
====
new_a
>>>> REPLACE

<<<< SEARCH b.rs
old_b
====
new_b
>>>> REPLACE
"#;
        let blocks = parse_edit_blocks(input);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].file, PathBuf::from("a.rs"));
        assert_eq!(blocks[1].file, PathBuf::from("b.rs"));
    }
}
