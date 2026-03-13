//! Keyboard input handling for each editing mode.

use crate::app::{App, Mode};
use aura_core::AuthorId;
use crossterm::event::{KeyCode, KeyModifiers};

/// Handle keys in Normal mode.
pub fn handle_normal(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    // Leader key sequences: <Space> followed by another key.
    if app.leader_pending {
        app.leader_pending = false;
        handle_leader(app, code);
        return;
    }

    // g-prefix sequences: gg → top, gd → definition.
    if app.g_pending {
        app.g_pending = false;
        match code {
            KeyCode::Char('g') => {
                app.cursor.row = 0;
                app.cursor.col = 0;
                return;
            }
            KeyCode::Char('d') => {
                app.lsp_goto_definition();
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
            app.cursor.col += 1;
            app.clamp_cursor();
            app.mode = Mode::Insert;
        }
        KeyCode::Char('o') => {
            // Open line below.
            let line_end = app
                .buffer
                .line(app.cursor.row)
                .map(|l| l.len_chars().saturating_sub(1))
                .unwrap_or(0);
            let pos = app
                .buffer
                .cursor_to_char_idx(&aura_core::Cursor::new(app.cursor.row, line_end));
            app.buffer.insert(pos, "\n", AuthorId::human());
            app.cursor.row += 1;
            app.cursor.col = 0;
            app.mode = Mode::Insert;
            app.mark_highlights_dirty();
        }
        KeyCode::Char(':') => {
            app.mode = Mode::Command;
            app.command_input.clear();
        }
        KeyCode::Char('v') => {
            app.mode = Mode::Visual;
            app.visual_anchor = Some(app.cursor);
        }
        KeyCode::Char('V') => {
            app.mode = Mode::VisualLine;
            app.visual_anchor = Some(app.cursor);
        }

        // Navigation (clears hover popup on move)
        KeyCode::Char('h') | KeyCode::Left => {
            app.cursor.col = app.cursor.col.saturating_sub(1);
            app.hover_info = None;
        }
        KeyCode::Char('l') | KeyCode::Right => {
            app.cursor.col += 1;
            app.clamp_cursor();
            app.hover_info = None;
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.cursor.row = app.cursor.row.saturating_sub(1);
            app.clamp_cursor();
            app.hover_info = None;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.cursor.row += 1;
            app.clamp_cursor();
            app.hover_info = None;
        }
        KeyCode::Char('0') => {
            app.cursor.col = 0;
        }
        KeyCode::Char('$') => {
            if let Some(line) = app.buffer.line(app.cursor.row) {
                app.cursor.col = line.len_chars().saturating_sub(1);
            }
        }
        KeyCode::Char('g') => {
            // gg → go to top, gd → go to definition.
            app.g_pending = true;
        }
        KeyCode::Char('G') => {
            app.cursor.row = app.buffer.line_count().saturating_sub(1);
            app.cursor.col = 0;
        }

        // Word movement
        KeyCode::Char('w') => {
            let pos = app.buffer.cursor_to_char_idx(&app.cursor);
            let new_pos = app.buffer.next_word_start(pos);
            app.cursor = app.buffer.char_idx_to_cursor(new_pos);
            app.clamp_cursor();
        }
        KeyCode::Char('b') => {
            let pos = app.buffer.cursor_to_char_idx(&app.cursor);
            let new_pos = app.buffer.prev_word_start(pos);
            app.cursor = app.buffer.char_idx_to_cursor(new_pos);
        }
        KeyCode::Char('e') => {
            let pos = app.buffer.cursor_to_char_idx(&app.cursor);
            let new_pos = app.buffer.word_end(pos);
            app.cursor = app.buffer.char_idx_to_cursor(new_pos);
            app.clamp_cursor();
        }

        // Editing
        KeyCode::Char('x') => {
            // Delete character under cursor.
            let pos = app.buffer.cursor_to_char_idx(&app.cursor);
            if pos < app.buffer.len_chars() {
                app.buffer.delete(pos, pos + 1, AuthorId::human());
                app.clamp_cursor();
                app.mark_highlights_dirty();
            }
        }
        KeyCode::Char('u') => {
            if let Some(author) = app.buffer.undo() {
                app.set_status(format!("Undid edit by {}", author));
                app.clamp_cursor();
                app.mark_highlights_dirty();
            } else {
                app.set_status("Nothing to undo");
            }
        }
        // dd — delete current line
        KeyCode::Char('d') => {
            if let Some(text) = app.buffer.delete_line(app.cursor.row, AuthorId::human()) {
                app.register = Some(text);
                app.clamp_cursor();
                app.mark_highlights_dirty();
            }
        }
        // yy — yank current line
        KeyCode::Char('y') => {
            if let Some(text) = app.buffer.line_text(app.cursor.row) {
                app.register = Some(text);
                app.set_status("Yanked line");
            }
        }
        // p — paste register after current line
        KeyCode::Char('p') => {
            if let Some(text) = app.register.clone() {
                if text.ends_with('\n') {
                    // Line-wise paste: insert on the next line.
                    let line_count = app.buffer.line_count();
                    let insert_line = (app.cursor.row + 1).min(line_count);
                    let pos = app
                        .buffer
                        .cursor_to_char_idx(&aura_core::Cursor::new(insert_line, 0));
                    app.buffer.insert(pos, &text, AuthorId::human());
                    app.cursor.row += 1;
                    app.cursor.col = 0;
                } else {
                    // Character-wise paste: insert after cursor.
                    let pos = app.buffer.cursor_to_char_idx(&app.cursor) + 1;
                    app.buffer.insert(pos, &text, AuthorId::human());
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

        // ]d — next diagnostic, [d — previous diagnostic
        KeyCode::Char(']') => {
            app.next_diagnostic();
        }
        KeyCode::Char('[') => {
            app.prev_diagnostic();
        }

        // Save shortcut
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            match app.buffer.save() {
                Ok(_) => app.set_status("Saved"),
                Err(e) => app.set_status(format!("Error saving: {}", e)),
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
            app.cursor.col = app.cursor.col.saturating_sub(1);
            app.clamp_cursor();
        }
        KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
            let new_pos = app.buffer.insert_char(&app.cursor, c, AuthorId::human());
            app.cursor = app.buffer.char_idx_to_cursor(new_pos);
            app.mark_highlights_dirty();
        }
        KeyCode::Enter => {
            let new_pos = app.buffer.insert_char(&app.cursor, '\n', AuthorId::human());
            app.cursor = app.buffer.char_idx_to_cursor(new_pos);
            app.mark_highlights_dirty();
        }
        KeyCode::Backspace => {
            if let Some(new_pos) = app.buffer.backspace(&app.cursor, AuthorId::human()) {
                app.cursor = app.buffer.char_idx_to_cursor(new_pos);
                app.mark_highlights_dirty();
            }
        }
        KeyCode::Left => {
            app.cursor.col = app.cursor.col.saturating_sub(1);
        }
        KeyCode::Right => {
            app.cursor.col += 1;
            app.clamp_cursor();
        }
        KeyCode::Up => {
            app.cursor.row = app.cursor.row.saturating_sub(1);
            app.clamp_cursor();
        }
        KeyCode::Down => {
            app.cursor.row += 1;
            app.clamp_cursor();
        }
        // Ctrl+S to save even in insert mode.
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            match app.buffer.save() {
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
            app.visual_anchor = None;
        }

        // Navigation (same as normal mode)
        KeyCode::Char('h') | KeyCode::Left => {
            app.cursor.col = app.cursor.col.saturating_sub(1);
        }
        KeyCode::Char('l') | KeyCode::Right => {
            app.cursor.col += 1;
            app.clamp_cursor();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.cursor.row = app.cursor.row.saturating_sub(1);
            app.clamp_cursor();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.cursor.row += 1;
            app.clamp_cursor();
        }
        KeyCode::Char('w') => {
            let pos = app.buffer.cursor_to_char_idx(&app.cursor);
            let new_pos = app.buffer.next_word_start(pos);
            app.cursor = app.buffer.char_idx_to_cursor(new_pos);
            app.clamp_cursor();
        }
        KeyCode::Char('b') => {
            let pos = app.buffer.cursor_to_char_idx(&app.cursor);
            let new_pos = app.buffer.prev_word_start(pos);
            app.cursor = app.buffer.char_idx_to_cursor(new_pos);
        }
        KeyCode::Char('0') => app.cursor.col = 0,
        KeyCode::Char('$') => {
            if let Some(line) = app.buffer.line(app.cursor.row) {
                app.cursor.col = line.len_chars().saturating_sub(1);
            }
        }
        KeyCode::Char('G') => {
            app.cursor.row = app.buffer.line_count().saturating_sub(1);
            app.cursor.col = 0;
        }
        KeyCode::Char('g') => {
            app.cursor.row = 0;
            app.cursor.col = 0;
        }

        // Delete selection
        KeyCode::Char('d') | KeyCode::Char('x') => {
            if let Some((start, end)) = app.visual_selection_range() {
                let text = app.buffer.rope().slice(start..end).to_string();
                app.register = Some(text);
                app.buffer.delete(start, end, AuthorId::human());
                app.cursor = app.buffer.char_idx_to_cursor(start);
                app.clamp_cursor();
                app.mark_highlights_dirty();
            }
            app.mode = Mode::Normal;
            app.visual_anchor = None;
        }
        // Yank selection
        KeyCode::Char('y') => {
            if let Some((start, end)) = app.visual_selection_range() {
                let text = app.buffer.rope().slice(start..end).to_string();
                app.register = Some(text);
                app.set_status("Yanked selection");
            }
            app.mode = Mode::Normal;
            app.visual_anchor = None;
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
            if app.buffer.undo_by_author(&ai_id) {
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
        // <leader>s — show semantic info for symbol at cursor
        KeyCode::Char('s') => {
            app.update_semantic_context();
            if let Some(info) = &app.semantic_info {
                app.set_status(info.replace('\n', " │ "));
            } else {
                app.set_status("No semantic info at cursor");
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
            app.mode = Mode::Normal;
            app.intent_input.clear();
            app.set_status("");
        }
        KeyCode::Enter => {
            let intent = app.intent_input.clone();
            app.intent_input.clear();
            if intent.is_empty() {
                app.mode = Mode::Normal;
            } else {
                app.send_intent(&intent);
            }
        }
        KeyCode::Backspace => {
            app.intent_input.pop();
            if app.intent_input.is_empty() {
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
        _ => {}
    }
}

/// Execute a command-mode command.
fn execute_command(app: &mut App, cmd: &str) {
    match cmd.trim() {
        "w" => match app.buffer.save() {
            Ok(_) => app.set_status("Written"),
            Err(e) => app.set_status(format!("Error: {}", e)),
        },
        "q" => {
            if app.buffer.is_modified() {
                app.set_status("Unsaved changes! Use :q! to force quit or :wq to save and quit");
            } else {
                app.should_quit = true;
            }
        }
        "q!" => {
            app.should_quit = true;
        }
        "wq" => match app.buffer.save() {
            Ok(_) => app.should_quit = true,
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
        other => {
            app.set_status(format!("Unknown command: {}", other));
        }
    }
}
