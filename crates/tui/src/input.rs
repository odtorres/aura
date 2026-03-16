//! Keyboard input handling for each editing mode.

use crate::app::{App, Mode};
use crate::source_control::{GitPanelSection, SidebarView};
use aura_core::AuthorId;
use crossterm::event::{KeyCode, KeyModifiers};

/// Handle keys in Normal mode.
pub fn handle_normal(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    // When the terminal pane is focused, route all keystrokes to the PTY.
    if app.terminal_focused {
        match code {
            // Esc — unfocus terminal (return focus to editor).
            KeyCode::Esc => {
                app.terminal_focused = false;
            }
            // Ctrl+` or Ctrl+t — unfocus terminal.
            KeyCode::Char('`') | KeyCode::Char('t')
                if modifiers.contains(KeyModifiers::CONTROL) =>
            {
                app.terminal_focused = false;
            }
            // Ctrl+Shift+Up / Ctrl+Shift+Down — resize terminal pane.
            KeyCode::Up if modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) => {
                app.terminal.height = (app.terminal.height + 2).min(50);
            }
            KeyCode::Down if modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) => {
                app.terminal.height = app.terminal.height.saturating_sub(2).max(5);
            }
            // Ctrl+C — send interrupt.
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.terminal.send_ctrl_c();
            }
            // Ctrl+D — send EOF.
            KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.terminal.send_ctrl_d();
            }
            // Ctrl+L — clear screen.
            KeyCode::Char('l') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.terminal.send_ctrl_l();
            }
            // Other Ctrl+char — send as control code.
            KeyCode::Char(c) if modifiers.contains(KeyModifiers::CONTROL) => {
                let ctrl_byte = (c as u8).wrapping_sub(b'a').wrapping_add(1);
                app.terminal.send_bytes(&[ctrl_byte]);
            }
            KeyCode::Enter => {
                app.terminal.send_enter();
            }
            KeyCode::Backspace => {
                app.terminal.send_backspace();
            }
            KeyCode::Tab => {
                app.terminal.send_tab();
            }
            KeyCode::Up => {
                app.terminal.send_arrow_up();
            }
            KeyCode::Down => {
                app.terminal.send_arrow_down();
            }
            KeyCode::Left => {
                app.terminal.send_arrow_left();
            }
            KeyCode::Right => {
                app.terminal.send_arrow_right();
            }
            KeyCode::Char(c) => {
                app.terminal.send_char(c);
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
            KeyCode::Char('n') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.file_tree_focused = false;
                app.file_tree.toggle();
            }
            KeyCode::Char('g') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.toggle_sidebar_view();
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
                // Ctrl+V — paste from system clipboard.
                KeyCode::Char('v') if modifiers.contains(KeyModifiers::CONTROL) => {
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
            KeyCode::Char('u') => {
                app.sc_unstage_selected();
            }
            KeyCode::Char('d') => {
                if app.source_control.focused_section == GitPanelSection::ChangedFiles {
                    if let Some(entry) = app.source_control.changed.get(app.source_control.selected) {
                        let path = entry.rel_path.clone();
                        app.set_status(format!("Discard changes to {}? (y to confirm)", path));
                        app.source_control.pending_discard = Some(path);
                    }
                }
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
                // Open side-by-side diff view for the selected file.
                if let Some(rel_path) = app.source_control.selected_path().map(|s| s.to_string()) {
                    app.open_diff_view(&rel_path);
                }
            }
            KeyCode::Esc => {
                app.source_control_focused = false;
            }
            KeyCode::Char('n') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.source_control_focused = false;
                app.file_tree.toggle();
            }
            KeyCode::Char('g') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.toggle_sidebar_view();
            }
            _ => {}
        }
        return;
    }

    // When the chat panel is focused, route all keys to chat input.
    if app.chat_panel_focused {
        match code {
            KeyCode::Enter => {
                if !app.chat_panel.streaming {
                    app.send_chat_message();
                }
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
            KeyCode::PageUp => {
                app.chat_panel.page_up(10);
            }
            KeyCode::PageDown => {
                app.chat_panel.page_down(10);
            }
            KeyCode::Esc => {
                app.chat_panel_focused = false;
            }
            KeyCode::Char('j') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.toggle_chat_panel();
            }
            KeyCode::Char('h') if modifiers.contains(KeyModifiers::CONTROL) => {
                // Switch to conversation history.
                app.chat_panel.visible = false;
                app.chat_panel_focused = false;
                app.toggle_conversation_history();
            }
            _ => {}
        }
        return;
    }

    // When the conversation history panel is focused, route keys to it.
    if app.conversation_history_focused {
        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                app.conversation_history.select_down();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.conversation_history.select_up();
            }
            KeyCode::Enter => {
                app.conversation_history_toggle_expand();
            }
            KeyCode::Char('u') => {
                app.conversation_history.scroll_messages_up();
            }
            KeyCode::Char('d') => {
                app.conversation_history.scroll_messages_down();
            }
            KeyCode::Esc => {
                app.conversation_history_focused = false;
            }
            KeyCode::Char('n') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.conversation_history_focused = false;
                app.file_tree.toggle();
            }
            KeyCode::Char('g') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.conversation_history_focused = false;
                app.toggle_sidebar_view();
            }
            KeyCode::Char('h') if modifiers.contains(KeyModifiers::CONTROL) => {
                app.toggle_conversation_history();
            }
            KeyCode::Char('j') if modifiers.contains(KeyModifiers::CONTROL) => {
                // Switch to chat panel.
                app.conversation_history.visible = false;
                app.conversation_history_focused = false;
                app.toggle_chat_panel();
            }
            _ => {}
        }
        return;
    }

    // Ctrl+t — toggle terminal visibility and focus.
    if code == KeyCode::Char('t') && modifiers.contains(KeyModifiers::CONTROL) {
        if app.terminal.visible {
            app.terminal_focused = !app.terminal_focused;
        } else {
            app.terminal.visible = true;
            app.terminal_focused = true;
        }
        return;
    }

    // Ctrl+` — toggle terminal visibility and focus (alternate binding).
    if code == KeyCode::Char('`') && modifiers.contains(KeyModifiers::CONTROL) {
        if app.terminal.visible {
            // Toggle focus: if already focused, unfocus; otherwise focus.
            app.terminal_focused = !app.terminal_focused;
        } else {
            app.terminal.visible = true;
            app.terminal_focused = true;
        }
        return;
    }

    // Ctrl+g — switch sidebar to Git / toggle source control panel.
    if code == KeyCode::Char('g') && modifiers.contains(KeyModifiers::CONTROL) {
        if !app.file_tree.visible {
            app.file_tree.toggle();
        }
        if app.sidebar_view != SidebarView::Git {
            app.sidebar_view = SidebarView::Git;
            app.refresh_source_control();
        }
        app.file_tree_focused = false;
        app.source_control_focused = true;
        app.conversation_history_focused = false;
        return;
    }

    // Ctrl+h — toggle AI conversation history panel.
    if code == KeyCode::Char('h') && modifiers.contains(KeyModifiers::CONTROL) {
        app.toggle_conversation_history();
        return;
    }

    // Ctrl+j — toggle interactive chat panel.
    if code == KeyCode::Char('j') && modifiers.contains(KeyModifiers::CONTROL) {
        app.toggle_chat_panel();
        return;
    }

    // F1 — open help from any mode.
    if code == KeyCode::F(1) {
        app.help.open();
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
                if app.help.in_topics_view()
                    && !modifiers.contains(KeyModifiers::CONTROL) =>
            {
                app.help.type_char(c);
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

    match code {
        // Leader key (Space)
        KeyCode::Char(' ') => {
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

        // Navigation (clears hover popup and notifies speculative engine)
        KeyCode::Char('h') | KeyCode::Left => {
            let tab = app.tab_mut();
            tab.cursor.col = tab.cursor.col.saturating_sub(1);
            tab.hover_info = None;
            app.notify_cursor_moved();
        }
        KeyCode::Char('l') | KeyCode::Right => {
            app.tab_mut().cursor.col += 1;
            app.clamp_cursor();
            app.tab_mut().hover_info = None;
            app.notify_cursor_moved();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let tab = app.tab_mut();
            tab.cursor.row = tab.cursor.row.saturating_sub(1);
            tab.hover_info = None;
            app.clamp_cursor();
            app.notify_cursor_moved();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.tab_mut().cursor.row += 1;
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
        KeyCode::Char('G') => {
            let tab = app.tab_mut();
            tab.cursor.row = tab.buffer.line_count().saturating_sub(1);
            tab.cursor.col = 0;
        }

        // Word movement
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
        KeyCode::Char('e') => {
            let tab = app.tab_mut();
            let pos = tab.buffer.cursor_to_char_idx(&tab.cursor);
            let new_pos = tab.buffer.word_end(pos);
            tab.cursor = tab.buffer.char_idx_to_cursor(new_pos);
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
        // dd — delete current line
        KeyCode::Char('d') => {
            let row = app.tab().cursor.row;
            if let Some(text) = app.tab_mut().buffer.delete_line(row, AuthorId::human()) {
                app.register = Some(text);
                app.clamp_cursor();
                app.mark_highlights_dirty();
            }
        }
        // yy — yank current line
        KeyCode::Char('y') => {
            let row = app.tab().cursor.row;
            if let Some(text) = app.tab().buffer.line_text(row) {
                app.register = Some(text);
                app.set_status("Yanked line");
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
    match code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            // In normal mode, cursor sits on the last char, not after it.
            let tab = app.tab_mut();
            tab.cursor.col = tab.cursor.col.saturating_sub(1);
            app.clamp_cursor();
        }
        KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
            let tab = app.tab_mut();
            let new_pos = tab.buffer.insert_char(&tab.cursor, c, AuthorId::human());
            tab.cursor = tab.buffer.char_idx_to_cursor(new_pos);
            app.mark_highlights_dirty();
        }
        KeyCode::Enter => {
            let tab = app.tab_mut();
            let new_pos = tab.buffer.insert_char(&tab.cursor, '\n', AuthorId::human());
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
        // Ctrl+S to save even in insert mode.
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            match app.tab_mut().buffer.save() {
                Ok(_) => app.set_status("Saved"),
                Err(e) => app.set_status(format!("Error saving: {}", e)),
            }
        }
        _ => {}
    }
}

/// Handle keys in Command mode.
pub fn handle_command(app: &mut App, code: KeyCode, _modifiers: KeyModifiers) {
    match code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.command_input.clear();
        }
        KeyCode::Enter => {
            let cmd = app.command_input.clone();
            app.command_input.clear();
            app.mode = Mode::Normal;
            execute_command(app, &cmd);
        }
        KeyCode::Backspace => {
            app.command_input.pop();
            if app.command_input.is_empty() {
                app.mode = Mode::Normal;
            }
        }
        KeyCode::Char(c) => {
            app.command_input.push(c);
        }
        _ => {}
    }
}

/// Handle keys in Visual / Visual-Line mode.
pub fn handle_visual(app: &mut App, code: KeyCode, _modifiers: KeyModifiers) {
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
            if let Some((start, end)) = app.visual_selection_range() {
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
        KeyCode::Char('y') => {
            if let Some((start, end)) = app.visual_selection_range() {
                let text = app.tab().buffer.rope().slice(start..end).to_string();
                app.register = Some(text);
                app.set_status("Yanked selection");
            }
            app.mode = Mode::Normal;
            app.tab_mut().visual_anchor = None;
        }

        _ => {}
    }
}

/// Handle leader key sequences (<Space> + key).
fn handle_leader(app: &mut App, code: KeyCode) {
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
fn execute_command(app: &mut App, cmd: &str) {
    match cmd.trim() {
        "w" => match app.tab_mut().buffer.save() {
            Ok(_) => {
                app.set_status("Written");
                if app.sidebar_view == SidebarView::Git {
                    app.refresh_source_control();
                }
            }
            Err(e) => app.set_status(format!("Error: {}", e)),
        },
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
            let branches = app.git_list_branches();
            if branches.is_empty() {
                app.set_status("No git branches found");
            } else {
                let list: Vec<String> = branches
                    .iter()
                    .map(|b| {
                        if b.is_current {
                            format!("*{}", b.name)
                        } else {
                            b.name.clone()
                        }
                    })
                    .collect();
                app.set_status(format!("Branches: {}", list.join(" │ ")));
            }
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
            if app.terminal.visible && app.terminal_focused {
                // Already visible and focused: hide it and release focus.
                app.terminal.visible = false;
                app.terminal_focused = false;
            } else {
                app.terminal.visible = true;
                app.terminal_focused = true;
            }
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
                    app.terminal.height = h;
                    app.set_status(format!("Terminal height: {h} rows"));
                }
                Err(_) => {
                    app.set_status(format!(
                        "Current terminal height: {} rows",
                        app.terminal.height
                    ));
                }
            }
        }
        other => {
            app.set_status(format!("Unknown command: {}", other));
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
                let workdir = app
                    .git_repo
                    .as_ref()
                    .map(|r| r.workdir().to_path_buf());
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
