//! Markdown live preview rendering.
//!
//! Parses markdown source and renders formatted output using ratatui
//! styles (bold, italic, colors, indentation). Shown in a split pane
//! alongside the editor when `:preview` is active.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Render markdown source into styled ratatui Lines for display.
pub fn render_markdown(source: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut in_code_block = false;
    let mut code_block_lines: Vec<String> = Vec::new();

    for raw_line in source.lines() {
        // Code blocks (fenced with ```)
        if raw_line.trim_start().starts_with("```") {
            if in_code_block {
                // End of code block — render collected lines.
                for cl in &code_block_lines {
                    let display: String = format!("  {}", cl).chars().take(width).collect();
                    lines.push(Line::from(Span::styled(
                        display,
                        Style::default()
                            .fg(Color::Rgb(180, 180, 180))
                            .bg(Color::Rgb(40, 44, 52)),
                    )));
                }
                code_block_lines.clear();
                in_code_block = false;
            } else {
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            code_block_lines.push(raw_line.to_string());
            continue;
        }

        let trimmed = raw_line.trim();

        // Empty lines.
        if trimmed.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        // Headers.
        if let Some(rest) = trimmed.strip_prefix("######") {
            lines.push(render_header(rest.trim(), 6, width));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("#####") {
            lines.push(render_header(rest.trim(), 5, width));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("####") {
            lines.push(render_header(rest.trim(), 4, width));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("###") {
            if !rest.starts_with('#') {
                lines.push(render_header(rest.trim(), 3, width));
                continue;
            }
        }
        if let Some(rest) = trimmed.strip_prefix("##") {
            if !rest.starts_with('#') {
                lines.push(render_header(rest.trim(), 2, width));
                continue;
            }
        }
        if let Some(rest) = trimmed.strip_prefix('#') {
            if !rest.starts_with('#') {
                lines.push(render_header(rest.trim(), 1, width));
                continue;
            }
        }

        // Horizontal rule.
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            let rule: String = "─".repeat(width.min(60));
            lines.push(Line::from(Span::styled(
                rule,
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }

        // Blockquotes.
        if let Some(rest) = trimmed.strip_prefix('>') {
            let text = rest.trim();
            let display: String = format!("  │ {}", text).chars().take(width).collect();
            lines.push(Line::from(Span::styled(
                display,
                Style::default()
                    .fg(Color::Rgb(150, 150, 180))
                    .add_modifier(Modifier::ITALIC),
            )));
            continue;
        }

        // Unordered list items.
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            let indent = raw_line.len() - raw_line.trim_start().len();
            let level = indent / 2;
            let text = &trimmed[2..];
            let prefix = "  ".repeat(level);
            let bullet = if level % 2 == 0 { "•" } else { "◦" };
            let display: String = format!("  {}{} {}", prefix, bullet, text)
                .chars()
                .take(width)
                .collect();
            lines.push(Line::from(render_inline_spans(&display)));
            continue;
        }

        // Ordered list items.
        if let Some(dot_pos) = trimmed.find(". ") {
            if dot_pos <= 3 && trimmed[..dot_pos].chars().all(|c| c.is_ascii_digit()) {
                let text = &trimmed[dot_pos + 2..];
                let num = &trimmed[..dot_pos];
                let display: String = format!("  {}. {}", num, text).chars().take(width).collect();
                lines.push(Line::from(render_inline_spans(&display)));
                continue;
            }
        }

        // Table rows.
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            // Check if it's a separator row (|---|---|).
            if trimmed
                .chars()
                .all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
            {
                let sep: String = "─".repeat(width.min(60));
                lines.push(Line::from(Span::styled(
                    sep,
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                let display: String = format!("  {}", trimmed).chars().take(width).collect();
                lines.push(Line::from(Span::styled(
                    display,
                    Style::default().fg(Color::Rgb(200, 200, 220)),
                )));
            }
            continue;
        }

        // Regular paragraph text with inline formatting.
        let display: String = format!("  {}", trimmed).chars().take(width).collect();
        lines.push(Line::from(render_inline_spans(&display)));
    }

    // Close any unclosed code block.
    if in_code_block {
        for cl in &code_block_lines {
            let display: String = format!("  {}", cl).chars().take(width).collect();
            lines.push(Line::from(Span::styled(
                display,
                Style::default()
                    .fg(Color::Rgb(180, 180, 180))
                    .bg(Color::Rgb(40, 44, 52)),
            )));
        }
    }

    lines
}

/// Render a header line with appropriate styling.
fn render_header(text: &str, level: usize, width: usize) -> Line<'static> {
    let prefix = match level {
        1 => "█ ",
        2 => "▌ ",
        3 => "▎ ",
        _ => "  ",
    };
    let color = match level {
        1 => Color::Rgb(100, 180, 255),
        2 => Color::Rgb(130, 200, 160),
        3 => Color::Rgb(220, 200, 120),
        4 => Color::Rgb(200, 160, 220),
        _ => Color::Rgb(180, 180, 180),
    };
    let display: String = format!("{}{}", prefix, text).chars().take(width).collect();
    Line::from(Span::styled(
        display,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ))
}

/// Parse inline formatting (bold, italic, code) and return styled spans.
fn render_inline_spans(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut chars = text.chars().peekable();
    let mut current = String::new();

    while let Some(c) = chars.next() {
        if c == '`' {
            // Inline code.
            if !current.is_empty() {
                spans.push(Span::raw(current.clone()));
                current.clear();
            }
            let mut code = String::new();
            for ch in chars.by_ref() {
                if ch == '`' {
                    break;
                }
                code.push(ch);
            }
            spans.push(Span::styled(
                code,
                Style::default().fg(Color::Rgb(230, 150, 100)),
            ));
        } else if c == '*' && chars.peek() == Some(&'*') {
            // Bold.
            chars.next();
            if !current.is_empty() {
                spans.push(Span::raw(current.clone()));
                current.clear();
            }
            let mut bold = String::new();
            loop {
                match chars.next() {
                    Some('*') if chars.peek() == Some(&'*') => {
                        chars.next();
                        break;
                    }
                    Some(ch) => bold.push(ch),
                    None => break,
                }
            }
            spans.push(Span::styled(
                bold,
                Style::default().add_modifier(Modifier::BOLD),
            ));
        } else if c == '*' || c == '_' {
            // Italic.
            if !current.is_empty() {
                spans.push(Span::raw(current.clone()));
                current.clear();
            }
            let mut italic = String::new();
            for ch in chars.by_ref() {
                if ch == c {
                    break;
                }
                italic.push(ch);
            }
            spans.push(Span::styled(
                italic,
                Style::default().add_modifier(Modifier::ITALIC),
            ));
        } else {
            current.push(c);
        }
    }

    if !current.is_empty() {
        spans.push(Span::raw(current));
    }

    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_header() {
        let lines = render_markdown("# Title", 80);
        assert_eq!(lines.len(), 1);
        // The rendered line should contain the title text.
        let line_str: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(line_str.contains("Title"));
    }

    #[test]
    fn test_render_code_block() {
        let source = "```rust\nfn main() {}\n```\n";
        let lines = render_markdown(source, 80);
        // The code block should produce at least one line for "fn main() {}".
        assert!(!lines.is_empty());
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref().to_string()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all_text.contains("fn main()"));
    }

    #[test]
    fn test_render_list() {
        let source = "- item1\n- item2\n";
        let lines = render_markdown(source, 80);
        assert_eq!(lines.len(), 2);
        let first: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        let second: String = lines[1].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(first.contains("item1"));
        assert!(second.contains("item2"));
    }
}
