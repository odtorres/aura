//! Embedded terminal / command runner plugin.
//!
//! Provides a bottom-split pane that can execute shell commands and display
//! their output without requiring a full VTE terminal emulator.

use std::path::PathBuf;

/// A single line of terminal output.
#[derive(Debug, Clone)]
pub struct TerminalLine {
    /// The text content of this line.
    pub content: String,
    /// `true` for "$ command" prompt lines, `false` for command output.
    pub is_command: bool,
}

/// An embedded command-runner pane displayed at the bottom of the editor.
pub struct EmbeddedTerminal {
    /// Whether the terminal pane is visible.
    pub visible: bool,
    /// Command history output lines.
    pub output: Vec<TerminalLine>,
    /// Current command input.
    pub input: String,
    /// Scroll offset in the output.
    pub scroll: usize,
    /// Height of the terminal pane (in rows).
    pub height: u16,
    /// Working directory for spawned commands.
    cwd: PathBuf,
    /// Whether a command is currently running.
    pub running: bool,
}

impl EmbeddedTerminal {
    /// Create a new `EmbeddedTerminal` rooted at `cwd`.
    ///
    /// The pane starts hidden with no output and a default height of 10 rows.
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            visible: false,
            output: Vec::new(),
            input: String::new(),
            scroll: 0,
            height: 10,
            cwd,
            running: false,
        }
    }

    /// Toggle pane visibility.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Append a character to the command input.
    pub fn type_char(&mut self, c: char) {
        self.input.push(c);
    }

    /// Remove the last character from the command input (backspace).
    pub fn backspace(&mut self) {
        self.input.pop();
    }

    /// Execute the current command input.
    ///
    /// Adds a prompt line (`$ cmd`), runs the command via `sh -c`, appends
    /// stdout and stderr lines to the output buffer, clears the input, and
    /// auto-scrolls to the bottom.
    pub fn execute(&mut self) {
        if self.input.trim().is_empty() {
            return;
        }

        let cmd = std::mem::take(&mut self.input);

        // Echo the command as a prompt line.
        self.output.push(TerminalLine {
            content: format!("$ {}", cmd),
            is_command: true,
        });

        self.running = true;

        let result = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(&self.cwd)
            .output();

        self.running = false;

        match result {
            Ok(out) => {
                // Append stdout lines.
                let stdout = String::from_utf8_lossy(&out.stdout);
                for line in stdout.lines() {
                    self.output.push(TerminalLine {
                        content: line.to_string(),
                        is_command: false,
                    });
                }

                // Append stderr lines (if any).
                let stderr = String::from_utf8_lossy(&out.stderr);
                for line in stderr.lines() {
                    self.output.push(TerminalLine {
                        content: line.to_string(),
                        is_command: false,
                    });
                }

                // Show exit code on non-zero exit.
                if let Some(code) = out.status.code() {
                    if code != 0 {
                        self.output.push(TerminalLine {
                            content: format!("[exit {}]", code),
                            is_command: false,
                        });
                    }
                }
            }
            Err(e) => {
                self.output.push(TerminalLine {
                    content: format!("[error: {}]", e),
                    is_command: false,
                });
            }
        }

        // Auto-scroll to bottom.
        self.scroll = self.output.len().saturating_sub(1);
    }

    /// Scroll the output view up by one line.
    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    /// Scroll the output view down by one line.
    pub fn scroll_down(&mut self) {
        if self.scroll + 1 < self.output.len() {
            self.scroll = self.scroll.saturating_add(1);
        }
    }

    /// Clear all output lines and reset the scroll offset.
    pub fn clear(&mut self) {
        self.output.clear();
        self.scroll = 0;
    }

    /// Resize the pane by `delta` rows, clamped between 3 and 30.
    pub fn resize(&mut self, delta: i16) {
        let new_height = (self.height as i16).saturating_add(delta);
        self.height = new_height.clamp(3, 30) as u16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a terminal rooted at a known temp dir.
    fn make_terminal() -> EmbeddedTerminal {
        EmbeddedTerminal::new(std::env::temp_dir())
    }

    #[test]
    fn test_new_terminal() {
        let t = make_terminal();
        assert!(!t.visible, "should start hidden");
        assert!(t.output.is_empty(), "output should start empty");
        assert!(t.input.is_empty(), "input should start empty");
        assert_eq!(t.scroll, 0);
        assert_eq!(t.height, 10);
        assert!(!t.running);
    }

    #[test]
    fn test_toggle() {
        let mut t = make_terminal();
        assert!(!t.visible);
        t.toggle();
        assert!(t.visible);
        t.toggle();
        assert!(!t.visible);
    }

    #[test]
    fn test_type_and_backspace() {
        let mut t = make_terminal();
        t.type_char('l');
        t.type_char('s');
        assert_eq!(t.input, "ls");
        t.backspace();
        assert_eq!(t.input, "l");
        t.backspace();
        assert!(t.input.is_empty());
        // Extra backspace on empty input should be a no-op.
        t.backspace();
        assert!(t.input.is_empty());
    }

    #[test]
    fn test_execute_echo() {
        let mut t = make_terminal();
        t.input = "echo hello".to_string();
        t.execute();

        // Input should be cleared after execution.
        assert!(t.input.is_empty());

        // First output line should be the prompt.
        assert!(!t.output.is_empty(), "output should not be empty");
        let prompt = &t.output[0];
        assert!(prompt.is_command);
        assert_eq!(prompt.content, "$ echo hello");

        // Second line should contain the command output.
        let found = t
            .output
            .iter()
            .any(|l| !l.is_command && l.content.contains("hello"));
        assert!(found, "expected 'hello' in output; got: {:?}", t.output);
    }

    #[test]
    fn test_clear() {
        let mut t = make_terminal();
        t.input = "echo test".to_string();
        t.execute();
        assert!(!t.output.is_empty());
        t.clear();
        assert!(t.output.is_empty());
        assert_eq!(t.scroll, 0);
    }

    #[test]
    fn test_scroll() {
        let mut t = make_terminal();

        // Populate output with several lines.
        for i in 0..5 {
            t.output.push(TerminalLine {
                content: format!("line {i}"),
                is_command: false,
            });
        }
        t.scroll = 0;

        t.scroll_down();
        assert_eq!(t.scroll, 1);

        t.scroll_up();
        assert_eq!(t.scroll, 0);

        // scroll_up at 0 should stay at 0.
        t.scroll_up();
        assert_eq!(t.scroll, 0);
    }
}
