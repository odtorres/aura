//! Source control panel — a sidebar view for staging, unstaging, and committing.

use crate::git::GitRepo;

/// Which sidebar view is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarView {
    /// The file tree explorer.
    Files,
    /// The git source control panel.
    Git,
}

/// Display status for a file in the git panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitFileStatus {
    /// File has been modified.
    Modified,
    /// File has been added (new, tracked).
    Added,
    /// File has been deleted.
    Deleted,
    /// File has been renamed.
    Renamed,
    /// File is untracked.
    Untracked,
}

impl GitFileStatus {
    /// Short label for display (single character).
    pub fn label(self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Added => "A",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Untracked => "?",
        }
    }
}

/// A file entry in the source control panel.
#[derive(Debug, Clone)]
pub struct GitFileEntry {
    /// Path relative to the repo root.
    pub rel_path: String,
    /// Filename only, for display.
    pub name: String,
    /// The file's status.
    pub status: GitFileStatus,
}

/// Which section of the git panel is focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitPanelSection {
    /// The commit message input area.
    CommitMessage,
    /// The staged files list.
    StagedFiles,
    /// The unstaged/changed files list.
    ChangedFiles,
}

/// State for the source control sidebar panel.
pub struct SourceControlPanel {
    /// Unstaged changed files.
    pub changed: Vec<GitFileEntry>,
    /// Staged files.
    pub staged: Vec<GitFileEntry>,
    /// The commit message being composed.
    pub commit_message: String,
    /// Which section currently has focus.
    pub focused_section: GitPanelSection,
    /// Selected index within the currently focused file list section.
    pub selected: usize,
    /// Width of the panel (matches file tree width).
    pub width: u16,
    /// Whether the user is actively editing the commit message.
    pub editing_commit_message: bool,
}

impl SourceControlPanel {
    /// Create a new source control panel with the given width.
    pub fn new(width: u16) -> Self {
        Self {
            changed: Vec::new(),
            staged: Vec::new(),
            commit_message: String::new(),
            focused_section: GitPanelSection::ChangedFiles,
            selected: 0,
            width,
            editing_commit_message: false,
        }
    }

    /// Refresh the panel's file lists from git status.
    pub fn refresh(&mut self, git_repo: &GitRepo) {
        self.changed.clear();
        self.staged.clear();

        let entries = match git_repo.file_status() {
            Ok(entries) => entries,
            Err(e) => {
                tracing::debug!("Failed to get git status: {}", e);
                return;
            }
        };

        for entry in entries {
            let name = entry
                .rel_path
                .rsplit('/')
                .next()
                .unwrap_or(&entry.rel_path)
                .to_string();

            // Index status (X) — staged changes.
            if entry.index_status != ' ' && entry.index_status != '?' {
                let status = char_to_status(entry.index_status);
                self.staged.push(GitFileEntry {
                    rel_path: entry.rel_path.clone(),
                    name: name.clone(),
                    status,
                });
            }

            // Worktree status (Y) — unstaged changes.
            if entry.worktree_status != ' ' {
                let status = if entry.worktree_status == '?' {
                    GitFileStatus::Untracked
                } else {
                    char_to_status(entry.worktree_status)
                };
                self.changed.push(GitFileEntry {
                    rel_path: entry.rel_path.clone(),
                    name,
                    status,
                });
            }
        }

        // Clamp selection to valid range.
        self.clamp_selected();
    }

    /// Move selection up within the current section.
    pub fn select_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Move selection down within the current section.
    pub fn select_down(&mut self) {
        let max = self.current_list_len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
    }

    /// Cycle to the next section.
    pub fn next_section(&mut self) {
        self.focused_section = match self.focused_section {
            GitPanelSection::CommitMessage => GitPanelSection::StagedFiles,
            GitPanelSection::StagedFiles => GitPanelSection::ChangedFiles,
            GitPanelSection::ChangedFiles => GitPanelSection::CommitMessage,
        };
        self.selected = 0;
    }

    /// Cycle to the previous section.
    pub fn prev_section(&mut self) {
        self.focused_section = match self.focused_section {
            GitPanelSection::CommitMessage => GitPanelSection::ChangedFiles,
            GitPanelSection::StagedFiles => GitPanelSection::CommitMessage,
            GitPanelSection::ChangedFiles => GitPanelSection::StagedFiles,
        };
        self.selected = 0;
    }

    /// Stage the currently selected changed file.
    pub fn stage_selected(&mut self, git_repo: &GitRepo) {
        if self.focused_section != GitPanelSection::ChangedFiles {
            return;
        }
        if let Some(entry) = self.changed.get(self.selected) {
            let path = entry.rel_path.clone();
            if let Err(e) = git_repo.stage_file(&path) {
                tracing::warn!("Failed to stage {}: {}", path, e);
                return;
            }
            self.refresh(git_repo);
        }
    }

    /// Unstage the currently selected staged file.
    pub fn unstage_selected(&mut self, git_repo: &GitRepo) {
        if self.focused_section != GitPanelSection::StagedFiles {
            return;
        }
        if let Some(entry) = self.staged.get(self.selected) {
            let path = entry.rel_path.clone();
            if let Err(e) = git_repo.unstage_file(&path) {
                tracing::warn!("Failed to unstage {}: {}", path, e);
                return;
            }
            self.refresh(git_repo);
        }
    }

    /// Commit staged changes with the current commit message.
    /// Returns the commit hash on success, or an error message.
    pub fn commit(&mut self, git_repo: &GitRepo) -> Result<String, String> {
        let msg = self.commit_message.trim().to_string();
        if msg.is_empty() {
            return Err("Commit message is empty".to_string());
        }
        if self.staged.is_empty() {
            return Err("Nothing staged to commit".to_string());
        }

        match git_repo.commit_staged(&msg) {
            Ok(hash) => {
                self.commit_message.clear();
                self.refresh(git_repo);
                Ok(hash)
            }
            Err(e) => Err(format!("Commit failed: {}", e)),
        }
    }

    /// Get the path of the currently selected file entry, if any.
    pub fn selected_path(&self) -> Option<&str> {
        match self.focused_section {
            GitPanelSection::StagedFiles => self.staged.get(self.selected).map(|e| e.rel_path.as_str()),
            GitPanelSection::ChangedFiles => self.changed.get(self.selected).map(|e| e.rel_path.as_str()),
            GitPanelSection::CommitMessage => None,
        }
    }

    /// Number of items in the currently focused file list section.
    fn current_list_len(&self) -> usize {
        match self.focused_section {
            GitPanelSection::CommitMessage => 0,
            GitPanelSection::StagedFiles => self.staged.len(),
            GitPanelSection::ChangedFiles => self.changed.len(),
        }
    }

    /// Clamp selected index to the valid range for the current section.
    fn clamp_selected(&mut self) {
        let max = self.current_list_len().saturating_sub(1);
        if self.selected > max {
            self.selected = max;
        }
    }
}

/// Map a porcelain status character to our enum.
fn char_to_status(c: char) -> GitFileStatus {
    match c {
        'M' => GitFileStatus::Modified,
        'A' => GitFileStatus::Added,
        'D' => GitFileStatus::Deleted,
        'R' => GitFileStatus::Renamed,
        '?' => GitFileStatus::Untracked,
        _ => GitFileStatus::Modified,
    }
}
