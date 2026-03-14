//! Rendering the editor UI with ratatui.

use crate::app::{App, ConversationPanel, Mode};
use crate::config::Theme;
use crate::embedded_terminal::EmbeddedTerminal;
use crate::git::LineStatus;
use crate::speculative::GhostSuggestion;
use aura_core::conversation::MessageRole;
use aura_core::AuthorId;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Draw the full editor frame.
pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Layout: editor area + optional proposal + optional terminal + status bar + command bar.
    let has_proposal = app.proposal.is_some() && app.mode == Mode::Review;
    let has_terminal = app.terminal.visible;
    let terminal_height = if has_terminal { app.terminal.height } else { 0 };

    let chunks = if has_proposal {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(50),          // Editor (original)
                Constraint::Percentage(50),          // Proposal (diff)
                Constraint::Length(terminal_height), // Terminal pane (0 when hidden)
                Constraint::Length(1),               // Status bar
                Constraint::Length(1),               // Command bar
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),                  // Editor
                Constraint::Length(0),               // No proposal pane
                Constraint::Length(terminal_height), // Terminal pane (0 when hidden)
                Constraint::Length(1),               // Status bar
                Constraint::Length(1),               // Command bar
            ])
            .split(area)
    };

    let editor_area_raw = chunks[0];
    let proposal_area = chunks[1];
    let terminal_area = chunks[2];
    let status_area = chunks[3];
    let command_area = chunks[4];

    // If the file tree is visible, split the editor area horizontally.
    let (file_tree_area, editor_area) = if app.file_tree.visible {
        let tree_width = app.file_tree.width;
        let hsplit = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(tree_width), Constraint::Min(1)])
            .split(editor_area_raw);
        (Some(hsplit[0]), hsplit[1])
    } else {
        (None, editor_area_raw)
    };

    // Draw file tree sidebar if visible.
    if let Some(tree_area) = file_tree_area {
        draw_file_tree(frame, app, tree_area);
    }

    // Adjust scroll so cursor is visible.
    app.scroll_to_cursor(editor_area.height as usize, editor_area.width as usize - 6);

    // Pre-compute git line status for visible lines.
    let git_status: std::collections::HashMap<usize, LineStatus> = {
        let visible_lines = editor_area.height as usize;
        let mut status = std::collections::HashMap::new();
        for i in 0..visible_lines {
            let line_idx = app.scroll_row + i;
            if let Some(s) = app.git_line_status(line_idx) {
                status.insert(line_idx, s);
            }
        }
        status
    };

    draw_editor(frame, app, editor_area, &git_status);

    if has_proposal {
        draw_proposal(frame, app, proposal_area);
    }

    if has_terminal {
        draw_terminal(frame, app, terminal_area);
    }

    draw_status_bar(frame, app, status_area);
    draw_command_bar(frame, app, command_area);

    // Render ghost suggestion if present.
    if let Some(suggestion) = app.current_ghost_suggestion() {
        draw_ghost_text(frame, app, editor_area, suggestion);
    }

    // Render hover popup if present.
    if let Some(hover_text) = &app.hover_info {
        draw_hover_popup(frame, app, editor_area, hover_text);
    }

    // Render conversation panel if present.
    if let Some(panel) = &app.conversation_panel {
        draw_conversation_panel(frame, editor_area, panel);
    }

    // Render file picker overlay if visible.
    if app.file_picker.visible {
        draw_file_picker(frame, app, area);
    }

    // Position the terminal cursor.
    if app.file_picker.visible {
        // No editor cursor while the file picker is open.
    } else if app.terminal_focused && has_terminal {
        // Place cursor at the input line: last row of the terminal border inner area.
        // The input prompt "$ <input>" sits on the last inner row.
        let inner_x = terminal_area.x + 1; // inside left border
        let inner_bottom = terminal_area.bottom().saturating_sub(2); // above bottom border
        let prompt_prefix = format!("$ {}", app.terminal.input);
        let cursor_x = (inner_x + prompt_prefix.len() as u16).min(terminal_area.right() - 1);
        frame.set_cursor_position((cursor_x, inner_bottom));
    } else if app.mode != Mode::Review {
        // Editor cursor (6 = gutter width).
        let cursor_x = (app.cursor.col - app.scroll_col) as u16 + editor_area.x + 6;
        let cursor_y = (app.cursor.row - app.scroll_row) as u16 + editor_area.y;
        if cursor_x < editor_area.right() && cursor_y < editor_area.bottom() {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

/// Draw the embedded terminal pane.
fn draw_terminal(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.terminal_focused;

    let title = if focused {
        " Terminal (focused) "
    } else {
        " Terminal "
    };

    let border_style = if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    // The bottom row of inner is reserved for the input line; the rest shows output.
    let output_height = inner.height.saturating_sub(1) as usize;
    let input_y = inner.y + inner.height.saturating_sub(1);

    // Render output lines with scroll offset.
    let terminal: &EmbeddedTerminal = &app.terminal;
    let visible_output: Vec<&crate::embedded_terminal::TerminalLine> = terminal
        .output
        .iter()
        .skip(terminal.scroll)
        .take(output_height)
        .collect();

    for (i, line) in visible_output.iter().enumerate() {
        let y = inner.y + i as u16;
        if y >= input_y {
            break;
        }
        let style = if line.is_command {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let display: String = line.content.chars().take(inner.width as usize).collect();
        let line_widget = Paragraph::new(Span::styled(display, style));
        frame.render_widget(line_widget, Rect::new(inner.x, y, inner.width, 1));
    }

    // Render input line.
    let input_text = format!("$ {}", terminal.input);
    let display_input: String = input_text.chars().take(inner.width as usize).collect();
    let input_style = Style::default().fg(Color::Cyan);
    let input_widget = Paragraph::new(Span::styled(display_input, input_style));
    frame.render_widget(input_widget, Rect::new(inner.x, input_y, inner.width, 1));
}

/// Draw the file tree sidebar.
fn draw_file_tree(frame: &mut Frame, app: &App, area: Rect) {
    let (title, border_color) = if app.file_tree_focused {
        (" Files [focused] ", Color::Yellow)
    } else {
        (" Files ", Color::Cyan)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || app.file_tree.entries.is_empty() {
        let empty = Paragraph::new(Span::styled(
            " (empty)",
            Style::default().fg(Color::DarkGray),
        ));
        frame.render_widget(empty, inner);
        return;
    }

    let visible_height = inner.height as usize;
    let selected = app.file_tree.selected;

    // Compute scroll offset so the selected entry is always visible.
    let scroll_offset = if selected >= visible_height {
        selected.saturating_sub(visible_height - 1)
    } else {
        0
    };

    let entries = app
        .file_tree
        .entries
        .iter()
        .skip(scroll_offset)
        .take(visible_height);
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    let dir_style = Style::default().fg(Color::Cyan);
    let file_style = Style::default().fg(Color::White);

    for (i, entry) in entries.enumerate() {
        let y = inner.y + i as u16;
        let abs_idx = scroll_offset + i;

        // Build display string: indentation + icon + name.
        let indent = "  ".repeat(entry.depth);
        let icon = if entry.is_dir {
            if entry.expanded {
                "▾ "
            } else {
                "▸ "
            }
        } else {
            "  "
        };
        let display = format!("{}{}{}", indent, icon, entry.name);
        let display: String = display.chars().take(inner.width as usize).collect();

        let style = if abs_idx == selected {
            selected_style
        } else if entry.is_dir {
            dir_style
        } else {
            file_style
        };

        let line = Paragraph::new(Span::styled(display, style));
        frame.render_widget(line, Rect::new(inner.x, y, inner.width, 1));
    }
}

/// Draw the fuzzy file picker overlay centered in the given area.
fn draw_file_picker(frame: &mut Frame, app: &App, area: Rect) {
    let width = area.width.min(80);
    let height = area.height.min(20);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Open file ")
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    if inner.height == 0 {
        return;
    }

    // First inner row: search query.
    let query_line = format!("> {}", app.file_picker.query);
    let query_display: String = query_line.chars().take(inner.width as usize).collect();
    let query_widget = Paragraph::new(Span::styled(
        query_display,
        Style::default().fg(Color::Yellow),
    ));
    frame.render_widget(query_widget, Rect::new(inner.x, inner.y, inner.width, 1));

    // Remaining rows: filtered results list.
    let list_area_y = inner.y + 1;
    let list_height = inner.height.saturating_sub(1) as usize;
    let selected = app.file_picker.selected;

    // Scroll the list so the selected item is always visible.
    let scroll_start = if selected >= list_height {
        selected - list_height + 1
    } else {
        0
    };

    for (i, entry) in app
        .file_picker
        .filtered
        .iter()
        .enumerate()
        .skip(scroll_start)
        .take(list_height)
    {
        let row_y = list_area_y + (i - scroll_start) as u16;
        let is_selected = i == selected;
        let style = if is_selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let display: String = entry.chars().take(inner.width as usize).collect();
        let line_widget = Paragraph::new(Span::styled(display, style));
        frame.render_widget(line_widget, Rect::new(inner.x, row_y, inner.width, 1));
    }
}

/// Map an AuthorId to a terminal color using the theme.
fn author_color(author: &AuthorId, theme: &Theme) -> Color {
    match author {
        AuthorId::Human => theme.author_human,
        AuthorId::Ai(_) => theme.author_ai,
    }
}

/// Draw the main editor area with line numbers and authorship gutter.
fn draw_editor(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    git_status: &std::collections::HashMap<usize, LineStatus>,
) {
    // Gutter: 1 (author marker) + 4 (line number) + 1 (space) = 6
    let gutter_width = 6u16;
    let text_width = area.width.saturating_sub(gutter_width);

    let visible_lines = area.height as usize;
    let mut lines: Vec<Line> = Vec::with_capacity(visible_lines);

    let selection = app.visual_selection_range();
    let theme = &app.theme;
    let sel_style = Style::default()
        .bg(theme.selection_bg)
        .fg(theme.selection_fg);
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

            // Gutter marker: diagnostic > conversation > git > authorship.
            let marker_span = if let Some(diag) = app.line_diagnostics(line_idx) {
                if diag.is_error() {
                    Span::styled(
                        "E",
                        Style::default()
                            .fg(theme.error)
                            .add_modifier(Modifier::BOLD),
                    )
                } else if diag.is_warning() {
                    Span::styled(
                        "W",
                        Style::default()
                            .fg(theme.warning)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled("I", Style::default().fg(theme.info))
                }
            } else if app.show_conversations && app.line_has_conversation(line_idx) {
                Span::styled("C", Style::default().fg(Color::Magenta))
            } else if let Some(gs) = git_status.get(&line_idx) {
                match gs {
                    LineStatus::Added => Span::styled("▎", Style::default().fg(theme.git_added)),
                    LineStatus::Modified => {
                        Span::styled("▎", Style::default().fg(theme.git_modified))
                    }
                    LineStatus::Deleted => {
                        Span::styled("▁", Style::default().fg(theme.git_deleted))
                    }
                }
            } else if show_authorship {
                if let Some(author) = app.buffer.line_author(line_idx) {
                    Span::styled("▎", Style::default().fg(author_color(author, theme)))
                } else {
                    Span::raw(" ")
                }
            } else {
                Span::raw(" ")
            };

            let mut spans = vec![
                marker_span,
                Span::styled(line_num, Style::default().fg(theme.gutter_fg)),
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
                Style::default().fg(theme.gutter_fg),
            )]));
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

/// Draw the AI proposal pane with diff highlighting.
fn draw_proposal(frame: &mut Frame, app: &App, area: Rect) {
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
    let theme = &app.theme;
    let mode_style = match app.mode {
        Mode::Normal => Style::default().fg(Color::Black).bg(theme.mode_normal),
        Mode::Insert => Style::default().fg(Color::Black).bg(theme.mode_insert),
        Mode::Command => Style::default().fg(Color::Black).bg(theme.mode_command),
        Mode::Visual | Mode::VisualLine => Style::default().fg(Color::Black).bg(theme.mode_visual),
        Mode::Intent => Style::default().fg(Color::Black).bg(theme.mode_intent),
        Mode::Review => Style::default().fg(Color::Black).bg(theme.mode_review),
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

    let git_indicator = app
        .git_branch()
        .map(|b| format!(" │ {b}"))
        .unwrap_or_default();

    let mcp_indicator = if let Some(port) = app.mcp_port() {
        let agent_count = app.agent_registry.count();
        if agent_count > 0 {
            format!(" │ MCP:{port} ({agent_count} agents)")
        } else {
            format!(" │ MCP:{port}")
        }
    } else {
        String::new()
    };

    let experiment_indicator = if app.experimental_mode {
        " │ [EXPERIMENT]"
    } else {
        ""
    };

    let left = format!(
        " {} │ {}{}{}{}{}{}{}{}",
        app.mode.label(),
        file_name,
        modified,
        git_indicator,
        last_change,
        diag_str,
        lsp_indicator,
        mcp_indicator,
        experiment_indicator
    );
    let right = format!(" {}:{} ", app.cursor.row + 1, app.cursor.col + 1);

    let padding = area
        .width
        .saturating_sub(left.len() as u16 + right.len() as u16);

    let status = Line::from(vec![
        Span::styled(&left, mode_style),
        Span::styled(
            " ".repeat(padding as usize),
            Style::default().bg(theme.status_bg),
        ),
        Span::styled(
            &right,
            Style::default().fg(theme.status_fg).bg(theme.status_bg),
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
                    // Show impact analysis alongside review controls.
                    let start_line = app
                        .buffer
                        .char_idx_to_cursor(proposal.start.min(app.buffer.len_chars()))
                        .row;
                    let end_line = app
                        .buffer
                        .char_idx_to_cursor(proposal.end.min(app.buffer.len_chars()))
                        .row;
                    let impact = app.impact_summary(start_line, end_line).unwrap_or_default();
                    if impact.is_empty() {
                        "a: accept | r: reject | Esc: cancel".to_string()
                    } else {
                        format!("a: accept | r: reject │ {impact}")
                    }
                }
            } else {
                String::new()
            }
        }
        _ => {
            // Show ghost suggestion status if available, otherwise status message.
            app.ghost_suggestion_status()
                .or_else(|| app.status_message.clone())
                .unwrap_or_default()
        }
    };

    let style = match app.mode {
        Mode::Intent => Style::default().fg(app.theme.mode_intent),
        Mode::Review => Style::default().fg(app.theme.mode_review),
        _ => {
            if app.current_ghost_suggestion().is_some() {
                Style::default().fg(app.theme.ghost)
            } else {
                Style::default().fg(app.theme.fg)
            }
        }
    };

    let paragraph = Paragraph::new(content).style(style);
    frame.render_widget(paragraph, area);
}

/// Draw a hover information popup near the cursor.
fn draw_hover_popup(frame: &mut Frame, app: &App, editor_area: Rect, text: &str) {
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

/// Draw the conversation history panel as a right-side split.
fn draw_conversation_panel(frame: &mut Frame, editor_area: Rect, panel: &ConversationPanel) {
    // Take right 40% of the editor area.
    let width = editor_area.width * 2 / 5;
    let panel_area = Rect::new(
        editor_area.right().saturating_sub(width),
        editor_area.y,
        width,
        editor_area.height,
    );

    frame.render_widget(Clear, panel_area);

    let title = format!(" Conversation — {} ", panel.file_info);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Magenta));

    let inner = block.inner(panel_area);
    frame.render_widget(block, panel_area);

    // Render messages.
    let mut lines: Vec<Line> = Vec::new();
    for msg in &panel.messages {
        let (role_style, role_str) = match msg.role {
            MessageRole::HumanIntent => (Style::default().fg(Color::Green), "You"),
            MessageRole::AiResponse => (Style::default().fg(Color::Blue), "AI"),
            MessageRole::System => (Style::default().fg(Color::DarkGray), "Sys"),
        };

        lines.push(Line::from(vec![
            Span::styled(format!("[{role_str}] "), role_style),
            Span::styled(
                &msg.created_at[..16.min(msg.created_at.len())],
                Style::default().fg(Color::DarkGray),
            ),
        ]));

        // Wrap message content to fit panel width.
        let content_width = inner.width.saturating_sub(2) as usize;
        for line in msg.content.lines() {
            for chunk in line.as_bytes().chunks(content_width.max(1)) {
                if let Ok(s) = std::str::from_utf8(chunk) {
                    lines.push(Line::from(format!("  {s}")));
                }
            }
        }
        lines.push(Line::from("")); // Spacer between messages.
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No messages",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Apply scroll offset.
    let visible: Vec<Line> = lines
        .into_iter()
        .skip(panel.scroll)
        .take(inner.height as usize)
        .collect();

    let paragraph = Paragraph::new(visible);
    frame.render_widget(paragraph, inner);
}

/// Draw ghost text overlay for a speculative suggestion.
fn draw_ghost_text(frame: &mut Frame, app: &App, editor_area: Rect, suggestion: &GhostSuggestion) {
    let ghost_style = Style::default()
        .fg(app.theme.ghost)
        .add_modifier(Modifier::ITALIC);

    // Show ghost text inline after the cursor position.
    let cursor_screen_y = app.cursor.row.saturating_sub(app.scroll_row) as u16 + editor_area.y;
    let cursor_screen_x = app.cursor.col.saturating_sub(app.scroll_col) as u16 + editor_area.x + 6; // gutter width

    // Only render if cursor is visible.
    if cursor_screen_y >= editor_area.bottom() || cursor_screen_x >= editor_area.right() {
        return;
    }

    // Show the first line of the suggestion as inline ghost text.
    let first_line = suggestion.text.lines().next().unwrap_or("");
    let available_width = editor_area.right().saturating_sub(cursor_screen_x) as usize;
    let display_text: String = first_line.chars().take(available_width).collect();

    if !display_text.is_empty() {
        let ghost_area = Rect::new(
            cursor_screen_x,
            cursor_screen_y,
            display_text.len() as u16,
            1,
        );
        let ghost_line = Paragraph::new(Span::styled(display_text, ghost_style));
        frame.render_widget(ghost_line, ghost_area);
    }

    // If suggestion has multiple lines, show a hint below.
    let line_count = suggestion.text.lines().count();
    if line_count > 1 && cursor_screen_y + 1 < editor_area.bottom() {
        let hint = format!(
            "  ... +{} more lines ({})",
            line_count - 1,
            suggestion.category.label()
        );
        let hint_width = hint
            .len()
            .min((editor_area.right() - editor_area.x - 6) as usize);
        let hint_area = Rect::new(editor_area.x + 6, cursor_screen_y + 1, hint_width as u16, 1);
        let hint_line = Paragraph::new(Span::styled(
            &hint[..hint_width],
            Style::default().fg(app.theme.ghost),
        ));
        frame.render_widget(hint_line, hint_area);
    }
}
