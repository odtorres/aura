//! Branch picker modal for switching git branches.
//!
//! Opened by clicking the branch name in the status bar. Shows all local
//! branches with the current branch highlighted. Select and Enter to switch.

use crate::git::BranchInfo;

/// Branch picker modal state.
pub struct BranchPicker {
    /// Whether the picker is visible.
    pub visible: bool,
    /// All branches.
    pub branches: Vec<BranchInfo>,
    /// Currently selected index.
    pub selected: usize,
    /// Filter query.
    pub query: String,
    /// Filtered indices into `branches`.
    pub filtered: Vec<usize>,
}

impl BranchPicker {
    /// Create a new branch picker (initially hidden).
    pub fn new() -> Self {
        Self {
            visible: false,
            branches: Vec::new(),
            selected: 0,
            query: String::new(),
            filtered: Vec::new(),
        }
    }

    /// Open the picker with the given branch list.
    pub fn open(&mut self, branches: Vec<BranchInfo>) {
        self.branches = branches;
        self.query.clear();
        self.selected = 0;
        self.filter();
        // Pre-select the current branch.
        for (i, &idx) in self.filtered.iter().enumerate() {
            if self.branches[idx].is_current {
                self.selected = i;
                break;
            }
        }
        self.visible = true;
    }

    /// Close the picker.
    pub fn close(&mut self) {
        self.visible = false;
        self.query.clear();
    }

    /// Type a character into the filter.
    pub fn type_char(&mut self, c: char) {
        self.query.push(c);
        self.filter();
    }

    /// Delete last character from the filter.
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

    /// Get the selected branch name (if any).
    pub fn selected_branch(&self) -> Option<&str> {
        self.filtered
            .get(self.selected)
            .and_then(|&idx| self.branches.get(idx))
            .map(|b| b.name.as_str())
    }

    /// Apply the filter.
    fn filter(&mut self) {
        let query = self.query.to_lowercase();
        self.selected = 0;
        if query.is_empty() {
            self.filtered = (0..self.branches.len()).collect();
        } else {
            self.filtered = self
                .branches
                .iter()
                .enumerate()
                .filter(|(_, b)| b.name.to_lowercase().contains(&query))
                .map(|(i, _)| i)
                .collect();
        }
    }
}

impl Default for BranchPicker {
    fn default() -> Self {
        Self::new()
    }
}
