//! Syntax highlighting via Tree-sitter.
//!
//! Provides incremental parsing and highlight-query-based colorisation for
//! supported languages. The highlighter is attached to the buffer and
//! re-parses incrementally on each edit.

use crate::config::Theme;
use ratatui::style::Color;
use tree_sitter::Parser;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

/// Recognised highlight group names, mapped to terminal colours.
const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constant.builtin",
    "constructor",
    "function",
    "function.builtin",
    "function.macro",
    "keyword",
    "label",
    "number",
    "operator",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "string",
    "string.special",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

/// Map a highlight group index to a terminal colour, consulting the theme when
/// available and falling back to built-in defaults otherwise.
fn highlight_color(idx: usize, theme: Option<&Theme>) -> Color {
    match HIGHLIGHT_NAMES.get(idx) {
        Some(&"comment") => theme.map(|t| t.comment).unwrap_or(Color::DarkGray),
        Some(&"keyword") => theme.map(|t| t.keyword).unwrap_or(Color::Magenta),
        Some(&"string") | Some(&"string.special") => {
            theme.map(|t| t.string).unwrap_or(Color::Green)
        }
        Some(&"number") | Some(&"constant" | &"constant.builtin") => {
            theme.map(|t| t.number).unwrap_or(Color::Yellow)
        }
        Some(&"function" | &"function.builtin" | &"function.macro") => {
            theme.map(|t| t.function).unwrap_or(Color::Blue)
        }
        Some(&"type" | &"type.builtin") => theme.map(|t| t.type_name).unwrap_or(Color::Cyan),
        Some(&"operator") => Color::LightRed,
        Some(&"variable.builtin" | &"variable.parameter") => Color::LightYellow,
        Some(&"attribute") => Color::LightCyan,
        Some(&"constructor") => Color::LightBlue,
        Some(&"property") => Color::LightGreen,
        Some(&"punctuation" | &"punctuation.bracket" | &"punctuation.delimiter") => Color::Reset,
        _ => Color::Reset,
    }
}

/// Supported languages for syntax highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    Python,
    TypeScript,
    Tsx,
    Go,
}

impl Language {
    /// Detect language from a file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Self::Rust),
            "py" => Some(Self::Python),
            "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "go" => Some(Self::Go),
            _ => None,
        }
    }
}

/// Per-character colour information for a line of text.
#[derive(Debug, Clone, Default)]
pub struct HighlightedLine {
    /// One colour per character in the line.
    pub colors: Vec<Color>,
}

/// Syntax highlighter that wraps tree-sitter.
pub struct SyntaxHighlighter {
    language: Language,
    config: HighlightConfiguration,
    parser: Parser,
    /// Most-recently parsed tree, kept for node-at-position queries.
    last_tree: Option<tree_sitter::Tree>,
}

impl SyntaxHighlighter {
    /// Create a new highlighter for the given language.
    pub fn new(language: Language) -> Option<Self> {
        let (ts_language, highlights_query): (tree_sitter::Language, &str) = match language {
            Language::Rust => (
                tree_sitter_rust::LANGUAGE.into(),
                tree_sitter_rust::HIGHLIGHTS_QUERY,
            ),
            Language::Python => (
                tree_sitter_python::LANGUAGE.into(),
                tree_sitter_python::HIGHLIGHTS_QUERY,
            ),
            Language::TypeScript => (
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
            ),
            Language::Tsx => (
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
            ),
            Language::Go => (
                tree_sitter_go::LANGUAGE.into(),
                tree_sitter_go::HIGHLIGHTS_QUERY,
            ),
        };

        let mut config =
            HighlightConfiguration::new(ts_language.clone(), "highlight", highlights_query, "", "")
                .ok()?;
        config.configure(HIGHLIGHT_NAMES);

        let mut parser = Parser::new();
        parser.set_language(&ts_language).ok()?;

        Some(Self {
            language,
            config,
            parser,
            last_tree: None,
        })
    }

    /// Highlight the given source text and return per-line colour information.
    ///
    /// When a `theme` is provided the theme's syntax colours are used;
    /// otherwise built-in defaults are applied.
    pub fn highlight(&mut self, source: &str, theme: Option<&Theme>) -> Vec<HighlightedLine> {
        // Re-parse the tree so node-at-position queries stay current.
        self.last_tree = self.parser.parse(source, None);

        let mut highlighter = Highlighter::new();
        let events = highlighter.highlight(&self.config, source.as_bytes(), None, |_| None);

        let events = match events {
            Ok(e) => e,
            Err(_) => return self.empty_lines(source),
        };

        let mut result: Vec<HighlightedLine> = Vec::new();
        let mut current_line = HighlightedLine::default();
        let mut current_color = Color::Reset;

        let source_bytes = source.as_bytes();
        let mut byte_offset = 0;

        for event in events {
            match event {
                Ok(HighlightEvent::Source { start, end }) => {
                    // Walk through the source bytes in this range.
                    let slice = &source_bytes[start..end];
                    for &b in slice {
                        if b == b'\n' {
                            result.push(std::mem::take(&mut current_line));
                        } else {
                            // Only push colour for non-continuation bytes (start of a UTF-8 char).
                            if (b & 0xC0) != 0x80 {
                                current_line.colors.push(current_color);
                            }
                        }
                    }
                    byte_offset = end;
                }
                Ok(HighlightEvent::HighlightStart(highlight)) => {
                    current_color = highlight_color(highlight.0, theme);
                }
                Ok(HighlightEvent::HighlightEnd) => {
                    current_color = Color::Reset;
                }
                Err(_) => break,
            }
        }

        // Handle any remaining bytes after the last event.
        if byte_offset < source_bytes.len() {
            for &b in &source_bytes[byte_offset..] {
                if b == b'\n' {
                    result.push(std::mem::take(&mut current_line));
                } else if (b & 0xC0) != 0x80 {
                    current_line.colors.push(current_color);
                }
            }
        }

        // Push the last line if non-empty.
        if !current_line.colors.is_empty() {
            result.push(current_line);
        }

        result
    }

    /// Return the tree-sitter node kind (e.g. `"function_item"`, `"identifier"`)
    /// for the innermost node covering the given byte offset in the source.
    ///
    /// Returns `None` if no tree has been parsed yet or the offset is out of range.
    pub fn node_type_at_byte(&self, byte_offset: usize) -> Option<&str> {
        let tree = self.last_tree.as_ref()?;
        let node = tree
            .root_node()
            .named_descendant_for_byte_range(byte_offset, byte_offset)?;
        Some(node.kind())
    }

    /// Return empty (no colour) lines for fallback.
    fn empty_lines(&self, source: &str) -> Vec<HighlightedLine> {
        source
            .lines()
            .map(|line| HighlightedLine {
                colors: vec![Color::Reset; line.chars().count()],
            })
            .collect()
    }

    /// Get the detected language.
    pub fn language(&self) -> Language {
        self.language
    }
}
