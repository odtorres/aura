# TUI Crate (`aura-tui`)

The TUI crate handles everything the user sees and interacts with: rendering, input handling, and integration with external systems.

## Modules

| Module | Purpose |
|--------|---------|
| `app` | Main `App` struct, `Mode` enum, event loop |
| `render` | Ratatui drawing: buffer, gutter, status bar, panels |
| `input` | Key handling dispatched by mode |
| `config` | `aura.toml` loading, `AuraConfig`, theme engine |
| `highlight` | Tree-sitter syntax highlighting |
| `lsp` | LSP client: diagnostics, hover, go-to-def, code actions |
| `git` | Git integration via gitoxide: diff markers, blame, branches |
| `embedded_terminal` | PTY terminal: portable-pty + VTE state machine |
| `file_tree` | Directory sidebar with keyboard navigation |
| `file_picker` | Fuzzy file finder overlay |
| `mcp_server` | MCP server exposing editor tools/resources |
| `mcp_client` | MCP client connecting to external servers |
| `plugin` | Plugin trait and `PluginManager` |
| `speculative` | Background AI analysis and ghost suggestions |
| `semantic_index` | Semantic indexer for dependency graph updates |
| `tab` | Tab/multi-buffer management |
| `diff_view` | Side-by-side diff rendering for AI proposals |
| `conversation_history` | Conversation history panel |
| `source_control` | Source control sidebar (git staging, commits) |
| `chat_panel` | Interactive AI chat panel with multi-turn conversations |
| `chat_tools` | Tool execution engine for chat panel (read, edit, diagnostics) |
| `session` | Session persistence — save/restore tabs, cursors, UI state |

## App State Machine

The `App` struct is the central state container. Key fields:

- `buffer: Buffer` — the text being edited
- `cursor: Cursor` — current cursor position
- `mode: Mode` — current editing mode
- `proposal: Option<Proposal>` — pending AI proposal (in Review mode)
- `terminal: EmbeddedTerminal` — PTY terminal state
- `file_tree: FileTree` — sidebar state
- `file_picker: FilePicker` — fuzzy finder state
- `tab_manager: TabManager` — multi-buffer tab management
- `source_control: SourceControlPanel` — git staging panel
- `conversation_history: ConversationHistoryPanel` — history panel
- `ai_client` / `conversation_store` — AI integration and history
- `mcp_server` / `mcp_client` — MCP server and client instances
- `speculative_engine` — background AI analysis engine
- `git_repo` — gitoxide repository handle
- `plugin_manager` — plugin lifecycle management
- `config` / `theme` — configuration and theme state

### Mode Enum

```rust
pub enum Mode {
    Normal,
    Insert,
    Command,
    Visual,
    VisualLine,
    Intent,
    Review,
    Diff,
}
```

### Event Loop

The main loop (in `App::run`) uses crossterm's event polling with a short timeout to balance responsiveness with CPU usage:

1. Poll for crossterm events (keyboard, mouse, resize)
2. Check for async events (AI streaming, LSP, MCP)
3. Dispatch key events to the mode-specific handler
4. Re-render the frame via ratatui

## Rendering Pipeline

The `render` module draws the UI using ratatui's immediate-mode rendering:

1. **Buffer area**: Text content with syntax highlighting, authorship gutter, git markers
2. **Line numbers**: In the gutter column
3. **Status bar**: Mode indicator, filename, cursor position, git branch, agent count
4. **Command bar**: Command input (in Command mode) or intent input (in Intent mode)
5. **Panels**: File tree (left), terminal (bottom), conversation panel (floating)
6. **Overlays**: File picker, hover info, ghost suggestions, diagnostic popups

## Input Handling

`input.rs` dispatches key events based on the current state:

1. Check for focused panels (terminal, file tree, file picker, conversation)
2. If no panel is focused, check for leader key sequences
3. Dispatch to mode-specific handler: `handle_normal`, `handle_insert`, `handle_command`, `handle_visual`, `handle_intent`, `handle_review`

## Configuration & Themes

`config.rs` handles `aura.toml` parsing via serde. The theme engine supports:

- 3 built-in themes: dark, light, monokai
- Custom themes via `[theme_definitions.<name>]` in config
- Colors as named values or `#RRGGBB` hex
- 30+ configurable color slots (syntax, UI, git, diagnostics, authorship)

## Plugin System

The `Plugin` trait defines the extension interface. `PluginManager` handles registration and lifecycle. Plugins can register new intents, modes, and UI panels.

## API Reference

See the [rustdoc for `aura-tui`](/api/aura_tui/).
