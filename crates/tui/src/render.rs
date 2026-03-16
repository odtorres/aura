//! Rendering the editor UI with ratatui.

use crate::app::{App, ConversationPanel, Mode};
use crate::config::Theme;
use crate::diff_view::DiffLine;
use crate::git::LineStatus;
use crate::source_control::{GitFileStatus, GitPanelSection, SidebarView};
use crate::speculative::GhostSuggestion;
use aura_core::conversation::MessageRole;
use aura_core::AuthorId;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Draw the full editor frame.
pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Layout: optional tab bar + editor area + optional proposal + optional terminal + status bar + command bar.
    let has_proposal = app.proposal.is_some() && app.mode == Mode::Review;
    let has_terminal = app.terminal.visible;
    let terminal_height = if has_terminal { app.terminal.height } else { 0 };
    let has_tab_bar = app.tabs.count() > 1;
    let tab_bar_height: u16 = if has_tab_bar { 1 } else { 0 };

    let chunks = if has_proposal {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(tab_bar_height),  // Tab bar (0 or 1 row)
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
                Constraint::Length(tab_bar_height),  // Tab bar (0 or 1 row)
                Constraint::Min(1),                  // Editor
                Constraint::Length(0),               // No proposal pane
                Constraint::Length(terminal_height), // Terminal pane (0 when hidden)
                Constraint::Length(1),               // Status bar
                Constraint::Length(1),               // Command bar
            ])
            .split(area)
    };

    let tab_bar_area = chunks[0];
    let editor_area_raw = chunks[1];
    let proposal_area = chunks[2];
    let terminal_area = chunks[3];
    let status_area = chunks[4];
    let command_area = chunks[5];

    // Draw tab bar if multiple tabs are open.
    if has_tab_bar {
        draw_tab_bar(frame, app, tab_bar_area);
    }

    // If the file tree is visible, split the editor area horizontally.
    let (file_tree_area, editor_area_outer) = if app.file_tree.visible {
        let tree_width = app.file_tree.width;
        let hsplit = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(tree_width), Constraint::Min(1)])
            .split(editor_area_raw);
        (Some(hsplit[0]), hsplit[1])
    } else {
        (None, editor_area_raw)
    };

    // Draw sidebar if visible (file tree or source control).
    if let Some(tree_area) = file_tree_area {
        app.file_tree_rect = tree_area;
        match app.sidebar_view {
            SidebarView::Files => draw_file_tree(frame, app, tree_area),
            SidebarView::Git => draw_source_control(frame, app, tree_area),
        }
    } else {
        app.file_tree_rect = Rect::default();
    }

    // If the conversation history panel is visible, split off the right side.
    let (editor_area, conv_history_area) = if app.conversation_history.visible {
        let hsplit = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(app.conversation_history.width),
            ])
            .split(editor_area_outer);
        (hsplit[0], Some(hsplit[1]))
    } else {
        (editor_area_outer, None)
    };

    // Draw conversation history panel if visible.
    if let Some(area) = conv_history_area {
        app.conv_history_rect = area;
        draw_conversation_history(frame, app, area);
    } else {
        app.conv_history_rect = Rect::default();
    }

    // Save panel rects for mouse click-to-focus.
    app.editor_rect = editor_area;
    if has_terminal {
        app.terminal_rect = terminal_area;
    } else {
        app.terminal_rect = Rect::default();
    }

    // If diff view is active, render it instead of the normal editor.
    if app.diff_view.is_some() {
        draw_diff_view(frame, app, editor_area);

        if has_terminal {
            let inner_h = terminal_area.height.saturating_sub(2);
            let inner_w = terminal_area.width.saturating_sub(2);
            if inner_h > 0 && inner_w > 0 {
                app.terminal.resize(inner_w, inner_h);
            }
            draw_terminal(frame, app, terminal_area);
        }

        draw_status_bar(frame, app, status_area);
        draw_command_bar(frame, app, command_area);
    } else {
        // Draw editor border with filename as title.
        let border_color = if !app.terminal_focused && !app.file_tree_focused && !app.source_control_focused && !app.conversation_history_focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };
        let editor_title = format!(" {} ", app.tab().title());
        let editor_block = Block::default()
            .borders(Borders::ALL)
            .title(editor_title)
            .border_style(Style::default().fg(border_color));
        let editor_inner = editor_block.inner(editor_area);
        frame.render_widget(editor_block, editor_area);

        // Carve out 1 column on the right for the minimap scrollbar.
        let (editor_content_area, minimap_area) = {
            let hsplit = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(editor_inner);
            (hsplit[0], hsplit[1])
        };

        // Adjust scroll so cursor is visible (using inner dimensions).
        let gutter_width_usize = 6;
        let viewport_h = editor_content_area.height as usize;
        let viewport_w = editor_content_area.width.saturating_sub(gutter_width_usize as u16) as usize;
        app.scroll_to_cursor(viewport_h, viewport_w);

        // Pre-compute git line status for visible lines.
        let git_status: std::collections::HashMap<usize, LineStatus> = {
            let visible_lines = viewport_h;
            let mut status = std::collections::HashMap::new();
            for i in 0..visible_lines {
                let line_idx = app.tab().scroll_row + i;
                if let Some(s) = app.git_line_status(line_idx) {
                    status.insert(line_idx, s);
                }
            }
            status
        };

        draw_editor(frame, app, editor_content_area, &git_status);

        // Draw minimap with diagnostic markers.
        {
            let theme = &app.theme;
            let total_lines = app.buffer().line_count();
            let scroll_row = app.tab().scroll_row;

            // Collect diagnostic markers sorted by priority (info < warning < error).
            let mut markers: Vec<(usize, Color)> = Vec::new();
            for d in &app.tab().diagnostics {
                let color = if d.is_error() {
                    theme.error
                } else if d.is_warning() {
                    theme.warning
                } else {
                    theme.info
                };
                let priority = if d.is_error() {
                    2
                } else if d.is_warning() {
                    1
                } else {
                    0
                };
                markers.push((d.range.start.line as usize, color));
                // Store priority for sorting — we use a stable sort so same-line
                // markers with higher priority end up last (overwriting lower).
                let _ = priority; // used implicitly by insertion order below
            }
            // Sort: info first, then warnings, then errors so errors overwrite.
            markers.sort_by_key(|&(line, color)| {
                let prio = match color {
                    c if c == theme.error => 2u8,
                    c if c == theme.warning => 1,
                    _ => 0,
                };
                (prio, line)
            });

            draw_minimap(frame, minimap_area, &markers, total_lines, scroll_row, viewport_h);
        }

        if has_proposal {
            draw_proposal(frame, app, proposal_area);
        }

        if has_terminal {
            // Sync the PTY screen size with the actual rendered inner area.
            let inner_h = terminal_area.height.saturating_sub(2); // borders
            let inner_w = terminal_area.width.saturating_sub(2);
            if inner_h > 0 && inner_w > 0 {
                app.terminal.resize(inner_w, inner_h);
            }
            draw_terminal(frame, app, terminal_area);
        }

        draw_status_bar(frame, app, status_area);
        draw_command_bar(frame, app, command_area);

        // Render ghost suggestion if present.
        if let Some(suggestion) = app.current_ghost_suggestion() {
            draw_ghost_text(frame, app, editor_content_area, suggestion);
        }

        // Render hover popup if present.
        if let Some(hover_text) = app.tab().hover_info.clone() {
            draw_hover_popup(frame, app, editor_content_area, &hover_text);
        }

        // Render conversation panel if present.
        if let Some(panel) = &app.conversation_panel {
            draw_conversation_panel(frame, editor_content_area, panel);
        }

        // Render file picker overlay if visible.
        if app.file_picker.visible {
            draw_file_picker(frame, app, area);
        }

        // Render help overlay if visible.
        if app.help.visible {
            draw_help(frame, app, area);
        }

        // Position the terminal cursor.
        if app.file_picker.visible {
            // No editor cursor while the file picker is open.
        } else if app.terminal_focused && has_terminal {
            // Position the hardware cursor at the PTY cursor location.
            let inner = terminal_area.inner(ratatui::layout::Margin { horizontal: 1, vertical: 1 });
            let (_snap, t_cursor_row, t_cursor_col) = app.terminal.snapshot();
            let cx = inner.x + t_cursor_col as u16;
            let cy = inner.y + t_cursor_row as u16;
            if cx < inner.right() && cy < inner.bottom() {
                frame.set_cursor_position((cx, cy));
            }
        } else if app.mode != Mode::Review {
            // Editor cursor (6 = gutter width), positioned inside the border.
            let tab = app.tab();
            let cursor_x = (tab.cursor.col - tab.scroll_col) as u16 + editor_content_area.x + 6;
            let cursor_y = (tab.cursor.row - tab.scroll_row) as u16 + editor_content_area.y;
            if cursor_x < editor_content_area.right() && cursor_y < editor_content_area.bottom() {
                frame.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }
}

/// Map a file line number to a minimap row index.
///
/// Returns the row in `[0, minimap_height)` that corresponds to `line` in a
/// file of `total_lines` lines.  Returns `None` when `total_lines` or
/// `minimap_height` is zero.
fn map_line_to_row(line: usize, total_lines: usize, minimap_height: usize) -> Option<usize> {
    if total_lines == 0 || minimap_height == 0 {
        return None;
    }
    Some(line.min(total_lines.saturating_sub(1)) * minimap_height / total_lines)
}

/// Draw a 1-column minimap overview scrollbar.
///
/// `markers` is a list of `(file_line, color)` pairs sorted by ascending
/// priority — later entries overwrite earlier ones when they map to the same
/// minimap row.  `viewport_start` / `viewport_lines` describe the currently
/// visible region so it can be highlighted.
fn draw_minimap(
    frame: &mut Frame,
    area: Rect,
    markers: &[(usize, Color)],
    total_lines: usize,
    viewport_start: usize,
    viewport_lines: usize,
) {
    let h = area.height as usize;
    if h == 0 || total_lines == 0 {
        return;
    }

    // Build per-row background colours.
    let dark_bg = Color::Rgb(40, 40, 40);
    let viewport_bg = Color::Rgb(100, 100, 100);

    let mut row_colors: Vec<Color> = Vec::with_capacity(h);

    // Compute viewport row range.
    let vp_row_start = map_line_to_row(viewport_start, total_lines, h).unwrap_or(0);
    let vp_end_line = viewport_start.saturating_add(viewport_lines).min(total_lines);
    let vp_row_end = if vp_end_line == 0 {
        0
    } else {
        map_line_to_row(vp_end_line.saturating_sub(1), total_lines, h)
            .unwrap_or(0)
            .saturating_add(1)
    };

    for r in 0..h {
        if r >= vp_row_start && r < vp_row_end {
            row_colors.push(viewport_bg);
        } else {
            row_colors.push(dark_bg);
        }
    }

    // Apply markers (lowest priority first, so higher priorities overwrite).
    for &(line, color) in markers {
        if let Some(row) = map_line_to_row(line, total_lines, h) {
            row_colors[row] = color;
        }
    }

    // Render each row as a single space with the computed background.
    for (r, &bg) in row_colors.iter().enumerate() {
        let cell_area = Rect::new(area.x, area.y + r as u16, 1, 1);
        frame.render_widget(
            Paragraph::new(Span::styled(" ", Style::default().bg(bg))),
            cell_area,
        );
    }
}

/// Draw the side-by-side diff view.
fn draw_diff_view(frame: &mut Frame, app: &mut App, area: Rect) {
    let dv = match &app.diff_view {
        Some(dv) => dv,
        None => return,
    };

    // Split horizontally: 50/50 for diff panes + 1 column for minimap.
    let hsplit = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
            Constraint::Length(1),
        ])
        .split(area);
    let diff_minimap_area = hsplit[2];

    let left_title = format!(" HEAD: {} ", dv.file_path);
    let right_title = format!(" Working: {} ", dv.file_path);

    let left_block = Block::default()
        .borders(Borders::ALL)
        .title(left_title)
        .border_style(Style::default().fg(Color::Red));
    let right_block = Block::default()
        .borders(Borders::ALL)
        .title(right_title)
        .border_style(Style::default().fg(Color::Green));

    let left_inner = left_block.inner(hsplit[0]);
    let right_inner = right_block.inner(hsplit[1]);

    frame.render_widget(left_block, hsplit[0]);
    frame.render_widget(right_block, hsplit[1]);

    let viewport_height = left_inner.height as usize;

    // Update the diff view's scroll clamp with actual viewport height.
    if let Some(dv) = &mut app.diff_view {
        let max_scroll = dv.lines.len().saturating_sub(viewport_height);
        if dv.scroll > max_scroll {
            dv.scroll = max_scroll;
        }
    }

    let dv = match &app.diff_view {
        Some(dv) => dv,
        None => return,
    };

    let scroll = dv.scroll;
    let mut old_line_no: usize = 0;
    let mut new_line_no: usize = 0;

    // Count line numbers up to scroll offset.
    for line in dv.lines.iter().take(scroll) {
        match line {
            DiffLine::Both(_, _) => {
                old_line_no += 1;
                new_line_no += 1;
            }
            DiffLine::LeftOnly(_) => {
                old_line_no += 1;
            }
            DiffLine::RightOnly(_) => {
                new_line_no += 1;
            }
        }
    }

    let gutter_width: u16 = 5;

    for (i, diff_line) in dv.lines.iter().skip(scroll).take(viewport_height).enumerate() {
        let y = left_inner.y + i as u16;
        let left_content_x = left_inner.x + gutter_width;
        let left_content_w = left_inner.width.saturating_sub(gutter_width) as usize;
        let right_content_x = right_inner.x + gutter_width;
        let right_content_w = right_inner.width.saturating_sub(gutter_width) as usize;

        match diff_line {
            DiffLine::Both(l, _r) => {
                old_line_no += 1;
                new_line_no += 1;

                // Left gutter.
                let left_gutter = format!("{:>4} ", old_line_no);
                frame.render_widget(
                    Paragraph::new(Span::styled(left_gutter, Style::default().fg(Color::DarkGray))),
                    Rect::new(left_inner.x, y, gutter_width, 1),
                );
                // Left content.
                let left_text: String = l.chars().take(left_content_w).collect();
                frame.render_widget(
                    Paragraph::new(Span::raw(left_text)),
                    Rect::new(left_content_x, y, left_inner.width.saturating_sub(gutter_width), 1),
                );

                // Right gutter.
                let right_gutter = format!("{:>4} ", new_line_no);
                frame.render_widget(
                    Paragraph::new(Span::styled(right_gutter, Style::default().fg(Color::DarkGray))),
                    Rect::new(right_inner.x, y, gutter_width, 1),
                );
                // Right content.
                let right_text: String = l.chars().take(right_content_w).collect();
                frame.render_widget(
                    Paragraph::new(Span::raw(right_text)),
                    Rect::new(right_content_x, y, right_inner.width.saturating_sub(gutter_width), 1),
                );
            }
            DiffLine::LeftOnly(l) => {
                old_line_no += 1;

                let del_style = Style::default().fg(Color::White).bg(Color::Red);

                // Left gutter.
                let left_gutter = format!("{:>4} ", old_line_no);
                frame.render_widget(
                    Paragraph::new(Span::styled(left_gutter, Style::default().fg(Color::DarkGray).bg(Color::Red))),
                    Rect::new(left_inner.x, y, gutter_width, 1),
                );
                // Left content — red background, fill full width.
                let left_text: String = l.chars().take(left_content_w).collect();
                let padded = format!("{:<width$}", left_text, width = left_content_w);
                frame.render_widget(
                    Paragraph::new(Span::styled(padded, del_style)),
                    Rect::new(left_content_x, y, left_inner.width.saturating_sub(gutter_width), 1),
                );

                // Right side: empty.
                let empty = " ".repeat(right_inner.width as usize);
                frame.render_widget(
                    Paragraph::new(Span::styled(empty, Style::default().fg(Color::DarkGray))),
                    Rect::new(right_inner.x, y, right_inner.width, 1),
                );
            }
            DiffLine::RightOnly(r) => {
                new_line_no += 1;

                let add_style = Style::default().fg(Color::White).bg(Color::Green);

                // Left side: empty.
                let empty = " ".repeat(left_inner.width as usize);
                frame.render_widget(
                    Paragraph::new(Span::styled(empty, Style::default().fg(Color::DarkGray))),
                    Rect::new(left_inner.x, y, left_inner.width, 1),
                );

                // Right gutter.
                let right_gutter = format!("{:>4} ", new_line_no);
                frame.render_widget(
                    Paragraph::new(Span::styled(right_gutter, Style::default().fg(Color::DarkGray).bg(Color::Green))),
                    Rect::new(right_inner.x, y, gutter_width, 1),
                );
                // Right content — green background, fill full width.
                let right_text: String = r.chars().take(right_content_w).collect();
                let padded = format!("{:<width$}", right_text, width = right_content_w);
                frame.render_widget(
                    Paragraph::new(Span::styled(padded, add_style)),
                    Rect::new(right_content_x, y, right_inner.width.saturating_sub(gutter_width), 1),
                );
            }
        }
    }

    // Draw minimap with diff change markers.
    let diff_markers: Vec<(usize, Color)> = dv
        .lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| match line {
            DiffLine::LeftOnly(_) => Some((i, Color::Red)),
            DiffLine::RightOnly(_) => Some((i, Color::Green)),
            DiffLine::Both(_, _) => None,
        })
        .collect();
    draw_minimap(
        frame,
        diff_minimap_area,
        &diff_markers,
        dv.lines.len(),
        scroll,
        viewport_height,
    );
}

/// Draw the tab bar showing all open tabs.
fn draw_tab_bar(frame: &mut Frame, app: &App, area: Rect) {
    if area.height == 0 {
        return;
    }

    let active_idx = app.tabs.active_index();
    let tabs = app.tabs.tabs();
    let mut spans: Vec<Span> = Vec::new();
    let max_width = area.width as usize;
    let mut used_width = 0;

    for (i, tab) in tabs.iter().enumerate() {
        let is_active = i == active_idx;
        let label = if i < 9 {
            format!(" {}:{} ", i + 1, tab.title())
        } else {
            format!(" {} ", tab.title())
        };

        let label_len = label.len();
        if used_width + label_len + 1 > max_width {
            // Truncate with indicator.
            if used_width < max_width {
                let remaining = max_width - used_width;
                let truncated: String = "...".chars().take(remaining).collect();
                spans.push(Span::styled(
                    truncated,
                    Style::default().fg(Color::DarkGray),
                ));
            }
            break;
        }

        let style = if is_active {
            Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        spans.push(Span::styled(label, style));

        // Separator between tabs.
        if i + 1 < tabs.len() {
            spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
            used_width += 1;
        }
        used_width += label_len;
    }

    let line = ratatui::text::Line::from(spans);
    let bg_style = Style::default().bg(Color::Black);
    let paragraph = Paragraph::new(line).style(bg_style);
    frame.render_widget(paragraph, area);
}

/// Convert a `TermColor` to a ratatui `Color`, using `fallback` for Default.
fn term_color_to_ratatui(tc: crate::embedded_terminal::TermColor, fallback: Color) -> Color {
    use crate::embedded_terminal::TermColor;
    match tc {
        TermColor::Default => fallback,
        TermColor::Indexed(idx) => match idx {
            0 => Color::Black,
            1 => Color::Red,
            2 => Color::Green,
            3 => Color::Yellow,
            4 => Color::Blue,
            5 => Color::Magenta,
            6 => Color::Cyan,
            7 => Color::White,
            8 => Color::DarkGray,
            9 => Color::LightRed,
            10 => Color::LightGreen,
            11 => Color::LightYellow,
            12 => Color::LightBlue,
            13 => Color::LightMagenta,
            14 => Color::LightCyan,
            15 => Color::Gray,
            n => Color::Indexed(n),
        },
        TermColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Draw the embedded PTY terminal pane.
fn draw_terminal(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.terminal_focused;

    let title = if focused {
        let offset = app.terminal.scroll_offset();
        if offset > 0 {
            " Terminal (scrollback) "
        } else {
            " Terminal (focused) "
        }
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

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    // Get a bottom-anchored snapshot of the terminal screen.
    let (snapshot, cursor_row, cursor_col) = app.terminal.snapshot();

    for (row_idx, row) in snapshot.iter().enumerate() {
        let y = inner.y + row_idx as u16;
        if y >= inner.y + inner.height {
            break;
        }

        // Build styled spans for this row.
        let mut spans: Vec<Span> = Vec::new();
        let max_col = (inner.width as usize).min(row.len());

        let mut col = 0;
        while col < max_col {
            // Group consecutive cells with the same style.
            let cell = &row[col];
            let fg = term_color_to_ratatui(cell.fg, Color::White);
            let bg = term_color_to_ratatui(cell.bg, Color::Reset);
            let bold = cell.bold;

            let mut text = String::new();
            text.push(cell.ch);

            let mut next = col + 1;
            while next < max_col {
                let nc = &row[next];
                let nfg = term_color_to_ratatui(nc.fg, Color::White);
                let nbg = term_color_to_ratatui(nc.bg, Color::Reset);
                if nfg == fg && nbg == bg && nc.bold == bold {
                    text.push(nc.ch);
                    next += 1;
                } else {
                    break;
                }
            }

            let mut style = Style::default().fg(fg).bg(bg);
            if bold {
                style = style.add_modifier(Modifier::BOLD);
            }

            // Show cursor as reversed when terminal is focused.
            if focused
                && row_idx == cursor_row
                && col <= cursor_col
                && cursor_col < next
                && app.terminal.scroll_offset() == 0
            {
                // Split the span at the cursor position to reverse just that cell.
                let cursor_offset = cursor_col - col;
                let before: String = text.chars().take(cursor_offset).collect();
                let cursor_ch: String = text.chars().skip(cursor_offset).take(1).collect();
                let after: String = text.chars().skip(cursor_offset + 1).collect();

                if !before.is_empty() {
                    spans.push(Span::styled(before, style));
                }
                if !cursor_ch.is_empty() {
                    spans.push(Span::styled(
                        cursor_ch,
                        style.add_modifier(Modifier::REVERSED),
                    ));
                }
                if !after.is_empty() {
                    spans.push(Span::styled(after, style));
                }
            } else {
                spans.push(Span::styled(text, style));
            }

            col = next;
        }

        let line = ratatui::text::Line::from(spans);
        frame.render_widget(Paragraph::new(line), Rect::new(inner.x, y, inner.width, 1));
    }
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

    if inner.height == 0 {
        return;
    }

    // Tab header row: Files | Git
    let files_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let git_style = Style::default().fg(Color::DarkGray);
    let tab_line = Line::from(vec![
        Span::styled(" Files", files_style),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled("Git ", git_style),
    ]);
    frame.render_widget(
        Paragraph::new(tab_line),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    // Adjust inner area below the tab header.
    let tree_inner = Rect::new(inner.x, inner.y + 1, inner.width, inner.height.saturating_sub(1));

    if tree_inner.height == 0 || app.file_tree.entries.is_empty() {
        let empty = Paragraph::new(Span::styled(
            " (empty)",
            Style::default().fg(Color::DarkGray),
        ));
        frame.render_widget(empty, tree_inner);
        return;
    }

    let visible_height = tree_inner.height as usize;
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
        let y = tree_inner.y + i as u16;
        let abs_idx = scroll_offset + i;
        let is_selected = abs_idx == selected;

        // Build display string: indentation + icon + name.
        let indent = "  ".repeat(entry.depth);
        if entry.is_dir {
            let icon = if entry.expanded { "▾ " } else { "▸ " };
            let display = format!("{}{}{}", indent, icon, entry.name);
            let display: String = display.chars().take(tree_inner.width as usize).collect();
            let style = if is_selected { selected_style } else { dir_style };
            let line = Paragraph::new(Span::styled(display, style));
            frame.render_widget(line, Rect::new(tree_inner.x, y, tree_inner.width, 1));
        } else {
            let icon = file_icon(&entry.name);
            let icon_color = file_icon_color(&entry.name);

            if is_selected {
                // When selected, render as one span with REVERSED style.
                let display = format!("{}{}{}", indent, icon, entry.name);
                let display: String = display.chars().take(tree_inner.width as usize).collect();
                let line = Paragraph::new(Span::styled(display, selected_style));
                frame.render_widget(line, Rect::new(tree_inner.x, y, tree_inner.width, 1));
            } else {
                // Render icon with its own color, name in white.
                let spans = Line::from(vec![
                    Span::raw(&indent),
                    Span::styled(icon, Style::default().fg(icon_color)),
                    Span::styled(&entry.name, file_style),
                ]);
                let line = Paragraph::new(spans);
                frame.render_widget(line, Rect::new(tree_inner.x, y, tree_inner.width, 1));
            }
        }
    }
}

/// Draw the source control sidebar panel.
fn draw_conversation_history(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.conversation_history_focused;
    let (title, border_color) = if focused {
        (" AI History [focused] ", Color::Yellow)
    } else {
        (" AI History ", Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(border_color));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let panel = &app.conversation_history;
    let max_width = inner.width as usize;
    let mut y = inner.y;
    let end_y = inner.y + inner.height;

    if panel.conversations.is_empty() {
        let msg = Paragraph::new("No conversations")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(
            msg,
            Rect::new(inner.x, y, inner.width, 1),
        );
        return;
    }

    let visible_start = panel.scroll;
    for (i, entry) in panel.conversations.iter().enumerate().skip(visible_start) {
        if y >= end_y {
            break;
        }

        let is_selected = i == panel.selected;
        let is_expanded = panel.expanded == Some(i);

        // Conversation header line: summary or file path + badge.
        let label = entry
            .summary
            .as_deref()
            .unwrap_or(&entry.file_path);
        let badge = format!(" {}f {}m", entry.files_changed, entry.message_count);
        let avail = max_width.saturating_sub(badge.len());
        let truncated: String = label.chars().take(avail).collect();
        let line = format!("{truncated}{badge}");

        let style = if is_selected && focused {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        } else if is_selected {
            Style::default().fg(Color::Black).bg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };

        let prefix = if is_expanded { "v " } else { "> " };
        let display: String = format!("{prefix}{line}").chars().take(max_width).collect();

        frame.render_widget(
            Paragraph::new(display).style(style),
            Rect::new(inner.x, y, inner.width, 1),
        );
        y += 1;

        // Timestamp line.
        if y < end_y {
            let ts: String = entry
                .updated_at
                .chars()
                .take(max_width.saturating_sub(2))
                .collect();
            let ts_line = format!("  {ts}");
            frame.render_widget(
                Paragraph::new(ts_line).style(Style::default().fg(Color::DarkGray)),
                Rect::new(inner.x, y, inner.width, 1),
            );
            y += 1;
        }

        // Git context line (branch @ commit).
        let git_info = match (entry.branch.as_deref(), entry.git_commit.as_deref()) {
            (Some(b), Some(c)) => Some(format!("{b} @ {c}")),
            (Some(b), None) => Some(b.to_string()),
            (None, Some(c)) => Some(format!("@ {c}")),
            (None, None) => None,
        };
        if let Some(info) = git_info {
            if y < end_y {
                let git_line: String = format!("  {info}")
                    .chars()
                    .take(max_width)
                    .collect();
                frame.render_widget(
                    Paragraph::new(git_line).style(Style::default().fg(Color::Magenta)),
                    Rect::new(inner.x, y, inner.width, 1),
                );
                y += 1;
            }
        }

        // If expanded, show messages.
        if is_expanded {
            let msgs = &panel.expanded_messages;
            for (mi, msg) in msgs.iter().enumerate().skip(panel.message_scroll) {
                if y >= end_y {
                    break;
                }
                let _ = mi;
                let role_color = match msg.role {
                    MessageRole::HumanIntent => Color::Green,
                    MessageRole::AiResponse => Color::Cyan,
                    MessageRole::System => Color::DarkGray,
                };
                let role_label = format!("  {}: ", msg.role);
                let content_avail = max_width.saturating_sub(role_label.len());
                let content: String = msg.content.chars().take(content_avail).collect();
                let line = format!("{role_label}{content}");
                let display: String = line.chars().take(max_width).collect();

                frame.render_widget(
                    Paragraph::new(display).style(Style::default().fg(role_color)),
                    Rect::new(inner.x, y, inner.width, 1),
                );
                y += 1;
            }
            // Separator after messages.
            if y < end_y {
                let sep: String = "─".repeat(max_width);
                frame.render_widget(
                    Paragraph::new(sep).style(Style::default().fg(Color::DarkGray)),
                    Rect::new(inner.x, y, inner.width, 1),
                );
                y += 1;
            }
        }
    }
}

fn draw_source_control(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.source_control_focused;
    let (title, border_color) = if focused {
        (" Git [focused] ", Color::Yellow)
    } else {
        (" Git ", Color::Cyan)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let sc = &app.source_control;
    let mut y = inner.y;
    let max_y = inner.y + inner.height;

    // --- Tab header row: Files | Git ---
    if y < max_y {
        let files_style = Style::default().fg(Color::DarkGray);
        let git_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
        let line = Line::from(vec![
            Span::styled(" Files", files_style),
            Span::styled(" | ", Style::default().fg(Color::DarkGray)),
            Span::styled("Git ", git_style),
        ]);
        frame.render_widget(Paragraph::new(line), Rect::new(inner.x, y, inner.width, 1));
        y += 1;
    }

    // --- Branch and sync status ---
    if y < max_y {
        let branch_name = sc.branch.as_deref().unwrap_or("detached");
        let mut spans = vec![
            Span::styled(" \u{e0a0} ", Style::default().fg(Color::Cyan)),
            Span::styled(branch_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ];

        if sc.ahead > 0 || sc.behind > 0 {
            let mut sync_parts = String::from("  ");
            if sc.behind > 0 {
                sync_parts.push_str(&format!("{}↓", sc.behind));
            }
            if sc.ahead > 0 {
                if sc.behind > 0 {
                    sync_parts.push(' ');
                }
                sync_parts.push_str(&format!("{}↑", sc.ahead));
            }
            spans.push(Span::styled(sync_parts, Style::default().fg(Color::Yellow)));
        }

        frame.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect::new(inner.x, y, inner.width, 1),
        );
        y += 1;
    }

    // Blank separator.
    if y < max_y {
        y += 1;
    }

    // --- Commit message box ---
    let msg_focused = sc.focused_section == GitPanelSection::CommitMessage;
    if y < max_y {
        let header_style = if msg_focused {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let label = if sc.editing_commit_message {
            " Commit Message (editing)"
        } else {
            " Commit Message"
        };
        frame.render_widget(
            Paragraph::new(Span::styled(label, header_style)),
            Rect::new(inner.x, y, inner.width, 1),
        );
        y += 1;
    }

    // Show the commit message (up to 3 lines).
    let msg_lines = if sc.commit_message.is_empty() {
        vec!["  (type commit message)".to_string()]
    } else {
        sc.commit_message
            .lines()
            .take(3)
            .map(|l| format!("  {l}"))
            .collect::<Vec<_>>()
    };
    for line_text in &msg_lines {
        if y >= max_y {
            break;
        }
        let style = if sc.editing_commit_message && msg_focused {
            Style::default().fg(Color::White)
        } else if sc.commit_message.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        let display: String = line_text.chars().take(inner.width as usize).collect();
        frame.render_widget(
            Paragraph::new(Span::styled(display, style)),
            Rect::new(inner.x, y, inner.width, 1),
        );
        y += 1;
    }

    // Blank separator line.
    if y < max_y {
        y += 1;
    }

    // --- Staged changes ---
    let staged_focused = sc.focused_section == GitPanelSection::StagedFiles;
    if y < max_y {
        let header_style = if staged_focused {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green)
        };
        let header = format!(" Staged Changes ({})", sc.staged.len());
        frame.render_widget(
            Paragraph::new(Span::styled(header, header_style)),
            Rect::new(inner.x, y, inner.width, 1),
        );
        y += 1;
    }

    for (i, entry) in sc.staged.iter().enumerate() {
        if y >= max_y {
            break;
        }
        let is_selected = staged_focused && i == sc.selected;
        let status_style = status_color(entry.status);
        let display = format!("  {} {}", entry.status.label(), entry.rel_path);
        let display: String = display.chars().take(inner.width as usize).collect();

        if is_selected {
            let style = Style::default().add_modifier(Modifier::REVERSED);
            frame.render_widget(
                Paragraph::new(Span::styled(display, style)),
                Rect::new(inner.x, y, inner.width, 1),
            );
        } else {
            frame.render_widget(
                Paragraph::new(Span::styled(display, status_style)),
                Rect::new(inner.x, y, inner.width, 1),
            );
        }
        y += 1;
    }

    // Blank separator.
    if y < max_y {
        y += 1;
    }

    // --- Unstaged changes ---
    let changed_focused = sc.focused_section == GitPanelSection::ChangedFiles;
    if y < max_y {
        let header_style = if changed_focused {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Red)
        };
        let header = format!(" Changes ({})", sc.changed.len());
        frame.render_widget(
            Paragraph::new(Span::styled(header, header_style)),
            Rect::new(inner.x, y, inner.width, 1),
        );
        y += 1;
    }

    for (i, entry) in sc.changed.iter().enumerate() {
        if y >= max_y {
            break;
        }
        let is_selected = changed_focused && i == sc.selected;
        let status_style = status_color(entry.status);
        let display = format!("  {} {}", entry.status.label(), entry.rel_path);
        let display: String = display.chars().take(inner.width as usize).collect();

        if is_selected {
            let style = Style::default().add_modifier(Modifier::REVERSED);
            frame.render_widget(
                Paragraph::new(Span::styled(display, style)),
                Rect::new(inner.x, y, inner.width, 1),
            );
        } else {
            frame.render_widget(
                Paragraph::new(Span::styled(display, status_style)),
                Rect::new(inner.x, y, inner.width, 1),
            );
        }
        y += 1;
    }
}

/// Get the display color for a git file status.
fn status_color(status: GitFileStatus) -> Style {
    match status {
        GitFileStatus::Modified => Style::default().fg(Color::Yellow),
        GitFileStatus::Added => Style::default().fg(Color::Green),
        GitFileStatus::Deleted => Style::default().fg(Color::Red),
        GitFileStatus::Renamed => Style::default().fg(Color::Blue),
        GitFileStatus::Untracked => Style::default().fg(Color::DarkGray),
    }
}

/// Return a Nerd Font icon (+ trailing space) for a file based on its extension.
fn file_icon(name: &str) -> &'static str {
    let ext = match name.rsplit_once('.') {
        Some((_, e)) => e,
        None => "",
    };
    match ext {
        // Rust
        "rs" => "\u{e7a8} ",  //
        // JavaScript / TypeScript
        "js" | "mjs" | "cjs" => "\u{e781} ",  //
        "ts" | "mts" | "cts" => "\u{e628} ",  //
        "jsx" => "\u{e7ba} ",  //
        "tsx" => "\u{e7ba} ",  //
        // Web
        "html" | "htm" => "\u{e736} ",  //
        "css" => "\u{e749} ",  //
        "scss" | "sass" => "\u{e749} ",
        // Data / Config
        "json" => "\u{e60b} ",  //
        "yaml" | "yml" => "\u{e6a8} ",  //
        "toml" => "\u{e6b2} ",  //
        "xml" => "\u{e619} ",  //
        // Elixir / Erlang
        "ex" | "exs" => "\u{e62d} ",  //
        "erl" | "hrl" => "\u{e7b1} ",  //
        // Python
        "py" | "pyi" => "\u{e73c} ",  //
        // Go
        "go" => "\u{e626} ",  //
        // C / C++
        "c" | "h" => "\u{e61e} ",  //
        "cpp" | "cxx" | "cc" | "hpp" => "\u{e61d} ",  //
        // Shell
        "sh" | "bash" | "zsh" | "fish" => "\u{e795} ",  //
        // Ruby
        "rb" => "\u{e791} ",  //
        // Java / Kotlin
        "java" => "\u{e738} ",  //
        "kt" | "kts" => "\u{e634} ",  //
        // Markdown / Text
        "md" | "mdx" => "\u{e73e} ",  //
        "txt" => "\u{f0f6} ",  //
        // Docker
        "dockerfile" => "\u{e7b0} ",  //
        // Git
        "gitignore" | "gitmodules" | "gitattributes" => "\u{e702} ",  //
        // Images
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" | "webp" => "\u{f1c5} ",  //
        // Lock files
        "lock" => "\u{f023} ",  //
        // Catch-all
        _ => match name {
            "Dockerfile" => "\u{e7b0} ",
            "Makefile" | "CMakeLists.txt" => "\u{e779} ",
            _ => "\u{f15b} ",  //  generic file
        },
    }
}

/// Return a color for a file icon based on its extension.
fn file_icon_color(name: &str) -> Color {
    let ext = match name.rsplit_once('.') {
        Some((_, e)) => e,
        None => "",
    };
    match ext {
        "rs" => Color::Rgb(222, 165, 132),         // Rust orange
        "js" | "mjs" | "cjs" => Color::Yellow,     // JS yellow
        "ts" | "mts" | "cts" => Color::Rgb(49, 120, 198), // TS blue
        "jsx" | "tsx" => Color::Rgb(97, 218, 251),  // React cyan
        "html" | "htm" => Color::Rgb(227, 76, 38),  // HTML orange
        "css" | "scss" | "sass" => Color::Rgb(86, 61, 124), // CSS purple
        "json" => Color::Yellow,
        "yaml" | "yml" => Color::Rgb(203, 23, 30),  // Red
        "toml" => Color::Rgb(156, 154, 150),        // Gray
        "xml" => Color::Rgb(227, 76, 38),
        "ex" | "exs" => Color::Rgb(110, 74, 126),   // Elixir purple
        "erl" | "hrl" => Color::Rgb(169, 36, 52),   // Erlang red
        "py" | "pyi" => Color::Rgb(55, 118, 171),   // Python blue
        "go" => Color::Rgb(0, 173, 216),             // Go cyan
        "c" | "h" => Color::Rgb(85, 85, 255),       // C blue
        "cpp" | "cxx" | "cc" | "hpp" => Color::Rgb(0, 89, 156),
        "sh" | "bash" | "zsh" | "fish" => Color::Green,
        "rb" => Color::Rgb(204, 52, 45),            // Ruby red
        "java" => Color::Rgb(176, 114, 25),          // Java orange
        "kt" | "kts" => Color::Rgb(169, 123, 255),  // Kotlin purple
        "md" | "mdx" => Color::Rgb(66, 165, 245),   // Markdown blue
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" | "webp" => Color::Magenta,
        "lock" => Color::DarkGray,
        _ => match name {
            "Dockerfile" => Color::Rgb(56, 152, 236),
            "Makefile" | "CMakeLists.txt" => Color::Rgb(111, 166, 58),
            _ => Color::DarkGray,
        },
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

/// Draw the help overlay (centered popup).
fn draw_help(frame: &mut Frame, app: &App, area: Rect) {
    let width = area.width.min(90);
    let height = (area.height * 4 / 5).max(10);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup_area);

    if app.help.in_topics_view() {
        // --- Topics view ---
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Help ")
            .border_style(Style::default().fg(Color::Cyan));
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        if inner.height < 3 {
            return;
        }

        // Search bar.
        let query_line = format!("> {}", app.help.query);
        let query_display: String = query_line.chars().take(inner.width as usize).collect();
        let query_widget = Paragraph::new(Span::styled(
            query_display,
            Style::default().fg(Color::Yellow),
        ));
        frame.render_widget(query_widget, Rect::new(inner.x, inner.y, inner.width, 1));

        // Hint bar at the bottom.
        let hint = " ?/F1 open  j/k navigate  Enter view  Esc close ";
        let hint_y = inner.y + inner.height.saturating_sub(1);
        frame.render_widget(
            Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray))),
            Rect::new(inner.x, hint_y, inner.width, 1),
        );

        // Topic list area.
        let list_y = inner.y + 1;
        let list_height = inner.height.saturating_sub(3) as usize; // -1 query, -1 blank, -1 hint

        // Build display lines with section headers.
        let mut display_lines: Vec<(Option<usize>, String, String)> = Vec::new(); // (topic_idx, display, section)
        let mut last_section = String::new();
        for (filter_pos, &topic_idx) in app.help.filtered.iter().enumerate() {
            let topic = &app.help.topics()[topic_idx];
            if topic.section != last_section {
                display_lines.push((None, topic.section.clone(), String::new()));
                last_section = topic.section.clone();
            }
            display_lines.push((Some(filter_pos), format!("  {}", topic.title), topic.section.clone()));
        }

        // Scroll so selected item is visible.
        let selected_display_idx = display_lines
            .iter()
            .position(|(fp, _, _)| *fp == Some(app.help.selected))
            .unwrap_or(0);
        let scroll_start = if selected_display_idx >= list_height {
            selected_display_idx - list_height + 1
        } else {
            0
        };

        for (i, (filter_pos, text, _)) in display_lines
            .iter()
            .enumerate()
            .skip(scroll_start)
            .take(list_height)
        {
            let row_y = list_y + (i - scroll_start) as u16;
            let style = if filter_pos.is_none() {
                // Section header.
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if *filter_pos == Some(app.help.selected) {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let display: String = text.chars().take(inner.width as usize).collect();
            frame.render_widget(
                Paragraph::new(Span::styled(display, style)),
                Rect::new(inner.x, row_y, inner.width, 1),
            );
        }
    } else {
        // --- Content view ---
        let title = app
            .help
            .current_topic()
            .map(|t| format!(" {} ", t.title))
            .unwrap_or_else(|| " Help ".to_string());
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::Cyan));
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        if inner.height < 3 {
            return;
        }

        // Hint bar at the bottom.
        let hint = " Esc back  j/k scroll  u/d page ";
        let hint_y = inner.y + inner.height.saturating_sub(1);
        frame.render_widget(
            Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray))),
            Rect::new(inner.x, hint_y, inner.width, 1),
        );

        // Content area.
        let content_height = inner.height.saturating_sub(1) as usize; // -1 for hint bar

        if let Some(topic) = app.help.current_topic() {
            let total = topic.rendered.len();
            let scroll = app.help.scroll;

            // Scroll indicator.
            if total > content_height {
                let pct = if total > 0 {
                    (scroll * 100) / total.saturating_sub(1)
                } else {
                    0
                };
                let indicator = format!("{pct}%");
                let ind_x = inner.x + inner.width.saturating_sub(indicator.len() as u16 + 1);
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        indicator,
                        Style::default().fg(Color::DarkGray),
                    )),
                    Rect::new(ind_x, hint_y, inner.width, 1),
                );
            }

            for (i, line) in topic
                .rendered
                .iter()
                .skip(scroll)
                .take(content_height)
                .enumerate()
            {
                let row_y = inner.y + i as u16;
                // Truncate spans to fit width.
                let truncated = truncate_line(line, inner.width as usize);
                frame.render_widget(
                    Paragraph::new(truncated),
                    Rect::new(inner.x, row_y, inner.width, 1),
                );
            }
        }
    }
}

/// Truncate a ratatui line to fit within `max_width` columns.
fn truncate_line<'a>(line: &ratatui::text::Line<'a>, max_width: usize) -> ratatui::text::Line<'a> {
    let mut remaining = max_width;
    let mut spans = Vec::new();
    for span in &line.spans {
        if remaining == 0 {
            break;
        }
        let content = span.content.as_ref();
        if content.len() <= remaining {
            spans.push(span.clone());
            remaining -= content.len();
        } else {
            let truncated: String = content.chars().take(remaining).collect();
            remaining = 0;
            spans.push(Span::styled(truncated, span.style));
        }
    }
    ratatui::text::Line::from(spans)
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
    let tab = app.tab();

    for i in 0..visible_lines {
        let line_idx = tab.scroll_row + i;
        if let Some(rope_line) = tab.buffer.line(line_idx) {
            let line_num = format!("{:>4} ", line_idx + 1);
            let content: String = rope_line
                .chars()
                .skip(tab.scroll_col)
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
                if let Some(author) = tab.buffer.line_author(line_idx) {
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
            let line_start_idx = tab
                .buffer
                .cursor_to_char_idx(&aura_core::Cursor::new(line_idx, 0));
            let visible_start = tab.scroll_col;
            let visible_chars: Vec<char> = content.chars().collect();
            let hl_line = tab.highlight_lines.get(line_idx);

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
        Mode::Diff => Style::default().fg(Color::Black).bg(Color::Cyan),
    };

    let file_name = app
        .buffer()
        .file_path()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("[scratch]");

    let modified = if app.buffer().is_modified() {
        " [+]"
    } else {
        ""
    };

    // Build "last change by" indicator.
    let last_change = app
        .buffer()
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
    let right = format!(" {}:{} ", app.cursor().row + 1, app.cursor().col + 1);

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
                        .buffer()
                        .char_idx_to_cursor(proposal.start.min(app.buffer().len_chars()))
                        .row;
                    let end_line = app
                        .buffer()
                        .char_idx_to_cursor(proposal.end.min(app.buffer().len_chars()))
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
    let cursor_x = (app.cursor().col - app.tab().scroll_col) as u16 + editor_area.x + 6;
    let cursor_y = (app.cursor().row - app.tab().scroll_row) as u16 + editor_area.y + 1;

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
    let cursor_screen_y =
        app.cursor().row.saturating_sub(app.tab().scroll_row) as u16 + editor_area.y;
    let cursor_screen_x =
        app.cursor().col.saturating_sub(app.tab().scroll_col) as u16 + editor_area.x + 6; // gutter width

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_line_to_row_basic() {
        // 100-line file, 10-row minimap.
        assert_eq!(map_line_to_row(0, 100, 10), Some(0));
        assert_eq!(map_line_to_row(50, 100, 10), Some(5));
        assert_eq!(map_line_to_row(99, 100, 10), Some(9));
    }

    #[test]
    fn map_line_to_row_small_file_large_minimap() {
        // 5-line file, 20-row minimap.
        assert_eq!(map_line_to_row(0, 5, 20), Some(0));
        assert_eq!(map_line_to_row(1, 5, 20), Some(4));
        assert_eq!(map_line_to_row(4, 5, 20), Some(16));
    }

    #[test]
    fn map_line_to_row_zero_total_lines() {
        assert_eq!(map_line_to_row(0, 0, 10), None);
    }

    #[test]
    fn map_line_to_row_zero_minimap_height() {
        assert_eq!(map_line_to_row(0, 100, 0), None);
    }

    #[test]
    fn map_line_to_row_line_beyond_total_clamped() {
        // Line 200 in a 100-line file should clamp to the last row.
        assert_eq!(map_line_to_row(200, 100, 10), Some(9));
    }

    #[test]
    fn map_line_to_row_single_line_file() {
        // 1-line file: line 0 maps to row 0 regardless of minimap height.
        assert_eq!(map_line_to_row(0, 1, 10), Some(0));
        assert_eq!(map_line_to_row(0, 1, 1), Some(0));
    }
}
