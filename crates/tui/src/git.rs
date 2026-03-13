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

    /// Generate an AI-friendly diff summary for commit message generation.
    pub fn diff_summary(&self, file_path: &Path) -> anyhow::Result<String> {
        let output = std::process::Command::new("git")
            .args(["diff", "--cached", "--stat", "--"])
            .arg(file_path)
            .current_dir(&self.workdir)
            .output()?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
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
        assert!(status.get(&0).is_none());
        assert!(status.get(&2).is_none());
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
}
