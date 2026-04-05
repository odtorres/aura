# Embedded Terminal

AURA includes a fully functional embedded terminal powered by a real PTY (pseudo-terminal).

## Opening the Terminal

- `Ctrl+T` or `` Ctrl+` `` — toggle terminal visibility and focus
- `:term` or `:terminal` — toggle via command mode

`Ctrl+T` works both ways: press it to open the terminal, press it again to close it — even when the terminal is focused.

When the terminal is focused, all keystrokes are forwarded to the shell.

## Features

- **Real shell**: Inherits your `$SHELL` (bash, zsh, fish, etc.) with full history and tab completion
- **ANSI color**: Full 256-color support via VTE state machine parsing
- **Streaming output**: Non-blocking — long-running commands stream in real time
- **Scrollback buffer**: 5000 lines of scrollback history
- **Auto-resize**: PTY dimensions sync to the actual pane size
- **Cursor rendering**: Cursor appears as a reversed cell in the terminal pane

## Keybindings (Terminal Focused)

| Key | Action |
|-----|--------|
| `Esc` | Return focus to editor |
| `Ctrl+J` / `` Ctrl+` `` | Return focus to editor |
| `Ctrl+Shift+Up` | Increase terminal height (+2 rows) |
| `Ctrl+Shift+Down` | Decrease terminal height (-2 rows) |
| `Ctrl+C` | Send interrupt (SIGINT) |
| `Ctrl+D` | Send EOF |
| `Ctrl+L` | Clear screen |
| All other keys | Forwarded to PTY |

## Resizing

- Use `Ctrl+Shift+Up/Down` while the terminal is focused
- Use `:term-height <N>` or `:th <N>` to set an exact height (5–50 rows)

## Terminal Tabs

AURA supports multiple independent terminal instances:

| Key / Command | Action |
|---------------|--------|
| `:term new` | Open a new terminal tab |
| `:term close` | Close the active terminal tab |
| `:term next` | Switch to the next tab |
| `:term prev` | Switch to the previous tab |
| `Ctrl+Shift+T` | New terminal tab (when terminal focused) |
| `Ctrl+Shift+]` | Next terminal tab (when terminal focused) |
| `Ctrl+Shift+[` | Previous terminal tab (when terminal focused) |

When multiple tabs exist, a tab bar appears in the terminal pane header. Each tab has its own PTY, scrollback, and screen buffer. Task runner commands (`:task`) are sent to the active tab.

## Shell Integration

AURA automatically detects command boundaries and exit codes using the OSC 133 shell integration protocol. Shell hooks are injected for **zsh** and **bash** automatically.

### Features

- **Exit code display**: The terminal title bar shows `[ok]`, `[exit 1]`, or `[running]` after each command
- **Command tracking**: AURA records each command's text, exit code, and prompt position
- **`:fix` command**: Sends the last failed command and its exit code to the AI chat panel for diagnosis and suggested fixes

Shell integration works automatically — no manual setup required for zsh or bash.

## Inline AI Suggestions

When the terminal is focused and idle for 2 seconds (and no command is running), AURA generates an AI-suggested shell command based on your recent commands, exit codes, and project type.

- **Ghost text** appears in gray after the cursor on the prompt line
- Press **Tab** to accept the suggestion (sends it to the PTY)
- Press any other key to dismiss
- Requires an AI backend (Anthropic API key or Claude Code CLI)

Suggestions are context-aware: they consider your recent command history, whether the last command failed, and the detected project type (Rust, Node, Go, Python).

## Shared Terminal (Collaboration)

During a collab session, the host can share their terminal screen with all connected peers:

- `:share-term` — Toggle terminal sharing (host only)
- `:view-term` — Toggle between local and shared terminal view (client only)

The shared terminal is read-only for clients, displayed with a cyan border and "Host Terminal (read-only)" title. See [Collaborative Editing](collaborative-editing.md) for more details.

## Claude Code Integration

AURA sets `AURA_MCP_PORT` in the terminal's environment, allowing Claude Code (or any MCP client) running inside the terminal to auto-discover AURA's MCP server.

A discovery file is also written to `~/.aura/mcp.json` on startup with the host, port, PID, and current file — cleaned up on exit.

See [MCP Protocol](../architecture/mcp.md) for details on the MCP integration.
