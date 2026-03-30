# Configuration

AURA is configured via `aura.toml`. It searches for configuration in:

1. `./aura.toml` (current directory)
2. `~/.config/aura/aura.toml` (user config)

If no config file is found, sensible defaults are used.

You can also change common settings interactively with the **Settings modal** (`Ctrl+,` or `:settings`). Changes apply immediately.

## Full Example

```toml
theme = "dark"

[editor]
line_numbers = true
show_authorship = true
show_minimap = true
tab_width = 4
spaces_for_tabs = true
scroll_margin = 5
auto_save_seconds = 0

[ai]
model = "claude-sonnet-4-20250514"
max_tokens = 4096
aggressiveness = "moderate"
idle_threshold_ms = 3000

[keybindings]
leader = "Space"

[keybindings.leader_map]
e = "explain"
f = "fix"
t = "test"

[keybindings.normal_map]
# Custom normal mode key → action mappings

[conversations]
max_message_age_days = 90
max_messages_per_conversation = 200
max_conversations = 500
keep_recent_messages = 10
auto_compact = true
max_context_messages = 40

[collab]
display_name = "alice"
default_port = 0
use_tls = false
bind_address = "127.0.0.1"
require_auth = false

[debuggers.codelldb]
command = "codelldb"
args = ["--port", "0"]
extensions = ["rs", "c", "cpp"]

[mcp_servers]
# External MCP server connections
# [mcp_servers.my-server]
# url = "ws://localhost:8080"
```

## Editor Settings

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `line_numbers` | bool | `true` | Show line numbers in the gutter |
| `show_authorship` | bool | `true` | Show authorship markers (human/AI) in the gutter |
| `show_minimap` | bool | `true` | Show the minimap scrollbar on the right edge |
| `tab_width` | int | `4` | Tab display width in spaces |
| `spaces_for_tabs` | bool | `true` | Insert spaces when pressing Tab |
| `scroll_margin` | int | `5` | Lines from edge before viewport scrolls |
| `auto_save_seconds` | int | `0` | Auto-save interval (0 = disabled) |
| `relative_line_numbers` | bool | `false` | Show relative line numbers (toggle: `:set rnu`) |
| `word_wrap` | bool | `false` | Soft wrap long lines (toggle: `:set wrap`) |

## AI Settings

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `model` | string | `"claude-sonnet-4-20250514"` | Claude model to use |
| `max_tokens` | int | `4096` | Maximum tokens for AI responses |
| `aggressiveness` | string | `"moderate"` | Ghost suggestion level: `"minimal"`, `"moderate"`, `"proactive"` |
| `idle_threshold_ms` | int | `3000` | Idle time before speculative analysis triggers |

The AI also requires `ANTHROPIC_API_KEY` to be set in the environment.

## Themes

AURA ships with three built-in themes:

- **`dark`** (default) — dark background with standard colors
- **`light`** — light background
- **`monokai`** — Monokai-inspired color scheme

Set the theme in config:

```toml
theme = "monokai"
```

### Custom Themes

Define custom themes in `[theme_definitions.<name>]`:

```toml
theme = "catppuccin"

[theme_definitions.catppuccin]
name = "catppuccin"
bg = "#1e1e2e"
fg = "#cdd6f4"
keyword = "magenta"
string = "green"
comment = "#6c7086"
function = "blue"
type_name = "yellow"
```

Colors can be specified as:
- Named colors: `red`, `green`, `blue`, `yellow`, `magenta`, `cyan`, `white`, `black`, `gray`, `darkgray`, `reset`
- Hex values: `#RRGGBB` (e.g., `#1e1e2e`)

### Available Theme Colors

| Key | Description |
|-----|-------------|
| `bg` / `fg` | Editor background and foreground |
| `gutter_fg` | Line number color |
| `status_bg` / `status_fg` | Status bar colors |
| `selection_bg` / `selection_fg` | Selection highlight |
| `git_added` / `git_modified` / `git_deleted` | Git gutter markers |
| `error` / `warning` / `info` | Diagnostic colors |
| `keyword` / `string` / `comment` / `function` / `type_name` / `number` | Syntax highlight colors |
| `ghost` | Ghost suggestion text |
| `author_human` / `author_ai` | Authorship marker colors |

## Keybinding Customization

Override keybindings in the `[keybindings]` section:

```toml
[keybindings]
leader = "Space"

[keybindings.leader_map]
e = "explain"
f = "fix"
t = "test"
i = "intent"

[keybindings.normal_map]
# key = "action"
```

## MCP Server Connections

Configure external MCP servers that AURA connects to as a client:

```toml
[mcp_servers.filesystem]
url = "ws://localhost:9000"

[mcp_servers.custom-tools]
url = "ws://localhost:9001"
```

AURA also runs its own MCP server, exposing editor tools and resources. See [MCP Protocol](../architecture/mcp.md) for details.

## Conversations

```toml
[conversations]
max_message_age_days = 90       # Delete messages older than this (0 = no limit)
max_messages_per_conversation = 200  # Max messages per conversation (0 = no limit)
max_conversations = 500         # Max total conversations to retain (0 = no limit)
keep_recent_messages = 10       # Always preserve this many recent messages when compacting
auto_compact = true             # Auto-compact the database on startup
max_context_messages = 40       # Max messages sent to AI per chat turn
```

Use `:compact` to manually trigger database compaction. When AI is configured, long conversations are automatically summarized — the summary replaces old messages as context for future AI calls.

## Collaboration

```toml
[collab]
display_name = "alice"    # Name shown to peers (default: $USER)
default_port = 0          # Port to host on (0 = random available)
use_tls = false           # Encrypt traffic with TLS (self-signed cert)
bind_address = "127.0.0.1"  # "0.0.0.0" for internet access
require_auth = false      # Require authentication token to join
```

See [Collaborative Editing](../user-guide/collaborative-editing.md) for usage details.

## Debugger Adapters

AURA auto-detects debug adapters (CodeLLDB, debugpy, dlv, Node.js). Override with custom config:

```toml
[debuggers.codelldb]
command = "codelldb"
args = ["--port", "0"]
extensions = ["rs", "c", "cpp"]

[debuggers.debugpy]
command = "python3"
args = ["-m", "debugpy.adapter"]
extensions = ["py"]
```

See [Debugger (DAP)](../user-guide/debugger.md) for usage details.

## Session Persistence

AURA automatically saves editor state to `.aura/session.json` in the project root on exit and restores it on the next launch. This includes open tabs, cursor positions, scroll offsets, active tab, and panel visibility.

Session restore runs when AURA is launched without a file argument (`aura`). When a specific file is given (`aura file.rs`), the session is skipped.

See [Session Persistence](../user-guide/session.md) for full details.

## Tasks

Define project tasks to run from within the editor:

```toml
[tasks.build]
command = "cargo build"
description = "Build the project"

[tasks.test]
command = "cargo test"
description = "Run tests"

[tasks.lint]
command = "cargo clippy -- -D warnings"
description = "Run lints"
```

Run with `:task build`, `:task test`, etc. Tasks also appear in the command palette (`Ctrl+P`).

When no tasks are configured, AURA auto-detects common tasks based on project files (Cargo.toml, package.json, go.mod, Makefile, pyproject.toml).
