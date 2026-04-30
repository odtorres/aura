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

    // Fill the entire screen with the theme background color.
    let bg = app.theme.bg;
    if bg != Color::Reset {
        frame.render_widget(Block::default().style(Style::default().bg(bg)), area);
    }

    // Layout: optional tab bar + editor area + optional proposal + optional debug panel + optional terminal + status bar + command bar.
    let has_proposal = app.proposal.is_some() && app.mode == Mode::Review;
    let has_terminal = app.terminal().visible;
    let has_debug_panel = app.debug_panel.visible;
    let terminal_height = if has_terminal {
        app.terminal().height
    } else {
        0
    };
    let debug_panel_height = if has_debug_panel {
        app.debug_panel.height
    } else {
        0
    };
    let zen = app.zen_mode;
    let tab_bar_height: u16 = if zen { 0 } else { 1 };
    let status_height: u16 = if zen { 0 } else { 1 };
    let cmd_height: u16 = if zen { 0 } else { 1 };
    let terminal_height = if zen { 0 } else { terminal_height };
    let debug_panel_height = if zen { 0 } else { debug_panel_height };

    let chunks = if has_proposal {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(tab_bar_height),     // Tab bar (0 or 1 row)
                Constraint::Percentage(50),             // Editor (original)
                Constraint::Percentage(50),             // Proposal (diff)
                Constraint::Length(debug_panel_height), // Debug panel (0 when hidden)
                Constraint::Length(terminal_height),    // Terminal pane (0 when hidden)
                Constraint::Length(status_height),      // Status bar
                Constraint::Length(cmd_height),         // Command bar
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
                Constraint::Length(status_height),      // Status bar
                Constraint::Length(cmd_height),         // Command bar
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
    } else if app.ai_visor.visible {
        Some(app.ai_visor.width)
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
            app.ai_visor_rect = Rect::default();
            draw_chat_panel(frame, app, area);
        } else if app.ai_visor.visible {
            app.conv_history_rect = Rect::default();
            app.chat_panel_rect = Rect::default();
            app.ai_visor_rect = area;
            draw_ai_visor(frame, app, area);
        } else {
            app.conv_history_rect = area;
            app.chat_panel_rect = Rect::default();
            app.ai_visor_rect = Rect::default();
            draw_conversation_history(frame, app, area);
        }
    } else {
        app.conv_history_rect = Rect::default();
        app.chat_panel_rect = Rect::default();
        app.ai_visor_rect = Rect::default();
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
                app.terminal_mut().resize(inner_w, inner_h);
            }
            if app.viewing_shared_terminal && app.collab_shared_terminal.is_some() {
                draw_shared_terminal(frame, app, terminal_area);
            } else {
                draw_terminal(frame, app, terminal_area);
            }
        }

        if has_debug_panel {
            app.debug_panel_rect = debug_panel_area;
            draw_debug_panel(frame, app, debug_panel_area);
        }

        app.status_bar_rect = status_area;
        draw_status_bar(frame, app, status_area);
        draw_command_bar(frame, app, command_area);
        return;
    }

    // If the active tab has a diff attached, render it as a full-pane view.
    if app.tab().diff.is_some() {
        draw_diff_view(frame, app, editor_area);

        if has_terminal {
            let inner_h = terminal_area.height.saturating_sub(2);
            let inner_w = terminal_area.width.saturating_sub(2);
            if inner_h > 0 && inner_w > 0 {
                app.terminal_mut().resize(inner_w, inner_h);
            }
            if app.viewing_shared_terminal && app.collab_shared_terminal.is_some() {
                draw_shared_terminal(frame, app, terminal_area);
            } else {
                draw_terminal(frame, app, terminal_area);
            }
        }

        // Draw debug panel if visible.
        if has_debug_panel {
            app.debug_panel_rect = debug_panel_area;
            draw_debug_panel(frame, app, debug_panel_area);
        }

        app.status_bar_rect = status_area;
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
            // Sync scroll position from focused pane to the other if enabled.
            if app.split_scroll_sync
                && primary_idx < app.tabs.count()
                && secondary_idx < app.tabs.count()
            {
                if primary_focused {
                    let sr = app.tabs.tabs()[primary_idx].scroll_row;
                    let sc = app.tabs.tabs()[primary_idx].scroll_col;
                    app.tabs.tabs_mut()[secondary_idx].scroll_row = sr;
                    app.tabs.tabs_mut()[secondary_idx].scroll_col = sc;
                } else if secondary_focused {
                    let sr = app.tabs.tabs()[secondary_idx].scroll_row;
                    let sc = app.tabs.tabs()[secondary_idx].scroll_col;
                    app.tabs.tabs_mut()[primary_idx].scroll_row = sr;
                    app.tabs.tabs_mut()[primary_idx].scroll_col = sc;
                }
            }
            draw_editor_pane(frame, app, pane_b, secondary_idx, secondary_focused);
        } else {
            let is_focused = !app.terminal_focused
                && !app.file_tree_focused
                && !app.source_control_focused
                && !app.conversation_history_focused;

            // Markdown preview: split editor area 50/50 with preview on the right.
            if app.preview_active {
                let preview_split = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(editor_area);
                draw_editor_pane(
                    frame,
                    app,
                    preview_split[0],
                    app.tabs.active_index(),
                    is_focused,
                );
                draw_markdown_preview(frame, app, preview_split[1]);
            } else {
                draw_editor_pane(frame, app, editor_area, app.tabs.active_index(), is_focused);
            }
        }

        if has_proposal {
            draw_proposal(frame, app, proposal_area);
        }

        if has_terminal {
            // Sync the PTY screen size with the actual rendered inner area.
            let inner_h = terminal_area.height.saturating_sub(2); // borders
            let inner_w = terminal_area.width.saturating_sub(2);
            if inner_h > 0 && inner_w > 0 {
                app.terminal_mut().resize(inner_w, inner_h);
            }
            if app.viewing_shared_terminal && app.collab_shared_terminal.is_some() {
                draw_shared_terminal(frame, app, terminal_area);
            } else {
                draw_terminal(frame, app, terminal_area);
            }
        }

        // Draw debug panel if visible.
        if has_debug_panel {
            app.debug_panel_rect = debug_panel_area;
            draw_debug_panel(frame, app, debug_panel_area);
        }

        app.status_bar_rect = status_area;
        draw_status_bar(frame, app, status_area);
        draw_command_bar(frame, app, command_area);

        // Compute the editor inner area for overlays (account for block borders).
        let editor_inner_for_popups = Rect::new(
            editor_area.x + 1,
            editor_area.y + 1,
            editor_area.width.saturating_sub(2),
            editor_area.height.saturating_sub(2),
        );

        // Render inline conflict action hints.
        if !app.inline_conflicts.is_empty() {
            draw_conflict_actions(frame, app, editor_inner_for_popups);
        }

        // Render ghost suggestion if present.
        if let Some(suggestion) = app.current_ghost_suggestion() {
            draw_ghost_text(frame, app, editor_inner_for_popups, suggestion);
        }

        // Render inline AI completion (ghost text after cursor).
        if let Some(ref completion) = app.inline_completion {
            let cursor_row = app.tab().cursor.row;
            let cursor_col = app.tab().cursor.col;
            let scroll_row = app.tab().scroll_row;
            let scroll_col = app.tab().scroll_col;
            let gutter_w = 6u16;
            if cursor_row >= scroll_row {
                let screen_row = (cursor_row - scroll_row) as u16;
                let screen_col = (cursor_col.saturating_sub(scroll_col) + gutter_w as usize) as u16;
                let y = editor_inner_for_popups.y + screen_row;
                let x = editor_inner_for_popups.x + screen_col;
                if y < editor_inner_for_popups.y + editor_inner_for_popups.height {
                    let max_w = (editor_inner_for_popups.x + editor_inner_for_popups.width)
                        .saturating_sub(x) as usize;
                    let display: String = completion.chars().take(max_w).collect();
                    if !display.is_empty() {
                        frame.render_widget(
                            Paragraph::new(Span::styled(
                                display,
                                Style::default()
                                    .fg(app.theme.ghost)
                                    .add_modifier(Modifier::ITALIC),
                            )),
                            Rect::new(x, y, max_w as u16, 1),
                        );
                    }
                }
            }
        }

        // Render next-edit prediction markers.
        if !app.edit_predictions().is_empty() {
            draw_edit_predictions(frame, app, editor_inner_for_popups);
        }

        // Render peek definition popup if present.
        if app.peek_definition.is_some() {
            draw_peek_definition(frame, app, editor_inner_for_popups);
        }

        // Render signature help popup if active.
        if let Some(ref sig) = app.tab().signature_help.clone() {
            draw_signature_help(frame, app, editor_inner_for_popups, sig);
        }

        // Render hover popup if present.
        if let Some(hover_text) = app.tab().hover_info.clone() {
            draw_hover_popup(frame, app, editor_inner_for_popups, &hover_text);
        }

        // Render diagnostic popup when cursor is on a diagnostic line.
        if app.tab().hover_info.is_none()
            && app.peek_definition.is_none()
            && app.current_ghost_suggestion().is_none()
        {
            let cursor_row = app.cursor().row;
            let diag_clone = app.line_diagnostics(cursor_row).cloned();
            if let Some(ref diag) = diag_clone {
                draw_diagnostic_popup(frame, app, editor_inner_for_popups, diag);
            }
        }

        // Render references panel if present.
        if app.references_panel.is_some() {
            draw_references_panel(frame, app, area);
        }

        // Render inline AI input bar (Ctrl+K).
        if app.inline_ai_active {
            let input_text = app.inline_ai_input.as_deref().unwrap_or("");
            let display = format!(" AI> {} ", input_text);
            let w = editor_inner_for_popups.width.min(60);
            let x =
                editor_inner_for_popups.x + (editor_inner_for_popups.width.saturating_sub(w)) / 2;
            let cursor_row = app.tab().cursor.row;
            let scroll_row = app.tab().scroll_row;
            let screen_row = cursor_row.saturating_sub(scroll_row) as u16 + 1;
            let y = (editor_inner_for_popups.y + screen_row)
                .min(editor_inner_for_popups.y + editor_inner_for_popups.height - 1);
            let bar = Rect::new(x, y, w, 1);
            frame.render_widget(Clear, bar);
            frame.render_widget(
                Paragraph::new(display).style(
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Rgb(60, 60, 120))
                        .add_modifier(Modifier::BOLD),
                ),
                bar,
            );
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

        // Render project search overlay if visible.
        if app.project_search.visible {
            draw_project_search(frame, app, area);
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

        // Render interactive rebase modal if visible.
        if app.rebase_modal.visible {
            draw_rebase_modal(frame, &app.rebase_modal, area);
        }

        // Render plugin marketplace modal if visible.
        if app.marketplace.visible {
            draw_marketplace_modal(frame, &app.marketplace, area);
        }

        // Render undo tree modal if visible.
        if app.undo_tree.is_some() {
            draw_undo_tree_modal(frame, app, area);
        }

        // Render registers modal if visible.
        if app.registers_visible {
            draw_registers_modal(frame, app, area);
        }

        // Render document outline if visible.
        if app.outline_visible {
            draw_outline_modal(frame, app, area);
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

        // Render which-key popup when leader key is pending.
        if app.which_key_visible && !app.which_key_items.is_empty() {
            draw_which_key_popup(frame, app, area);
        }

        // Right-click context menu always renders on top of every other
        // overlay, since it is the most recently user-summoned popup.
        if app.context_menu.visible {
            draw_context_menu(frame, app, area);
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
            let (_snap, t_cursor_row, t_cursor_col) = app.terminal().snapshot();
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
/// Draw the minimap: condensed code overview with viewport indicator and diagnostic markers.
///
/// The minimap shows a compressed view of the file where each row maps to one
/// or more source lines. The viewport window is highlighted with a lighter background.
/// Diagnostic markers (errors/warnings) are shown as colored dots on the right edge.
fn draw_minimap(
    frame: &mut Frame,
    area: Rect,
    markers: &[(usize, Color)],
    total_lines: usize,
    viewport_start: usize,
    viewport_lines: usize,
    buffer_lines: &[String],
) {
    let h = area.height as usize;
    let w = area.width as usize;
    if h == 0 || total_lines == 0 || w == 0 {
        return;
    }

    let dark_bg = Color::Rgb(30, 30, 30);
    let viewport_bg = Color::Rgb(50, 50, 55);

    // Build marker lookup (line -> color, highest priority wins).
    let mut marker_map: std::collections::HashMap<usize, Color> = std::collections::HashMap::new();
    for &(line, color) in markers {
        marker_map.insert(line, color);
    }

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

    // Scrollbar column (rightmost): 1 char wide showing viewport position.
    let scrollbar_x = area.x + area.width.saturating_sub(1);
    let content_w = w.saturating_sub(1); // minimap code area width

    for r in 0..h {
        let in_viewport = r >= vp_row_start && r < vp_row_end;
        let bg = if in_viewport { viewport_bg } else { dark_bg };

        // Map this row to a source line.
        let source_line = if total_lines <= h {
            r
        } else {
            r * total_lines / h
        };

        // Get the condensed line content.
        let line_text = buffer_lines
            .get(source_line)
            .map(|s| s.as_str())
            .unwrap_or("");

        // Use block elements to represent code density — each column shows
        // whether that position has a non-space character, giving a "tiny text" look.
        let trimmed = line_text.trim_start();
        let indent = line_text.len().saturating_sub(trimmed.len());
        let indent_cols = (indent / 4).min(content_w); // 4:1 compression for indent

        // Also get the next source line for 2-line-per-row half-block rendering.
        let next_line = source_line + 1;
        let next_text = buffer_lines
            .get(next_line)
            .map(|s| s.as_str())
            .unwrap_or("");
        let next_trimmed = next_text.trim_start();
        let next_indent = next_text.len().saturating_sub(next_trimmed.len());
        let next_indent_cols = (next_indent / 4).min(content_w);

        // Dim the text for minimap appearance.
        let text_fg = if in_viewport {
            Color::Rgb(140, 140, 155)
        } else {
            Color::Rgb(90, 90, 100)
        };
        let dim_fg = if in_viewport {
            Color::Rgb(100, 100, 110)
        } else {
            Color::Rgb(60, 60, 65)
        };

        // Build the minimap row using half-block characters.
        let mut spans: Vec<Span> = Vec::new();
        for col in 0..content_w {
            let top_has_char = col >= indent_cols
                && col < indent_cols + trimmed.len() / 2 + 1
                && !trimmed.is_empty();
            let bot_has_char = col >= next_indent_cols
                && col < next_indent_cols + next_trimmed.len() / 2 + 1
                && !next_trimmed.is_empty();

            let (ch, fg) = match (top_has_char, bot_has_char) {
                (true, true) => ("█", text_fg),
                (true, false) => ("▀", text_fg),
                (false, true) => ("▄", dim_fg),
                (false, false) => (" ", dim_fg),
            };
            spans.push(Span::styled(ch, Style::default().fg(fg).bg(bg)));
        }

        let cell_area = Rect::new(area.x, area.y + r as u16, content_w as u16, 1);
        frame.render_widget(Paragraph::new(Line::from(spans)), cell_area);

        // Scrollbar column: filled block for viewport, thin for rest.
        let has_marker = marker_map.get(&source_line);
        let sb_char = if let Some(&color) = has_marker {
            Span::styled("●", Style::default().fg(color).bg(dark_bg))
        } else if in_viewport {
            Span::styled(
                "█",
                Style::default().fg(Color::Rgb(90, 90, 100)).bg(dark_bg),
            )
        } else {
            Span::styled("│", Style::default().fg(Color::Rgb(50, 50, 50)).bg(dark_bg))
        };
        let sb_area = Rect::new(scrollbar_x, area.y + r as u16, 1, 1);
        frame.render_widget(Paragraph::new(Line::from(sb_char)), sb_area);
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

    // Build syntax highlighter from the merge file's extension.
    let ext = view.file_path.rsplit('.').next().unwrap_or("");
    let mut highlighter = crate::highlight::Language::from_extension(ext)
        .and_then(crate::highlight::SyntaxHighlighter::new);

    // Pre-highlight all three panels.
    let incoming_lines = view.incoming_lines();
    let current_lines = view.current_lines();
    let result_lines = view.result_lines();

    let hl_incoming = highlighter.as_mut().map(|h| {
        let text: String = incoming_lines
            .iter()
            .map(|(l, _)| format!("{l}\n"))
            .collect();
        h.highlight(&text, Some(&app.theme))
    });
    let hl_current = highlighter.as_mut().map(|h| {
        let text: String = current_lines
            .iter()
            .map(|(l, _)| format!("{l}\n"))
            .collect();
        h.highlight(&text, Some(&app.theme))
    });
    let hl_result = highlighter.as_mut().map(|h| {
        let text: String = result_lines.iter().map(|(l, _)| format!("{l}\n")).collect();
        h.highlight(&text, Some(&app.theme))
    });

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

        let scroll = view.scroll_incoming;
        draw_merge_panel_lines(
            frame,
            inner,
            &incoming_lines,
            scroll,
            active_conflict,
            Color::Green,
            hl_incoming.as_deref(),
        );
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

        let scroll = view.scroll_current;
        draw_merge_panel_lines(
            frame,
            inner,
            &current_lines,
            scroll,
            active_conflict,
            Color::Blue,
            hl_current.as_deref(),
        );
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

        let scroll = view.scroll_result;

        for (i, (line, conflict_idx)) in result_lines.iter().skip(scroll).enumerate() {
            if i as u16 >= inner.height {
                break;
            }
            let row_y = inner.y + i as u16;
            let line_idx = scroll + i;

            // Determine conflict background.
            let conflict_bg = if let Some(idx) = conflict_idx {
                let res = view.conflict_resolution(*idx);
                if res == Resolution::Unresolved {
                    Some(Color::Rgb(60, 60, 20))
                } else {
                    Some(Color::Rgb(20, 50, 20))
                }
            } else {
                None
            };

            // Build styled spans with syntax highlighting.
            let hl = hl_result.as_ref().and_then(|hls| hls.get(line_idx));
            let chars: Vec<char> = line.chars().collect();
            let max_col = (inner.width as usize).min(chars.len());
            let mut spans: Vec<Span> = Vec::new();
            let mut col = 0;

            while col < max_col {
                let fg = hl
                    .and_then(|h| h.colors.get(col).copied())
                    .unwrap_or(Color::Reset);
                let mods = hl
                    .and_then(|h| h.modifiers.get(col).copied())
                    .unwrap_or(Modifier::empty());

                let mut text = String::new();
                text.push(chars[col]);
                let mut next = col + 1;
                while next < max_col {
                    let nfg = hl
                        .and_then(|h| h.colors.get(next).copied())
                        .unwrap_or(Color::Reset);
                    let nmods = hl
                        .and_then(|h| h.modifiers.get(next).copied())
                        .unwrap_or(Modifier::empty());
                    if nfg == fg && nmods == mods {
                        text.push(chars[next]);
                        next += 1;
                    } else {
                        break;
                    }
                }

                let mut style = if fg == Color::Reset {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(fg)
                };
                if !mods.is_empty() {
                    style = style.add_modifier(mods);
                }
                if let Some(bg) = conflict_bg {
                    style = style.bg(bg);
                }
                spans.push(Span::styled(text, style));
                col = next;
            }

            let styled_line = ratatui::text::Line::from(spans);
            frame.render_widget(
                Paragraph::new(styled_line),
                Rect::new(inner.x, row_y, inner.width, 1),
            );
        }
    }

    // Draw conflict action bar above the active conflict in all visible panels.
    let view = match &app.merge_view {
        Some(v) => v,
        None => return,
    };
    let active = view.active_conflict;
    let is_unresolved =
        view.conflict_resolution(active) == crate::merge_view::Resolution::Unresolved;

    if is_unresolved {
        let action_text = " [1]Current  [2]Incoming  [3]Both(C+I)  [4]Both(I+C)  [n]Next ";
        let action_style = Style::default()
            .fg(Color::Black)
            .bg(Color::Rgb(200, 180, 80))
            .add_modifier(Modifier::BOLD);

        // Find the screen row of the active conflict's first line in each panel.
        // Result panel:
        let result_inner = Block::default().borders(Borders::ALL).inner(bottom_area);
        for (i, (_line, conflict_idx)) in result_lines.iter().skip(view.scroll_result).enumerate() {
            if i as u16 >= result_inner.height {
                break;
            }
            if let Some(idx) = conflict_idx {
                if *idx == active {
                    let row_y = result_inner.y + i as u16;
                    // Draw action bar one row above the conflict (or on top row if at top).
                    let bar_y = if row_y > result_inner.y {
                        row_y - 1
                    } else {
                        row_y
                    };
                    let bar_width = (action_text.len() as u16).min(result_inner.width);
                    frame.render_widget(
                        Paragraph::new(action_text).style(action_style),
                        Rect::new(result_inner.x, bar_y, bar_width, 1),
                    );
                    break;
                }
            }
        }

        // Incoming panel:
        let incoming_inner = Block::default().borders(Borders::ALL).inner(incoming_area);
        for (i, (_line, conflict_idx)) in
            incoming_lines.iter().skip(view.scroll_incoming).enumerate()
        {
            if i as u16 >= incoming_inner.height {
                break;
            }
            if let Some(idx) = conflict_idx {
                if *idx == active {
                    let row_y = incoming_inner.y + i as u16;
                    let bar_y = if row_y > incoming_inner.y {
                        row_y - 1
                    } else {
                        row_y
                    };
                    let bar_width = (action_text.len() as u16).min(incoming_inner.width);
                    frame.render_widget(
                        Paragraph::new(action_text).style(action_style),
                        Rect::new(incoming_inner.x, bar_y, bar_width, 1),
                    );
                    break;
                }
            }
        }

        // Current panel:
        let current_inner = Block::default().borders(Borders::ALL).inner(current_area);
        for (i, (_line, conflict_idx)) in current_lines.iter().skip(view.scroll_current).enumerate()
        {
            if i as u16 >= current_inner.height {
                break;
            }
            if let Some(idx) = conflict_idx {
                if *idx == active {
                    let row_y = current_inner.y + i as u16;
                    let bar_y = if row_y > current_inner.y {
                        row_y - 1
                    } else {
                        row_y
                    };
                    let bar_width = (action_text.len() as u16).min(current_inner.width);
                    frame.render_widget(
                        Paragraph::new(action_text).style(action_style),
                        Rect::new(current_inner.x, bar_y, bar_width, 1),
                    );
                    break;
                }
            }
        }
    }
}

/// Render lines for a merge panel (incoming or current) with conflict highlighting.
fn draw_merge_panel_lines(
    frame: &mut Frame,
    area: Rect,
    lines: &[(String, Option<usize>)],
    scroll: usize,
    active_conflict: usize,
    conflict_color: Color,
    highlight_lines: Option<&[crate::highlight::HighlightedLine]>,
) {
    for (i, (line, conflict_idx)) in lines.iter().skip(scroll).enumerate() {
        if i as u16 >= area.height {
            break;
        }
        let row_y = area.y + i as u16;
        let line_idx = scroll + i;

        // Determine conflict background (if any).
        let conflict_bg = if let Some(idx) = conflict_idx {
            if *idx == active_conflict {
                Some((Color::Rgb(60, 60, 20), true)) // bg, bold
            } else {
                let (r, g, b) = match conflict_color {
                    Color::Green => (20, 50, 20),
                    Color::Blue => (20, 20, 60),
                    _ => (40, 40, 40),
                };
                Some((Color::Rgb(r, g, b), false))
            }
        } else {
            None
        };

        // Get highlight data for this line.
        let hl = highlight_lines.and_then(|hls| hls.get(line_idx));

        // Build styled spans with syntax highlighting + conflict background.
        let chars: Vec<char> = line.chars().collect();
        let max_col = (area.width as usize).min(chars.len());
        let mut spans: Vec<Span> = Vec::new();
        let mut col = 0;

        while col < max_col {
            let fg = hl
                .and_then(|h| h.colors.get(col).copied())
                .unwrap_or(Color::Reset);
            let mods = hl
                .and_then(|h| h.modifiers.get(col).copied())
                .unwrap_or(Modifier::empty());

            let mut text = String::new();
            text.push(chars[col]);
            let mut next = col + 1;
            while next < max_col {
                let nfg = hl
                    .and_then(|h| h.colors.get(next).copied())
                    .unwrap_or(Color::Reset);
                let nmods = hl
                    .and_then(|h| h.modifiers.get(next).copied())
                    .unwrap_or(Modifier::empty());
                if nfg == fg && nmods == mods {
                    text.push(chars[next]);
                    next += 1;
                } else {
                    break;
                }
            }

            let mut style = if fg == Color::Reset {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(fg)
            };
            if !mods.is_empty() {
                style = style.add_modifier(mods);
            }
            if let Some((bg, bold)) = conflict_bg {
                style = style.bg(bg);
                if bold {
                    style = style.add_modifier(Modifier::BOLD);
                }
            }
            spans.push(Span::styled(text, style));
            col = next;
        }

        let styled_line = ratatui::text::Line::from(spans);
        frame.render_widget(
            Paragraph::new(styled_line),
            Rect::new(area.x, row_y, area.width, 1),
        );
    }
}

fn draw_diff_view(frame: &mut Frame, app: &mut App, area: Rect) {
    let dv = match app.tab().diff.as_ref() {
        Some(dv) => dv,
        None => return,
    };

    // Detect pure-addition / pure-deletion so we can collapse the empty pane.
    let has_left = dv
        .lines
        .iter()
        .any(|l| matches!(l, DiffLine::LeftOnly(_) | DiffLine::Both(_, _)));
    let has_right = dv
        .lines
        .iter()
        .any(|l| matches!(l, DiffLine::RightOnly(_) | DiffLine::Both(_, _)));

    // Split horizontally: panes + 1 column for minimap. Collapse a pane to 0
    // width when its side has no content (pure addition or pure deletion).
    let constraints: [Constraint; 3] = match (has_left, has_right) {
        (true, false) => [
            Constraint::Min(1),
            Constraint::Length(0),
            Constraint::Length(1),
        ],
        (false, true) => [
            Constraint::Length(0),
            Constraint::Min(1),
            Constraint::Length(1),
        ],
        _ => [
            Constraint::Percentage(50),
            Constraint::Percentage(50),
            Constraint::Length(1),
        ],
    };
    let hsplit = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
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

    if has_left || !has_right {
        frame.render_widget(left_block, hsplit[0]);
    }
    if has_right || !has_left {
        frame.render_widget(right_block, hsplit[1]);
    }

    let viewport_height = if has_right {
        right_inner.height
    } else {
        left_inner.height
    } as usize;

    // Update the diff view's scroll clamp with the actual viewport height.
    if let Some(dv) = app.tab_mut().diff.as_mut() {
        let max_scroll = dv.lines.len().saturating_sub(viewport_height);
        if dv.scroll > max_scroll {
            dv.scroll = max_scroll;
        }
    }

    let dv = match app.tab().diff.as_ref() {
        Some(dv) => dv,
        None => return,
    };

    let scroll = dv.scroll;
    // Cumulative old/new line counts at the scroll offset come from the
    // prefix sum cached on `DiffView::new`. The line-number gutter and the
    // highlight-array indices both advance the same way (Both → +1 to each;
    // LeftOnly → +1 to old; RightOnly → +1 to new), so a single pair
    // (old_n, new_n) drives both.
    let (mut old_line_no, mut new_line_no) = dv.line_numbers_at(scroll);
    let (mut old_hl_idx, mut new_hl_idx) = (old_line_no, new_line_no);

    let gutter_width: u16 = 5;

    // Build syntax-highlighted lines for both sides. The reconstructed
    // old/new text is cached on the DiffView (computed once in new()).
    let ext = dv.file_path.rsplit('.').next().unwrap_or("");
    let mut highlighter = crate::highlight::Language::from_extension(ext)
        .and_then(crate::highlight::SyntaxHighlighter::new);

    let old_hl = highlighter
        .as_mut()
        .map(|h| h.highlight(dv.old_text(), Some(&app.theme)))
        .unwrap_or_default();
    let new_hl = highlighter
        .as_mut()
        .map(|h| h.highlight(dv.new_text(), Some(&app.theme)))
        .unwrap_or_default();

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
    // Build diff line content for minimap preview.
    let diff_lines: Vec<String> = dv
        .lines
        .iter()
        .map(|line| match line {
            DiffLine::LeftOnly(s) | DiffLine::RightOnly(s) => s.clone(),
            DiffLine::Both(_, r) => r.clone(),
        })
        .collect();
    draw_minimap(
        frame,
        diff_minimap_area,
        &diff_markers,
        dv.lines.len(),
        scroll,
        viewport_height,
        &diff_lines,
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
        let pin = if tab.pinned { " " } else { "" };
        let label = if i < 9 {
            format!(" {pin}{}:{} ", i + 1, tab.title())
        } else {
            format!(" {pin}{} ", tab.title())
        };
        // Close button: "× " (× is a single-width Unicode char).
        let close_btn = "\u{00d7} ";
        let label_len = label.chars().count();
        let close_len = close_btn.chars().count(); // Display width, not byte length.
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
/// Draw a single terminal pane (used in split mode).
fn draw_single_terminal(frame: &mut Frame, app: &App, area: Rect, idx: usize, is_active: bool) {
    if idx >= app.terminals.len() {
        return;
    }
    let term = &app.terminals[idx];
    let label = if term.label.is_empty() {
        format!("Terminal {}", idx + 1)
    } else {
        term.label.clone()
    };
    let border_color = if is_active && app.terminal_focused {
        Color::Yellow
    } else {
        Color::DarkGray
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", label))
        .border_style(Style::default().fg(border_color));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let (snapshot, cursor_row, cursor_col) = term.snapshot();
    let h = inner.height as usize;
    let w = inner.width as usize;

    for (row_idx, row) in snapshot.iter().take(h).enumerate() {
        let y = inner.y + row_idx as u16;
        let line_text: String = row.iter().map(|cell| cell.ch).take(w).collect();
        frame.render_widget(
            Paragraph::new(line_text).style(Style::default().fg(Color::White)),
            Rect::new(inner.x, y, inner.width, 1),
        );
    }

    // Draw cursor if this is the active pane.
    if is_active && app.terminal_focused && cursor_row < h && cursor_col < w {
        let cursor_y = inner.y + cursor_row as u16;
        let cursor_x = inner.x + cursor_col as u16;
        frame.render_widget(
            Paragraph::new(" ").style(Style::default().bg(Color::White).fg(Color::Black)),
            Rect::new(cursor_x, cursor_y, 1, 1),
        );
    }
}

fn draw_terminal(frame: &mut Frame, app: &App, area: Rect) {
    // If split mode, draw two terminals side by side.
    if app.terminal_split && app.terminals.len() >= 2 {
        let hsplit = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        draw_single_terminal(frame, app, hsplit[0], app.active_terminal, true);
        draw_single_terminal(frame, app, hsplit[1], app.terminal_split_idx, false);
        return;
    }

    let focused = app.terminal_focused;
    let multi_tab = app.terminals.len() > 1;

    // Build exit code indicator from last command.
    let last_exit = {
        let cmds = app.terminal().commands();
        if let Some(last) = cmds.last() {
            match last.exit_code {
                Some(0) => " [ok] ".to_string(),
                Some(code) => format!(" [exit {code}] "),
                None => " [running] ".to_string(),
            }
        } else {
            String::new()
        }
    };

    let title = if multi_tab {
        // Build tab bar: [1: zsh] [2: cargo] ...
        let mut tabs = String::from(" ");
        for (i, t) in app.terminals.iter().enumerate() {
            let label = if t.label.is_empty() {
                format!("{}", i + 1)
            } else {
                t.label.clone()
            };
            if i == app.active_terminal {
                tabs.push_str(&format!("[{label}] "));
            } else {
                tabs.push_str(&format!(" {label}  "));
            }
        }
        format!("{tabs}{last_exit}")
    } else if focused {
        let offset = app.terminal().scroll_offset();
        if offset > 0 {
            format!(" Terminal (scrollback){last_exit}")
        } else {
            format!(" Terminal (focused){last_exit}")
        }
    } else {
        format!(" Terminal{last_exit}")
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
    let (snapshot, cursor_row, cursor_col) = app.terminal().snapshot();

    for (row_idx, row) in snapshot.iter().enumerate() {
        let y = inner.y + row_idx as u16;
        if y >= inner.y + inner.height {
            break;
        }

        // Build styled spans for this row.
        let mut spans: Vec<Span> = Vec::new();
        let max_col = (inner.width as usize).min(row.len());

        // Check if this row is within the terminal selection range.
        let selection = app.terminal().selection_range();

        let mut col = 0;
        while col < max_col {
            // Skip continuation cells (spacers for wide characters).
            if row[col].continuation {
                col += 1;
                continue;
            }
            // Group consecutive cells with the same style.
            let cell = &row[col];
            let raw_fg = term_color_to_ratatui(cell.fg, Color::White);
            let raw_bg = term_color_to_ratatui(cell.bg, Color::Reset);
            // Apply reverse video: swap fg/bg.
            let (fg, bg) = if cell.reverse {
                (raw_bg, raw_fg)
            } else {
                (raw_fg, raw_bg)
            };
            let bold = cell.bold;
            let dim = cell.dim;
            let italic = cell.italic;
            let underline = cell.underline;
            let strikethrough = cell.strikethrough;

            // Check if this cell is inside the selection.
            let in_selection = if let Some((sr, sc, er, ec)) = selection {
                if row_idx > sr && row_idx < er {
                    true
                } else if row_idx == sr && row_idx == er {
                    col >= sc && col <= ec
                } else if row_idx == sr {
                    col >= sc
                } else if row_idx == er {
                    col <= ec
                } else {
                    false
                }
            } else {
                false
            };

            let mut text = String::new();
            text.push(cell.ch);

            let mut next = col + 1;
            while next < max_col {
                // Skip continuation cells (wide char spacers).
                if row[next].continuation {
                    next += 1;
                    continue;
                }
                let nc = &row[next];
                let nfg = term_color_to_ratatui(nc.fg, Color::White);
                let nbg = term_color_to_ratatui(nc.bg, Color::Reset);
                let (nfg, nbg) = if nc.reverse { (nbg, nfg) } else { (nfg, nbg) };
                // Check if next cell has the same selection state.
                let next_in_sel = if let Some((sr, sc, er, ec)) = selection {
                    if row_idx > sr && row_idx < er {
                        true
                    } else if row_idx == sr && row_idx == er {
                        next >= sc && next <= ec
                    } else if row_idx == sr {
                        next >= sc
                    } else if row_idx == er {
                        next <= ec
                    } else {
                        false
                    }
                } else {
                    false
                };
                if nfg == fg
                    && nbg == bg
                    && nc.bold == bold
                    && nc.dim == dim
                    && nc.italic == italic
                    && nc.underline == underline
                    && nc.strikethrough == strikethrough
                    && next_in_sel == in_selection
                {
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
            if dim {
                style = style.add_modifier(Modifier::DIM);
            }
            if italic {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if underline {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            if strikethrough {
                style = style.add_modifier(Modifier::CROSSED_OUT);
            }
            // Highlight selected cells.
            if in_selection {
                style = style.bg(Color::Indexed(237)).fg(Color::White);
            }
            // Underline detected file links.
            let in_link = app
                .terminal_links
                .iter()
                .any(|(lr, lcs, lce, _, _, _)| *lr == row_idx && col < *lce && next > *lcs);
            if in_link {
                style = style.fg(Color::Cyan).add_modifier(Modifier::UNDERLINED);
            }

            // Show cursor as reversed when terminal is focused.
            if focused
                && row_idx == cursor_row
                && col <= cursor_col
                && cursor_col < next
                && app.terminal().scroll_offset() == 0
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

        // Append AI suggestion ghost text on the cursor row.
        if focused && row_idx == cursor_row && app.terminal().scroll_offset() == 0 {
            if let Some(ref suggestion) = app.terminal_suggestion {
                let avail = (inner.width as usize).saturating_sub(col);
                let ghost: String = suggestion.chars().take(avail).collect();
                if !ghost.is_empty() {
                    spans.push(Span::styled(ghost, Style::default().fg(Color::DarkGray)));
                }
            }
        }

        let line = ratatui::text::Line::from(spans);
        frame.render_widget(Paragraph::new(line), Rect::new(inner.x, y, inner.width, 1));
    }
}

/// Draw a read-only shared terminal received from the collab host.
fn draw_shared_terminal(frame: &mut Frame, app: &App, area: Rect) {
    let snapshot = match &app.collab_shared_terminal {
        Some(s) => s,
        None => return,
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Host Terminal (read-only) ")
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    for (row_idx, row) in snapshot.cells.iter().enumerate() {
        let y = inner.y + row_idx as u16;
        if y >= inner.y + inner.height {
            break;
        }

        let mut spans: Vec<Span> = Vec::new();
        let max_col = (inner.width as usize).min(row.len());

        let mut col = 0;
        while col < max_col {
            if row[col].continuation {
                col += 1;
                continue;
            }
            let cell = &row[col];
            let fg = term_color_to_ratatui(cell.fg, Color::White);
            let bg = term_color_to_ratatui(cell.bg, Color::Reset);
            let bold = cell.bold;

            let mut text = String::new();
            text.push(cell.ch);

            let mut next = col + 1;
            while next < max_col {
                if row[next].continuation {
                    next += 1;
                    continue;
                }
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
            spans.push(Span::styled(text, style));

            col = next;
        }

        let line = ratatui::text::Line::from(spans);
        frame.render_widget(Paragraph::new(line), Rect::new(inner.x, y, inner.width, 1));
    }
}

/// Draw the file tree sidebar.
fn draw_file_tree(frame: &mut Frame, app: &mut App, area: Rect) {
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

    // Action bar row with clickable icons.
    if inner.height > 2 {
        let action_bar = Line::from(vec![
            Span::styled(
                " + ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("◻ ", Style::default().fg(Color::Cyan)),
            Span::styled("✎ ", Style::default().fg(Color::Yellow)),
            Span::styled("✕ ", Style::default().fg(Color::Red)),
            Span::styled("⧉ ", Style::default().fg(Color::Magenta)),
            Span::styled("⟳ ", Style::default().fg(Color::Blue)),
            Span::styled("⊙ ", Style::default().fg(Color::DarkGray)),
        ]);
        frame.render_widget(
            Paragraph::new(action_bar),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
    }

    // Adjust inner area below the tab header + action bar.
    let tree_inner = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(2),
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

    // Adjust scroll only if the selected entry is off-screen.
    app.file_tree.ensure_visible(visible_height);
    let scroll_offset = app.file_tree.scroll_offset;

    let entries = app
        .file_tree
        .entries
        .iter()
        .skip(scroll_offset)
        .take(visible_height);
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    let dir_style = Style::default().fg(app.theme.function);
    let file_style = Style::default().fg(app.theme.fg);

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
/// Draw the AI Visor panel (Claude Code config browser).
fn draw_ai_visor(frame: &mut Frame, app: &App, area: Rect) {
    use crate::ai_visor::VisorTab;

    let visor = &app.ai_visor;
    let focused = app.ai_visor_focused;
    let border_color = if focused {
        Color::Yellow
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" AI Visor ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 3 || inner.width < 10 {
        return;
    }

    // Tab bar.
    let tab_row = Rect::new(inner.x, inner.y, inner.width, 1);
    let content = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1),
    );

    let tabs = [
        ("1:Overview", VisorTab::Overview),
        ("2:Settings", VisorTab::Settings),
        ("3:Skills", VisorTab::Skills),
        ("4:Hooks", VisorTab::Hooks),
        ("5:Plugins", VisorTab::Plugins),
        ("6:Agents", VisorTab::Agents),
    ];
    let mut tab_spans = Vec::new();
    for (label, tab) in &tabs {
        let style = if *tab == visor.active_tab {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        tab_spans.push(Span::styled(format!(" {} ", label), style));
    }
    frame.render_widget(Paragraph::new(Line::from(tab_spans)), tab_row);

    let w = content.width as usize;

    match visor.active_tab {
        VisorTab::Overview => {
            let s = &visor.sections;
            let mut lines = Vec::new();
            lines.push((
                "Model",
                s.model.as_deref().unwrap_or("(default)").to_string(),
            ));
            lines.push((
                "Effort",
                s.effort.as_deref().unwrap_or("(default)").to_string(),
            ));
            lines.push(("", String::new()));
            let md_status = if s.claude_md.is_some() {
                format!("✓ ({:.1} KB)", s.claude_md_size as f64 / 1024.0)
            } else {
                "✗ not found".to_string()
            };
            lines.push(("CLAUDE.md", md_status));
            lines.push(("Settings", format!("{} entries", s.settings.len())));
            lines.push(("Skills", format!("{} available", s.skills.len())));
            lines.push(("Hooks", format!("{} configured", s.hooks.len())));
            lines.push(("Plugins", format!("{} installed", s.plugins.len())));
            lines.push(("Agents", format!("{} discovered", s.agents.len())));
            lines.push(("", String::new()));
            lines.push((
                "Permissions",
                format!("{} allowed", s.permissions_allow_count),
            ));
            lines.push(("Rules", format!("{} files", s.rules_count)));
            lines.push(("Docs", format!("{} indexed", s.docs_count)));

            for (i, (label, value)) in lines.iter().enumerate() {
                if i as u16 >= content.height {
                    break;
                }
                let row_y = content.y + i as u16;
                if label.is_empty() {
                    continue;
                }
                let text = format!(" {:<14} {}", label, value);
                let style = if i == 0 || i == 1 {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::White)
                };
                frame.render_widget(
                    Paragraph::new(text).style(style),
                    Rect::new(content.x, row_y, content.width, 1),
                );
            }
        }
        VisorTab::Settings => {
            for (i, entry) in visor.sections.settings.iter().enumerate() {
                if i as u16 >= content.height {
                    break;
                }
                let row_y = content.y + i as u16;
                let is_selected = focused && i == visor.selected;
                let scope_color = match entry.scope.as_str() {
                    "G" => Color::Blue,
                    "P" => Color::Green,
                    "L" => Color::Yellow,
                    _ => Color::White,
                };
                let text = format!(" [{}] {}: {}", entry.scope, entry.key, entry.value);
                let text: String = text.chars().take(w).collect();
                let style = if is_selected {
                    Style::default().fg(Color::Black).bg(scope_color)
                } else {
                    Style::default().fg(scope_color)
                };
                frame.render_widget(
                    Paragraph::new(text).style(style),
                    Rect::new(content.x, row_y, content.width, 1),
                );
            }
            if visor.sections.settings.is_empty() {
                frame.render_widget(
                    Paragraph::new("  No settings found")
                        .style(Style::default().fg(Color::DarkGray)),
                    Rect::new(content.x, content.y, content.width, 1),
                );
            }
        }
        VisorTab::Skills => {
            let mut y = content.y;
            for (i, skill) in visor.sections.skills.iter().enumerate() {
                if y >= content.y + content.height {
                    break;
                }
                let is_selected = focused && i == visor.selected;
                let prefix = if skill.invocable { "▶" } else { "○" };
                let name_line = format!(" {} {}", prefix, skill.name);
                let desc_line = format!("   {}", skill.description);

                let name_style = if is_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                };
                let desc_style = if is_selected {
                    Style::default().fg(Color::Black).bg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                frame.render_widget(
                    Paragraph::new(name_line.chars().take(w).collect::<String>()).style(name_style),
                    Rect::new(content.x, y, content.width, 1),
                );
                y += 1;
                if y < content.y + content.height {
                    frame.render_widget(
                        Paragraph::new(desc_line.chars().take(w).collect::<String>())
                            .style(desc_style),
                        Rect::new(content.x, y, content.width, 1),
                    );
                    y += 1;
                }
            }
            if visor.sections.skills.is_empty() {
                frame.render_widget(
                    Paragraph::new("  No skills found").style(Style::default().fg(Color::DarkGray)),
                    Rect::new(content.x, content.y, content.width, 1),
                );
            }
        }
        VisorTab::Hooks => {
            for (i, hook) in visor.sections.hooks.iter().enumerate() {
                if (i * 2) as u16 >= content.height {
                    break;
                }
                let is_selected = focused && i == visor.selected;
                let event_line = format!(" {} ({})", hook.event, hook.hook_type);
                let cmd_line = format!("   {}", hook.command);

                let event_style = if is_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Magenta)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Magenta)
                };
                let cmd_style = Style::default().fg(Color::DarkGray);

                let y = content.y + (i * 2) as u16;
                frame.render_widget(
                    Paragraph::new(event_line.chars().take(w).collect::<String>())
                        .style(event_style),
                    Rect::new(content.x, y, content.width, 1),
                );
                if y + 1 < content.y + content.height {
                    frame.render_widget(
                        Paragraph::new(cmd_line.chars().take(w).collect::<String>())
                            .style(cmd_style),
                        Rect::new(content.x, y + 1, content.width, 1),
                    );
                }
            }
            if visor.sections.hooks.is_empty() {
                frame.render_widget(
                    Paragraph::new("  No hooks configured")
                        .style(Style::default().fg(Color::DarkGray)),
                    Rect::new(content.x, content.y, content.width, 1),
                );
            }
        }
        VisorTab::Plugins => {
            for (i, plugin) in visor.sections.plugins.iter().enumerate() {
                if (i * 2) as u16 >= content.height {
                    break;
                }
                let is_selected = focused && i == visor.selected;
                let name_line = format!(" ✓ {}", plugin.name);
                let source_line = if plugin.source.is_empty() {
                    String::new()
                } else {
                    format!("   {}", plugin.source)
                };

                let name_style = if is_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Cyan)
                };

                let y = content.y + (i * 2) as u16;
                frame.render_widget(
                    Paragraph::new(name_line.chars().take(w).collect::<String>()).style(name_style),
                    Rect::new(content.x, y, content.width, 1),
                );
                if !source_line.is_empty() && y + 1 < content.y + content.height {
                    frame.render_widget(
                        Paragraph::new(source_line.chars().take(w).collect::<String>())
                            .style(Style::default().fg(Color::DarkGray)),
                        Rect::new(content.x, y + 1, content.width, 1),
                    );
                }
            }
            if visor.sections.plugins.is_empty() {
                frame.render_widget(
                    Paragraph::new("  No plugins installed")
                        .style(Style::default().fg(Color::DarkGray)),
                    Rect::new(content.x, content.y, content.width, 1),
                );
            }
        }
        VisorTab::Agents => {
            let mut y = content.y;
            for (i, agent) in visor.sections.agents.iter().enumerate() {
                if y >= content.y + content.height {
                    break;
                }
                let is_selected = focused && i == visor.selected;
                let scope_tag = match agent.scope.as_str() {
                    "project" => "P",
                    "global" => "G",
                    _ => "?",
                };
                let name_line = format!(" [{}] {}", scope_tag, agent.name);
                let desc_line = format!("     {}", agent.description);

                let scope_color = match agent.scope.as_str() {
                    "project" => Color::Green,
                    "global" => Color::Blue,
                    _ => Color::White,
                };
                let name_style = if is_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(scope_color)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(scope_color)
                        .add_modifier(Modifier::BOLD)
                };
                let desc_style = if is_selected {
                    Style::default().fg(Color::Black).bg(scope_color)
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                frame.render_widget(
                    Paragraph::new(name_line.chars().take(w).collect::<String>()).style(name_style),
                    Rect::new(content.x, y, content.width, 1),
                );
                y += 1;
                if y < content.y + content.height && !agent.description.is_empty() {
                    frame.render_widget(
                        Paragraph::new(desc_line.chars().take(w).collect::<String>())
                            .style(desc_style),
                        Rect::new(content.x, y, content.width, 1),
                    );
                    y += 1;
                }
            }
            if visor.sections.agents.is_empty() {
                frame.render_widget(
                    Paragraph::new("  No agents found").style(Style::default().fg(Color::DarkGray)),
                    Rect::new(content.x, content.y, content.width, 1),
                );
                frame.render_widget(
                    Paragraph::new("  .claude/agents/*.md")
                        .style(Style::default().fg(Color::Rgb(80, 80, 80))),
                    Rect::new(content.x, content.y + 1, content.width, 1),
                );
            }
        }
    }
}

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
                    wrapped_lines.push((
                        ChatRole::System,
                        "   ┌─────────────────────────────┐".to_string(),
                        Some(Color::Yellow),
                    ));
                    wrapped_lines.push((
                        ChatRole::System,
                        "   │  Allow? [Y]es / [N]o / Esc  │".to_string(),
                        Some(Color::Yellow),
                    ));
                    wrapped_lines.push((
                        ChatRole::System,
                        "   └─────────────────────────────┘".to_string(),
                        Some(Color::Yellow),
                    ));
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

    // ── Render @-mention autocomplete dropdown if active ──
    if app.chat_panel.mention_active && !app.chat_panel.mention_matches.is_empty() {
        let matches = &app.chat_panel.mention_matches;
        let max_visible = 8.min(matches.len());
        let popup_height = max_visible as u16 + 2; // +2 for border
        let popup_width = area.width.saturating_sub(2).min(40);
        let popup_y = input_area.y.saturating_sub(popup_height);
        let popup_x = area.x + 1;
        let popup = Rect::new(popup_x, popup_y, popup_width, popup_height);

        frame.render_widget(Clear, popup);
        let query_display = format!(" @{} ", app.chat_panel.mention_query);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(query_display);
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        for (i, item) in matches.iter().take(max_visible).enumerate() {
            let row_y = inner.y + i as u16;
            let is_selected = i == app.chat_panel.mention_selected;
            let label = item.label();
            let is_special = matches!(
                item,
                crate::chat_panel::MentionItem::Selection
                    | crate::chat_panel::MentionItem::Buffer
                    | crate::chat_panel::MentionItem::Diagnostics
            );

            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if is_special {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::White)
            };

            let display: String = label.chars().take(inner.width as usize).collect();
            frame.render_widget(
                Paragraph::new(format!(" {display}")).style(style),
                Rect::new(inner.x, row_y, inner.width, 1),
            );
        }
    }
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

    // --- Merge changes (conflicts) ---
    if !sc.merge_changes.is_empty() {
        let merge_focused = sc.focused_section == GitPanelSection::MergeChanges;
        if y < max_y {
            let header_style = if merge_focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD)
            };
            let header = format!(" Merge Changes ({})", sc.merge_changes.len());
            frame.render_widget(
                Paragraph::new(Span::styled(header, header_style)),
                Rect::new(inner.x, y, inner.width, 1),
            );
            y += 1;
        }

        for (i, entry) in sc.merge_changes.iter().enumerate() {
            if y >= max_y {
                break;
            }
            let is_selected = merge_focused && i == sc.selected;
            let (filename, dir) = split_path_filename(&entry.rel_path);
            let line =
                format_git_entry(entry.status.label(), &filename, &dir, inner.width as usize);

            if is_selected {
                let style = Style::default().add_modifier(Modifier::REVERSED);
                frame.render_widget(
                    Paragraph::new(line).style(style),
                    Rect::new(inner.x, y, inner.width, 1),
                );
            } else {
                let style = Style::default().fg(Color::Magenta);
                frame.render_widget(
                    Paragraph::new(line).style(style),
                    Rect::new(inner.x, y, inner.width, 1),
                );
            }
            y += 1;
        }

        // Blank separator.
        if y < max_y {
            y += 1;
        }
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

    // --- Stashes ---
    if !sc.stashes.is_empty() {
        // Blank separator.
        if y < max_y {
            y += 1;
        }

        let stash_focused = sc.focused_section == GitPanelSection::Stashes;
        if y < max_y {
            let header_style = if stash_focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };
            let header = format!(" Stashes ({})", sc.stashes.len());
            frame.render_widget(
                Paragraph::new(Span::styled(header, header_style)),
                Rect::new(inner.x, y, inner.width, 1),
            );
            y += 1;
        }

        for (i, stash) in sc.stashes.iter().enumerate() {
            if y >= max_y {
                break;
            }
            let is_selected = stash_focused && i == sc.selected;
            let display: String = format!(" {} {}", stash.name, stash.message)
                .chars()
                .take(inner.width as usize)
                .collect();

            if is_selected {
                let style = Style::default().add_modifier(Modifier::REVERSED);
                frame.render_widget(
                    Paragraph::new(display).style(style),
                    Rect::new(inner.x, y, inner.width, 1),
                );
            } else {
                let style = Style::default().fg(Color::Cyan);
                frame.render_widget(
                    Paragraph::new(display).style(style),
                    Rect::new(inner.x, y, inner.width, 1),
                );
            }
            y += 1;
        }
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
/// Draw the project-wide search/replace panel (full-screen overlay).
fn draw_project_search(frame: &mut Frame, app: &App, area: Rect) {
    use crate::project_search::SearchFocus;

    let panel = &app.project_search;
    let width = area.width.saturating_sub(6).min(100);
    let height = area.height.saturating_sub(4).min(40);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup);

    let title = format!(
        " Project Search — {} match(es) in {} file(s) ",
        panel.total_matches, panel.file_count
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.height < 4 || inner.width < 20 {
        return;
    }

    let mut row_y = inner.y;

    // Search input line.
    {
        let focus_indicator = if panel.focus == SearchFocus::Query {
            ">"
        } else {
            " "
        };
        let case_flag = if panel.case_sensitive {
            Span::styled(" Aa ", Style::default().fg(Color::Black).bg(Color::Green))
        } else {
            Span::styled(" Aa ", Style::default().fg(Color::DarkGray))
        };
        let query_text = format!("{} Search: {}", focus_indicator, panel.query);
        let mut spans = vec![Span::styled(
            query_text,
            Style::default().fg(if panel.focus == SearchFocus::Query {
                Color::Yellow
            } else {
                Color::White
            }),
        )];
        // Pad to push flags to the right.
        let used = spans.iter().map(|s| s.content.len()).sum::<usize>();
        let pad = (inner.width as usize).saturating_sub(used + 5);
        spans.push(Span::raw(" ".repeat(pad)));
        spans.push(case_flag);

        frame.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect::new(inner.x, row_y, inner.width, 1),
        );
        row_y += 1;
    }

    // Replace input line (if replace mode active).
    if panel.replace_mode {
        let focus_indicator = if panel.focus == SearchFocus::Replace {
            ">"
        } else {
            " "
        };
        let text = format!("{} Replace: {}", focus_indicator, panel.replace_text);
        let style = if panel.focus == SearchFocus::Replace {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        frame.render_widget(
            Paragraph::new(text).style(style),
            Rect::new(inner.x, row_y, inner.width, 1),
        );
        row_y += 1;
    }

    // Separator.
    let sep: String = "─".repeat(inner.width as usize);
    frame.render_widget(
        Paragraph::new(sep).style(Style::default().fg(Color::DarkGray)),
        Rect::new(inner.x, row_y, inner.width, 1),
    );
    row_y += 1;

    // Results list.
    let list_height = (inner.y + inner.height).saturating_sub(row_y + 1) as usize; // -1 for footer
    let mut current_file = String::new();
    let selected = panel.selected;

    // Compute scroll to keep selected visible.
    let scroll = if selected >= list_height {
        selected.saturating_sub(list_height / 2)
    } else {
        0
    };

    let mut rendered = 0usize;
    for (i, result) in panel.results.iter().enumerate().skip(scroll) {
        if rendered >= list_height {
            break;
        }

        // File header when file changes.
        if result.file_path != current_file {
            current_file = result.file_path.clone();
            if rendered > 0 && rendered < list_height {
                // Blank line between files.
                row_y += 1;
                rendered += 1;
                if rendered >= list_height {
                    break;
                }
            }
            let header_style = Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD);
            let display: String = current_file.chars().take(inner.width as usize).collect();
            frame.render_widget(
                Paragraph::new(display).style(header_style),
                Rect::new(inner.x, row_y, inner.width, 1),
            );
            row_y += 1;
            rendered += 1;
            if rendered >= list_height {
                break;
            }
        }

        let is_selected = i == selected;
        let prefix = format!("  {}:{} ", result.line_number, result.column);

        // Truncate line text.
        let max_text = (inner.width as usize).saturating_sub(prefix.len());
        let line_text = result.line_text.trim();
        let line_display: String = line_text.chars().take(max_text).collect();

        let sel_bg = if is_selected {
            Some(Color::Yellow)
        } else {
            None
        };

        // Try syntax highlighting for the result line.
        let ext = std::path::Path::new(&result.file_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let hl_spans = crate::highlight::Language::from_extension(ext)
            .and_then(crate::highlight::SyntaxHighlighter::new)
            .map(|mut hl| hl.highlight(&line_display, Some(&app.theme)));

        let prefix_style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let mut spans = vec![Span::styled(prefix.clone(), prefix_style)];

        if let Some(hl_lines) = hl_spans {
            if let Some(hl_line) = hl_lines.first() {
                // Build spans from per-char colors.
                let chars: Vec<char> = line_display.chars().collect();
                let mut run_start = 0;
                let mut run_color = hl_line.colors.first().copied().unwrap_or(Color::White);
                for (j, &color) in hl_line.colors.iter().enumerate() {
                    if color != run_color || j >= chars.len() {
                        let text: String = chars[run_start..j.min(chars.len())].iter().collect();
                        if !text.is_empty() {
                            let mut style = Style::default().fg(run_color);
                            if let Some(bg) = sel_bg {
                                style = style.bg(bg);
                            }
                            spans.push(Span::styled(text, style));
                        }
                        run_start = j;
                        run_color = color;
                    }
                }
                let text: String = chars[run_start..].iter().collect();
                if !text.is_empty() {
                    let mut style = Style::default().fg(run_color);
                    if let Some(bg) = sel_bg {
                        style = style.bg(bg);
                    }
                    spans.push(Span::styled(text, style));
                }
            }
        } else {
            let style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default().fg(Color::White)
            };
            spans.push(Span::styled(line_display, style));
        }

        frame.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect::new(inner.x, row_y, inner.width, 1),
        );
        row_y += 1;
        rendered += 1;
    }

    if panel.results.is_empty() && !panel.query.is_empty() {
        frame.render_widget(
            Paragraph::new("  No matches found").style(Style::default().fg(Color::DarkGray)),
            Rect::new(inner.x, row_y, inner.width, 1),
        );
    }

    // Footer hint line.
    let footer_y = inner.y + inner.height.saturating_sub(1);
    let hint = if panel.replace_mode {
        " Enter:Go  Tab:Switch  Ctrl+R:Replace  R:Replace All  Esc:Close"
    } else {
        " Enter:Go  Tab:Switch  Ctrl+R:Replace Mode  Esc:Close"
    };
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
        Rect::new(inner.x, footer_y, inner.width, 1),
    );
}

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
/// Draw the undo tree visualization modal (two-panel: list + detail).
/// Draw the document outline modal (symbol list for current file).
/// Draw the registers modal — shows yank register and macro registers.
fn draw_registers_modal(frame: &mut Frame, app: &App, area: Rect) {
    let width = area.width.saturating_sub(6).min(80);
    let height = area.height.saturating_sub(4).min(30);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup);

    if let Some(editing_ch) = app.macro_editing {
        // Macro editing sub-view.
        let keys = app.macro_registers.get(&editing_ch);
        let count = keys.map_or(0, |k| k.len());
        let title = format!(" Edit Macro @{editing_ch} ({count} keys) — d:delete  Esc:back ");
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(title);
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        if let Some(keys) = keys {
            let list_h = inner.height as usize;
            let selected = app.macro_edit_selected;
            let scroll = if selected >= list_h {
                selected.saturating_sub(list_h / 2)
            } else {
                0
            };

            for (vis_idx, key) in keys.iter().enumerate().skip(scroll).take(list_h) {
                let row_y = inner.y + (vis_idx - scroll) as u16;
                if row_y >= inner.y + inner.height {
                    break;
                }
                let is_selected = vis_idx == selected;
                let key_str = crate::app::format_key_event(key);
                let display = format!("  {:>3}  {}", vis_idx + 1, key_str);
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                frame.render_widget(
                    Paragraph::new(display).style(style),
                    Rect::new(inner.x, row_y, inner.width, 1),
                );
            }

            if keys.is_empty() {
                frame.render_widget(
                    Paragraph::new("  (empty)").style(Style::default().fg(Color::DarkGray)),
                    Rect::new(inner.x, inner.y, inner.width, 1),
                );
            }
        }
    } else {
        // Register list view.
        let entries = app.register_entries();
        let count = entries.len();
        let title = format!(" Registers ({count}) — e:edit  q:close ");
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(title);
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        if inner.height < 2 {
            return;
        }

        // Header row.
        let header = format!("  {:<5} {}", "Reg", "Content");
        frame.render_widget(
            Paragraph::new(header).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );

        let list_y = inner.y + 1;
        let list_h = inner.height.saturating_sub(1) as usize;
        let selected = app.registers_selected;
        let scroll = if selected >= list_h {
            selected.saturating_sub(list_h / 2)
        } else {
            0
        };

        for (vis_idx, (name, preview)) in entries.iter().enumerate().skip(scroll).take(list_h) {
            let row_y = list_y + (vis_idx - scroll) as u16;
            if row_y >= inner.y + inner.height {
                break;
            }
            let is_selected = vis_idx == selected;
            let max_preview = (inner.width as usize).saturating_sub(8);
            let truncated: String = preview.chars().take(max_preview).collect();
            let is_macro =
                name.len() == 1 && name.chars().next().is_some_and(|c| c.is_ascii_lowercase());
            let prefix = if is_macro { "@" } else { " " };
            let display = format!("  {}{:<4} {}", prefix, name, truncated);
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if is_macro {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::White)
            };
            frame.render_widget(
                Paragraph::new(display).style(style),
                Rect::new(inner.x, row_y, inner.width, 1),
            );
        }

        if entries.is_empty() {
            frame.render_widget(
                Paragraph::new("  No registers in use").style(Style::default().fg(Color::DarkGray)),
                Rect::new(inner.x, list_y, inner.width, 1),
            );
        }
    }
}

fn draw_outline_modal(frame: &mut Frame, app: &App, area: Rect) {
    let width = area.width.saturating_sub(6).min(80);
    let height = area.height.saturating_sub(4).min(30);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup);

    let count = app.outline_filtered.len();
    let title = format!(" Document Outline ({count} symbols) ");
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.height < 3 {
        return;
    }

    // Query input.
    let query_text = format!("> {}", app.outline_query);
    frame.render_widget(
        Paragraph::new(query_text).style(Style::default().fg(Color::Yellow)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    // Results list.
    let list_y = inner.y + 1;
    let list_h = inner.height.saturating_sub(1) as usize;
    let selected = app.outline_selected;
    let scroll = if selected >= list_h {
        selected.saturating_sub(list_h / 2)
    } else {
        0
    };

    for (vis_idx, &item_idx) in app
        .outline_filtered
        .iter()
        .enumerate()
        .skip(scroll)
        .take(list_h)
    {
        let row_y = list_y + (vis_idx - scroll) as u16;
        if row_y >= inner.y + inner.height {
            break;
        }
        let is_selected = vis_idx == selected;
        if let Some((line, label)) = app.outline_items.get(item_idx) {
            let prefix = format!("{:>4}: ", line + 1);
            let display: String = label
                .chars()
                .take((inner.width as usize).saturating_sub(7))
                .collect();
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            frame.render_widget(
                Paragraph::new(format!("{prefix}{display}")).style(style),
                Rect::new(inner.x, row_y, inner.width, 1),
            );
        }
    }

    if app.outline_filtered.is_empty() {
        frame.render_widget(
            Paragraph::new("  No symbols found").style(Style::default().fg(Color::DarkGray)),
            Rect::new(inner.x, list_y, inner.width, 1),
        );
    }
}

fn draw_undo_tree_modal(frame: &mut Frame, app: &App, area: Rect) {
    let modal = match &app.undo_tree {
        Some(m) => m,
        None => return,
    };

    let width = area.width.saturating_sub(4).min(120);
    let height = area.height.saturating_sub(4).min(40);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup);

    let title = format!(" Undo History — {} edits ", modal.entries.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    if inner.height < 2 || inner.width < 20 {
        return;
    }

    // Split into list (65%) and detail (35%) if detail is shown.
    let (list_area, detail_area) = if modal.show_detail && inner.width > 50 {
        let hsplit = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(inner);
        (hsplit[0], Some(hsplit[1]))
    } else {
        (inner, None)
    };

    // --- Left panel: entry list ---
    for (i, entry) in modal.entries.iter().enumerate() {
        if i as u16 >= list_area.height {
            break;
        }
        let row_y = list_area.y + i as u16;
        let is_selected = i == modal.selected;

        // Build the line.
        let marker = if entry.is_current { "→" } else { " " };
        let redo_suffix = if entry.is_redo { " (redo)" } else { "" };
        let text = format!(
            "{} #{:<3} [{}]  {}  {}{}",
            marker, entry.index, entry.author_label, entry.kind_label, entry.timestamp, redo_suffix
        );
        let text = if text.len() > list_area.width as usize {
            text[..list_area.width as usize].to_string()
        } else {
            text
        };

        let style = if is_selected {
            Style::default()
                .fg(Color::Black)
                .bg(entry.author_color)
                .add_modifier(Modifier::BOLD)
        } else if entry.is_current {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else if entry.is_redo {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(entry.author_color)
        };

        frame.render_widget(
            Paragraph::new(text).style(style),
            Rect::new(list_area.x, row_y, list_area.width, 1),
        );
    }

    // --- Right panel: detail ---
    if let Some(detail) = detail_area {
        let detail_block = Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Detail ");
        let detail_inner = detail_block.inner(detail);
        frame.render_widget(detail_block, detail);

        if let Some(entry) = modal.entries.get(modal.selected) {
            let mut lines = vec![
                format!("Edit #{}", entry.index),
                format!("Author: {}", entry.author_label),
                format!("Action: {}", entry.kind_label),
                format!("Position: char {}", entry.position),
                format!("Time: {}", entry.timestamp),
                String::new(),
                "Preview:".to_string(),
            ];
            // Wrap preview text.
            let preview = &entry.preview;
            let w = detail_inner.width as usize;
            for chunk in preview.as_bytes().chunks(w.max(1)) {
                if let Ok(s) = std::str::from_utf8(chunk) {
                    lines.push(s.to_string());
                }
            }
            if entry.is_current {
                lines.push(String::new());
                lines.push("◆ Current position".to_string());
            }
            if entry.is_redo {
                lines.push(String::new());
                lines.push("↻ Available for redo".to_string());
            }

            for (i, line) in lines.iter().enumerate() {
                if i as u16 >= detail_inner.height {
                    break;
                }
                let style = if i == 0 {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                frame.render_widget(
                    Paragraph::new(line.as_str()).style(style),
                    Rect::new(
                        detail_inner.x,
                        detail_inner.y + i as u16,
                        detail_inner.width,
                        1,
                    ),
                );
            }
        }
    }
}

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

        // Show conversation indicator if this commit has linked AI conversations.
        if modal.commits_with_conversations.contains(&commit_idx) {
            spans.push(Span::styled(
                " [c]AI",
                bg.fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ));
        }

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

/// Draw the interactive rebase modal.
/// Draw the markdown preview pane.
fn draw_markdown_preview(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Preview ")
        .border_style(Style::default().fg(Color::Magenta))
        .style(Style::default().bg(app.theme.bg).fg(app.theme.fg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let source = app.tab().buffer.rope().to_string();
    let rendered = crate::markdown_preview::render_markdown(&source, inner.width as usize);

    // Scroll based on editor scroll position.
    let scroll = app.tab().scroll_row;
    let visible = inner.height as usize;

    for (i, line) in rendered.iter().skip(scroll).take(visible).enumerate() {
        let y = inner.y + i as u16;
        frame.render_widget(
            Paragraph::new(line.clone()),
            Rect::new(inner.x, y, inner.width, 1),
        );
    }
}

fn draw_rebase_modal(
    frame: &mut Frame,
    modal: &crate::rebase_modal::InteractiveRebaseModal,
    area: Rect,
) {
    use crate::rebase_modal::RebaseAction;

    let width = area.width.min(80);
    let height = area.height.min(30);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let rect = Rect::new(x, y, width, height);

    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(" Interactive Rebase ")
        .title_style(
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        );
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    if inner.height < 3 {
        return;
    }

    // Reserve last 2 lines for status.
    let list_height = inner.height.saturating_sub(2) as usize;
    let w = inner.width as usize;

    // Scroll to keep selected visible.
    let scroll = if modal.selected >= list_height {
        modal.selected - list_height + 1
    } else {
        0
    };

    for (i, entry) in modal
        .entries
        .iter()
        .skip(scroll)
        .take(list_height)
        .enumerate()
    {
        let abs_idx = scroll + i;
        let is_selected = abs_idx == modal.selected;

        let action_color = match entry.action {
            RebaseAction::Pick => Color::Green,
            RebaseAction::Reword => Color::Cyan,
            RebaseAction::Edit => Color::Yellow,
            RebaseAction::Squash => Color::Magenta,
            RebaseAction::Fixup => Color::Blue,
            RebaseAction::Drop => Color::Red,
        };

        let action_label = format!("{:<7}", entry.action.label());
        let commit_text = format!("{} {}", entry.commit.short, entry.commit.summary);
        let line_text = format!(" {} {} ", action_label, commit_text);
        let line_text: String = line_text.chars().take(w).collect();

        let style = if is_selected {
            Style::default()
                .fg(Color::Black)
                .bg(action_color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(action_color)
        };

        let row_y = inner.y + i as u16;
        frame.render_widget(
            Paragraph::new(line_text).style(style),
            Rect::new(inner.x, row_y, inner.width, 1),
        );
    }

    // Status line at bottom.
    let status_y = inner.y + inner.height.saturating_sub(1);
    let status_text: String = modal.status.chars().take(w).collect();
    frame.render_widget(
        Paragraph::new(format!(" {}", status_text)).style(Style::default().fg(Color::DarkGray)),
        Rect::new(inner.x, status_y, inner.width, 1),
    );
}

/// Draw the plugin marketplace modal.
fn draw_marketplace_modal(
    frame: &mut Frame,
    modal: &crate::marketplace::MarketplaceModal,
    area: Rect,
) {
    let width = area.width.min(70);
    let height = area.height.min(25);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let rect = Rect::new(x, y, width, height);

    frame.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Plugin Marketplace ")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    if inner.height < 4 {
        return;
    }

    let w = inner.width as usize;

    // Search bar.
    let search_text = if modal.query.is_empty() {
        " Search plugins... (r: refresh, Enter: install, d: uninstall)".to_string()
    } else {
        format!(" > {}", modal.query)
    };
    let search_style = if modal.query.is_empty() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };
    frame.render_widget(
        Paragraph::new(search_text.chars().take(w).collect::<String>()).style(search_style),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    // Plugin list.
    let list_height = inner.height.saturating_sub(3) as usize;
    let scroll = if modal.selected >= list_height {
        modal.selected - list_height + 1
    } else {
        0
    };

    for (i, &idx) in modal
        .filtered
        .iter()
        .skip(scroll)
        .take(list_height)
        .enumerate()
    {
        let listing = &modal.registry[idx];
        let is_selected = scroll + i == modal.selected;
        let installed = modal.is_installed(&listing.name);
        let has_update = modal.has_update(&listing.name);

        let tag = if has_update {
            "[update]"
        } else if installed {
            "[installed]"
        } else {
            ""
        };

        let line = format!(
            " {} v{} {} — {}",
            listing.name, listing.version, tag, listing.description
        );
        let line: String = line.chars().take(w).collect();

        let color = if has_update {
            Color::Yellow
        } else if installed {
            Color::Green
        } else {
            Color::White
        };

        let style = if is_selected {
            Style::default()
                .fg(Color::Black)
                .bg(color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(color)
        };

        let row_y = inner.y + 1 + i as u16;
        frame.render_widget(
            Paragraph::new(line).style(style),
            Rect::new(inner.x, row_y, inner.width, 1),
        );
    }

    if modal.filtered.is_empty() && !modal.registry.is_empty() {
        frame.render_widget(
            Paragraph::new("  No matching plugins").style(Style::default().fg(Color::DarkGray)),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
    } else if modal.registry.is_empty() {
        frame.render_widget(
            Paragraph::new("  Press 'r' to fetch the plugin registry")
                .style(Style::default().fg(Color::DarkGray)),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
    }

    // Status line.
    let status_y = inner.y + inner.height.saturating_sub(1);
    frame.render_widget(
        Paragraph::new(format!(" {}", modal.status)).style(Style::default().fg(Color::DarkGray)),
        Rect::new(inner.x, status_y, inner.width, 1),
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
        let shortcut = item.shortcut();
        let mut spans = vec![
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
        ];
        if !shortcut.is_empty() {
            spans.push(Span::styled(
                format!("  ({shortcut})"),
                if is_selected {
                    Style::default().fg(Color::DarkGray).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::Yellow)
                },
            ));
        }
        let line = ratatui::text::Line::from(spans);

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
            SettingValue::Select { current, .. } => format!("< {current} >"),
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
                        SettingValue::Number { .. } | SettingValue::Select { .. } => {
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD)
                        }
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

    // Sticky scroll and breadcrumbs are rendered by draw_editor_pane using cached
    // foldable_ranges and app.breadcrumbs. Doing them here would duplicate the work
    // and call enclosing_scopes() with a full rope-to-string clone every frame.

    let selection = app.visual_selection_range();
    let theme = &app.theme;
    let sel_style = Style::default()
        .bg(theme.selection_bg)
        .fg(theme.selection_fg);
    let show_authorship = app.show_authorship;
    let tab = app.tab();

    // Build the list of visible buffer lines, skipping folded ranges.
    let mut visible_buffer_lines: Vec<usize> = Vec::with_capacity(visible_lines);
    {
        let mut buf_line = tab.scroll_row;
        let total_lines = tab.buffer.line_count();
        while visible_buffer_lines.len() < visible_lines && buf_line < total_lines {
            visible_buffer_lines.push(buf_line);
            // If this line starts a fold, skip the folded body.
            if let Some(&fold_end) = tab.folded_ranges.get(&buf_line) {
                buf_line = fold_end + 1;
            } else {
                buf_line += 1;
            }
        }
    }

    // Use the tab's cached bracket depths (warmed in draw_editor_pane before this call).
    // Falls back to an empty map if the cache is somehow missing so rendering still works.
    let empty_bracket_map = std::collections::HashMap::new();
    let bracket_depths: &std::collections::HashMap<usize, Vec<(usize, u8)>> =
        tab.bracket_cache.as_ref().unwrap_or(&empty_bracket_map);

    for &line_idx in &visible_buffer_lines {
        if let Some(rope_line) = tab.buffer.line(line_idx) {
            let line_num = if app.config.editor.relative_line_numbers {
                let offset = (line_idx as i64 - tab.cursor.row as i64).unsigned_abs();
                if offset == 0 {
                    format!("{:>4} ", line_idx + 1) // Current line: absolute
                } else {
                    format!("{:>4} ", offset)
                }
            } else {
                format!("{:>4} ", line_idx + 1)
            };
            let content: String = if app.config.editor.word_wrap {
                // Word wrap: show full line, no horizontal scroll.
                rope_line
                    .chars()
                    .take(text_width as usize) // First visual line of wrapped content.
                    .filter(|c| *c != '\n' && *c != '\r')
                    .collect()
            } else {
                rope_line
                    .chars()
                    .skip(tab.scroll_col)
                    .take(text_width as usize)
                    .filter(|c| *c != '\n' && *c != '\r')
                    .collect()
            };

            // If this line is folded, append a fold indicator.
            let (content, fold_suffix) = if let Some(&fold_end) = tab.folded_ranges.get(&line_idx) {
                let folded_count = fold_end.saturating_sub(line_idx);
                let suffix = format!(" ··· ({} lines)", folded_count);
                let max_content = text_width.saturating_sub(suffix.len() as u16 + 1) as usize;
                let truncated: String = content.chars().take(max_content).collect();
                (truncated, Some(suffix))
            } else {
                (content, None)
            };

            // Check if this line is the stopped execution line in the debugger.
            let is_debug_stopped = app
                .debug_panel
                .state
                .stopped_file
                .as_ref()
                .is_some_and(|f| tab.buffer.file_path() == Some(f.as_path()))
                && app.debug_panel.state.stopped_line == Some(line_idx);

            // Fold indicator: show ▶ for folded, ▼ for foldable.
            let is_folded = tab.folded_ranges.contains_key(&line_idx);
            let is_foldable = tab.foldable_ranges.contains_key(&line_idx);

            // Gutter marker: fold > breakpoint/debug > diagnostic > conversation > git > authorship.
            let marker_span = if is_folded {
                Span::styled("▶", Style::default().fg(Color::Yellow))
            } else if is_foldable {
                Span::styled("▼", Style::default().fg(Color::DarkGray))
            } else if tab.breakpoints.contains_key(&line_idx) {
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
            } else if tab.test_lines.iter().any(|(l, _)| *l == line_idx) {
                Span::styled("▶", Style::default().fg(Color::Green))
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

            // Check if this line is in an inline conflict block.
            let conflict_bg = app.inline_conflicts.iter().find_map(|c| {
                if line_idx == c.marker_start {
                    // <<<<<<< line — show action hints
                    Some(Color::Rgb(80, 60, 0)) // bright yellow-ish for marker
                } else if line_idx > c.marker_start && line_idx < c.separator {
                    // "ours" (current) section — green tint
                    Some(Color::Rgb(20, 50, 20))
                } else if line_idx == c.separator {
                    // ======= line
                    Some(Color::Rgb(60, 60, 60))
                } else if line_idx > c.separator && line_idx < c.marker_end {
                    // "theirs" (incoming) section — blue tint
                    Some(Color::Rgb(20, 20, 60))
                } else if line_idx == c.marker_end {
                    // >>>>>>> line
                    Some(Color::Rgb(80, 60, 0))
                } else {
                    None
                }
            });

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
                        let mut s = if color == Color::Reset {
                            Style::default()
                        } else {
                            Style::default().fg(color)
                        };
                        if let Some(&mods) = hl.modifiers.get(char_idx) {
                            if !mods.is_empty() {
                                s = s.add_modifier(mods);
                            }
                        }
                        s
                    } else {
                        Style::default()
                    }
                } else {
                    Style::default()
                };

                // Rainbow bracket colorization.
                let style = if matches!(*ch, '(' | ')' | '{' | '}' | '[' | ']') {
                    if let Some(line_brackets) = bracket_depths.get(&line_idx) {
                        let original_col = visible_start + col;
                        if let Some(&(_, depth)) =
                            line_brackets.iter().find(|(c, _)| *c == original_col)
                        {
                            let rainbow = [
                                Color::Yellow,
                                Color::Magenta,
                                Color::Cyan,
                                Color::Green,
                                Color::Blue,
                                Color::LightRed,
                            ];
                            style.fg(rainbow[depth as usize % 6])
                        } else {
                            style
                        }
                    } else {
                        style
                    }
                } else {
                    style
                };

                // Apply conflict background tint if this line is in a conflict block.
                let style = if let Some(bg) = conflict_bg {
                    style.bg(bg)
                } else {
                    style
                };

                // Indent guide: replace leading spaces at indent boundaries with │.
                let display_ch = if *ch == ' ' && col < content.len() {
                    let indent_width = match tab.indent_style {
                        crate::tab::IndentStyle::Spaces(w) => w as usize,
                        crate::tab::IndentStyle::Tabs => 4,
                    };
                    let actual_col = visible_start + col;
                    // Check if all chars before this are spaces (we're in leading whitespace).
                    let in_leading_ws = content.chars().take(col + 1).all(|c| c == ' ');
                    if in_leading_ws
                        && indent_width > 0
                        && actual_col.is_multiple_of(indent_width)
                        && actual_col > 0
                    {
                        '│'
                    } else {
                        *ch
                    }
                } else {
                    *ch
                };

                // Use indent guide color for guide characters.
                let style = if display_ch == '│' && *ch == ' ' {
                    Style::default().fg(Color::Rgb(60, 60, 60))
                } else {
                    style
                };

                // Overlay diagnostic underlines (errors = red, warnings = yellow).
                let style = {
                    let actual_col = visible_start + col;
                    let diag = tab.diagnostics.iter().find(|d| {
                        let dl = d.range.start.line as usize;
                        let el = d.range.end.line as usize;
                        let sc = d.range.start.character as usize;
                        let ec = d.range.end.character as usize;
                        if line_idx == dl && line_idx == el {
                            actual_col >= sc && actual_col < ec
                        } else if line_idx == dl {
                            actual_col >= sc
                        } else if line_idx == el {
                            actual_col < ec
                        } else {
                            line_idx > dl && line_idx < el
                        }
                    });
                    if let Some(d) = diag {
                        let color = if d.is_error() {
                            app.theme.error
                        } else if d.is_warning() {
                            app.theme.warning
                        } else {
                            app.theme.info
                        };
                        style.add_modifier(Modifier::UNDERLINED).fg(color)
                    } else {
                        style
                    }
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
                current_span.push(display_ch);
            }
            if !current_span.is_empty() {
                spans.push(Span::styled(
                    current_span,
                    current_style.unwrap_or_default(),
                ));
            }

            // Append fold suffix if this line is folded.
            if let Some(ref suffix) = fold_suffix {
                spans.push(Span::styled(
                    suffix.clone(),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
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
    let theme_bg = app.theme.bg;
    let theme_fg = app.theme.fg;
    let border_color = if is_focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let title = format!(" {} ", app.tabs.tabs()[tab_idx].title());
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(theme_bg).fg(theme_fg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Minimap + scrollbar column (hidden in zen mode).
    let show_minimap = app.config.editor.show_minimap && !app.zen_mode;
    let minimap_width: u16 = 12; // narrow code overview
    let (content_area, minimap_area) = if show_minimap && inner.width > minimap_width + 20 {
        let hsplit = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(minimap_width)])
            .split(inner);
        (hsplit[0], Some(hsplit[1]))
    } else {
        (inner, None)
    };

    // Breadcrumbs bar (1 row above editor content, hidden in zen mode).
    let content_area =
        if !app.zen_mode && is_focused && !app.breadcrumbs.is_empty() && content_area.height > 3 {
            // Render breadcrumbs at the top of content_area.
            let bc_text = app.breadcrumbs.join(" > ");
            let bc_display: String = bc_text.chars().take(content_area.width as usize).collect();
            frame.render_widget(
                Paragraph::new(bc_display).style(
                    Style::default()
                        .fg(app.theme.gutter_fg)
                        .add_modifier(Modifier::ITALIC),
                ),
                Rect::new(content_area.x, content_area.y, content_area.width, 1),
            );
            // Shrink content area by 1 row.
            Rect::new(
                content_area.x,
                content_area.y + 1,
                content_area.width,
                content_area.height - 1,
            )
        } else {
            content_area
        };

    // Sticky scroll: pin enclosing scope headers at the top of the viewport.
    let content_area = if !app.zen_mode && is_focused && content_area.height > 5 {
        let scroll_row = app.tabs.tabs()[tab_idx].scroll_row;
        let tab = &app.tabs.tabs()[tab_idx];

        // Find scopes whose start line is above the viewport but end is below.
        let mut sticky_lines: Vec<(usize, String)> = tab
            .foldable_ranges
            .iter()
            .filter_map(|(&start, &end)| {
                if start < scroll_row && end >= scroll_row {
                    tab.buffer.line_text(start).map(|text| {
                        let label = text.trim().to_string();
                        let label: String =
                            label.chars().take(content_area.width as usize).collect();
                        (start, label)
                    })
                } else {
                    None
                }
            })
            .collect();

        // Sort by start line so outermost is first; keep only last 3.
        sticky_lines.sort_by_key(|(line, _)| *line);
        let sticky_count = sticky_lines
            .len()
            .min(3)
            .min(content_area.height as usize / 4);
        let sticky_lines = &sticky_lines[sticky_lines.len().saturating_sub(sticky_count)..];

        if !sticky_lines.is_empty() {
            let sticky_bg = Style::default()
                .fg(app.theme.keyword)
                .bg(app.theme.status_bg)
                .add_modifier(Modifier::DIM);
            for (i, (_, label)) in sticky_lines.iter().enumerate() {
                let y = content_area.y + i as u16;
                frame.render_widget(
                    Paragraph::new(format!(" {}", label)).style(sticky_bg),
                    Rect::new(content_area.x, y, content_area.width, 1),
                );
            }
            // Shrink content area.
            let n = sticky_lines.len() as u16;
            Rect::new(
                content_area.x,
                content_area.y + n,
                content_area.width,
                content_area.height.saturating_sub(n),
            )
        } else {
            content_area
        }
    } else {
        content_area
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

    // Warm the bracket-depth cache; draw_editor only has &App and can't rebuild it.
    app.tabs.tabs_mut()[tab_idx].bracket_depths();

    draw_editor(frame, app, content_area, &git_status);
    // Semantic highlighting from LSP (drawn first so tree-sitter colors are overridden).
    if !app.tab().semantic_tokens.is_empty() {
        draw_semantic_highlights(frame, app, content_area);
    }
    // Rainbow indent guides.
    draw_indent_guides(frame, app, content_area);
    // Code lens from LSP (reference counts, etc.).
    if !app.tab().code_lens.is_empty() {
        draw_code_lens(frame, app, content_area);
    }
    // Inlay hints from LSP.
    if !app.tab().inlay_hints.is_empty() {
        draw_inlay_hints(frame, app, content_area);
    }
    // Word-under-cursor highlights (dim underline on all occurrences).
    if !app.cursor_word_matches.is_empty() && app.search_matches.is_empty() {
        draw_word_highlights(frame, app, content_area);
    }
    // Search match highlights (drawn as overlay on top of text).
    if !app.search_matches.is_empty() {
        draw_search_highlights(frame, app, content_area);
    }
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

        // Collect buffer lines for minimap code preview.
        let buffer_lines: Vec<String> = (0..total_lines)
            .map(|i| {
                tab.buffer
                    .rope()
                    .get_line(i)
                    .map(|l| l.to_string().trim_end_matches('\n').to_string())
                    .unwrap_or_default()
            })
            .collect();

        draw_minimap(
            frame,
            minimap_rect,
            &markers,
            total_lines,
            scroll_row,
            viewport_h,
            &buffer_lines,
        );
    }
}

/// Rainbow colors for indent guide levels (6 colors, cycling).
const INDENT_COLORS: [Color; 6] = [
    Color::Rgb(80, 80, 120),  // blue-grey
    Color::Rgb(80, 120, 80),  // green-grey
    Color::Rgb(120, 100, 60), // amber-grey
    Color::Rgb(120, 70, 100), // pink-grey
    Color::Rgb(60, 110, 120), // cyan-grey
    Color::Rgb(110, 80, 60),  // brown-grey
];

/// Draw rainbow indent guides as thin vertical lines at indent boundaries.
fn draw_indent_guides(frame: &mut Frame, app: &App, area: Rect) {
    let gutter_width = 6u16;
    let tab = app.tab();
    let scroll_row = tab.scroll_row;
    let scroll_col = tab.scroll_col;
    let visible_rows = area.height as usize;
    let text_x = area.x + gutter_width;
    let text_w = area.width.saturating_sub(gutter_width) as usize;
    let tab_w = app.config.editor.tab_width;

    if tab_w == 0 || text_w == 0 {
        return;
    }

    for vis_row in 0..visible_rows {
        let line_idx = scroll_row + vis_row;
        if line_idx >= tab.buffer.line_count() {
            break;
        }

        let line = tab.buffer.rope().line(line_idx);
        let line_str: String = line.chars().collect();

        // Count leading whitespace (visual columns, expanding tabs).
        let mut indent_cols = 0usize;
        for ch in line_str.chars() {
            if ch == ' ' {
                indent_cols += 1;
            } else if ch == '\t' {
                indent_cols = (indent_cols / tab_w + 1) * tab_w;
            } else {
                break;
            }
        }

        // Draw a guide at each tab-stop within the indent region.
        let screen_y = area.y + vis_row as u16;
        let mut col = tab_w;
        let mut level = 0usize;
        while col < indent_cols {
            // Only draw if the character at this position is whitespace.
            if col < line_str.len() {
                let ch_at_col = line_str.as_bytes().get(col).copied().unwrap_or(b' ');
                if ch_at_col != b' ' && ch_at_col != b'\t' {
                    col += tab_w;
                    level += 1;
                    continue;
                }
            }
            let screen_col = col.saturating_sub(scroll_col);
            if screen_col < text_w {
                let color = INDENT_COLORS[level % INDENT_COLORS.len()];
                let cell = Rect::new(text_x + screen_col as u16, screen_y, 1, 1);
                frame.render_widget(
                    Paragraph::new(Span::styled("│", Style::default().fg(color))),
                    cell,
                );
            }
            col += tab_w;
            level += 1;
        }
    }
}

/// Map semantic token type index to a color.
fn semantic_token_color(token_type: u32) -> Option<Color> {
    match token_type {
        0 => Some(Color::Rgb(180, 180, 220)),  // namespace
        1 => Some(Color::Rgb(78, 201, 176)),   // type
        2 => Some(Color::Rgb(78, 201, 176)),   // class
        3 => Some(Color::Rgb(184, 215, 163)),  // enum
        4 => Some(Color::Rgb(78, 201, 176)),   // interface
        5 => Some(Color::Rgb(78, 201, 176)),   // struct
        6 => Some(Color::Rgb(78, 201, 176)),   // typeParameter
        7 => Some(Color::Rgb(156, 220, 254)),  // parameter
        8 => Some(Color::Rgb(156, 220, 254)),  // variable
        9 => Some(Color::Rgb(156, 220, 254)),  // property
        10 => Some(Color::Rgb(184, 215, 163)), // enumMember
        12 => Some(Color::Rgb(220, 220, 170)), // function
        13 => Some(Color::Rgb(220, 220, 170)), // method
        14 => Some(Color::Rgb(190, 140, 220)), // macro
        15 => Some(Color::Rgb(86, 156, 214)),  // keyword
        17 => Some(Color::Rgb(106, 153, 85)),  // comment
        18 => Some(Color::Rgb(206, 145, 120)), // string
        19 => Some(Color::Rgb(181, 206, 168)), // number
        21 => Some(Color::Rgb(180, 180, 180)), // operator
        22 => Some(Color::Rgb(220, 220, 170)), // decorator
        _ => None,
    }
}

/// Draw semantic token highlighting as overlays on the editor text.
fn draw_semantic_highlights(frame: &mut Frame, app: &App, area: Rect) {
    let gutter_width = 6u16;
    let tab = app.tab();
    let scroll_row = tab.scroll_row;
    let scroll_col = tab.scroll_col;
    let visible_rows = area.height as usize;
    let text_x = area.x + gutter_width;
    let text_w = area.width.saturating_sub(gutter_width) as usize;

    for token in &tab.semantic_tokens {
        let row = token.line as usize;
        if row < scroll_row || row >= scroll_row + visible_rows {
            continue;
        }
        let color = match semantic_token_color(token.token_type) {
            Some(c) => c,
            None => continue,
        };

        let col_start = (token.start_char as usize).saturating_sub(scroll_col);
        let col_end = col_start + token.length as usize;
        if col_start >= text_w {
            continue;
        }
        let col_end = col_end.min(text_w);

        let screen_y = area.y + (row - scroll_row) as u16;

        // Read the characters to overlay with semantic color.
        if let Some(line) = tab.buffer.rope().get_line(row) {
            let display: String = line
                .chars()
                .skip(scroll_col + col_start)
                .take(col_end - col_start)
                .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
                .collect();

            if !display.is_empty() {
                let cell = Rect::new(
                    text_x + col_start as u16,
                    screen_y,
                    display.len().min(col_end - col_start) as u16,
                    1,
                );
                frame.render_widget(
                    Paragraph::new(Span::styled(display, Style::default().fg(color))),
                    cell,
                );
            }
        }
    }
}

/// Draw code lens text at the end of function lines.
fn draw_code_lens(frame: &mut Frame, app: &App, area: Rect) {
    let gutter_width = 6u16;
    let tab = app.tab();
    let scroll_row = tab.scroll_row;
    let visible_rows = area.height as usize;
    let text_x = area.x + gutter_width;
    let text_w = area.width.saturating_sub(gutter_width) as usize;

    let style = Style::default()
        .fg(Color::Rgb(120, 120, 120))
        .add_modifier(Modifier::ITALIC);

    for lens in &tab.code_lens {
        let row = lens.line as usize;
        if row < scroll_row || row >= scroll_row + visible_rows {
            continue;
        }
        let screen_y = area.y + (row - scroll_row) as u16;
        let line_len = tab
            .buffer
            .rope()
            .get_line(row)
            .map(|l| l.len_chars().saturating_sub(1))
            .unwrap_or(0);
        let col = line_len.saturating_sub(tab.scroll_col) + 2;
        if col >= text_w {
            continue;
        }
        let display = format!(" {} ", lens.text);
        let max_len = text_w.saturating_sub(col);
        let display: String = display.chars().take(max_len).collect();
        let cell = Rect::new(
            text_x + col as u16,
            screen_y,
            display.len().min(max_len) as u16,
            1,
        );
        frame.render_widget(Paragraph::new(Span::styled(display, style)), cell);
    }
}

/// Draw inlay hints from LSP as dimmed inline text.
fn draw_inlay_hints(frame: &mut Frame, app: &App, area: Rect) {
    let gutter_width = 6u16;
    let tab = app.tab();
    let scroll_row = tab.scroll_row;
    let scroll_col = tab.scroll_col;
    let visible_rows = area.height as usize;
    let text_x = area.x + gutter_width;
    let text_w = area.width.saturating_sub(gutter_width) as usize;

    let type_style = Style::default()
        .fg(Color::Rgb(120, 160, 180))
        .add_modifier(Modifier::ITALIC);
    let param_style = Style::default()
        .fg(Color::Rgb(160, 140, 120))
        .add_modifier(Modifier::ITALIC);

    for hint in &tab.inlay_hints {
        let row = hint.line as usize;
        if row < scroll_row || row >= scroll_row + visible_rows {
            continue;
        }
        let screen_y = area.y + (row - scroll_row) as u16;
        let col = (hint.character as usize).saturating_sub(scroll_col);
        if col >= text_w {
            continue;
        }

        // Render the hint label after the position.
        let label = if hint.is_type {
            format!(": {}", hint.label)
        } else {
            format!("{}:", hint.label)
        };
        let max_len = text_w.saturating_sub(col);
        let display: String = label.chars().take(max_len).collect();
        let style = if hint.is_type {
            type_style
        } else {
            param_style
        };

        let cell = Rect::new(
            text_x + col as u16,
            screen_y,
            display.len().min(max_len) as u16,
            1,
        );
        frame.render_widget(Paragraph::new(Span::styled(display, style)), cell);
    }
}

/// Draw word-under-cursor highlights as dim underline overlays.
fn draw_word_highlights(frame: &mut Frame, app: &App, area: Rect) {
    let gutter_width = 6u16;
    let tab = app.tab();
    let scroll_row = tab.scroll_row;
    let scroll_col = tab.scroll_col;
    let visible_rows = area.height as usize;
    let text_x = area.x + gutter_width;
    let text_w = area.width.saturating_sub(gutter_width) as usize;
    let bg = Color::Rgb(60, 60, 70);
    let rope_len = tab.buffer.rope().len_chars();

    for &(start, end) in &app.cursor_word_matches {
        // Matches may be stale after a tab switch — skip any whose indices
        // fall outside the active buffer to avoid a ropey panic.
        if start >= rope_len || end > rope_len {
            continue;
        }
        let start_row = tab.buffer.rope().char_to_line(start);
        if start_row < scroll_row || start_row >= scroll_row + visible_rows {
            continue;
        }
        let line_start_char = tab.buffer.rope().line_to_char(start_row);
        let col_start = (start - line_start_char).saturating_sub(scroll_col);
        let col_end = (end - line_start_char)
            .saturating_sub(scroll_col)
            .min(text_w);
        if col_start >= text_w || col_start >= col_end {
            continue;
        }
        let screen_y = area.y + (start_row - scroll_row) as u16;
        let line = tab.buffer.rope().line(start_row);
        let display: String = line
            .chars()
            .skip(scroll_col + col_start)
            .take(col_end - col_start)
            .map(|c| if c == '\n' { ' ' } else { c })
            .collect();
        let cell = Rect::new(
            text_x + col_start as u16,
            screen_y,
            display.len().min(col_end - col_start) as u16,
            1,
        );
        frame.render_widget(
            Paragraph::new(Span::styled(
                display,
                Style::default().bg(bg).add_modifier(Modifier::UNDERLINED),
            )),
            cell,
        );
    }
}

/// Draw search match highlights as overlays on the editor text.
///
/// Matches are highlighted with a yellow/orange background. The current match
/// (focused by n/N navigation) is shown with a brighter highlight.
fn draw_search_highlights(frame: &mut Frame, app: &App, area: Rect) {
    let gutter_width = 6u16;
    let tab = app.tab();
    let scroll_row = tab.scroll_row;
    let scroll_col = tab.scroll_col;
    let visible_rows = area.height as usize;
    let text_x = area.x + gutter_width;
    let text_w = area.width.saturating_sub(gutter_width) as usize;

    let match_bg = Color::Rgb(120, 100, 30); // dim yellow for all matches
    let current_bg = Color::Rgb(200, 160, 30); // bright yellow for current match
    let match_fg = Color::Black;
    let rope_len = tab.buffer.rope().len_chars();

    for (idx, &(start, end)) in app.search_matches.iter().enumerate() {
        // Matches may be stale after a tab switch — skip any whose indices
        // fall outside the active buffer to avoid a ropey panic.
        if start >= rope_len || end > rope_len {
            continue;
        }
        let is_current = idx == app.search_current;
        let bg = if is_current { current_bg } else { match_bg };

        // Convert char indices to (row, col) positions.
        let start_row = tab.buffer.rope().char_to_line(start);
        let end_row = tab
            .buffer
            .rope()
            .char_to_line(end.saturating_sub(1).max(start));

        for row in start_row..=end_row {
            if row < scroll_row || row >= scroll_row + visible_rows {
                continue;
            }
            let screen_y = area.y + (row - scroll_row) as u16;
            let line_start_char = tab.buffer.rope().line_to_char(row);

            let col_start = if row == start_row {
                (start - line_start_char).saturating_sub(scroll_col)
            } else {
                0
            };
            let col_end = if row == end_row {
                (end - line_start_char).saturating_sub(scroll_col)
            } else {
                let line_len = tab.buffer.rope().line(row).len_chars().saturating_sub(1);
                line_len.saturating_sub(scroll_col)
            };

            if col_start >= text_w || col_end == 0 || col_start >= col_end {
                continue;
            }
            let col_end = col_end.min(text_w);

            // Read the actual characters to overlay.
            let line = tab.buffer.rope().line(row);
            let display: String = line
                .chars()
                .skip(scroll_col + col_start)
                .take(col_end - col_start)
                .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
                .collect();

            let cell_area = Rect::new(
                text_x + col_start as u16,
                screen_y,
                display.len().min(text_w - col_start) as u16,
                1,
            );
            frame.render_widget(
                Paragraph::new(Span::styled(display, Style::default().fg(match_fg).bg(bg))),
                cell_area,
            );
        }
    }
}

/// Draw remote peer cursors and selections as overlays on the editor.
fn draw_peer_cursors(frame: &mut Frame, app: &App, area: Rect) {
    let peers = app.collab_peer_awareness();
    if peers.is_empty() || area.width < 8 || area.height == 0 {
        return;
    }

    let gutter_width = 6u16;
    let tab = app.tab();
    let scroll_row = tab.scroll_row;
    let scroll_col = tab.scroll_col;
    let visible_rows = area.height as usize;
    let buf_area = frame.area();
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
                    if screen_x < buf_area.width && screen_y < buf_area.height {
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

                if screen_x < buf_area.width && screen_y < buf_area.height {
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
                            if lx < buf_area.width && label_y < buf_area.height {
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

    let agent_indicator = if let Some(ref session) = app.agent_mode {
        let elapsed = session.started_at.elapsed().as_secs();
        let state = if session.paused { "⏸" } else { "▶" };
        let trust = session.trust_level.label();
        let subs = session.subagent_manager.active_count();
        let sub_str = if subs > 0 {
            format!(" {subs}sub")
        } else {
            String::new()
        };
        format!(
            " │ AGENT {state} [{}/{}] {}f {}c{sub_str} {}s {trust}",
            session.iteration,
            session.max_iterations,
            session.files_changed.len(),
            session.commands_run,
            elapsed
        )
    } else {
        String::new()
    };

    let git_indicator = app
        .git_branch()
        .map(|b| format!(" │ {b}"))
        .unwrap_or_default();

    let mcp_indicator = if let Some(port) = app.mcp_port() {
        let agent_count = app.agent_registry.count();
        let acp_part = app
            .acp_port()
            .map(|p| format!(" ACP:{p}"))
            .unwrap_or_default();
        if agent_count > 0 {
            format!(" │ MCP:{port}{acp_part} ({agent_count} agents)")
        } else {
            format!(" │ MCP:{port}{acp_part}")
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

    let follow_indicator = if let Some(peer_id) = app.collab_follow_peer {
        let name = app
            .collab
            .as_ref()
            .and_then(|s| s.peers.get(&peer_id))
            .map(|p| p.name.as_str())
            .unwrap_or("?");
        format!(" │ FOLLOWING {name}")
    } else {
        String::new()
    };

    let share_term_indicator = if app.collab_sharing_terminal {
        " │ [sharing term]".to_string()
    } else {
        String::new()
    };

    // Detect line ending style from the buffer.
    let line_ending = {
        let rope = app.buffer().rope();
        let has_crlf = (0..rope.len_lines().min(20)).any(|i| {
            let line = rope.line(i);
            let s: String = line.chars().collect();
            s.ends_with("\r\n")
        });
        if has_crlf {
            "CRLF"
        } else {
            "LF"
        }
    };

    let left = format!(
        " {} │ {}{} │ UTF-8 {}{}{}{}{}{}{}{}{}{}{}{}{}",
        app.mode.label(),
        file_name,
        modified,
        line_ending,
        git_indicator,
        last_change,
        diag_str,
        lsp_indicator,
        mcp_indicator,
        collab_indicator,
        follow_indicator,
        share_term_indicator,
        claude_indicator,
        experiment_indicator,
        agent_indicator,
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
            let selected_text = app.buffer().rope().slice(sel_start..sel_end).to_string();
            let words = selected_text.split_whitespace().count();
            if lines > 1 {
                format!(" {lines}L {words}W {chars}C │")
            } else {
                format!(" {words}W {chars}C │")
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
    let total_lines = app.buffer().line_count();
    let file_size = app
        .buffer()
        .file_path()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| {
            let bytes = m.len();
            if bytes < 1024 {
                format!("{bytes}B")
            } else if bytes < 1024 * 1024 {
                format!("{}KB", bytes / 1024)
            } else {
                format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
            }
        })
        .unwrap_or_default();
    let size_info = if file_size.is_empty() {
        String::new()
    } else {
        format!("{file_size} ")
    };

    let right = format!(
        "{}{} {}:{} {}{total_lines}L ",
        search_info,
        selection_info,
        app.cursor().row + 1,
        app.cursor().col + 1,
        size_info,
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
            Mode::Command => {
                // Build command input with inline ghost completion.
                let base = format!(":{}", app.command_input);
                if let Some(idx) = app.command_completion_idx {
                    if let Some((cmd, _, _)) = app.command_completions.get(idx) {
                        if let Some(suffix) = cmd.strip_prefix(app.command_input.trim()) {
                            format!("{base}{suffix}")
                        } else {
                            base
                        }
                    } else {
                        base
                    }
                } else {
                    base
                }
            }
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
                // When the source control panel is focused or an AI commit message
                // is being generated, prioritise the status message so that errors
                // and progress are not hidden by ghost suggestion text.
                if app.source_control_focused || app.is_generating_commit_msg() {
                    app.status_message
                        .clone()
                        .or_else(|| app.ghost_suggestion_status())
                        .unwrap_or_default()
                } else {
                    app.ghost_suggestion_status()
                        .or_else(|| app.status_message.clone())
                        .unwrap_or_default()
                }
            }
        }
    };

    // Render command completion popup above the command bar.
    if app.mode == Mode::Command && !app.command_completions.is_empty() {
        let max_items = app.command_completions.len().min(10);
        let popup_height = max_items as u16 + 2; // +2 for borders
                                                 // Find max width needed.
        let max_width = app
            .command_completions
            .iter()
            .take(max_items)
            .map(|(cmd, desc, shortcut)| {
                let shortcut_len = if shortcut.is_empty() {
                    0
                } else {
                    shortcut.len() + 3 // "  (Ctrl+T)"
                };
                cmd.len() + desc.len() + 5 + shortcut_len
            })
            .max()
            .unwrap_or(20)
            .min(area.width as usize);
        let popup_width = (max_width as u16 + 2).min(area.width); // +2 for borders

        let popup_y = area.y.saturating_sub(popup_height);
        let popup_area = Rect::new(area.x, popup_y, popup_width, popup_height);

        let items: Vec<ratatui::text::Line> = app
            .command_completions
            .iter()
            .take(max_items)
            .enumerate()
            .map(|(i, (cmd, desc, shortcut))| {
                let is_selected = app.command_completion_idx == Some(i);
                let style = if is_selected {
                    Style::default().fg(Color::Black).bg(app.theme.mode_command)
                } else {
                    Style::default().fg(Color::White)
                };
                let desc_style = if is_selected {
                    Style::default()
                        .fg(Color::DarkGray)
                        .bg(app.theme.mode_command)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let shortcut_style = if is_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .bg(app.theme.mode_command)
                } else {
                    Style::default().fg(Color::Yellow)
                };
                let mut spans = vec![
                    Span::styled(format!(" :{cmd}"), style),
                    Span::styled(format!("  {desc}"), desc_style),
                ];
                if !shortcut.is_empty() {
                    spans.push(Span::styled(format!("  ({shortcut})"), shortcut_style));
                }
                spans.push(Span::styled(" ", desc_style));
                ratatui::text::Line::from(spans)
            })
            .collect();

        let popup_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .style(Style::default().bg(app.theme.bg));
        let popup_paragraph = Paragraph::new(items).block(popup_block);
        frame.render_widget(ratatui::widgets::Clear, popup_area);
        frame.render_widget(popup_paragraph, popup_area);
    }

    // Render command bar content — use spans for ghost completion styling.
    if app.mode == Mode::Command {
        let input_part = format!(":{}", app.command_input);
        let ghost_part = if let Some(idx) = app.command_completion_idx {
            if let Some((cmd, _, _)) = app.command_completions.get(idx) {
                cmd.strip_prefix(app.command_input.trim())
                    .unwrap_or("")
                    .to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let line = ratatui::text::Line::from(vec![
            Span::styled(&input_part, Style::default().fg(app.theme.fg)),
            Span::styled(ghost_part, Style::default().fg(Color::DarkGray)),
        ]);
        let paragraph = Paragraph::new(line);
        frame.render_widget(paragraph, area);
        return;
    }

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

/// Draw the peek definition inline popup near the cursor.
fn draw_peek_definition(frame: &mut Frame, app: &App, editor_area: Rect) {
    let peek = match &app.peek_definition {
        Some(p) => p,
        None => return,
    };

    let max_visible: usize = 18;
    let visible_lines: Vec<&str> = peek
        .lines
        .iter()
        .skip(peek.scroll_offset)
        .take(max_visible)
        .map(|s| s.as_str())
        .collect();

    let height = (visible_lines.len() as u16 + 2).min(editor_area.height / 2);
    let width = 80u16.min(editor_area.width.saturating_sub(4));

    // Position below the cursor, or above if not enough room.
    let cursor_x =
        (app.cursor().col.saturating_sub(app.tab().scroll_col)) as u16 + editor_area.x + 6;
    let cursor_y =
        (app.cursor().row.saturating_sub(app.tab().scroll_row)) as u16 + editor_area.y + 1;

    let x = cursor_x.min(editor_area.right().saturating_sub(width));
    let y = if cursor_y + height + 1 < editor_area.bottom() {
        cursor_y + 1
    } else {
        cursor_y.saturating_sub(height + 1)
    };

    let popup_area = Rect::new(x, y, width, height);
    frame.render_widget(Clear, popup_area);

    let display_name = peek
        .file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let title = format!(" Peek: {}:{} ", display_name, peek.target_line + 1);

    // Scroll indicator.
    let can_scroll = peek.lines.len() > max_visible;
    let scroll_hint = if can_scroll {
        let pos = peek.scroll_offset + 1;
        let total = peek.lines.len().saturating_sub(max_visible) + 1;
        format!(" {}/{} ", pos, total)
    } else {
        String::new()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .title_alignment(ratatui::layout::Alignment::Left)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    // Render syntax-highlighted lines.
    let hl_offset = peek.scroll_offset;
    let inner_width = inner.width as usize;

    for (i, line_text) in visible_lines.iter().enumerate() {
        if i as u16 >= inner.height {
            break;
        }
        let line_row = inner.y + i as u16;
        let line_no = peek.first_line + hl_offset + i;
        let is_target = line_no == peek.target_line;

        // Line number gutter (4 chars).
        let gutter = format!("{:>3} ", line_no + 1);
        let gutter_style = if is_target {
            Style::default().fg(Color::Yellow).bg(Color::Black)
        } else {
            Style::default().fg(Color::DarkGray).bg(Color::Black)
        };
        let gutter_span = Span::styled(gutter, gutter_style);

        // Build syntax-highlighted spans for the code portion.
        let hl_line = peek.highlighted.get(hl_offset + i);
        let code_width = inner_width.saturating_sub(4);
        let truncated: String = line_text.chars().take(code_width).collect();

        let code_spans: Vec<Span> = if let Some(hl) = hl_line {
            // Build character-by-character coloured spans, coalescing adjacent same-colour chars.
            let mut spans = Vec::new();
            let mut current_color = Color::Reset;
            let mut buf = String::new();
            let bg = if is_target {
                Color::Rgb(40, 40, 60)
            } else {
                Color::Black
            };
            for (ci, ch) in truncated.chars().enumerate() {
                let c = hl.colors.get(ci).copied().unwrap_or(Color::Reset);
                if c != current_color && !buf.is_empty() {
                    spans.push(Span::styled(
                        std::mem::take(&mut buf),
                        Style::default().fg(current_color).bg(bg),
                    ));
                }
                current_color = c;
                buf.push(ch);
            }
            if !buf.is_empty() {
                spans.push(Span::styled(buf, Style::default().fg(current_color).bg(bg)));
            }
            spans
        } else {
            let bg = if is_target {
                Color::Rgb(40, 40, 60)
            } else {
                Color::Black
            };
            vec![Span::styled(
                truncated,
                Style::default().fg(Color::White).bg(bg),
            )]
        };

        let mut all_spans = vec![gutter_span];
        all_spans.extend(code_spans);
        let line_widget = Line::from(all_spans);

        frame.render_widget(
            Paragraph::new(line_widget).style(Style::default().bg(Color::Black)),
            Rect::new(inner.x, line_row, inner.width, 1),
        );
    }

    // Scroll hint in bottom-right corner.
    if !scroll_hint.is_empty() {
        let hint_len = scroll_hint.len() as u16;
        let hint_x = popup_area.right().saturating_sub(hint_len + 1);
        let hint_y = popup_area.bottom().saturating_sub(1);
        frame.render_widget(
            Paragraph::new(scroll_hint)
                .style(Style::default().fg(Color::DarkGray).bg(Color::Black)),
            Rect::new(hint_x, hint_y, hint_len, 1),
        );
    }
}

/// Draw the signature help popup with the active parameter highlighted.
fn draw_signature_help(
    frame: &mut Frame,
    app: &App,
    editor_area: Rect,
    sig: &crate::lsp::SignatureHelpResult,
) {
    let label = &sig.label;
    let height = if sig.documentation.is_some() {
        4u16
    } else {
        3u16
    };
    let width = (label.len() as u16 + 4).clamp(20, editor_area.width.saturating_sub(4));

    let cursor_x =
        (app.cursor().col.saturating_sub(app.tab().scroll_col)) as u16 + editor_area.x + 6;
    let cursor_y = (app.cursor().row.saturating_sub(app.tab().scroll_row)) as u16 + editor_area.y;

    let x = cursor_x.min(editor_area.right().saturating_sub(width));
    let y = cursor_y.saturating_sub(height);

    let popup_area = Rect::new(x, y, width, height);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Signature ")
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    if inner.height == 0 {
        return;
    }

    // Build the signature line with the active parameter highlighted.
    let active_range = sig.parameters.get(sig.active_parameter);
    let mut spans: Vec<Span> = Vec::new();

    if let Some(&(start, end)) = active_range {
        let start = start.min(label.len());
        let end = end.min(label.len());
        if start > 0 {
            spans.push(Span::styled(
                &label[..start],
                Style::default().fg(Color::White),
            ));
        }
        spans.push(Span::styled(
            &label[start..end],
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        ));
        if end < label.len() {
            spans.push(Span::styled(
                &label[end..],
                Style::default().fg(Color::White),
            ));
        }
    } else {
        spans.push(Span::styled(
            label.as_str(),
            Style::default().fg(Color::White),
        ));
    }

    let sig_line = Line::from(spans);
    frame.render_widget(
        Paragraph::new(sig_line).style(Style::default().bg(Color::Black)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    // Render documentation if available.
    if let Some(ref doc) = sig.documentation {
        let doc_line: String = doc.chars().take(inner.width as usize).collect();
        frame.render_widget(
            Paragraph::new(Span::styled(
                doc_line,
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ))
            .style(Style::default().bg(Color::Black)),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
    }
}

fn draw_hover_popup(frame: &mut Frame, app: &App, editor_area: Rect, text: &str) {
    let max_lines = 15;

    // Parse markdown: detect code blocks (```lang ... ```) and format them.
    let mut hover_lines: Vec<Line> = Vec::new();
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_lines: Vec<String> = Vec::new();

    for line in text.lines().take(max_lines + 5) {
        if line.starts_with("```") {
            if in_code_block {
                // End of code block — syntax-highlight the accumulated code.
                let ext = if code_lang.is_empty() {
                    "rs"
                } else {
                    code_lang.as_str()
                };
                let code_text = code_lines.join("\n");
                let highlighted =
                    if let Some(lang) = crate::highlight::Language::from_extension(ext) {
                        crate::highlight::SyntaxHighlighter::new(lang)
                            .map(|mut hl| hl.highlight(&code_text, Some(&app.theme)))
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };

                for (i, code_line) in code_lines.iter().enumerate() {
                    if hover_lines.len() >= max_lines {
                        break;
                    }
                    if let Some(hl_line) = highlighted.get(i) {
                        // Convert per-char colors into spans by grouping consecutive same-color chars.
                        let chars: Vec<char> = code_line.chars().collect();
                        let mut spans: Vec<Span> = Vec::new();
                        let mut run_start = 0;
                        let mut run_color = hl_line
                            .colors
                            .first()
                            .copied()
                            .unwrap_or(Color::Rgb(200, 200, 200));
                        for (j, &color) in hl_line.colors.iter().enumerate() {
                            if color != run_color || j >= chars.len() {
                                let text: String =
                                    chars[run_start..j.min(chars.len())].iter().collect();
                                if !text.is_empty() {
                                    spans.push(Span::styled(
                                        text,
                                        Style::default().fg(run_color).bg(Color::Rgb(30, 30, 30)),
                                    ));
                                }
                                run_start = j;
                                run_color = color;
                            }
                        }
                        // Flush remaining.
                        let text: String = chars[run_start..].iter().collect();
                        if !text.is_empty() {
                            spans.push(Span::styled(
                                text,
                                Style::default().fg(run_color).bg(Color::Rgb(30, 30, 30)),
                            ));
                        }
                        hover_lines.push(Line::from(spans));
                    } else {
                        hover_lines.push(Line::from(Span::styled(
                            code_line.clone(),
                            Style::default()
                                .fg(Color::Rgb(200, 200, 200))
                                .bg(Color::Rgb(30, 30, 30)),
                        )));
                    }
                }
                code_lines.clear();
                in_code_block = false;
            } else {
                // Start of code block.
                code_lang = line.trim_start_matches('`').trim().to_string();
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            code_lines.push(line.to_string());
            continue;
        }

        if hover_lines.len() >= max_lines {
            break;
        }

        // Markdown formatting: bold, italic, headers.
        if line.starts_with("# ") || line.starts_with("## ") || line.starts_with("### ") {
            let header_text = line.trim_start_matches('#').trim();
            hover_lines.push(Line::from(Span::styled(
                header_text.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
        } else if line.starts_with("---") || line.starts_with("___") {
            hover_lines.push(Line::from(Span::styled(
                "─".repeat(20),
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            // Inline code: `code`
            let mut spans: Vec<Span> = Vec::new();
            let mut rest = line;
            while let Some(start) = rest.find('`') {
                if start > 0 {
                    spans.push(Span::raw(rest[..start].to_string()));
                }
                rest = &rest[start + 1..];
                if let Some(end) = rest.find('`') {
                    spans.push(Span::styled(
                        rest[..end].to_string(),
                        Style::default()
                            .fg(Color::Rgb(206, 145, 120))
                            .bg(Color::Rgb(40, 40, 40)),
                    ));
                    rest = &rest[end + 1..];
                } else {
                    spans.push(Span::raw(format!("`{rest}")));
                    rest = "";
                }
            }
            if !rest.is_empty() {
                spans.push(Span::raw(rest.to_string()));
            }
            hover_lines.push(Line::from(spans));
        }
    }

    let display_lines = hover_lines.len().min(max_lines);
    let height = (display_lines as u16 + 2).min(editor_area.height / 2);
    let max_width = text
        .lines()
        .take(max_lines)
        .map(|l| l.len() as u16)
        .max()
        .unwrap_or(20)
        .clamp(20, editor_area.width.saturating_sub(8));
    let width = max_width + 4;

    let cursor_x = (app.cursor().col - app.tab().scroll_col) as u16 + editor_area.x + 6;
    let cursor_y = (app.cursor().row - app.tab().scroll_row) as u16 + editor_area.y + 1;

    let x = cursor_x.min(editor_area.right().saturating_sub(width));
    let y = if cursor_y + height < editor_area.bottom() {
        cursor_y
    } else {
        cursor_y.saturating_sub(height + 1)
    };

    let popup_area = Rect::new(x, y, width, height);
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Hover ")
        .border_style(Style::default().fg(Color::Cyan));

    let paragraph = Paragraph::new(hover_lines)
        .block(block)
        .style(Style::default().fg(Color::White).bg(Color::Black));
    frame.render_widget(paragraph, popup_area);
}

/// Draw a diagnostic popup showing the error/warning message at the cursor line.
fn draw_diagnostic_popup(
    frame: &mut Frame,
    app: &App,
    editor_area: Rect,
    diag: &crate::lsp::Diagnostic,
) {
    let severity = if diag.is_error() {
        "Error"
    } else if diag.is_warning() {
        "Warning"
    } else {
        "Info"
    };
    let source = diag
        .source
        .as_deref()
        .map(|s| format!(" ({s})"))
        .unwrap_or_default();
    let title = format!(" {severity}{source} ");

    let lines: Vec<&str> = diag.message.lines().take(7).collect();
    let has_lsp = app.tab().lsp_client.is_some();
    let height = (lines.len() as u16 + if has_lsp { 3 } else { 2 }).min(editor_area.height / 3);
    let max_width = lines
        .iter()
        .map(|l| l.len() as u16)
        .max()
        .unwrap_or(20)
        .clamp(20, editor_area.width.saturating_sub(10));
    let width = (max_width + 4).min(editor_area.width);

    // Position below the cursor line.
    let cursor_y =
        (app.cursor().row.saturating_sub(app.tab().scroll_row)) as u16 + editor_area.y + 1;
    let cursor_x =
        (app.cursor().col.saturating_sub(app.tab().scroll_col)) as u16 + editor_area.x + 6;

    let x = cursor_x.min(editor_area.right().saturating_sub(width));
    let y = if cursor_y + 1 + height < editor_area.bottom() {
        cursor_y + 1
    } else {
        cursor_y.saturating_sub(height + 1)
    };

    let popup_area = Rect::new(x, y, width, height);
    frame.render_widget(Clear, popup_area);

    let border_color = if diag.is_error() {
        app.theme.error
    } else if diag.is_warning() {
        app.theme.warning
    } else {
        app.theme.info
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(border_color));

    let mut diag_lines: Vec<Line> = lines.iter().map(|l| Line::from(l.to_string())).collect();
    if has_lsp {
        diag_lines.push(Line::from(Span::styled(
            "  <leader>f to fix",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )));
    }

    let paragraph = Paragraph::new(diag_lines)
        .block(block)
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .wrap(ratatui::widgets::Wrap { trim: false });
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
/// Draw inline conflict action hints on <<<<<<< marker lines.
fn draw_conflict_actions(frame: &mut Frame, app: &App, editor_area: Rect) {
    let gutter_w = 6u16;
    let scroll_row = app.tab().scroll_row;
    let cursor_row = app.tab().cursor.row;

    for conflict in &app.inline_conflicts {
        // Check if the <<<<<<< line is visible.
        let marker_line = conflict.marker_start;
        if marker_line < scroll_row {
            continue;
        }
        let screen_row = (marker_line - scroll_row) as u16;
        if screen_row >= editor_area.height {
            continue;
        }

        // Check if cursor is in this conflict block.
        let cursor_in_block =
            cursor_row >= conflict.marker_start && cursor_row <= conflict.marker_end;

        // Render action hint on the marker line.
        let hint = if cursor_in_block {
            "Accept: [1]Current [2]Incoming [3]Both(C+I) [4]Both(I+C)"
        } else {
            "Accept Current | Accept Incoming | Accept Both"
        };

        let hint_style = Style::default()
            .fg(Color::Rgb(150, 150, 100))
            .add_modifier(Modifier::ITALIC);

        let x = editor_area.x + gutter_w;
        let y = editor_area.y + screen_row;
        let max_w = editor_area.width.saturating_sub(gutter_w);

        // Find end of the <<<<<<< text to position hint after it.
        let marker_text_len = app
            .tab()
            .buffer
            .line_text(marker_line)
            .map(|t| t.trim_end().len())
            .unwrap_or(0) as u16;
        let hint_x = x + marker_text_len.min(max_w) + 1;

        if hint_x < editor_area.x + editor_area.width {
            let available = (editor_area.x + editor_area.width).saturating_sub(hint_x);
            let display: String = hint.chars().take(available as usize).collect();
            frame.render_widget(
                Paragraph::new(display).style(hint_style),
                Rect::new(hint_x, y, available, 1),
            );
        }
    }
}

/// Draw next-edit prediction markers (gutter arrows + faint line highlight).
fn draw_edit_predictions(frame: &mut Frame, app: &App, editor_area: Rect) {
    let predictions = app.edit_predictions();
    if predictions.is_empty() || editor_area.width < 8 || editor_area.height == 0 {
        return;
    }

    let tab = app.tab();
    let scroll_row = tab.scroll_row;
    let visible_rows = editor_area.height as usize;
    let gutter_width = 6u16;
    let pred_color = Color::Indexed(75); // Muted blue
    let buf_area = frame.area();

    for (i, pred) in predictions.iter().enumerate() {
        if pred.line < scroll_row || pred.line >= scroll_row + visible_rows {
            continue;
        }
        if pred.line == tab.cursor.row {
            continue; // Don't show on current line.
        }

        let screen_y = editor_area.y + (pred.line - scroll_row) as u16;

        // Gutter marker: '›' for top prediction, '·' for others.
        let marker = if i == 0 { "›" } else { "·" };
        let marker_x = editor_area.x + gutter_width.saturating_sub(2);
        if marker_x < buf_area.width && screen_y < buf_area.height {
            let cell = &mut frame.buffer_mut()[(marker_x, screen_y)];
            cell.set_char(marker.chars().next().unwrap());
            cell.set_fg(pred_color);
        }

        // Faint line highlight (subtle background tint).
        let line_start = editor_area.x + gutter_width;
        let line_width = editor_area.width.saturating_sub(gutter_width);
        for col in 0..line_width {
            let x = line_start + col;
            if x < buf_area.width && screen_y < buf_area.height {
                let cell = &mut frame.buffer_mut()[(x, screen_y)];
                cell.set_bg(Color::Indexed(236)); // Very dark gray tint
            }
        }
    }
}

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

// ---------------------------------------------------------------------------
// Update notification & modal
// ---------------------------------------------------------------------------

/// Draw a floating notification toast in the top-right corner.
fn draw_update_notification(frame: &mut Frame, app: &mut App, area: Rect) {
    let version = match &app.update_status {
        Some(crate::update::UpdateStatus::Available { version, .. }) => version.clone(),
        _ => return,
    };

    let text = format!(" \u{2191} v{} available  [u]pdate ", version);
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
        Span::styled(" [u]", Style::default().fg(Color::Green)),
        Span::styled("pdate ", Style::default().fg(Color::DarkGray)),
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

/// Draw the which-key popup showing available leader key bindings.
fn draw_which_key_popup(frame: &mut Frame, app: &App, area: Rect) {
    let items = &app.which_key_items;
    let cols: usize = 2;
    let rows_per_col = items.len().div_ceil(cols);
    let col_width: u16 = 28;
    let width = (col_width * cols as u16) + 3;
    let height = (rows_per_col as u16) + 2;

    // Position at bottom center of the screen.
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height + 2);
    let popup = Rect::new(x, y, width.min(area.width), height.min(area.height));

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" Leader (Space) ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    for (i, (key, desc)) in items.iter().enumerate() {
        let col = i / rows_per_col;
        let row = i % rows_per_col;
        let x_off = inner.x + (col as u16) * col_width;
        let y_off = inner.y + row as u16;
        if y_off >= inner.y + inner.height {
            break;
        }
        let line = ratatui::text::Line::from(vec![
            Span::styled(
                format!(" {} ", key),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(desc.as_str(), Style::default().fg(Color::White)),
        ]);
        let w = col_width.min(inner.width.saturating_sub(col as u16 * col_width));
        frame.render_widget(Paragraph::new(line), Rect::new(x_off, y_off, w, 1));
    }
}

/// Draw the right-click context menu overlay.
///
/// Anchored at the click position, clamped so the popup never escapes the
/// screen. Disabled items are dimmed; the highlighted item gets a reverse-
/// video bar. The rendered rect is cached on the menu so left-click hit-
/// testing in the event loop knows where the items are.
fn draw_context_menu(frame: &mut Frame, app: &mut App, area: Rect) {
    let items = app.context_menu.items.clone();
    if items.is_empty() {
        return;
    }
    let label_width = items.iter().map(|i| i.label.len()).max().unwrap_or(8) as u16;
    // 2 cells of padding on each side inside the borders.
    let width = (label_width + 4).max(12);
    let height = items.len() as u16 + 2;

    let (anchor_x, anchor_y) = app.context_menu.anchor;
    let x = anchor_x.min(area.x + area.width.saturating_sub(width));
    let y = anchor_y.min(area.y + area.height.saturating_sub(height));
    let popup = Rect::new(x, y, width, height);
    app.context_menu.rect = popup;

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .style(Style::default().bg(Color::Rgb(40, 40, 50)));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    for (i, item) in items.iter().enumerate() {
        let row_y = inner.y + i as u16;
        if row_y >= inner.y + inner.height {
            break;
        }
        let is_selected = i == app.context_menu.selected && item.enabled;
        let label_style = if !item.enabled {
            Style::default()
                .fg(Color::DarkGray)
                .bg(Color::Rgb(40, 40, 50))
        } else if is_selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(180, 180, 220))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White).bg(Color::Rgb(40, 40, 50))
        };
        let line = format!(" {:<width$} ", item.label, width = label_width as usize);
        let row_rect = Rect::new(inner.x, row_y, inner.width, 1);
        frame.render_widget(Paragraph::new(Span::styled(line, label_style)), row_rect);
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
