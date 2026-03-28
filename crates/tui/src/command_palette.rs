//! VS Code-style command palette overlay.
//!
//! Provides a unified search interface for commands, files, and settings.
//! Opens with `Ctrl+P` and supports fuzzy filtering.

/// An item that can appear in the command palette.
#[derive(Debug, Clone)]
pub enum PaletteItem {
    /// An editor command (e.g., `:w`, `:q`).
    Command {
        /// Command string to execute.
        id: String,
        /// Human-readable label.
        label: String,
    },
    /// A file in the workspace.
    File {
        /// Relative path to the file.
        path: String,
    },
    /// A setting that can be toggled.
    Setting {
        /// Setting key (e.g., "editor.show_minimap").
        key: String,
        /// Human-readable label.
        label: String,
    },
}

impl PaletteItem {
    /// Get the display text for this item.
    pub fn display_text(&self) -> &str {
        match self {
            PaletteItem::Command { label, .. } => label,
            PaletteItem::File { path } => path,
            PaletteItem::Setting { label, .. } => label,
        }
    }

    /// Get the search text (used for fuzzy matching).
    pub fn search_text(&self) -> String {
        match self {
            PaletteItem::Command { id, label } => format!("{label} {id}"),
            PaletteItem::File { path } => path.clone(),
            PaletteItem::Setting { label, key } => format!("{label} {key}"),
        }
    }

    /// Get the type badge for display.
    pub fn badge(&self) -> &str {
        match self {
            PaletteItem::Command { .. } => "cmd",
            PaletteItem::File { .. } => "file",
            PaletteItem::Setting { .. } => "set",
        }
    }
}

/// The command palette overlay state.
pub struct CommandPalette {
    /// Whether the palette is visible.
    pub visible: bool,
    /// Current search query.
    pub query: String,
    /// All available items.
    items: Vec<PaletteItem>,
    /// Indices into `items` matching the current query.
    pub filtered: Vec<usize>,
    /// Currently selected index in the filtered list.
    pub selected: usize,
}

impl CommandPalette {
    /// Create a new command palette (initially hidden).
    pub fn new() -> Self {
        Self {
            visible: false,
            query: String::new(),
            items: Vec::new(),
            filtered: Vec::new(),
            selected: 0,
        }
    }

    /// Open the palette and populate items from the current editor state.
    pub fn open(
        &mut self,
        commands: Vec<PaletteItem>,
        files: Vec<PaletteItem>,
        settings: Vec<PaletteItem>,
    ) {
        self.items.clear();
        self.items.extend(commands);
        self.items.extend(files);
        self.items.extend(settings);
        self.query.clear();
        self.selected = 0;
        self.filter();
        self.visible = true;
    }

    /// Close the palette.
    pub fn close(&mut self) {
        self.visible = false;
        self.query.clear();
    }

    /// Type a character into the query.
    pub fn type_char(&mut self, c: char) {
        self.query.push(c);
        self.filter();
    }

    /// Delete the last character from the query.
    pub fn backspace(&mut self) {
        self.query.pop();
        self.filter();
    }

    /// Move selection down.
    pub fn select_down(&mut self) {
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    /// Move selection up.
    pub fn select_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Get the currently selected item (if any).
    pub fn selected_item(&self) -> Option<&PaletteItem> {
        self.filtered
            .get(self.selected)
            .and_then(|&idx| self.items.get(idx))
    }

    /// Get all items (for rendering).
    pub fn items(&self) -> &[PaletteItem] {
        &self.items
    }

    /// Re-filter items based on the current query.
    fn filter(&mut self) {
        let query = self.query.to_lowercase();
        self.selected = 0;

        if query.is_empty() {
            self.filtered = (0..self.items.len()).collect();
            return;
        }

        // Score and sort: exact substring > fuzzy.
        let mut exact: Vec<usize> = Vec::new();
        let mut fuzzy: Vec<usize> = Vec::new();

        for (i, item) in self.items.iter().enumerate() {
            let text = item.search_text().to_lowercase();
            if text.contains(&query) {
                exact.push(i);
            } else if is_fuzzy_match(&text, &query) {
                fuzzy.push(i);
            }
        }

        // Sort exact matches by display text length (shorter = better).
        exact.sort_by_key(|&i| self.items[i].display_text().len());
        fuzzy.sort_by_key(|&i| self.items[i].display_text().len());

        self.filtered = exact;
        self.filtered.extend(fuzzy);
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if `text` is a fuzzy match for `query`.
/// Every character of query must appear in text in order.
fn is_fuzzy_match(text: &str, query: &str) -> bool {
    let mut text_chars = text.chars();
    for qc in query.chars() {
        loop {
            match text_chars.next() {
                Some(tc) if tc == qc => break,
                Some(_) => continue,
                None => return false,
            }
        }
    }
    true
}

/// Build the list of editor commands for the palette.
pub fn editor_commands() -> Vec<PaletteItem> {
    vec![
        PaletteItem::Command {
            id: "w".into(),
            label: "Save (:w)".into(),
        },
        PaletteItem::Command {
            id: "q".into(),
            label: "Quit (:q)".into(),
        },
        PaletteItem::Command {
            id: "wq".into(),
            label: "Save & Quit (:wq)".into(),
        },
        PaletteItem::Command {
            id: "q!".into(),
            label: "Force Quit (:q!)".into(),
        },
        PaletteItem::Command {
            id: "qa".into(),
            label: "Quit All (:qa)".into(),
        },
        PaletteItem::Command {
            id: "wqa".into(),
            label: "Save All & Quit (:wqa)".into(),
        },
        PaletteItem::Command {
            id: "vsplit".into(),
            label: "Vertical Split (:vsplit)".into(),
        },
        PaletteItem::Command {
            id: "hsplit".into(),
            label: "Horizontal Split (:hsplit)".into(),
        },
        PaletteItem::Command {
            id: "only".into(),
            label: "Close Split (:only)".into(),
        },
        PaletteItem::Command {
            id: "settings".into(),
            label: "Settings (:settings)".into(),
        },
        PaletteItem::Command {
            id: "compact".into(),
            label: "Compact Conversations (:compact)".into(),
        },
        PaletteItem::Command {
            id: "host".into(),
            label: "Host Collab Session (:host)".into(),
        },
        PaletteItem::Command {
            id: "collab-stop".into(),
            label: "Stop Collab (:collab-stop)".into(),
        },
        PaletteItem::Command {
            id: "commit".into(),
            label: "AI Commit Message (:commit)".into(),
        },
        PaletteItem::Command {
            id: "blame".into(),
            label: "Toggle Blame (:blame)".into(),
        },
        PaletteItem::Command {
            id: "help".into(),
            label: "Help (:help)".into(),
        },
        PaletteItem::Command {
            id: "plugins".into(),
            label: "List Plugins (:plugins)".into(),
        },
        PaletteItem::Command {
            id: "update".into(),
            label: "Check for Updates (:update)".into(),
        },
        PaletteItem::Command {
            id: "version".into(),
            label: "Show Version (:version)".into(),
        },
        PaletteItem::Command {
            id: "tree".into(),
            label: "Toggle File Tree (:tree)".into(),
        },
        PaletteItem::Command {
            id: "term".into(),
            label: "Toggle Terminal (:term)".into(),
        },
        PaletteItem::Command {
            id: "chat".into(),
            label: "Toggle Chat Panel (:chat)".into(),
        },
        PaletteItem::Command {
            id: "files".into(),
            label: "File Picker (:files)".into(),
        },
        PaletteItem::Command {
            id: "intent".into(),
            label: "AI Intent Mode (:intent)".into(),
        },
    ]
}

/// Build settings items for the palette.
pub fn settings_items(config: &crate::config::AuraConfig) -> Vec<PaletteItem> {
    vec![
        PaletteItem::Setting {
            key: "editor.show_minimap".into(),
            label: format!(
                "Minimap: {}",
                if config.editor.show_minimap {
                    "on"
                } else {
                    "off"
                }
            ),
        },
        PaletteItem::Setting {
            key: "editor.line_numbers".into(),
            label: format!(
                "Line Numbers: {}",
                if config.editor.line_numbers {
                    "on"
                } else {
                    "off"
                }
            ),
        },
        PaletteItem::Setting {
            key: "editor.show_authorship".into(),
            label: format!(
                "Authorship: {}",
                if config.editor.show_authorship {
                    "on"
                } else {
                    "off"
                }
            ),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_match() {
        assert!(is_fuzzy_match("hello world", "hlo"));
        assert!(is_fuzzy_match("settings", "set"));
        assert!(!is_fuzzy_match("hello", "xyz"));
        assert!(is_fuzzy_match("src/main.rs", "smr"));
    }

    #[test]
    fn test_filter_empty_query() {
        let mut palette = CommandPalette::new();
        palette.items = vec![
            PaletteItem::Command {
                id: "w".into(),
                label: "Save".into(),
            },
            PaletteItem::File {
                path: "test.rs".into(),
            },
        ];
        palette.filter();
        assert_eq!(palette.filtered.len(), 2);
    }

    #[test]
    fn test_filter_exact_match() {
        let mut palette = CommandPalette::new();
        palette.items = vec![
            PaletteItem::Command {
                id: "w".into(),
                label: "Save".into(),
            },
            PaletteItem::Command {
                id: "q".into(),
                label: "Quit".into(),
            },
        ];
        palette.query = "save".into();
        palette.filter();
        assert_eq!(palette.filtered.len(), 1);
        assert_eq!(palette.items[palette.filtered[0]].display_text(), "Save");
    }
}
