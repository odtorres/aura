//! Context Pinning — pin files and symbols as always-included AI context.
//!
//! Pinned items persist across conversations and are automatically
//! injected into every AI prompt as additional context.

use std::path::PathBuf;

/// A pinned context item.
#[derive(Debug, Clone)]
pub enum PinnedContext {
    /// An entire file.
    File {
        /// File path.
        path: PathBuf,
    },
    /// A specific symbol or line range.
    Symbol {
        /// File path.
        path: PathBuf,
        /// Symbol name or description.
        name: String,
        /// Start line (0-indexed).
        start_line: usize,
        /// End line (0-indexed, exclusive).
        end_line: usize,
    },
    /// Free-text note (project context, conventions, etc.).
    Note {
        /// Note content.
        text: String,
    },
}

impl PinnedContext {
    /// Display label for the pinned item.
    pub fn label(&self) -> String {
        match self {
            Self::File { path } => format!(
                "{}",
                path.file_name().and_then(|n| n.to_str()).unwrap_or("?")
            ),
            Self::Symbol { name, path, .. } => format!(
                "{}:{}",
                path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
                name
            ),
            Self::Note { text } => {
                let preview: String = text.chars().take(30).collect();
                format!("note: {}", preview)
            }
        }
    }

    /// Get the full text content of this pinned item.
    pub fn content(&self) -> String {
        match self {
            Self::File { path } => std::fs::read_to_string(path).unwrap_or_default(),
            Self::Symbol {
                path,
                start_line,
                end_line,
                ..
            } => {
                let content = std::fs::read_to_string(path).unwrap_or_default();
                content
                    .lines()
                    .skip(*start_line)
                    .take(end_line.saturating_sub(*start_line))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            Self::Note { text } => text.clone(),
        }
    }
}

/// Manager for pinned context items.
#[derive(Debug, Default)]
pub struct ContextPinManager {
    /// All pinned items.
    pub pins: Vec<PinnedContext>,
}

impl ContextPinManager {
    /// Create a new manager.
    pub fn new() -> Self {
        Self { pins: Vec::new() }
    }

    /// Pin a file.
    pub fn pin_file(&mut self, path: PathBuf) {
        // Don't duplicate.
        if !self
            .pins
            .iter()
            .any(|p| matches!(p, PinnedContext::File { path: ref pp } if pp == &path))
        {
            self.pins.push(PinnedContext::File { path });
        }
    }

    /// Pin a note.
    pub fn pin_note(&mut self, text: String) {
        self.pins.push(PinnedContext::Note { text });
    }

    /// Unpin by index.
    pub fn unpin(&mut self, index: usize) -> bool {
        if index < self.pins.len() {
            self.pins.remove(index);
            true
        } else {
            false
        }
    }

    /// Clear all pins.
    pub fn clear(&mut self) {
        self.pins.clear();
    }

    /// Build the combined context string for AI prompts.
    pub fn build_context(&self) -> String {
        if self.pins.is_empty() {
            return String::new();
        }
        let mut ctx = String::from("\n\n--- Pinned Context ---\n");
        for pin in &self.pins {
            ctx.push_str(&format!("\n[{}]\n{}\n", pin.label(), pin.content()));
        }
        ctx
    }

    /// List all pins with labels.
    pub fn list_labels(&self) -> Vec<String> {
        self.pins.iter().map(|p| p.label()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pin_and_list() {
        let mut mgr = ContextPinManager::new();
        mgr.pin_note("Always use snake_case".to_string());
        mgr.pin_note("Prefer Result over unwrap".to_string());
        assert_eq!(mgr.pins.len(), 2);
        assert_eq!(mgr.list_labels().len(), 2);
    }

    #[test]
    fn test_unpin() {
        let mut mgr = ContextPinManager::new();
        mgr.pin_note("test".to_string());
        assert!(mgr.unpin(0));
        assert!(mgr.pins.is_empty());
    }

    #[test]
    fn test_build_context() {
        let mut mgr = ContextPinManager::new();
        mgr.pin_note("Always test".to_string());
        let ctx = mgr.build_context();
        assert!(ctx.contains("Pinned Context"));
        assert!(ctx.contains("Always test"));
    }
}
