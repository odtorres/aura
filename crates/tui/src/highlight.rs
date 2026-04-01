//! Syntax highlighting via Tree-sitter.
//!
//! Provides incremental parsing and highlight-query-based colorisation for
//! supported languages. The highlighter is attached to the buffer and
//! re-parses incrementally on each edit.

use crate::config::Theme;
use ratatui::style::{Color, Modifier};
use tree_sitter::Parser;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

/// Recognised highlight group names, mapped to terminal colours.
///
/// This list covers the standard tree-sitter highlight groups used across
/// all supported language grammars. The order must match the index used
/// in `highlight_color()`.
const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "boolean",
    "comment",
    "conditional",
    "constant",
    "constant.builtin",
    "constant.macro",
    "constructor",
    "define",
    "escape",
    "exception",
    "field",
    "float",
    "function",
    "function.builtin",
    "function.call",
    "function.macro",
    "function.method",
    "function.method.call",
    "include",
    "keyword",
    "keyword.function",
    "keyword.operator",
    "keyword.return",
    "label",
    "method",
    "method.call",
    "namespace",
    "number",
    "operator",
    "parameter",
    "preproc",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "repeat",
    "storageclass",
    "string",
    "string.escape",
    "string.regex",
    "string.special",
    "symbol",
    "tag",
    "tag.attribute",
    "tag.delimiter",
    "text",
    "text.emphasis",
    "text.literal",
    "text.reference",
    "text.strong",
    "text.title",
    "text.underline",
    "text.uri",
    "type",
    "type.builtin",
    "type.definition",
    "type.qualifier",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

/// Map a highlight group index to a (colour, modifier) pair.
fn highlight_style(idx: usize, theme: Option<&Theme>) -> (Color, Modifier) {
    let m = Modifier::empty();
    match HIGHLIGHT_NAMES.get(idx) {
        // Comments — subtle gray + italic.
        Some(&"comment") => (
            theme
                .map(|t| t.comment)
                .unwrap_or(Color::Rgb(100, 100, 100)),
            Modifier::ITALIC,
        ),

        // Keywords — magenta/purple family.
        Some(
            &"keyword" | &"keyword.function" | &"keyword.operator" | &"keyword.return"
            | &"conditional" | &"repeat" | &"exception" | &"include" | &"define" | &"preproc"
            | &"storageclass",
        ) => (theme.map(|t| t.keyword).unwrap_or(Color::Magenta), m),

        // Strings — green family.
        Some(&"string" | &"string.special" | &"string.regex") => {
            (theme.map(|t| t.string).unwrap_or(Color::Green), m)
        }

        // Escape sequences inside strings — orange/yellow.
        Some(&"string.escape" | &"escape") => (Color::Rgb(220, 150, 50), m),

        // Numbers and constants — yellow/orange.
        Some(
            &"number" | &"float" | &"boolean" | &"constant" | &"constant.builtin"
            | &"constant.macro",
        ) => (theme.map(|t| t.number).unwrap_or(Color::Yellow), m),

        // Functions — blue family.
        Some(
            &"function"
            | &"function.builtin"
            | &"function.call"
            | &"function.macro"
            | &"function.method"
            | &"function.method.call"
            | &"method"
            | &"method.call",
        ) => (theme.map(|t| t.function).unwrap_or(Color::Blue), m),

        // Types — cyan family.
        Some(&"type" | &"type.builtin" | &"type.definition" | &"type.qualifier") => {
            (theme.map(|t| t.type_name).unwrap_or(Color::Cyan), m)
        }

        // Operators — light red.
        Some(&"operator") => (Color::LightRed, m),

        // Variables — warm white / light foreground.
        Some(&"variable") => (Color::Rgb(200, 200, 220), m),

        // Built-in variables (self, this, etc.) — light yellow.
        Some(&"variable.builtin") => (Color::LightYellow, m),

        // Parameters — soft orange.
        Some(&"variable.parameter" | &"parameter") => (Color::Rgb(220, 180, 120), m),

        // Properties / fields — light green.
        Some(&"property" | &"field") => (Color::LightGreen, m),

        // Constructors — light blue.
        Some(&"constructor") => (Color::LightBlue, m),

        // Attributes / decorators — light cyan.
        Some(&"attribute" | &"tag.attribute") => (Color::LightCyan, m),

        // Tags (HTML/XML) — red/orange.
        Some(&"tag") => (Color::Rgb(230, 100, 100), m),

        // Tag delimiters (<, >, </>) — dimmer red.
        Some(&"tag.delimiter") => (Color::Rgb(150, 80, 80), m),

        // Namespace / module — light magenta.
        Some(&"namespace") => (Color::Rgb(180, 140, 220), m),

        // Labels — yellow.
        Some(&"label" | &"symbol") => (Color::Yellow, m),

        // Punctuation — subtle gray.
        Some(
            &"punctuation"
            | &"punctuation.bracket"
            | &"punctuation.delimiter"
            | &"punctuation.special",
        ) => (Color::Rgb(150, 150, 150), m),

        // Markdown / documentation text.
        Some(&"text") => (Color::Reset, m),
        Some(&"text.literal") => (Color::Rgb(180, 220, 170), m), // Code spans — green-ish
        Some(&"text.emphasis") => (Color::Rgb(220, 200, 170), Modifier::ITALIC),
        Some(&"text.strong") => (Color::Rgb(255, 230, 180), Modifier::BOLD),
        Some(&"text.title") => (Color::Rgb(100, 180, 255), Modifier::BOLD),
        Some(&"text.uri" | &"text.underline") => (Color::Rgb(100, 150, 255), Modifier::UNDERLINED),
        Some(&"text.reference") => (Color::Cyan, m),

        _ => (Color::Reset, m),
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
    /// Elixir language.
    Elixir,
    /// HEEx (HTML+EEx) templates — Phoenix LiveView.
    HEEx,
    /// Dotenv (.env) files.
    Dotenv,
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
            "ex" | "exs" => Some(Self::Elixir),
            "heex" | "eex" | "leex" => Some(Self::HEEx),
            "env" => Some(Self::Dotenv),
            _ => None,
        }
    }

    /// Detect language from a filename (for dotfiles like `.env`).
    pub fn from_filename(name: &str) -> Option<Self> {
        match name {
            ".env" | ".env.local" | ".env.development" | ".env.production" | ".env.test"
            | ".env.staging" | ".env.example" => Some(Self::Dotenv),
            "Dockerfile" => Some(Self::Bash), // Close enough for highlighting
            "Makefile" | "makefile" => Some(Self::Bash),
            _ => None,
        }
    }
}

/// Per-character colour and modifier information for a line of text.
#[derive(Debug, Clone, Default)]
pub struct HighlightedLine {
    /// One colour per character in the line.
    pub colors: Vec<Color>,
    /// Optional per-character modifier (bold, italic, underline, etc.).
    /// Same length as `colors`; `Modifier::empty()` means no modifier.
    pub modifiers: Vec<ratatui::style::Modifier>,
}

/// Syntax highlighter that wraps tree-sitter (or regex for Markdown).
pub struct SyntaxHighlighter {
    language: Language,
    /// None for Markdown (uses regex-only highlighting).
    config: Option<HighlightConfiguration>,
    /// None for Markdown (no tree-sitter parser needed).
    parser: Option<Parser>,
    /// Most-recently parsed tree, kept for node-at-position queries.
    last_tree: Option<tree_sitter::Tree>,
}

impl SyntaxHighlighter {
    /// Create a new highlighter for the given language.
    pub fn new(language: Language) -> Option<Self> {
        // Markdown uses a pure regex-based highlighter — tree-sitter-md's block
        // grammar doesn't work with HighlightConfiguration for inline syntax.
        // Markdown and Dotenv use pure regex-based highlighting (no tree-sitter).
        if language == Language::Markdown || language == Language::Dotenv {
            return Some(Self {
                language,
                config: None,
                parser: None,
                last_tree: None,
            });
        }

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
            Language::Markdown | Language::Dotenv => unreachable!("handled above"),
            Language::Elixir => (
                tree_sitter_elixir::LANGUAGE.into(),
                tree_sitter_elixir::HIGHLIGHTS_QUERY,
            ),
            Language::HEEx => (
                tree_sitter_heex::LANGUAGE.into(),
                tree_sitter_heex::HIGHLIGHTS_QUERY,
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
            config: Some(config),
            parser: Some(parser),
            last_tree: None,
        })
    }

    /// Highlight the given source text and return per-line colour information.
    ///
    /// When a `theme` is provided the theme's syntax colours are used;
    /// otherwise built-in defaults are applied.
    pub fn highlight(&mut self, source: &str, theme: Option<&Theme>) -> Vec<HighlightedLine> {
        // Re-parse the tree so node-at-position queries stay current.
        if let Some(parser) = &mut self.parser {
            self.last_tree = parser.parse(source, None);
        }

        // For Markdown (no tree-sitter config), produce empty lines then apply regex.
        let config = match &self.config {
            Some(c) => c,
            None => {
                let mut result = self.empty_lines(source);
                match self.language {
                    Language::Markdown => highlight_markdown_inline(&mut result, source),
                    Language::Dotenv => highlight_dotenv(&mut result, source),
                    _ => {}
                }
                return result;
            }
        };

        let mut highlighter = Highlighter::new();
        let events = highlighter.highlight(config, source.as_bytes(), None, |_| None);

        let events = match events {
            Ok(e) => e,
            Err(_) => return self.empty_lines(source),
        };

        let mut result: Vec<HighlightedLine> = Vec::new();
        let mut current_line = HighlightedLine::default();
        let mut current_color = Color::Reset;
        let mut current_modifier = Modifier::empty();

        let source_bytes = source.as_bytes();
        let mut byte_offset = 0;

        for event in events {
            match event {
                Ok(HighlightEvent::Source { start, end }) => {
                    let slice = &source_bytes[start..end];
                    for &b in slice {
                        if b == b'\n' {
                            result.push(std::mem::take(&mut current_line));
                        } else if (b & 0xC0) != 0x80 {
                            current_line.colors.push(current_color);
                            current_line.modifiers.push(current_modifier);
                        }
                    }
                    byte_offset = end;
                }
                Ok(HighlightEvent::HighlightStart(highlight)) => {
                    let (color, modifier) = highlight_style(highlight.0, theme);
                    current_color = color;
                    current_modifier = modifier;
                }
                Ok(HighlightEvent::HighlightEnd) => {
                    current_color = Color::Reset;
                    current_modifier = Modifier::empty();
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
                    current_line.modifiers.push(current_modifier);
                }
            }
        }

        if !current_line.colors.is_empty() {
            result.push(current_line);
        }

        // For Markdown, enhance with regex-based inline highlighting since
        // tree-sitter-md block grammar doesn't capture inline elements.
        if self.language == Language::Markdown {
            highlight_markdown_inline(&mut result, source);
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
            .map(|line| {
                let len = line.chars().count();
                HighlightedLine {
                    colors: vec![Color::Reset; len],
                    modifiers: vec![Modifier::empty(); len],
                }
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

// ---------------------------------------------------------------------------
// Markdown inline highlighting (regex-based post-processing)
// ---------------------------------------------------------------------------

/// Enhance Markdown highlighting with inline syntax that tree-sitter-md's
/// block grammar doesn't capture (bold, italic, code spans, links, etc.).
fn highlight_markdown_inline(lines: &mut [HighlightedLine], source: &str) {
    for (line_idx, line_text) in source.lines().enumerate() {
        let hl = match lines.get_mut(line_idx) {
            Some(h) => h,
            None => continue,
        };

        // Ensure modifiers vec is the same length as colors.
        while hl.modifiers.len() < hl.colors.len() {
            hl.modifiers.push(Modifier::empty());
        }

        let chars: Vec<char> = line_text.chars().collect();
        let len = chars.len().min(hl.colors.len());

        // --- Headings: lines starting with # ---
        if line_text.starts_with('#') {
            let level = line_text.chars().take_while(|c| *c == '#').count();
            let heading_color = match level {
                1 => Color::Rgb(100, 180, 255), // Bright blue
                2 => Color::Rgb(130, 200, 255), // Lighter blue
                3 => Color::Rgb(160, 210, 230), // Even lighter
                _ => Color::Rgb(180, 200, 220), // Subtle blue
            };
            // Color the # markers as punctuation, rest as heading.
            for i in 0..len {
                if i < level {
                    hl.colors[i] = Color::Rgb(100, 130, 180);
                    hl.modifiers[i] = Modifier::BOLD;
                } else {
                    hl.colors[i] = heading_color;
                    hl.modifiers[i] = Modifier::BOLD;
                }
            }
            continue; // Done with this line.
        }

        // --- Horizontal rules: --- or *** or ___ ---
        let trimmed = line_text.trim();
        if trimmed.len() >= 3
            && (trimmed.chars().all(|c| c == '-' || c == ' ')
                || trimmed.chars().all(|c| c == '*' || c == ' ')
                || trimmed.chars().all(|c| c == '_' || c == ' '))
            && trimmed.chars().filter(|c| !c.is_whitespace()).count() >= 3
        {
            for i in 0..len {
                hl.colors[i] = Color::Rgb(80, 80, 80);
            }
            continue;
        }

        // --- List markers: -, *, +, or 1. at start ---
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            let offset = line_text.len() - trimmed.len();
            if offset < len {
                hl.colors[offset] = Color::Rgb(200, 160, 80); // Orange marker
                hl.modifiers[offset] = Modifier::BOLD;
            }
        }

        // --- Blockquote: > at start ---
        if trimmed.starts_with('>') {
            let offset = line_text.len() - trimmed.len();
            for i in 0..len {
                if i == offset {
                    hl.colors[i] = Color::Rgb(100, 160, 100); // Green >
                    hl.modifiers[i] = Modifier::BOLD;
                } else if i > offset {
                    hl.colors[i] = Color::Rgb(160, 180, 160); // Dimmed green text
                    hl.modifiers[i] = Modifier::ITALIC;
                }
            }
            continue;
        }

        // --- Inline patterns (processed left-to-right) ---
        let mut i = 0;
        while i < len {
            // Bold+Italic: ***text*** or ___text___
            if i + 2 < len
                && ((chars[i] == '*' && chars[i + 1] == '*' && chars[i + 2] == '*')
                    || (chars[i] == '_' && chars[i + 1] == '_' && chars[i + 2] == '_'))
            {
                let marker = chars[i];
                if let Some(end) = find_closing_marker(&chars, i + 3, &[marker, marker, marker]) {
                    for j in i..=end + 2 {
                        if j < len {
                            hl.colors[j] = Color::Rgb(255, 220, 150);
                            hl.modifiers[j] = Modifier::BOLD | Modifier::ITALIC;
                        }
                    }
                    // Dim the markers.
                    for j in [i, i + 1, i + 2, end, end + 1, end + 2] {
                        if j < len {
                            hl.colors[j] = Color::Rgb(100, 100, 80);
                        }
                    }
                    i = end + 3;
                    continue;
                }
            }

            // Bold: **text** or __text__
            if i + 1 < len
                && ((chars[i] == '*' && chars[i + 1] == '*')
                    || (chars[i] == '_' && chars[i + 1] == '_'))
            {
                let marker = chars[i];
                if let Some(end) = find_closing_marker(&chars, i + 2, &[marker, marker]) {
                    for j in i..=end + 1 {
                        if j < len {
                            hl.colors[j] = Color::Rgb(255, 230, 180);
                            hl.modifiers[j] = Modifier::BOLD;
                        }
                    }
                    for j in [i, i + 1, end, end + 1] {
                        if j < len {
                            hl.colors[j] = Color::Rgb(100, 100, 80);
                        }
                    }
                    i = end + 2;
                    continue;
                }
            }

            // Italic: *text* or _text_
            if (chars[i] == '*' || chars[i] == '_') && i + 1 < len && !chars[i + 1].is_whitespace()
            {
                let marker = chars[i];
                if let Some(end) = find_closing_marker(&chars, i + 1, &[marker]) {
                    if end > i + 1 {
                        for j in i..=end {
                            if j < len {
                                hl.colors[j] = Color::Rgb(220, 200, 170);
                                hl.modifiers[j] = Modifier::ITALIC;
                            }
                        }
                        for j in [i, end] {
                            if j < len {
                                hl.colors[j] = Color::Rgb(100, 100, 80);
                            }
                        }
                        i = end + 1;
                        continue;
                    }
                }
            }

            // Inline code: `text`
            if chars[i] == '`' && i + 1 < len {
                if let Some(end) = chars[i + 1..].iter().position(|c| *c == '`') {
                    let end = i + 1 + end;
                    for j in i..=end {
                        if j < len {
                            hl.colors[j] = Color::Rgb(180, 220, 170); // Greenish
                        }
                    }
                    // Dim the backticks.
                    if i < len {
                        hl.colors[i] = Color::Rgb(100, 130, 100);
                    }
                    if end < len {
                        hl.colors[end] = Color::Rgb(100, 130, 100);
                    }
                    i = end + 1;
                    continue;
                }
            }

            // Links: [text](url)
            if chars[i] == '[' {
                if let Some(bracket_end) = chars[i + 1..].iter().position(|c| *c == ']') {
                    let bracket_end = i + 1 + bracket_end;
                    if bracket_end + 1 < len && chars[bracket_end + 1] == '(' {
                        if let Some(paren_end) =
                            chars[bracket_end + 2..].iter().position(|c| *c == ')')
                        {
                            let paren_end = bracket_end + 2 + paren_end;
                            // Link text — cyan.
                            for j in i + 1..bracket_end {
                                if j < len {
                                    hl.colors[j] = Color::Cyan;
                                    hl.modifiers[j] = Modifier::UNDERLINED;
                                }
                            }
                            // URL — dim blue.
                            for j in bracket_end + 2..paren_end {
                                if j < len {
                                    hl.colors[j] = Color::Rgb(100, 150, 255);
                                    hl.modifiers[j] = Modifier::UNDERLINED;
                                }
                            }
                            // Brackets/parens — dim.
                            for j in [i, bracket_end, bracket_end + 1, paren_end] {
                                if j < len {
                                    hl.colors[j] = Color::Rgb(100, 100, 100);
                                    hl.modifiers[j] = Modifier::empty();
                                }
                            }
                            i = paren_end + 1;
                            continue;
                        }
                    }
                }
            }

            i += 1;
        }
    }
}

/// Find the position of a closing marker sequence in chars starting from `start`.
fn find_closing_marker(chars: &[char], start: usize, marker: &[char]) -> Option<usize> {
    let mlen = marker.len();
    if start + mlen > chars.len() {
        return None;
    }
    (start..=chars.len().saturating_sub(mlen))
        .find(|&i| &chars[i..i + mlen] == marker && (i == start || !chars[i - 1].is_whitespace()))
}

// ---------------------------------------------------------------------------
// Dotenv (.env) highlighting (regex-based)
// ---------------------------------------------------------------------------

/// Highlight `.env` files: comments (#), keys (KEY=), values, quoted strings.
fn highlight_dotenv(lines: &mut [HighlightedLine], source: &str) {
    for (line_idx, line_text) in source.lines().enumerate() {
        let hl = match lines.get_mut(line_idx) {
            Some(h) => h,
            None => continue,
        };
        while hl.modifiers.len() < hl.colors.len() {
            hl.modifiers.push(Modifier::empty());
        }

        let chars: Vec<char> = line_text.chars().collect();
        let len = chars.len().min(hl.colors.len());
        let trimmed = line_text.trim();

        // Empty lines — skip.
        if trimmed.is_empty() {
            continue;
        }

        // Comments: lines starting with #
        if trimmed.starts_with('#') {
            let offset = line_text.len() - trimmed.len();
            for i in offset..len {
                hl.colors[i] = Color::Rgb(100, 100, 100);
                hl.modifiers[i] = Modifier::ITALIC;
            }
            continue;
        }

        // KEY=VALUE pattern.
        if let Some(eq_pos) = chars.iter().position(|c| *c == '=') {
            // KEY part — cyan bold.
            for i in 0..eq_pos.min(len) {
                hl.colors[i] = Color::Cyan;
                hl.modifiers[i] = Modifier::BOLD;
            }

            // = sign — dim gray.
            if eq_pos < len {
                hl.colors[eq_pos] = Color::Rgb(120, 120, 120);
            }

            // VALUE part.
            let val_start = eq_pos + 1;
            if val_start < len {
                // Check for quoted values.
                let first_val_char = chars.get(val_start).copied().unwrap_or(' ');
                if first_val_char == '"' || first_val_char == '\'' {
                    // Quoted string — green.
                    for i in val_start..len {
                        hl.colors[i] = Color::Green;
                    }
                    // Dim the quote characters.
                    if val_start < len {
                        hl.colors[val_start] = Color::Rgb(80, 130, 80);
                    }
                    // Find and dim the closing quote.
                    if let Some(close) = chars[val_start + 1..]
                        .iter()
                        .position(|c| *c == first_val_char)
                    {
                        let close_idx = val_start + 1 + close;
                        if close_idx < len {
                            hl.colors[close_idx] = Color::Rgb(80, 130, 80);
                        }
                    }
                } else {
                    // Unquoted value — yellow.
                    for i in val_start..len {
                        hl.colors[i] = Color::Yellow;
                    }
                    // Inline comments after value.
                    if let Some(hash) = chars[val_start..].iter().position(|c| *c == '#') {
                        let hash_idx = val_start + hash;
                        // Only if preceded by whitespace.
                        if hash_idx > 0 && chars[hash_idx - 1].is_whitespace() {
                            for i in hash_idx..len {
                                hl.colors[i] = Color::Rgb(100, 100, 100);
                                hl.modifiers[i] = Modifier::ITALIC;
                            }
                        }
                    }
                }

                // Highlight ${VAR} and $VAR references within values.
                let mut i = val_start;
                while i < len {
                    if chars[i] == '$' {
                        if i + 1 < len && chars[i + 1] == '{' {
                            // ${VAR} — orange.
                            let end = chars[i + 2..]
                                .iter()
                                .position(|c| *c == '}')
                                .map(|p| i + 2 + p)
                                .unwrap_or(len - 1);
                            for j in i..=end.min(len - 1) {
                                hl.colors[j] = Color::Rgb(220, 160, 80);
                                hl.modifiers[j] = Modifier::BOLD;
                            }
                            i = end + 1;
                        } else {
                            // $VAR — orange.
                            let end = chars[i + 1..]
                                .iter()
                                .position(|c| !c.is_alphanumeric() && *c != '_')
                                .map(|p| i + 1 + p)
                                .unwrap_or(len);
                            for j in i..end.min(len) {
                                hl.colors[j] = Color::Rgb(220, 160, 80);
                                hl.modifiers[j] = Modifier::BOLD;
                            }
                            i = end;
                        }
                    } else {
                        i += 1;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_highlighter_creates() {
        let hl = SyntaxHighlighter::new(Language::Markdown);
        assert!(hl.is_some(), "Markdown SyntaxHighlighter should be created");
    }

    #[test]
    fn test_markdown_heading_highlighted() {
        let mut hl = SyntaxHighlighter::new(Language::Markdown).unwrap();
        let source = "# Heading\n\nHello world\n";
        let lines = hl.highlight(source, None);
        assert!(!lines.is_empty());
        // The heading line should have non-Reset colors (from tree-sitter or inline pass).
        let heading = &lines[0];
        let has_color = heading.colors.iter().any(|c| *c != Color::Reset);
        assert!(has_color, "Heading line should have syntax colors");
    }

    #[test]
    fn test_markdown_bold_gets_modifier() {
        let mut hl = SyntaxHighlighter::new(Language::Markdown).unwrap();
        let source = "Hello **bold** world\n";
        let lines = hl.highlight(source, None);
        assert!(!lines.is_empty());
        let line = &lines[0];
        let has_bold = line.modifiers.iter().any(|m| m.contains(Modifier::BOLD));
        assert!(has_bold, "Bold text should have BOLD modifier");
    }
}
