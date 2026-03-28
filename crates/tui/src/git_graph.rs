//! Visual git graph modal showing commit history with branch lines.
//!
//! Opens with `:graph`. Shows ASCII branch graph, commit messages,
//! authors, dates. Select a commit to see changed files.

use crate::git::GraphCommit;
use std::collections::HashMap;

/// The git graph modal state.
pub struct GitGraphModal {
    /// Whether the modal is visible.
    pub visible: bool,
    /// All commits (newest first).
    pub commits: Vec<GraphCommit>,
    /// Pre-computed graph characters per row.
    pub graph_lines: Vec<String>,
    /// Graph column colors per row.
    pub graph_colors: Vec<Vec<u8>>,
    /// Currently selected commit index.
    pub selected: usize,
    /// Scroll offset.
    pub scroll: usize,
    /// Changed files for the selected commit.
    pub detail_files: Vec<(char, String)>,
    /// Whether the detail panel is shown.
    pub show_detail: bool,
}

impl GitGraphModal {
    /// Create a new modal (initially hidden).
    pub fn new() -> Self {
        Self {
            visible: false,
            commits: Vec::new(),
            graph_lines: Vec::new(),
            graph_colors: Vec::new(),
            selected: 0,
            scroll: 0,
            detail_files: Vec::new(),
            show_detail: true,
        }
    }

    /// Open the modal with commits.
    pub fn open(&mut self, commits: Vec<GraphCommit>) {
        let (lines, colors) = compute_graph(&commits);
        self.graph_lines = lines;
        self.graph_colors = colors;
        self.commits = commits;
        self.selected = 0;
        self.scroll = 0;
        self.detail_files.clear();
        self.visible = true;
    }

    /// Close the modal.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Move selection down.
    pub fn select_down(&mut self) {
        if self.selected + 1 < self.commits.len() {
            self.selected += 1;
        }
    }

    /// Move selection up.
    pub fn select_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Page down.
    pub fn page_down(&mut self, lines: usize) {
        self.selected = (self.selected + lines).min(self.commits.len().saturating_sub(1));
    }

    /// Page up.
    pub fn page_up(&mut self, lines: usize) {
        self.selected = self.selected.saturating_sub(lines);
    }

    /// Get the selected commit hash.
    pub fn selected_hash(&self) -> Option<&str> {
        self.commits.get(self.selected).map(|c| c.hash.as_str())
    }
}

impl Default for GitGraphModal {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute ASCII graph lines and colors from commits.
///
/// Returns (graph_strings, color_indices) where each row has
/// the ASCII art for that commit's graph position.
fn compute_graph(commits: &[GraphCommit]) -> (Vec<String>, Vec<Vec<u8>>) {
    let mut lines = Vec::with_capacity(commits.len());
    let mut colors = Vec::with_capacity(commits.len());

    // Map commit hash → index for parent lookup.
    let hash_idx: HashMap<&str, usize> = commits
        .iter()
        .enumerate()
        .map(|(i, c)| (c.hash.as_str(), i))
        .collect();

    // Active lanes: each lane tracks a commit hash it's heading toward.
    let mut lanes: Vec<Option<String>> = Vec::new();

    for (i, commit) in commits.iter().enumerate() {
        // Find which lane this commit occupies (or create a new one).
        let my_lane = lanes
            .iter()
            .position(|l| l.as_deref() == Some(&commit.hash))
            .unwrap_or_else(|| {
                // New lane.
                lanes.push(Some(commit.hash.clone()));
                lanes.len() - 1
            });

        // Build the graph line.
        let width = lanes.len().max(1);
        let mut graph = String::with_capacity(width * 2);
        let mut color_row = Vec::with_capacity(width * 2);

        for (col, lane) in lanes.iter().enumerate() {
            if col == my_lane {
                graph.push('*');
                color_row.push((i % 6) as u8);
            } else if lane.is_some() {
                graph.push('|');
                color_row.push(
                    lane.as_ref()
                        .and_then(|h| hash_idx.get(h.as_str()))
                        .map(|&idx| (idx % 6) as u8)
                        .unwrap_or(7),
                );
            } else {
                graph.push(' ');
                color_row.push(7);
            }
            graph.push(' ');
            color_row.push(7);
        }

        lines.push(graph);
        colors.push(color_row);

        // Update lanes for this commit's parents.
        // First parent continues in the same lane.
        if let Some(first_parent) = commit.parents.first() {
            lanes[my_lane] = Some(first_parent.clone());
        } else {
            lanes[my_lane] = None; // Root commit — lane ends.
        }

        // Additional parents get new lanes (merge commits).
        for parent in commit.parents.iter().skip(1) {
            if !lanes.iter().any(|l| l.as_deref() == Some(parent.as_str())) {
                lanes.push(Some(parent.clone()));
            }
        }

        // Clean up empty trailing lanes.
        while lanes.last() == Some(&None) {
            lanes.pop();
        }
    }

    (lines, colors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_graph() {
        let commits = vec![
            GraphCommit {
                hash: "aaa".into(),
                short: "aaa".into(),
                author: "a".into(),
                summary: "c1".into(),
                parents: vec!["bbb".into()],
                timestamp: 100,
                refs: vec![],
            },
            GraphCommit {
                hash: "bbb".into(),
                short: "bbb".into(),
                author: "a".into(),
                summary: "c2".into(),
                parents: vec![],
                timestamp: 99,
                refs: vec![],
            },
        ];
        let (lines, _colors) = compute_graph(&commits);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains('*'));
        assert!(lines[1].contains('*'));
    }

    #[test]
    fn test_empty_graph() {
        let (lines, colors) = compute_graph(&[]);
        assert!(lines.is_empty());
        assert!(colors.is_empty());
    }
}
