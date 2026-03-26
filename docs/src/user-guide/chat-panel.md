# Chat Panel

AURA includes an interactive AI chat panel for conversational interaction with Claude directly inside the editor. Unlike Intent mode (which targets specific code edits), the chat panel supports free-form multi-turn conversations with tool execution capabilities.

## Opening the Chat Panel

| Shortcut | Action |
|----------|--------|
| `Ctrl+J` | Toggle chat panel visibility and focus |
| `<Space>j` | Toggle chat panel from Normal mode |

The chat panel appears on the right side of the editor. When visible but unfocused, pressing the shortcut again focuses it. When focused, pressing it hides the panel.

## Sending Messages

With the chat panel focused, type your message in the input area at the bottom and press `Enter` to send. The AI response streams in real time.

You can have multi-turn conversations — the full message history is preserved for context.

## Selection Context

When you have text selected in Visual or Visual Line mode, the chat panel automatically shows a context indicator (e.g., "12 lines from main.rs"). The selected code is included as context when you send a message, so you can ask questions like:

- "What does this function do?"
- "Refactor this to use iterators"
- "Are there any bugs here?"

## Tool Execution

The chat panel supports Claude Code-style tool use. When the AI determines it needs to take an action (read a file, edit code, run a command), it displays a tool call with an approval prompt:

```
🔧 edit_buffer { start_line: 10, end_line: 15, text: "..." }
   ⏳ Approve? [Y/N]
```

| Key | Action |
|-----|--------|
| `Y` | Approve and execute the tool call |
| `N` / `Esc` | Deny the tool call |

After execution, the result is fed back to the AI for follow-up reasoning. Tool calls can chain — the AI may request multiple tools in sequence to complete a task.

### Available Tools

The chat panel has access to the same tools as the MCP server:

| Tool | Description |
|------|-------------|
| `read_buffer` | Read the current buffer content |
| `edit_buffer` | Apply edits to the buffer |
| `get_cursor_context` | Get cursor position and surrounding context |
| `get_diagnostics` | Get LSP diagnostics |
| `get_selection` | Get the current visual selection |

## Keybindings (Chat Panel Focused)

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Esc` | Unfocus chat panel (return to editor) |
| `Ctrl+J` | Close chat panel |
| `Ctrl+H` | Switch to conversation history panel |
| `Ctrl+Up` | Scroll messages up |
| `Ctrl+Down` | Scroll messages down |
| `PageUp` / `PageDown` | Scroll messages by page |
| `Up` / `Down` | Move cursor in multi-line input |
| `Left` / `Right` | Move cursor in input |
| `Home` / `End` | Jump to start/end of input |
| `Backspace` / `Delete` | Delete characters in input |

## Mouse Support

Scroll the chat panel messages with the mouse wheel when the cursor is over the panel area.

## Persistence

Chat conversations are stored in the local SQLite conversation database (`.aura/conversations.db`), so they persist across sessions. View past conversations with `Ctrl+H` (conversation history panel).
