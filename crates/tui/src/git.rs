//! Git integration for AURA using gitoxide (`gix`).
//!
//! Provides git diff gutter markers, blame information, commit creation,
//! and branch management — all using native Rust git operations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Line-level git diff status for gutter rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStatus {
    /// Line was added (not in HEAD).
    Added,
    /// Line was modified (different from HEAD).
    Modified,
    /// A line was deleted after this position.
    Deleted,
}

/// Blame information for a single line.
#[derive(Debug, Clone)]
pub struct BlameEntry {
    /// Short commit hash.
    pub commit_short: String,
    /// Author name.
    pub author: String,
    /// Relative time string (e.g. "2d ago").
    pub time_ago: String,
    /// Commit summary (first line of message).
    pub summary: String,
}

/// Branch information.
#[derive(Debug, Clone)]
pub struct BranchInfo {
    /// Branch name.
    pub name: String,
    /// Whether this is the currently checked out branch.
    pub is_current: bool,
    /// Short commit hash of the branch tip.
    pub tip_short: String,
}

/// A single entry from `git log --aura` — includes the Aura-Conversation trailer if present.
#[derive(Debug, Clone)]
pub struct AuraLogEntry {
    /// Short (7-char) commit hash.
    pub commit_short: String,
    /// Author name.
    pub author: String,
    /// First line of the commit message.
    pub summary: String,
    /// Value of the `Aura-Conversation` git trailer, if present in the commit message.
    pub conversation_id: Option<String>,
}

/// Parsed entry from `git status --porcelain=v1`.
#[derive(Debug, Clone)]
pub struct GitStatusEntry {
    /// Relative path to the file from the repo root.
    pub rel_path: String,
    /// X in XY — index (staged) status character.
    pub index_status: char,
    /// Y in XY — worktree (unstaged) status character.
    pub worktree_status: char,
}

/// Check if a `GitStatusEntry` represents a merge conflict.
///
/// Conflicts in porcelain v1 are marked with combinations of U/A/D
/// in the index and worktree status columns.
pub fn is_conflict_entry(entry: &GitStatusEntry) -> bool {
    matches!(
        (entry.index_status, entry.worktree_status),
        ('U', 'U') | ('A', 'A') | ('D', 'D') | ('A', 'U') | ('U', 'A') | ('D', 'U') | ('U', 'D')
    )
}

/// Aligned diff line for side-by-side diff view.
#[derive(Debug, Clone)]
pub enum DiffLine {
    /// Line present in both old and new (matched by LCS).
    Both(String, String),
    /// Line only in old (deletion) — shown on left, blank on right.
    LeftOnly(String),
    /// Line only in new (addition) — blank on left, shown on right.
    RightOnly(String),
}

/// Produce aligned side-by-side diff lines from two text blocks.
///
/// Reuses the LCS algorithm from `diff_lines()` to find matched lines,
/// then interleaves deletions and additions in order.
pub fn aligned_diff_lines(old: &str, new: &str) -> Vec<DiffLine> {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let old_len = old_lines.len();
    let new_len = new_lines.len();

    if old_len > 5000 || new_len > 5000 {
        return aligned_diff_simple(&old_lines, &new_lines);
    }

    // Build LCS table.
    let mut lcs = vec![vec![0u32; new_len + 1]; old_len + 1];
    for i in 1..=old_len {
        for j in 1..=new_len {
            if old_lines[i - 1] == new_lines[j - 1] {
                lcs[i][j] = lcs[i - 1][j - 1] + 1;
            } else {
                lcs[i][j] = lcs[i - 1][j].max(lcs[i][j - 1]);
            }
        }
    }

    // Backtrack to collect the diff sequence.
    let mut result = Vec::new();
    let mut i = old_len;
    let mut j = new_len;

    // We collect in reverse, then reverse at the end.
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old_lines[i - 1] == new_lines[j - 1] {
            result.push(DiffLine::Both(
                old_lines[i - 1].to_string(),
                new_lines[j - 1].to_string(),
            ));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || lcs[i][j - 1] >= lcs[i - 1][j]) {
            result.push(DiffLine::RightOnly(new_lines[j - 1].to_string()));
            j -= 1;
        } else {
            result.push(DiffLine::LeftOnly(old_lines[i - 1].to_string()));
            i -= 1;
        }
    }

    result.reverse();
    result
}

/// Simple aligned diff for large files — line-by-line comparison.
fn aligned_diff_simple(old_lines: &[&str], new_lines: &[&str]) -> Vec<DiffLine> {
    let mut result = Vec::new();
    let max_common = old_lines.len().min(new_lines.len());

    for i in 0..max_common {
        if old_lines[i] == new_lines[i] {
            result.push(DiffLine::Both(
                old_lines[i].to_string(),
                new_lines[i].to_string(),
            ));
        } else {
            result.push(DiffLine::LeftOnly(old_lines[i].to_string()));
            result.push(DiffLine::RightOnly(new_lines[i].to_string()));
        }
    }

    for line in old_lines.iter().skip(max_common) {
        result.push(DiffLine::LeftOnly(line.to_string()));
    }
    for line in new_lines.iter().skip(max_common) {
        result.push(DiffLine::RightOnly(line.to_string()));
    }

    result
}

/// Git integration handle. Wraps a `gix::Repository`.
pub struct GitRepo {
    /// The opened repository.
    repo: gix::Repository,
    /// Path to the repository work directory.
    workdir: PathBuf,
    /// Cached line status for the current file.
    cached_status: HashMap<usize, LineStatus>,
    /// The file this status was computed for.
    cached_file: Option<PathBuf>,
    /// Cached blame data.
    cached_blame: Vec<Option<BlameEntry>>,
    /// The file blame was computed for.
    cached_blame_file: Option<PathBuf>,
}

impl GitRepo {
    /// Open a git repository by discovering it from a file path.
    pub fn discover(file_path: &Path) -> anyhow::Result<Self> {
        let repo = gix::discover(file_path)?;
        let workdir = repo
            .work_dir()
            .ok_or_else(|| anyhow::anyhow!("Bare repository not supported"))?
            .to_path_buf();

        Ok(Self {
            repo,
            workdir,
            cached_status: HashMap::new(),
            cached_file: None,
            cached_blame: Vec::new(),
            cached_blame_file: None,
        })
    }

    /// Get the workdir path.
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// Compute line-level diff status for a file (comparing working tree to HEAD).
    pub fn line_status(&mut self, file_path: &Path) -> &HashMap<usize, LineStatus> {
        // Return cached if same file.
        if self.cached_file.as_deref() == Some(file_path) {
            return &self.cached_status;
        }

        self.cached_status.clear();
        self.cached_file = Some(file_path.to_path_buf());

        if let Err(e) = self.compute_line_status(file_path) {
            tracing::debug!("Failed to compute git diff: {}", e);
        }

        &self.cached_status
    }

    /// Internal: compute diff between HEAD version and working tree version.
    fn compute_line_status(&mut self, file_path: &Path) -> anyhow::Result<()> {
        let rel_path = file_path.strip_prefix(&self.workdir).unwrap_or(file_path);

        // Get the HEAD tree.
        let head_commit = self
            .repo
            .head_commit()
            .map_err(|e| anyhow::anyhow!("No HEAD commit: {}", e))?;
        let head_tree = head_commit.tree()?;

        // Find the file in HEAD.
        let head_entry = head_tree.lookup_entry_by_path(rel_path)?;

        // Read working tree content.
        let working_content = std::fs::read_to_string(file_path)?;
        let working_lines: Vec<&str> = working_content.lines().collect();

        match head_entry {
            None => {
                // File is new (all lines are Added).
                for i in 0..working_lines.len() {
                    self.cached_status.insert(i, LineStatus::Added);
                }
            }
            Some(entry) => {
                let object = entry.object()?;
                let head_content = std::str::from_utf8(object.data.as_ref())
                    .unwrap_or("")
                    .to_string();
                let head_lines: Vec<&str> = head_content.lines().collect();

                // Simple line-by-line diff using LCS-based approach.
                diff_lines(&head_lines, &working_lines, &mut self.cached_status);
            }
        }

        Ok(())
    }

    /// Read the HEAD version of a file. Returns `None` if the file is new
    /// (not in HEAD) or the repo has no commits.
    pub fn head_file_content(&self, rel_path: &Path) -> anyhow::Result<Option<String>> {
        let head_commit = match self.repo.head_commit() {
            Ok(c) => c,
            Err(_) => return Ok(None),
        };
        let head_tree = head_commit.tree()?;
        let entry = head_tree.lookup_entry_by_path(rel_path)?;
        match entry {
            None => Ok(None),
            Some(entry) => {
                let object = entry.object()?;
                let text = std::str::from_utf8(object.data.as_ref())
                    .unwrap_or("")
                    .to_string();
                Ok(Some(text))
            }
        }
    }

    /// Invalidate cached diff status (call after buffer edits).
    pub fn invalidate_status(&mut self) {
        self.cached_file = None;
        self.cached_status.clear();
    }

    /// Get blame information for a file using `git blame` via command line.
    /// (gix doesn't have built-in blame yet, so we shell out.)
    pub fn blame(&mut self, file_path: &Path) -> &[Option<BlameEntry>] {
        if self.cached_blame_file.as_deref() == Some(file_path) {
            return &self.cached_blame;
        }

        self.cached_blame.clear();
        self.cached_blame_file = Some(file_path.to_path_buf());

        if let Err(e) = self.compute_blame(file_path) {
            tracing::debug!("Failed to compute blame: {}", e);
        }

        &self.cached_blame
    }

    /// Internal: run `git blame --porcelain` and parse the output.
    fn compute_blame(&mut self, file_path: &Path) -> anyhow::Result<()> {
        let output = std::process::Command::new("git")
            .args(["blame", "--porcelain"])
            .arg(file_path)
            .current_dir(&self.workdir)
            .output()?;

        if !output.status.success() {
            anyhow::bail!("git blame failed");
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut entries: Vec<Option<BlameEntry>> = Vec::new();
        let mut current_commit = String::new();
        let mut current_author = String::new();
        let mut current_time: i64 = 0;
        let mut current_summary = String::new();
        let mut in_header = false;

        for line in stdout.lines() {
            if line.starts_with('\t') {
                // Content line — marks end of a blame block.
                let entry = BlameEntry {
                    commit_short: current_commit.chars().take(7).collect(),
                    author: current_author.clone(),
                    time_ago: format_time_ago(current_time),
                    summary: current_summary.clone(),
                };
                entries.push(Some(entry));
                in_header = false;
            } else if !in_header
                && line.len() >= 40
                && line.chars().take(40).all(|c| c.is_ascii_hexdigit())
            {
                // New blame header: <hash> <orig_line> <final_line> [<num_lines>]
                current_commit = line.split_whitespace().next().unwrap_or("").to_string();
                in_header = true;
            } else if in_header {
                if let Some(author) = line.strip_prefix("author ") {
                    current_author = author.to_string();
                } else if let Some(time_str) = line.strip_prefix("author-time ") {
                    current_time = time_str.parse().unwrap_or(0);
                } else if let Some(summary) = line.strip_prefix("summary ") {
                    current_summary = summary.to_string();
                }
            }
        }

        self.cached_blame = entries;
        Ok(())
    }

    /// Invalidate cached blame data.
    pub fn invalidate_blame(&mut self) {
        self.cached_blame_file = None;
        self.cached_blame.clear();
    }

    /// List branches.
    pub fn list_branches(&self) -> anyhow::Result<Vec<BranchInfo>> {
        let mut branches = Vec::new();

        // Get current branch name.
        let head_ref = self.repo.head_ref().ok().flatten();
        let current_branch = head_ref.as_ref().map(|r| r.name().shorten().to_string());

        // List local branches via refs.
        let refs = self.repo.references()?;
        let local_refs = refs.local_branches()?;

        for reference in local_refs.flatten() {
            let name = reference.name().shorten().to_string();
            let is_current = current_branch.as_deref() == Some(&name);

            let tip_short = reference.id().to_hex_with_len(7).to_string();

            branches.push(BranchInfo {
                name,
                is_current,
                tip_short,
            });
        }

        // Sort: current branch first, then alphabetical.
        branches.sort_by(|a, b| {
            if a.is_current {
                std::cmp::Ordering::Less
            } else if b.is_current {
                std::cmp::Ordering::Greater
            } else {
                a.name.cmp(&b.name)
            }
        });

        Ok(branches)
    }

    /// Get the current branch name.
    pub fn current_branch(&self) -> Option<String> {
        let head_ref = self.repo.head_ref().ok()??;
        Some(head_ref.name().shorten().to_string())
    }

    /// Get the current HEAD short hash.
    pub fn head_short(&self) -> Option<String> {
        let commit = self.repo.head_commit().ok()?;
        Some(commit.id().to_hex_with_len(7).to_string())
    }

    /// Create a commit with the given message. Stages the specified file first.
    pub fn commit(&self, file_path: &Path, message: &str) -> anyhow::Result<String> {
        // Use git CLI for commit since gix's commit API is complex.
        let rel_path = file_path.strip_prefix(&self.workdir).unwrap_or(file_path);

        // Stage the file.
        let add_output = std::process::Command::new("git")
            .args(["add", "--"])
            .arg(rel_path)
            .current_dir(&self.workdir)
            .output()?;

        if !add_output.status.success() {
            anyhow::bail!(
                "git add failed: {}",
                String::from_utf8_lossy(&add_output.stderr)
            );
        }

        // Create the commit.
        let commit_output = std::process::Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(&self.workdir)
            .output()?;

        if !commit_output.status.success() {
            anyhow::bail!(
                "git commit failed: {}",
                String::from_utf8_lossy(&commit_output.stderr)
            );
        }

        // Return the new commit hash.
        self.head_short()
            .ok_or_else(|| anyhow::anyhow!("Failed to get commit hash"))
    }

    /// Create a commit with a conversation trailer.
    pub fn commit_with_conversation(
        &self,
        file_path: &Path,
        message: &str,
        conversation_summary: Option<&str>,
    ) -> anyhow::Result<String> {
        let full_message = if let Some(summary) = conversation_summary {
            format!("{message}\n\nAura-Conversation: {summary}")
        } else {
            message.to_string()
        };
        self.commit(file_path, &full_message)
    }

    /// Switch to a branch (checkout).
    pub fn checkout_branch(&mut self, branch_name: &str) -> anyhow::Result<()> {
        let output = std::process::Command::new("git")
            .args(["checkout", branch_name])
            .current_dir(&self.workdir)
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "git checkout failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        // Invalidate caches after branch switch.
        self.invalidate_status();
        self.invalidate_blame();
        Ok(())
    }

    /// Create and checkout a new branch.
    pub fn create_branch(&mut self, branch_name: &str) -> anyhow::Result<()> {
        let output = std::process::Command::new("git")
            .args(["checkout", "-b", branch_name])
            .current_dir(&self.workdir)
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "git checkout -b failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        self.invalidate_status();
        self.invalidate_blame();
        Ok(())
    }

    /// Read git log and extract `Aura-Conversation` trailers.
    ///
    /// Returns up to `limit` entries. Each entry contains the short commit hash,
    /// author name, commit summary, and the conversation ID extracted from the
    /// `Aura-Conversation` git trailer (if present).
    pub fn aura_log(&self, limit: usize) -> Vec<AuraLogEntry> {
        let count_arg = format!("-{}", limit.max(1));

        // Use a NUL-delimited format: hash, author, full body separated by \x1f (unit separator).
        // Format: <hash>\x1f<author>\x1f<subject>\x1f<body>\0
        let output = std::process::Command::new("git")
            .args(["log", &count_arg, "--format=%h\x1f%an\x1f%s\x1f%b\x00"])
            .current_dir(&self.workdir)
            .output();

        let output = match output {
            Ok(o) if o.status.success() => o,
            Ok(o) => {
                tracing::debug!("git log failed: {}", String::from_utf8_lossy(&o.stderr));
                return Vec::new();
            }
            Err(e) => {
                tracing::debug!("git log error: {}", e);
                return Vec::new();
            }
        };

        let raw = String::from_utf8_lossy(&output.stdout);
        let mut entries = Vec::new();

        // Records are separated by NUL bytes.
        for record in raw.split('\x00') {
            let record = record.trim();
            if record.is_empty() {
                continue;
            }
            // Fields are separated by \x1f (unit separator).
            let parts: Vec<&str> = record.splitn(4, '\x1f').collect();
            let commit_short = parts.first().copied().unwrap_or("").to_string();
            let author = parts.get(1).copied().unwrap_or("").to_string();
            let summary = parts.get(2).copied().unwrap_or("").to_string();
            let body = parts.get(3).copied().unwrap_or("");

            if commit_short.is_empty() {
                continue;
            }

            // Extract `Aura-Conversation: <value>` trailer from the body.
            let conversation_id = body.lines().find_map(|line| {
                line.strip_prefix("Aura-Conversation:")
                    .map(|v| v.trim().to_string())
            });

            entries.push(AuraLogEntry {
                commit_short,
                author,
                summary,
                conversation_id,
            });
        }

        entries
    }

    /// Generate an AI-friendly diff summary for commit message generation.
    pub fn diff_summary(&self, file_path: &Path) -> anyhow::Result<String> {
        let output = std::process::Command::new("git")
            .args(["diff", "--cached", "--stat", "--"])
            .arg(file_path)
            .current_dir(&self.workdir)
            .output()?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Parse `git status --porcelain=v1` into structured entries.
    pub fn file_status(&self) -> anyhow::Result<Vec<GitStatusEntry>> {
        let output = std::process::Command::new("git")
            .args(["status", "--porcelain=v1"])
            .current_dir(&self.workdir)
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "git status failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut entries = Vec::new();

        for line in stdout.lines() {
            if line.len() < 4 {
                continue;
            }
            let bytes = line.as_bytes();
            let index_status = bytes[0] as char;
            let worktree_status = bytes[1] as char;
            // Skip the space at position 2.
            let rel_path = line[3..].to_string();

            entries.push(GitStatusEntry {
                rel_path,
                index_status,
                worktree_status,
            });
        }

        Ok(entries)
    }

    /// Stage a file by relative path.
    pub fn stage_file(&self, rel_path: &str) -> anyhow::Result<()> {
        let output = std::process::Command::new("git")
            .args(["add", "--", rel_path])
            .current_dir(&self.workdir)
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "git add failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    /// Unstage a file by relative path.
    pub fn unstage_file(&self, rel_path: &str) -> anyhow::Result<()> {
        let output = std::process::Command::new("git")
            .args(["restore", "--staged", "--", rel_path])
            .current_dir(&self.workdir)
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "git restore --staged failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    /// Discard unstaged changes for a file by relative path (git restore).
    pub fn discard_file(&self, rel_path: &str) -> anyhow::Result<()> {
        let output = std::process::Command::new("git")
            .args(["restore", "--", rel_path])
            .current_dir(&self.workdir)
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "git restore failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    /// List all stashes. Returns (name, message) pairs.
    pub fn stash_list(&self) -> anyhow::Result<Vec<(String, String)>> {
        let output = std::process::Command::new("git")
            .args(["stash", "list", "--format=%gd|%s"])
            .current_dir(&self.workdir)
            .output()?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stashes = stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(|line| {
                let parts: Vec<&str> = line.splitn(2, '|').collect();
                let name = parts.first().unwrap_or(&"").to_string();
                let message = parts.get(1).unwrap_or(&"").to_string();
                (name, message)
            })
            .collect();
        Ok(stashes)
    }

    /// Push a new stash with the given message.
    pub fn stash_push(&self, message: &str) -> anyhow::Result<()> {
        let output = std::process::Command::new("git")
            .args(["stash", "push", "-m", message])
            .current_dir(&self.workdir)
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "git stash push failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    /// Pop a stash by name (e.g. "stash@{0}").
    pub fn stash_pop(&self, name: &str) -> anyhow::Result<()> {
        let output = std::process::Command::new("git")
            .args(["stash", "pop", name])
            .current_dir(&self.workdir)
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "git stash pop failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    /// Drop a stash by name.
    pub fn stash_drop(&self, name: &str) -> anyhow::Result<()> {
        let output = std::process::Command::new("git")
            .args(["stash", "drop", name])
            .current_dir(&self.workdir)
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "git stash drop failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    /// Commit staged changes with the given message. Returns the new commit short hash.
    pub fn commit_staged(&self, message: &str) -> anyhow::Result<String> {
        let output = std::process::Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(&self.workdir)
            .output()?;

        if !output.status.success() {
            anyhow::bail!(
                "git commit failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        self.head_short()
            .ok_or_else(|| anyhow::anyhow!("Failed to get commit hash"))
    }

    /// Get the number of commits ahead of and behind the upstream branch.
    ///
    /// Returns `(ahead, behind)`. Returns `(0, 0)` if there is no upstream
    /// tracking branch configured.
    pub fn ahead_behind(&self) -> (usize, usize) {
        let output = std::process::Command::new("git")
            .args(["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])
            .current_dir(&self.workdir)
            .output();

        match output {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout);
                let parts: Vec<&str> = text.trim().split('\t').collect();
                if parts.len() == 2 {
                    let ahead = parts[0].parse().unwrap_or(0);
                    let behind = parts[1].parse().unwrap_or(0);
                    (ahead, behind)
                } else {
                    (0, 0)
                }
            }
            _ => (0, 0),
        }
    }

    /// Get a stat summary of the staged diff.
    pub fn staged_diff_summary(&self) -> anyhow::Result<String> {
        let output = std::process::Command::new("git")
            .args(["diff", "--cached", "--stat"])
            .current_dir(&self.workdir)
            .output()?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get the full staged diff patch, truncated to a reasonable size for AI context.
    pub fn staged_diff_patch(&self, max_bytes: usize) -> anyhow::Result<String> {
        let output = std::process::Command::new("git")
            .args(["diff", "--cached"])
            .current_dir(&self.workdir)
            .output()?;

        let full = String::from_utf8_lossy(&output.stdout).to_string();
        if full.len() <= max_bytes {
            Ok(full)
        } else {
            let mut truncated = full[..max_bytes].to_string();
            truncated.push_str("\n\n... (diff truncated)");
            Ok(truncated)
        }
    }

    /// Get extended commit log with parent hashes and decorations for graph view.
    pub fn graph_log(&self, limit: usize) -> anyhow::Result<Vec<GraphCommit>> {
        let output = std::process::Command::new("git")
            .args([
                "log",
                "--all",
                &format!("-n{limit}"),
                "--format=%H%x00%h%x00%an%x00%s%x00%P%x00%ct%x00%D",
            ])
            .current_dir(&self.workdir)
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut commits = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\x00').collect();
            if parts.len() >= 6 {
                commits.push(GraphCommit {
                    hash: parts[0].to_string(),
                    short: parts[1].to_string(),
                    author: parts[2].to_string(),
                    summary: parts[3].to_string(),
                    parents: parts[4].split_whitespace().map(String::from).collect(),
                    timestamp: parts[5].parse().unwrap_or(0),
                    refs: if parts.len() > 6 && !parts[6].is_empty() {
                        parts[6].split(", ").map(|s| s.trim().to_string()).collect()
                    } else {
                        Vec::new()
                    },
                });
            }
        }
        Ok(commits)
    }

    /// Get the list of files changed in a specific commit.
    pub fn commit_files(&self, hash: &str) -> anyhow::Result<Vec<(char, String)>> {
        let output = std::process::Command::new("git")
            .args(["diff-tree", "--name-status", "-r", "--no-commit-id", hash])
            .current_dir(&self.workdir)
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut files = Vec::new();
        for line in stdout.lines() {
            if let Some((status, path)) = line.split_once('\t') {
                let status_char = status.chars().next().unwrap_or('?');
                files.push((status_char, path.to_string()));
            }
        }
        Ok(files)
    }
}

/// A commit with parent/ancestry info for graph visualization.
#[derive(Debug, Clone)]
pub struct GraphCommit {
    /// Full commit hash.
    pub hash: String,
    /// Short (7-char) commit hash.
    pub short: String,
    /// Author name.
    pub author: String,
    /// First line of commit message.
    pub summary: String,
    /// Parent commit hashes.
    pub parents: Vec<String>,
    /// Unix timestamp.
    pub timestamp: i64,
    /// Branch/tag decorations.
    pub refs: Vec<String>,
}

impl GraphCommit {
    /// Format the timestamp as a relative time string.
    pub fn time_ago(&self) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let diff = now - self.timestamp;
        if diff < 60 {
            "just now".to_string()
        } else if diff < 3600 {
            format!("{}m ago", diff / 60)
        } else if diff < 86400 {
            format!("{}h ago", diff / 3600)
        } else if diff < 604800 {
            format!("{}d ago", diff / 86400)
        } else {
            format!("{}w ago", diff / 604800)
        }
    }
}

/// Simple line diff using a longest common subsequence approach.
/// Marks lines in `new_lines` as Added, Modified, or records deletions.
fn diff_lines(old_lines: &[&str], new_lines: &[&str], status: &mut HashMap<usize, LineStatus>) {
    let old_len = old_lines.len();
    let new_len = new_lines.len();

    // For large files, use a simpler heuristic to avoid O(n²) LCS.
    if old_len > 5000 || new_len > 5000 {
        diff_lines_simple(old_lines, new_lines, status);
        return;
    }

    // Build LCS table.
    let mut lcs = vec![vec![0u32; new_len + 1]; old_len + 1];
    for i in 1..=old_len {
        for j in 1..=new_len {
            if old_lines[i - 1] == new_lines[j - 1] {
                lcs[i][j] = lcs[i - 1][j - 1] + 1;
            } else {
                lcs[i][j] = lcs[i - 1][j].max(lcs[i][j - 1]);
            }
        }
    }

    // Backtrack to find changes.
    let mut i = old_len;
    let mut j = new_len;
    let mut old_matched = vec![false; old_len];
    let mut new_matched = vec![false; new_len];

    while i > 0 && j > 0 {
        if old_lines[i - 1] == new_lines[j - 1] {
            old_matched[i - 1] = true;
            new_matched[j - 1] = true;
            i -= 1;
            j -= 1;
        } else if lcs[i - 1][j] >= lcs[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }

    // Mark unmatched new lines as Added.
    for (idx, matched) in new_matched.iter().enumerate() {
        if !matched {
            status.insert(idx, LineStatus::Added);
        }
    }

    // Mark positions where old lines were deleted.
    let mut last_new_pos = 0;
    for (idx, matched) in old_matched.iter().enumerate() {
        if !matched {
            // Find the nearest new line position.
            while last_new_pos < new_len && new_matched[last_new_pos] {
                last_new_pos += 1;
            }
            let delete_pos = if idx < old_len {
                // Map to approximate new position.
                (idx * new_len / old_len.max(1)).min(new_len.saturating_sub(1))
            } else {
                new_len.saturating_sub(1)
            };
            // If the position already has Added, upgrade to Modified.
            match status.entry(delete_pos) {
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    e.insert(LineStatus::Modified);
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(LineStatus::Deleted);
                }
            }
        }
    }
}

/// Simple line-by-line comparison for large files (O(n)).
fn diff_lines_simple(
    old_lines: &[&str],
    new_lines: &[&str],
    status: &mut HashMap<usize, LineStatus>,
) {
    let max_common = old_lines.len().min(new_lines.len());
    for i in 0..max_common {
        if old_lines[i] != new_lines[i] {
            status.insert(i, LineStatus::Modified);
        }
    }
    // Extra new lines are Added.
    for i in old_lines.len()..new_lines.len() {
        status.insert(i, LineStatus::Added);
    }
    // If old had more lines, mark last position as Deleted.
    if old_lines.len() > new_lines.len() && !new_lines.is_empty() {
        let last = new_lines.len().saturating_sub(1);
        status.insert(last, LineStatus::Deleted);
    }
}

/// Format a Unix timestamp as a relative time string.
fn format_time_ago(timestamp: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let diff = now - timestamp;

    if diff < 60 {
        format!("{diff}s ago")
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 604800 {
        format!("{}d ago", diff / 86400)
    } else if diff < 2592000 {
        format!("{}w ago", diff / 604800)
    } else {
        format!("{}mo ago", diff / 2592000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_lines_no_changes() {
        let old = vec!["line1", "line2", "line3"];
        let new = vec!["line1", "line2", "line3"];
        let mut status = HashMap::new();
        diff_lines(&old, &new, &mut status);
        assert!(status.is_empty());
    }

    #[test]
    fn test_diff_lines_added() {
        let old = vec!["line1", "line3"];
        let new = vec!["line1", "line2", "line3"];
        let mut status = HashMap::new();
        diff_lines(&old, &new, &mut status);
        assert_eq!(status.get(&1), Some(&LineStatus::Added));
    }

    #[test]
    fn test_diff_lines_all_new() {
        let old: Vec<&str> = vec![];
        let new = vec!["line1", "line2"];
        let mut status = HashMap::new();
        diff_lines(&old, &new, &mut status);
        assert_eq!(status.get(&0), Some(&LineStatus::Added));
        assert_eq!(status.get(&1), Some(&LineStatus::Added));
    }

    #[test]
    fn test_diff_lines_simple_modified() {
        let old = vec!["line1", "line2", "line3"];
        let new = vec!["line1", "changed", "line3"];
        let mut status = HashMap::new();
        diff_lines_simple(&old, &new, &mut status);
        assert_eq!(status.get(&1), Some(&LineStatus::Modified));
        assert!(!status.contains_key(&0));
        assert!(!status.contains_key(&2));
    }

    #[test]
    fn test_format_time_ago() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!(format_time_ago(now - 30).contains("s ago"));
        assert!(format_time_ago(now - 300).contains("m ago"));
        assert!(format_time_ago(now - 7200).contains("h ago"));
        assert!(format_time_ago(now - 172800).contains("d ago"));
    }

    #[test]
    fn test_line_status_eq() {
        assert_eq!(LineStatus::Added, LineStatus::Added);
        assert_ne!(LineStatus::Added, LineStatus::Modified);
    }

    #[test]
    fn test_branch_info_display() {
        let branch = BranchInfo {
            name: "feature/test".to_string(),
            is_current: true,
            tip_short: "abc1234".to_string(),
        };
        assert!(branch.is_current);
        assert_eq!(branch.name, "feature/test");
    }

    #[test]
    fn test_aligned_diff_no_changes() {
        let lines = aligned_diff_lines("a\nb\nc\n", "a\nb\nc\n");
        assert_eq!(lines.len(), 3);
        assert!(matches!(&lines[0], DiffLine::Both(l, _) if l == "a"));
        assert!(matches!(&lines[1], DiffLine::Both(l, _) if l == "b"));
        assert!(matches!(&lines[2], DiffLine::Both(l, _) if l == "c"));
    }

    #[test]
    fn test_aligned_diff_addition() {
        let lines = aligned_diff_lines("a\nc\n", "a\nb\nc\n");
        assert_eq!(lines.len(), 3);
        assert!(matches!(&lines[0], DiffLine::Both(l, _) if l == "a"));
        assert!(matches!(&lines[1], DiffLine::RightOnly(r) if r == "b"));
        assert!(matches!(&lines[2], DiffLine::Both(l, _) if l == "c"));
    }

    #[test]
    fn test_aligned_diff_deletion() {
        let lines = aligned_diff_lines("a\nb\nc\n", "a\nc\n");
        assert_eq!(lines.len(), 3);
        assert!(matches!(&lines[0], DiffLine::Both(l, _) if l == "a"));
        assert!(matches!(&lines[1], DiffLine::LeftOnly(l) if l == "b"));
        assert!(matches!(&lines[2], DiffLine::Both(l, _) if l == "c"));
    }

    #[test]
    fn test_aligned_diff_empty_old() {
        let lines = aligned_diff_lines("", "a\nb\n");
        assert_eq!(lines.len(), 2);
        assert!(matches!(&lines[0], DiffLine::RightOnly(r) if r == "a"));
        assert!(matches!(&lines[1], DiffLine::RightOnly(r) if r == "b"));
    }
}
