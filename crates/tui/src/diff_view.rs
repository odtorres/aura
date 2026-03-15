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
