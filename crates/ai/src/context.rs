//! Context assembly for AI requests.
//!
//! Gathers editor state (buffer content, cursor position, selection,
//! file metadata, recent edit history) and formats it as context for
//! the AI model. Uses a truncation strategy that prioritises code near
//! the cursor.

use aura_core::{Buffer, Cursor};

/// Maximum characters of buffer content to include in context.
const MAX_CONTEXT_CHARS: usize = 30_000;

/// Number of recent edits to include in context.
const MAX_RECENT_EDITS: usize = 10;

/// Assembled editor context ready to be sent to the AI.
#[derive(Debug, Clone)]
pub struct EditorContext {
    /// The file path, if any.
    pub file_path: Option<String>,
    /// Detected language from file extension.
    pub language: Option<String>,
    /// The buffer content (possibly truncated).
    pub content: String,
    /// Current cursor position (1-indexed for display).
    pub cursor_line: usize,
    pub cursor_col: usize,
    /// The line at the cursor.
    pub current_line: String,
    /// Selected text, if any.
    pub selection: Option<String>,
    /// Recent edit summary.
    pub recent_edits: Vec<String>,
    /// Total line count.
    pub total_lines: usize,
}

impl EditorContext {
    /// Build context from the current editor state.
    pub fn from_buffer(
        buffer: &Buffer,
        cursor: &Cursor,
        selection: Option<(usize, usize)>,
    ) -> Self {
        let file_path = buffer.file_path().map(|p| p.display().to_string());
        let language = buffer
            .file_path()
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .map(|ext| detect_language(ext).to_string());

        let total_lines = buffer.line_count();
        let content = build_truncated_content(buffer, cursor);

        let current_line = buffer
            .line(cursor.row)
            .map(|l| l.to_string().trim_end().to_string())
            .unwrap_or_default();

        let selection_text = selection.map(|(start, end)| {
            let end = end.min(buffer.len_chars());
            buffer.rope().slice(start..end).to_string()
        });

        let recent_edits = buffer
            .history()
            .iter()
            .rev()
            .take(MAX_RECENT_EDITS)
            .map(|edit| {
                let author = &edit.author;
                match &edit.kind {
                    aura_core::buffer::EditKind::Insert { pos, text } => {
                        let preview: String = text.chars().take(40).collect();
                        format!("{author} inserted at {pos}: \"{preview}\"")
                    }
                    aura_core::buffer::EditKind::Delete {
                        start,
                        end,
                        deleted,
                    } => {
                        let preview: String = deleted.chars().take(40).collect();
                        format!("{author} deleted {start}..{end}: \"{preview}\"")
                    }
                }
            })
            .collect();

        Self {
            file_path,
            language,
            content,
            cursor_line: cursor.row + 1,
            cursor_col: cursor.col + 1,
            current_line,
            selection: selection_text,
            recent_edits,
            total_lines,
        }
    }

    /// Format the context as a system prompt for the AI.
    pub fn to_system_prompt(&self) -> String {
        let mut prompt = String::from(
            "You are an AI code editor assistant integrated into the AURA editor. \
             You help the user edit code by proposing changes.\n\n\
             IMPORTANT: When proposing code changes, output ONLY the replacement code. \
             Do not include explanations, markdown formatting, or code fences unless asked. \
             The output will be directly applied to the buffer.\n\n",
        );

        if let Some(path) = &self.file_path {
            prompt.push_str(&format!("File: {path}\n"));
        }
        if let Some(lang) = &self.language {
            prompt.push_str(&format!("Language: {lang}\n"));
        }
        prompt.push_str(&format!(
            "Cursor: line {}, column {}\n",
            self.cursor_line, self.cursor_col
        ));
        prompt.push_str(&format!("Total lines: {}\n", self.total_lines));

        if !self.recent_edits.is_empty() {
            prompt.push_str("\nRecent edits:\n");
            for edit in &self.recent_edits {
                prompt.push_str(&format!("  - {edit}\n"));
            }
        }

        prompt.push_str("\n--- FILE CONTENT ---\n");
        prompt.push_str(&self.content);
        prompt.push_str("\n--- END ---\n");

        if let Some(sel) = &self.selection {
            prompt.push_str("\n--- SELECTED TEXT ---\n");
            prompt.push_str(sel);
            prompt.push_str("\n--- END SELECTION ---\n");
        }

        prompt
    }
}

/// Build buffer content with truncation, prioritising text near the cursor.
fn build_truncated_content(buffer: &Buffer, cursor: &Cursor) -> String {
    let full_text = buffer.text();
    if full_text.len() <= MAX_CONTEXT_CHARS {
        return full_text;
    }

    // Include a window around the cursor line.
    let lines: Vec<&str> = full_text.lines().collect();
    let total = lines.len();
    let cursor_line = cursor.row.min(total.saturating_sub(1));

    // Try to include ~200 lines around cursor, plus file header.
    let window = 100;
    let start = cursor_line.saturating_sub(window);
    let end = (cursor_line + window).min(total);

    let mut result = String::new();

    // Always include the first 10 lines (imports, module header).
    let header_end = 10.min(start);
    for line in lines.iter().take(header_end) {
        result.push_str(line);
        result.push('\n');
    }
    if header_end < start {
        result.push_str(&format!(
            "\n... ({} lines omitted) ...\n\n",
            start - header_end
        ));
    }

    // Cursor window.
    for line in lines.iter().take(end).skip(start) {
        result.push_str(line);
        result.push('\n');
    }

    if end < total {
        result.push_str(&format!("\n... ({} lines omitted) ...\n", total - end));
    }

    // Final safety truncation.
    if result.len() > MAX_CONTEXT_CHARS {
        result.truncate(MAX_CONTEXT_CHARS);
        result.push_str("\n... (truncated) ...\n");
    }

    result
}

/// Detect language from file extension.
fn detect_language(ext: &str) -> &str {
    match ext {
        "rs" => "Rust",
        "py" => "Python",
        "js" => "JavaScript",
        "ts" => "TypeScript",
        "tsx" => "TypeScript (React)",
        "jsx" => "JavaScript (React)",
        "go" => "Go",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" => "C++",
        "java" => "Java",
        "rb" => "Ruby",
        "sh" | "bash" | "zsh" => "Shell",
        "toml" => "TOML",
        "yaml" | "yml" => "YAML",
        "json" => "JSON",
        "md" => "Markdown",
        "html" => "HTML",
        "css" => "CSS",
        "sql" => "SQL",
        _ => ext,
    }
}
