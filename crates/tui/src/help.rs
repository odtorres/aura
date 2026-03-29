//! In-editor help overlay that renders embedded documentation.
//!
//! The help system embeds all mdBook documentation at compile time via
//! `include_str!` and provides a two-view overlay: a searchable topic
//! browser and a scrollable content viewer with basic markdown rendering.

use ratatui::prelude::*;
use ratatui::text::Line as RatatuiLine;

// ---------------------------------------------------------------------------
// Compile-time embedded documentation
// ---------------------------------------------------------------------------

const DOC_INTRODUCTION: &str = include_str!("../../../docs/src/introduction.md");
const DOC_INSTALLATION: &str = include_str!("../../../docs/src/getting-started/installation.md");
const DOC_FIRST_RUN: &str = include_str!("../../../docs/src/getting-started/first-run.md");
const DOC_CONFIGURATION: &str = include_str!("../../../docs/src/getting-started/configuration.md");
const DOC_MODES: &str = include_str!("../../../docs/src/user-guide/modes.md");
const DOC_KEYBINDINGS: &str = include_str!("../../../docs/src/user-guide/keybindings.md");
const DOC_FILE_NAVIGATION: &str = include_str!("../../../docs/src/user-guide/file-navigation.md");
const DOC_TERMINAL: &str = include_str!("../../../docs/src/user-guide/terminal.md");
const DOC_AI_FEATURES: &str = include_str!("../../../docs/src/user-guide/ai-features.md");
const DOC_GIT: &str = include_str!("../../../docs/src/user-guide/git.md");
const DOC_LSP: &str = include_str!("../../../docs/src/user-guide/lsp.md");
const DOC_PLUGINS: &str = include_str!("../../../docs/src/user-guide/plugins.md");
const DOC_DEBUGGER: &str = include_str!("../../../docs/src/user-guide/debugger.md");
const DOC_ARCH_OVERVIEW: &str = include_str!("../../../docs/src/architecture/overview.md");
const DOC_ARCH_CORE: &str = include_str!("../../../docs/src/architecture/core.md");
const DOC_ARCH_TUI: &str = include_str!("../../../docs/src/architecture/tui.md");
const DOC_ARCH_AI: &str = include_str!("../../../docs/src/architecture/ai.md");
const DOC_ARCH_MCP: &str = include_str!("../../../docs/src/architecture/mcp.md");
const DOC_DEV_GUIDE: &str = include_str!("../../../docs/src/contributing/development.md");
const DOC_TESTING: &str = include_str!("../../../docs/src/contributing/testing.md");
const DOC_ROADMAP: &str = include_str!("../../../docs/src/contributing/roadmap.md");
const DOC_API_REF: &str = include_str!("../../../docs/src/api-reference.md");

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single help topic backed by embedded documentation.
pub struct HelpTopic {
    /// Display title (e.g. "Keybindings").
    pub title: String,
    /// Section grouping (e.g. "User Guide").
    pub section: String,
    /// Raw embedded markdown content.
    pub raw: &'static str,
    /// Pre-rendered ratatui lines.
    pub rendered: Vec<RatatuiLine<'static>>,
}

/// Which view the help overlay is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelpView {
    /// Searchable topic list.
    Topics,
    /// Rendered content for a single topic.
    Content,
}

/// The help overlay state.
pub struct HelpOverlay {
    /// Whether the overlay is currently visible.
    pub visible: bool,
    /// Current view mode.
    view: HelpView,
    /// All available topics.
    topics: Vec<HelpTopic>,
    /// Current search query in the topic browser.
    pub query: String,
    /// Indices into `topics` that match the current query.
    pub filtered: Vec<usize>,
    /// Currently selected index in the filtered list.
    pub selected: usize,
    /// Index of the topic currently being viewed in content view.
    current_topic: usize,
    /// Scroll offset in the content view.
    pub scroll: usize,
    /// Search index: (topic_idx, lowercase full text) for content search.
    search_index: Vec<(usize, String)>,
}

impl Default for HelpOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl HelpOverlay {
    /// Build the help overlay with all embedded documentation topics.
    pub fn new() -> Self {
        let topic_defs: Vec<(&str, &str, &'static str)> = vec![
            ("Introduction", "Introduction", DOC_INTRODUCTION),
            ("Installation", "Getting Started", DOC_INSTALLATION),
            ("First Run", "Getting Started", DOC_FIRST_RUN),
            ("Configuration", "Getting Started", DOC_CONFIGURATION),
            ("Modes", "User Guide", DOC_MODES),
            ("Keybindings", "User Guide", DOC_KEYBINDINGS),
            ("File Navigation", "User Guide", DOC_FILE_NAVIGATION),
            ("Terminal", "User Guide", DOC_TERMINAL),
            ("AI Features", "User Guide", DOC_AI_FEATURES),
            ("Git Integration", "User Guide", DOC_GIT),
            ("LSP", "User Guide", DOC_LSP),
            ("Plugins", "User Guide", DOC_PLUGINS),
            ("Debugger (DAP)", "User Guide", DOC_DEBUGGER),
            ("Architecture Overview", "Architecture", DOC_ARCH_OVERVIEW),
            ("Core Crate", "Architecture", DOC_ARCH_CORE),
            ("TUI Crate", "Architecture", DOC_ARCH_TUI),
            ("AI Crate", "Architecture", DOC_ARCH_AI),
            ("MCP Protocol", "Architecture", DOC_ARCH_MCP),
            ("Development Guide", "Contributing", DOC_DEV_GUIDE),
            ("Testing", "Contributing", DOC_TESTING),
            ("Roadmap", "Contributing", DOC_ROADMAP),
            ("API Reference", "API Reference", DOC_API_REF),
        ];

        let mut topics = Vec::with_capacity(topic_defs.len());
        let mut search_index = Vec::with_capacity(topic_defs.len());

        for (title, section, raw) in topic_defs {
            let rendered = render_markdown(raw);
            let lowercase_text = format!("{} {} {}", title, section, raw).to_lowercase();
            let idx = topics.len();
            search_index.push((idx, lowercase_text));
            topics.push(HelpTopic {
                title: title.to_string(),
                section: section.to_string(),
                raw,
                rendered,
            });
        }

        let filtered: Vec<usize> = (0..topics.len()).collect();

        Self {
            visible: false,
            view: HelpView::Topics,
            topics,
            query: String::new(),
            filtered,
            selected: 0,
            current_topic: 0,
            scroll: 0,
            search_index,
        }
    }

    /// Open the help overlay to the topic browser.
    pub fn open(&mut self) {
        self.visible = true;
        self.view = HelpView::Topics;
        self.query.clear();
        self.selected = 0;
        self.scroll = 0;
        self.filter();
    }

    /// Open the help overlay directly to a specific topic by slug.
    ///
    /// The slug is matched case-insensitively against topic titles. If no
    /// match is found, the topic browser is shown with the slug as query.
    pub fn open_topic(&mut self, slug: &str) {
        self.visible = true;
        let slug_lower = slug.to_lowercase();
        if let Some(idx) = self.topics.iter().position(|t| {
            t.title.to_lowercase() == slug_lower || t.title.to_lowercase().contains(&slug_lower)
        }) {
            self.current_topic = idx;
            self.scroll = 0;
            self.view = HelpView::Content;
        } else {
            // No exact match — open topics view with slug as search query.
            self.view = HelpView::Topics;
            self.query = slug.to_string();
            self.selected = 0;
            self.filter();
        }
    }

    /// Close the help overlay.
    pub fn close(&mut self) {
        self.visible = false;
        self.view = HelpView::Topics;
        self.query.clear();
    }

    /// Append a character to the search query and re-filter.
    pub fn type_char(&mut self, c: char) {
        self.query.push(c);
        self.selected = 0;
        self.filter();
    }

    /// Remove the last character from the search query and re-filter.
    pub fn backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
        self.filter();
    }

    /// Move selection up in the topic list.
    pub fn select_up(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.filtered.len().saturating_sub(1);
        } else {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    /// Move selection down in the topic list.
    pub fn select_down(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.filtered.len();
    }

    /// Open the currently selected topic in the content view.
    pub fn enter(&mut self) {
        if self.view == HelpView::Topics {
            if let Some(&idx) = self.filtered.get(self.selected) {
                self.current_topic = idx;
                self.scroll = 0;
                self.view = HelpView::Content;
            }
        }
    }

    /// Go back from content view to topics view.
    pub fn back(&mut self) {
        if self.view == HelpView::Content {
            self.view = HelpView::Topics;
        } else {
            self.close();
        }
    }

    /// Scroll content up by one line.
    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    /// Scroll content down by one line.
    pub fn scroll_down(&mut self) {
        if let Some(topic) = self.topics.get(self.current_topic) {
            let max = topic.rendered.len().saturating_sub(1);
            if self.scroll < max {
                self.scroll += 1;
            }
        }
    }

    /// Scroll content up by `h` lines (page up).
    pub fn page_up(&mut self, h: usize) {
        self.scroll = self.scroll.saturating_sub(h);
    }

    /// Scroll content down by `h` lines (page down).
    pub fn page_down(&mut self, h: usize) {
        if let Some(topic) = self.topics.get(self.current_topic) {
            let max = topic.rendered.len().saturating_sub(1);
            self.scroll = (self.scroll + h).min(max);
        }
    }

    /// Whether we are in the topic browser view.
    pub fn in_topics_view(&self) -> bool {
        self.view == HelpView::Topics
    }

    /// Whether we are in the content viewer.
    pub fn in_content_view(&self) -> bool {
        self.view == HelpView::Content
    }

    /// Return the current topic being viewed (if in content view).
    pub fn current_topic(&self) -> Option<&HelpTopic> {
        if self.view == HelpView::Content {
            self.topics.get(self.current_topic)
        } else {
            None
        }
    }

    /// Access all topics.
    pub fn topics(&self) -> &[HelpTopic] {
        &self.topics
    }

    // -----------------------------------------------------------------------
    // Private
    // -----------------------------------------------------------------------

    /// Re-filter topics against the current query using fuzzy title match
    /// plus full-text content search.
    fn filter(&mut self) {
        let query_lower = self.query.to_lowercase();

        if query_lower.is_empty() {
            self.filtered = (0..self.topics.len()).collect();
            return;
        }

        // Score each topic: title fuzzy match gets priority, then content hits.
        let mut scored: Vec<(usize, i32)> = Vec::new();

        for (topic_idx, lowercase_text) in &self.search_index {
            let title_lower = self.topics[*topic_idx].title.to_lowercase();
            let title_fuzzy = is_fuzzy_match(&title_lower, &query_lower);
            let title_exact = title_lower.contains(&query_lower);
            let content_hits = lowercase_text.matches(&query_lower).count();

            if title_fuzzy || content_hits > 0 {
                let score = if title_exact {
                    1000
                } else if title_fuzzy {
                    500
                } else {
                    0
                } + content_hits as i32;
                scored.push((*topic_idx, score));
            }
        }

        scored.sort_by(|a, b| b.1.cmp(&a.1));
        self.filtered = scored.into_iter().map(|(idx, _)| idx).collect();

        if self.filtered.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
        }
    }
}

/// Returns `true` if every character of `query` appears in `text` in order.
fn is_fuzzy_match(text: &str, query: &str) -> bool {
    let mut text_chars = text.chars();
    'outer: for qc in query.chars() {
        for tc in text_chars.by_ref() {
            if tc == qc {
                continue 'outer;
            }
        }
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// Markdown renderer
// ---------------------------------------------------------------------------

/// Render raw markdown into styled ratatui lines.
///
/// Handles headings, bold, inline code, code blocks, bullet lists, tables,
/// and mdBook `{{#include ...}}` directives (skipped).
pub fn render_markdown(raw: &str) -> Vec<RatatuiLine<'static>> {
    let mut lines: Vec<RatatuiLine<'static>> = Vec::new();
    let mut in_code_block = false;

    for line in raw.lines() {
        // Skip mdBook include directives.
        if line.trim_start().starts_with("{{#include") {
            continue;
        }

        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            // Show a dim separator for code block boundaries.
            lines.push(RatatuiLine::from(Span::styled(
                line.to_string(),
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }

        if in_code_block {
            lines.push(RatatuiLine::from(Span::styled(
                format!("  {line}"),
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }

        // Headings
        if line.starts_with("### ") {
            let text = line.trim_start_matches('#').trim();
            lines.push(RatatuiLine::from(Span::styled(
                format!("   {text}"),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if line.starts_with("## ") {
            let text = line.trim_start_matches('#').trim();
            lines.push(RatatuiLine::from(Span::styled(
                format!("  {text}"),
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if line.starts_with("# ") {
            let text = line.trim_start_matches('#').trim();
            lines.push(RatatuiLine::from(Span::styled(
                text.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        // Table rows — render verbatim.
        if line.trim_start().starts_with('|') {
            lines.push(RatatuiLine::from(Span::styled(
                line.to_string(),
                Style::default().fg(Color::White),
            )));
            continue;
        }

        // Bullet lists — render with indent.
        if line.trim_start().starts_with("- ") {
            let indent = line.len() - line.trim_start().len();
            let text = line.trim_start().strip_prefix("- ").unwrap_or(line);
            let prefix = " ".repeat(indent);
            let spans = render_inline(&format!("{prefix}  {text}"));
            // Prepend bullet
            let mut all_spans = vec![Span::styled(
                format!("{prefix}  "),
                Style::default().fg(Color::DarkGray),
            )];
            all_spans.extend(spans);
            lines.push(RatatuiLine::from(all_spans));
            continue;
        }

        // Horizontal rule
        if line.trim() == "---" {
            lines.push(RatatuiLine::from(Span::styled(
                "─".repeat(40),
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }

        // Regular paragraph text — handle inline formatting.
        if line.trim().is_empty() {
            lines.push(RatatuiLine::from(""));
        } else {
            let spans = render_inline(line);
            lines.push(RatatuiLine::from(spans));
        }
    }

    lines
}

/// Parse inline markdown formatting (bold, inline code) into styled spans.
fn render_inline(text: &str) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut chars = text.chars().peekable();
    let mut current = String::new();

    while let Some(c) = chars.next() {
        if c == '`' {
            // Flush current text.
            if !current.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current),
                    Style::default().fg(Color::White),
                ));
            }
            // Collect code span.
            let mut code = String::new();
            for cc in chars.by_ref() {
                if cc == '`' {
                    break;
                }
                code.push(cc);
            }
            spans.push(Span::styled(code, Style::default().fg(Color::Yellow)));
        } else if c == '*' && chars.peek() == Some(&'*') {
            chars.next(); // consume second *
                          // Flush current text.
            if !current.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current),
                    Style::default().fg(Color::White),
                ));
            }
            // Collect bold span.
            let mut bold = String::new();
            loop {
                match chars.next() {
                    Some('*') if chars.peek() == Some(&'*') => {
                        chars.next();
                        break;
                    }
                    Some(bc) => bold.push(bc),
                    None => break,
                }
            }
            spans.push(Span::styled(
                bold,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            current.push(c);
        }
    }

    // Flush remaining text.
    if !current.is_empty() {
        spans.push(Span::styled(current, Style::default().fg(Color::White)));
    }

    if spans.is_empty() {
        spans.push(Span::raw(""));
    }

    spans
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_help_overlay_new_has_topics() {
        let help = HelpOverlay::new();
        assert!(!help.topics.is_empty());
        assert_eq!(help.filtered.len(), help.topics.len());
    }

    #[test]
    fn test_open_close() {
        let mut help = HelpOverlay::new();
        assert!(!help.visible);
        help.open();
        assert!(help.visible);
        assert!(help.in_topics_view());
        help.close();
        assert!(!help.visible);
    }

    #[test]
    fn test_search_filters_topics() {
        let mut help = HelpOverlay::new();
        help.open();
        for c in "keybind".chars() {
            help.type_char(c);
        }
        assert!(!help.filtered.is_empty());
        // "Keybindings" should be the top result.
        let top_idx = help.filtered[0];
        assert!(
            help.topics[top_idx]
                .title
                .to_lowercase()
                .contains("keybind"),
            "Expected Keybindings in top result, got: {}",
            help.topics[top_idx].title
        );
    }

    #[test]
    fn test_navigation() {
        let mut help = HelpOverlay::new();
        help.open();
        assert_eq!(help.selected, 0);
        help.select_down();
        assert_eq!(help.selected, 1);
        help.select_up();
        assert_eq!(help.selected, 0);
        // Wrap around.
        help.select_up();
        assert_eq!(help.selected, help.filtered.len() - 1);
    }

    #[test]
    fn test_enter_and_back() {
        let mut help = HelpOverlay::new();
        help.open();
        help.enter();
        assert!(help.in_content_view());
        assert!(help.current_topic().is_some());
        help.back();
        assert!(help.in_topics_view());
    }

    #[test]
    fn test_open_topic_by_slug() {
        let mut help = HelpOverlay::new();
        help.open_topic("keybindings");
        assert!(help.visible);
        assert!(help.in_content_view());
        assert_eq!(
            help.topics[help.current_topic].title.to_lowercase(),
            "keybindings"
        );
    }

    #[test]
    fn test_open_topic_partial_slug() {
        let mut help = HelpOverlay::new();
        help.open_topic("git");
        assert!(help.visible);
        assert!(help.in_content_view());
        assert!(help.topics[help.current_topic]
            .title
            .to_lowercase()
            .contains("git"));
    }

    #[test]
    fn test_open_topic_unknown_falls_back_to_search() {
        let mut help = HelpOverlay::new();
        help.open_topic("zzzznotfound");
        assert!(help.visible);
        assert!(help.in_topics_view());
        assert_eq!(help.query, "zzzznotfound");
    }

    #[test]
    fn test_content_scrolling() {
        let mut help = HelpOverlay::new();
        help.open_topic("keybindings");
        assert_eq!(help.scroll, 0);
        help.scroll_down();
        assert_eq!(help.scroll, 1);
        help.scroll_up();
        assert_eq!(help.scroll, 0);
        help.page_down(10);
        assert_eq!(help.scroll, 10);
        help.page_up(5);
        assert_eq!(help.scroll, 5);
    }

    #[test]
    fn test_render_markdown_headings() {
        let md = "# Title\n## Subtitle\n### Section\nPlain text\n";
        let lines = render_markdown(md);
        assert!(!lines.is_empty());
        // First line should be the H1 heading.
        assert!(lines[0].spans.iter().any(|s| s.content.contains("Title")));
    }

    #[test]
    fn test_render_markdown_code_block() {
        let md = "Before\n```rust\nlet x = 1;\n```\nAfter\n";
        let lines = render_markdown(md);
        assert!(lines.len() >= 4);
    }

    #[test]
    fn test_render_inline_bold_and_code() {
        let spans = render_inline("Use **bold** and `code` here");
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("bold"));
        assert!(text.contains("code"));
    }

    #[test]
    fn test_fuzzy_match() {
        assert!(is_fuzzy_match("keybindings", "keyb"));
        assert!(is_fuzzy_match("keybindings", "kbn"));
        assert!(!is_fuzzy_match("keybindings", "xyz"));
    }

    #[test]
    fn test_empty_search_shows_all() {
        let mut help = HelpOverlay::new();
        help.open();
        assert_eq!(help.filtered.len(), help.topics.len());
    }

    #[test]
    fn test_backspace_widens_search() {
        let mut help = HelpOverlay::new();
        help.open();
        let initial = help.filtered.len();
        for c in "zzzzz".chars() {
            help.type_char(c);
        }
        let narrowed = help.filtered.len();
        assert!(narrowed <= initial);
        for _ in 0..5 {
            help.backspace();
        }
        assert_eq!(help.filtered.len(), initial);
    }
}
