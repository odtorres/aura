//! Embedded PTY terminal pane for AURA.
//!
//! Spawns a real shell (zsh/bash) in a pseudo-terminal so that interactive
//! commands, colors, and shell features work natively. Output is parsed
//! via the `vte` crate and stored as a grid of styled cells.

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

/// Terminal color — supports default, ANSI 256, and true color (RGB).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TermColor {
    /// Default terminal color.
    #[default]
    Default,
    /// ANSI 256-color palette index (0–255).
    Indexed(u8),
    /// 24-bit true color.
    Rgb(u8, u8, u8),
}

/// A single cell in the terminal screen grid.
#[derive(Debug, Clone, Copy)]
pub struct TerminalCell {
    /// The character displayed in this cell.
    pub ch: char,
    /// Foreground color.
    pub fg: TermColor,
    /// Background color.
    pub bg: TermColor,
    /// Whether the cell is bold.
    pub bold: bool,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: TermColor::Default,
            bg: TermColor::Default,
            bold: false,
        }
    }
}

/// The virtual screen buffer that the PTY output renders into.
pub struct TerminalScreen {
    /// 2D grid of cells: `cells[row][col]`.
    pub cells: Vec<Vec<TerminalCell>>,
    /// Current cursor row.
    pub cursor_row: usize,
    /// Current cursor column.
    pub cursor_col: usize,
    /// Screen width in columns.
    pub cols: usize,
    /// Screen height in rows.
    pub rows: usize,
    /// Current SGR foreground color.
    current_fg: TermColor,
    /// Current SGR background color.
    current_bg: TermColor,
    /// Current SGR bold flag.
    current_bold: bool,
    /// Top of the scroll region (0-indexed).
    scroll_top: usize,
    /// Bottom of the scroll region (0-indexed, inclusive).
    scroll_bottom: usize,
    /// Scrollback buffer: lines that scrolled off the top.
    pub scrollback: Vec<Vec<TerminalCell>>,
    /// Maximum scrollback lines to keep.
    max_scrollback: usize,
    /// Scrollback viewing offset (0 = live view, >0 = looking at history).
    pub scroll_offset: usize,
    /// Saved cursor position (row, col) for ESC 7 / ESC 8 and CSI s / CSI u.
    saved_cursor: (usize, usize),
    /// Saved SGR state for cursor save/restore.
    saved_fg: TermColor,
    /// Saved SGR state for cursor save/restore.
    saved_bg: TermColor,
    /// Saved bold state for cursor save/restore.
    saved_bold: bool,
    /// Alternate screen buffer (saved when switching to alt screen).
    alt_screen: Option<Vec<Vec<TerminalCell>>>,
    /// Saved main-screen cursor for alt screen switch.
    alt_saved_cursor: (usize, usize),
    /// Whether auto-wrap is enabled (DECAWM).
    auto_wrap: bool,
    /// Whether we are pending a wrap (cursor is past last col).
    wrap_pending: bool,
}

impl TerminalScreen {
    /// Create a new screen with the given dimensions.
    fn new(cols: usize, rows: usize) -> Self {
        let cells = vec![vec![TerminalCell::default(); cols]; rows];
        Self {
            cells,
            cursor_row: 0,
            cursor_col: 0,
            cols,
            rows,
            current_fg: TermColor::Default,
            current_bg: TermColor::Default,
            current_bold: false,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
            scrollback: Vec::new(),
            max_scrollback: 5000,
            scroll_offset: 0,
            saved_cursor: (0, 0),
            saved_fg: TermColor::Default,
            saved_bg: TermColor::Default,
            saved_bold: false,
            alt_screen: None,
            alt_saved_cursor: (0, 0),
            auto_wrap: true,
            wrap_pending: false,
        }
    }

    /// Scroll the screen contents up by one line within the scroll region.
    fn scroll_up(&mut self) {
        if self.scroll_top < self.rows {
            // Save the top line to scrollback.
            if self.scroll_top == 0 {
                let line = self.cells[0].clone();
                self.scrollback.push(line);
                if self.scrollback.len() > self.max_scrollback {
                    self.scrollback.remove(0);
                }
            }

            let bottom = self.scroll_bottom.min(self.rows.saturating_sub(1));
            for r in self.scroll_top..bottom {
                self.cells[r] = self.cells[r + 1].clone();
            }
            self.cells[bottom] = vec![TerminalCell::default(); self.cols];
        }
    }

    /// Scroll the screen contents down by one line within the scroll region.
    fn scroll_down_region(&mut self) {
        let bottom = self.scroll_bottom.min(self.rows.saturating_sub(1));
        for r in (self.scroll_top + 1..=bottom).rev() {
            self.cells[r] = self.cells[r - 1].clone();
        }
        self.cells[self.scroll_top] = vec![TerminalCell::default(); self.cols];
    }

    /// Clear the entire screen.
    fn clear(&mut self) {
        for row in &mut self.cells {
            for cell in row.iter_mut() {
                *cell = TerminalCell::default();
            }
        }
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    /// Erase from cursor to end of line.
    fn erase_to_eol(&mut self) {
        if self.cursor_row < self.rows {
            for c in self.cursor_col..self.cols {
                self.cells[self.cursor_row][c] = TerminalCell::default();
            }
        }
    }

    /// Erase from start of line to cursor (inclusive).
    fn erase_to_bol(&mut self) {
        if self.cursor_row < self.rows {
            let end = (self.cursor_col + 1).min(self.cols);
            for c in 0..end {
                self.cells[self.cursor_row][c] = TerminalCell::default();
            }
        }
    }

    /// Erase entire line.
    fn erase_line(&mut self) {
        if self.cursor_row < self.rows {
            for cell in &mut self.cells[self.cursor_row] {
                *cell = TerminalCell::default();
            }
        }
    }

    /// Save cursor position and attributes.
    fn save_cursor(&mut self) {
        self.saved_cursor = (self.cursor_row, self.cursor_col);
        self.saved_fg = self.current_fg;
        self.saved_bg = self.current_bg;
        self.saved_bold = self.current_bold;
    }

    /// Restore cursor position and attributes.
    fn restore_cursor(&mut self) {
        let (row, col) = self.saved_cursor;
        self.cursor_row = row.min(self.rows.saturating_sub(1));
        self.cursor_col = col.min(self.cols.saturating_sub(1));
        self.current_fg = self.saved_fg;
        self.current_bg = self.saved_bg;
        self.current_bold = self.saved_bold;
        self.wrap_pending = false;
    }

    /// Enter alternate screen buffer.
    fn enter_alt_screen(&mut self) {
        if self.alt_screen.is_some() {
            return; // Already in alt screen.
        }
        self.alt_saved_cursor = (self.cursor_row, self.cursor_col);
        self.alt_screen = Some(std::mem::replace(
            &mut self.cells,
            vec![vec![TerminalCell::default(); self.cols]; self.rows],
        ));
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.scroll_top = 0;
        self.scroll_bottom = self.rows.saturating_sub(1);
    }

    /// Leave alternate screen buffer.
    fn leave_alt_screen(&mut self) {
        if let Some(saved) = self.alt_screen.take() {
            self.cells = saved;
            let (row, col) = self.alt_saved_cursor;
            self.cursor_row = row.min(self.rows.saturating_sub(1));
            self.cursor_col = col.min(self.cols.saturating_sub(1));
            self.scroll_top = 0;
            self.scroll_bottom = self.rows.saturating_sub(1);
        }
    }

    /// Resize the screen to new dimensions.
    ///
    /// Content is anchored so the cursor row stays at the same position
    /// relative to the bottom of the screen. When the screen grows, blank
    /// lines appear at the top (or scrollback lines are restored). When it
    /// shrinks, lines above the cursor are pushed into scrollback.
    fn resize(&mut self, cols: usize, rows: usize) {
        if rows == self.rows && cols == self.cols {
            return;
        }

        let old_rows = self.rows;

        if rows > old_rows {
            // Screen grew — add blank rows at the bottom, keeping cursor
            // position unchanged. This matches how real terminal emulators
            // handle resize. The visual bottom-anchoring is handled by
            // `snapshot()` instead, avoiding double-shifting artifacts.
            for old_row in &mut self.cells {
                old_row.resize(cols, TerminalCell::default());
            }
            for _ in old_rows..rows {
                self.cells.push(vec![TerminalCell::default(); cols]);
            }
        } else if rows < old_rows {
            // Screen shrank — push lines above the cursor area into scrollback.
            let removed = old_rows - rows;
            // Number of lines above cursor that can be pushed to scrollback.
            let pushable = removed.min(self.cursor_row);
            for i in 0..pushable {
                self.scrollback.push(self.cells[i].clone());
                if self.scrollback.len() > self.max_scrollback {
                    self.scrollback.remove(0);
                }
            }

            // Remove from the top.
            self.cells.drain(0..pushable);
            // If we still have too many rows, truncate from the bottom.
            self.cells.truncate(rows);
            // Pad if needed (shouldn't happen normally).
            while self.cells.len() < rows {
                self.cells.push(vec![TerminalCell::default(); cols]);
            }

            self.cursor_row = self.cursor_row.saturating_sub(pushable);

            // Resize columns for remaining rows.
            for row in &mut self.cells {
                row.resize(cols, TerminalCell::default());
            }
        } else {
            // Only columns changed — resize each row.
            for row in &mut self.cells {
                row.resize(cols, TerminalCell::default());
            }
        }

        self.cols = cols;
        self.rows = rows;
        self.cursor_row = self.cursor_row.min(rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(cols.saturating_sub(1));
        self.scroll_top = 0;
        self.scroll_bottom = rows.saturating_sub(1);
    }
}

/// VTE performer: receives parsed escape sequences and updates the screen.
struct Performer {
    screen: Arc<Mutex<TerminalScreen>>,
}

impl vte::Perform for Performer {
    fn print(&mut self, c: char) {
        let mut scr = self.screen.lock().unwrap();

        // If a wrap is pending (cursor was at the last column), wrap now.
        if scr.wrap_pending {
            scr.wrap_pending = false;
            scr.cursor_col = 0;
            if scr.cursor_row == scr.scroll_bottom {
                scr.scroll_up();
            } else if scr.cursor_row + 1 < scr.rows {
                scr.cursor_row += 1;
            }
        }

        let row = scr.cursor_row;
        let col = scr.cursor_col;
        if row < scr.rows && col < scr.cols {
            let cell = TerminalCell {
                ch: c,
                fg: scr.current_fg,
                bg: scr.current_bg,
                bold: scr.current_bold,
            };
            scr.cells[row][col] = cell;
            if scr.cursor_col + 1 >= scr.cols {
                if scr.auto_wrap {
                    // Don't wrap yet — set pending so the next print wraps.
                    scr.wrap_pending = true;
                }
                // cursor stays at last col until next print triggers wrap
            } else {
                scr.cursor_col += 1;
            }
        }
    }

    fn execute(&mut self, byte: u8) {
        let mut scr = self.screen.lock().unwrap();
        match byte {
            // Newline (LF).
            b'\n' => {
                if scr.cursor_row == scr.scroll_bottom {
                    scr.scroll_up();
                } else if scr.cursor_row + 1 < scr.rows {
                    scr.cursor_row += 1;
                }
            }
            // Carriage return.
            b'\r' => {
                scr.cursor_col = 0;
            }
            // Backspace.
            8 => {
                scr.cursor_col = scr.cursor_col.saturating_sub(1);
            }
            // Tab.
            b'\t' => {
                let next_tab = ((scr.cursor_col / 8) + 1) * 8;
                scr.cursor_col = next_tab.min(scr.cols.saturating_sub(1));
            }
            // Bell — ignore.
            7 => {}
            _ => {}
        }
    }

    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {
    }

    fn put(&mut self, _byte: u8) {}

    fn unhook(&mut self) {}

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {
        // OSC sequences (title changes, etc.) — ignore for now.
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let mut scr = self.screen.lock().unwrap();

        // Helper: extract params as a Vec<u16> for easy access.
        let pv: Vec<u16> = params.iter().map(|sub| sub[0]).collect();
        let p0 = pv.first().copied().unwrap_or(0);
        let p1 = pv.get(1).copied().unwrap_or(0);

        // Handle private mode sequences: CSI ? ... h/l
        let is_private = intermediates.first() == Some(&b'?');
        if is_private {
            match action {
                'h' => {
                    // DECSET — enable private modes.
                    for &p in &pv {
                        match p {
                            1 => {} // DECCKM — application cursor keys (handled by shell)
                            7 => scr.auto_wrap = true,
                            25 => {} // DECTCEM — show cursor (we always show)
                            1049 => scr.enter_alt_screen(),
                            2004 => {} // Bracketed paste mode — ignore
                            _ => {}
                        }
                    }
                    return;
                }
                'l' => {
                    // DECRST — disable private modes.
                    for &p in &pv {
                        match p {
                            1 => {}
                            7 => scr.auto_wrap = false,
                            25 => {}
                            1049 => scr.leave_alt_screen(),
                            2004 => {}
                            _ => {}
                        }
                    }
                    return;
                }
                _ => return,
            }
        }

        // Clear wrap pending on any cursor movement.
        scr.wrap_pending = false;

        match action {
            // CUU — Cursor Up.
            'A' => {
                let n = if p0 == 0 { 1 } else { p0 as usize };
                scr.cursor_row = scr.cursor_row.saturating_sub(n);
            }
            // CUB — Cursor Backward.
            'D' => {
                let n = if p0 == 0 { 1 } else { p0 as usize };
                scr.cursor_col = scr.cursor_col.saturating_sub(n);
            }
            // CUF — Cursor Forward.
            'C' => {
                let n = if p0 == 0 { 1 } else { p0 as usize };
                scr.cursor_col = (scr.cursor_col + n).min(scr.cols.saturating_sub(1));
            }
            // CUD — Cursor Down.
            'B' => {
                let n = if p0 == 0 { 1 } else { p0 as usize };
                scr.cursor_row = (scr.cursor_row + n).min(scr.rows.saturating_sub(1));
            }
            // CUP / HVP — Cursor Position (1-based).
            'H' | 'f' => {
                let row = if p0 == 0 { 1 } else { p0 as usize };
                let col = if p1 == 0 { 1 } else { p1 as usize };
                scr.cursor_row = (row.saturating_sub(1)).min(scr.rows.saturating_sub(1));
                scr.cursor_col = (col.saturating_sub(1)).min(scr.cols.saturating_sub(1));
            }
            // ED — Erase in Display.
            'J' => match p0 {
                0 => {
                    // Erase from cursor to end of screen.
                    scr.erase_to_eol();
                    for r in (scr.cursor_row + 1)..scr.rows {
                        for cell in &mut scr.cells[r] {
                            *cell = TerminalCell::default();
                        }
                    }
                }
                1 => {
                    // Erase from start to cursor.
                    for r in 0..scr.cursor_row {
                        for cell in &mut scr.cells[r] {
                            *cell = TerminalCell::default();
                        }
                    }
                    scr.erase_to_bol();
                }
                2 | 3 => {
                    scr.clear();
                }
                _ => {}
            },
            // EL — Erase in Line.
            'K' => match p0 {
                0 => scr.erase_to_eol(),
                1 => scr.erase_to_bol(),
                2 => scr.erase_line(),
                _ => {}
            },
            // SGR — Select Graphic Rendition.
            'm' => {
                if pv.is_empty() || (pv.len() == 1 && p0 == 0) {
                    scr.current_fg = TermColor::Default;
                    scr.current_bg = TermColor::Default;
                    scr.current_bold = false;
                    return;
                }
                let mut i = 0;
                while i < pv.len() {
                    match pv[i] {
                        0 => {
                            scr.current_fg = TermColor::Default;
                            scr.current_bg = TermColor::Default;
                            scr.current_bold = false;
                        }
                        1 => scr.current_bold = true,
                        22 => scr.current_bold = false,
                        // Standard foreground colors 30-37.
                        30..=37 => scr.current_fg = TermColor::Indexed((pv[i] - 30) as u8),
                        // Default foreground.
                        39 => scr.current_fg = TermColor::Default,
                        // Standard background colors 40-47.
                        40..=47 => scr.current_bg = TermColor::Indexed((pv[i] - 40) as u8),
                        // Default background.
                        49 => scr.current_bg = TermColor::Default,
                        // Bright foreground 90-97.
                        90..=97 => scr.current_fg = TermColor::Indexed((pv[i] - 90 + 8) as u8),
                        // Bright background 100-107.
                        100..=107 => scr.current_bg = TermColor::Indexed((pv[i] - 100 + 8) as u8),
                        // Extended color: 38;5;N (256-color) or 38;2;R;G;B (true color)
                        38 => {
                            if i + 2 < pv.len() && pv[i + 1] == 5 {
                                scr.current_fg = TermColor::Indexed(pv[i + 2] as u8);
                                i += 2;
                            } else if i + 4 < pv.len() && pv[i + 1] == 2 {
                                scr.current_fg = TermColor::Rgb(
                                    pv[i + 2] as u8,
                                    pv[i + 3] as u8,
                                    pv[i + 4] as u8,
                                );
                                i += 4;
                            }
                        }
                        48 => {
                            if i + 2 < pv.len() && pv[i + 1] == 5 {
                                scr.current_bg = TermColor::Indexed(pv[i + 2] as u8);
                                i += 2;
                            } else if i + 4 < pv.len() && pv[i + 1] == 2 {
                                scr.current_bg = TermColor::Rgb(
                                    pv[i + 2] as u8,
                                    pv[i + 3] as u8,
                                    pv[i + 4] as u8,
                                );
                                i += 4;
                            }
                        }
                        _ => {} // Ignore unsupported attributes.
                    }
                    i += 1;
                }
            }
            // IL — Insert Lines.
            'L' => {
                let n = if p0 == 0 { 1 } else { p0 as usize };
                for _ in 0..n {
                    scr.scroll_down_region();
                }
            }
            // DL — Delete Lines.
            'M' => {
                let n = if p0 == 0 { 1 } else { p0 as usize };
                for _ in 0..n {
                    scr.scroll_up();
                }
            }
            // DCH — Delete Characters.
            'P' => {
                let n = if p0 == 0 { 1 } else { p0 as usize };
                let row = scr.cursor_row;
                let col = scr.cursor_col;
                if row < scr.rows {
                    let end = (col + n).min(scr.cols);
                    for c in col..scr.cols {
                        scr.cells[row][c] = if c + n < scr.cols {
                            scr.cells[row][c + n]
                        } else {
                            TerminalCell::default()
                        };
                    }
                    let _ = end; // suppress warning
                }
            }
            // ICH — Insert Characters.
            '@' => {
                let n = if p0 == 0 { 1 } else { p0 as usize };
                let row = scr.cursor_row;
                let col = scr.cursor_col;
                if row < scr.rows {
                    for c in (col..scr.cols).rev() {
                        if c + n < scr.cols {
                            scr.cells[row][c + n] = scr.cells[row][c];
                        }
                    }
                    let end = (col + n).min(scr.cols);
                    for c in col..end {
                        scr.cells[row][c] = TerminalCell::default();
                    }
                }
            }
            // DECSTBM — Set Scrolling Region.
            'r' => {
                let top = if p0 == 0 { 1 } else { p0 as usize };
                let bot = if p1 == 0 { scr.rows } else { p1 as usize };
                scr.scroll_top = top.saturating_sub(1);
                scr.scroll_bottom = bot.saturating_sub(1).min(scr.rows.saturating_sub(1));
                scr.cursor_row = 0;
                scr.cursor_col = 0;
            }
            // CHA — Cursor Horizontal Absolute (1-based).
            'G' => {
                let col = if p0 == 0 { 1 } else { p0 as usize };
                scr.cursor_col = col.saturating_sub(1).min(scr.cols.saturating_sub(1));
            }
            // VPA — Vertical Position Absolute (1-based).
            'd' => {
                let row = if p0 == 0 { 1 } else { p0 as usize };
                scr.cursor_row = row.saturating_sub(1).min(scr.rows.saturating_sub(1));
            }
            // ECH — Erase Characters.
            'X' => {
                let n = if p0 == 0 { 1 } else { p0 as usize };
                let row = scr.cursor_row;
                if row < scr.rows {
                    let end = (scr.cursor_col + n).min(scr.cols);
                    for c in scr.cursor_col..end {
                        scr.cells[row][c] = TerminalCell::default();
                    }
                }
            }
            // CSI s — Save cursor position.
            's' => {
                scr.save_cursor();
            }
            // CSI u — Restore cursor position.
            'u' => {
                scr.restore_cursor();
            }
            // CSI n — Device Status Report (ignored — can't write back to PTY from here).
            'n' => {}
            _ => {} // Ignore unsupported sequences.
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        let mut scr = self.screen.lock().unwrap();
        match byte {
            // RI (Reverse Index) = ESC M.
            b'M' => {
                if scr.cursor_row == scr.scroll_top {
                    scr.scroll_down_region();
                } else {
                    scr.cursor_row = scr.cursor_row.saturating_sub(1);
                }
            }
            // DECSC — Save cursor = ESC 7.
            b'7' => {
                scr.save_cursor();
            }
            // DECRC — Restore cursor = ESC 8.
            b'8' => {
                scr.restore_cursor();
            }
            // ESC c — Full Reset (RIS).
            b'c' => {
                scr.clear();
                scr.current_fg = TermColor::Default;
                scr.current_bg = TermColor::Default;
                scr.current_bold = false;
                scr.auto_wrap = true;
                scr.wrap_pending = false;
                scr.scroll_top = 0;
                scr.scroll_bottom = scr.rows.saturating_sub(1);
            }
            // ESC D — Index (move cursor down, scroll if at bottom).
            b'D' => {
                if scr.cursor_row == scr.scroll_bottom {
                    scr.scroll_up();
                } else if scr.cursor_row + 1 < scr.rows {
                    scr.cursor_row += 1;
                }
            }
            // ESC E — Next Line (CR + LF).
            b'E' => {
                scr.cursor_col = 0;
                if scr.cursor_row == scr.scroll_bottom {
                    scr.scroll_up();
                } else if scr.cursor_row + 1 < scr.rows {
                    scr.cursor_row += 1;
                }
            }
            _ => {}
        }
    }
}

/// An embedded PTY terminal pane displayed at the bottom of the editor.
///
/// Spawns a real shell process in a pseudo-terminal, so interactive commands,
/// colors, and shell features (tab completion, history) all work.
pub struct EmbeddedTerminal {
    /// Whether the terminal pane is visible.
    pub visible: bool,
    /// Height of the terminal pane (in rows).
    pub height: u16,
    /// Shared screen buffer updated by the reader thread.
    screen: Arc<Mutex<TerminalScreen>>,
    /// Writer end of the PTY master — used to send keystrokes to the shell.
    writer: Option<Box<dyn Write + Send>>,
    /// The PTY master handle — kept alive to allow resize operations.
    master: Option<Box<dyn MasterPty + Send>>,
    /// Whether the PTY is alive.
    pub running: bool,
}

impl EmbeddedTerminal {
    /// Create a new PTY-backed terminal with the given working directory.
    ///
    /// Spawns a shell (prefers `$SHELL`, falls back to `/bin/zsh` then `/bin/bash`)
    /// in a pseudo-terminal. A background thread reads output and feeds it to
    /// the VTE parser, which updates the shared screen buffer.
    ///
    /// Extra environment variables can be injected via `env_vars` — these are
    /// set in the spawned shell so child processes (e.g. Claude Code) inherit them.
    pub fn new(cwd: PathBuf) -> Self {
        Self::with_env(cwd, &[])
    }

    /// Create a new PTY-backed terminal with extra environment variables.
    pub fn with_env(cwd: PathBuf, env_vars: &[(&str, &str)]) -> Self {
        let cols = 80u16;
        let rows = 10u16;

        let screen = Arc::new(Mutex::new(TerminalScreen::new(
            cols as usize,
            rows as usize,
        )));

        let pty_system = native_pty_system();
        let pair = match pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("Failed to open PTY: {}", e);
                return Self {
                    visible: false,
                    height: rows,
                    screen,
                    writer: None,
                    master: None,
                    running: false,
                };
            }
        };

        // Determine the shell to use.
        let shell = std::env::var("SHELL").unwrap_or_else(|_| {
            if std::path::Path::new("/bin/zsh").exists() {
                "/bin/zsh".to_string()
            } else {
                "/bin/bash".to_string()
            }
        });

        let mut cmd = CommandBuilder::new(&shell);
        cmd.cwd(&cwd);
        // Ensure interactive shell with login.
        cmd.arg("-l");

        // Set TERM so the shell/programs know what escape sequences to use.
        cmd.env("TERM", "xterm-256color");

        // Inject additional environment variables (e.g., AURA_MCP_PORT).
        for (key, value) in env_vars {
            cmd.env(key, value);
        }

        let _child = match pair.slave.spawn_command(cmd) {
            Ok(child) => child,
            Err(e) => {
                tracing::error!("Failed to spawn shell: {}", e);
                return Self {
                    visible: false,
                    height: rows,
                    screen,
                    writer: None,
                    master: None,
                    running: false,
                };
            }
        };

        // Get the writer (master PTY write end).
        let writer = match pair.master.take_writer() {
            Ok(w) => w,
            Err(e) => {
                tracing::error!("Failed to get PTY writer: {}", e);
                return Self {
                    visible: false,
                    height: rows,
                    screen,
                    writer: None,
                    master: None,
                    running: false,
                };
            }
        };

        // Get the reader (master PTY read end) and spawn a background thread.
        let reader = match pair.master.try_clone_reader() {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Failed to get PTY reader: {}", e);
                return Self {
                    visible: false,
                    height: rows,
                    screen,
                    writer: Some(writer),
                    master: None,
                    running: false,
                };
            }
        };

        let screen_clone = Arc::clone(&screen);
        thread::spawn(move || {
            Self::reader_thread(reader, screen_clone);
        });

        Self {
            visible: false,
            height: rows,
            screen,
            writer: Some(writer),
            master: Some(pair.master),
            running: true,
        }
    }

    /// Background thread: reads bytes from the PTY and feeds them to the VTE parser.
    fn reader_thread(mut reader: Box<dyn Read + Send>, screen: Arc<Mutex<TerminalScreen>>) {
        let performer = Performer {
            screen: Arc::clone(&screen),
        };
        let mut parser = vte::Parser::new();
        let mut stash = performer;
        let mut buf = [0u8; 4096];

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF — shell exited.
                Ok(n) => {
                    for &byte in &buf[..n] {
                        parser.advance(&mut stash, byte);
                    }
                    // Reset scroll offset to live view when new output arrives.
                    if let Ok(mut scr) = screen.lock() {
                        scr.scroll_offset = 0;
                    }
                }
                Err(_) => break,
            }
        }
    }

    /// Toggle pane visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Send a single character to the PTY (as the user types).
    pub fn send_char(&mut self, c: char) {
        if let Some(ref mut writer) = self.writer {
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            let _ = writer.write_all(s.as_bytes());
            let _ = writer.flush();
        }
    }

    /// Send raw bytes to the PTY (for special keys).
    pub fn send_bytes(&mut self, bytes: &[u8]) {
        if let Some(ref mut writer) = self.writer {
            let _ = writer.write_all(bytes);
            let _ = writer.flush();
        }
    }

    /// Send Enter key to the PTY.
    pub fn send_enter(&mut self) {
        self.send_bytes(b"\r");
    }

    /// Send backspace to the PTY.
    pub fn send_backspace(&mut self) {
        self.send_bytes(b"\x7f");
    }

    /// Send Ctrl+C to the PTY.
    pub fn send_ctrl_c(&mut self) {
        self.send_bytes(b"\x03");
    }

    /// Send Ctrl+D to the PTY.
    pub fn send_ctrl_d(&mut self) {
        self.send_bytes(b"\x04");
    }

    /// Send Ctrl+L (clear) to the PTY.
    pub fn send_ctrl_l(&mut self) {
        self.send_bytes(b"\x0c");
    }

    /// Send an arrow key escape sequence to the PTY.
    pub fn send_arrow_up(&mut self) {
        self.send_bytes(b"\x1b[A");
    }

    /// Send an arrow key escape sequence to the PTY.
    pub fn send_arrow_down(&mut self) {
        self.send_bytes(b"\x1b[B");
    }

    /// Send an arrow key escape sequence to the PTY.
    pub fn send_arrow_right(&mut self) {
        self.send_bytes(b"\x1b[C");
    }

    /// Send an arrow key escape sequence to the PTY.
    pub fn send_arrow_left(&mut self) {
        self.send_bytes(b"\x1b[D");
    }

    /// Send a Tab key to the PTY.
    pub fn send_tab(&mut self) {
        self.send_bytes(b"\t");
    }

    /// Scroll the scrollback buffer view up.
    pub fn scroll_up(&mut self) {
        if let Ok(mut scr) = self.screen.lock() {
            if scr.scroll_offset < scr.scrollback.len() {
                scr.scroll_offset += 1;
            }
        }
    }

    /// Scroll the scrollback buffer view down (towards live view).
    pub fn scroll_down(&mut self) {
        if let Ok(mut scr) = self.screen.lock() {
            scr.scroll_offset = scr.scroll_offset.saturating_sub(1);
        }
    }

    /// Clear the terminal screen by sending Ctrl+L.
    pub fn clear(&mut self) {
        self.send_ctrl_l();
    }

    /// Resize the PTY and screen buffer to match the visible inner area.
    ///
    /// Note: this does NOT change `self.height` (the layout pane height including
    /// borders). It only updates the internal screen buffer and PTY dimensions.
    /// Only sends a resize to the PTY if the dimensions actually changed,
    /// to avoid unnecessary SIGWINCH signals on every frame.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let changed = if let Ok(scr) = self.screen.lock() {
            scr.cols != cols as usize || scr.rows != rows as usize
        } else {
            false
        };
        if changed {
            if let Ok(mut scr) = self.screen.lock() {
                scr.resize(cols as usize, rows as usize);
            }
            // Notify the PTY of the new size so the shell adjusts.
            if let Some(ref master) = self.master {
                let _ = master.resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                });
            }
        }
    }

    /// Get a snapshot of the current screen for rendering.
    ///
    /// Returns the visible rows of cells. If the user has scrolled back,
    /// scrollback lines replace screen lines at the top.
    /// Get a snapshot of the current screen for rendering.
    ///
    /// Returns a bottom-anchored view: empty rows below the cursor are
    /// removed and replaced by scrollback or blank lines at the top, so
    /// the active content sits at the bottom of the pane (like VS Code).
    /// Also returns the adjusted cursor row for rendering.
    pub fn snapshot(&self) -> (Vec<Vec<TerminalCell>>, usize, usize) {
        let scr = self.screen.lock().unwrap();
        let offset = scr.scroll_offset;

        if offset > 0 {
            // Scrollback view — show historical lines.
            let total_scrollback = scr.scrollback.len();
            let start = total_scrollback.saturating_sub(offset);
            let mut result = Vec::with_capacity(scr.rows);

            for line in &scr.scrollback[start..] {
                if result.len() >= scr.rows {
                    break;
                }
                let mut padded = line.clone();
                padded.resize(scr.cols, TerminalCell::default());
                result.push(padded);
            }

            let remaining = scr.rows.saturating_sub(result.len());
            for r in 0..remaining {
                if r < scr.cells.len() {
                    result.push(scr.cells[r].clone());
                }
            }

            return (result, scr.cursor_row, scr.cursor_col);
        }

        // Live view — bottom-anchor content.
        // If the alternate screen is active (TUI app), show raw buffer.
        if scr.alt_screen.is_some() {
            return (scr.cells.clone(), scr.cursor_row, scr.cursor_col);
        }

        // Bottom-anchor: find the last row with content or the cursor row,
        // then shift everything so that active content sits at the bottom
        // of the pane (like VS Code's integrated terminal).
        let active_bottom = {
            let mut last_content = scr.cursor_row;
            for (r, row) in scr.cells.iter().enumerate() {
                if row.iter().any(|c| c.ch != ' ') {
                    last_content = last_content.max(r);
                }
            }
            last_content
        };

        let active_height = active_bottom + 1;
        if active_height >= scr.rows {
            // Content fills or exceeds the screen — no shifting needed.
            return (scr.cells.clone(), scr.cursor_row, scr.cursor_col);
        }

        let shift = scr.rows - active_height;
        let mut result = Vec::with_capacity(scr.rows);

        // Fill the top with scrollback lines (if any), otherwise blank.
        let sb_len = scr.scrollback.len();
        let sb_start = sb_len.saturating_sub(shift);
        for i in 0..shift {
            if sb_start + i < sb_len {
                let mut line = scr.scrollback[sb_start + i].clone();
                line.resize(scr.cols, TerminalCell::default());
                result.push(line);
            } else {
                result.push(vec![TerminalCell::default(); scr.cols]);
            }
        }

        // Append the active screen rows.
        for r in 0..active_height {
            result.push(scr.cells[r].clone());
        }

        let adjusted_cursor_row = scr.cursor_row + shift;
        (result, adjusted_cursor_row, scr.cursor_col)
    }

    /// Get the current cursor position (row, col) for rendering.
    /// Note: prefer the cursor position returned by `snapshot()` as it
    /// accounts for bottom-anchoring.
    pub fn cursor_position(&self) -> (usize, usize) {
        let scr = self.screen.lock().unwrap();
        (scr.cursor_row, scr.cursor_col)
    }

    /// Get the current scroll offset.
    pub fn scroll_offset(&self) -> usize {
        self.screen.lock().unwrap().scroll_offset
    }

    // Keep these stubs for compatibility — old code called them.
    // They are no-ops since we now send raw chars to the PTY.

    /// Deprecated: use `send_char` instead.
    pub fn type_char(&mut self, c: char) {
        self.send_char(c);
    }

    /// Deprecated: use `send_backspace` instead.
    pub fn backspace(&mut self) {
        self.send_backspace();
    }

    /// Deprecated: use `send_enter` instead.
    pub fn execute(&mut self) {
        self.send_enter();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screen_new() {
        let scr = TerminalScreen::new(80, 24);
        assert_eq!(scr.cols, 80);
        assert_eq!(scr.rows, 24);
        assert_eq!(scr.cursor_row, 0);
        assert_eq!(scr.cursor_col, 0);
        assert_eq!(scr.cells.len(), 24);
        assert_eq!(scr.cells[0].len(), 80);
    }

    #[test]
    fn test_screen_clear() {
        let mut scr = TerminalScreen::new(10, 5);
        scr.cells[0][0].ch = 'A';
        scr.cursor_row = 3;
        scr.cursor_col = 5;
        scr.clear();
        assert_eq!(scr.cells[0][0].ch, ' ');
        assert_eq!(scr.cursor_row, 0);
        assert_eq!(scr.cursor_col, 0);
    }

    #[test]
    fn test_screen_scroll_up() {
        let mut scr = TerminalScreen::new(5, 3);
        scr.cells[0][0].ch = 'A';
        scr.cells[1][0].ch = 'B';
        scr.cells[2][0].ch = 'C';
        scr.scroll_up();
        // Row 0 should now be what was row 1.
        assert_eq!(scr.cells[0][0].ch, 'B');
        assert_eq!(scr.cells[1][0].ch, 'C');
        assert_eq!(scr.cells[2][0].ch, ' ');
        // Scrollback should contain the old row 0.
        assert_eq!(scr.scrollback.len(), 1);
        assert_eq!(scr.scrollback[0][0].ch, 'A');
    }

    #[test]
    fn test_screen_resize() {
        let mut scr = TerminalScreen::new(10, 5);
        scr.cells[0][0].ch = 'X';
        scr.cells[3][0].ch = 'Y';
        scr.cursor_row = 4;
        scr.cursor_col = 9;
        scr.resize(20, 3);
        assert_eq!(scr.cols, 20);
        assert_eq!(scr.rows, 3);
        // Rows 0 and 1 pushed to scrollback to keep cursor at bottom.
        assert_eq!(scr.scrollback.len(), 2);
        assert_eq!(scr.scrollback[0][0].ch, 'X');
        // Row 3 (with 'Y') is now at row 1 in the resized screen.
        assert_eq!(scr.cells[1][0].ch, 'Y');
        // Cursor stays relative: was row 4 of 5, now row 2 of 3.
        assert_eq!(scr.cursor_row, 2);
        assert_eq!(scr.cursor_col, 9);
    }

    #[test]
    fn test_terminal_new() {
        let t = EmbeddedTerminal::new(std::env::temp_dir());
        assert!(!t.visible);
        assert!(t.running);
    }

    #[test]
    fn test_terminal_toggle() {
        let mut t = EmbeddedTerminal::new(std::env::temp_dir());
        assert!(!t.visible);
        t.toggle();
        assert!(t.visible);
        t.toggle();
        assert!(!t.visible);
    }
}
