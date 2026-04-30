//! Source control panel — a sidebar view for staging, unstaging, and committing.

use crate::git::GitRepo;
use crate::git_worker::{GitCommand, GitEvent, GitRefreshKind, GitWorker};
use std::time::Instant;

/// Per-section refresh state. Tracks whether a request is in flight so
/// the panel can render a "refreshing…" indicator and coalesce overlapping
/// 2 s tick requests onto a single worker invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshState {
    /// No request outstanding.
    Idle,
    /// A request is in flight; `since` is when it was sent.
    InFlight {
        /// When the request was sent — used to render an elapsed time.
        since: Instant,
    },
    /// A new request arrived while one was in flight; re-issue when the
    /// in-flight one returns so the panel reflects the latest state.
    Stale,
}

impl Default for RefreshState {
    fn default() -> Self {
        Self::Idle
    }
}

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
    /// File has merge conflicts.
    Conflict,
}

impl GitFileStatus {
    /// Short label for display (single character).
    pub fn label(self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Added => "A",
            Self::Deleted => "D",
            Self::Conflict => "C",
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
    /// Merge conflict files.
    MergeChanges,
    /// The staged files list.
    StagedFiles,
    /// The unstaged/changed files list.
    ChangedFiles,
    /// Git stashes.
    Stashes,
}

/// A git stash entry.
#[derive(Debug, Clone)]
pub struct StashEntry {
    /// Stash name (e.g. "stash@{0}").
    pub name: String,
    /// Stash message.
    pub message: String,
}

/// State for the source control sidebar panel.
pub struct SourceControlPanel {
    /// Git stashes.
    pub stashes: Vec<StashEntry>,
    /// Files with merge conflicts.
    pub merge_changes: Vec<GitFileEntry>,
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
    /// When `Some`, the user pressed `d` on an unstaged file and we're waiting for confirmation.
    pub pending_discard: Option<String>,
    /// When `Some`, the user pressed `d` on a staged file — unstage then discard.
    pub pending_discard_staged: Option<String>,
    /// Current branch name.
    pub branch: Option<String>,
    /// Number of commits ahead of upstream.
    pub ahead: usize,
    /// Number of commits behind upstream.
    pub behind: usize,
    /// Status (`git status --porcelain`) refresh state.
    pub status_state: RefreshState,
    /// Branch ahead/behind refresh state.
    pub branch_state: RefreshState,
    /// Stash list refresh state.
    pub stash_state: RefreshState,
    /// Commit-in-flight state (separate from refreshes).
    pub commit_state: RefreshState,
    /// Last error message surfaced from the worker, if any. Cleared
    /// when the next refresh succeeds.
    pub last_error: Option<String>,
}

impl SourceControlPanel {
    /// Create a new source control panel with the given width.
    pub fn new(width: u16) -> Self {
        Self {
            stashes: Vec::new(),
            merge_changes: Vec::new(),
            changed: Vec::new(),
            staged: Vec::new(),
            commit_message: String::new(),
            focused_section: GitPanelSection::ChangedFiles,
            selected: 0,
            width,
            editing_commit_message: false,
            pending_discard: None,
            pending_discard_staged: None,
            branch: None,
            ahead: 0,
            behind: 0,
            status_state: RefreshState::Idle,
            branch_state: RefreshState::Idle,
            stash_state: RefreshState::Idle,
            commit_state: RefreshState::Idle,
            last_error: None,
        }
    }

    /// Send refresh requests to the git worker. Returns immediately —
    /// results arrive later via [`Self::apply_event`].
    ///
    /// If a section is already in flight, mark it `Stale` so the next
    /// completion will re-issue. This collapses overlapping 2 s ticks
    /// onto at most two worker invocations per section (one running, one
    /// pending), instead of stacking up into a thundering herd on slow
    /// repos.
    pub fn request_refresh(&mut self, git_repo: &GitRepo, worker: &GitWorker) {
        // Branch name is gix-native (fast); update synchronously.
        self.branch = git_repo.current_branch();

        for (state, cmd) in [
            (&mut self.status_state, GitCommand::RefreshStatus),
            (&mut self.branch_state, GitCommand::RefreshBranchInfo),
            (&mut self.stash_state, GitCommand::RefreshStashes),
        ] {
            match *state {
                RefreshState::Idle => {
                    *state = RefreshState::InFlight {
                        since: Instant::now(),
                    };
                    worker.send(cmd);
                }
                RefreshState::InFlight { .. } => {
                    *state = RefreshState::Stale;
                }
                RefreshState::Stale => {
                    // already coalesced
                }
            }
        }
    }

    /// Send a commit request to the worker. The panel switches to a
    /// committing state; the result arrives later via `apply_event`.
    pub fn request_commit(&mut self, worker: &GitWorker, message: String) {
        self.commit_state = RefreshState::InFlight {
            since: Instant::now(),
        };
        worker.send(GitCommand::Commit { message });
    }

    /// Drain a worker event into the panel's state.
    ///
    /// Returns `true` if the panel should be re-rendered immediately
    /// (the caller may use this to short-circuit a frame skip).
    pub fn apply_event(&mut self, ev: GitEvent, worker: &GitWorker) -> bool {
        match ev {
            GitEvent::StatusReady(entries) => {
                self.merge_changes.clear();
                self.changed.clear();
                self.staged.clear();
                self.last_error = None;
                for entry in entries {
                    let name = entry
                        .rel_path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&entry.rel_path)
                        .to_string();

                    if crate::git::is_conflict_entry(&entry) {
                        self.merge_changes.push(GitFileEntry {
                            rel_path: entry.rel_path.clone(),
                            name,
                            status: GitFileStatus::Conflict,
                        });
                        continue;
                    }

                    if entry.index_status != ' ' && entry.index_status != '?' {
                        let status = char_to_status(entry.index_status);
                        self.staged.push(GitFileEntry {
                            rel_path: entry.rel_path.clone(),
                            name: name.clone(),
                            status,
                        });
                    }

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
                self.clamp_selected();
                self.complete_refresh(GitRefreshKind::Status, worker);
                true
            }
            GitEvent::BranchInfoReady { ahead, behind } => {
                self.ahead = ahead;
                self.behind = behind;
                self.complete_refresh(GitRefreshKind::BranchInfo, worker);
                true
            }
            GitEvent::StashesReady(list) => {
                self.stashes.clear();
                for (name, message) in list {
                    self.stashes.push(StashEntry { name, message });
                }
                self.complete_refresh(GitRefreshKind::Stashes, worker);
                true
            }
            GitEvent::CommitDone(result) => {
                match result {
                    Ok(_) => {
                        self.commit_message.clear();
                        self.last_error = None;
                    }
                    Err(msg) => {
                        self.last_error = Some(msg);
                    }
                }
                self.commit_state = RefreshState::Idle;
                // Trigger a status refresh so the panel reflects the new
                // tree immediately.
                if matches!(self.status_state, RefreshState::Idle) {
                    self.status_state = RefreshState::InFlight {
                        since: Instant::now(),
                    };
                    worker.send(GitCommand::RefreshStatus);
                }
                true
            }
            GitEvent::Timeout(kind) => {
                self.last_error = Some(format!("git {kind:?} timed out"));
                self.complete_refresh(kind, worker);
                if matches!(kind, GitRefreshKind::Commit) {
                    self.commit_state = RefreshState::Idle;
                }
                true
            }
            GitEvent::Failed { kind, message } => {
                self.last_error = Some(message);
                self.complete_refresh(kind, worker);
                if matches!(kind, GitRefreshKind::Commit) {
                    self.commit_state = RefreshState::Idle;
                }
                true
            }
            GitEvent::GpgSignWarning => {
                // Handled at App level (banner). Panel just records it
                // so a follow-up refresh isn't surprised by a slow commit.
                false
            }
        }
    }

    /// Mark a refresh-kind slot as completed and re-issue if it went
    /// stale during the in-flight window.
    fn complete_refresh(&mut self, kind: GitRefreshKind, worker: &GitWorker) {
        let (state, cmd) = match kind {
            GitRefreshKind::Status => (&mut self.status_state, GitCommand::RefreshStatus),
            GitRefreshKind::BranchInfo => (&mut self.branch_state, GitCommand::RefreshBranchInfo),
            GitRefreshKind::Stashes => (&mut self.stash_state, GitCommand::RefreshStashes),
            GitRefreshKind::Commit => return,
        };
        let was_stale = matches!(*state, RefreshState::Stale);
        *state = RefreshState::Idle;
        if was_stale {
            *state = RefreshState::InFlight {
                since: Instant::now(),
            };
            worker.send(cmd);
        }
    }

    /// True if any section is currently waiting on the worker.
    pub fn any_in_flight(&self) -> bool {
        matches!(
            self.status_state,
            RefreshState::InFlight { .. } | RefreshState::Stale
        ) || matches!(
            self.branch_state,
            RefreshState::InFlight { .. } | RefreshState::Stale
        ) || matches!(
            self.stash_state,
            RefreshState::InFlight { .. } | RefreshState::Stale
        )
    }

    /// True if a commit is currently being executed by the worker.
    pub fn commit_in_flight(&self) -> bool {
        matches!(self.commit_state, RefreshState::InFlight { .. })
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
            GitPanelSection::CommitMessage => GitPanelSection::MergeChanges,
            GitPanelSection::MergeChanges => GitPanelSection::StagedFiles,
            GitPanelSection::StagedFiles => GitPanelSection::ChangedFiles,
            GitPanelSection::ChangedFiles => GitPanelSection::Stashes,
            GitPanelSection::Stashes => GitPanelSection::CommitMessage,
        };
        self.selected = 0;
    }

    /// Cycle to the previous section.
    pub fn prev_section(&mut self) {
        self.focused_section = match self.focused_section {
            GitPanelSection::CommitMessage => GitPanelSection::Stashes,
            GitPanelSection::MergeChanges => GitPanelSection::CommitMessage,
            GitPanelSection::StagedFiles => GitPanelSection::MergeChanges,
            GitPanelSection::ChangedFiles => GitPanelSection::StagedFiles,
            GitPanelSection::Stashes => GitPanelSection::ChangedFiles,
        };
        self.selected = 0;
    }

    /// Stage the currently selected changed file. Triggers an async
    /// status refresh through the worker so the panel updates without
    /// blocking the UI on a `git status` traversal.
    pub fn stage_selected(&mut self, git_repo: &GitRepo, worker: &GitWorker) {
        if self.focused_section != GitPanelSection::ChangedFiles {
            return;
        }
        if let Some(entry) = self.changed.get(self.selected) {
            let path = entry.rel_path.clone();
            if let Err(e) = git_repo.stage_file(&path) {
                tracing::warn!("Failed to stage {}: {}", path, e);
                return;
            }
            self.request_refresh(git_repo, worker);
        }
    }

    /// Stage all changed files at once.
    pub fn stage_all(&mut self, git_repo: &GitRepo, worker: &GitWorker) {
        let paths: Vec<String> = self.changed.iter().map(|e| e.rel_path.clone()).collect();
        for path in &paths {
            if let Err(e) = git_repo.stage_file(path) {
                tracing::warn!("Failed to stage {}: {}", path, e);
            }
        }
        if !paths.is_empty() {
            self.request_refresh(git_repo, worker);
        }
    }

    /// Unstage the currently selected staged file.
    pub fn unstage_selected(&mut self, git_repo: &GitRepo, worker: &GitWorker) {
        if self.focused_section != GitPanelSection::StagedFiles {
            return;
        }
        if let Some(entry) = self.staged.get(self.selected) {
            let path = entry.rel_path.clone();
            if let Err(e) = git_repo.unstage_file(&path) {
                tracing::warn!("Failed to unstage {}: {}", path, e);
                return;
            }
            self.request_refresh(git_repo, worker);
        }
    }

    /// Begin a commit through the worker. The commit message must already
    /// be set; on success the worker emits `CommitDone` which the panel's
    /// `apply_event` handler clears the message and triggers a refresh.
    /// Returns immediately — no UI block.
    pub fn commit(&mut self, worker: &GitWorker) -> Result<(), String> {
        let msg = self.commit_message.trim().to_string();
        if msg.is_empty() {
            return Err("Commit message is empty".to_string());
        }
        if self.staged.is_empty() {
            return Err("Nothing staged to commit".to_string());
        }
        if self.commit_in_flight() {
            return Err("Commit already in progress".to_string());
        }
        self.request_commit(worker, msg);
        Ok(())
    }

    /// Discard changes for the currently selected unstaged file.
    pub fn discard_selected(&mut self, git_repo: &GitRepo, worker: &GitWorker) {
        if let Some(path) = self.pending_discard.take() {
            if let Err(e) = git_repo.discard_file(&path) {
                tracing::warn!("Failed to discard {}: {}", path, e);
                return;
            }
            self.request_refresh(git_repo, worker);
        }
    }

    /// Unstage and discard changes for the currently selected staged file.
    pub fn discard_staged_selected(&mut self, git_repo: &GitRepo, worker: &GitWorker) {
        if let Some(path) = self.pending_discard_staged.take() {
            // First unstage the file.
            if let Err(e) = git_repo.unstage_file(&path) {
                tracing::warn!("Failed to unstage {}: {}", path, e);
                return;
            }
            // Then discard the working tree changes.
            if let Err(e) = git_repo.discard_file(&path) {
                tracing::warn!("Failed to discard {}: {}", path, e);
            }
            self.request_refresh(git_repo, worker);
        }
    }

    /// Get the path of the currently selected file entry, if any.
    pub fn selected_path(&self) -> Option<&str> {
        match self.focused_section {
            GitPanelSection::MergeChanges => self
                .merge_changes
                .get(self.selected)
                .map(|e| e.rel_path.as_str()),
            GitPanelSection::StagedFiles => {
                self.staged.get(self.selected).map(|e| e.rel_path.as_str())
            }
            GitPanelSection::ChangedFiles => {
                self.changed.get(self.selected).map(|e| e.rel_path.as_str())
            }
            GitPanelSection::CommitMessage | GitPanelSection::Stashes => None,
        }
    }

    /// Get the currently selected file entry (if any).
    pub fn selected_entry(&self) -> Option<&GitFileEntry> {
        match self.focused_section {
            GitPanelSection::MergeChanges => self.merge_changes.get(self.selected),
            GitPanelSection::StagedFiles => self.staged.get(self.selected),
            GitPanelSection::ChangedFiles => self.changed.get(self.selected),
            GitPanelSection::CommitMessage | GitPanelSection::Stashes => None,
        }
    }

    /// Get the currently selected stash entry (if in Stashes section).
    pub fn selected_stash(&self) -> Option<&StashEntry> {
        if self.focused_section == GitPanelSection::Stashes {
            self.stashes.get(self.selected)
        } else {
            None
        }
    }

    /// Number of items in the currently focused file list section.
    fn current_list_len(&self) -> usize {
        match self.focused_section {
            GitPanelSection::CommitMessage => 0,
            GitPanelSection::MergeChanges => self.merge_changes.len(),
            GitPanelSection::StagedFiles => self.staged.len(),
            GitPanelSection::ChangedFiles => self.changed.len(),
            GitPanelSection::Stashes => self.stashes.len(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(rel: &str, status: GitFileStatus) -> GitFileEntry {
        let name = rel.rsplit('/').next().unwrap_or(rel).to_string();
        GitFileEntry {
            rel_path: rel.to_string(),
            name,
            status,
        }
    }

    fn populated() -> SourceControlPanel {
        let mut p = SourceControlPanel::new(40);
        p.changed.push(entry("src/a.rs", GitFileStatus::Modified));
        p.changed.push(entry("src/b.rs", GitFileStatus::Untracked));
        p.changed.push(entry("README.md", GitFileStatus::Modified));
        p.staged.push(entry("src/c.rs", GitFileStatus::Added));
        p.staged.push(entry("src/d.rs", GitFileStatus::Modified));
        p.merge_changes
            .push(entry("Cargo.toml", GitFileStatus::Conflict));
        p.stashes.push(StashEntry {
            name: "stash@{0}".to_string(),
            message: "WIP on main".to_string(),
        });
        p
    }

    #[test]
    fn new_starts_focused_on_changed_files_with_zero_selected() {
        let p = SourceControlPanel::new(40);
        assert_eq!(p.focused_section, GitPanelSection::ChangedFiles);
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn select_down_clamps_at_section_max() {
        let mut p = populated(); // ChangedFiles has 3 items.
        p.select_down();
        p.select_down();
        p.select_down();
        // Already at last item (index 2); this is a no-op.
        p.select_down();
        assert_eq!(p.selected, 2);
    }

    #[test]
    fn select_up_saturates_at_zero() {
        let mut p = populated();
        p.selected = 1;
        p.select_up();
        p.select_up();
        p.select_up();
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn next_section_cycles_through_all_five_and_resets_selected() {
        let mut p = populated();
        // Start: ChangedFiles, selected = 2.
        p.selected = 2;
        // ChangedFiles → Stashes
        p.next_section();
        assert_eq!(p.focused_section, GitPanelSection::Stashes);
        assert_eq!(p.selected, 0);
        // Stashes → CommitMessage
        p.next_section();
        assert_eq!(p.focused_section, GitPanelSection::CommitMessage);
        // CommitMessage → MergeChanges
        p.next_section();
        assert_eq!(p.focused_section, GitPanelSection::MergeChanges);
        // MergeChanges → StagedFiles
        p.next_section();
        assert_eq!(p.focused_section, GitPanelSection::StagedFiles);
        // StagedFiles → ChangedFiles (full cycle)
        p.next_section();
        assert_eq!(p.focused_section, GitPanelSection::ChangedFiles);
    }

    #[test]
    fn prev_section_is_inverse_of_next_section() {
        let mut p = SourceControlPanel::new(40);
        let order = [
            GitPanelSection::ChangedFiles,
            GitPanelSection::Stashes,
            GitPanelSection::CommitMessage,
            GitPanelSection::MergeChanges,
            GitPanelSection::StagedFiles,
        ];
        for &expected in &order {
            assert_eq!(p.focused_section, expected);
            p.next_section();
        }
        // Back to ChangedFiles.
        assert_eq!(p.focused_section, GitPanelSection::ChangedFiles);

        // Now reverse: prev_section should walk the order backward.
        for &expected in order.iter().rev() {
            p.prev_section();
            assert_eq!(p.focused_section, expected);
        }
    }

    #[test]
    fn selected_path_returns_path_for_changed_files() {
        let p = populated();
        assert_eq!(p.selected_path(), Some("src/a.rs"));
    }

    #[test]
    fn selected_path_returns_path_for_staged_files() {
        let mut p = populated();
        p.focused_section = GitPanelSection::StagedFiles;
        assert_eq!(p.selected_path(), Some("src/c.rs"));
    }

    #[test]
    fn selected_path_returns_path_for_merge_changes() {
        let mut p = populated();
        p.focused_section = GitPanelSection::MergeChanges;
        assert_eq!(p.selected_path(), Some("Cargo.toml"));
    }

    #[test]
    fn selected_path_returns_none_for_commit_message_section() {
        let mut p = populated();
        p.focused_section = GitPanelSection::CommitMessage;
        assert_eq!(p.selected_path(), None);
    }

    #[test]
    fn selected_path_returns_none_for_stashes_section() {
        let mut p = populated();
        p.focused_section = GitPanelSection::Stashes;
        assert_eq!(p.selected_path(), None);
    }

    #[test]
    fn selected_entry_matches_selected_path() {
        let p = populated();
        let e = p.selected_entry().expect("entry expected");
        assert_eq!(e.rel_path, "src/a.rs");
        assert_eq!(e.status, GitFileStatus::Modified);
    }

    #[test]
    fn selected_stash_returns_only_in_stashes_section() {
        let mut p = populated();
        // Wrong section first.
        assert!(p.selected_stash().is_none());
        p.focused_section = GitPanelSection::Stashes;
        assert_eq!(
            p.selected_stash().map(|s| s.name.as_str()),
            Some("stash@{0}")
        );
    }

    #[test]
    fn select_down_in_empty_section_is_no_op() {
        let mut p = SourceControlPanel::new(40);
        p.focused_section = GitPanelSection::Stashes; // empty
        p.select_down();
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn pending_discard_state_is_settable_and_takeable() {
        let mut p = SourceControlPanel::new(40);
        assert!(p.pending_discard.is_none());
        p.pending_discard = Some("src/a.rs".to_string());
        assert_eq!(p.pending_discard.take(), Some("src/a.rs".to_string()));
        assert!(p.pending_discard.is_none());
    }

    #[test]
    fn char_to_status_maps_known_chars() {
        assert_eq!(char_to_status('M'), GitFileStatus::Modified);
        assert_eq!(char_to_status('A'), GitFileStatus::Added);
        assert_eq!(char_to_status('D'), GitFileStatus::Deleted);
        assert_eq!(char_to_status('R'), GitFileStatus::Renamed);
        assert_eq!(char_to_status('?'), GitFileStatus::Untracked);
    }

    #[test]
    fn char_to_status_falls_back_to_modified_for_unknown() {
        // The unknown-char fallback exists so unrecognised statuses don't drop
        // entries entirely; we surface them as Modified rather than panic.
        assert_eq!(char_to_status('X'), GitFileStatus::Modified);
        assert_eq!(char_to_status(' '), GitFileStatus::Modified);
    }
}
