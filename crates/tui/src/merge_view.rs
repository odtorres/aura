//! 3-panel git merge conflict editor.
//!
//! Parses files with `<<<<<<<` / `=======` / `>>>>>>>` conflict markers into
//! structured [`MergeSegment`]s and provides a [`MergeConflictView`] for
//! interactive resolution.  The view tracks per-conflict resolution choices
//! and can produce the final resolved file content.

/// How a conflict block has been resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// Not yet resolved.
    Unresolved,
    /// Keep the current branch (ours/HEAD) version.
    AcceptCurrent,
    /// Keep the incoming branch (theirs) version.
    AcceptIncoming,
    /// Keep both: current first, then incoming.
    AcceptBothCurrentFirst,
    /// Keep both: incoming first, then current.
    AcceptBothIncomingFirst,
    /// Ignore — remove conflict markers, keep current version.
    Ignore,
}

/// A single conflict block extracted from merge markers.
#[derive(Debug, Clone)]
pub struct ConflictBlock {
    /// Lines from the current branch (between `<<<<<<<` and `=======`).
    pub ours: Vec<String>,
    /// Lines from the incoming branch (between `=======` and `>>>>>>>`).
    pub theirs: Vec<String>,
    /// Current resolution choice.
    pub resolution: Resolution,
}

impl ConflictBlock {
    /// Return the resolved lines for this conflict.
    pub fn resolved_lines(&self) -> Vec<String> {
        match self.resolution {
            Resolution::Unresolved => {
                vec!["<<<< UNRESOLVED CONFLICT >>>>".to_string()]
            }
            Resolution::AcceptCurrent | Resolution::Ignore => self.ours.clone(),
            Resolution::AcceptIncoming => self.theirs.clone(),
            Resolution::AcceptBothCurrentFirst => {
                let mut lines = self.ours.clone();
                lines.extend(self.theirs.iter().cloned());
                lines
            }
            Resolution::AcceptBothIncomingFirst => {
                let mut lines = self.theirs.clone();
                lines.extend(self.ours.iter().cloned());
                lines
            }
        }
    }
}

/// A segment of a file: either non-conflicting context or a conflict block.
#[derive(Debug, Clone)]
pub enum MergeSegment {
    /// Lines that are not part of any conflict.
    Context(Vec<String>),
    /// A conflict block with ours/theirs sides.
    Conflict(ConflictBlock),
}

/// Which panel currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeFocus {
    /// Top-left: incoming (theirs) panel.
    Incoming,
    /// Top-right: current (ours) panel.
    Current,
    /// Bottom: result panel.
    Result,
}

/// State for the 3-panel merge conflict editor.
pub struct MergeConflictView {
    /// Relative file path.
    pub file_path: String,
    /// Parsed segments (context + conflicts).
    pub segments: Vec<MergeSegment>,
    /// Index of the currently active conflict (0-based among conflict segments).
    pub active_conflict: usize,
    /// Total number of conflict blocks.
    pub total_conflicts: usize,
    /// Number of resolved conflicts.
    pub resolved_count: usize,
    /// Scroll offset for the incoming (top-left) panel.
    pub scroll_incoming: usize,
    /// Scroll offset for the current (top-right) panel.
    pub scroll_current: usize,
    /// Scroll offset for the result (bottom) panel.
    pub scroll_result: usize,
    /// Which panel has keyboard focus.
    pub focus: MergeFocus,
}

impl MergeConflictView {
    /// Create a new merge conflict view from parsed segments.
    pub fn new(file_path: String, segments: Vec<MergeSegment>) -> Self {
        let total_conflicts = segments
            .iter()
            .filter(|s| matches!(s, MergeSegment::Conflict(_)))
            .count();
        Self {
            file_path,
            segments,
            active_conflict: 0,
            total_conflicts,
            resolved_count: 0,
            scroll_incoming: 0,
            scroll_current: 0,
            scroll_result: 0,
            focus: MergeFocus::Incoming,
        }
    }

    /// Scroll the focused panel up.
    pub fn scroll_up(&mut self, n: usize) {
        match self.focus {
            MergeFocus::Incoming => {
                self.scroll_incoming = self.scroll_incoming.saturating_sub(n);
            }
            MergeFocus::Current => {
                self.scroll_current = self.scroll_current.saturating_sub(n);
            }
            MergeFocus::Result => {
                self.scroll_result = self.scroll_result.saturating_sub(n);
            }
        }
    }

    /// Scroll the focused panel down.
    pub fn scroll_down(&mut self, n: usize, _viewport_height: usize) {
        match self.focus {
            MergeFocus::Incoming => {
                self.scroll_incoming += n;
            }
            MergeFocus::Current => {
                self.scroll_current += n;
            }
            MergeFocus::Result => {
                self.scroll_result += n;
            }
        }
    }

    /// Jump to the next unresolved conflict.
    pub fn next_conflict(&mut self) {
        if self.total_conflicts == 0 {
            return;
        }
        let mut conflict_idx = 0;
        let start = self.active_conflict;
        for (i, seg) in self.segments.iter().enumerate() {
            if let MergeSegment::Conflict(block) = seg {
                if conflict_idx > start && block.resolution == Resolution::Unresolved {
                    self.active_conflict = conflict_idx;
                    return;
                }
                if i > 0 {
                    // Only count conflicts we've passed
                }
                conflict_idx += 1;
            }
        }
        // Wrap around: find the first unresolved.
        conflict_idx = 0;
        for seg in &self.segments {
            if let MergeSegment::Conflict(block) = seg {
                if block.resolution == Resolution::Unresolved {
                    self.active_conflict = conflict_idx;
                    return;
                }
                conflict_idx += 1;
            }
        }
        // All resolved — stay at current.
    }

    /// Jump to the previous unresolved conflict.
    pub fn prev_conflict(&mut self) {
        if self.total_conflicts == 0 {
            return;
        }
        let mut last_unresolved = None;
        let mut conflict_idx = 0;
        for seg in &self.segments {
            if let MergeSegment::Conflict(block) = seg {
                if conflict_idx < self.active_conflict && block.resolution == Resolution::Unresolved
                {
                    last_unresolved = Some(conflict_idx);
                }
                conflict_idx += 1;
            }
        }
        if let Some(idx) = last_unresolved {
            self.active_conflict = idx;
            return;
        }
        // Wrap around: find the last unresolved.
        conflict_idx = 0;
        for seg in &self.segments {
            if let MergeSegment::Conflict(block) = seg {
                if block.resolution == Resolution::Unresolved {
                    last_unresolved = Some(conflict_idx);
                }
                conflict_idx += 1;
            }
        }
        if let Some(idx) = last_unresolved {
            self.active_conflict = idx;
        }
    }

    /// Resolve the currently active conflict with the given resolution.
    pub fn resolve(&mut self, resolution: Resolution) {
        let mut conflict_idx = 0;
        for seg in &mut self.segments {
            if let MergeSegment::Conflict(block) = seg {
                if conflict_idx == self.active_conflict {
                    let was_unresolved = block.resolution == Resolution::Unresolved;
                    let is_now_resolved = resolution != Resolution::Unresolved;
                    block.resolution = resolution;
                    if was_unresolved && is_now_resolved {
                        self.resolved_count += 1;
                    } else if !was_unresolved && !is_now_resolved {
                        self.resolved_count = self.resolved_count.saturating_sub(1);
                    }
                    break;
                }
                conflict_idx += 1;
            }
        }
        // Auto-advance to next unresolved.
        self.next_conflict();
    }

    /// Check if all conflicts have been resolved.
    pub fn all_resolved(&self) -> bool {
        self.resolved_count == self.total_conflicts && self.total_conflicts > 0
    }

    /// Build the final resolved file content.
    pub fn build_result(&self) -> String {
        let mut lines = Vec::new();
        for seg in &self.segments {
            match seg {
                MergeSegment::Context(ctx) => {
                    lines.extend(ctx.iter().cloned());
                }
                MergeSegment::Conflict(block) => {
                    lines.extend(block.resolved_lines());
                }
            }
        }
        lines.join("\n")
    }

    /// Build lines for the incoming (theirs) panel.
    pub fn incoming_lines(&self) -> Vec<(String, Option<usize>)> {
        let mut lines = Vec::new();
        let mut conflict_idx = 0;
        for seg in &self.segments {
            match seg {
                MergeSegment::Context(ctx) => {
                    for line in ctx {
                        lines.push((line.clone(), None));
                    }
                }
                MergeSegment::Conflict(block) => {
                    for line in &block.theirs {
                        lines.push((line.clone(), Some(conflict_idx)));
                    }
                    conflict_idx += 1;
                }
            }
        }
        lines
    }

    /// Build lines for the current (ours) panel.
    pub fn current_lines(&self) -> Vec<(String, Option<usize>)> {
        let mut lines = Vec::new();
        let mut conflict_idx = 0;
        for seg in &self.segments {
            match seg {
                MergeSegment::Context(ctx) => {
                    for line in ctx {
                        lines.push((line.clone(), None));
                    }
                }
                MergeSegment::Conflict(block) => {
                    for line in &block.ours {
                        lines.push((line.clone(), Some(conflict_idx)));
                    }
                    conflict_idx += 1;
                }
            }
        }
        lines
    }

    /// Build lines for the result panel.
    pub fn result_lines(&self) -> Vec<(String, Option<usize>)> {
        let mut lines = Vec::new();
        let mut conflict_idx = 0;
        for seg in &self.segments {
            match seg {
                MergeSegment::Context(ctx) => {
                    for line in ctx {
                        lines.push((line.clone(), None));
                    }
                }
                MergeSegment::Conflict(block) => {
                    for line in block.resolved_lines() {
                        lines.push((line, Some(conflict_idx)));
                    }
                    conflict_idx += 1;
                }
            }
        }
        lines
    }

    /// Cycle focus between panels.
    pub fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            MergeFocus::Incoming => MergeFocus::Current,
            MergeFocus::Current => MergeFocus::Result,
            MergeFocus::Result => MergeFocus::Incoming,
        };
    }

    /// Get the conflict index for a given segment-level conflict.
    pub fn conflict_resolution(&self, conflict_idx: usize) -> Resolution {
        let mut idx = 0;
        for seg in &self.segments {
            if let MergeSegment::Conflict(block) = seg {
                if idx == conflict_idx {
                    return block.resolution;
                }
                idx += 1;
            }
        }
        Resolution::Unresolved
    }
}

/// Parse a file with conflict markers into segments.
///
/// Recognises the standard `<<<<<<<` / `=======` / `>>>>>>>` format.
/// Lines between `<<<<<<<` and `=======` are "ours" (current branch).
/// Lines between `=======` and `>>>>>>>` are "theirs" (incoming branch).
/// The optional `|||||||` base marker (diff3 style) is skipped.
pub fn parse_conflict_markers(content: &str) -> Vec<MergeSegment> {
    let mut segments = Vec::new();
    let mut context = Vec::new();

    #[derive(PartialEq)]
    enum State {
        Normal,
        Ours,
        Base, // diff3 ||||||| section — skip
        Theirs,
    }

    let mut state = State::Normal;
    let mut ours = Vec::new();
    let mut theirs = Vec::new();

    for line in content.lines() {
        match state {
            State::Normal => {
                if line.starts_with("<<<<<<<") {
                    // Flush context.
                    if !context.is_empty() {
                        segments.push(MergeSegment::Context(std::mem::take(&mut context)));
                    }
                    state = State::Ours;
                } else {
                    context.push(line.to_string());
                }
            }
            State::Ours => {
                if line.starts_with("|||||||") {
                    // diff3 base marker — skip to =======
                    state = State::Base;
                } else if line.starts_with("=======") {
                    state = State::Theirs;
                } else {
                    ours.push(line.to_string());
                }
            }
            State::Base => {
                if line.starts_with("=======") {
                    state = State::Theirs;
                }
                // Skip base lines.
            }
            State::Theirs => {
                if line.starts_with(">>>>>>>") {
                    segments.push(MergeSegment::Conflict(ConflictBlock {
                        ours: std::mem::take(&mut ours),
                        theirs: std::mem::take(&mut theirs),
                        resolution: Resolution::Unresolved,
                    }));
                    state = State::Normal;
                } else {
                    theirs.push(line.to_string());
                }
            }
        }
    }

    // Flush remaining context.
    if !context.is_empty() {
        segments.push(MergeSegment::Context(context));
    }

    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_conflict() {
        let content = "\
line1
line2
<<<<<<< HEAD
our change
=======
their change
>>>>>>> incoming
line3";
        let segments = parse_conflict_markers(content);
        assert_eq!(segments.len(), 3);
        assert!(matches!(&segments[0], MergeSegment::Context(lines) if lines.len() == 2));
        assert!(matches!(&segments[1], MergeSegment::Conflict(block)
            if block.ours == vec!["our change"] && block.theirs == vec!["their change"]));
        assert!(matches!(&segments[2], MergeSegment::Context(lines) if lines == &["line3"]));
    }

    #[test]
    fn test_parse_multiple_conflicts() {
        let content = "\
<<<<<<< HEAD
a
=======
b
>>>>>>> branch
middle
<<<<<<< HEAD
c
=======
d
>>>>>>> branch";
        let segments = parse_conflict_markers(content);
        assert_eq!(segments.len(), 3);
        assert!(matches!(&segments[0], MergeSegment::Conflict(_)));
        assert!(matches!(&segments[1], MergeSegment::Context(lines) if lines == &["middle"]));
        assert!(matches!(&segments[2], MergeSegment::Conflict(_)));
    }

    #[test]
    fn test_parse_no_conflicts() {
        let content = "just\nsome\ntext";
        let segments = parse_conflict_markers(content);
        assert_eq!(segments.len(), 1);
        assert!(matches!(&segments[0], MergeSegment::Context(lines) if lines.len() == 3));
    }

    #[test]
    fn test_parse_diff3_base_section() {
        let content = "\
<<<<<<< HEAD
ours
||||||| base
original
=======
theirs
>>>>>>> branch";
        let segments = parse_conflict_markers(content);
        assert_eq!(segments.len(), 1);
        if let MergeSegment::Conflict(block) = &segments[0] {
            assert_eq!(block.ours, vec!["ours"]);
            assert_eq!(block.theirs, vec!["theirs"]);
        } else {
            panic!("expected conflict");
        }
    }

    #[test]
    fn test_resolve_accept_current() {
        let segments = vec![MergeSegment::Conflict(ConflictBlock {
            ours: vec!["ours".to_string()],
            theirs: vec!["theirs".to_string()],
            resolution: Resolution::Unresolved,
        })];
        let mut view = MergeConflictView::new("test.rs".to_string(), segments);
        assert_eq!(view.total_conflicts, 1);
        assert!(!view.all_resolved());

        view.resolve(Resolution::AcceptCurrent);
        assert!(view.all_resolved());
        assert_eq!(view.build_result(), "ours");
    }

    #[test]
    fn test_resolve_accept_incoming() {
        let segments = vec![MergeSegment::Conflict(ConflictBlock {
            ours: vec!["ours".to_string()],
            theirs: vec!["theirs".to_string()],
            resolution: Resolution::Unresolved,
        })];
        let mut view = MergeConflictView::new("test.rs".to_string(), segments);
        view.resolve(Resolution::AcceptIncoming);
        assert_eq!(view.build_result(), "theirs");
    }

    #[test]
    fn test_resolve_both_current_first() {
        let segments = vec![MergeSegment::Conflict(ConflictBlock {
            ours: vec!["ours".to_string()],
            theirs: vec!["theirs".to_string()],
            resolution: Resolution::Unresolved,
        })];
        let mut view = MergeConflictView::new("test.rs".to_string(), segments);
        view.resolve(Resolution::AcceptBothCurrentFirst);
        assert_eq!(view.build_result(), "ours\ntheirs");
    }

    #[test]
    fn test_build_result_with_context() {
        let segments = vec![
            MergeSegment::Context(vec!["before".to_string()]),
            MergeSegment::Conflict(ConflictBlock {
                ours: vec!["ours".to_string()],
                theirs: vec!["theirs".to_string()],
                resolution: Resolution::AcceptIncoming,
            }),
            MergeSegment::Context(vec!["after".to_string()]),
        ];
        let view = MergeConflictView::new("test.rs".to_string(), segments);
        assert_eq!(view.build_result(), "before\ntheirs\nafter");
    }
}
