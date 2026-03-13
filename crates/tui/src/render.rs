//! Rendering the editor UI with ratatui.

use crate::app::{App, Mode};
use aura_core::AuthorId;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

/// Draw the full editor frame.
pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Layout: editor area + optional proposal + status bar + command bar.
    let has_proposal = app.proposal.is_some() && app.mode == Mode::Review;

    let chunks = if has_proposal {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(50), // Editor (original)
                Constraint::Percentage(50), // Proposal (diff)
                Constraint::Length(1),      // Status bar
                Constraint::Length(1),      // Command bar
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),    // Editor
                Constraint::Length(0), // No proposal pane
                Constraint::Length(1), // Status bar
                Constraint::Length(1), // Command bar
            ])
            .split(area)
    };

    let editor_area = chunks[0];
    let proposal_area = chunks[1];
    let status_area = chunks[2];
    let command_area = chunks[3];

    // Adjust scroll so cursor is visible.
    app.scroll_to_cursor(editor_area.height as usize, editor_area.width as usize - 6);

    draw_editor(frame, app, editor_area);

    if has_proposal {
        draw_proposal(frame, app, proposal_area);
    }

    draw_status_bar(frame, app, status_area);
    draw_command_bar(frame, app, command_area);

    // Render hover popup if present.
    if let Some(hover_text) = &app.hover_info {
        draw_hover_popup(frame, app, editor_area, hover_text);
    }

    // Position the terminal cursor (6 = gutter width).
    if app.mode != Mode::Review {
        let cursor_x = (app.cursor.col - app.scroll_col) as u16 + editor_area.x + 6;
        let cursor_y = (app.cursor.row - app.scroll_row) as u16 + editor_area.y;
        if cursor_x < editor_area.right() && cursor_y < editor_area.bottom() {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

/// Map an AuthorId to a terminal color.
fn author_color(author: &AuthorId) -> Color {
    match author {
        AuthorId::Human => Color::Green,
        AuthorId::Ai(_) => Color::Blue,
    }
}

/// Draw the main editor area with line numbers and authorship gutter.
fn draw_editor(frame: &mut Frame, app: &App, area: Rect) {
    // Gutter: 1 (author marker) + 4 (line number) + 1 (space) = 6
    let gutter_width = 6u16;
    let text_width = area.width.saturating_sub(gutter_width);

    let visible_lines = area.height as usize;
    let mut lines: Vec<Line> = Vec::with_capacity(visible_lines);

    let selection = app.visual_selection_range();
    let sel_style = Style::default().bg(Color::DarkGray).fg(Color::White);
    let show_authorship = app.show_authorship;

    for i in 0..visible_lines {
        let line_idx = app.scroll_row + i;
        if let Some(rope_line) = app.buffer.line(line_idx) {
            let line_num = format!("{:>4} ", line_idx + 1);
            let content: String = rope_line
                .chars()
                .skip(app.scroll_col)
                .take(text_width as usize)
                .filter(|c| *c != '\n' && *c != '\r')
                .collect();

            // Gutter marker: diagnostic severity takes priority over authorship.
            let marker_span = if let Some(diag) = app.line_diagnostics(line_idx) {
                if diag.is_error() {
                    Span::styled(
                        "E",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    )
                } else if diag.is_warning() {
                    Span::styled(
                        "W",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled("I", Style::default().fg(Color::Cyan))
                }
            } else if show_authorship {
                if let Some(author) = app.buffer.line_author(line_idx) {
                    Span::styled("▎", Style::default().fg(author_color(author)))
                } else {
                    Span::raw(" ")
                }
            } else {
                Span::raw(" ")
            };

            let mut spans = vec![
                marker_span,
                Span::styled(line_num, Style::default().fg(Color::DarkGray)),
            ];

            // Build per-character styles combining syntax highlighting and selection.
            let line_start_idx = app
                .buffer
                .cursor_to_char_idx(&aura_core::Cursor::new(line_idx, 0));
            let visible_start = app.scroll_col;
            let visible_chars: Vec<char> = content.chars().collect();
            let hl_line = app.highlight_lines.get(line_idx);

            let mut current_span = String::new();
            let mut current_style: Option<Style> = None;

            for (col, ch) in visible_chars.iter().enumerate() {
                let char_abs = line_start_idx + visible_start + col;
                let in_selection = selection
                    .map(|(s, e)| char_abs >= s && char_abs < e)
                    .unwrap_or(false);

                let style = if in_selection {
                    sel_style
                } else if let Some(hl) = hl_line {
                    let char_idx = visible_start + col;
                    if let Some(&color) = hl.colors.get(char_idx) {
                        if color == Color::Reset {
                            Style::default()
                        } else {
                            Style::default().fg(color)
                        }
                    } else {
                        Style::default()
                    }
                } else {
                    Style::default()
                };

                if current_style != Some(style) {
                    if !current_span.is_empty() {
                        spans.push(Span::styled(
                            std::mem::take(&mut current_span),
                            current_style.unwrap_or_default(),
                        ));
                    }
                    current_style = Some(style);
                }
                current_span.push(*ch);
            }
            if !current_span.is_empty() {
                spans.push(Span::styled(
                    current_span,
                    current_style.unwrap_or_default(),
                ));
            }

            lines.push(Line::from(spans));
        } else {
            lines.push(Line::from(vec![Span::styled(
                "    ~ ",
                Style::default().fg(Color::DarkGray),
            )]));
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

/// Draw the AI proposal pane with diff highlighting.
fn draw_proposal(frame: &mut Frame, app: &App, area: Rect) {
    use ratatui::widgets::{Block, Borders};

    let proposal = match &app.proposal {
        Some(p) => p,
        None => return,
    };

    let title = if proposal.streaming {
        " AI Proposal (streaming...) "
    } else {
        " AI Proposal — a: accept | r: reject "
    };

    let block = Block::default()
        .borders(Borders::TOP)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Show the proposed text with green highlighting.
    let proposed_style = Style::default().fg(Color::Green);
    let proposed_lines: Vec<Line> = proposal
        .proposed_text
        .lines()
        .take(inner.height as usize)
        .map(|l| Line::from(Span::styled(l.to_string(), proposed_style)))
        .collect();

    let paragraph = Paragraph::new(proposed_lines);
    frame.render_widget(paragraph, inner);
}

/// Draw the status bar showing mode, file info, and cursor position.
fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let mode_style = match app.mode {
        Mode::Normal => Style::default().fg(Color::Black).bg(Color::Blue),
        Mode::Insert => Style::default().fg(Color::Black).bg(Color::Green),
        Mode::Command => Style::default().fg(Color::Black).bg(Color::Yellow),
        Mode::Visual | Mode::VisualLine => Style::default().fg(Color::Black).bg(Color::Magenta),
        Mode::Intent => Style::default().fg(Color::Black).bg(Color::Cyan),
        Mode::Review => Style::default().fg(Color::Black).bg(Color::LightRed),
    };

    let file_name = app
        .buffer
        .file_path()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("[scratch]");

    let modified = if app.buffer.is_modified() { " [+]" } else { "" };

    // Build "last change by" indicator.
    let last_change = app
        .buffer
        .last_edit()
        .map(|(author, when)| {
            let ago = when.elapsed().as_secs();
            let time_str = if ago < 60 {
                format!("{ago}s ago")
            } else if ago < 3600 {
                format!("{}m ago", ago / 60)
            } else {
                format!("{}h ago", ago / 3600)
            };
            format!(" │ last: {} {}", author, time_str)
        })
        .unwrap_or_default();

    // Diagnostic counts.
    let (errors, warnings) = app.diagnostic_counts();
    let diag_str = if errors > 0 || warnings > 0 {
        format!(" │ E:{errors} W:{warnings}")
    } else {
        String::new()
    };

    let lsp_indicator = if app.has_lsp() { " │ LSP" } else { "" };

    let left = format!(
        " {} │ {}{}{}{}{}",
        app.mode.label(),
        file_name,
        modified,
        last_change,
        diag_str,
        lsp_indicator
    );
    let right = format!(" {}:{} ", app.cursor.row + 1, app.cursor.col + 1);

    let padding = area
        .width
        .saturating_sub(left.len() as u16 + right.len() as u16);

    let status = Line::from(vec![
        Span::styled(&left, mode_style),
        Span::styled(
            " ".repeat(padding as usize),
            Style::default().bg(Color::DarkGray),
        ),
        Span::styled(
            &right,
            Style::default().fg(Color::White).bg(Color::DarkGray),
        ),
    ]);

    let paragraph = Paragraph::new(status);
    frame.render_widget(paragraph, area);
}

/// Draw the command bar at the bottom.
fn draw_command_bar(frame: &mut Frame, app: &App, area: Rect) {
    let content = match app.mode {
        Mode::Command => format!(":{}", app.command_input),
        Mode::Intent => format!("intent> {}", app.intent_input),
        Mode::Review => {
            if let Some(proposal) = &app.proposal {
                if proposal.streaming {
                    format!(
                        "AI streaming... ({} chars) — Esc to cancel",
                        proposal.proposed_text.len()
                    )
                } else {
                    "a: accept | r: reject | Esc: cancel".to_string()
                }
            } else {
                String::new()
            }
        }
        _ => app.status_message.clone().unwrap_or_default(),
    };

    let style = match app.mode {
        Mode::Intent => Style::default().fg(Color::Cyan),
        Mode::Review => Style::default().fg(Color::LightRed),
        _ => Style::default().fg(Color::White),
    };

    let paragraph = Paragraph::new(content).style(style);
    frame.render_widget(paragraph, area);
}

/// Draw a hover information popup near the cursor.
fn draw_hover_popup(frame: &mut Frame, app: &App, editor_area: Rect, text: &str) {
    use ratatui::widgets::{Block, Borders, Clear};

    let lines: Vec<&str> = text.lines().take(10).collect();
    let height = (lines.len() as u16 + 2).min(editor_area.height / 2);
    let max_width = lines
        .iter()
        .map(|l| l.len() as u16)
        .max()
        .unwrap_or(20)
        .clamp(20, editor_area.width.saturating_sub(8));
    let width = max_width + 4; // border + padding

    // Position below and to the right of the cursor.
    let cursor_x = (app.cursor.col - app.scroll_col) as u16 + editor_area.x + 6;
    let cursor_y = (app.cursor.row - app.scroll_row) as u16 + editor_area.y + 1;

    let x = cursor_x.min(editor_area.right().saturating_sub(width));
    let y = if cursor_y + height < editor_area.bottom() {
        cursor_y
    } else {
        cursor_y.saturating_sub(height + 1)
    };

    let popup_area = Rect::new(x, y, width, height);

    // Clear background and draw.
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Hover ")
        .border_style(Style::default().fg(Color::Cyan));

    let hover_lines: Vec<Line> = lines.iter().map(|l| Line::from(l.to_string())).collect();

    let paragraph = Paragraph::new(hover_lines)
        .block(block)
        .style(Style::default().fg(Color::White).bg(Color::Black));
    frame.render_widget(paragraph, popup_area);
}
