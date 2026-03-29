//! Rendering the editor UI with ratatui.

use crate::app::{App, ConversationPanel, Mode};
use crate::chat_panel::{ChatItem, ChatRole, ToolCallStatus};
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

    // Layout: optional tab bar + editor area + optional proposal + optional debug panel + optional terminal + status bar + command bar.
    let has_proposal = app.proposal.is_some() && app.mode == Mode::Review;
    let has_terminal = app.terminal.visible;
    let has_debug_panel = app.debug_panel.visible;
    let terminal_height = if has_terminal { app.terminal.height } else { 0 };
    let debug_panel_height = if has_debug_panel {
        app.debug_panel.height
    } else {
        0
    };
    let tab_bar_height: u16 = 1;

    let chunks = if has_proposal {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(tab_bar_height),     // Tab bar (0 or 1 row)
                Constraint::Percentage(50),             // Editor (original)
                Constraint::Percentage(50),             // Proposal (diff)
                Constraint::Length(debug_panel_height), // Debug panel (0 when hidden)
                Constraint::Length(terminal_height),    // Terminal pane (0 when hidden)
                Constraint::Length(1),                  // Status bar
                Constraint::Length(1),                  // Command bar
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(tab_bar_height),     // Tab bar (0 or 1 row)
                Constraint::Min(1),                     // Editor
                Constraint::Length(0),                  // No proposal pane
                Constraint::Length(debug_panel_height), // Debug panel (0 when hidden)
                Constraint::Length(terminal_height),    // Terminal pane (0 when hidden)
                Constraint::Length(1),                  // Status bar
                Constraint::Length(1),                  // Command bar
            ])
            .split(area)
    };

    let tab_bar_area = chunks[0];
    let editor_area_raw = chunks[1];
    let proposal_area = chunks[2];
    let debug_panel_area = chunks[3];
    let terminal_area = chunks[4];
    let status_area = chunks[5];
    let command_area = chunks[6];

    // Always draw the tab bar.
    app.tab_bar_rect = tab_bar_area;
    draw_tab_bar(frame, app, tab_bar_area);

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

    // If the conversation history panel or chat panel is visible, split off the right side.
    let right_panel_width = if app.chat_panel.visible {
        Some(app.chat_panel.width)
    } else if app.conversation_history.visible {
        Some(app.conversation_history.width)
    } else {
        None
    };

    let (editor_area, right_panel_area) = if let Some(width) = right_panel_width {
        let hsplit = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(width)])
            .split(editor_area_outer);
        (hsplit[0], Some(hsplit[1]))
    } else {
        (editor_area_outer, None)
    };

    // Draw the active right panel.
    if let Some(area) = right_panel_area {
        if app.chat_panel.visible {
            app.chat_panel_rect = area;
            app.conv_history_rect = Rect::default();
            draw_chat_panel(frame, app, area);
        } else {
            app.conv_history_rect = area;
            app.chat_panel_rect = Rect::default();
            draw_conversation_history(frame, app, area);
        }
    } else {
        app.conv_history_rect = Rect::default();
        app.chat_panel_rect = Rect::default();
    }

    // Save panel rects for mouse click-to-focus.
    app.editor_rect = editor_area;
    if has_terminal {
        app.terminal_rect = terminal_area;
    } else {
        app.terminal_rect = Rect::default();
    }

    // If merge view is active, render the 3-panel merge conflict editor.
    if app.merge_view.is_some() {
        draw_merge_view(frame, app, editor_area);

        if has_terminal {
            let inner_h = terminal_area.height.saturating_sub(2);
            let inner_w = terminal_area.width.saturating_sub(2);
            if inner_h > 0 && inner_w > 0 {
                app.terminal.resize(inner_w, inner_h);
            }
            draw_terminal(frame, app, terminal_area);
        }

        if has_debug_panel {
            app.debug_panel_rect = debug_panel_area;
            draw_debug_panel(frame, app, debug_panel_area);
        }

        draw_status_bar(frame, app, status_area);
        draw_command_bar(frame, app, command_area);
        return;
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

        // Draw debug panel if visible.
        if has_debug_panel {
            app.debug_panel_rect = debug_panel_area;
            draw_debug_panel(frame, app, debug_panel_area);
        }

        draw_status_bar(frame, app, status_area);
        draw_command_bar(frame, app, command_area);
    } else {
        // Split the editor area if split panes are active.
        if app.split_active {
            use crate::app::SplitDirection;
            let (pane_a, pane_b) = match app.split_direction {
                SplitDirection::Vertical => {
                    let hsplit = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(editor_area);
                    (hsplit[0], hsplit[1])
                }
                SplitDirection::Horizontal => {
                    let vsplit = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(editor_area);
                    (vsplit[0], vsplit[1])
                }
            };

            let primary_idx = app.tabs.active_index();
            let secondary_idx = app.split_tab_idx.min(app.tabs.count().saturating_sub(1));
            let primary_focused = !app.split_focus_secondary
                && !app.terminal_focused
                && !app.file_tree_focused
                && !app.source_control_focused;
            let secondary_focused = app.split_focus_secondary
                && !app.terminal_focused
                && !app.file_tree_focused
                && !app.source_control_focused;

            draw_editor_pane(frame, app, pane_a, primary_idx, primary_focused);
            draw_editor_pane(frame, app, pane_b, secondary_idx, secondary_focused);
        } else {
            let is_focused = !app.terminal_focused
                && !app.file_tree_focused
                && !app.source_control_focused
                && !app.conversation_history_focused;
            draw_editor_pane(frame, app, editor_area, app.tabs.active_index(), is_focused);
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

        // Draw debug panel if visible.
        if has_debug_panel {
            app.debug_panel_rect = debug_panel_area;
            draw_debug_panel(frame, app, debug_panel_area);
        }

        draw_status_bar(frame, app, status_area);
        draw_command_bar(frame, app, command_area);

        // Compute the editor inner area for overlays (account for block borders).
        let editor_inner_for_popups = Rect::new(
            editor_area.x + 1,
            editor_area.y + 1,
            editor_area.width.saturating_sub(2),
            editor_area.height.saturating_sub(2),
        );

        // Render ghost suggestion if present.
        if let Some(suggestion) = app.current_ghost_suggestion() {
            draw_ghost_text(frame, app, editor_inner_for_popups, suggestion);
        }

        // Render hover popup if present.
        if let Some(hover_text) = app.tab().hover_info.clone() {
            draw_hover_popup(frame, app, editor_inner_for_popups, &hover_text);
        }

        // Render references panel if present.
        if app.references_panel.is_some() {
            draw_references_panel(frame, app, area);
        }

        // Render rename input if active.
        if app.rename_active {
            draw_rename_input(frame, app, command_area);
        }

        // Render conversation panel if present.
        if let Some(panel) = &app.conversation_panel {
            draw_conversation_panel(frame, editor_inner_for_popups, panel);
        }

        // Render file picker overlay if visible.
        if app.file_picker.visible {
            draw_file_picker(frame, app, area);
        }

        // Render command palette if visible.
        if app.command_palette.visible {
            draw_command_palette(frame, &app.command_palette, area);
        }

        // Render help overlay if visible.
        if app.help.visible {
            draw_help(frame, app, area);
        }

        // Render git graph modal if visible.
        if app.git_graph.visible {
            draw_git_graph_modal(frame, &app.git_graph, area);
        }

        // Render branch picker if visible.
        if app.branch_picker.visible {
            draw_branch_picker(frame, &app.branch_picker, area);
        }

        // Render settings modal if visible.
        if app.settings_modal.visible {
            draw_settings_modal(frame, &app.settings_modal, area);
        }

        // Render conversation detail modal if visible.
        if app.conversation_history.detail_view {
            draw_conversation_detail(frame, &app.conversation_history, area);
        }

        // Render update notification toast.
        if app.update_notification_visible {
            draw_update_notification(frame, app, area);
        }

        // Render update confirmation modal.
        if app.update_modal_visible {
            draw_update_modal(frame, app, area);
        }

        // Render close-tab confirmation modal.
        if app.tab_close_confirm.is_some() {
            draw_close_tab_modal(frame, app, area);
        }

        // Position the terminal cursor.
        if app.file_picker.visible {
            // No editor cursor while the file picker is open.
        } else if app.chat_panel_focused && !app.chat_panel.streaming {
            // Chat input cursor is already set inside draw_chat_input — skip editor cursor.
        } else if app.terminal_focused && has_terminal {
            // Position the hardware cursor at the PTY cursor location.
            let inner = terminal_area.inner(ratatui::layout::Margin {
                horizontal: 1,
                vertical: 1,
            });
            let (_snap, t_cursor_row, t_cursor_col) = app.terminal.snapshot();
            let cx = inner.x + t_cursor_col as u16;
            let cy = inner.y + t_cursor_row as u16;
            if cx < inner.right() && cy < inner.bottom() {
                frame.set_cursor_position((cx, cy));
            }
        } else if app.mode != Mode::Review {
            // Compute the content area of the focused pane for cursor positioning.
            let pane_area = if app.split_active {
                use crate::app::SplitDirection;
                let (a, b) = match app.split_direction {
                    SplitDirection::Vertical => {
                        let s = Layout::default()
                            .direction(Direction::Horizontal)
                            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                            .split(editor_area);
                        (s[0], s[1])
                    }
                    SplitDirection::Horizontal => {
                        let s = Layout::default()
                            .direction(Direction::Vertical)
                            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                            .split(editor_area);
                        (s[0], s[1])
                    }
                };
                if app.split_focus_secondary {
                    b
                } else {
                    a
                }
            } else {
                editor_area
            };
            // Account for block border (1px each side) + gutter (6).
            let content_x = pane_area.x + 1 + 6; // border + gutter
            let content_y = pane_area.y + 1; // border
            let content_right = pane_area.right().saturating_sub(1); // border
            let content_bottom = pane_area.bottom().saturating_sub(1); // border

            let tab = app.tab();
            let cursor_x = (tab.cursor.col.saturating_sub(tab.scroll_col)) as u16 + content_x;
            let cursor_y = (tab.cursor.row.saturating_sub(tab.scroll_row)) as u16 + content_y;
            if cursor_x < content_right && cursor_y < content_bottom {
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
    let vp_end_line = viewport_start
        .saturating_add(viewport_lines)
        .min(total_lines);
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
/// Draw the 3-panel merge conflict editor (Incoming | Current | Result).
fn draw_merge_view(frame: &mut Frame, app: &mut App, area: Rect) {
    use crate::merge_view::{MergeFocus, Resolution};

    let view = match &app.merge_view {
        Some(v) => v,
        None => return,
    };

    // Layout: 55% top (two panels side by side), 45% bottom (result).
    let vsplit = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let top_area = vsplit[0];
    let bottom_area = vsplit[1];

    let hsplit = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(top_area);

    let incoming_area = hsplit[0];
    let current_area = hsplit[1];

    // Draw the three panels.
    let focus = view.focus;
    let active_conflict = view.active_conflict;

    // --- Incoming (theirs) panel ---
    {
        let border_color = if focus == MergeFocus::Incoming {
            Color::Green
        } else {
            Color::DarkGray
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(" Incoming (theirs) ");
        let inner = block.inner(incoming_area);
        frame.render_widget(block, incoming_area);

        let lines = view.incoming_lines();
        let scroll = view.scroll_incoming;
        draw_merge_panel_lines(frame, inner, &lines, scroll, active_conflict, Color::Green);
    }

    // --- Current (ours) panel ---
    {
        let border_color = if focus == MergeFocus::Current {
            Color::Blue
        } else {
            Color::DarkGray
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(" Current (ours/HEAD) ");
        let inner = block.inner(current_area);
        frame.render_widget(block, current_area);

        let lines = view.current_lines();
        let scroll = view.scroll_current;
        draw_merge_panel_lines(frame, inner, &lines, scroll, active_conflict, Color::Blue);
    }

    // --- Result panel ---
    {
        let remaining = view.total_conflicts - view.resolved_count;
        let (border_color, title) = if remaining == 0 {
            (
                Color::Green,
                " Result — All Resolved  [c] Complete Merge ".to_string(),
            )
        } else {
            (
                Color::Yellow,
                format!(" Result — {} Conflict(s) Remaining ", remaining),
            )
        };
        let border_color = if focus == MergeFocus::Result {
            border_color
        } else {
            Color::DarkGray
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(title);
        let inner = block.inner(bottom_area);
        frame.render_widget(block, bottom_area);

        let lines = view.result_lines();
        let scroll = view.scroll_result;

        for (i, (line, conflict_idx)) in lines.iter().skip(scroll).enumerate() {
            if i as u16 >= inner.height {
                break;
            }
            let row_y = inner.y + i as u16;
            let style = if let Some(idx) = conflict_idx {
                let res = view.conflict_resolution(*idx);
                if res == Resolution::Unresolved {
                    // Unresolved — yellow background.
                    Style::default().fg(Color::White).bg(Color::Rgb(60, 60, 20))
                } else {
                    // Resolved — green background.
                    Style::default().fg(Color::White).bg(Color::Rgb(20, 50, 20))
                }
            } else {
                Style::default().fg(Color::White)
            };

            let display = if line.len() > inner.width as usize {
                &line[..inner.width as usize]
            } else {
                line.as_str()
            };
            frame.render_widget(
                Paragraph::new(display).style(style),
                Rect::new(inner.x, row_y, inner.width, 1),
            );
        }
    }

    // Draw hint line in the command bar area (handled by command bar already).
    // The status message shows resolution hints.
}

/// Render lines for a merge panel (incoming or current) with conflict highlighting.
fn draw_merge_panel_lines(
    frame: &mut Frame,
    area: Rect,
    lines: &[(String, Option<usize>)],
    scroll: usize,
    active_conflict: usize,
    conflict_color: Color,
) {
    for (i, (line, conflict_idx)) in lines.iter().skip(scroll).enumerate() {
        if i as u16 >= area.height {
            break;
        }
        let row_y = area.y + i as u16;

        let style = if let Some(idx) = conflict_idx {
            if *idx == active_conflict {
                // Active conflict — bright highlight.
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(60, 60, 20))
                    .add_modifier(Modifier::BOLD)
            } else {
                // Other conflict — dim tint.
                let (r, g, b) = match conflict_color {
                    Color::Green => (20, 50, 20),
                    Color::Blue => (20, 20, 60),
                    _ => (40, 40, 40),
                };
                Style::default().fg(Color::White).bg(Color::Rgb(r, g, b))
            }
        } else {
            Style::default().fg(Color::White)
        };

        let display = if line.len() > area.width as usize {
            &line[..area.width as usize]
        } else {
            line.as_str()
        };
        frame.render_widget(
            Paragraph::new(display).style(style),
            Rect::new(area.x, row_y, area.width, 1),
        );
    }
}

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

    // Build syntax-highlighted lines for both sides.
    let ext = dv.file_path.rsplit('.').next().unwrap_or("");
    let mut highlighter = crate::highlight::Language::from_extension(ext)
        .and_then(crate::highlight::SyntaxHighlighter::new);

    // Reconstruct old and new text from diff lines for highlighting.
    let (old_text, new_text) = {
        let mut old = String::new();
        let mut new = String::new();
        for line in &dv.lines {
            match line {
                DiffLine::Both(l, _) => {
                    old.push_str(l);
                    old.push('\n');
                    new.push_str(l);
                    new.push('\n');
                }
                DiffLine::LeftOnly(l) => {
                    old.push_str(l);
                    old.push('\n');
                }
                DiffLine::RightOnly(r) => {
                    new.push_str(r);
                    new.push('\n');
                }
            }
        }
        (old, new)
    };

    let old_hl = highlighter
        .as_mut()
        .map(|h| h.highlight(&old_text, Some(&app.theme)))
        .unwrap_or_default();
    let new_hl = highlighter
        .as_mut()
        .map(|h| h.highlight(&new_text, Some(&app.theme)))
        .unwrap_or_default();

    // Track line indices into highlighted arrays.
    let mut old_hl_idx: usize = 0;
    let mut new_hl_idx: usize = 0;

    // Advance highlight indices up to scroll.
    for line in dv.lines.iter().take(scroll) {
        match line {
            DiffLine::Both(_, _) => {
                old_hl_idx += 1;
                new_hl_idx += 1;
            }
            DiffLine::LeftOnly(_) => {
                old_hl_idx += 1;
            }
            DiffLine::RightOnly(_) => {
                new_hl_idx += 1;
            }
        }
    }

    for (i, diff_line) in dv
        .lines
        .iter()
        .skip(scroll)
        .take(viewport_height)
        .enumerate()
    {
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
                    Paragraph::new(Span::styled(
                        left_gutter,
                        Style::default().fg(Color::DarkGray),
                    )),
                    Rect::new(left_inner.x, y, gutter_width, 1),
                );
                // Left content with syntax highlighting.
                let left_spans = build_highlighted_spans(l, old_hl.get(old_hl_idx), left_content_w);
                frame.render_widget(
                    Paragraph::new(ratatui::text::Line::from(left_spans)),
                    Rect::new(
                        left_content_x,
                        y,
                        left_inner.width.saturating_sub(gutter_width),
                        1,
                    ),
                );

                // Right gutter.
                let right_gutter = format!("{:>4} ", new_line_no);
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        right_gutter,
                        Style::default().fg(Color::DarkGray),
                    )),
                    Rect::new(right_inner.x, y, gutter_width, 1),
                );
                // Right content with syntax highlighting.
                let right_spans =
                    build_highlighted_spans(l, new_hl.get(new_hl_idx), right_content_w);
                frame.render_widget(
                    Paragraph::new(ratatui::text::Line::from(right_spans)),
                    Rect::new(
                        right_content_x,
                        y,
                        right_inner.width.saturating_sub(gutter_width),
                        1,
                    ),
                );

                old_hl_idx += 1;
                new_hl_idx += 1;
            }
            DiffLine::LeftOnly(l) => {
                old_line_no += 1;

                let del_style = Style::default()
                    .fg(Color::LightRed)
                    .bg(Color::Rgb(60, 20, 20));

                // Left gutter.
                let left_gutter = format!("{:>4} ", old_line_no);
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        left_gutter,
                        Style::default().fg(Color::DarkGray).bg(Color::Red),
                    )),
                    Rect::new(left_inner.x, y, gutter_width, 1),
                );
                // Left content with syntax highlighting + red background.
                let left_spans = build_highlighted_spans_with_bg(
                    l,
                    old_hl.get(old_hl_idx),
                    left_content_w,
                    del_style,
                );
                frame.render_widget(
                    Paragraph::new(ratatui::text::Line::from(left_spans)).style(del_style),
                    Rect::new(
                        left_content_x,
                        y,
                        left_inner.width.saturating_sub(gutter_width),
                        1,
                    ),
                );

                // Right side: empty.
                let empty = " ".repeat(right_inner.width as usize);
                frame.render_widget(
                    Paragraph::new(Span::styled(empty, Style::default().fg(Color::DarkGray))),
                    Rect::new(right_inner.x, y, right_inner.width, 1),
                );

                old_hl_idx += 1;
            }
            DiffLine::RightOnly(r) => {
                new_line_no += 1;

                let add_style = Style::default()
                    .fg(Color::LightGreen)
                    .bg(Color::Rgb(20, 50, 20));

                // Left side: empty.
                let empty = " ".repeat(left_inner.width as usize);
                frame.render_widget(
                    Paragraph::new(Span::styled(empty, Style::default().fg(Color::DarkGray))),
                    Rect::new(left_inner.x, y, left_inner.width, 1),
                );

                // Right gutter.
                let right_gutter = format!("{:>4} ", new_line_no);
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        right_gutter,
                        Style::default().fg(Color::DarkGray).bg(Color::Green),
                    )),
                    Rect::new(right_inner.x, y, gutter_width, 1),
                );
                // Right content with syntax highlighting + green background.
                let right_spans = build_highlighted_spans_with_bg(
                    r,
                    new_hl.get(new_hl_idx),
                    right_content_w,
                    add_style,
                );
                frame.render_widget(
                    Paragraph::new(ratatui::text::Line::from(right_spans)).style(add_style),
                    Rect::new(
                        right_content_x,
                        y,
                        right_inner.width.saturating_sub(gutter_width),
                        1,
                    ),
                );

                new_hl_idx += 1;
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
fn draw_tab_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.height == 0 {
        return;
    }

    let active_idx = app.tabs.active_index();
    let tabs = app.tabs.tabs();
    let mut spans: Vec<Span> = Vec::new();
    let max_width = area.width as usize;
    let mut used_width = 0;
    let mut close_btn_ranges: Vec<(usize, u16, u16)> = Vec::new();

    for (i, tab) in tabs.iter().enumerate() {
        let is_active = i == active_idx;
        let label = if i < 9 {
            format!(" {}:{} ", i + 1, tab.title())
        } else {
            format!(" {} ", tab.title())
        };
        // Close button: " × "
        let close_btn = "\u{00d7} ";
        let label_len = label.len();
        let close_len = close_btn.len();
        let total_len = label_len + close_len;

        if used_width + total_len + 1 > max_width {
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

        // Close button with distinct styling.
        let close_x_start = area.x + used_width as u16 + label_len as u16;
        let close_x_end = close_x_start + close_len as u16;
        close_btn_ranges.push((i, close_x_start, close_x_end));
        let close_style = if is_active {
            Style::default().fg(Color::Red).bg(Color::DarkGray)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(close_btn, close_style));

        // Separator between tabs.
        if i + 1 < tabs.len() {
            spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
            used_width += 1;
        }
        used_width += total_len;
    }

    app.tab_close_btn_ranges = close_btn_ranges;

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

/// Draw the debug panel (call stack, variables, output).
fn draw_debug_panel(frame: &mut Frame, app: &App, area: Rect) {
    use crate::debug_panel::{DebugTab, SessionStatus};

    let focused = app.debug_panel_focused;
    let border_color = if focused {
        Color::Yellow
    } else {
        Color::DarkGray
    };

    // Status indicator.
    let status_str = match &app.debug_panel.state.status {
        SessionStatus::Inactive => "Inactive",
        SessionStatus::Running => "Running",
        SessionStatus::Stopped(reason) => reason.as_str(),
        SessionStatus::Terminated => "Terminated",
    };

    let title = format!(" Debug — {} ", status_str);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 2 || inner.width < 10 {
        return;
    }

    // Tab bar row.
    let tab_row_area = Rect::new(inner.x, inner.y, inner.width, 1);
    let content_area = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1),
    );

    // Draw tab selector.
    let tabs = [
        ("1:Stack", DebugTab::CallStack),
        ("2:Vars", DebugTab::Variables),
        ("3:Output", DebugTab::Output),
    ];
    let mut tab_spans = Vec::new();
    for (label, tab) in &tabs {
        let style = if *tab == app.debug_panel.active_tab {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        tab_spans.push(Span::styled(format!(" {} ", label), style));
        tab_spans.push(Span::raw(" "));
    }

    // Add control hints.
    tab_spans.push(Span::styled(
        " F5:Continue  F10:Step  F11:In  Shift+F5:Stop ",
        Style::default().fg(Color::DarkGray),
    ));

    frame.render_widget(Paragraph::new(Line::from(tab_spans)), tab_row_area);

    // Draw content based on active tab.
    match app.debug_panel.active_tab {
        DebugTab::CallStack => {
            let frames = &app.debug_panel.state.stack_frames;
            for (i, sf) in frames.iter().enumerate() {
                if i as u16 >= content_area.height {
                    break;
                }
                let row_y = content_area.y + i as u16;
                let is_selected = i == app.debug_panel.state.selected_frame;
                let style = if is_selected {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                };
                let source = sf
                    .source_path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .and_then(|f| f.to_str())
                    .unwrap_or("??");
                let text = format!("#{:<2} {}  {}:{}", i, sf.name, source, sf.line);
                let text = if text.len() > content_area.width as usize {
                    text[..content_area.width as usize].to_string()
                } else {
                    text
                };
                frame.render_widget(
                    Paragraph::new(text).style(style),
                    Rect::new(content_area.x, row_y, content_area.width, 1),
                );
            }
            if frames.is_empty() {
                frame.render_widget(
                    Paragraph::new("  No stack frames").style(Style::default().fg(Color::DarkGray)),
                    Rect::new(content_area.x, content_area.y, content_area.width, 1),
                );
            }
        }
        DebugTab::Variables => {
            let vars = &app.debug_panel.state.variables;
            for (i, var) in vars.iter().enumerate() {
                if i as u16 >= content_area.height {
                    break;
                }
                let row_y = content_area.y + i as u16;
                let is_selected = i == app.debug_panel.state.selected_var;
                let style = if is_selected {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                };
                let indent = "  ".repeat(var.indent);
                let prefix = if var.expandable {
                    if var.expanded {
                        "▼ "
                    } else {
                        "▶ "
                    }
                } else {
                    "  "
                };
                let type_hint = if var.type_name.is_empty() {
                    String::new()
                } else {
                    format!(": {} ", var.type_name)
                };
                let text = format!(
                    "{}{}{} {}= {}",
                    indent, prefix, var.name, type_hint, var.value
                );
                let text = if text.len() > content_area.width as usize {
                    text[..content_area.width as usize].to_string()
                } else {
                    text
                };
                frame.render_widget(
                    Paragraph::new(text).style(style),
                    Rect::new(content_area.x, row_y, content_area.width, 1),
                );
            }
            if vars.is_empty() {
                frame.render_widget(
                    Paragraph::new("  No variables").style(Style::default().fg(Color::DarkGray)),
                    Rect::new(content_area.x, content_area.y, content_area.width, 1),
                );
            }
        }
        DebugTab::Output => {
            let lines = &app.debug_panel.state.output_lines;
            let scroll = app.debug_panel.state.output_scroll;
            let visible = content_area.height as usize;
            // Show the last `visible` lines if scroll is at the end.
            let start = if lines.len() > visible + scroll {
                lines.len() - visible - scroll
            } else {
                0
            };
            for (i, line) in lines.iter().skip(start).take(visible).enumerate() {
                let row_y = content_area.y + i as u16;
                let text = if line.len() > content_area.width as usize {
                    &line[..content_area.width as usize]
                } else {
                    line.as_str()
                };
                frame.render_widget(
                    Paragraph::new(text).style(Style::default().fg(Color::White)),
                    Rect::new(content_area.x, row_y, content_area.width, 1),
                );
            }
            if lines.is_empty() {
                frame.render_widget(
                    Paragraph::new("  No output").style(Style::default().fg(Color::DarkGray)),
                    Rect::new(content_area.x, content_area.y, content_area.width, 1),
                );
            }
        }
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
    let tree_inner = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1),
    );

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
            let style = if is_selected {
                selected_style
            } else {
                dir_style
            };
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
    let panel = &app.conversation_history;
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

    let max_width = inner.width as usize;
    let mut y = inner.y;
    let end_y = inner.y + inner.height;

    // Search bar (if active).
    if panel.search_active {
        let query_line = format!("/{}", panel.search_query);
        frame.render_widget(
            Paragraph::new(Span::styled(
                query_line,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Rect::new(inner.x, y, inner.width, 1),
        );
        y += 1;
    }

    if panel.conversations.is_empty() {
        let msg = Paragraph::new("No conversations").style(Style::default().fg(Color::DarkGray));
        frame.render_widget(msg, Rect::new(inner.x, y, inner.width, 1));
        return;
    }

    // Render grouped by branch.
    let groups = panel.grouped_by_branch();
    let mut flat_idx = 0usize; // Index into the flat visible list.

    for (branch, entry_indices) in &groups {
        if y >= end_y {
            break;
        }

        // Branch header.
        let header: String = format!(" {branch} ({}) ", entry_indices.len())
            .chars()
            .take(max_width)
            .collect();
        frame.render_widget(
            Paragraph::new(Span::styled(
                header,
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            )),
            Rect::new(inner.x, y, inner.width, 1),
        );
        y += 1;

        // Entries in this branch group.
        for &entry_idx in entry_indices {
            if y >= end_y {
                break;
            }

            let entry = &panel.conversations[entry_idx];
            let is_selected = flat_idx == panel.selected;
            let is_expanded = panel.expanded == Some(entry_idx);
            flat_idx += 1;

            // Title line: intent > summary > file basename + badges.
            let title = entry.display_title();
            let badge = format!(" {}m", entry.message_count);
            let acceptance = entry
                .acceptance_badge()
                .map(|b| format!(" [{b}]"))
                .unwrap_or_default();
            let avail = max_width.saturating_sub(badge.len() + acceptance.len() + 2);
            let truncated: String = title.chars().take(avail).collect();

            let style = if is_selected && focused {
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else if is_selected {
                Style::default().fg(Color::Black).bg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            };

            let prefix = if is_expanded { "v " } else { "> " };
            let line = ratatui::text::Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(truncated, style),
                Span::styled(
                    badge,
                    if is_selected {
                        style
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
                Span::styled(
                    acceptance,
                    if is_selected {
                        style
                    } else if entry.accepted > entry.rejected {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::Red)
                    },
                ),
            ]);
            frame.render_widget(Paragraph::new(line), Rect::new(inner.x, y, inner.width, 1));
            y += 1;

            // Relative timestamp.
            if y < end_y {
                let time_str = entry.relative_time();
                frame.render_widget(
                    Paragraph::new(format!("  {time_str}"))
                        .style(Style::default().fg(Color::DarkGray)),
                    Rect::new(inner.x, y, inner.width, 1),
                );
                y += 1;
            }

            // Expanded messages.
            if is_expanded {
                let msgs = &panel.expanded_messages;
                for msg in msgs.iter().skip(panel.message_scroll) {
                    if y >= end_y {
                        break;
                    }
                    let role_color = match msg.role {
                        MessageRole::HumanIntent => Color::Green,
                        MessageRole::AiResponse => Color::Cyan,
                        MessageRole::System => Color::DarkGray,
                    };
                    let role_label = match msg.role {
                        MessageRole::HumanIntent => "  You: ",
                        MessageRole::AiResponse => "  AI: ",
                        MessageRole::System => "  Sys: ",
                    };
                    let content_avail = max_width.saturating_sub(role_label.len());
                    let content: String = msg
                        .content
                        .replace('\n', " ")
                        .chars()
                        .take(content_avail)
                        .collect();
                    let display: String = format!("{role_label}{content}")
                        .chars()
                        .take(max_width)
                        .collect();

                    frame.render_widget(
                        Paragraph::new(display).style(Style::default().fg(role_color)),
                        Rect::new(inner.x, y, inner.width, 1),
                    );
                    y += 1;
                }
                if y < end_y {
                    frame.render_widget(
                        Paragraph::new("─".repeat(max_width))
                            .style(Style::default().fg(Color::DarkGray)),
                        Rect::new(inner.x, y, inner.width, 1),
                    );
                    y += 1;
                }
            }
        }
    }
}

/// Draw the interactive chat panel.
fn draw_chat_panel(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.chat_panel_focused;
    let panel = &app.chat_panel;

    let (title, border_color) = if panel.pending_approval.is_some() {
        (" Chat [approve? Y/N] ", Color::Yellow)
    } else if focused {
        if panel.streaming {
            (" Chat [streaming...] ", Color::Cyan)
        } else if panel.in_tool_loop {
            (" Chat [tools] ", Color::Magenta)
        } else {
            (" Chat [focused] ", Color::Yellow)
        }
    } else if panel.streaming {
        (" Chat [streaming...] ", Color::Cyan)
    } else {
        (" Chat ", Color::DarkGray)
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

    // Vertical layout: messages area + input area (dynamic height).
    // Calculate input height based on content: at least 3 rows, grows up to half the panel.
    let input_inner_width = inner.width.saturating_sub(2) as usize; // account for borders
    let input_lines = if input_inner_width > 0 && !panel.input.is_empty() {
        let char_count = panel.input.chars().count();
        char_count.div_ceil(input_inner_width).max(1) as u16
    } else {
        1u16
    };
    // Selection context badge takes 1 row when present.
    let has_selection_ctx = panel.selection_context.is_some();
    let selection_ctx_height: u16 = if has_selection_ctx { 1 } else { 0 };

    // 2 for borders + number of content lines, minimum 3, max half of panel
    let max_input_height = (inner.height / 2).max(3);
    let input_height = (input_lines + 2).clamp(3, max_input_height);
    let bottom_height = input_height + selection_ctx_height;
    let msg_height = inner.height.saturating_sub(bottom_height);
    let msg_area = Rect::new(inner.x, inner.y, inner.width, msg_height);
    let selection_ctx_area = Rect::new(
        inner.x,
        inner.y + msg_height,
        inner.width,
        selection_ctx_height,
    );
    let input_area = Rect::new(
        inner.x,
        inner.y + msg_height + selection_ctx_height,
        inner.width,
        input_height,
    );

    // Draw selection context badge if present.
    if let Some(ctx) = &panel.selection_context {
        let badge = ratatui::text::Line::from(vec![
            Span::styled(" @ ", Style::default().fg(Color::Black).bg(Color::Cyan)),
            Span::styled(format!(" {ctx}"), Style::default().fg(Color::Cyan)),
        ]);
        frame.render_widget(Paragraph::new(badge), selection_ctx_area);
    }

    // ── Render messages ──
    let max_width = msg_area.width as usize;
    if max_width == 0 || msg_area.height == 0 {
        draw_chat_input(frame, app, input_area);
        return;
    }

    // Build wrapped lines from all items + streaming text.
    let mut wrapped_lines: Vec<(ChatRole, String, Option<Color>)> = Vec::new();
    for item in &panel.items {
        match item {
            ChatItem::Text { role, content, .. } => {
                let prefix = match role {
                    ChatRole::User => "You: ",
                    ChatRole::Assistant => "AI: ",
                    ChatRole::System => "Sys: ",
                };
                let full = format!("{prefix}{content}");
                for wl in wrap_text(&full, max_width) {
                    wrapped_lines.push((*role, wl, None));
                }
                wrapped_lines.push((ChatRole::System, String::new(), None));
            }
            ChatItem::ToolCall {
                name,
                input,
                status,
                result,
                ..
            } => {
                // Tool call header.
                let (status_icon, status_color) = match status {
                    ToolCallStatus::PendingApproval => ("⏳", Color::Yellow),
                    ToolCallStatus::Running => ("⟳", Color::Blue),
                    ToolCallStatus::Completed => ("✓", Color::Green),
                    ToolCallStatus::Denied => ("✗", Color::Red),
                    ToolCallStatus::Failed(_) => ("✗", Color::Red),
                };
                let header = format!(" {status_icon} Tool: {name}");
                for wl in wrap_text(&header, max_width) {
                    wrapped_lines.push((ChatRole::System, wl, Some(status_color)));
                }

                // Show compact input summary.
                let input_summary = format_tool_input(name, input);
                if !input_summary.is_empty() {
                    for wl in wrap_text(&format!("   {input_summary}"), max_width) {
                        wrapped_lines.push((ChatRole::System, wl, Some(Color::DarkGray)));
                    }
                }

                // Show approval prompt if pending.
                if *status == ToolCallStatus::PendingApproval {
                    let prompt = "   Allow? [Y]es / [N]o".to_string();
                    wrapped_lines.push((ChatRole::System, prompt, Some(Color::Yellow)));
                }

                // Show result summary if available.
                if let Some(res) = result {
                    let lines: Vec<&str> = res.lines().take(3).collect();
                    for line in &lines {
                        for wl in wrap_text(&format!("   {line}"), max_width) {
                            wrapped_lines.push((ChatRole::System, wl, Some(Color::DarkGray)));
                        }
                    }
                    let total = res.lines().count();
                    if total > 3 {
                        wrapped_lines.push((
                            ChatRole::System,
                            format!("   ... ({} more lines)", total - 3),
                            Some(Color::DarkGray),
                        ));
                    }
                }

                wrapped_lines.push((ChatRole::System, String::new(), None));
            }
        }
    }

    // Add streaming text if active.
    if panel.streaming && !panel.streaming_text.is_empty() {
        let full = format!("AI: {}", panel.streaming_text);
        for wl in wrap_text(&full, max_width) {
            wrapped_lines.push((ChatRole::Assistant, wl, None));
        }
        // Blinking cursor indicator.
        wrapped_lines.push((ChatRole::Assistant, "▌".to_string(), None));
    } else if panel.streaming {
        wrapped_lines.push((ChatRole::Assistant, "AI: ...".to_string(), None));
    }

    let visible_height = msg_area.height as usize;
    let total_lines = wrapped_lines.len();

    // Clamp scroll so we don't go past the end.
    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll = panel.scroll.min(max_scroll);

    let visible = wrapped_lines.iter().skip(scroll).take(visible_height);

    for (i, (role, text, override_color)) in visible.enumerate() {
        let y = msg_area.y + i as u16;
        let color = if let Some(c) = override_color {
            *c
        } else {
            match role {
                ChatRole::User => Color::Green,
                ChatRole::Assistant => Color::Cyan,
                ChatRole::System => Color::Red,
            }
        };
        let style = if text.is_empty() {
            Style::default()
        } else {
            Style::default().fg(color)
        };
        let display: String = text.chars().take(max_width).collect();
        frame.render_widget(
            Paragraph::new(display).style(style),
            Rect::new(msg_area.x, y, msg_area.width, 1),
        );
    }

    // ── Render input ──
    draw_chat_input(frame, app, input_area);
}

/// Draw the chat input box.
fn draw_chat_input(frame: &mut Frame, app: &App, area: Rect) {
    let panel = &app.chat_panel;
    let focused = app.chat_panel_focused;

    let input_block = Block::default()
        .borders(Borders::ALL)
        .title(if panel.streaming { " ... " } else { " > " })
        .border_style(Style::default().fg(if focused {
            Color::Yellow
        } else {
            Color::DarkGray
        }));
    let input_inner = input_block.inner(area);
    frame.render_widget(input_block, area);

    if input_inner.width == 0 || input_inner.height == 0 {
        return;
    }

    let w = input_inner.width as usize;

    if panel.input.is_empty() {
        // Placeholder or empty.
        let (text, style) = if !focused {
            (
                "Ctrl+J to chat...".to_string(),
                Style::default().fg(Color::DarkGray),
            )
        } else {
            (String::new(), Style::default().fg(Color::White))
        };
        frame.render_widget(
            Paragraph::new(text).style(style),
            Rect::new(input_inner.x, input_inner.y, input_inner.width, 1),
        );
    } else if w > 0 {
        // Wrap the input text into visual lines.
        let chars: Vec<char> = panel.input.chars().collect();
        let mut start = 0;
        let mut row = 0u16;
        while start < chars.len() && row < input_inner.height {
            let end = (start + w).min(chars.len());
            let line: String = chars[start..end].iter().collect();
            frame.render_widget(
                Paragraph::new(line).style(Style::default().fg(Color::White)),
                Rect::new(input_inner.x, input_inner.y + row, input_inner.width, 1),
            );
            start = end;
            row += 1;
        }
    }

    // Show blinking cursor in the input box when focused.
    if focused && !panel.streaming {
        let cursor_char_pos = panel.input_cursor;
        let (cursor_row, cursor_col) = if w > 0 {
            ((cursor_char_pos / w) as u16, (cursor_char_pos % w) as u16)
        } else {
            (0u16, 0u16)
        };
        let cursor_y =
            (input_inner.y + cursor_row).min(input_inner.y + input_inner.height.saturating_sub(1));
        let cursor_x =
            (input_inner.x + cursor_col).min(input_inner.x + input_inner.width.saturating_sub(1));
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

/// Word-wrap text to the given width.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    for raw_line in text.split('\n') {
        if raw_line.is_empty() {
            lines.push(String::new());
            continue;
        }
        let chars: Vec<char> = raw_line.chars().collect();
        let mut start = 0;
        while start < chars.len() {
            let end = (start + width).min(chars.len());
            lines.push(chars[start..end].iter().collect());
            start = end;
        }
    }
    lines
}

/// Format tool input parameters for display in a compact way.
fn format_tool_input(name: &str, input: &serde_json::Value) -> String {
    match name {
        "read_file" | "edit_file" => {
            let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            if name == "edit_file" {
                let old_len = input
                    .get("old_text")
                    .and_then(|v| v.as_str())
                    .map(|s| s.len())
                    .unwrap_or(0);
                let new_len = input
                    .get("new_text")
                    .and_then(|v| v.as_str())
                    .map(|s| s.len())
                    .unwrap_or(0);
                format!("{path} ({old_len}→{new_len} chars)")
            } else {
                path.to_string()
            }
        }
        "list_files" => {
            let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            let recursive = input
                .get("recursive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if recursive {
                format!("{path} (recursive)")
            } else {
                path.to_string()
            }
        }
        "search_files" => {
            let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");
            let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            format!("\"{pattern}\" in {path}")
        }
        "run_command" => {
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("?");
            if cmd.len() > 60 {
                format!("{}...", &cmd[..60])
            } else {
                cmd.to_string()
            }
        }
        _ => {
            let s = serde_json::to_string(input).unwrap_or_default();
            if s.len() > 80 {
                format!("{}...", &s[..80])
            } else {
                s
            }
        }
    }
}

fn draw_source_control(frame: &mut Frame, app: &mut App, area: Rect) {
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
            Span::styled(
                branch_name,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
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
        let is_generating = app.is_generating_commit_msg();
        let label = if is_generating {
            " Commit Message (AI...)"
        } else if sc.editing_commit_message {
            " Commit Message (editing)"
        } else {
            " Commit Message"
        };

        // Show AI button when there are staged files and AI is available.
        let has_staged = !sc.staged.is_empty();
        let has_ai = app.has_ai();
        if has_staged && has_ai && !is_generating {
            let label_len = label.len() as u16;
            let btn_text = " \u{2728} ";
            let btn_x = inner.x + inner.width.saturating_sub(btn_text.len() as u16 + 1);
            app.ai_commit_btn_rect = Rect::new(btn_x, y, btn_text.len() as u16, 1);
            let btn_style = Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD);
            let line = ratatui::text::Line::from(vec![
                Span::styled(label, header_style),
                Span::styled(
                    " ".repeat(
                        (inner
                            .width
                            .saturating_sub(label_len + btn_text.len() as u16))
                            as usize,
                    ),
                    Style::default(),
                ),
                Span::styled(btn_text, btn_style),
            ]);
            frame.render_widget(Paragraph::new(line), Rect::new(inner.x, y, inner.width, 1));
        } else {
            app.ai_commit_btn_rect = Rect::default();
            frame.render_widget(
                Paragraph::new(Span::styled(label, header_style)),
                Rect::new(inner.x, y, inner.width, 1),
            );
        }
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
        let (filename, dir) = split_path_filename(&entry.rel_path);
        let line = format_git_entry(entry.status.label(), &filename, &dir, inner.width as usize);

        if is_selected {
            let style = Style::default().add_modifier(Modifier::REVERSED);
            frame.render_widget(
                Paragraph::new(line).style(style),
                Rect::new(inner.x, y, inner.width, 1),
            );
        } else {
            frame.render_widget(Paragraph::new(line), Rect::new(inner.x, y, inner.width, 1));
            let _ = status_style; // used for non-selected coloring via spans
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

        // Show "+" button when there are unstaged changes.
        if sc.changed.is_empty() {
            app.stage_all_btn_rect = Rect::default();
            frame.render_widget(
                Paragraph::new(Span::styled(header, header_style)),
                Rect::new(inner.x, y, inner.width, 1),
            );
        } else {
            let header_len = header.len() as u16;
            let btn_text = " + ";
            let btn_x = inner.x + inner.width.saturating_sub(btn_text.len() as u16 + 1);
            app.stage_all_btn_rect = Rect::new(btn_x, y, btn_text.len() as u16, 1);
            let btn_style = Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD);

            let line = ratatui::text::Line::from(vec![
                Span::styled(header, header_style),
                Span::styled(
                    " ".repeat(
                        (inner
                            .width
                            .saturating_sub(header_len + btn_text.len() as u16))
                            as usize,
                    ),
                    Style::default(),
                ),
                Span::styled(btn_text, btn_style),
            ]);
            frame.render_widget(Paragraph::new(line), Rect::new(inner.x, y, inner.width, 1));
        }
        y += 1;
    }

    for (i, entry) in sc.changed.iter().enumerate() {
        if y >= max_y {
            break;
        }
        let is_selected = changed_focused && i == sc.selected;
        let status_style = status_color(entry.status);
        let (filename, dir) = split_path_filename(&entry.rel_path);
        let line = format_git_entry(entry.status.label(), &filename, &dir, inner.width as usize);

        if is_selected {
            let style = Style::default().add_modifier(Modifier::REVERSED);
            frame.render_widget(
                Paragraph::new(line).style(style),
                Rect::new(inner.x, y, inner.width, 1),
            );
        } else {
            frame.render_widget(Paragraph::new(line), Rect::new(inner.x, y, inner.width, 1));
            let _ = status_style;
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
        GitFileStatus::Conflict => Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    }
}

/// Build syntax-highlighted spans for a diff line.
fn build_highlighted_spans<'a>(
    text: &str,
    hl_line: Option<&crate::highlight::HighlightedLine>,
    max_width: usize,
) -> Vec<Span<'a>> {
    let chars: Vec<char> = text.chars().take(max_width).collect();
    if chars.is_empty() {
        return vec![Span::raw("")];
    }
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut current_color = Color::Reset;

    for (i, &ch) in chars.iter().enumerate() {
        let color = hl_line
            .and_then(|hl| hl.colors.get(i).copied())
            .unwrap_or(Color::Reset);
        if color != current_color && !current.is_empty() {
            let style = if current_color == Color::Reset {
                Style::default()
            } else {
                Style::default().fg(current_color)
            };
            spans.push(Span::styled(std::mem::take(&mut current), style));
        }
        current_color = color;
        current.push(ch);
    }
    if !current.is_empty() {
        let style = if current_color == Color::Reset {
            Style::default()
        } else {
            Style::default().fg(current_color)
        };
        spans.push(Span::styled(current, style));
    }
    spans
}

/// Build syntax-highlighted spans with a background color overlay (for diff additions/deletions).
fn build_highlighted_spans_with_bg<'a>(
    text: &str,
    hl_line: Option<&crate::highlight::HighlightedLine>,
    max_width: usize,
    bg_style: Style,
) -> Vec<Span<'a>> {
    let chars: Vec<char> = text.chars().take(max_width).collect();
    if chars.is_empty() {
        return vec![Span::styled(" ".repeat(max_width), bg_style)];
    }
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut current_color = Color::Reset;

    for (i, &ch) in chars.iter().enumerate() {
        let color = hl_line
            .and_then(|hl| hl.colors.get(i).copied())
            .unwrap_or(Color::Reset);
        if color != current_color && !current.is_empty() {
            let fg = if current_color == Color::Reset {
                bg_style.fg.unwrap_or(Color::White)
            } else {
                current_color
            };
            spans.push(Span::styled(std::mem::take(&mut current), bg_style.fg(fg)));
        }
        current_color = color;
        current.push(ch);
    }
    if !current.is_empty() {
        let fg = if current_color == Color::Reset {
            bg_style.fg.unwrap_or(Color::White)
        } else {
            current_color
        };
        spans.push(Span::styled(current, bg_style.fg(fg)));
    }
    // Pad to fill width.
    let rendered_len: usize = chars.len();
    if rendered_len < max_width {
        spans.push(Span::styled(" ".repeat(max_width - rendered_len), bg_style));
    }
    spans
}

/// Split a path into (filename, directory). Like Cursor's git panel format.
fn split_path_filename(path: &str) -> (String, String) {
    if let Some(pos) = path.rfind('/') {
        let filename = path[pos + 1..].to_string();
        let dir = path[..pos].to_string();
        (filename, dir)
    } else {
        (path.to_string(), String::new())
    }
}

/// Format a git entry line: "  M filename  dir/path"
/// Filename is bright, directory is dimmed gray — like Cursor/VS Code.
fn format_git_entry<'a>(
    status_label: &str,
    filename: &str,
    dir: &str,
    max_width: usize,
) -> ratatui::text::Line<'a> {
    let status_color = match status_label {
        "M" => Color::Yellow,
        "A" => Color::Green,
        "D" => Color::Red,
        "R" => Color::Blue,
        "?" => Color::DarkGray,
        _ => Color::White,
    };

    let prefix = format!("  {status_label} ");
    let dir_display = if dir.is_empty() {
        String::new()
    } else {
        let available = max_width.saturating_sub(prefix.len() + filename.len() + 2);
        if dir.len() <= available {
            format!("  {dir}")
        } else if available > 4 {
            format!("  {}...", &dir[..available.saturating_sub(3)])
        } else {
            String::new()
        }
    };

    ratatui::text::Line::from(vec![
        Span::styled(prefix, Style::default().fg(status_color)),
        Span::styled(
            filename.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(dir_display, Style::default().fg(Color::DarkGray)),
    ])
}

/// Return a Nerd Font icon (+ trailing space) for a file based on its extension.
fn file_icon(name: &str) -> &'static str {
    let ext = match name.rsplit_once('.') {
        Some((_, e)) => e,
        None => "",
    };
    match ext {
        // Rust
        "rs" => "\u{e7a8} ", //
        // JavaScript / TypeScript
        "js" | "mjs" | "cjs" => "\u{e781} ", //
        "ts" | "mts" | "cts" => "\u{e628} ", //
        "jsx" => "\u{e7ba} ",                //
        "tsx" => "\u{e7ba} ",                //
        // Web
        "html" | "htm" => "\u{e736} ", //
        "css" => "\u{e749} ",          //
        "scss" | "sass" => "\u{e749} ",
        // Data / Config
        "json" => "\u{e60b} ",         //
        "yaml" | "yml" => "\u{e6a8} ", //
        "toml" => "\u{e6b2} ",         //
        "xml" => "\u{e619} ",          //
        // Elixir / Erlang
        "ex" | "exs" => "\u{e62d} ",  //
        "erl" | "hrl" => "\u{e7b1} ", //
        // Python
        "py" | "pyi" => "\u{e73c} ", //
        // Go
        "go" => "\u{e626} ", //
        // C / C++
        "c" | "h" => "\u{e61e} ",                    //
        "cpp" | "cxx" | "cc" | "hpp" => "\u{e61d} ", //
        // Shell
        "sh" | "bash" | "zsh" | "fish" => "\u{e795} ", //
        // Ruby
        "rb" => "\u{e791} ", //
        // Java / Kotlin
        "java" => "\u{e738} ",       //
        "kt" | "kts" => "\u{e634} ", //
        // Markdown / Text
        "md" | "mdx" => "\u{e73e} ", //
        "txt" => "\u{f0f6} ",        //
        // Docker
        "dockerfile" => "\u{e7b0} ", //
        // Git
        "gitignore" | "gitmodules" | "gitattributes" => "\u{e702} ", //
        // Images
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" | "webp" => "\u{f1c5} ", //
        // Lock files
        "lock" => "\u{f023} ", //
        // Catch-all
        _ => match name {
            "Dockerfile" => "\u{e7b0} ",
            "Makefile" | "CMakeLists.txt" => "\u{e779} ",
            _ => "\u{f15b} ", //  generic file
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
        "rs" => Color::Rgb(222, 165, 132),                  // Rust orange
        "js" | "mjs" | "cjs" => Color::Yellow,              // JS yellow
        "ts" | "mts" | "cts" => Color::Rgb(49, 120, 198),   // TS blue
        "jsx" | "tsx" => Color::Rgb(97, 218, 251),          // React cyan
        "html" | "htm" => Color::Rgb(227, 76, 38),          // HTML orange
        "css" | "scss" | "sass" => Color::Rgb(86, 61, 124), // CSS purple
        "json" => Color::Yellow,
        "yaml" | "yml" => Color::Rgb(203, 23, 30), // Red
        "toml" => Color::Rgb(156, 154, 150),       // Gray
        "xml" => Color::Rgb(227, 76, 38),
        "ex" | "exs" => Color::Rgb(110, 74, 126), // Elixir purple
        "erl" | "hrl" => Color::Rgb(169, 36, 52), // Erlang red
        "py" | "pyi" => Color::Rgb(55, 118, 171), // Python blue
        "go" => Color::Rgb(0, 173, 216),          // Go cyan
        "c" | "h" => Color::Rgb(85, 85, 255),     // C blue
        "cpp" | "cxx" | "cc" | "hpp" => Color::Rgb(0, 89, 156),
        "sh" | "bash" | "zsh" | "fish" => Color::Green,
        "rb" => Color::Rgb(204, 52, 45),           // Ruby red
        "java" => Color::Rgb(176, 114, 25),        // Java orange
        "kt" | "kts" => Color::Rgb(169, 123, 255), // Kotlin purple
        "md" | "mdx" => Color::Rgb(66, 165, 245),  // Markdown blue
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

/// Draw the conversation detail modal (near full-screen popup with word-wrapped messages).
fn draw_conversation_detail(
    frame: &mut Frame,
    panel: &crate::conversation_history::ConversationHistoryPanel,
    area: Rect,
) {
    // Get the expanded conversation and messages.
    let conv_idx = match panel.expanded {
        Some(idx) => idx,
        None => return,
    };
    let entry = match panel.conversations.get(conv_idx) {
        Some(e) => e,
        None => return,
    };
    let messages = &panel.expanded_messages;

    // Full-screen popup with margin.
    let margin = 2u16;
    let width = area.width.saturating_sub(margin * 2);
    let height = area.height.saturating_sub(margin * 2);
    let x = area.x + margin;
    let y = area.y + margin;
    let rect = Rect::new(x, y, width, height);

    frame.render_widget(Clear, rect);
    let title = format!(" {} ", entry.display_title());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(title)
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let content_width = inner.width as usize;

    // Build wrapped lines from all messages.
    let mut lines: Vec<(Color, String)> = Vec::new();

    // Header info.
    lines.push((
        Color::DarkGray,
        format!(
            "File: {}  |  Branch: {}  |  {}",
            entry.file_path,
            entry.branch_name(),
            entry.relative_time()
        ),
    ));
    if let Some(badge) = entry.acceptance_badge() {
        lines.push((Color::DarkGray, format!("Decisions: {badge} accepted")));
    }
    lines.push((Color::DarkGray, "─".repeat(content_width)));

    // Messages with word wrapping.
    for msg in messages {
        let (role_color, role_label) = match msg.role {
            MessageRole::HumanIntent => (Color::Green, "You"),
            MessageRole::AiResponse => (Color::Cyan, "AI"),
            MessageRole::System => (Color::DarkGray, "System"),
        };

        lines.push((role_color, format!("{role_label}:")));

        // Word-wrap the content.
        let indent = "  ";
        let _wrap_width = content_width.saturating_sub(indent.len());
        for paragraph in msg.content.split('\n') {
            if paragraph.is_empty() {
                lines.push((role_color, String::new()));
                continue;
            }
            let mut current_line = String::from(indent);
            for word in paragraph.split_whitespace() {
                if current_line.len() + word.len() + 1 > content_width
                    && current_line.len() > indent.len()
                {
                    lines.push((role_color, current_line));
                    current_line = String::from(indent);
                }
                if current_line.len() > indent.len() {
                    current_line.push(' ');
                }
                current_line.push_str(word);
            }
            if !current_line.trim().is_empty() {
                lines.push((role_color, current_line));
            }
        }
        lines.push((Color::DarkGray, String::new())); // Blank line between messages.
    }

    // Render with scroll.
    let visible_height = inner.height as usize;
    let max_scroll = lines.len().saturating_sub(visible_height);
    let scroll = panel.detail_scroll.min(max_scroll);

    for (i, (color, text)) in lines.iter().enumerate().skip(scroll).take(visible_height) {
        let row_y = inner.y + (i - scroll) as u16;
        let display: String = text.chars().take(content_width).collect();
        frame.render_widget(
            Paragraph::new(display).style(Style::default().fg(*color)),
            Rect::new(inner.x, row_y, inner.width, 1),
        );
    }

    // Footer hint.
    let footer_y = rect.y + rect.height.saturating_sub(1);
    let hint = " j/k:scroll  d/u:page  Esc:close ";
    frame.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray))),
        Rect::new(rect.x + 1, footer_y, rect.width.saturating_sub(2), 1),
    );
}

/// Draw the git graph modal (near full-screen with commit list + detail panel).
fn draw_git_graph_modal(frame: &mut Frame, modal: &crate::git_graph::GitGraphModal, area: Rect) {
    let margin = 1u16;
    let width = area.width.saturating_sub(margin * 2);
    let height = area.height.saturating_sub(margin * 2);
    let x = area.x + margin;
    let y = area.y + margin;
    let rect = Rect::new(x, y, width, height);

    frame.render_widget(Clear, rect);

    // Split: left (commit list) + right (detail).
    let (list_area, detail_area) = if modal.show_detail && width > 60 {
        let hsplit = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(rect);
        (hsplit[0], Some(hsplit[1]))
    } else {
        (rect, None)
    };

    // Left panel: commit graph + list.
    let left_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Git Graph ")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    let left_inner = left_block.inner(list_area);
    frame.render_widget(left_block, list_area);

    if left_inner.height == 0 || modal.commits.is_empty() {
        return;
    }

    let graph_colors: [Color; 6] = [
        Color::Green,
        Color::Cyan,
        Color::Magenta,
        Color::Yellow,
        Color::Blue,
        Color::Red,
    ];

    let visible_h = left_inner.height as usize;
    let selected = modal.selected;
    let scroll = if selected >= visible_h {
        selected - visible_h + 1
    } else {
        0
    };

    for (vis_idx, commit_idx) in (scroll..).take(visible_h).enumerate() {
        if commit_idx >= modal.commits.len() {
            break;
        }
        let row_y = left_inner.y + vis_idx as u16;
        let commit = &modal.commits[commit_idx];
        let is_selected = commit_idx == selected;
        let max_w = left_inner.width as usize;

        // Graph chars.
        let graph_str = modal
            .graph_lines
            .get(commit_idx)
            .map(|s| s.as_str())
            .unwrap_or("");
        let graph_w = graph_str.len().min(12);

        // Refs badges.
        let refs_str = if commit.refs.is_empty() {
            String::new()
        } else {
            format!(
                " [{}]",
                commit
                    .refs
                    .iter()
                    .map(|r| r.replace("HEAD -> ", ""))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        // Time.
        let time = commit.time_ago();

        // Build spans.
        let avail_msg = max_w.saturating_sub(graph_w + 1 + 8 + refs_str.len() + time.len() + 4);
        let msg: String = commit.summary.chars().take(avail_msg).collect();

        let bg = if is_selected {
            Style::default().bg(Color::Rgb(30, 30, 60))
        } else {
            Style::default()
        };

        let mut spans: Vec<Span> = Vec::new();

        // Graph characters with colors.
        for (ci, ch) in graph_str.chars().take(graph_w).enumerate() {
            let color_idx = modal
                .graph_colors
                .get(commit_idx)
                .and_then(|c| c.get(ci))
                .copied()
                .unwrap_or(7);
            let color = if color_idx < 6 {
                graph_colors[color_idx as usize]
            } else {
                Color::DarkGray
            };
            spans.push(Span::styled(
                ch.to_string(),
                bg.fg(if ch == '*' { Color::White } else { color }),
            ));
        }

        spans.push(Span::styled(
            format!(" {} ", commit.short),
            bg.fg(Color::Yellow),
        ));

        if !refs_str.is_empty() {
            spans.push(Span::styled(
                refs_str,
                bg.fg(Color::Green).add_modifier(Modifier::BOLD),
            ));
        }

        spans.push(Span::styled(
            msg,
            if is_selected {
                bg.fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                bg.fg(Color::White)
            },
        ));

        spans.push(Span::styled(format!("  {time}"), bg.fg(Color::DarkGray)));

        let line = ratatui::text::Line::from(spans);
        frame.render_widget(
            Paragraph::new(line).style(bg),
            Rect::new(left_inner.x, row_y, left_inner.width, 1),
        );
    }

    // Right panel: commit detail.
    if let Some(detail_rect) = detail_area {
        let detail_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Commit Detail ")
            .title_style(Style::default().fg(Color::DarkGray));
        let detail_inner = detail_block.inner(detail_rect);
        frame.render_widget(detail_block, detail_rect);

        if let Some(commit) = modal.commits.get(selected) {
            let mut row_y = detail_inner.y;
            let dw = detail_inner.width as usize;

            // Hash.
            if row_y < detail_inner.y + detail_inner.height {
                let h: String = format!("  {}", commit.hash).chars().take(dw).collect();
                frame.render_widget(
                    Paragraph::new(h).style(Style::default().fg(Color::Yellow)),
                    Rect::new(detail_inner.x, row_y, detail_inner.width, 1),
                );
                row_y += 1;
            }
            // Author + time.
            if row_y < detail_inner.y + detail_inner.height {
                let info: String = format!("  {} | {}", commit.author, commit.time_ago())
                    .chars()
                    .take(dw)
                    .collect();
                frame.render_widget(
                    Paragraph::new(info).style(Style::default().fg(Color::Cyan)),
                    Rect::new(detail_inner.x, row_y, detail_inner.width, 1),
                );
                row_y += 1;
            }
            // Refs.
            if !commit.refs.is_empty() && row_y < detail_inner.y + detail_inner.height {
                let r: String = format!("  {}", commit.refs.join(", "))
                    .chars()
                    .take(dw)
                    .collect();
                frame.render_widget(
                    Paragraph::new(r).style(Style::default().fg(Color::Green)),
                    Rect::new(detail_inner.x, row_y, detail_inner.width, 1),
                );
                row_y += 1;
            }
            // Message.
            if row_y < detail_inner.y + detail_inner.height {
                let m: String = format!("  {}", commit.summary).chars().take(dw).collect();
                frame.render_widget(
                    Paragraph::new(m).style(Style::default().fg(Color::White)),
                    Rect::new(detail_inner.x, row_y, detail_inner.width, 1),
                );
                row_y += 1;
            }
            // Separator.
            if row_y < detail_inner.y + detail_inner.height {
                frame.render_widget(
                    Paragraph::new("─".repeat(dw)).style(Style::default().fg(Color::DarkGray)),
                    Rect::new(detail_inner.x, row_y, detail_inner.width, 1),
                );
                row_y += 1;
            }
            // Changed files.
            if row_y < detail_inner.y + detail_inner.height {
                let header = format!("  Files ({})", modal.detail_files.len());
                frame.render_widget(
                    Paragraph::new(header).style(
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Rect::new(detail_inner.x, row_y, detail_inner.width, 1),
                );
                row_y += 1;
            }
            for (status, path) in &modal.detail_files {
                if row_y >= detail_inner.y + detail_inner.height {
                    break;
                }
                let color = match status {
                    'M' => Color::Yellow,
                    'A' => Color::Green,
                    'D' => Color::Red,
                    'R' => Color::Blue,
                    _ => Color::DarkGray,
                };
                let filename = path.rsplit('/').next().unwrap_or(path);
                let dir = if path.contains('/') {
                    &path[..path.len() - filename.len() - 1]
                } else {
                    ""
                };
                let line = ratatui::text::Line::from(vec![
                    Span::styled(format!("  {status} "), Style::default().fg(color)),
                    Span::styled(
                        filename.to_string(),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        if dir.is_empty() {
                            String::new()
                        } else {
                            format!("  {dir}")
                        },
                        Style::default().fg(Color::DarkGray),
                    ),
                ]);
                frame.render_widget(
                    Paragraph::new(line),
                    Rect::new(detail_inner.x, row_y, detail_inner.width, 1),
                );
                row_y += 1;
            }
        }
    }

    // Footer hint.
    let footer_y = rect.y + rect.height.saturating_sub(1);
    frame.render_widget(
        Paragraph::new(Span::styled(
            " j/k:navigate  d/u:page  Enter:detail  Esc:close ",
            Style::default().fg(Color::DarkGray),
        )),
        Rect::new(rect.x + 1, footer_y, rect.width.saturating_sub(2), 1),
    );
}

/// Draw the branch picker modal (centered popup).
fn draw_branch_picker(frame: &mut Frame, picker: &crate::branch_picker::BranchPicker, area: Rect) {
    let width = area.width.min(60);
    let height = area.height.min(20);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(" Switch Branch ")
        .title_style(
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        );
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    if inner.height == 0 {
        return;
    }

    // Filter input.
    let query_line = if picker.query.is_empty() {
        "Select a branch...".to_string()
    } else {
        format!("> {}", picker.query)
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            query_line,
            Style::default().fg(if picker.query.is_empty() {
                Color::DarkGray
            } else {
                Color::Yellow
            }),
        )),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    // Branch list.
    let list_height = inner.height.saturating_sub(1) as usize;
    let selected = picker.selected;
    let scroll = if selected >= list_height {
        selected - list_height + 1
    } else {
        0
    };

    for (vis_idx, &branch_idx) in picker
        .filtered
        .iter()
        .enumerate()
        .skip(scroll)
        .take(list_height)
    {
        let branch = &picker.branches[branch_idx];
        let row_y = inner.y + 1 + (vis_idx - scroll) as u16;
        if row_y >= inner.y + inner.height {
            break;
        }

        let is_selected = vis_idx == selected;
        let is_current = branch.is_current;

        let indicator = if is_current { "* " } else { "  " };
        let name = &branch.name;
        let tip = &branch.tip_short;

        let max_name = (inner.width as usize).saturating_sub(indicator.len() + tip.len() + 3);
        let name_display: String = name.chars().take(max_name).collect();

        let line = ratatui::text::Line::from(vec![
            Span::styled(
                indicator,
                if is_current {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default()
                },
            ),
            Span::styled(
                name_display,
                if is_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else if is_current {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
            Span::styled(
                format!("  {tip}"),
                if is_selected {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
        ]);

        let bg = if is_selected {
            Style::default().bg(Color::Cyan)
        } else {
            Style::default()
        };
        frame.render_widget(
            Paragraph::new(line).style(bg),
            Rect::new(inner.x, row_y, inner.width, 1),
        );
    }
}

/// Draw the command palette (centered popup).
fn draw_command_palette(
    frame: &mut Frame,
    palette: &crate::command_palette::CommandPalette,
    area: Rect,
) {
    let width = area.width.min(80);
    let height = area.height.min(20);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(" Command Palette ")
        .title_style(
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        );
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    if inner.height == 0 {
        return;
    }

    // Query line.
    let query_line = format!("> {}", palette.query);
    frame.render_widget(
        Paragraph::new(Span::styled(
            query_line,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    // Results.
    let list_height = inner.height.saturating_sub(1) as usize;
    let selected = palette.selected;
    let scroll_start = if selected >= list_height {
        selected - list_height + 1
    } else {
        0
    };

    for (vis_idx, &item_idx) in palette
        .filtered
        .iter()
        .enumerate()
        .skip(scroll_start)
        .take(list_height)
    {
        let item = &palette.items()[item_idx];
        let row_y = inner.y + 1 + (vis_idx - scroll_start) as u16;
        if row_y >= inner.y + inner.height {
            break;
        }

        let is_selected = vis_idx == selected;
        let badge = item.badge();
        let badge_color = match item {
            crate::command_palette::PaletteItem::Command { .. } => Color::Cyan,
            crate::command_palette::PaletteItem::File { .. } => Color::Green,
            crate::command_palette::PaletteItem::Setting { .. } => Color::Yellow,
        };

        let display = item.display_text();
        let line = ratatui::text::Line::from(vec![
            Span::styled(
                format!(" [{badge}] "),
                if is_selected {
                    Style::default().fg(Color::Black).bg(badge_color)
                } else {
                    Style::default().fg(badge_color)
                },
            ),
            Span::styled(
                display.to_string(),
                if is_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ]);

        let bg = if is_selected {
            Style::default().bg(Color::Cyan)
        } else {
            Style::default()
        };
        frame.render_widget(
            Paragraph::new(line).style(bg),
            Rect::new(inner.x, row_y, inner.width, 1),
        );
    }
}

/// Draw the settings modal (centered popup).
fn draw_settings_modal(
    frame: &mut Frame,
    modal: &crate::settings_modal::SettingsModal,
    area: Rect,
) {
    use crate::settings_modal::SettingValue;

    let entry_count = modal.entries.len() as u16;
    let width = 50u16.min(area.width.saturating_sub(4));
    let height = (entry_count + 4).min(area.height.saturating_sub(2)); // title + entries + footer + borders
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let rect = Rect::new(x, y, width, height);

    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Settings ")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let mut row_y = inner.y;

    for (i, entry) in modal.entries.iter().enumerate() {
        if row_y >= inner.y + inner.height {
            break;
        }
        let is_selected = i == modal.selected;

        let value_str = match &entry.value {
            SettingValue::Bool(true) => "[x]".to_string(),
            SettingValue::Bool(false) => "[ ]".to_string(),
            SettingValue::Number { current, .. } => format!("< {current} >"),
        };

        let label_width = inner.width.saturating_sub(value_str.len() as u16 + 2) as usize;
        let label: String = entry.label.chars().take(label_width).collect();
        let padding = label_width.saturating_sub(label.len());

        let line = ratatui::text::Line::from(vec![
            Span::styled(
                format!(" {label}{}", " ".repeat(padding)),
                if is_selected {
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                },
            ),
            Span::styled(
                format!("{value_str} "),
                if is_selected {
                    match &entry.value {
                        SettingValue::Bool(true) => Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                        SettingValue::Bool(false) => Style::default().fg(Color::DarkGray),
                        SettingValue::Number { .. } => Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    }
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
        ]);

        let bg = if is_selected {
            Style::default().bg(Color::Rgb(30, 30, 50))
        } else {
            Style::default()
        };
        frame.render_widget(
            Paragraph::new(line).style(bg),
            Rect::new(inner.x, row_y, inner.width, 1),
        );
        row_y += 1;
    }

    // Footer hint.
    if row_y < inner.y + inner.height {
        row_y = inner.y + inner.height - 1;
        let hint = " j/k:navigate  Enter:toggle  Esc:close";
        let hint: String = hint.chars().take(inner.width as usize).collect();
        frame.render_widget(
            Paragraph::new(Span::styled(hint, Style::default().fg(Color::DarkGray))),
            Rect::new(inner.x, row_y, inner.width, 1),
        );
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
            display_lines.push((
                Some(filter_pos),
                format!("  {}", topic.title),
                topic.section.clone(),
            ));
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
        AuthorId::Peer { peer_id, .. } => {
            use aura_core::AuthorColor;
            match AuthorColor::for_peer(*peer_id) {
                AuthorColor::Cyan => Color::Cyan,
                AuthorColor::Magenta => Color::Magenta,
                AuthorColor::Orange => Color::Indexed(208),
                AuthorColor::Teal => Color::Indexed(30),
                AuthorColor::Purple => Color::Indexed(141),
                AuthorColor::Yellow => Color::Yellow,
                _ => Color::Gray,
            }
        }
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

            // Check if this line is the stopped execution line in the debugger.
            let is_debug_stopped = app
                .debug_panel
                .state
                .stopped_file
                .as_ref()
                .is_some_and(|f| tab.buffer.file_path() == Some(f.as_path()))
                && app.debug_panel.state.stopped_line == Some(line_idx);

            // Gutter marker: breakpoint/debug > diagnostic > conversation > git > authorship.
            let marker_span = if tab.breakpoints.contains(&line_idx) {
                if is_debug_stopped {
                    Span::styled(
                        "⏸",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled("●", Style::default().fg(Color::Red))
                }
            } else if is_debug_stopped {
                Span::styled(
                    "→",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else if let Some(diag) = app.line_diagnostics(line_idx) {
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

            // Get block selection rectangle if in VisualBlock mode.
            let block_rect = app.visual_block_rect();

            for (col, ch) in visible_chars.iter().enumerate() {
                let char_abs = line_start_idx + visible_start + col;
                let actual_col = visible_start + col;

                // Block selection: check if this cell is within the rectangle.
                let in_block = block_rect
                    .map(|(sr, er, sc, ec)| {
                        line_idx >= sr && line_idx <= er && actual_col >= sc && actual_col <= ec
                    })
                    .unwrap_or(false);

                let in_selection = in_block
                    || selection
                        .map(|(s, e)| char_abs >= s && char_abs < e)
                        .unwrap_or(false);

                // Check bracket match highlight.
                let is_bracket_match = app
                    .matching_bracket
                    .map(|(r, c)| r == line_idx && c == visible_start + col)
                    .unwrap_or(false);

                // Check search match highlight.
                let is_current_search = app
                    .search_matches
                    .get(app.search_current)
                    .map(|&(s, e)| char_abs >= s && char_abs < e)
                    .unwrap_or(false);
                let is_search_match = !is_current_search
                    && app
                        .search_matches
                        .iter()
                        .any(|&(s, e)| char_abs >= s && char_abs < e);

                let style = if in_selection {
                    sel_style
                } else if is_current_search {
                    Style::default()
                        .bg(Color::Yellow)
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD)
                } else if is_search_match {
                    Style::default().bg(Color::Rgb(100, 80, 0)).fg(Color::White)
                } else if is_bracket_match {
                    Style::default()
                        .bg(Color::DarkGray)
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
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

/// Draw a single editor pane (border + editor content + optional minimap).
fn draw_editor_pane(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    tab_idx: usize,
    is_focused: bool,
) {
    let tab_idx = tab_idx.min(app.tabs.count().saturating_sub(1));
    let border_color = if is_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let title = format!(" {} ", app.tabs.tabs()[tab_idx].title());
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(border_color));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Minimap.
    let show_minimap = app.config.editor.show_minimap;
    let (content_area, minimap_area) = if show_minimap {
        let hsplit = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner);
        (hsplit[0], Some(hsplit[1]))
    } else {
        (inner, None)
    };

    // Scroll the focused pane's tab to keep cursor visible.
    if is_focused {
        let gutter_w = 6u16;
        let vp_h = content_area.height as usize;
        let vp_w = content_area.width.saturating_sub(gutter_w) as usize;
        app.scroll_to_cursor(vp_h, vp_w);
    }

    // Git line status for visible lines.
    let pane_scroll_row = app.tabs.tabs()[tab_idx].scroll_row;
    let git_status: std::collections::HashMap<usize, LineStatus> = {
        let visible = content_area.height as usize;
        let mut status = std::collections::HashMap::new();
        for i in 0..visible {
            let line_idx = pane_scroll_row + i;
            if let Some(s) = app.git_line_status(line_idx) {
                status.insert(line_idx, s);
            }
        }
        status
    };

    draw_editor(frame, app, content_area, &git_status);
    if is_focused {
        draw_peer_cursors(frame, app, content_area);
        draw_secondary_cursors(frame, app, content_area);
    }

    // Minimap.
    if let Some(minimap_rect) = minimap_area {
        let tab = &app.tabs.tabs()[tab_idx];
        let theme = &app.theme;
        let total_lines = tab.buffer.line_count();
        let scroll_row = tab.scroll_row;
        let viewport_h = content_area.height as usize;

        let mut markers: Vec<(usize, Color)> = Vec::new();
        for d in &tab.diagnostics {
            let color = if d.is_error() {
                theme.error
            } else if d.is_warning() {
                theme.warning
            } else {
                theme.info
            };
            markers.push((d.range.start.line as usize, color));
        }
        markers.sort_by_key(|&(line, color)| {
            let prio = match color {
                c if c == theme.error => 2u8,
                c if c == theme.warning => 1,
                _ => 0,
            };
            (prio, line)
        });

        draw_minimap(
            frame,
            minimap_rect,
            &markers,
            total_lines,
            scroll_row,
            viewport_h,
        );
    }
}

/// Draw remote peer cursors and selections as overlays on the editor.
fn draw_peer_cursors(frame: &mut Frame, app: &App, area: Rect) {
    let peers = app.collab_peer_awareness();
    if peers.is_empty() {
        return;
    }

    let gutter_width = 6u16;
    let tab = app.tab();
    let scroll_row = tab.scroll_row;
    let scroll_col = tab.scroll_col;
    let visible_rows = area.height as usize;
    let text_area_x = area.x + gutter_width;
    let text_area_width = area.width.saturating_sub(gutter_width) as usize;

    for peer in &peers {
        let color = app.collab_peer_color(peer.peer_id);

        // Draw selection highlight if the peer has one.
        if let Some(((sr, sc), (er, ec))) = peer.selection {
            let (start_row, start_col, end_row, end_col) = if (sr, sc) <= (er, ec) {
                (sr, sc, er, ec)
            } else {
                (er, ec, sr, sc)
            };

            for row in start_row..=end_row {
                if row < scroll_row || row >= scroll_row + visible_rows {
                    continue;
                }
                let screen_y = area.y + (row - scroll_row) as u16;

                let col_start = if row == start_row {
                    start_col.saturating_sub(scroll_col)
                } else {
                    0
                };
                let col_end = if row == end_row {
                    end_col.saturating_sub(scroll_col)
                } else {
                    text_area_width
                };

                for col in col_start..col_end.min(text_area_width) {
                    let screen_x = text_area_x + col as u16;
                    if screen_x < area.x + area.width {
                        let cell = &mut frame.buffer_mut()[(screen_x, screen_y)];
                        cell.set_bg(color);
                        cell.set_fg(Color::Black);
                    }
                }
            }
        }

        // Draw cursor block.
        if let Some((row, col)) = peer.cursor {
            if row >= scroll_row
                && row < scroll_row + visible_rows
                && col >= scroll_col
                && col < scroll_col + text_area_width
            {
                let screen_y = area.y + (row - scroll_row) as u16;
                let screen_x = text_area_x + (col - scroll_col) as u16;

                if screen_x < area.x + area.width {
                    let cell = &mut frame.buffer_mut()[(screen_x, screen_y)];
                    cell.set_bg(color);
                    cell.set_fg(Color::Black);

                    // Draw name label above the cursor (if there's room).
                    if screen_y > area.y {
                        let label = &peer.name;
                        let label_len = label.len().min(12);
                        let label_y = screen_y - 1;
                        let label_start = screen_x;

                        for (i, ch) in label.chars().take(label_len).enumerate() {
                            let lx = label_start + i as u16;
                            if lx < area.x + area.width {
                                let cell = &mut frame.buffer_mut()[(lx, label_y)];
                                cell.set_char(ch);
                                cell.set_bg(color);
                                cell.set_fg(Color::Black);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Draw secondary cursors (multi-cursor editing) as colored blocks.
fn draw_secondary_cursors(frame: &mut Frame, app: &App, area: Rect) {
    let tab = app.tab();
    if tab.secondary_cursors.is_empty() {
        return;
    }

    let gutter_width = 6u16;
    let scroll_row = tab.scroll_row;
    let scroll_col = tab.scroll_col;
    let visible_rows = area.height as usize;
    let text_area_x = area.x + gutter_width;
    let text_area_width = area.width.saturating_sub(gutter_width) as usize;

    for cursor in &tab.secondary_cursors {
        if cursor.row >= scroll_row
            && cursor.row < scroll_row + visible_rows
            && cursor.col >= scroll_col
            && cursor.col < scroll_col + text_area_width
        {
            let screen_y = area.y + (cursor.row - scroll_row) as u16;
            let screen_x = text_area_x + (cursor.col - scroll_col) as u16;

            if screen_x < area.x + area.width {
                let cell = &mut frame.buffer_mut()[(screen_x, screen_y)];
                cell.set_bg(Color::Yellow);
                cell.set_fg(Color::Black);
            }
        }
    }
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
        Mode::Visual | Mode::VisualLine | Mode::VisualBlock => {
            Style::default().fg(Color::Black).bg(theme.mode_visual)
        }
        Mode::Intent => Style::default().fg(Color::Black).bg(theme.mode_intent),
        Mode::Review => Style::default().fg(Color::Black).bg(theme.mode_review),
        Mode::Diff => Style::default().fg(Color::Black).bg(Color::Cyan),
        Mode::MergeConflict => Style::default().fg(Color::Black).bg(Color::Magenta),
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

    let update_indicator = match &app.update_status {
        Some(crate::update::UpdateStatus::Available { version, .. }) => {
            format!(" │ \u{2191} v{}", version)
        }
        _ => String::new(),
    };

    let collab_indicator = match app.collab_status() {
        crate::collab::CollabStatus::Hosting { port, peer_count } => {
            if peer_count > 0 {
                format!(" │ COLLAB:{port} ({peer_count} peers)")
            } else {
                format!(" │ COLLAB:{port}")
            }
        }
        crate::collab::CollabStatus::Connected { peer_count } => {
            format!(" │ COLLAB ({peer_count} peers)")
        }
        crate::collab::CollabStatus::Reconnecting { attempt } => {
            format!(" │ COLLAB reconnecting #{attempt}...")
        }
        crate::collab::CollabStatus::Inactive => String::new(),
    };

    let claude_indicator = app
        .claude_watcher
        .as_ref()
        .and_then(|w| w.latest_activity.as_ref())
        .map(|a| format!(" │ {a}"))
        .unwrap_or_default();

    let left = format!(
        " {} │ {}{}{}{}{}{}{}{}{}{}{}",
        app.mode.label(),
        file_name,
        modified,
        git_indicator,
        last_change,
        diag_str,
        lsp_indicator,
        mcp_indicator,
        collab_indicator,
        claude_indicator,
        experiment_indicator,
        update_indicator
    );
    // Show selection info when in visual mode.
    let selection_info = if matches!(
        app.mode,
        Mode::Visual | Mode::VisualLine | Mode::VisualBlock
    ) {
        if let Some((sel_start, sel_end)) = app.visual_selection_range() {
            let start_cur = app.buffer().char_idx_to_cursor(sel_start);
            let end_cur = app.buffer().char_idx_to_cursor(sel_end);
            let lines = end_cur.row.saturating_sub(start_cur.row) + 1;
            let chars = sel_end.saturating_sub(sel_start);
            if lines > 1 {
                format!(" {} lines selected │", lines)
            } else {
                format!(
                    " {} char{} selected │",
                    chars,
                    if chars == 1 { "" } else { "s" }
                )
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let search_info = if let Some(ref q) = app.search_query {
        if app.search_matches.is_empty() {
            format!("/{q} ")
        } else {
            format!(
                "/{q} {}/{} ",
                app.search_current + 1,
                app.search_matches.len()
            )
        }
    } else {
        String::new()
    };
    let right = format!(
        "{}{} {}:{} ",
        search_info,
        selection_info,
        app.cursor().row + 1,
        app.cursor().col + 1,
    );

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
    let content = if app.search_active {
        // Show search input bar.
        let match_info = if app.search_matches.is_empty() {
            if app.search_input.is_empty() {
                String::new()
            } else {
                " (no matches)".to_string()
            }
        } else {
            format!(" ({}/{})", app.search_current + 1, app.search_matches.len())
        };
        format!("/{}{}", app.search_input, match_info)
    } else {
        match app.mode {
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
/// Draw the references panel (floating popup listing all symbol references).
fn draw_references_panel(frame: &mut Frame, app: &App, area: Rect) {
    let panel = match &app.references_panel {
        Some(p) => p,
        None => return,
    };

    let count = panel.locations.len();
    let title = format!(" References ({count}) ");
    let width = area.width.clamp(30, 80);
    let height = (count as u16 + 2).min(area.height.saturating_sub(4)).max(4);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    for (i, loc) in panel.locations.iter().enumerate() {
        if i as u16 >= inner.height {
            break;
        }
        let row_y = inner.y + i as u16;
        let is_selected = i == panel.selected;
        let style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };

        // Format: filename:line
        let path = loc.uri.strip_prefix("file://").unwrap_or(&loc.uri);
        let filename = std::path::Path::new(path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(path);
        let line = loc.range.start.line + 1;
        let col = loc.range.start.character + 1;
        let text = format!("  {filename}:{line}:{col}");
        let text = if text.len() > inner.width as usize {
            text[..inner.width as usize].to_string()
        } else {
            text
        };

        frame.render_widget(
            Paragraph::new(text).style(style),
            Rect::new(inner.x, row_y, inner.width, 1),
        );
    }

    if panel.locations.is_empty() {
        frame.render_widget(
            Paragraph::new("  No references found").style(Style::default().fg(Color::DarkGray)),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
    }
}

/// Draw the rename input overlay in the command bar area.
fn draw_rename_input(frame: &mut Frame, app: &App, area: Rect) {
    let text = format!("Rename: {}", app.rename_input);
    let style = Style::default().fg(Color::Yellow).bg(Color::Black);
    frame.render_widget(Paragraph::new(text).style(style), area);
}

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

// ---------------------------------------------------------------------------
// Update notification & modal
// ---------------------------------------------------------------------------

/// Draw a floating notification toast in the top-right corner.
fn draw_update_notification(frame: &mut Frame, app: &mut App, area: Rect) {
    let version = match &app.update_status {
        Some(crate::update::UpdateStatus::Available { version, .. }) => version.clone(),
        _ => return,
    };

    let text = format!(" Update v{} available — click to update ", version);
    let width = (text.len() as u16 + 2).min(area.width);
    let height = 3u16;
    let x = area.x + area.width.saturating_sub(width + 1);
    let y = area.y + 1;
    let rect = Rect::new(x, y, width, height);

    // Save rect for mouse click detection.
    app.update_notification_rect = rect;

    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" New Version ")
        .title_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let line = Line::from(vec![
        Span::styled(
            format!(" \u{2191} v{} available ", version),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" [click] ", Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(line), inner);
}

/// Draw the update confirmation modal (centered popup).
fn draw_update_modal(frame: &mut Frame, app: &App, area: Rect) {
    let (version, url) = match &app.update_status {
        Some(crate::update::UpdateStatus::Available { version, url }) => {
            (version.clone(), url.clone())
        }
        _ => return,
    };

    let method = crate::update::detect_install_method();
    let cmd = crate::update::upgrade_instructions(&method, &version);

    let width = 60u16.min(area.width.saturating_sub(4));
    let height = 9u16;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let rect = Rect::new(x, y, width, height);

    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Update Available ")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let current = crate::update::CURRENT_VERSION;
    let lines = vec![
        Line::from(Span::styled(
            format!("  Current: v{}  \u{2192}  New: v{}", current, version),
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}", cmd),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}", url),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  [Y] Update  ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  [N/Esc] Cancel  ", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    frame.render_widget(Paragraph::new(lines), inner);
}

/// Draw the close-tab confirmation modal (centered popup).
fn draw_close_tab_modal(frame: &mut Frame, app: &App, area: Rect) {
    let idx = match app.tab_close_confirm {
        Some(idx) => idx,
        None => return,
    };
    let tab_name = if idx < app.tabs.count() {
        app.tabs.tabs()[idx].file_name().to_string()
    } else {
        return;
    };

    let width = 50u16.min(area.width.saturating_sub(4));
    let height = 7u16;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let rect = Rect::new(x, y, width, height);

    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" Unsaved Changes ")
        .title_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let lines = vec![
        Line::from(Span::styled(
            format!("  \"{}\" has unsaved changes.", tab_name),
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  [S] Save & Close  ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  [D] Discard  ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  [Esc] Cancel  ", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    frame.render_widget(Paragraph::new(lines), inner);
}
