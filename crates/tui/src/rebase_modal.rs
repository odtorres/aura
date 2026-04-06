//! Interactive rebase modal for visual `git rebase -i`.
//!
//! Opens with `:rebase`. Shows commits with editable actions (pick, reword,
//! edit, squash, fixup, drop). Supports reordering via Alt+j/k.

use crate::git::GraphCommit;

/// Rebase action for each commit (mirrors git rebase -i todo keywords).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebaseAction {
    /// Use commit as-is.
    Pick,
    /// Use commit but edit the message.
    Reword,
    /// Use commit but pause for amending.
    Edit,
    /// Meld into previous commit (combine messages).
    Squash,
    /// Meld into previous commit (discard this message).
    Fixup,
    /// Remove commit entirely.
    Drop,
}

impl RebaseAction {
    /// Short label for display.
    pub fn label(self) -> &'static str {
        match self {
            Self::Pick => "pick",
            Self::Reword => "reword",
            Self::Edit => "edit",
            Self::Squash => "squash",
            Self::Fixup => "fixup",
            Self::Drop => "drop",
        }
    }

    /// Single-char shortcut.
    pub fn key(self) -> char {
        match self {
            Self::Pick => 'p',
            Self::Reword => 'r',
            Self::Edit => 'e',
            Self::Squash => 's',
            Self::Fixup => 'f',
            Self::Drop => 'd',
        }
    }
}

/// A commit entry in the rebase plan.
#[derive(Debug, Clone)]
pub struct RebaseEntry {
    /// The underlying commit data.
    pub commit: GraphCommit,
    /// The rebase action to apply.
    pub action: RebaseAction,
}

/// Interactive rebase modal state.
pub struct InteractiveRebaseModal {
    /// Whether the modal is visible.
    pub visible: bool,
    /// Rebase entries (oldest first — rebase order).
    pub entries: Vec<RebaseEntry>,
    /// Currently selected entry index.
    pub selected: usize,
    /// Base commit hash (the commit we rebase onto).
    pub base_hash: String,
    /// Status/error message displayed at the bottom.
    pub status: String,
}

impl InteractiveRebaseModal {
    /// Create a new modal (initially hidden).
    pub fn new() -> Self {
        Self {
            visible: false,
            entries: Vec::new(),
            selected: 0,
            base_hash: String::new(),
            status: String::new(),
        }
    }

    /// Open the modal with commits to rebase.
    ///
    /// `commits` should be newest-first (as returned by `graph_log`).
    /// `count` is how many commits from HEAD to include.
    /// They are reversed to oldest-first for the rebase todo order.
    pub fn open(&mut self, commits: Vec<GraphCommit>, count: usize) {
        let count = count.min(commits.len());
        if count == 0 {
            return;
        }
        // Base is the commit just before the range we're rebasing.
        self.base_hash = if count < commits.len() {
            commits[count].hash.clone()
        } else {
            // Rebasing all commits — use --root.
            String::new()
        };
        // Take the commits to rebase and reverse to oldest-first.
        self.entries = commits[..count]
            .iter()
            .rev()
            .map(|c| RebaseEntry {
                commit: c.clone(),
                action: RebaseAction::Pick,
            })
            .collect();
        self.selected = 0;
        self.status = format!(
            "p:pick r:reword e:edit s:squash f:fixup d:drop | Alt+j/k:reorder | w:execute q:abort"
        );
        self.visible = true;
    }

    /// Close the modal.
    pub fn close(&mut self) {
        self.visible = false;
        self.entries.clear();
    }

    /// Move selection down.
    pub fn select_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }

    /// Move selection up.
    pub fn select_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Move the selected entry down (reorder).
    pub fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.entries.swap(self.selected, self.selected + 1);
            self.selected += 1;
        }
    }

    /// Move the selected entry up (reorder).
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.entries.swap(self.selected, self.selected - 1);
            self.selected -= 1;
        }
    }

    /// Set the action for the selected entry.
    pub fn set_action(&mut self, action: RebaseAction) {
        if let Some(entry) = self.entries.get_mut(self.selected) {
            entry.action = action;
        }
    }

    /// Generate the rebase todo text (for `GIT_SEQUENCE_EDITOR`).
    pub fn generate_todo(&self) -> String {
        self.entries
            .iter()
            .map(|e| {
                format!(
                    "{} {} {}",
                    e.action.label(),
                    e.commit.short,
                    e.commit.summary
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Get the base ref for `git rebase -i <base>`.
    /// Returns `--root` if rebasing all commits.
    pub fn base_ref(&self) -> &str {
        if self.base_hash.is_empty() {
            "--root"
        } else {
            &self.base_hash
        }
    }
}

impl Default for InteractiveRebaseModal {
    fn default() -> Self {
        Self::new()
    }
}
