//! Side-by-side diff view for reviewing git changes.

pub use crate::git::DiffLine;

/// State for the side-by-side diff view.
///
/// Several derived values (the reconstructed `old_text` / `new_text` strings
/// fed to the syntax highlighter, and the cumulative line-number prefix sum
/// used to label each visible row) are computed once in [`Self::new`] and
/// cached. The renderer used to rebuild them on every frame, which was
/// O(total_lines) per frame even when only the scroll offset changed.
///
/// Mutating [`Self::lines`] after construction invalidates the caches and is
/// not supported. Construct a fresh `DiffView` whenever the diff content
/// changes.
pub struct DiffView {
    /// Relative path of the file being diffed.
    pub file_path: String,
    /// Aligned diff lines for side-by-side rendering.
    pub lines: Vec<DiffLine>,
    /// Current scroll offset (top visible line).
    pub scroll: usize,
    /// Reconstructed left-side (old) text fed to the syntax highlighter.
    /// Cached from `lines` at construction.
    old_text: String,
    /// Reconstructed right-side (new) text fed to the syntax highlighter.
    /// Cached from `lines` at construction.
    new_text: String,
    /// Cumulative `(old_count, new_count)` prefix sum.
    /// `cumulative[i]` is the count of old/new lines emitted by the first
    /// `i` entries of `lines` (so `cumulative[0] == (0, 0)` and
    /// `cumulative[lines.len()]` is the total). Used to map a scroll
    /// offset into the line-number gutter and into highlight-array
    /// indices in O(1) per render.
    cumulative: Vec<(usize, usize)>,
}

impl DiffView {
    /// Create a new diff view.
    ///
    /// Precomputes the highlighter-input strings and the line-number prefix
    /// sum so the per-frame render is O(viewport) rather than O(total).
    pub fn new(file_path: String, lines: Vec<DiffLine>) -> Self {
        let mut old_text = String::new();
        let mut new_text = String::new();
        let mut cumulative = Vec::with_capacity(lines.len() + 1);
        cumulative.push((0, 0));
        let (mut old_n, mut new_n) = (0usize, 0usize);
        for line in &lines {
            match line {
                DiffLine::Both(l, _) => {
                    old_text.push_str(l);
                    old_text.push('\n');
                    new_text.push_str(l);
                    new_text.push('\n');
                    old_n += 1;
                    new_n += 1;
                }
                DiffLine::LeftOnly(l) => {
                    old_text.push_str(l);
                    old_text.push('\n');
                    old_n += 1;
                }
                DiffLine::RightOnly(r) => {
                    new_text.push_str(r);
                    new_text.push('\n');
                    new_n += 1;
                }
            }
            cumulative.push((old_n, new_n));
        }
        Self {
            file_path,
            lines,
            scroll: 0,
            old_text,
            new_text,
            cumulative,
        }
    }

    /// Reconstructed left-side (old) text, suitable for syntax highlighting.
    pub fn old_text(&self) -> &str {
        &self.old_text
    }

    /// Reconstructed right-side (new) text, suitable for syntax highlighting.
    pub fn new_text(&self) -> &str {
        &self.new_text
    }

    /// Cumulative `(old_count, new_count)` after processing the first
    /// `line_idx` entries of [`Self::lines`].
    ///
    /// `line_numbers_at(0)` is `(0, 0)`. `line_numbers_at(self.lines.len())`
    /// is the total `(old, new)` count. Out-of-range indices clamp to the
    /// last valid value.
    pub fn line_numbers_at(&self, line_idx: usize) -> (usize, usize) {
        let i = line_idx.min(self.cumulative.len().saturating_sub(1));
        self.cumulative[i]
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
    fn cached_old_text_concatenates_left_visible_lines() {
        let lines = vec![
            DiffLine::Both("a".to_string(), "a".to_string()),
            DiffLine::LeftOnly("removed".to_string()),
            DiffLine::RightOnly("added".to_string()),
            DiffLine::Both("c".to_string(), "c".to_string()),
        ];
        let dv = make(lines);
        // Old side sees: a, removed, c (RightOnly absent).
        assert_eq!(dv.old_text(), "a\nremoved\nc\n");
    }

    #[test]
    fn cached_new_text_concatenates_right_visible_lines() {
        let lines = vec![
            DiffLine::Both("a".to_string(), "a".to_string()),
            DiffLine::LeftOnly("removed".to_string()),
            DiffLine::RightOnly("added".to_string()),
            DiffLine::Both("c".to_string(), "c".to_string()),
        ];
        let dv = make(lines);
        // New side sees: a, added, c (LeftOnly absent).
        assert_eq!(dv.new_text(), "a\nadded\nc\n");
    }

    #[test]
    fn line_numbers_at_zero_is_origin() {
        let dv = make(line_count(10));
        assert_eq!(dv.line_numbers_at(0), (0, 0));
    }

    #[test]
    fn line_numbers_at_end_is_total() {
        // 5 Both + 3 LeftOnly + 2 RightOnly → total old=8, new=7.
        let mut lines = Vec::new();
        for i in 0..5 {
            lines.push(DiffLine::Both(format!("b{i}"), format!("b{i}")));
        }
        for i in 0..3 {
            lines.push(DiffLine::LeftOnly(format!("l{i}")));
        }
        for i in 0..2 {
            lines.push(DiffLine::RightOnly(format!("r{i}")));
        }
        let total = lines.len();
        let dv = make(lines);
        assert_eq!(dv.line_numbers_at(total), (8, 7));
    }

    #[test]
    fn line_numbers_at_progresses_correctly_per_variant() {
        let lines = vec![
            DiffLine::Both("a".to_string(), "a".to_string()), // (1,1)
            DiffLine::LeftOnly("b".to_string()),              // (2,1)
            DiffLine::RightOnly("c".to_string()),             // (2,2)
            DiffLine::Both("d".to_string(), "d".to_string()), // (3,3)
        ];
        let dv = make(lines);
        assert_eq!(dv.line_numbers_at(0), (0, 0));
        assert_eq!(dv.line_numbers_at(1), (1, 1));
        assert_eq!(dv.line_numbers_at(2), (2, 1));
        assert_eq!(dv.line_numbers_at(3), (2, 2));
        assert_eq!(dv.line_numbers_at(4), (3, 3));
    }

    #[test]
    fn line_numbers_at_clamps_oversized_index() {
        let dv = make(line_count(5));
        // 5 Both lines → (5, 5) is the last valid value.
        assert_eq!(dv.line_numbers_at(999), (5, 5));
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
