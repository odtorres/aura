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
        Some(&"comment") => theme
            .map(|t| t.comment)
            .unwrap_or(Color::Rgb(100, 100, 100)),
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
    /// Rust language.
    Rust,
    /// Python language.
    Python,
    /// TypeScript language.
    TypeScript,
    /// TSX (TypeScript JSX) language.
    Tsx,
    /// Go language.
    Go,
    /// JavaScript language.
    JavaScript,
    /// Java language.
    Java,
    /// C language.
    C,
    /// C++ language.
    Cpp,
    /// Ruby language.
    Ruby,
    /// HTML language.
    Html,
    /// CSS language.
    Css,
    /// JSON language.
    Json,
    /// Bash / Shell language.
    Bash,
    /// TOML language.
    Toml,
    /// YAML language.
    Yaml,
    /// Markdown language.
    Markdown,
}

impl Language {
    /// Detect language from a file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Self::Rust),
            "py" => Some(Self::Python),
            "ts" | "mts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "go" => Some(Self::Go),
            "js" | "mjs" | "cjs" => Some(Self::JavaScript),
            "jsx" => Some(Self::Tsx), // JSX uses the TSX grammar for JSX support
            "java" => Some(Self::Java),
            "c" | "h" => Some(Self::C),
            "cpp" | "cxx" | "cc" | "hpp" | "hxx" | "hh" => Some(Self::Cpp),
            "rb" => Some(Self::Ruby),
            "html" | "htm" => Some(Self::Html),
            "css" => Some(Self::Css),
            "json" => Some(Self::Json),
            "sh" | "bash" | "zsh" => Some(Self::Bash),
            "toml" => Some(Self::Toml),
            "yaml" | "yml" => Some(Self::Yaml),
            "md" | "markdown" => Some(Self::Markdown),
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
            Language::JavaScript => (
                tree_sitter_javascript::LANGUAGE.into(),
                tree_sitter_javascript::HIGHLIGHT_QUERY,
            ),
            Language::Java => (
                tree_sitter_java::LANGUAGE.into(),
                tree_sitter_java::HIGHLIGHTS_QUERY,
            ),
            Language::C => (
                tree_sitter_c::LANGUAGE.into(),
                tree_sitter_c::HIGHLIGHT_QUERY,
            ),
            Language::Cpp => (
                tree_sitter_cpp::LANGUAGE.into(),
                tree_sitter_cpp::HIGHLIGHT_QUERY,
            ),
            Language::Ruby => (
                tree_sitter_ruby::LANGUAGE.into(),
                tree_sitter_ruby::HIGHLIGHTS_QUERY,
            ),
            Language::Html => (
                tree_sitter_html::LANGUAGE.into(),
                tree_sitter_html::HIGHLIGHTS_QUERY,
            ),
            Language::Css => (
                tree_sitter_css::LANGUAGE.into(),
                tree_sitter_css::HIGHLIGHTS_QUERY,
            ),
            Language::Json => (
                tree_sitter_json::LANGUAGE.into(),
                tree_sitter_json::HIGHLIGHTS_QUERY,
            ),
            Language::Bash => (
                tree_sitter_bash::LANGUAGE.into(),
                tree_sitter_bash::HIGHLIGHT_QUERY,
            ),
            Language::Toml => (
                tree_sitter_toml_ng::LANGUAGE.into(),
                tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
            ),
            Language::Yaml => (
                tree_sitter_yaml::LANGUAGE.into(),
                tree_sitter_yaml::HIGHLIGHTS_QUERY,
            ),
            Language::Markdown => (
                tree_sitter_md::LANGUAGE.into(),
                tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
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

    /// Compute foldable ranges from the tree-sitter AST.
    ///
    /// Returns a map of start_line → end_line for nodes that span 2+ lines
    /// and are typically foldable (functions, blocks, structs, classes, etc.).
    pub fn foldable_ranges(&self) -> std::collections::HashMap<usize, usize> {
        let mut ranges = std::collections::HashMap::new();
        if let Some(tree) = &self.last_tree {
            Self::collect_foldable(tree.root_node(), &mut ranges);
        }
        ranges
    }

    /// Recursively collect foldable nodes.
    fn collect_foldable(
        node: tree_sitter::Node<'_>,
        ranges: &mut std::collections::HashMap<usize, usize>,
    ) {
        let start = node.start_position().row;
        let end = node.end_position().row;

        // Only fold nodes that span at least 2 lines.
        if end > start + 1 && node.is_named() {
            let kind = node.kind();
            // Fold common block-like constructs across languages.
            let foldable = matches!(
                kind,
                "function_item"
                    | "function_definition"
                    | "function_declaration"
                    | "method_definition"
                    | "method_declaration"
                    | "impl_item"
                    | "struct_item"
                    | "struct_declaration"
                    | "enum_item"
                    | "enum_declaration"
                    | "class_declaration"
                    | "class_definition"
                    | "class_body"
                    | "interface_declaration"
                    | "module"
                    | "block"
                    | "statement_block"
                    | "if_expression"
                    | "if_statement"
                    | "for_expression"
                    | "for_statement"
                    | "while_expression"
                    | "while_statement"
                    | "match_expression"
                    | "switch_statement"
                    | "try_statement"
                    | "array"
                    | "object"
                    | "hash"
                    | "dictionary"
                    | "trait_item"
                    | "mod_item"
            );
            if foldable {
                ranges.entry(start).or_insert(end);
            }
        }

        // Recurse into children.
        let child_count = node.child_count();
        for i in 0..child_count {
            if let Some(child) = node.child(i) {
                Self::collect_foldable(child, ranges);
            }
        }
    }

    /// Find the enclosing scope node for a given line (for sticky scroll).
    ///
    /// Returns a vec of (start_line, first_line_text) for ancestor nodes
    /// that are scope-like (functions, classes, impls) and start above the given line.
    pub fn enclosing_scopes(&self, line: usize, source: &str) -> Vec<(usize, String)> {
        let tree = match &self.last_tree {
            Some(t) => t,
            None => return Vec::new(),
        };

        let mut scopes = Vec::new();
        let node = tree.root_node();

        // Walk down the tree to find the deepest named descendant at this line.
        let byte_offset = source
            .lines()
            .take(line)
            .map(|l| l.len() + 1) // +1 for newline
            .sum::<usize>();

        if let Some(leaf) = node.named_descendant_for_byte_range(byte_offset, byte_offset) {
            let mut current = Some(leaf);
            while let Some(n) = current {
                let kind = n.kind();
                let start_line = n.start_position().row;
                let end_line = n.end_position().row;

                if start_line < line
                    && end_line >= line
                    && end_line > start_line + 1
                    && n.is_named()
                {
                    let is_scope = matches!(
                        kind,
                        "function_item"
                            | "function_definition"
                            | "function_declaration"
                            | "method_definition"
                            | "impl_item"
                            | "struct_item"
                            | "class_declaration"
                            | "class_definition"
                            | "trait_item"
                            | "mod_item"
                            | "module"
                    );
                    if is_scope {
                        if let Some(first_line) = source.lines().nth(start_line) {
                            scopes.push((start_line, first_line.to_string()));
                        }
                    }
                }
                current = n.parent();
            }
        }

        scopes.reverse(); // Outermost first.
        scopes
    }

    /// Get the detected language.
    pub fn language(&self) -> Language {
        self.language
    }
}
