//! Keyboard input handling for each editing mode.

use crate::app::{App, Mode};
use crate::source_control::{GitPanelSection, SidebarView};
use aura_core::AuthorId;
use crossterm::event::{KeyCode, KeyModifiers};

/// Map a character to its opening/closing delimiter pair.
fn delimiter_pair(c: char) -> (char, char) {
    match c {
        '(' | ')' | 'b' => ('(', ')'),
        '{' | '}' | 'B' => ('{', '}'),
        '[' | ']' => ('[', ']'),
        '<' | '>' => ('<', '>'),
        '"' => ('"', '"'),
        '\'' => ('\'', '\''),
        '`' => ('`', '`'),
        _ => (c, c), // Same-char delimiters.
    }
}

/// Unfocus all side panels and special focus states, returning to the editor.
fn unfocus_all_panels(app: &mut App) {
    app.terminal_focused = false;
    app.file_tree_focused = false;
    app.source_control_focused = false;
    app.chat_panel_focused = false;
    app.conversation_history_focused = false;
    app.debug_panel_focused = false;
    app.ai_visor_focused = false;
}

/// Execute a named action from the keybinding config. Returns true if handled.
fn execute_action(app: &mut App, action: &str) -> bool {
    match action {
        "toggle_terminal" => {
            if app.terminal().visible && app.terminal_focused {
                app.terminal_mut().visible = false;
                app.terminal_focused = false;
            } else {
                app.terminal_mut().visible = true;
                app.terminal_focused = true;
            }
        }
        "toggle_chat" => app.toggle_chat_panel(),
        "toggle_history" => app.toggle_conversation_history(),
        "toggle_file_tree" => {
            if app.file_tree.visible && app.file_tree_focused {
                app.file_tree_focused = false;
                app.file_tree.toggle();
            } else {
                if !app.file_tree.visible {
                    app.file_tree.toggle();
                }
                if app.sidebar_view != SidebarView::Files {
                    app.sidebar_view = SidebarView::Files;
                }
                app.file_tree_focused = true;
            }
        }
        "toggle_git" => {
            if app.source_control_focused && app.sidebar_view == SidebarView::Git {
                app.source_control_focused = false;
            } else {
                if !app.file_tree.visible {
                    app.file_tree.toggle();
                }
                if app.sidebar_view != SidebarView::Git {
                    app.sidebar_view = SidebarView::Git;
                    app.refresh_source_control();
                }
                app.source_control_focused = true;
            }
        }
        "open_file_picker" => app.open_file_picker(),
        "open_command_palette" => app.open_command_palette(),
        "open_git_graph" => app.open_git_graph(),
        "open_settings" => app.open_settings(),
        "open_outline" => app.open_outline(),
        "open_visor" | "toggle_visor" => app.toggle_ai_visor(),
        "open_branch_picker" => app.open_branch_picker(),
        "project_search" => app.open_project_search(),
        "save" => match app.tab_mut().buffer.save() {
            Ok(_) => app.set_status("Saved"),
            Err(e) => app.set_status(format!("Save failed: {e}")),
        },
        "intent" => {
            app.mode = Mode::Intent;
            app.command_input.clear();
        }
        "toggle_blame" => app.toggle_blame(),
        "cycle_aggressiveness" => app.cycle_aggressiveness(),
        "recent_decisions" => app.show_recent_decisions(),
        "next_tab" => {
            if app.tabs.count() > 1 {
                app.tabs.next();
            }
        }
        "prev_tab" => {
            if app.tabs.count() > 1 {
                app.tabs.prev();
            }
        }
        "close_tab" => {
            let idx = app.tabs.active_index();
            if app.close_tab_by_index(idx) {
                app.should_quit = true;
            }
        }
        _ => return false,
    }
    true
}

/// Handle keys in Normal mode.
pub fn handle_normal(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    // Global: if a tool call is pending approval, intercept Y/N regardless of focus.
    if app.chat_panel.pending_approval.is_some() {
        match code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.approve_pending_tool();
                return;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.deny_pending_tool();
                return;
            }
            _ => {}
        }
    }

    // When the peek definition popup is visible, intercept navigation/close keys.
    if app.peek_definition.is_some() {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.peek_definition = None;
                return;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(peek) = &mut app.peek_definition {
                    let max = peek.lines.len().saturating_sub(1);
                    if peek.scroll_offset < max {
                        peek.scroll_offset += 1;
                    }
                }
                return;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(peek) = &mut app.peek_definition {
                    peek.scroll_offset = peek.scroll_offset.saturating_sub(1);
                }
                return;
            }
            KeyCode::Enter => {
                // Navigate to definition and close peek.
                if let Some(peek) = app.peek_definition.take() {
                    let current_uri = app
                        .tab()
                        .buffer
                        .file_path()
                        .map(|p| format!("file://{}", p.display()))
                        .unwrap_or_default();
                    let peek_uri = format!("file://{}", peek.file_path.display());
                    if peek_uri == current_uri {
                        let tab = app.tab_mut();
                        tab.cursor.row = peek.target_line;
                        tab.cursor.col = peek.target_col;
                        app.clamp_cursor();
                    }
                    app.set_status(format!(
                        "Definition at {}:{}",
                        peek.target_line + 1,
                        peek.target_col + 1
                    ));
                }
                return;
            }
            _ => {
                // Any other key closes the peek and is processed normally.
                app.peek_definition = None;
            }
        }
    }

    // When the close-tab confirmation modal is visible, intercept S/D/Esc.
    if app.tab_close_confirm.is_some() {
        match code {
            KeyCode::Char('s') | KeyCode::Char('S') => {
                app.handle_close_confirm_save();
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                app.handle_close_confirm_discard();
            }
            KeyCode::Esc => {
                app.handle_close_confirm_cancel();
            }
            _ => {}
        }
        return;
    }

    // When the update modal is visible, intercept Y/N/Esc.
    if app.update_modal_visible {
        match code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                app.run_update();
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.update_modal_visible = false;
            }
            _ => {}
        }
        return;
    }

    // Dismiss update notification on any key press.
    if app.update_notification_visible {
        app.update_notification_visible = false;
    }

    // When search bar is active, route keys to search input.
    if app.search_active {
        match code {
            KeyCode::Esc => {
                app.search_active = false;
                app.search_input.clear();
                app.search_history_idx = None;
                // Keep previous search_query/matches for n/N.
            }
            KeyCode::Enter => {
                app.search_active = false;
                app.search_history_idx = None;
                if app.search_input.is_empty() {
                    // Repeat last search if input is empty.
                    if app.search_query.is_some() {
                        app.search_next();
                    }
                } else {
                    // Save to history (avoid duplicates at end).
                    let query = app.search_input.clone();
                    if app.search_history.last() != Some(&query) {
                        app.search_history.push(query);
                    }
                    app.search_query = Some(app.search_input.clone());
                    app.execute_search();
                    if app.search_matches.is_empty() {
                        app.set_status(format!("Pattern not found: {}", app.search_input));
                    } else {
                        app.search_next();
                        let total = app.search_matches.len();
                        let cur = app.search_current + 1;
                        app.set_status(format!("{cur}/{total}"));
                    }
                }
                app.search_input.clear();
            }
            KeyCode::Up => {
                // Browse search history (older).
                if !app.search_history.is_empty() {
                    let idx = match app.search_history_idx {
                        Some(i) => i.saturating_sub(1),
                        None => app.search_history.len() - 1,
                    };
                    app.search_history_idx = Some(idx);
                    app.search_input = app.search_history[idx].clone();
                    app.search_query = Some(app.search_input.clone());
                    app.execute_search();
                }
            }
            KeyCode::Down => {
                // Browse search history (newer).
                if let Some(idx) = app.search_history_idx {
                    if idx + 1 < app.search_history.len() {
                        let next = idx + 1;
                        app.search_history_idx = Some(next);
                        app.search_input = app.search_history[next].clone();
                    } else {
                        // Past the end — clear to empty.
                        app.search_history_idx = None;
                        app.search_input.clear();
                    }
                    app.search_query = Some(app.search_input.clone());
                    app.execute_search();
                }
            }
            KeyCode::Backspace => {
                app.search_input.pop();
                // Incremental search.
                if !app.search_input.is_empty() {
                    app.search_query = Some(app.search_input.clone());
                    app.execute_search();
                    if !app.search_matches.is_empty() {
                        app.jump_to_nearest_match();
                    }
                } else {
                    app.search_matches.clear();
                }
            }
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                app.search_input.push(c);
                // Incremental search: update matches and jump to nearest.
                app.search_query = Some(app.search_input.clone());
                app.execute_search();
                if !app.search_matches.is_empty() {
                    app.jump_to_nearest_match();
                }
            }
            _ => {}
        }
        return;
    }

    // ── Global panel-switching shortcuts ──────────────────────────────
    // These work from ANY focused panel (terminal, git, chat, file tree, etc.)
    // so users never have to Esc back to the editor first.
    if modifiers.contains(KeyModifiers::CONTROL) {
        // Check custom global keybindings from aura.toml first.
        if let Some(action) = app.config.keybindings.global_action(code, modifiers) {
            let action = action.to_string();
            unfocus_all_panels(app);
            if execute_action(app, &action) {
                return;
            }
        }
        let handled = match code {
            // Ctrl+T / Ctrl+` — toggle terminal.
            KeyCode::Char('t') | KeyCode::Char('`') => {
                // Unfocus whatever panel is focused.
                unfocus_all_panels(app);
                if app.terminal().visible && app.terminal_focused {
                    app.terminal_mut().visible = false;
                    app.terminal_focused = false;
                } else {
                    app.terminal_mut().visible = true;
                    app.terminal_focused = true;
                }
                true
            }
            // Ctrl+G — toggle git / source control panel.
            KeyCode::Char('g') => {
                unfocus_all_panels(app);
                if app.source_control_focused && app.sidebar_view == SidebarView::Git {
                    // Already focused on git — close sidebar.
                    app.source_control_focused = false;
                } else {
                    if !app.file_tree.visible {
                        app.file_tree.toggle();
                    }
                    if app.sidebar_view != SidebarView::Git {
                        app.sidebar_view = SidebarView::Git;
                        app.refresh_source_control();
                    }
                    app.source_control_focused = true;
                }
                true
            }
            // Ctrl+N — toggle file tree sidebar.
            KeyCode::Char('n') => {
                unfocus_all_panels(app);
                if app.file_tree.visible && app.file_tree_focused {
                    app.file_tree_focused = false;
                    app.file_tree.toggle();
                } else {
                    if !app.file_tree.visible {
                        app.file_tree.toggle();
                    }
                    if app.sidebar_view != SidebarView::Files {
                        app.sidebar_view = SidebarView::Files;
                    }
                    app.file_tree_focused = true;
                }
                true
            }
            // Ctrl+J — toggle chat panel.
            KeyCode::Char('j') => {
                unfocus_all_panels(app);
                app.toggle_chat_panel();
                true
            }
            // Ctrl+H — toggle conversation history panel.
            // Note: some terminals send Backspace for Ctrl+H.
            KeyCode::Char('h') | KeyCode::Backspace => {
                unfocus_all_panels(app);
                app.toggle_conversation_history();
                true
            }
            // Ctrl+O — open document outline.
            KeyCode::Char('o') => {
                unfocus_all_panels(app);
                app.open_outline();
                true
            }
            // Ctrl+F — open project-wide search.
            KeyCode::Char('f') => {
                unfocus_all_panels(app);
                app.open_project_search();
                true
            }
            // Ctrl+I — toggle AI Visor panel.
            KeyCode::Char('i') => {
                unfocus_all_panels(app);
                app.toggle_ai_visor();
                true
            }
            // Ctrl+, — open settings.
            KeyCode::Char(',') => {
                unfocus_all_panels(app);
                app.open_settings();
                true
            }
            // Ctrl+Shift+G — open git graph.
            KeyCode::Char('G') if modifiers.contains(KeyModifiers::SHIFT) => {
                unfocus_all_panels(app);
                app.open_git_graph();
                true
            }
            _ => false,
        };
        if handled {
            return;
        }
    }

    // When rename mode is active, route keys to the rename input.
    if app.rename_active {
        match code {
            KeyCode::Esc => {
                app.rename_active = false;
                app.rename_input.clear();
                app.set_status("Rename cancelled");
            }
            KeyCode::Enter => {
                app.lsp_rename_execute();
            }
            KeyCode::Backspace => {
                app.rename_input.pop();
            }
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                app.rename_input.push(c);
            }
            _ => {}
        }
        return;
    }

    // When the references panel is visible, route navigation keys.
    if app.references_panel.is_some() {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.references_panel = None;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(panel) = &mut app.references_panel {
                    panel.select_down();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(panel) = &mut app.references_panel {
                    panel.select_up();
                }
            }
            KeyCode::Enter => {
                app.goto_reference();
            }
            _ => {}
        }
        return;
    }

    // When the registers modal is visible, route keys to it.
    if app.registers_visible {
        if let Some(_editing_reg) = app.macro_editing {
            // Macro editing sub-view.
            match code {
                KeyCode::Esc => {
                    app.macro_editing = None;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if let Some(ch) = app.macro_editing {
                        let len = app.macro_registers.get(&ch).map_or(0, |k| k.len());
                        if app.macro_edit_selected < len.saturating_sub(1) {
                            app.macro_edit_selected += 1;
                        }
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    app.macro_edit_selected = app.macro_edit_selected.saturating_sub(1);
                }
                KeyCode::Char('d') | KeyCode::Char('x') | KeyCode::Delete => {
                    app.delete_macro_key();
                }
                _ => {}
            }
        } else {
            // Register list view.
            match code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    app.registers_visible = false;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    let count = app.register_entries().len();
                    if app.registers_selected < count.saturating_sub(1) {
                        app.registers_selected += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    app.registers_selected = app.registers_selected.saturating_sub(1);
                }
                KeyCode::Enter | KeyCode::Char('e') => {
                    app.edit_selected_macro();
                }
                _ => {}
            }
        }
        return;
    }

    // When the document outline is visible, route keys to it.
    if app.outline_visible {
        match code {
            KeyCode::Esc => {
                app.outline_visible = false;
            }
            KeyCode::Enter => {
                app.goto_outline_selection();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = app.outline_filtered.len().saturating_sub(1);
                if app.outline_selected < max {
                    app.outline_selected += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if app.outline_selected > 0 {
                    app.outline_selected -= 1;
                }
            }
            KeyCode::Backspace => {
                app.outline_query.pop();
                app.filter_outline();
            }
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                app.outline_query.push(c);
                app.filter_outline();
            }
            _ => {}
        }
        return;
    }

    // When the project search panel is active, route keys to it.
    if app.project_search.visible {
        use crate::project_search::SearchFocus;
        match app.project_search.focus {
            SearchFocus::Query | SearchFocus::Replace => match code {
                KeyCode::Esc => app.project_search.close(),
                KeyCode::Enter => {
                    if app.project_search.focus == SearchFocus::Query {
                        app.execute_project_search();
                    } else {
                        app.project_search.focus = SearchFocus::Results;
                    }
                }
                KeyCode::Tab => app.project_search.cycle_focus(),
                KeyCode::Backspace => app.project_search.backspace(),
                KeyCode::Char('r') if modifiers.contains(KeyModifiers::CONTROL) => {
                    app.project_search.toggle_replace_mode();
                }
                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    app.project_search.type_char(c);
                }
                _ => {}
            },
            SearchFocus::Results => match code {
                KeyCode::Esc => app.project_search.close(),
                KeyCode::Tab => app.project_search.cycle_focus(),
                KeyCode::Char('j') | KeyCode::Down => app.project_search.select_down(),
                KeyCode::Char('k') | KeyCode::Up => app.project_search.select_up(),
                KeyCode::Char('d') => app.project_search.page_down(),
                KeyCode::Char('u') => app.project_search.page_up(),
                KeyCode::Enter => app.goto_search_result(),
                KeyCode::Char('R') => app.replace_all_project(),
                KeyCode::Char('r') if modifiers.contains(KeyModifiers::CONTROL) => {
                    app.project_search.toggle_replace_mode();
                }
                _ => {}
            },
        }
        return;
    }

    // When the AI Visor panel is focused, route keys to it.
    if app.ai_visor_focused {
        match code {
            KeyCode::Esc => {
                app.ai_visor_focused = false;
            }
            KeyCode::Char('1') => {
                app.ai_visor.active_tab = crate::ai_visor::VisorTab::Overview;
                app.ai_visor.selected = 0;
            }
            KeyCode::Char('2') => {
                app.ai_visor.active_tab = crate::ai_visor::VisorTab::Settings;
                app.ai_visor.selected = 0;
            }
            KeyCode::Char('3') => {
                app.ai_visor.active_tab = crate::ai_visor::VisorTab::Skills;
                app.ai_visor.selected = 0;
            }
            KeyCode::Char('4') => {
                app.ai_visor.active_tab = crate::ai_visor::VisorTab::Hooks;
                app.ai_visor.selected = 0;
            }
            KeyCode::Char('5') => {
                app.ai_visor.active_tab = crate::ai_visor::VisorTab::Plugins;
                app.ai_visor.selected = 0;
            }
            KeyCode::Tab => app.ai_visor.next_tab(),
            KeyCode::Char('j') | KeyCode::Down => app.ai_visor.select_down(),
            KeyCode::Char('k') | KeyCode::Up => app.ai_visor.select_up(),
            KeyCode::Char('e') | KeyCode::Enter => {
                // Open the selected skill's source file in the editor.
                if let Some(path) = app.ai_visor.selected_skill_path().map(|p| p.to_path_buf()) {
                    if let Err(e) = app.open_file(path) {
                        app.set_status(e);
                    }
                    app.ai_visor_focused = false;
                }
            }
            _ => {}
        }
        return;
    }

    // When the undo tree modal is visible, route keys to it.
    if app.undo_tree.is_some() {
        if let Some(modal) = &mut app.undo_tree {
            match code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    app.undo_tree = None;
                }
                KeyCode::Char('j') | KeyCode::Down => modal.select_down(),
                KeyCode::Char('k') | KeyCode::Up => modal.select_up(),
                KeyCode::Char('d') => modal.page_down(),
                KeyCode::Char('u') => modal.page_up(),
                KeyCode::Char('t') => modal.toggle_detail(),
                KeyCode::Enter => {
                    app.restore_to_undo_pos();
                }
                _ => {}
            }
        }
        return;
    }

    // When the debug panel is focused, route keys to debug navigation.
    if app.debug_panel_focused {
        match code {
            KeyCode::Esc => {
                app.debug_panel_focused = false;
            }
            KeyCode::Char('1') => {
                app.debug_panel.active_tab = crate::debug_panel::DebugTab::CallStack;
            }
            KeyCode::Char('2') => {
                app.debug_panel.active_tab = crate::debug_panel::DebugTab::Variables;
            }
            KeyCode::Char('3') => {
                app.debug_panel.active_tab = crate::debug_panel::DebugTab::Output;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                app.debug_panel.select_down();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.debug_panel.select_up();
            }
            KeyCode::Enter => {
                // In call stack: navigate to selected frame.
                if app.debug_panel.active_tab == crate::debug_panel::DebugTab::CallStack {
                    let idx = app.debug_panel.state.selected_frame;
                    if let Some(frame) = app.debug_panel.state.stack_frames.get(idx) {
                        if let Some(ref path) = frame.source_path {
                            let _line = frame.line.saturating_sub(1) as usize;
                            // Update stopped location to the selected frame.
                            app.debug_panel.state.stopped_file = Some(path.clone());
                            app.debug_panel.state.stopped_line = Some(_line);
                        }
                    }
                }
                // In variables: toggle expand (future: request children).
                if app.debug_panel.active_tab == crate::debug_panel::DebugTab::Variables {
                    let idx = app.debug_panel.state.selected_var;
                    if let Some(node) = app.debug_panel.state.variables.get_mut(idx) {
                        if node.expandable {
                            node.expanded = !node.expanded;
                            // TODO: fetch children via request_variables
                        }
                    }
                }
            }
            _ => {}
        }
        return;
    }

    // When the terminal pane is focused, route all keystrokes to the PTY.
    if app.terminal_focused {
        // Reset idle timer for AI suggestions.
        app.terminal_last_key = std::time::Instant::now();

        // Handle AI suggestion: Tab to accept, any other key dismisses.
        if app.terminal_suggestion.is_some() {
            if code == KeyCode::Tab {
                if let Some(suggestion) = app.terminal_suggestion.take() {
                    app.terminal_mut().send_bytes(suggestion.as_bytes());
                }
                return;
            }
            // Dismiss suggestion on any other key (key is still processed below).
            app.terminal_suggestion = None;
        }

        match code {
            // Esc — unfocus terminal (return focus to editor).
            KeyCode::Esc => {
                app.terminal_focused = false;
            }
            // Ctrl+Shift+T — new terminal tab.
            KeyCode::Char('T')
                if modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) =>
            {
                app.new_terminal_tab();
            }
            // Ctrl+Shift+] — next terminal tab.
            KeyCode::Char(']')
                if modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) =>
            {
                app.next_terminal_tab();
            }
            // Ctrl+Shift+[ — previous terminal tab.
            KeyCode::Char('[')
                if modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) =>
            {
                app.prev_terminal_tab();
            }
            // Ctrl+Shift+Up / Ctrl+Shift+Down — resize terminal pane.
            KeyCode::Up if modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) => {
                app.terminal_mut().height = (app.terminal().height + 2).min(50);
            }
            KeyCode::Down if modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) => {
                app.terminal_mut().height = app.terminal().height.saturating_sub(2).max(5);
            }
            // Cmd+C (macOS) or Ctrl+Shift+C — copy selection to clipboard.
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::SUPER) => {
                if let Some(text) = app.terminal().selected_text() {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(&text);
                    }
                    app.terminal_mut().clear_selection();
                    app.set_status("Copied from terminal");
                }
            }
            KeyCode::Char('C')
                if modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) =>
            {
                if let Some(text) = app.terminal().selected_text() {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(&text);
                    }
                    app.terminal_mut().clear_selection();
                    app.set_status("Copied from terminal");
                }
            }
            // Cmd+V (macOS) or Ctrl+Shift+V — paste from clipboard into terminal.
            KeyCode::Char('v') if modifiers.contains(KeyModifiers::SUPER) => {
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    if let Ok(text) = clipboard.get_text() {
                        // Use bracketed paste to avoid shell interpretation issues.
                        app.terminal_mut().send_bytes(b"\x1b[200~");
                        app.terminal_mut().send_bytes(text.as_bytes());
                        app.terminal_mut().send_bytes(b"\x1b[201~");
                    }
                }
            }
            KeyCode::Char('V')
                if modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) =>
            {
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    if let Ok(text) = clipboard.get_text() {
                        app.terminal_mut().send_bytes(b"\x1b[200~");
                        app.terminal_mut().send_bytes(text.as_bytes());
                        app.terminal_mut().send_bytes(b"\x1b[201~");
                    }
                }
            }
            // Shift+Arrow keys — text selection.
            KeyCode::Left if modifiers.contains(KeyModifiers::SHIFT) => {
                let (_, cursor_row, cursor_col) = app.terminal().snapshot();
                let new_col = cursor_col.saturating_sub(1);
                app.terminal_mut().start_selection(cursor_row, cursor_col);
                app.terminal_mut().extend_selection(cursor_row, new_col);
            }
            KeyCode::Right if modifiers.contains(KeyModifiers::SHIFT) => {
                let (snapshot, cursor_row, cursor_col) = app.terminal().snapshot();
                let max_col = snapshot
                    .first()
                    .map(|r| r.len().saturating_sub(1))
                    .unwrap_or(0);
                let new_col = (cursor_col + 1).min(max_col);
                app.terminal_mut().start_selection(cursor_row, cursor_col);
                app.terminal_mut().extend_selection(cursor_row, new_col);
            }
            KeyCode::Up if modifiers.contains(KeyModifiers::SHIFT) => {
                let (_, cursor_row, cursor_col) = app.terminal().snapshot();
                let new_row = cursor_row.saturating_sub(1);
                app.terminal_mut().start_selection(cursor_row, cursor_col);
                app.terminal_mut().extend_selection(new_row, cursor_col);
            }
            KeyCode::Down if modifiers.contains(KeyModifiers::SHIFT) => {
                let (snapshot, cursor_row, cursor_col) = app.terminal().snapshot();
                let max_row = snapshot.len().saturating_sub(1);
                let new_row = (cursor_row + 1).min(max_row);
                app.terminal_mut().start_selection(cursor_row, cursor_col);
                app.terminal_mut().extend_selection(new_row, cursor_col);
            }
            // Shift+Home — select to start of line.
            KeyCode::Home if modifiers.contains(KeyModifiers::SHIFT) => {
                let (_, cursor_row, cursor_col) = app.terminal().snapshot();
                app.terminal_mut().start_selection(cursor_row, cursor_col);
                app.terminal_mut().extend_selection(cursor_row, 0);
            }
            // Shift+End — select to end of line.
            KeyCode::End if modifiers.contains(KeyModifiers::SHIFT) => {
                let (snapshot, cursor_row, cursor_col) = app.terminal().snapshot();
                let max_col = snapshot
                    .get(cursor_row)
                    .map(|r| r.len().saturating_sub(1))
                    .unwrap_or(0);
                app.terminal_mut().start_selection(cursor_row, cursor_col);
                app.terminal_mut().extend_selection(cursor_row, max_col);
            }
            // Cmd+A or Ctrl+Shift+A — select all visible terminal content.
            KeyCode::Char('a') if modifiers.contains(KeyModifiers::SUPER) => {
                let (snapshot, _, _) = app.terminal().snapshot();
                let max_row = snapshot.len().saturating_sub(1);
                let max_col = snapshot
                    .last()
                    .map(|r| r.len().saturating_sub(1))
                    .unwrap_or(0);
                app.terminal_mut().selection_anchor = Some((0, 0));
                app.terminal_mut().selection_end = Some((max_row, max_col));
            }
            // Ctrl+C — send interrupt (clear selection first if any).
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.terminal_mut().clear_selection();
                app.terminal_mut().send_ctrl_c();
            }
            // Ctrl+D — send EOF.
            KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.terminal_mut().send_ctrl_d();
            }
            // Ctrl+L — clear screen.
            KeyCode::Char('l') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.terminal_mut().send_ctrl_l();
            }
            // Other Ctrl+char — send as control code.
            KeyCode::Char(c) if modifiers.contains(KeyModifiers::CONTROL) => {
                let ctrl_byte = (c as u8).wrapping_sub(b'a').wrapping_add(1);
                app.terminal_mut().send_bytes(&[ctrl_byte]);
            }
            KeyCode::Enter => {
                app.terminal_mut().clear_selection();
                app.terminal_mut().send_enter();
            }
            KeyCode::Backspace => {
                app.terminal_mut().clear_selection();
                app.terminal_mut().send_backspace();
            }
            KeyCode::Tab => {
                app.terminal_mut().send_tab();
            }
            KeyCode::Up => {
                app.terminal_mut().clear_selection();
                app.terminal_mut().send_arrow_up();
            }
            KeyCode::Down => {
                app.terminal_mut().clear_selection();
                app.terminal_mut().send_arrow_down();
            }
            KeyCode::Left => {
                app.terminal_mut().clear_selection();
                app.terminal_mut().send_arrow_left();
            }
            KeyCode::Right => {
                app.terminal_mut().clear_selection();
                app.terminal_mut().send_arrow_right();
            }
            KeyCode::Char(c) => {
                app.terminal_mut().clear_selection();
                app.terminal_mut().send_char(c);
            }
            _ => {}
        }
        return;
    }

    // When the file tree sidebar is focused, route navigation keys to it.
    if app.file_tree_focused {
        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                app.file_tree.select_down();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.file_tree.select_up();
            }
            KeyCode::Enter | KeyCode::Char('l') => {
                app.open_file_tree_selection();
            }
            KeyCode::Char('h') => {
                // Collapse current dir, or move to parent.
                let idx = app.file_tree.selected;
                if idx < app.file_tree.entries.len() {
                    let entry = &app.file_tree.entries[idx];
                    if entry.is_dir && entry.expanded {
                        app.file_tree.toggle_expand();
                    } else if entry.depth > 0 {
                        // Jump up to the parent directory entry.
                        let target_depth = entry.depth - 1;
                        let mut i = idx;
                        while i > 0 {
                            i -= 1;
                            if app.file_tree.entries[i].is_dir
                                && app.file_tree.entries[i].depth == target_depth
                            {
                                app.file_tree.selected = i;
                                break;
                            }
                        }
                    }
                }
            }
            KeyCode::Esc => {
                app.file_tree_focused = false;
            }
            _ => {}
        }
        return;
    }

    // When the source control panel is focused, route keys to it.
    if app.source_control_focused {
        // Handle commit message editing sub-mode.
        if app.source_control.editing_commit_message {
            match code {
                KeyCode::Esc => {
                    app.source_control.editing_commit_message = false;
                }
                KeyCode::Enter => {
                    app.source_control.commit_message.push('\n');
                }
                KeyCode::Backspace => {
                    app.source_control.commit_message.pop();
                }
                // Ctrl+V / Cmd+V — paste from system clipboard.
                KeyCode::Char('v')
                    if modifiers.contains(KeyModifiers::CONTROL)
                        || modifiers.contains(KeyModifiers::SUPER) =>
                {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        if let Ok(text) = clipboard.get_text() {
                            app.source_control.commit_message.push_str(&text);
                        }
                    }
                }
                // Ctrl+P — paste from internal yank register.
                KeyCode::Char('p') if modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Some(ref text) = app.register {
                        app.source_control.commit_message.push_str(text);
                    }
                }
                // Ctrl+G — generate AI commit message.
                KeyCode::Char('g') if modifiers.contains(KeyModifiers::CONTROL) => {
                    app.generate_commit_message();
                }
                KeyCode::Char(c) => {
                    app.source_control.commit_message.push(c);
                }
                _ => {}
            }
            return;
        }

        // Handle pending discard confirmation.
        if app.source_control.pending_discard.is_some() {
            match code {
                KeyCode::Char('y') => {
                    app.sc_discard_selected();
                    app.set_status("Changes discarded");
                }
                _ => {
                    app.source_control.pending_discard = None;
                    app.set_status("Discard cancelled");
                }
            }
            return;
        }

        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                app.source_control.select_down();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.source_control.select_up();
            }
            KeyCode::Tab => {
                app.source_control.next_section();
            }
            KeyCode::BackTab => {
                app.source_control.prev_section();
            }
            KeyCode::Char('s') => {
                app.sc_stage_selected();
            }
            KeyCode::Char('S') => {
                app.sc_stage_all();
            }
            KeyCode::Char('u') => {
                app.sc_unstage_selected();
            }
            KeyCode::Char('d') => {
                if app.source_control.focused_section == GitPanelSection::ChangedFiles {
                    if let Some(entry) = app.source_control.changed.get(app.source_control.selected)
                    {
                        let path = entry.rel_path.clone();
                        app.set_status(format!("Discard changes to {}? (y to confirm)", path));
                        app.source_control.pending_discard = Some(path);
                    }
                } else if app.source_control.focused_section == GitPanelSection::Stashes {
                    // Drop the selected stash.
                    app.sc_stash_drop();
                }
            }
            // z — stash push (save current changes to a stash).
            KeyCode::Char('z') => {
                app.sc_stash_push();
            }
            // p — stash pop (when in Stashes section).
            KeyCode::Char('p')
                if app.source_control.focused_section == GitPanelSection::Stashes =>
            {
                app.sc_stash_pop();
            }
            KeyCode::Char('c') => {
                app.sc_commit();
            }
            KeyCode::Char('i') | KeyCode::Enter
                if app.source_control.focused_section == GitPanelSection::CommitMessage =>
            {
                app.source_control.editing_commit_message = true;
            }
            KeyCode::Enter => {
                // Open merge view for conflicts, diff view for everything else.
                if let Some(rel_path) = app.source_control.selected_path().map(|s| s.to_string()) {
                    let is_conflict = app
                        .source_control
                        .selected_entry()
                        .map(|e| e.status == crate::source_control::GitFileStatus::Conflict)
                        .unwrap_or(false);
                    if is_conflict {
                        app.open_merge_view(&rel_path);
                    } else {
                        app.open_diff_view(&rel_path);
                    }
                }
            }
            KeyCode::Esc => {
                app.source_control_focused = false;
            }
            _ => {}
        }
        return;
    }

    // When the chat panel is focused, route all keys to chat input.
    if app.chat_panel_focused {
        // Ctrl+P toggles agent pause/resume when in agent mode.
        if code == KeyCode::Char('p') && modifiers.contains(KeyModifiers::CONTROL) {
            if let Some(ref session) = app.agent_mode {
                if session.paused {
                    app.resume_agent();
                } else {
                    app.pause_agent();
                }
                return;
            }
        }

        // If a tool call is pending approval, intercept Y/N.
        if app.chat_panel.pending_approval.is_some() {
            match code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    app.approve_pending_tool();
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    app.deny_pending_tool();
                }
                _ => {}
            }
            return;
        }

        // If an agent plan is pending approval, intercept Y/N.
        if app.chat_panel.plan_pending_approval {
            match code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    app.approve_agent_plan();
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    app.deny_agent_plan();
                }
                _ => {}
            }
            return;
        }

        // @-mention autocomplete routing.
        if app.chat_panel.mention_active {
            match code {
                KeyCode::Esc => {
                    app.chat_panel.cancel_mention();
                    return;
                }
                KeyCode::Enter | KeyCode::Tab => {
                    app.chat_panel.complete_mention();
                    return;
                }
                KeyCode::Down => {
                    app.chat_panel.mention_next();
                    return;
                }
                KeyCode::Up => {
                    app.chat_panel.mention_prev();
                    return;
                }
                KeyCode::Backspace => {
                    app.chat_panel.mention_backspace();
                    return;
                }
                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    app.chat_panel.mention_type_char(c);
                    return;
                }
                _ => {
                    app.chat_panel.cancel_mention();
                }
            }
            return;
        }

        match code {
            KeyCode::Enter => {
                if !app.chat_panel.streaming {
                    app.send_chat_message();
                }
            }
            KeyCode::Char('@') if !modifiers.contains(KeyModifiers::CONTROL) => {
                // Start @-mention autocomplete.
                app.chat_panel.input_char('@');
                app.chat_panel.start_mention();
            }
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                app.chat_panel.input_char(c);
            }
            KeyCode::Backspace => {
                app.chat_panel.input_backspace();
            }
            KeyCode::Delete => {
                app.chat_panel.input_delete();
            }
            KeyCode::Left => {
                app.chat_panel.input_left();
            }
            KeyCode::Right => {
                app.chat_panel.input_right();
            }
            KeyCode::Home => {
                app.chat_panel.input_home();
            }
            KeyCode::End => {
                app.chat_panel.input_end();
            }
            KeyCode::Up if modifiers.contains(KeyModifiers::CONTROL) => {
                app.chat_panel.scroll_up();
            }
            KeyCode::Down if modifiers.contains(KeyModifiers::CONTROL) => {
                app.chat_panel.scroll_down();
            }
            KeyCode::Up => {
                let wrap_w = app.chat_panel_rect.width.saturating_sub(4) as usize;
                app.chat_panel.input_up(wrap_w);
            }
            KeyCode::Down => {
                let wrap_w = app.chat_panel_rect.width.saturating_sub(4) as usize;
                app.chat_panel.input_down(wrap_w);
            }
            KeyCode::PageUp => {
                app.chat_panel.page_up(10);
            }
            KeyCode::PageDown => {
                app.chat_panel.page_down(10);
            }
            KeyCode::Esc => {
                app.chat_panel_focused = false;
            }
            _ => {}
        }
        return;
    }

    // When the conversation history panel is focused, route keys to it.
    if app.conversation_history_focused {
        // Detail modal: full-screen conversation view.
        if app.conversation_history.detail_view {
            match code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    app.conversation_history.close_detail();
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    app.conversation_history.detail_scroll_down();
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    app.conversation_history.detail_scroll_up();
                }
                KeyCode::Char('d') => {
                    app.conversation_history.detail_page_down(10);
                }
                KeyCode::Char('u') => {
                    app.conversation_history.detail_page_up(10);
                }
                _ => {}
            }
            return;
        }

        match code {
            KeyCode::Char('/') if !app.conversation_history.search_active => {
                app.conversation_history.start_search();
            }
            // Search mode input.
            KeyCode::Char(c) if app.conversation_history.search_active => {
                app.conversation_history.search_type_char(c);
            }
            KeyCode::Backspace if app.conversation_history.search_active => {
                app.conversation_history.search_backspace();
            }
            KeyCode::Esc if app.conversation_history.search_active => {
                app.conversation_history.cancel_search();
            }
            // Normal navigation.
            KeyCode::Char('j') | KeyCode::Down => {
                app.conversation_history.select_down();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.conversation_history.select_up();
            }
            KeyCode::Enter => {
                if app.conversation_history.expanded.is_some() {
                    // Already expanded — open the detail modal.
                    app.conversation_history.open_detail();
                } else {
                    app.conversation_history_toggle_expand();
                }
            }
            KeyCode::Char('u') => {
                app.conversation_history.scroll_messages_up();
            }
            KeyCode::Char('d') => {
                app.conversation_history.scroll_messages_down();
            }
            KeyCode::Esc => {
                app.conversation_history.visible = false;
                app.conversation_history_focused = false;
            }
            _ => {}
        }
        return;
    }

    // NOTE: Global panel-switching shortcuts (Ctrl+T/G/N/J/H/,) are handled
    // earlier in the function, before panel focus handlers, so they work from
    // any focused panel.

    // Route keys to the settings modal when visible.
    if app.settings_modal.visible {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.close_settings();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                app.settings_modal.select_down();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.settings_modal.select_up();
            }
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right | KeyCode::Char('l') => {
                app.settings_modal.toggle_selected();
                // Apply changes live.
                app.settings_modal.apply_to_config(&mut app.config);
                app.show_authorship = app.config.editor.show_authorship;
                app.chat_panel.max_context_messages = app.config.conversations.max_context_messages;
            }
            KeyCode::Left | KeyCode::Char('h') => {
                app.settings_modal.decrement_selected();
                app.settings_modal.apply_to_config(&mut app.config);
                app.show_authorship = app.config.editor.show_authorship;
                app.chat_panel.max_context_messages = app.config.conversations.max_context_messages;
            }
            _ => {}
        }
        return;
    }

    // Ctrl+W — toggle split pane focus.
    if code == KeyCode::Char('w') && modifiers.contains(KeyModifiers::CONTROL) {
        app.split_toggle_focus();
        return;
    }

    // F1 — open help from any mode.
    if code == KeyCode::F(1) {
        app.help.open();
        return;
    }

    // F2 — rename symbol.
    if code == KeyCode::F(2) {
        app.lsp_rename_start();
        return;
    }

    // F9 — toggle breakpoint (works with or without active debug session).
    if code == KeyCode::F(9) {
        app.toggle_breakpoint();
        return;
    }

    // F5 — continue or start debug session.
    if code == KeyCode::F(5) {
        if modifiers.contains(KeyModifiers::SHIFT) {
            // Shift+F5 — stop debug session.
            app.debug_stop();
        } else if app.dap_client.is_some() {
            app.debug_continue();
        } else {
            app.start_debug_session();
        }
        return;
    }

    // F10 — step over.
    if code == KeyCode::F(10) && app.dap_client.is_some() {
        app.debug_step_over();
        return;
    }

    // F11 — step in / Shift+F11 — step out.
    if code == KeyCode::F(11) && app.dap_client.is_some() {
        if modifiers.contains(KeyModifiers::SHIFT) {
            app.debug_step_out();
        } else {
            app.debug_step_in();
        }
        return;
    }

    // Route keys to the help overlay when it is visible.
    if app.help.visible {
        match code {
            KeyCode::Esc => {
                app.help.back();
            }
            KeyCode::Enter => {
                app.help.enter();
            }
            KeyCode::Backspace => {
                if app.help.in_content_view() {
                    app.help.back();
                } else {
                    app.help.backspace();
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if app.help.in_topics_view() {
                    app.help.select_down();
                } else {
                    app.help.scroll_down();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if app.help.in_topics_view() {
                    app.help.select_up();
                } else {
                    app.help.scroll_up();
                }
            }
            KeyCode::Char('u') if app.help.in_content_view() => {
                app.help.page_up(15);
            }
            KeyCode::Char('d') if app.help.in_content_view() => {
                app.help.page_down(15);
            }
            KeyCode::Char(c)
                if app.help.in_topics_view() && !modifiers.contains(KeyModifiers::CONTROL) =>
            {
                app.help.type_char(c);
            }
            _ => {}
        }
        return;
    }

    // Ctrl+B — open branch picker from any mode.
    if code == KeyCode::Char('b') && modifiers.contains(KeyModifiers::CONTROL) {
        app.open_branch_picker();
        return;
    }

    // Route keys to the git graph modal when visible.
    if app.git_graph.visible {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => app.git_graph.close(),
            KeyCode::Char('j') | KeyCode::Down => {
                app.git_graph.select_down();
                app.load_graph_commit_files();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.git_graph.select_up();
                app.load_graph_commit_files();
            }
            KeyCode::Char('d') => app.git_graph.page_down(10),
            KeyCode::Char('u') => app.git_graph.page_up(10),
            KeyCode::Enter => {
                app.git_graph.show_detail = !app.git_graph.show_detail;
            }
            // c — open AI conversation linked to this commit.
            KeyCode::Char('c') => {
                app.open_graph_commit_conversation();
            }
            _ => {}
        }
        return;
    }

    // Route keys to the branch picker when visible.
    if app.branch_picker.visible {
        match code {
            KeyCode::Esc => app.branch_picker.close(),
            KeyCode::Enter => app.execute_branch_pick(),
            KeyCode::Backspace => app.branch_picker.backspace(),
            KeyCode::Up | KeyCode::Char('k') => app.branch_picker.select_up(),
            KeyCode::Down | KeyCode::Char('j') => app.branch_picker.select_down(),
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                app.branch_picker.type_char(c);
            }
            _ => {}
        }
        return;
    }

    // Ctrl+P — open fuzzy file picker (VS Code convention).
    if code == KeyCode::Char('p')
        && modifiers.contains(KeyModifiers::CONTROL)
        && !modifiers.contains(KeyModifiers::SHIFT)
    {
        app.open_file_picker();
        return;
    }
    // Ctrl+Shift+P — open command palette.
    if code == KeyCode::Char('P') && modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT)
    {
        app.open_command_palette();
        return;
    }

    // Route keys to the command palette when visible.
    if app.command_palette.visible {
        match code {
            KeyCode::Esc => app.command_palette.close(),
            KeyCode::Enter => app.execute_palette_selection(),
            KeyCode::Backspace => app.command_palette.backspace(),
            KeyCode::Up | KeyCode::Char('k') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.command_palette.select_up();
            }
            KeyCode::Down | KeyCode::Char('j') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.command_palette.select_down();
            }
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                app.command_palette.type_char(c);
            }
            _ => {}
        }
        return;
    }

    // Route keys to the file picker when it is visible.
    if app.file_picker.visible {
        match code {
            KeyCode::Esc => {
                app.file_picker.close();
            }
            KeyCode::Enter => {
                app.open_selected_file();
            }
            KeyCode::Backspace => {
                app.file_picker.backspace();
            }
            KeyCode::Up | KeyCode::Char('k') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.file_picker.select_up();
            }
            KeyCode::Down | KeyCode::Char('j') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.file_picker.select_down();
            }
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                app.file_picker.type_char(c);
            }
            _ => {}
        }
        return;
    }

    // Close conversation panel if open.
    if app.conversation_panel.is_some() {
        if code == KeyCode::Esc || code == KeyCode::Char('q') {
            app.conversation_panel = None;
            return;
        }
        // Scroll conversation panel.
        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(panel) = &mut app.conversation_panel {
                    panel.scroll = panel.scroll.saturating_add(1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(panel) = &mut app.conversation_panel {
                    panel.scroll = panel.scroll.saturating_sub(1);
                }
            }
            _ => {}
        }
        return;
    }

    // Leader key sequences: <Space> followed by another key.
    if app.leader_pending {
        app.leader_pending = false;
        handle_leader(app, code);
        return;
    }

    // g-prefix sequences: gg → top, gd → definition, gt/gT → tab nav.
    // Handle mark setting: m{a-z}.
    if app.mark_pending {
        app.mark_pending = false;
        if let KeyCode::Char(c) = code {
            if c.is_ascii_lowercase() {
                let cursor = app.tab().cursor;
                app.tab_mut().marks.insert(c, cursor);
                app.set_status(format!("Mark '{c}' set"));
            }
        }
        return;
    }

    // Handle mark jumping: '{a-z} or `{a-z}.
    if app.jump_mark_pending {
        app.jump_mark_pending = false;
        if let KeyCode::Char(c) = code {
            if c.is_ascii_lowercase() {
                if let Some(&mark) = app.tab().marks.get(&c) {
                    app.tab_mut().cursor = mark;
                    app.set_status(format!("Jumped to mark '{c}'"));
                } else {
                    app.set_status(format!("Mark '{c}' not set"));
                }
            }
        }
        return;
    }

    // Handle surround editing (cs/ds/ys).
    if let Some(ref state) = app.surround_pending.clone() {
        if let KeyCode::Char(c) = code {
            match state {
                crate::app::SurroundState::ChangeWaitOld => {
                    app.surround_pending = Some(crate::app::SurroundState::ChangeWaitNew(c));
                    return;
                }
                crate::app::SurroundState::ChangeWaitNew(old) => {
                    // Change surrounding: replace old delimiters with new.
                    let old = *old;
                    let (open_old, close_old) = delimiter_pair(old);
                    let (open_new, close_new) = delimiter_pair(c);
                    let char_idx = app.tab().buffer.cursor_to_char_idx(&app.tab().cursor);
                    if let Some((start, end)) = app
                        .tab()
                        .buffer
                        .find_around_delimited(char_idx, open_old, close_old)
                    {
                        // Replace closing first (higher index), then opening.
                        let close_idx = end.saturating_sub(1);
                        app.tab_mut().buffer.delete(
                            close_idx,
                            close_idx + 1,
                            aura_core::AuthorId::Human,
                        );
                        app.tab_mut().buffer.insert(
                            close_idx,
                            &close_new.to_string(),
                            aura_core::AuthorId::Human,
                        );
                        app.tab_mut()
                            .buffer
                            .delete(start, start + 1, aura_core::AuthorId::Human);
                        app.tab_mut().buffer.insert(
                            start,
                            &open_new.to_string(),
                            aura_core::AuthorId::Human,
                        );
                        app.tab_mut().mark_highlights_dirty();
                    }
                    app.surround_pending = None;
                    return;
                }
                crate::app::SurroundState::DeleteWait => {
                    // Delete surrounding delimiters.
                    let (open, close) = delimiter_pair(c);
                    let char_idx = app.tab().buffer.cursor_to_char_idx(&app.tab().cursor);
                    if let Some((start, end)) = app
                        .tab()
                        .buffer
                        .find_around_delimited(char_idx, open, close)
                    {
                        let close_idx = end.saturating_sub(1);
                        app.tab_mut().buffer.delete(
                            close_idx,
                            close_idx + 1,
                            aura_core::AuthorId::Human,
                        );
                        app.tab_mut()
                            .buffer
                            .delete(start, start + 1, aura_core::AuthorId::Human);
                        app.tab_mut().mark_highlights_dirty();
                    }
                    app.surround_pending = None;
                    return;
                }
                crate::app::SurroundState::AddWaitDelimiter(start, end) => {
                    // Wrap range with delimiters.
                    let start = *start;
                    let end = *end;
                    let (open, close) = delimiter_pair(c);
                    app.tab_mut().buffer.insert(
                        end,
                        &close.to_string(),
                        aura_core::AuthorId::Human,
                    );
                    app.tab_mut().buffer.insert(
                        start,
                        &open.to_string(),
                        aura_core::AuthorId::Human,
                    );
                    app.tab_mut().mark_highlights_dirty();
                    app.surround_pending = None;
                    return;
                }
                crate::app::SurroundState::AddWaitMotion => {
                    // ys + motion: resolve the motion range, then wait for delimiter.
                    // Handle 'iw' (inner word) as the most common case.
                    if c == 'i' || c == 'a' {
                        // Will need the next char for text object... simplified: use inner word.
                        let char_idx = app.tab().buffer.cursor_to_char_idx(&app.tab().cursor);
                        let (start, end) = if c == 'i' {
                            app.tab().buffer.find_inner_word(char_idx)
                        } else {
                            app.tab().buffer.find_around_word(char_idx)
                        };
                        app.surround_pending =
                            Some(crate::app::SurroundState::AddWaitDelimiter(start, end));
                    } else if c == 'w' {
                        let char_idx = app.tab().buffer.cursor_to_char_idx(&app.tab().cursor);
                        let (start, end) = app.tab().buffer.find_inner_word(char_idx);
                        app.surround_pending =
                            Some(crate::app::SurroundState::AddWaitDelimiter(start, end));
                    } else {
                        app.surround_pending = None;
                    }
                    return;
                }
            }
        } else {
            app.surround_pending = None;
        }
        return;
    }

    // Handle z-prefix (fold commands).
    if app.z_pending {
        app.z_pending = false;
        match code {
            KeyCode::Char('a') => {
                app.toggle_fold();
                return;
            }
            KeyCode::Char('c') => {
                app.close_fold();
                return;
            }
            KeyCode::Char('o') => {
                app.open_fold();
                return;
            }
            KeyCode::Char('M') => {
                app.close_all_folds();
                return;
            }
            KeyCode::Char('R') => {
                app.open_all_folds();
                return;
            }
            _ => {}
        }
    }

    if app.g_pending {
        app.g_pending = false;
        match code {
            KeyCode::Char('g') => {
                app.tab_mut().cursor.row = 0;
                app.tab_mut().cursor.col = 0;
                return;
            }
            KeyCode::Char('d') => {
                app.lsp_goto_definition();
                return;
            }
            KeyCode::Char('p') => {
                app.lsp_peek_definition();
                return;
            }
            KeyCode::Char('r') => {
                app.lsp_references();
                return;
            }
            KeyCode::Char('n') => {
                app.lsp_rename_start();
                return;
            }
            KeyCode::Char('e') => {
                // ge — backward to end of previous word.
                let tab = app.tab_mut();
                let mut pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
                if pos > 0 {
                    pos = pos.saturating_sub(1);
                    pos = tab.buffer.word_end_backward(pos);
                }
                tab.cursor = tab.buffer.char_idx_to_cursor(pos);
                app.clamp_cursor();
                return;
            }
            KeyCode::Char('t') => {
                app.tabs.next();
                return;
            }
            KeyCode::Char('T') => {
                app.tabs.prev();
                return;
            }
            _ => {
                // Unknown g-sequence, ignore.
                return;
            }
        }
    }

    // --- Macro record pending (q + {a-z}) ---
    if app.macro_record_pending {
        app.macro_record_pending = false;
        if let KeyCode::Char(c @ 'a'..='z') = code {
            app.start_macro_recording(c);
        } else {
            app.set_status("Cancelled — use a-z for macro register");
        }
        return;
    }

    // --- Macro play pending (@ + {a-z}) ---
    if app.macro_play_pending {
        app.macro_play_pending = false;
        if let KeyCode::Char(c @ 'a'..='z') = code {
            app.play_macro(c);
        } else {
            app.set_status("Cancelled — use a-z for macro register");
        }
        return;
    }

    // --- Replace char pending (r{char}) ---
    if app.replace_char_pending {
        app.replace_char_pending = false;
        if let KeyCode::Char(ch) = code {
            let tab = app.tab_mut();
            let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            if pos < tab.buffer.len_chars() {
                tab.buffer.delete(pos, pos + 1, AuthorId::human());
                tab.buffer.insert(pos, &ch.to_string(), AuthorId::human());
                app.mark_highlights_dirty();
            }
        }
        return;
    }

    // --- Character search pending (f/F/t/T + {char}) ---
    if let Some(mode) = app.find_char_pending.take() {
        if let KeyCode::Char(ch) = code {
            app.last_find_char = Some((mode, ch));
            execute_find_char(app, mode, ch);
        }
        return;
    }

    // --- Count prefix accumulation ---
    if let KeyCode::Char(c @ '1'..='9') = code {
        if app.pending_operator.is_none() || app.count_prefix.is_some() {
            let digit = (c as u32 - '0' as u32) as usize;
            let current = app.count_prefix.unwrap_or(0);
            app.count_prefix = Some(current * 10 + digit);
            return;
        }
    }
    if let KeyCode::Char('0') = code {
        if app.count_prefix.is_some() {
            let current = app.count_prefix.unwrap_or(0);
            app.count_prefix = Some(current * 10);
            return;
        }
    }

    // --- Text object pending (i/a + delimiter) ---
    if let Some(is_inner) = app.text_object_pending.take() {
        if let KeyCode::Char(delim) = code {
            let op = app.pending_operator.take();
            let tab = app.tab_mut();
            let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            let range = match delim {
                '"' | '\'' | '`' => tab.buffer.find_inner_delimited(pos, delim, delim),
                '(' | ')' | 'b' => {
                    if is_inner {
                        tab.buffer.find_inner_delimited(pos, '(', ')')
                    } else {
                        tab.buffer.find_around_delimited(pos, '(', ')')
                    }
                }
                '{' | '}' | 'B' => {
                    if is_inner {
                        tab.buffer.find_inner_delimited(pos, '{', '}')
                    } else {
                        tab.buffer.find_around_delimited(pos, '{', '}')
                    }
                }
                '[' | ']' => {
                    if is_inner {
                        tab.buffer.find_inner_delimited(pos, '[', ']')
                    } else {
                        tab.buffer.find_around_delimited(pos, '[', ']')
                    }
                }
                '<' | '>' => {
                    if is_inner {
                        tab.buffer.find_inner_delimited(pos, '<', '>')
                    } else {
                        tab.buffer.find_around_delimited(pos, '<', '>')
                    }
                }
                'w' => {
                    if is_inner {
                        Some(tab.buffer.find_inner_word(pos))
                    } else {
                        Some(tab.buffer.find_around_word(pos))
                    }
                }
                _ => None,
            };
            // For quote delimiters, apply inner/around after finding.
            let range = if matches!(delim, '"' | '\'' | '`') && !is_inner {
                range.map(|(s, e)| {
                    (
                        s.saturating_sub(1),
                        (e + 1).min(app.tab().buffer.len_chars()),
                    )
                })
            } else {
                range
            };
            if let (Some((start, end)), Some(op)) = (range, op) {
                apply_operator(app, op, start, end);
            }
        } else {
            // Not a valid delimiter — cancel.
            app.pending_operator = None;
        }
        return;
    }

    // --- Operator-pending: d, c, y, >, < ---
    if app.pending_operator.is_some() {
        let op = app.pending_operator.take().unwrap();
        let count = app.count_prefix.take().unwrap_or(1);

        // Surround intercept: cs → change surrounding, ds → delete surrounding, ys → add surrounding.
        if code == KeyCode::Char('s') {
            match op {
                Operator::Change => {
                    app.surround_pending = Some(crate::app::SurroundState::ChangeWaitOld);
                    return;
                }
                Operator::Delete => {
                    app.surround_pending = Some(crate::app::SurroundState::DeleteWait);
                    return;
                }
                Operator::Yank => {
                    app.surround_pending = Some(crate::app::SurroundState::AddWaitMotion);
                    return;
                }
                _ => {} // Fall through for indent/dedent.
            }
        }

        // Double-press = line operation (dd, cc, yy, >>, <<).
        let is_line_op = matches!(
            (&op, code),
            (Operator::Delete, KeyCode::Char('d'))
                | (Operator::Change, KeyCode::Char('c'))
                | (Operator::Yank, KeyCode::Char('y'))
                | (Operator::Indent, KeyCode::Char('>'))
                | (Operator::Dedent, KeyCode::Char('<'))
        );

        if is_line_op {
            let tab = app.tab_mut();
            let start_line = tab.cursor.row;
            let end_line = (start_line + count).min(tab.buffer.line_count());
            let start_idx = tab
                .buffer
                .cursor_to_char_idx(&aura_core::Cursor::new(start_line, 0));
            let end_idx = if end_line >= tab.buffer.line_count() {
                tab.buffer.len_chars()
            } else {
                tab.buffer
                    .cursor_to_char_idx(&aura_core::Cursor::new(end_line, 0))
            };

            match op {
                Operator::Delete => {
                    let text = tab.buffer.rope().slice(start_idx..end_idx).to_string();
                    app.register = Some(text);
                    app.tab_mut()
                        .buffer
                        .delete(start_idx, end_idx, AuthorId::human());
                    app.clamp_cursor();
                    app.mark_highlights_dirty();
                }
                Operator::Change => {
                    let text = tab.buffer.rope().slice(start_idx..end_idx).to_string();
                    app.register = Some(text);
                    app.tab_mut()
                        .buffer
                        .delete(start_idx, end_idx, AuthorId::human());
                    app.clamp_cursor();
                    app.mark_highlights_dirty();
                    app.mode = Mode::Insert;
                }
                Operator::Yank => {
                    let text = tab.buffer.rope().slice(start_idx..end_idx).to_string();
                    app.register = Some(text);
                    app.set_status(format!(
                        "{count} line{} yanked",
                        if count == 1 { "" } else { "s" }
                    ));
                }
                Operator::Indent => {
                    let indent = app.tab().indent_style.unit();
                    app.tab_mut().buffer.indent_lines(
                        start_line,
                        end_line.saturating_sub(1),
                        &indent,
                        AuthorId::human(),
                    );
                    app.mark_highlights_dirty();
                }
                Operator::Dedent => {
                    let tw = app.config.editor.tab_width;
                    app.tab_mut().buffer.dedent_lines(
                        start_line,
                        end_line.saturating_sub(1),
                        tw,
                        AuthorId::human(),
                    );
                    app.mark_highlights_dirty();
                }
            }
            return;
        }

        // Text objects: i/a + delimiter.
        if matches!(code, KeyCode::Char('i')) {
            app.pending_operator = Some(op);
            app.text_object_pending = Some(true); // inner
            return;
        }
        if matches!(code, KeyCode::Char('a')) {
            app.pending_operator = Some(op);
            app.text_object_pending = Some(false); // around
            return;
        }

        // Operator + motion: compute range and apply.
        if let Some((start, end)) = resolve_operator_motion(app, code, count) {
            apply_operator(app, op, start, end);
        }
        return;
    }

    // --- Operator keys: set pending_operator ---
    match code {
        KeyCode::Char('d') if !modifiers.contains(KeyModifiers::CONTROL) => {
            app.pending_operator = Some(Operator::Delete);
            return;
        }
        KeyCode::Char('c') if !modifiers.contains(KeyModifiers::CONTROL) => {
            app.pending_operator = Some(Operator::Change);
            return;
        }
        KeyCode::Char('y') if !modifiers.contains(KeyModifiers::CONTROL) => {
            app.pending_operator = Some(Operator::Yank);
            return;
        }
        KeyCode::Char('>') => {
            app.pending_operator = Some(Operator::Indent);
            return;
        }
        KeyCode::Char('<') => {
            app.pending_operator = Some(Operator::Dedent);
            return;
        }
        _ => {}
    }

    // Consume count for standalone motions.
    let count = app.count_prefix.take().unwrap_or(1);

    match code {
        // Leader key (configurable, default: Space)
        _ if app.config.keybindings.is_leader_key(code) => {
            app.leader_pending = true;
        }

        // Mode transitions
        KeyCode::Char('i') => {
            app.mode = Mode::Insert;
            app.status_message = None;
        }
        KeyCode::Char('a') => {
            app.tab_mut().cursor.col += 1;
            app.clamp_cursor();
            app.mode = Mode::Insert;
        }
        KeyCode::Char('o') => {
            // Open line below.
            let tab = app.tab_mut();
            let line_end = tab
                .buffer
                .line(tab.cursor.row)
                .map(|l| l.len_chars().saturating_sub(1))
                .unwrap_or(0);
            let pos = tab
                .buffer
                .cursor_to_char_idx(&aura_core::Cursor::new(tab.cursor.row, line_end));
            tab.buffer.insert(pos, "\n", AuthorId::human());
            tab.cursor.row += 1;
            tab.cursor.col = 0;
            app.mode = Mode::Insert;
            app.mark_highlights_dirty();
        }
        KeyCode::Char(':') => {
            app.mode = Mode::Command;
            app.command_input.clear();
        }
        // Cmd+V — paste from system clipboard (macOS).
        KeyCode::Char('v') if modifiers.contains(KeyModifiers::SUPER) => {
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                if let Ok(text) = clipboard.get_text() {
                    let tab = app.tab_mut();
                    let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
                    tab.buffer.insert(pos, &text, AuthorId::human());
                    let new_pos = pos + text.chars().count();
                    tab.cursor = tab.buffer.char_idx_to_cursor(new_pos);
                    app.mark_highlights_dirty();
                    app.set_status("Pasted from clipboard");
                }
            }
        }
        // Cmd+S — save (macOS).
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::SUPER) => {
            match app.tab_mut().buffer.save() {
                Ok(_) => app.set_status("Saved"),
                Err(e) => app.set_status(format!("Save failed: {e}")),
            }
        }
        KeyCode::Char('v') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.mode = Mode::VisualBlock;
            let cursor = app.tab().cursor;
            app.tab_mut().visual_anchor = Some(cursor);
        }
        KeyCode::Char('v') => {
            app.mode = Mode::Visual;
            let cursor = app.tab().cursor;
            app.tab_mut().visual_anchor = Some(cursor);
        }
        KeyCode::Char('V') => {
            app.mode = Mode::VisualLine;
            let cursor = app.tab().cursor;
            app.tab_mut().visual_anchor = Some(cursor);
        }

        // Ghost suggestion controls
        KeyCode::Tab if app.current_ghost_suggestion().is_some() => {
            app.accept_ghost_suggestion();
        }
        KeyCode::Esc if app.current_ghost_suggestion().is_some() => {
            app.dismiss_ghost_suggestions();
        }

        // Next-edit prediction navigation.
        KeyCode::Char(']')
            if modifiers.contains(KeyModifiers::CONTROL) && !app.edit_predictions().is_empty() =>
        {
            app.jump_to_prediction(true);
            app.notify_cursor_moved();
        }
        KeyCode::Char('[')
            if modifiers.contains(KeyModifiers::CONTROL) && !app.edit_predictions().is_empty() =>
        {
            app.jump_to_prediction(false);
            app.notify_cursor_moved();
        }

        // Navigation with count support.
        KeyCode::Char('h') | KeyCode::Left => {
            let tab = app.tab_mut();
            tab.cursor.col = tab.cursor.col.saturating_sub(count);
            tab.hover_info = None;
            app.notify_cursor_moved();
        }
        KeyCode::Char('l') | KeyCode::Right => {
            app.tab_mut().cursor.col += count;
            app.clamp_cursor();
            app.tab_mut().hover_info = None;
            app.notify_cursor_moved();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let tab = app.tab_mut();
            tab.cursor.row = tab.cursor.row.saturating_sub(count);
            tab.hover_info = None;
            app.clamp_cursor();
            app.notify_cursor_moved();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.tab_mut().cursor.row += count;
            app.clamp_cursor();
            app.tab_mut().hover_info = None;
            app.notify_cursor_moved();
        }
        KeyCode::Char('0') => {
            app.tab_mut().cursor.col = 0;
        }
        KeyCode::Char('$') => {
            let tab = app.tab_mut();
            if let Some(line) = tab.buffer.line(tab.cursor.row) {
                tab.cursor.col = line.len_chars().saturating_sub(1);
            }
        }
        KeyCode::Char('g') => {
            // gg → go to top, gd → go to definition.
            app.g_pending = true;
        }
        KeyCode::Char('z') => {
            // za → toggle fold, zc → close, zo → open, zM → close all, zR → open all.
            app.z_pending = true;
        }
        KeyCode::Char('m') => {
            // m{a-z} — set mark.
            app.mark_pending = true;
        }
        KeyCode::Char('\'') => {
            // '{a-z} — jump to mark (line).
            app.jump_mark_pending = true;
        }
        KeyCode::Char('G') => {
            let tab = app.tab_mut();
            tab.cursor.row = tab.buffer.line_count().saturating_sub(1);
            tab.cursor.col = 0;
        }

        // Word movement with count.
        KeyCode::Char('w') => {
            let tab = app.tab_mut();
            let mut pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            for _ in 0..count {
                pos = tab.buffer.next_word_start(pos);
            }
            tab.cursor = tab.buffer.char_idx_to_cursor(pos);
            app.clamp_cursor();
        }
        KeyCode::Char('b') => {
            let tab = app.tab_mut();
            let mut pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            for _ in 0..count {
                pos = tab.buffer.prev_word_start(pos);
            }
            tab.cursor = tab.buffer.char_idx_to_cursor(pos);
        }
        KeyCode::Char('e') => {
            let tab = app.tab_mut();
            let mut pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            for _ in 0..count {
                pos = tab.buffer.word_end(pos);
            }
            tab.cursor = tab.buffer.char_idx_to_cursor(pos);
            app.clamp_cursor();
        }

        // Editing
        KeyCode::Char('x') => {
            // Delete character under cursor.
            let tab = app.tab_mut();
            let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            if pos < tab.buffer.len_chars() {
                tab.buffer.delete(pos, pos + 1, AuthorId::human());
                app.clamp_cursor();
                app.mark_highlights_dirty();
            }
        }
        KeyCode::Char('u') => {
            if let Some(author) = app.tab_mut().buffer.undo() {
                app.set_status(format!("Undid edit by {}", author));
                app.clamp_cursor();
                app.mark_highlights_dirty();
            } else {
                app.set_status("Nothing to undo");
            }
        }
        // f/F/t/T — character search on line.
        KeyCode::Char('f') => {
            app.find_char_pending = Some(FindCharMode::Forward);
        }
        KeyCode::Char('F') => {
            app.find_char_pending = Some(FindCharMode::Backward);
        }
        KeyCode::Char('t') => {
            app.find_char_pending = Some(FindCharMode::ForwardTill);
        }
        KeyCode::Char('T') => {
            app.find_char_pending = Some(FindCharMode::BackwardTill);
        }
        // ; — repeat last f/F/t/T search.
        KeyCode::Char(';') => {
            if let Some((mode, ch)) = app.last_find_char {
                execute_find_char(app, mode, ch);
            }
        }
        // , — reverse last f/F/t/T search.
        KeyCode::Char(',') => {
            if let Some((mode, ch)) = app.last_find_char {
                let reversed = match mode {
                    FindCharMode::Forward => FindCharMode::Backward,
                    FindCharMode::Backward => FindCharMode::Forward,
                    FindCharMode::ForwardTill => FindCharMode::BackwardTill,
                    FindCharMode::BackwardTill => FindCharMode::ForwardTill,
                };
                execute_find_char(app, reversed, ch);
            }
        }
        // r — replace character under cursor.
        KeyCode::Char('r') => {
            app.replace_char_pending = true;
        }
        // J — join current line with next.
        KeyCode::Char('J') => {
            let row = app.tab().cursor.row;
            app.tab_mut().buffer.join_lines(row, AuthorId::human());
            app.mark_highlights_dirty();
        }
        // ~ — toggle case of character under cursor.
        KeyCode::Char('~') => {
            let tab = app.tab_mut();
            let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            if let Some(ch) = tab.buffer.get_char(pos) {
                let toggled: String = if ch.is_uppercase() {
                    ch.to_lowercase().to_string()
                } else {
                    ch.to_uppercase().to_string()
                };
                tab.buffer.delete(pos, pos + 1, AuthorId::human());
                tab.buffer.insert(pos, &toggled, AuthorId::human());
                tab.cursor.col += 1;
                app.clamp_cursor();
                app.mark_highlights_dirty();
            }
        }
        // s — substitute: delete char and enter Insert.
        KeyCode::Char('s') if !app.source_control_focused => {
            let tab = app.tab_mut();
            let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            if pos < tab.buffer.len_chars() {
                tab.buffer.delete(pos, pos + 1, AuthorId::human());
                app.mark_highlights_dirty();
            }
            app.mode = Mode::Insert;
        }
        // S — substitute line: delete line content and enter Insert.
        KeyCode::Char('S') => {
            let tab = app.tab_mut();
            let row = tab.cursor.row;
            let line_start = tab
                .buffer
                .cursor_to_char_idx(&aura_core::Cursor::new(row, 0));
            let line_end = if row + 1 < tab.buffer.line_count() {
                tab.buffer
                    .cursor_to_char_idx(&aura_core::Cursor::new(row + 1, 0))
                    - 1
            } else {
                tab.buffer.len_chars()
            };
            if line_end > line_start {
                let text = tab.buffer.rope().slice(line_start..line_end).to_string();
                app.register = Some(text);
                app.tab_mut()
                    .buffer
                    .delete(line_start, line_end, AuthorId::human());
                app.tab_mut().cursor.col = 0;
                app.mark_highlights_dirty();
            }
            app.mode = Mode::Insert;
        }
        // C — change to end of line.
        KeyCode::Char('C') => {
            let tab = app.tab_mut();
            let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            let row = tab.cursor.row;
            let line_end = if row + 1 < tab.buffer.line_count() {
                tab.buffer
                    .cursor_to_char_idx(&aura_core::Cursor::new(row + 1, 0))
                    - 1
            } else {
                tab.buffer.len_chars()
            };
            if line_end > pos {
                let text = tab.buffer.rope().slice(pos..line_end).to_string();
                app.register = Some(text);
                app.tab_mut()
                    .buffer
                    .delete(pos, line_end, AuthorId::human());
                app.mark_highlights_dirty();
            }
            app.mode = Mode::Insert;
        }
        // D — delete to end of line.
        KeyCode::Char('D') => {
            let tab = app.tab_mut();
            let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            let row = tab.cursor.row;
            let line_end = if row + 1 < tab.buffer.line_count() {
                tab.buffer
                    .cursor_to_char_idx(&aura_core::Cursor::new(row + 1, 0))
                    - 1
            } else {
                tab.buffer.len_chars()
            };
            if line_end > pos {
                let text = tab.buffer.rope().slice(pos..line_end).to_string();
                app.register = Some(text);
                app.tab_mut()
                    .buffer
                    .delete(pos, line_end, AuthorId::human());
                app.clamp_cursor();
                app.mark_highlights_dirty();
            }
        }
        // Y — yank line (alias for yy).
        KeyCode::Char('Y') => {
            let row = app.tab().cursor.row;
            if let Some(text) = app.tab().buffer.line_text(row) {
                app.register = Some(text);
                app.set_status("Yanked line");
            }
        }
        // * — search word under cursor forward.
        KeyCode::Char('*') => {
            let tab = app.tab();
            let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            let (start, end) = tab.buffer.find_inner_word(pos);
            if end > start {
                let word = tab.buffer.rope().slice(start..end).to_string();
                app.search_query = Some(word.clone());
                app.search_input = word;
                app.search_forward = true;
                app.execute_search();
            }
        }
        // # — search word under cursor backward.
        KeyCode::Char('#') => {
            let tab = app.tab();
            let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            let (start, end) = tab.buffer.find_inner_word(pos);
            if end > start {
                let word = tab.buffer.rope().slice(start..end).to_string();
                app.search_query = Some(word.clone());
                app.search_input = word;
                app.search_forward = false;
                app.execute_search();
            }
        }
        // p — paste register after current line
        KeyCode::Char('p') => {
            if let Some(text) = app.register.clone() {
                if text.ends_with('\n') {
                    // Line-wise paste: insert on the next line.
                    let tab = app.tab_mut();
                    let line_count = tab.buffer.line_count();
                    let insert_line = (tab.cursor.row + 1).min(line_count);
                    let pos = tab
                        .buffer
                        .cursor_to_char_idx(&aura_core::Cursor::new(insert_line, 0));
                    tab.buffer.insert(pos, &text, AuthorId::human());
                    tab.cursor.row += 1;
                    tab.cursor.col = 0;
                } else {
                    // Character-wise paste: insert after cursor.
                    let tab = app.tab_mut();
                    let pos = tab.buffer.cursor_to_char_idx(&tab.cursor) + 1;
                    tab.buffer.insert(pos, &text, AuthorId::human());
                }
                app.clamp_cursor();
                app.mark_highlights_dirty();
            } else {
                app.set_status("Nothing to paste");
            }
        }

        // K — hover info
        KeyCode::Char('K') => {
            app.lsp_hover();
        }

        // Alt+] — next ghost suggestion, Alt+[ — previous ghost suggestion
        KeyCode::Char(']') if modifiers.contains(KeyModifiers::ALT) => {
            app.next_ghost_suggestion();
        }
        KeyCode::Char('[') if modifiers.contains(KeyModifiers::ALT) => {
            app.prev_ghost_suggestion();
        }

        // ]d — next diagnostic, [d — previous diagnostic
        KeyCode::Char(']') => {
            app.next_diagnostic();
        }
        KeyCode::Char('[') => {
            app.prev_diagnostic();
        }

        // Save shortcut
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            match app.tab_mut().buffer.save() {
                Ok(_) => app.set_status("Saved"),
                Err(e) => app.set_status(format!("Error saving: {}", e)),
            }
        }

        // Ctrl+D — add cursor at next occurrence of word under cursor.
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
            let tab = app.tab();
            let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            let (wstart, wend) = tab.buffer.find_inner_word(pos);
            if wend > wstart {
                let word = tab.buffer.rope().slice(wstart..wend).to_string();
                let text = tab.buffer.rope().to_string();
                // Search forward from the last cursor (primary or last secondary).
                let search_from = {
                    let last = app
                        .tab()
                        .secondary_cursors
                        .last()
                        .map(|c| app.tab().buffer.cursor_to_char_idx(c))
                        .unwrap_or(wend);
                    last
                };
                if let Some(found) = text[search_from..].find(&word) {
                    let match_pos = search_from + found;
                    let new_cursor = app.tab().buffer.char_idx_to_cursor(match_pos);
                    app.tab_mut().secondary_cursors.push(new_cursor);
                    let count = app.tab().secondary_cursors.len() + 1;
                    app.set_status(format!("{count} cursors"));
                } else {
                    // Wrap around — search from beginning.
                    if let Some(found) = text.find(&word) {
                        let match_pos = found;
                        let existing = app.tab().buffer.cursor_to_char_idx(&app.tab().cursor);
                        if match_pos != existing {
                            let new_cursor = app.tab().buffer.char_idx_to_cursor(match_pos);
                            // Check it's not already a cursor.
                            if !app.tab().secondary_cursors.contains(&new_cursor) {
                                app.tab_mut().secondary_cursors.push(new_cursor);
                            }
                        }
                        let count = app.tab().secondary_cursors.len() + 1;
                        app.set_status(format!("{count} cursors"));
                    }
                }
            }
        }

        // . — dot repeat last edit.
        KeyCode::Char('.') => {
            app.dot_repeat();
        }
        // q — toggle macro recording.
        KeyCode::Char('q') if !app.source_control_focused => {
            if app.macro_recording.is_some() {
                app.stop_macro_recording();
            } else {
                // Wait for next key to determine register.
                app.set_status("Record macro: press a-z for register");
                // We'll handle this via a pending state.
                app.find_char_pending = None; // Reset any pending.
                app.macro_record_pending = true;
            }
        }
        // @ — play macro.
        KeyCode::Char('@') => {
            app.macro_play_pending = true;
        }

        // % — jump to matching bracket.
        KeyCode::Char('%') => {
            if let Some((row, col)) = app.matching_bracket {
                let tab = app.tab_mut();
                tab.cursor.row = row;
                tab.cursor.col = col;
                app.clamp_cursor();
            }
        }

        // / — start forward search.
        KeyCode::Char('/') => {
            app.search_active = true;
            app.search_input.clear();
            app.search_forward = true;
        }

        // n — next search match (only when not leader-pending and not Ctrl).
        KeyCode::Char('n') if !modifiers.contains(KeyModifiers::CONTROL) && !app.leader_pending => {
            if app.search_query.is_some() {
                app.search_next();
                let total = app.search_matches.len();
                let cur = app.search_current + 1;
                app.set_status(format!("{cur}/{total}"));
            }
        }

        // N — previous search match.
        KeyCode::Char('N') if !app.leader_pending => {
            if app.search_query.is_some() {
                app.search_prev();
                let total = app.search_matches.len();
                let cur = app.search_current + 1;
                app.set_status(format!("{cur}/{total}"));
            }
        }

        // ? — open help overlay (vim convention).
        KeyCode::Char('?') => {
            app.help.open();
        }

        // Ctrl+n — toggle file tree sidebar and focus.
        // If the sidebar is open but the editor has focus, move focus to the
        // sidebar first instead of closing it.
        KeyCode::Char('n') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.conversation_history_focused = false;
            if app.file_tree.visible {
                let sidebar_focused = app.file_tree_focused || app.source_control_focused;
                if sidebar_focused {
                    // Already focused — close the sidebar.
                    app.file_tree_focused = false;
                    app.source_control_focused = false;
                    app.file_tree.toggle();
                    let state = if app.file_tree.visible { "on" } else { "off" };
                    app.set_status(format!("File tree: {state}"));
                } else {
                    // Sidebar open but editor focused — move focus to sidebar.
                    if app.sidebar_view == SidebarView::Files {
                        app.file_tree_focused = true;
                    } else {
                        app.source_control_focused = true;
                    }
                }
            } else {
                app.file_tree.toggle();
                if app.sidebar_view == SidebarView::Files {
                    app.file_tree_focused = true;
                } else {
                    app.source_control_focused = true;
                    app.refresh_source_control();
                }
                let state = if app.file_tree.visible { "on" } else { "off" };
                app.set_status(format!("File tree: {state}"));
            }
        }

        _ => {}
    }
}

/// Handle keys in Insert mode.
pub fn handle_insert(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    // If a tool call is pending approval, intercept Y/N regardless of mode.
    if app.chat_panel.pending_approval.is_some() {
        match code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.approve_pending_tool();
                return;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.deny_pending_tool();
                return;
            }
            _ => {}
        }
    }

    match code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            // Clear multi-cursors.
            app.tab_mut().secondary_cursors.clear();
            // In normal mode, cursor sits on the last char, not after it.
            let tab = app.tab_mut();
            tab.cursor.col = tab.cursor.col.saturating_sub(1);
            app.clamp_cursor();
        }
        KeyCode::Tab => {
            // 1. If active snippet with remaining placeholders → jump to next.
            let has_active = app.tab().snippet_engine.active.is_some();
            if has_active {
                let done = {
                    let engine = &mut app.tab_mut().snippet_engine;
                    if let Some(ref mut active) = engine.active {
                        if !active.next_placeholder() {
                            true // done
                        } else {
                            false
                        }
                    } else {
                        true
                    }
                };
                if done {
                    app.tab_mut().snippet_engine.active = None;
                } else if let Some(ref active) = app.tab().snippet_engine.active {
                    if let Some(ph) = active.current_placeholder() {
                        let pos = active.insert_offset + ph.offset;
                        let cursor = app.tab().buffer.char_idx_to_cursor(pos);
                        app.tab_mut().cursor = cursor;
                    }
                }
                return;
            }

            // 2. Check if word before cursor matches a snippet trigger.
            let lang_name = app.tab().language.map(|l| {
                use crate::highlight::Language;
                match l {
                    Language::Rust => "rust",
                    Language::Python => "python",
                    Language::TypeScript | Language::Tsx | Language::JavaScript => "typescript",
                    Language::Go => "go",
                    Language::Elixir | Language::HEEx => "elixir",
                    Language::Php => "php",
                    Language::Lua => "lua",
                    Language::Dart => "dart",
                    Language::Swift => "swift",
                    Language::Kotlin => "kotlin",
                    Language::Zig => "zig",
                    Language::Scala => "scala",
                    Language::Haskell => "haskell",
                    Language::Sql => "sql",
                    _ => "",
                }
                .to_string()
            });

            let trigger_word = {
                let tab = app.tab();
                let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
                let (wstart, _wend) = tab.buffer.find_inner_word(pos.saturating_sub(1));
                let word_pos = wstart;
                let word: String = tab.buffer.rope().slice(word_pos..pos).to_string();
                (word, word_pos, pos)
            };
            let (word, word_start, word_end) = trigger_word;

            if let Some(snippet) = app
                .tab()
                .snippet_engine
                .find(&word, lang_name.as_deref())
                .cloned()
            {
                // Get current line indent.
                let indent = {
                    let tab = app.tab();
                    let line_text = tab.buffer.line_text(tab.cursor.row).unwrap_or_default();
                    line_text
                        .chars()
                        .take_while(|c| *c == ' ' || *c == '\t')
                        .collect::<String>()
                };

                // Delete trigger word.
                app.tab_mut()
                    .buffer
                    .delete(word_start, word_end, aura_core::AuthorId::human());

                // Expand snippet.
                let (expanded, placeholders) =
                    crate::snippets::SnippetEngine::expand(&snippet.body, &indent);

                // Insert expanded text.
                app.tab_mut()
                    .buffer
                    .insert(word_start, &expanded, aura_core::AuthorId::human());

                // Set up active snippet for placeholder navigation.
                if !placeholders.is_empty() {
                    let active = crate::snippets::ActiveSnippet {
                        placeholders,
                        current: 0,
                        insert_offset: word_start,
                    };
                    // Position cursor at first placeholder.
                    if let Some(ph) = active.current_placeholder() {
                        let pos = word_start + ph.offset;
                        app.tab_mut().cursor = app.tab().buffer.char_idx_to_cursor(pos);
                    }
                    app.tab_mut().snippet_engine.active = Some(active);
                } else {
                    // No placeholders — position at end of inserted text.
                    let end = word_start + expanded.len();
                    app.tab_mut().cursor = app.tab().buffer.char_idx_to_cursor(end);
                }
                app.mark_highlights_dirty();
                return;
            }

            // 3. No snippet match — insert tab/spaces.
            let indent = app.tab().indent_style.unit();
            let tab = app.tab_mut();
            let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            tab.buffer
                .insert(pos, &indent, aura_core::AuthorId::human());
            tab.cursor = tab.buffer.char_idx_to_cursor(pos + indent.len());
            app.mark_highlights_dirty();
        }
        KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
            // Multi-cursor: insert at all cursor positions (right-to-left).
            let has_secondary = !app.tab().secondary_cursors.is_empty();
            if has_secondary {
                let mut all_positions: Vec<usize> = {
                    let tab = app.tab();
                    let mut pos = vec![tab.buffer.cursor_to_char_idx(&tab.cursor)];
                    for sc in &tab.secondary_cursors {
                        pos.push(tab.buffer.cursor_to_char_idx(sc));
                    }
                    pos.sort_unstable();
                    pos.dedup();
                    pos
                };
                // Insert right-to-left to avoid index shifting.
                all_positions.sort_unstable_by(|a, b| b.cmp(a));
                let ch_str = c.to_string();
                for &pos in &all_positions {
                    app.tab_mut().buffer.insert(pos, &ch_str, AuthorId::human());
                }
                // Update all cursor positions (each shifted by insertions before it).
                all_positions.sort_unstable();
                let tab = app.tab_mut();
                for (i, &pos) in all_positions.iter().enumerate() {
                    let new_pos = pos + i + 1; // +1 for the char, +i for prior insertions
                    let new_cursor = tab.buffer.char_idx_to_cursor(new_pos);
                    if i == 0 {
                        tab.cursor = new_cursor;
                    } else if let Some(sc) = tab.secondary_cursors.get_mut(i - 1) {
                        *sc = new_cursor;
                    }
                }
            } else {
                let tab = app.tab_mut();
                let new_pos = tab.buffer.insert_char(&tab.cursor, c, AuthorId::human());
                tab.cursor = tab.buffer.char_idx_to_cursor(new_pos);
            }
            app.mark_highlights_dirty();

            // Auto-close brackets: insert matching closing character.
            if matches!(c, '(' | '{' | '[') {
                let closing = match c {
                    '(' => ')',
                    '{' => '}',
                    '[' => ']',
                    _ => unreachable!(),
                };
                let tab = app.tab_mut();
                let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
                tab.buffer
                    .insert(pos, &closing.to_string(), AuthorId::human());
                // Cursor stays between the brackets (already at correct position).
            }

            // Auto-close quotes (only if not escaping or inside a string).
            if matches!(c, '"' | '\'') {
                let tab = app.tab_mut();
                let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
                // Check if the next char is already the same quote (skip instead).
                let next_char = tab.buffer.get_char(pos);
                if next_char != Some(c) {
                    tab.buffer.insert(pos, &c.to_string(), AuthorId::human());
                }
            }

            // Auto-dedent: if we just typed a closing bracket on an
            // otherwise-blank line, match the opener's indentation.
            if matches!(c, '}' | ')' | ']') {
                let tab = app.tab_mut();
                let line_text = tab.buffer.line_text(tab.cursor.row).unwrap_or_default();
                let trimmed = line_text.trim_end_matches('\n').trim_end_matches('\r');
                let trimmed_content = trimmed.trim_start();
                // Only auto-dedent if the line has just this bracket.
                if trimmed_content.len() == 1 && trimmed_content.starts_with(c) {
                    // Find the char index of the bracket we just inserted.
                    let bracket_char_idx =
                        tab.buffer.cursor_to_char_idx(&tab.cursor).saturating_sub(1);
                    if let Some(match_idx) = tab.buffer.find_matching_bracket(bracket_char_idx) {
                        let match_cursor = tab.buffer.char_idx_to_cursor(match_idx);
                        let match_line = tab.buffer.line_text(match_cursor.row).unwrap_or_default();
                        let target_indent: String = match_line
                            .chars()
                            .take_while(|ch| *ch == ' ' || *ch == '\t')
                            .collect();
                        let current_indent_len = trimmed.len() - trimmed_content.len();
                        let line_start = tab
                            .buffer
                            .cursor_to_char_idx(&aura_core::Cursor::new(tab.cursor.row, 0));
                        // Remove old indent and insert new one.
                        if current_indent_len > 0 {
                            tab.buffer.delete(
                                line_start,
                                line_start + current_indent_len,
                                AuthorId::human(),
                            );
                        }
                        tab.buffer
                            .insert(line_start, &target_indent, AuthorId::human());
                        // Position cursor after the bracket.
                        tab.cursor.col = target_indent.len();
                        app.mark_highlights_dirty();
                    }
                }
            }
        }
        KeyCode::Enter => {
            let tab = app.tab_mut();
            let current_line = tab.buffer.line_text(tab.cursor.row).unwrap_or_default();
            // Get leading whitespace from the current line.
            let base_indent: String = current_line
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            // Check if text before cursor ends with an opening bracket/colon.
            let text_before_cursor: String = current_line.chars().take(tab.cursor.col).collect();
            let trimmed = text_before_cursor.trim_end();
            let increase_indent = trimmed.ends_with('{')
                || trimmed.ends_with('(')
                || trimmed.ends_with('[')
                || trimmed.ends_with(':');
            let indent_unit = tab.indent_style.unit();
            let new_indent = if increase_indent {
                format!("{}{}", base_indent, indent_unit)
            } else {
                base_indent
            };
            let insert_text = format!("\n{}", new_indent);
            let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            tab.buffer.insert(pos, &insert_text, AuthorId::human());
            let new_pos = pos + insert_text.chars().count();
            tab.cursor = tab.buffer.char_idx_to_cursor(new_pos);
            app.mark_highlights_dirty();
        }
        KeyCode::Backspace => {
            let tab = app.tab_mut();
            if let Some(new_pos) = tab.buffer.backspace(&tab.cursor, AuthorId::human()) {
                tab.cursor = tab.buffer.char_idx_to_cursor(new_pos);
                app.mark_highlights_dirty();
            }
        }
        KeyCode::Delete => {
            // Forward delete: remove the character at (not before) the cursor.
            let tab = app.tab_mut();
            let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            if pos < tab.buffer.len_chars() {
                tab.buffer.delete(pos, pos + 1, AuthorId::human());
                app.mark_highlights_dirty();
            }
        }
        // Cmd+V — paste from system clipboard in Insert mode (macOS).
        KeyCode::Char('v') if modifiers.contains(KeyModifiers::SUPER) => {
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                if let Ok(text) = clipboard.get_text() {
                    let tab = app.tab_mut();
                    let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
                    tab.buffer.insert(pos, &text, AuthorId::human());
                    let new_pos = pos + text.chars().count();
                    tab.cursor = tab.buffer.char_idx_to_cursor(new_pos);
                    app.mark_highlights_dirty();
                }
            }
        }
        KeyCode::Left => {
            let tab = app.tab_mut();
            tab.cursor.col = tab.cursor.col.saturating_sub(1);
        }
        KeyCode::Right => {
            app.tab_mut().cursor.col += 1;
            app.clamp_cursor();
        }
        KeyCode::Up => {
            let tab = app.tab_mut();
            tab.cursor.row = tab.cursor.row.saturating_sub(1);
            app.clamp_cursor();
        }
        KeyCode::Down => {
            app.tab_mut().cursor.row += 1;
            app.clamp_cursor();
        }
        // Ctrl+S / Cmd+S to save even in insert mode.
        KeyCode::Char('s')
            if modifiers.contains(KeyModifiers::CONTROL)
                || modifiers.contains(KeyModifiers::SUPER) =>
        {
            match app.tab_mut().buffer.save() {
                Ok(_) => app.set_status("Saved"),
                Err(e) => app.set_status(format!("Error saving: {}", e)),
            }
        }
        _ => {}
    }
}

/// All available `:` commands: (name, description, shortcut).
const COMMAND_LIST: &[(&str, &str, &str)] = &[
    ("w", "Write (save) current file", ""),
    ("q", "Quit (close tab or exit)", ""),
    ("q!", "Force quit without saving", ""),
    ("qa", "Quit all tabs", ""),
    ("qa!", "Force quit all tabs", ""),
    ("wq", "Save and quit", ""),
    ("wqa", "Save all and quit", ""),
    ("intent", "Enter AI intent mode", ""),
    ("decisions", "Show recent AI decisions", ""),
    ("undo-tree", "Visual undo history", ""),
    ("commit", "Generate AI commit message", ""),
    ("branches", "Open branch picker", "Ctrl+B"),
    ("graph", "Visual git graph", "Ctrl+Shift+G"),
    ("blame", "Toggle git blame", ""),
    ("log", "Show Aura git log", ""),
    ("experiment", "Enter experiment mode", ""),
    ("code-action", "LSP code actions", ""),
    ("plugins", "List loaded plugins", ""),
    ("help", "Open help overlay", ""),
    ("files", "Fuzzy file picker", "Ctrl+P"),
    ("tabnew", "Open new scratch tab", ""),
    ("tabe", "Open file in new tab", ""),
    ("tabc", "Close current tab", ""),
    ("tabn", "Next tab", ""),
    ("tabp", "Previous tab", ""),
    ("term", "Toggle terminal", "Ctrl+T"),
    ("term new", "New terminal tab", "Ctrl+Shift+T"),
    ("term close", "Close terminal tab", ""),
    ("term next", "Next terminal tab", "Ctrl+Shift+]"),
    ("term prev", "Previous terminal tab", "Ctrl+Shift+["),
    ("tree", "Toggle file tree sidebar", "Ctrl+N"),
    ("git", "Open source control panel", "Ctrl+G"),
    ("term-height", "Set terminal height", ""),
    ("noh", "Clear search highlights", ""),
    ("version", "Show AURA version", ""),
    ("update", "Check for updates", ""),
    ("vsplit", "Vertical split", ""),
    ("hsplit", "Horizontal split", ""),
    ("split", "Horizontal split", ""),
    ("only", "Close all splits", ""),
    ("settings", "Open settings", "Ctrl+,"),
    ("compact", "Compact AI conversations", ""),
    ("host", "Start collab hosting", ""),
    ("collab-stop", "Stop collab session", ""),
    ("unfollow", "Stop following peer", ""),
    ("share-term", "Toggle terminal sharing", ""),
    ("view-term", "View shared terminal", ""),
    ("stash", "Git stash", ""),
    ("stash pop", "Git stash pop", ""),
    ("stash drop", "Git stash drop", ""),
    ("pr", "Create pull request", ""),
    ("tasks", "List project tasks", ""),
    ("outline", "Open symbol outline", "Ctrl+O"),
    ("agent", "Start AI agent", ""),
    ("agent stop", "Stop AI agent", ""),
    ("agent pause", "Pause running agent", "Ctrl+P"),
    ("agent resume", "Resume paused agent", ""),
    ("agent plan", "Start agent with planning phase", ""),
    ("agent trust", "Set agent trust level (read|write|full)", ""),
    ("agent timeline", "Toggle agent timeline view", ""),
    ("agent diff", "Show agent changes diff", ""),
    ("fix", "Fix last failed command", ""),
    ("registers", "Show registers", ""),
    ("marks", "Show marks", ""),
    ("search", "Project-wide search", "Ctrl+F"),
    ("grep", "Project-wide search", "Ctrl+F"),
    ("visor", "Claude Code config browser", "Ctrl+I"),
    ("ai-visor", "Claude Code config browser", "Ctrl+I"),
    ("references", "LSP find references", ""),
    ("rename", "LSP rename symbol", ""),
    ("merge", "Open merge conflict view", ""),
    ("debug", "Start debug session", ""),
    ("breakpoint", "Toggle breakpoint", ""),
    ("continue", "Debug continue", ""),
    ("step", "Debug step over", ""),
    ("stepin", "Debug step in", ""),
    ("stepout", "Debug step out", ""),
    ("debug stop", "Stop debugger", ""),
    ("debug panel", "Toggle debug panel", ""),
    ("set relativenumber", "Relative line numbers ON", ""),
    ("set norelativenumber", "Relative line numbers OFF", ""),
    ("set wrap", "Word wrap ON", ""),
    ("set nowrap", "Word wrap OFF", ""),
    ("accept-current", "Accept current in conflict", ""),
    ("accept-incoming", "Accept incoming in conflict", ""),
    ("accept-both", "Accept both in conflict", ""),
    ("seed-history", "Seed conversation history", ""),
];

/// Update command completions based on current input.
fn update_command_completions(app: &mut App) {
    let input = app.command_input.trim().to_lowercase();
    if input.is_empty() {
        app.command_completions.clear();
        app.command_completion_idx = None;
        return;
    }
    app.command_completions = COMMAND_LIST
        .iter()
        .filter(|(cmd, _, _)| cmd.starts_with(&input) && *cmd != input)
        .map(|(cmd, desc, shortcut)| (cmd.to_string(), desc.to_string(), shortcut.to_string()))
        .collect();
    app.command_completion_idx = if app.command_completions.is_empty() {
        None
    } else {
        Some(0)
    };
}

/// Handle keys in Command mode.
pub fn handle_command(app: &mut App, code: KeyCode, _modifiers: KeyModifiers) {
    match code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.command_input.clear();
            app.command_completions.clear();
            app.command_completion_idx = None;
        }
        KeyCode::Enter => {
            let cmd = app.command_input.clone();
            app.command_input.clear();
            app.command_completions.clear();
            app.command_completion_idx = None;
            app.mode = Mode::Normal;
            execute_command(app, &cmd);
        }
        // Tab — accept current completion or cycle to next.
        KeyCode::Tab => {
            if !app.command_completions.is_empty() {
                if let Some(idx) = app.command_completion_idx {
                    app.command_input = app.command_completions[idx].0.clone();
                    // After accepting, recompute for further narrowing.
                    update_command_completions(app);
                }
            }
        }
        // Shift+Tab or Up — cycle completion backwards.
        KeyCode::BackTab | KeyCode::Up => {
            if !app.command_completions.is_empty() {
                if let Some(idx) = app.command_completion_idx {
                    app.command_completion_idx = Some(if idx == 0 {
                        app.command_completions.len() - 1
                    } else {
                        idx - 1
                    });
                }
            }
        }
        // Down — cycle completion forward.
        KeyCode::Down => {
            if !app.command_completions.is_empty() {
                if let Some(idx) = app.command_completion_idx {
                    app.command_completion_idx = Some((idx + 1) % app.command_completions.len());
                }
            }
        }
        KeyCode::Backspace => {
            app.command_input.pop();
            if app.command_input.is_empty() {
                app.mode = Mode::Normal;
                app.command_completions.clear();
                app.command_completion_idx = None;
            } else {
                update_command_completions(app);
            }
        }
        KeyCode::Char(c) => {
            app.command_input.push(c);
            update_command_completions(app);
        }
        _ => {}
    }
}

/// Handle keys in Visual / Visual-Line mode.
pub fn handle_visual(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    match code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.tab_mut().visual_anchor = None;
        }

        // Navigation (same as normal mode)
        KeyCode::Char('h') | KeyCode::Left => {
            let tab = app.tab_mut();
            tab.cursor.col = tab.cursor.col.saturating_sub(1);
        }
        KeyCode::Char('l') | KeyCode::Right => {
            app.tab_mut().cursor.col += 1;
            app.clamp_cursor();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let tab = app.tab_mut();
            tab.cursor.row = tab.cursor.row.saturating_sub(1);
            app.clamp_cursor();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.tab_mut().cursor.row += 1;
            app.clamp_cursor();
        }
        KeyCode::Char('w') => {
            let tab = app.tab_mut();
            let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            let new_pos = tab.buffer.next_word_start(pos);
            tab.cursor = tab.buffer.char_idx_to_cursor(new_pos);
            app.clamp_cursor();
        }
        KeyCode::Char('b') => {
            let tab = app.tab_mut();
            let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            let new_pos = tab.buffer.prev_word_start(pos);
            tab.cursor = tab.buffer.char_idx_to_cursor(new_pos);
        }
        KeyCode::Char('0') => app.tab_mut().cursor.col = 0,
        KeyCode::Char('$') => {
            let tab = app.tab_mut();
            if let Some(line) = tab.buffer.line(tab.cursor.row) {
                tab.cursor.col = line.len_chars().saturating_sub(1);
            }
        }
        KeyCode::Char('G') => {
            let tab = app.tab_mut();
            tab.cursor.row = tab.buffer.line_count().saturating_sub(1);
            tab.cursor.col = 0;
        }
        KeyCode::Char('g') => {
            app.tab_mut().cursor.row = 0;
            app.tab_mut().cursor.col = 0;
        }

        // Delete selection
        KeyCode::Char('d') | KeyCode::Char('x') => {
            if app.mode == Mode::VisualBlock {
                // Block delete: remove column range from each row.
                if let Some((sr, er, sc, ec)) = app.visual_block_rect() {
                    let mut yanked = String::new();
                    // Delete from bottom to top to avoid index shifting.
                    for row in (sr..=er).rev() {
                        let tab = app.tab_mut();
                        let line_start = tab
                            .buffer
                            .cursor_to_char_idx(&aura_core::Cursor::new(row, 0));
                        let line_len = tab
                            .buffer
                            .line(row)
                            .map(|l| l.len_chars().saturating_sub(1))
                            .unwrap_or(0);
                        let del_start = line_start + sc.min(line_len);
                        let del_end = line_start + (ec + 1).min(line_len);
                        if del_end > del_start {
                            let text = tab.buffer.rope().slice(del_start..del_end).to_string();
                            yanked = format!("{text}\n{yanked}");
                            tab.buffer.delete(del_start, del_end, AuthorId::human());
                        }
                    }
                    app.register = Some(yanked);
                    app.tab_mut().cursor.col = sc;
                    app.clamp_cursor();
                    app.mark_highlights_dirty();
                }
            } else if let Some((start, end)) = app.visual_selection_range() {
                let tab = app.tab_mut();
                let text = tab.buffer.rope().slice(start..end).to_string();
                app.register = Some(text);
                let tab = app.tab_mut();
                tab.buffer.delete(start, end, AuthorId::human());
                tab.cursor = tab.buffer.char_idx_to_cursor(start);
                app.clamp_cursor();
                app.mark_highlights_dirty();
            }
            app.mode = Mode::Normal;
            app.tab_mut().visual_anchor = None;
        }
        // Yank selection
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if app.mode == Mode::VisualBlock {
                if let Some((sr, er, sc, ec)) = app.visual_block_rect() {
                    let mut yanked = String::new();
                    for row in sr..=er {
                        let tab = app.tab();
                        let line_start = tab
                            .buffer
                            .cursor_to_char_idx(&aura_core::Cursor::new(row, 0));
                        let line_len = tab
                            .buffer
                            .line(row)
                            .map(|l| l.len_chars().saturating_sub(1))
                            .unwrap_or(0);
                        let yank_start = line_start + sc.min(line_len);
                        let yank_end = line_start + (ec + 1).min(line_len);
                        if yank_end > yank_start {
                            let text = tab.buffer.rope().slice(yank_start..yank_end).to_string();
                            yanked.push_str(&text);
                        }
                        yanked.push('\n');
                    }
                    app.register = Some(yanked.clone());
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(&yanked);
                    }
                    app.set_status("Yanked block");
                }
            } else if let Some((start, end)) = app.visual_selection_range() {
                let text = app.tab().buffer.rope().slice(start..end).to_string();
                app.register = Some(text.clone());
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let _ = clipboard.set_text(&text);
                }
                app.set_status("Yanked selection");
            }
            app.mode = Mode::Normal;
            app.tab_mut().visual_anchor = None;
        }
        // I — block insert (prepend text to each line in block).
        KeyCode::Char('I') if app.mode == Mode::VisualBlock => {
            if let Some((sr, _er, sc, _ec)) = app.visual_block_rect() {
                app.tab_mut().cursor.row = sr;
                app.tab_mut().cursor.col = sc;
                app.mode = Mode::Insert;
                // The actual multi-line insertion happens when exiting Insert
                // mode — for now, just position cursor at block start.
                app.tab_mut().visual_anchor = None;
            }
        }
        // A — block append (append text after each line in block).
        KeyCode::Char('A') if app.mode == Mode::VisualBlock => {
            if let Some((sr, _er, _sc, ec)) = app.visual_block_rect() {
                app.tab_mut().cursor.row = sr;
                app.tab_mut().cursor.col = ec + 1;
                app.mode = Mode::Insert;
                app.tab_mut().visual_anchor = None;
            }
        }
        // Ctrl+C / Cmd+C — copy selection to system clipboard.
        KeyCode::Char('c')
            if modifiers.contains(KeyModifiers::CONTROL)
                || modifiers.contains(KeyModifiers::SUPER) =>
        {
            if let Some((start, end)) = app.visual_selection_range() {
                let text = app.tab().buffer.rope().slice(start..end).to_string();
                app.register = Some(text.clone());
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let _ = clipboard.set_text(&text);
                }
                app.set_status("Copied to clipboard");
            }
            app.mode = Mode::Normal;
            app.tab_mut().visual_anchor = None;
        }

        _ => {}
    }
}

/// Handle leader key sequences (<Space> + key).
fn handle_leader(app: &mut App, code: KeyCode) {
    // Check custom leader mappings from aura.toml first.
    if let KeyCode::Char(c) = code {
        if let Some(action) = app.config.keybindings.leader_action(c) {
            let action = action.to_string();
            if execute_action(app, &action) {
                return;
            }
        }
    }

    match code {
        // <leader>u — undo AI edits
        KeyCode::Char('u') => {
            let ai_id = AuthorId::ai("claude");
            if app.tab_mut().buffer.undo_by_author(&ai_id) {
                app.set_status("Undid last AI edit");
                app.clamp_cursor();
                app.mark_highlights_dirty();
            } else {
                app.set_status("No AI edits to undo");
            }
        }
        // <leader>a — toggle authorship markers
        KeyCode::Char('a') => {
            app.show_authorship = !app.show_authorship;
            let state = if app.show_authorship { "on" } else { "off" };
            app.set_status(format!("Authorship markers: {state}"));
        }
        // <leader>i — enter Intent mode
        KeyCode::Char('i') => {
            if app.has_ai() {
                app.mode = Mode::Intent;
                app.intent_input.clear();
                app.set_status("Type your intent, then press Enter");
            } else {
                app.set_status("No API key. Set ANTHROPIC_API_KEY");
            }
        }
        // <leader>e — explain selected code
        KeyCode::Char('e') => {
            if app.has_ai() {
                app.send_intent(
                    "Explain this code concisely. Output only the explanation as a comment block.",
                );
            } else {
                app.set_status("No API key. Set ANTHROPIC_API_KEY");
            }
        }
        // <leader>f — fix errors at cursor
        KeyCode::Char('f') => {
            if app.has_ai() {
                app.send_intent(
                    "Fix any bugs or errors in this code. Output only the corrected code.",
                );
            } else {
                app.set_status("No API key. Set ANTHROPIC_API_KEY");
            }
        }
        // <leader>t — generate test
        KeyCode::Char('t') => {
            if app.has_ai() {
                app.send_intent(
                    "Generate a unit test for this function. Output only the test code.",
                );
            } else {
                app.set_status("No API key. Set ANTHROPIC_API_KEY");
            }
        }
        // <leader>c — show conversation history for current line
        KeyCode::Char('c') => {
            app.show_conversation_at_cursor();
        }
        // <leader>d — show recent decision summary
        KeyCode::Char('d') => {
            app.show_recent_decisions();
        }
        // <leader>g — cycle AI suggestion aggressiveness
        KeyCode::Char('g') => {
            app.cycle_aggressiveness();
        }
        // <leader>b — toggle inline blame
        KeyCode::Char('b') => {
            app.toggle_blame();
        }
        // <leader>s — show semantic info for symbol at cursor
        KeyCode::Char('s') => {
            app.update_semantic_context();
            let info = app.tab().semantic_info.clone();
            if let Some(info) = info {
                app.set_status(info.replace('\n', " │ "));
            } else {
                app.set_status("No semantic info at cursor");
            }
        }
        // <leader>p — open fuzzy file picker
        KeyCode::Char('p') => {
            app.open_file_picker();
        }
        // <leader>n — next tab
        KeyCode::Char('n') => {
            if app.tabs.count() > 1 {
                app.tabs.next();
            } else {
                app.set_status("Only one tab open");
            }
        }
        // <leader>w — close current tab
        KeyCode::Char('w') => {
            if app.tabs.count() > 1 {
                if app.close_current_tab() {
                    app.should_quit = true;
                }
            } else {
                app.set_status("Cannot close last tab (use :q to quit)");
            }
        }
        // <leader>1-9 — switch to tab by index
        KeyCode::Char(c @ '1'..='9') => {
            let idx = (c as usize) - ('1' as usize);
            if idx < app.tabs.count() {
                app.tabs.switch_to(idx);
            } else {
                app.set_status(format!("No tab {}", c));
            }
        }
        _ => {
            app.set_status("Unknown leader command");
        }
    }
}

/// Handle keys in Intent mode (user typing natural language).
pub fn handle_intent(app: &mut App, code: KeyCode, _modifiers: KeyModifiers) {
    match code {
        KeyCode::Esc => {
            // If we were editing or revising a proposal, go back to Review.
            if app.editing_proposal || app.revising_proposal {
                app.editing_proposal = false;
                app.revising_proposal = false;
                app.intent_input.clear();
                app.mode = Mode::Review;
                app.set_status("Edit/revision cancelled — back in Review mode");
            } else {
                app.mode = Mode::Normal;
                app.intent_input.clear();
                app.set_status("");
            }
        }
        KeyCode::Enter => {
            let input = app.intent_input.clone();
            app.intent_input.clear();

            if app.editing_proposal {
                // Apply the (now-edited) text back to the proposal in-place.
                app.editing_proposal = false;
                if input.is_empty() {
                    // Empty edit → return to Review without changes.
                    app.mode = Mode::Review;
                    app.set_status("Edit cancelled (empty input) — back in Review mode");
                } else if let Some(proposal) = app.proposal.as_mut() {
                    proposal.proposed_text = input;
                    app.mode = Mode::Review;
                    app.set_status("Proposal updated — press 'a' to accept or 'r' to reject");
                } else {
                    app.mode = Mode::Normal;
                }
            } else if app.revising_proposal {
                // Build a revision prompt: current proposal + user instructions.
                app.revising_proposal = false;
                if input.is_empty() {
                    app.mode = Mode::Review;
                    app.set_status("Revision cancelled (empty input) — back in Review mode");
                } else {
                    // Grab the current proposed text to include in the revision
                    // request, then clear the proposal so send_intent creates a
                    // fresh one with the same buffer range.
                    let (revision_intent, start, end) = if let Some(proposal) = app.proposal.take()
                    {
                        let revision_intent = format!(
                            "Revise the following code:\n\n{}\n\nRevision request: {}",
                            proposal.proposed_text, input
                        );
                        (revision_intent, proposal.start, proposal.end)
                    } else {
                        (input.clone(), 0, 0)
                    };
                    // send_intent determines start/end from the visual
                    // selection or current line, which matches proposal.start/end.
                    // We just need the intent string; the range is re-derived.
                    let _ = (start, end); // suppress unused-variable warning
                    app.send_intent(&revision_intent);
                }
            } else if input.is_empty() {
                app.mode = Mode::Normal;
            } else {
                app.send_intent(&input);
            }
        }
        KeyCode::Backspace => {
            app.intent_input.pop();
            if app.intent_input.is_empty() && !app.editing_proposal && !app.revising_proposal {
                app.mode = Mode::Normal;
            }
        }
        KeyCode::Char(c) => {
            app.intent_input.push(c);
        }
        _ => {}
    }
}

/// Handle keys in Review mode (accepting/rejecting AI proposals).
pub fn handle_review(app: &mut App, code: KeyCode, _modifiers: KeyModifiers) {
    match code {
        // Accept the proposal.
        KeyCode::Char('a') | KeyCode::Enter => {
            app.accept_proposal();
        }
        // Reject the proposal.
        KeyCode::Char('r') | KeyCode::Esc => {
            app.reject_proposal();
        }
        // Edit-in-place: copy proposed text into intent_input and switch to
        // Intent mode. When Enter is pressed the edited text will be written
        // back as the proposal without sending a new AI request.
        KeyCode::Char('e') => {
            if let Some(proposal) = &app.proposal {
                // Only allow editing once streaming is complete.
                if proposal.streaming {
                    app.set_status("Wait for AI to finish streaming before editing");
                    return;
                }
                let text = proposal.proposed_text.clone();
                app.intent_input = text;
                app.editing_proposal = true;
                app.revising_proposal = false;
                app.mode = Mode::Intent;
                app.set_status("Edit proposal text, then press Enter to confirm (Esc to cancel)");
            }
        }
        // Revision request: pre-fill a prompt and switch to Intent mode. When
        // Enter is pressed a new AI call is made that includes the current
        // proposed text and the revision instructions typed by the user.
        KeyCode::Char('R') => {
            if let Some(proposal) = &app.proposal {
                if proposal.streaming {
                    app.set_status("Wait for AI to finish streaming before requesting a revision");
                    return;
                }
                app.intent_input.clear();
                app.revising_proposal = true;
                app.editing_proposal = false;
                app.mode = Mode::Intent;
                app.set_status("Describe your revision, then press Enter (Esc to cancel)");
            }
        }
        _ => {}
    }
}

/// Execute a command-mode command.
/// Execute a command from the command palette (public entry point).
pub fn execute_command_from_palette(app: &mut App, cmd: &str) {
    execute_command(app, cmd);
}

fn execute_command(app: &mut App, cmd: &str) {
    match cmd.trim() {
        "w" => {
            // Format on save if enabled.
            if app.config.editor.format_on_save {
                app.format_current_buffer();
            }
            match app.tab_mut().buffer.save() {
                Ok(_) => {
                    app.set_status("Written");
                    if app.sidebar_view == SidebarView::Git {
                        app.refresh_source_control();
                    }
                    // Notify plugins of the save.
                    if let Some(path) = app.tab().buffer.file_path() {
                        let path_str = path.display().to_string();
                        app.plugin_manager.notify_save(&path_str);
                    }
                }
                Err(e) => app.set_status(format!("Error: {}", e)),
            }
        }
        "q" => {
            if app.tabs.count() > 1 {
                // Multiple tabs: close current tab.
                if app.tab().is_modified() {
                    app.set_status("Unsaved changes! Use :q! to force or :wq to save and close");
                } else if app.close_current_tab() {
                    app.should_quit = true;
                }
            } else {
                // Last tab: quit app.
                if app.tab().buffer.is_modified() {
                    app.set_status(
                        "Unsaved changes! Use :q! to force quit or :wq to save and quit",
                    );
                } else {
                    app.should_quit = true;
                }
            }
        }
        "q!" => {
            if app.tabs.count() > 1 {
                if app.close_current_tab_force() {
                    app.should_quit = true;
                }
            } else {
                app.should_quit = true;
            }
        }
        // :qa — quit all tabs (warns if any have unsaved changes).
        "qa" => {
            let has_unsaved = app.tabs.tabs().iter().any(|t| t.is_modified());
            if has_unsaved {
                app.set_status(
                    "Unsaved changes! Use :qa! to force quit or :wqa to save all and quit",
                );
            } else {
                app.should_quit = true;
            }
        }
        // :qa! — force quit all tabs, discarding unsaved changes.
        "qa!" => {
            app.should_quit = true;
        }
        // :wqa — save all tabs and quit.
        "wqa" => {
            let mut save_failed = false;
            for tab in app.tabs.tabs_mut() {
                if tab.is_modified() {
                    if let Err(e) = tab.buffer.save() {
                        app.set_status(format!("Error saving: {}", e));
                        save_failed = true;
                        break;
                    }
                }
            }
            if !save_failed {
                app.should_quit = true;
            }
        }
        "wq" => match app.tab_mut().buffer.save() {
            Ok(_) => {
                if app.tabs.count() > 1 {
                    if app.close_current_tab_force() {
                        app.should_quit = true;
                    }
                } else {
                    app.should_quit = true;
                }
            }
            Err(e) => app.set_status(format!("Error saving: {}", e)),
        },
        "intent" => {
            if app.has_ai() {
                app.mode = Mode::Intent;
                app.intent_input.clear();
                app.set_status("Type your intent, then press Enter");
            } else {
                app.set_status("No API key. Set ANTHROPIC_API_KEY");
            }
        }
        _ if cmd.trim().starts_with("search ") => {
            let query = cmd.trim().strip_prefix("search ").unwrap_or("").trim();
            if !query.is_empty() {
                app.search_conversations(query);
            }
        }
        "decisions" | "dec" => {
            app.show_recent_decisions();
        }
        "seed-history" => {
            app.seed_conversation_history();
        }
        "undo-tree" | "ut" => {
            app.show_undo_tree();
        }
        // Git commands
        "commit" | "gc" => {
            app.generate_commit_message();
        }
        _ if cmd.trim().starts_with("commit ") => {
            let msg = cmd.trim().strip_prefix("commit ").unwrap_or("").trim();
            if !msg.is_empty() {
                app.git_commit(msg);
                app.set_status(format!("Committed: {msg}"));
            }
        }
        "branches" | "br" => {
            app.open_branch_picker();
        }
        "graph" | "git-graph" => {
            app.open_git_graph();
        }
        _ if cmd.trim().starts_with("checkout ") => {
            let branch = cmd.trim().strip_prefix("checkout ").unwrap_or("").trim();
            if !branch.is_empty() {
                app.git_checkout(branch);
            }
        }
        _ if cmd.trim().starts_with("branch ") => {
            let name = cmd.trim().strip_prefix("branch ").unwrap_or("").trim();
            if !name.is_empty() {
                app.git_create_branch(name);
            }
        }
        "blame" => {
            app.toggle_blame();
        }
        // Aura git log — shows commits with Aura-Conversation trailers.
        "log" | "gl" => {
            app.show_aura_log(50);
        }
        _ if cmd.trim().starts_with("log ") => {
            let limit_str = cmd.trim().strip_prefix("log ").unwrap_or("50").trim();
            let limit = limit_str.parse::<usize>().unwrap_or(50);
            app.show_aura_log(limit);
        }
        // Experimental mode — create a branch and auto-accept AI suggestions.
        _ if cmd.trim().starts_with("experiment ") => {
            let name = cmd.trim().strip_prefix("experiment ").unwrap_or("").trim();
            app.enter_experiment_mode(name);
        }
        "experiment" => {
            app.set_status("Usage: experiment <name>");
        }
        // Request LSP code actions at the cursor and optionally feed them to AI.
        "code-action" | "ca" => {
            app.lsp_request_code_actions();
        }
        // List all registered plugins.
        "plugins" => {
            let names = app.plugin_manager.plugin_names();
            if names.is_empty() {
                app.set_status("No plugins loaded");
            } else {
                app.set_status(format!("Plugins: {}", names.join(", ")));
            }
        }
        // :help — open help overlay; :help <topic> — open specific topic.
        "help" => {
            app.help.open();
        }
        _ if cmd.trim().starts_with("help ") => {
            let topic = cmd.trim().strip_prefix("help ").unwrap_or("").trim();
            if !topic.is_empty() {
                app.help.open_topic(topic);
            } else {
                app.help.open();
            }
        }
        // :files / :fp — open fuzzy file picker.
        "files" | "fp" => {
            app.open_file_picker();
        }
        // :tabnew — open a new scratch tab.
        "tabnew" => {
            let buf = aura_core::Buffer::new();
            let theme = app.theme.clone();
            let conv_store = app.conversation_store.as_ref();
            let tab = crate::tab::EditorTab::new(buf, conv_store, &theme);
            app.tabs.open(tab);
            app.set_status("[scratch] tab opened");
        }
        // :tabe <file> / :tabedit <file> — open file in new tab.
        _ if cmd.trim().starts_with("tabe ") || cmd.trim().starts_with("tabedit ") => {
            let arg = cmd
                .trim()
                .strip_prefix("tabedit ")
                .or_else(|| cmd.trim().strip_prefix("tabe "))
                .unwrap_or("")
                .trim();
            if arg.is_empty() {
                app.set_status("Usage: :tabe <file>");
            } else {
                let path = std::path::PathBuf::from(arg);
                if let Err(e) = app.open_file(path) {
                    app.set_status(e);
                }
            }
        }
        // :tabc / :tabclose — close current tab.
        "tabc" | "tabclose" => {
            if app.close_current_tab() {
                app.should_quit = true;
            }
        }
        // :tabc! / :tabclose! — force close current tab.
        "tabc!" | "tabclose!" => {
            if app.close_current_tab_force() {
                app.should_quit = true;
            }
        }
        // :tabn / :tabnext — next tab.
        "tabn" | "tabnext" => {
            app.tabs.next();
        }
        // :tabp / :tabprev — previous tab.
        "tabp" | "tabprev" => {
            app.tabs.prev();
        }
        // :term / :terminal — toggle terminal pane and give it focus.
        "term" | "terminal" => {
            if app.terminal().visible && app.terminal_focused {
                app.terminal_mut().visible = false;
                app.terminal_focused = false;
            } else {
                app.terminal_mut().visible = true;
                app.terminal_focused = true;
            }
        }
        "term new" => {
            app.new_terminal_tab();
        }
        "term close" => {
            app.close_terminal_tab();
        }
        "term next" => {
            app.next_terminal_tab();
        }
        "term prev" => {
            app.prev_terminal_tab();
        }
        // :tree — toggle the file tree sidebar.
        "tree" => {
            app.file_tree.toggle();
            let state = if app.file_tree.visible { "on" } else { "off" };
            app.set_status(format!("File tree: {state}"));
        }
        // :git / :sc — open the source control panel.
        "git" | "sc" => {
            if !app.file_tree.visible {
                app.file_tree.toggle();
            }
            app.sidebar_view = SidebarView::Git;
            app.refresh_source_control();
            app.file_tree_focused = false;
            app.source_control_focused = true;
        }
        // :term-height <N> — set terminal pane height in rows.
        cmd if cmd.starts_with("term-height ") || cmd.starts_with("th ") => {
            let arg = cmd.split_whitespace().nth(1).unwrap_or("");
            match arg.parse::<u16>() {
                Ok(h) => {
                    let h = h.clamp(5, 50);
                    app.terminal_mut().height = h;
                    app.set_status(format!("Terminal height: {h} rows"));
                }
                Err(_) => {
                    app.set_status(format!(
                        "Current terminal height: {} rows",
                        app.terminal().height
                    ));
                }
            }
        }
        // :%s/old/new/g — global search and replace.
        // :s/old/new/g — replace on current line only.
        _ if cmd.trim().starts_with("%s/") || cmd.trim().starts_with("s/") => {
            let trimmed = cmd.trim();
            let is_global = trimmed.starts_with("%s/");
            let pattern_str = if is_global {
                &trimmed[3..]
            } else {
                &trimmed[2..]
            };
            // Parse old/new from the pattern (split on unescaped /).
            let parts: Vec<&str> = pattern_str.splitn(3, '/').collect();
            if parts.len() >= 2 && !parts[0].is_empty() {
                let old = parts[0];
                let new_text = parts[1];
                let (range_start, range_end) = if is_global {
                    (0, app.tab().buffer.len_chars())
                } else {
                    let row = app.tab().cursor.row;
                    let start = app
                        .tab()
                        .buffer
                        .cursor_to_char_idx(&aura_core::Cursor::new(row, 0));
                    let end = if row + 1 < app.tab().buffer.line_count() {
                        app.tab()
                            .buffer
                            .cursor_to_char_idx(&aura_core::Cursor::new(row + 1, 0))
                    } else {
                        app.tab().buffer.len_chars()
                    };
                    (start, end)
                };
                let count = app.tab_mut().buffer.replace_all(
                    old,
                    new_text,
                    range_start,
                    range_end,
                    AuthorId::human(),
                );
                app.mark_highlights_dirty();
                app.set_status(format!(
                    "{count} replacement{}",
                    if count == 1 { "" } else { "s" }
                ));
                // Update search highlights if active.
                if app.search_query.is_some() {
                    app.execute_search();
                }
            } else {
                app.set_status("Usage: :%s/old/new/g or :s/old/new/g");
            }
        }
        // :noh / :nohlsearch — clear search highlights.
        "noh" | "nohlsearch" => {
            app.clear_search();
            app.set_status("Search cleared");
        }
        // :version — display current AURA version.
        "version" | "ver" => {
            app.set_status(format!("AURA v{}", crate::update::CURRENT_VERSION));
        }
        // :update / :check-update — show update info and upgrade instructions.
        "update" | "check-update" => {
            // Force a fresh check (bypasses cache).
            app.force_update_check();
        }
        // --- Split panes ---
        "vsplit" | "vs" => {
            app.split_vertical();
        }
        "hsplit" | "hs" | "split" | "sp" => {
            app.split_horizontal();
        }
        "only" => {
            app.split_close();
        }
        // --- Settings ---
        "settings" | "preferences" | "prefs" => {
            app.open_settings();
        }
        // --- Conversation compaction ---
        "compact" => {
            app.compact_conversations();
        }
        // --- Collaboration commands ---
        "host" => {
            app.start_collab_host();
        }
        "collab-stop" | "collab stop" => {
            app.stop_collab();
        }
        "unfollow" => {
            app.stop_follow();
        }
        "share-term" | "share-terminal" => {
            if let Some(session) = &app.collab {
                if session.is_host {
                    app.collab_sharing_terminal = !app.collab_sharing_terminal;
                    let state = if app.collab_sharing_terminal {
                        "on"
                    } else {
                        "off"
                    };
                    app.set_status(format!("Terminal sharing {state}"));
                } else {
                    app.set_status("Only the host can share their terminal");
                }
            } else {
                app.set_status("Not in a collab session");
            }
        }
        "view-term" | "view-terminal" => {
            if app.collab_shared_terminal.is_some() {
                app.viewing_shared_terminal = !app.viewing_shared_terminal;
                let state = if app.viewing_shared_terminal {
                    "showing host terminal"
                } else {
                    "showing local terminal"
                };
                app.set_status(format!("Terminal view: {state}"));
            } else {
                app.set_status("No shared terminal available");
            }
        }
        // --- Git stash & PR ---
        "stash" => {
            app.sc_stash_push();
        }
        "stash pop" => {
            app.sc_stash_pop();
        }
        "stash drop" => {
            app.sc_stash_drop();
        }
        "pr" | "pull-request" => {
            app.create_pr();
        }
        // --- Tasks ---
        "task" | "tasks" => {
            let tasks = app.get_tasks();
            if tasks.is_empty() {
                app.set_status("No tasks available. Add [tasks] to aura.toml");
            } else {
                let list: Vec<String> = tasks
                    .iter()
                    .map(|(name, t)| format!("{name}: {}", t.description))
                    .collect();
                app.set_status(format!("Tasks: {}", list.join(" | ")));
            }
        }
        // --- Navigation ---
        "outline" | "symbols" => {
            app.open_outline();
        }
        // --- Agent mode ---
        "agent stop" => {
            app.stop_agent("user request");
        }
        // --- Marks ---
        "fix" => {
            app.fix_last_failed_command();
        }
        "registers" | "reg" => {
            app.open_registers_modal();
        }
        "marks" => {
            let marks: Vec<String> = app
                .tab()
                .marks
                .iter()
                .map(|(k, v)| format!("'{k} → {}:{}", v.row + 1, v.col))
                .collect();
            if marks.is_empty() {
                app.set_status("No marks set");
            } else {
                app.set_status(marks.join("  "));
            }
        }
        // --- Set commands ---
        "set relativenumber" | "set rnu" => {
            app.config.editor.relative_line_numbers = true;
            app.set_status("Relative line numbers ON");
        }
        "set norelativenumber" | "set nornu" => {
            app.config.editor.relative_line_numbers = false;
            app.set_status("Relative line numbers OFF");
        }
        "set wrap" => {
            app.config.editor.word_wrap = true;
            app.set_status("Word wrap ON");
        }
        "set nowrap" => {
            app.config.editor.word_wrap = false;
            app.set_status("Word wrap OFF");
        }
        // --- Project search ---
        "search" | "grep" => {
            app.open_project_search();
        }
        // --- Inline conflict resolution ---
        "accept-current" | "ac" => {
            app.resolve_inline_conflict(crate::merge_view::Resolution::AcceptCurrent);
        }
        "accept-incoming" | "ai" => {
            app.resolve_inline_conflict(crate::merge_view::Resolution::AcceptIncoming);
        }
        "accept-both" | "ab" => {
            app.resolve_inline_conflict(crate::merge_view::Resolution::AcceptBothCurrentFirst);
        }
        "accept-both-incoming" | "abi" => {
            app.resolve_inline_conflict(crate::merge_view::Resolution::AcceptBothIncomingFirst);
        }
        // --- AI Visor ---
        "visor" | "ai-visor" => {
            app.toggle_ai_visor();
        }
        // --- LSP features ---
        "references" | "ref" => {
            app.lsp_references();
        }
        "rename" => {
            app.lsp_rename_start();
        }
        // --- Merge conflict ---
        "merge" => {
            // Open merge view for the currently selected file in source control.
            if let Some(rel_path) = app.source_control.selected_path().map(|s| s.to_string()) {
                app.open_merge_view(&rel_path);
            } else {
                app.set_status("Select a conflict file in source control first");
            }
        }
        // --- Debugger commands ---
        "debug" | "db" => {
            app.start_debug_session();
        }
        "breakpoint" | "bp" => {
            app.toggle_breakpoint();
        }
        "continue" | "dc" => {
            app.debug_continue();
        }
        "step" | "ds" => {
            app.debug_step_over();
        }
        "stepin" | "dsi" => {
            app.debug_step_in();
        }
        "stepout" | "dso" => {
            app.debug_step_out();
        }
        "debug stop" | "dstop" => {
            app.debug_stop();
        }
        "debug panel" | "dp" => {
            app.debug_panel.toggle();
            app.debug_panel_focused = app.debug_panel.visible;
        }
        other => {
            // Handle commands with arguments.
            if let Some(name) = other.strip_prefix("follow ") {
                let name = name.trim();
                if !name.is_empty() {
                    app.start_follow(name);
                } else {
                    app.set_status("Usage: :follow <peer-name>");
                }
            } else if let Some(rest) = other.strip_prefix("join ") {
                // Support ":join addr:port token" format.
                let parts: Vec<&str> = rest.trim().splitn(2, ' ').collect();
                let addr = parts[0];
                let token = parts.get(1).map(|t| t.trim());
                app.join_collab_with_token(addr, token);
            } else if let Some(task_name) = other.strip_prefix("task ") {
                let task_name = task_name.trim();
                if !task_name.is_empty() {
                    app.run_task(task_name);
                }
            } else if let Some(task) = other.strip_prefix("agent ") {
                let task = task.trim();
                if task == "stop" {
                    app.stop_agent("user request");
                } else if task == "pause" {
                    app.pause_agent();
                } else if task == "resume" {
                    app.resume_agent();
                } else if let Some(level) = task.strip_prefix("trust ") {
                    let level = level.trim();
                    if let Some(trust) = crate::app::TrustLevel::parse_str(level) {
                        if let Some(ref mut session) = app.agent_mode {
                            session.trust_level = trust;
                            app.set_status(format!("Agent trust level: {}", trust.label()));
                        } else {
                            app.set_status(
                                "No active agent. Start one with :agent <task>".to_string(),
                            );
                        }
                    } else {
                        app.set_status("Invalid trust level. Use: read, write, or full");
                    }
                } else if task == "timeline" {
                    if let Some(ref mut session) = app.agent_mode {
                        session.timeline.visible = !session.timeline.visible;
                    } else {
                        app.set_status("No active agent session");
                    }
                } else if task == "diff" {
                    app.show_agent_diff();
                } else if let Some(plan_task) = task.strip_prefix("plan ") {
                    let plan_task = plan_task.trim();
                    if !plan_task.is_empty() {
                        let (max_iters, actual_task) =
                            if let Some(rest) = plan_task.strip_prefix("-n ") {
                                let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                                let n = parts[0].parse::<usize>().unwrap_or(50);
                                let t = parts.get(1).unwrap_or(&"").trim().to_string();
                                (n, t)
                            } else {
                                (50, plan_task.to_string())
                            };
                        if !actual_task.is_empty() {
                            app.start_agent_with_options(
                                &actual_task,
                                max_iters,
                                crate::app::TrustLevel::FullAuto,
                                true,
                            );
                        }
                    }
                } else if !task.is_empty() {
                    let (max_iters, actual_task) = if let Some(rest) = task.strip_prefix("-n ") {
                        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                        let n = parts[0].parse::<usize>().unwrap_or(50);
                        let t = parts.get(1).unwrap_or(&"").trim().to_string();
                        (n, t)
                    } else {
                        (50, task.to_string())
                    };
                    if !actual_task.is_empty() {
                        app.start_agent(&actual_task, max_iters);
                    }
                }
            } else if let Some(query) = other
                .strip_prefix("search ")
                .or_else(|| other.strip_prefix("grep "))
            {
                let query = query.trim();
                if !query.is_empty() {
                    app.open_project_search();
                    app.project_search.query = query.to_string();
                    app.execute_project_search();
                }
            } else if let Some(new_name) = other.strip_prefix("rename ") {
                let new_name = new_name.trim();
                if !new_name.is_empty() {
                    app.rename_input = new_name.to_string();
                    app.lsp_rename_execute();
                }
            } else if let Some(program) = other.strip_prefix("debug ") {
                let program = program.trim();
                if program == "stop" {
                    app.debug_stop();
                } else if program == "panel" {
                    app.debug_panel.toggle();
                    app.debug_panel_focused = app.debug_panel.visible;
                } else if !program.is_empty() {
                    app.start_debug_session();
                    app.debug_launch(program);
                }
            } else {
                app.set_status(format!("Unknown command: {}", other));
            }
        }
    }
}

/// Handle keys in Diff (side-by-side diff view) mode.
pub fn handle_diff(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    match code {
        KeyCode::Esc | KeyCode::Char('q') => {
            // Close diff view, return to source control panel.
            app.diff_view = None;
            app.mode = Mode::Normal;
            app.source_control_focused = true;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(dv) = &mut app.diff_view {
                dv.scroll_down(1, 20); // viewport height updated at render time
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(dv) = &mut app.diff_view {
                dv.scroll_up(1);
            }
        }
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(dv) = &mut app.diff_view {
                dv.scroll_down(10, 20);
            }
        }
        KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(dv) = &mut app.diff_view {
                dv.scroll_up(10);
            }
        }
        KeyCode::Char('G') => {
            if let Some(dv) = &mut app.diff_view {
                dv.scroll_to_bottom(20);
            }
        }
        KeyCode::Char('g') => {
            if let Some(dv) = &mut app.diff_view {
                dv.scroll_to_top();
            }
        }
        KeyCode::Enter | KeyCode::Char('o') => {
            // Open the file for editing.
            let rel_path = app.diff_view.as_ref().map(|dv| dv.file_path.clone());
            app.diff_view = None;
            app.mode = Mode::Normal;
            app.source_control_focused = false;
            if let Some(rel_path) = rel_path {
                let workdir = app.git_repo.as_ref().map(|r| r.workdir().to_path_buf());
                if let Some(wd) = workdir {
                    let full_path = wd.join(&rel_path);
                    if let Err(e) = app.open_file(full_path) {
                        app.set_status(e);
                    }
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Vim operator/motion helpers
/// Handle keys in MergeConflict (3-panel merge editor) mode.
pub fn handle_merge_conflict(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    let view = match app.merge_view.as_mut() {
        Some(v) => v,
        None => {
            app.mode = Mode::Normal;
            return;
        }
    };

    match code {
        // Close merge view.
        KeyCode::Esc | KeyCode::Char('q') => {
            app.merge_view = None;
            app.mode = Mode::Normal;
            app.source_control_focused = true;
        }
        // Scroll.
        KeyCode::Char('j') | KeyCode::Down => view.scroll_down(1, 20),
        KeyCode::Char('k') | KeyCode::Up => view.scroll_up(1),
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
            view.scroll_down(10, 20);
        }
        KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
            view.scroll_up(10);
        }
        // Navigate conflicts.
        KeyCode::Char('n') => view.next_conflict(),
        KeyCode::Char('N') => view.prev_conflict(),
        // Resolution keys.
        KeyCode::Char('1') => {
            view.resolve(crate::merge_view::Resolution::AcceptCurrent);
            let remaining = view.total_conflicts - view.resolved_count;
            app.set_status(format!(
                "Accepted current — {remaining} conflict(s) remaining"
            ));
        }
        KeyCode::Char('2') => {
            view.resolve(crate::merge_view::Resolution::AcceptIncoming);
            let remaining = view.total_conflicts - view.resolved_count;
            app.set_status(format!(
                "Accepted incoming — {remaining} conflict(s) remaining"
            ));
        }
        KeyCode::Char('3') => {
            view.resolve(crate::merge_view::Resolution::AcceptBothCurrentFirst);
            let remaining = view.total_conflicts - view.resolved_count;
            app.set_status(format!(
                "Accepted both (current first) — {remaining} conflict(s) remaining"
            ));
        }
        KeyCode::Char('4') => {
            view.resolve(crate::merge_view::Resolution::AcceptBothIncomingFirst);
            let remaining = view.total_conflicts - view.resolved_count;
            app.set_status(format!(
                "Accepted both (incoming first) — {remaining} conflict(s) remaining"
            ));
        }
        KeyCode::Char('5') | KeyCode::Char('i') => {
            view.resolve(crate::merge_view::Resolution::Ignore);
            let remaining = view.total_conflicts - view.resolved_count;
            app.set_status(format!("Ignored — {remaining} conflict(s) remaining"));
        }
        // Cycle focus between panels.
        KeyCode::Tab => view.cycle_focus(),
        // Complete merge.
        KeyCode::Char('c') => {
            app.complete_merge();
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------

use crate::app::{FindCharMode, Operator};

/// Resolve a motion key into a (start, end) character index range.
fn resolve_operator_motion(app: &mut App, code: KeyCode, count: usize) -> Option<(usize, usize)> {
    let tab = app.tab_mut();
    let start = tab.buffer.cursor_to_char_idx(&tab.cursor);

    let end = match code {
        KeyCode::Char('w') => {
            let mut pos = start;
            for _ in 0..count {
                pos = tab.buffer.next_word_start(pos);
            }
            pos
        }
        KeyCode::Char('b') => {
            let mut pos = start;
            for _ in 0..count {
                pos = tab.buffer.prev_word_start(pos);
            }
            pos
        }
        KeyCode::Char('e') => {
            let mut pos = start;
            for _ in 0..count {
                pos = tab.buffer.word_end(pos);
            }
            (pos + 1).min(tab.buffer.len_chars())
        }
        KeyCode::Char('$') => {
            let row = tab.cursor.row;
            if row + 1 < tab.buffer.line_count() {
                tab.buffer
                    .cursor_to_char_idx(&aura_core::Cursor::new(row + 1, 0))
                    - 1
            } else {
                tab.buffer.len_chars()
            }
        }
        KeyCode::Char('0') => tab
            .buffer
            .cursor_to_char_idx(&aura_core::Cursor::new(tab.cursor.row, 0)),
        KeyCode::Char('G') => tab.buffer.len_chars(),
        KeyCode::Char('h') | KeyCode::Left => start.saturating_sub(count),
        KeyCode::Char('l') | KeyCode::Right => (start + count).min(tab.buffer.len_chars()),
        KeyCode::Char('j') | KeyCode::Down => {
            let target_row =
                (tab.cursor.row + count).min(tab.buffer.line_count().saturating_sub(1));
            tab.buffer
                .cursor_to_char_idx(&aura_core::Cursor::new(target_row + 1, 0))
                .min(tab.buffer.len_chars())
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let target_row = tab.cursor.row.saturating_sub(count);
            tab.buffer
                .cursor_to_char_idx(&aura_core::Cursor::new(target_row, 0))
        }
        KeyCode::Char('%') => {
            if let Some(match_idx) = tab.buffer.find_matching_bracket(start) {
                if match_idx > start {
                    match_idx + 1
                } else {
                    match_idx
                }
            } else {
                return None;
            }
        }
        // Text objects handled in the operator-pending section.
        KeyCode::Char('i') | KeyCode::Char('a') => {
            // Phase 2 will add text object support here.
            return None;
        }
        _ => return None,
    };

    let (s, e) = if end < start {
        (end, start)
    } else {
        (start, end)
    };
    Some((s, e))
}

/// Apply an operator to a character range.
fn apply_operator(app: &mut App, op: Operator, start: usize, end: usize) {
    if start == end {
        return;
    }
    match op {
        Operator::Delete => {
            let text = app.tab().buffer.rope().slice(start..end).to_string();
            app.register = Some(text);
            app.tab_mut().buffer.delete(start, end, AuthorId::human());
            let cursor = app.tab().buffer.char_idx_to_cursor(start);
            app.tab_mut().cursor = cursor;
            app.clamp_cursor();
            app.mark_highlights_dirty();
        }
        Operator::Change => {
            let text = app.tab().buffer.rope().slice(start..end).to_string();
            app.register = Some(text);
            app.tab_mut().buffer.delete(start, end, AuthorId::human());
            let cursor = app.tab().buffer.char_idx_to_cursor(start);
            app.tab_mut().cursor = cursor;
            app.clamp_cursor();
            app.mark_highlights_dirty();
            app.mode = Mode::Insert;
        }
        Operator::Yank => {
            let text = app.tab().buffer.rope().slice(start..end).to_string();
            app.register = Some(text);
        }
        Operator::Indent => {
            let start_line = app.tab().buffer.char_idx_to_cursor(start).row;
            let end_line = app
                .tab()
                .buffer
                .char_idx_to_cursor(end.saturating_sub(1).max(start))
                .row;
            let indent = app.tab().indent_style.unit();
            app.tab_mut()
                .buffer
                .indent_lines(start_line, end_line, &indent, AuthorId::human());
            app.mark_highlights_dirty();
        }
        Operator::Dedent => {
            let start_line = app.tab().buffer.char_idx_to_cursor(start).row;
            let end_line = app
                .tab()
                .buffer
                .char_idx_to_cursor(end.saturating_sub(1).max(start))
                .row;
            let tw = app.config.editor.tab_width;
            app.tab_mut()
                .buffer
                .dedent_lines(start_line, end_line, tw, AuthorId::human());
            app.mark_highlights_dirty();
        }
    }
}

/// Execute a character search (f/F/t/T) on the current line.
fn execute_find_char(app: &mut App, mode: FindCharMode, ch: char) {
    let tab = app.tab_mut();
    let row = tab.cursor.row;
    let col = tab.cursor.col;
    let line_text = match tab.buffer.line_text(row) {
        Some(t) => t,
        None => return,
    };
    let chars: Vec<char> = line_text.chars().collect();

    match mode {
        FindCharMode::Forward => {
            for (i, &c) in chars.iter().enumerate().skip(col + 1) {
                if c == ch {
                    tab.cursor.col = i;
                    return;
                }
            }
        }
        FindCharMode::Backward => {
            for i in (0..col).rev() {
                if chars[i] == ch {
                    tab.cursor.col = i;
                    return;
                }
            }
        }
        FindCharMode::ForwardTill => {
            for (i, &c) in chars.iter().enumerate().skip(col + 1) {
                if c == ch {
                    tab.cursor.col = i.saturating_sub(1);
                    return;
                }
            }
        }
        FindCharMode::BackwardTill => {
            for i in (0..col).rev() {
                if chars[i] == ch {
                    tab.cursor.col = i + 1;
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delimiter_pair() {
        assert_eq!(delimiter_pair('('), ('(', ')'));
        assert_eq!(delimiter_pair(')'), ('(', ')'));
        assert_eq!(delimiter_pair('b'), ('(', ')'));
        assert_eq!(delimiter_pair('{'), ('{', '}'));
        assert_eq!(delimiter_pair('}'), ('{', '}'));
        assert_eq!(delimiter_pair('B'), ('{', '}'));
        assert_eq!(delimiter_pair('['), ('[', ']'));
        assert_eq!(delimiter_pair(']'), ('[', ']'));
        assert_eq!(delimiter_pair('<'), ('<', '>'));
        assert_eq!(delimiter_pair('>'), ('<', '>'));
        assert_eq!(delimiter_pair('"'), ('"', '"'));
        assert_eq!(delimiter_pair('\''), ('\'', '\''));
        assert_eq!(delimiter_pair('`'), ('`', '`'));
        // Unknown delimiter maps to itself.
        assert_eq!(delimiter_pair('x'), ('x', 'x'));
    }

    #[test]
    fn test_execute_action_known_actions() {
        // Test that execute_action returns true for all known action names.
        let mut app = App::new(aura_core::Buffer::new());
        let known_actions = vec![
            "toggle_terminal",
            "toggle_chat",
            "toggle_history",
            "toggle_file_tree",
            "toggle_git",
            "open_file_picker",
            "open_command_palette",
            "open_git_graph",
            "open_settings",
            "open_outline",
            "open_visor",
            "open_branch_picker",
            "project_search",
            "save",
            "intent",
            "toggle_blame",
            "cycle_aggressiveness",
            "recent_decisions",
            "next_tab",
            "prev_tab",
            "close_tab",
        ];
        for action in &known_actions {
            let result = execute_action(&mut app, action);
            assert!(result, "Action '{action}' should be handled");
        }
    }

    #[test]
    fn test_execute_action_unknown() {
        let mut app = App::new(aura_core::Buffer::new());
        assert!(!execute_action(&mut app, "nonexistent_action"));
        assert!(!execute_action(&mut app, ""));
        assert!(!execute_action(&mut app, "foobar"));
    }

    #[test]
    fn test_execute_action_intent_sets_mode() {
        let mut app = App::new(aura_core::Buffer::new());
        assert_ne!(app.mode, Mode::Intent);
        execute_action(&mut app, "intent");
        assert_eq!(app.mode, Mode::Intent);
    }
}
