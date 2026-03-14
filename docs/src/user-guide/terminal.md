# Embedded Terminal

AURA includes a fully functional embedded terminal powered by a real PTY (pseudo-terminal).

## Opening the Terminal

- `Ctrl+J` or `` Ctrl+` `` — toggle terminal visibility and focus
- `:term` or `:terminal` — toggle via command mode

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

## Claude Code Integration

AURA sets `AURA_MCP_PORT` in the terminal's environment, allowing Claude Code (or any MCP client) running inside the terminal to auto-discover AURA's MCP server.

A discovery file is also written to `~/.aura/mcp.json` on startup with the host, port, PID, and current file — cleaned up on exit.

See [MCP Protocol](../architecture/mcp.md) for details on the MCP integration.
