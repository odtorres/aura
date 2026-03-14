# Configuration

AURA is configured via `aura.toml`. It searches for configuration in:

1. `./aura.toml` (current directory)
2. `~/.config/aura/aura.toml` (user config)

If no config file is found, sensible defaults are used.

## Full Example

```toml
theme = "dark"

[editor]
line_numbers = true
show_authorship = true
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
| `tab_width` | int | `4` | Tab display width in spaces |
| `spaces_for_tabs` | bool | `true` | Insert spaces when pressing Tab |
| `scroll_margin` | int | `5` | Lines from edge before viewport scrolls |
| `auto_save_seconds` | int | `0` | Auto-save interval (0 = disabled) |

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
