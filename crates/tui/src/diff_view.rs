//! Side-by-side diff view for reviewing git changes.

pub use crate::git::DiffLine;

/// State for the side-by-side diff view.
pub struct DiffView {
    /// Relative path of the file being diffed.
    pub file_path: String,
    /// Aligned diff lines for side-by-side rendering.
    pub lines: Vec<DiffLine>,
    /// Current scroll offset (top visible line).
    pub scroll: usize,
}

impl DiffView {
    /// Create a new diff view.
    pub fn new(file_path: String, lines: Vec<DiffLine>) -> Self {
        Self {
            file_path,
            lines,
            scroll: 0,
        }
    }

    /// Scroll up by `n` lines.
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll = self.scroll.saturating_sub(n);
    }

    /// Scroll down by `n` lines, clamped so content doesn't scroll past the end.
    pub fn scroll_down(&mut self, n: usize, viewport_height: usize) {
        let max_scroll = self.lines.len().saturating_sub(viewport_height);
        self.scroll = (self.scroll.saturating_add(n)).min(max_scroll);
    }

    /// Scroll to the top.
    pub fn scroll_to_top(&mut self) {
        self.scroll = 0;
    }

    /// Scroll to the bottom.
    pub fn scroll_to_bottom(&mut self, viewport_height: usize) {
        self.scroll = self.lines.len().saturating_sub(viewport_height);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::aligned_diff_lines;

    fn make(lines: Vec<DiffLine>) -> DiffView {
        DiffView::new("test.rs".to_string(), lines)
    }

    fn line_count(n: usize) -> Vec<DiffLine> {
        (0..n)
            .map(|i| DiffLine::Both(format!("line {i}"), format!("line {i}")))
            .collect()
    }

    #[test]
    fn scroll_up_saturates_at_zero() {
        let mut dv = make(line_count(50));
        dv.scroll = 5;
        dv.scroll_up(10);
        assert_eq!(dv.scroll, 0);
    }

    #[test]
    fn scroll_up_normal_case() {
        let mut dv = make(line_count(50));
        dv.scroll = 20;
        dv.scroll_up(7);
        assert_eq!(dv.scroll, 13);
    }

    #[test]
    fn scroll_down_clamps_to_max() {
        // 50 lines, viewport 10 → max scroll = 40.
        let mut dv = make(line_count(50));
        dv.scroll = 30;
        dv.scroll_down(50, 10);
        assert_eq!(dv.scroll, 40);
    }

    #[test]
    fn scroll_down_within_bounds() {
        let mut dv = make(line_count(50));
        dv.scroll = 5;
        dv.scroll_down(10, 10);
        assert_eq!(dv.scroll, 15);
    }

    #[test]
    fn scroll_down_with_viewport_larger_than_content() {
        // 5 lines, viewport 20 → max_scroll = 0.
        let mut dv = make(line_count(5));
        dv.scroll_down(5, 20);
        assert_eq!(dv.scroll, 0);
    }

    #[test]
    fn scroll_to_top_resets_to_zero() {
        let mut dv = make(line_count(50));
        dv.scroll = 30;
        dv.scroll_to_top();
        assert_eq!(dv.scroll, 0);
    }

    #[test]
    fn scroll_to_bottom_lands_at_max() {
        let mut dv = make(line_count(50));
        dv.scroll_to_bottom(10);
        assert_eq!(dv.scroll, 40);
    }

    #[test]
    fn scroll_to_bottom_with_oversized_viewport_lands_at_zero() {
        let mut dv = make(line_count(5));
        dv.scroll_to_bottom(20);
        assert_eq!(dv.scroll, 0);
    }

    #[test]
    fn aligned_diff_pure_addition_yields_only_right_only() {
        // New-file case: old text empty.
        let lines = aligned_diff_lines("", "a\nb\nc\n");
        assert_eq!(lines.len(), 3);
        assert!(
            lines.iter().all(|l| matches!(l, DiffLine::RightOnly(_))),
            "pure-addition diff should only contain RightOnly lines, got {lines:?}"
        );
    }

    #[test]
    fn aligned_diff_pure_deletion_yields_only_left_only() {
        // Full-delete case: new text empty.
        let lines = aligned_diff_lines("a\nb\nc\n", "");
        assert_eq!(lines.len(), 3);
        assert!(
            lines.iter().all(|l| matches!(l, DiffLine::LeftOnly(_))),
            "pure-deletion diff should only contain LeftOnly lines, got {lines:?}"
        );
    }

    #[test]
    fn aligned_diff_empty_inputs_produce_no_lines() {
        assert!(aligned_diff_lines("", "").is_empty());
    }

    #[test]
    fn aligned_diff_unchanged_yields_only_both() {
        let lines = aligned_diff_lines("a\nb\nc\n", "a\nb\nc\n");
        assert_eq!(lines.len(), 3);
        assert!(
            lines.iter().all(|l| matches!(l, DiffLine::Both(_, _))),
            "unchanged diff should only contain Both lines, got {lines:?}"
        );
    }

    #[test]
    fn aligned_diff_modification_mixes_variants() {
        // Replace middle line: should produce Both, then Left/Right pair, then Both.
        let lines = aligned_diff_lines("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(lines.len(), 4);
        // First and last are unchanged.
        assert!(matches!(&lines[0], DiffLine::Both(l, _) if l == "a"));
        assert!(matches!(&lines[lines.len() - 1], DiffLine::Both(l, _) if l == "c"));
        // Middle has one LeftOnly and one RightOnly (in some order).
        let has_left = lines
            .iter()
            .any(|l| matches!(l, DiffLine::LeftOnly(s) if s == "b"));
        let has_right = lines
            .iter()
            .any(|l| matches!(l, DiffLine::RightOnly(s) if s == "B"));
        assert!(has_left && has_right, "expected b→B replacement");
    }
}
