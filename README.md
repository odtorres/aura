# AURA

**AI-native Universal Reactive Authoring** — a terminal text editor built from the ground up for human + AI co-authoring.

> The human steers. The AI proposes. The editor mediates.

<!-- ANCHOR: overview-start -->

## What makes AURA different?

Current editors treat AI as a plugin — a guest in a house built for a single human cursor. AURA treats human and AI as **co-authors**, with the editor as the mediator between them.

- **Authorship-aware editing**: Every change is tagged with who made it (human or AI). Undo just the AI's changes without losing yours.
- **Intent-first workflow**: Express what you want to achieve in natural language. The AI proposes, you review in a structured diff, then accept or reject per-hunk.
- **Conversation as history**: The conversation that led to every piece of code is recorded and queryable. Six months later, ask "why was this written this way?"
- **Multi-agent collaboration**: Multiple AI agents can work simultaneously via CRDT, with conflict-free concurrent editing.
- **Speculative execution**: The AI thinks ahead in the background, offering improvement suggestions as ghost text overlays.

<!-- ANCHOR: overview-end -->

## Status

**All development phases complete.** AURA is a fully-featured AI-native editor with CRDT-based multi-author editing, Anthropic AI integration, Tree-sitter syntax highlighting, LSP support, MCP protocol, speculative execution, git integration, an embedded PTY terminal, and a plugin system.

See [TODO.md](TODO.md) for the full roadmap and phase history.

<!-- ANCHOR: quickstart-start -->

## Installation

### Homebrew (macOS / Linux)

```bash
brew install aura-editor/tap/aura
```

### Shell installer (macOS / Linux)

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/odtorres/aura/releases/latest/download/aura-installer.sh | sh
```

### Cargo

```bash
cargo install aura
```

### Download binaries

Pre-built binaries for macOS (Intel & ARM), Linux (x86-64 & ARM), and Windows are available on the [GitHub Releases](https://github.com/odtorres/aura/releases) page.

## Quick Start

```bash
# Open a file
aura path/to/file.rs

# Open scratch buffer
aura

# Or run from source
cargo run -p aura -- path/to/file.rs
```

<!-- ANCHOR: quickstart-end -->

<!-- ANCHOR: keybindings-start -->

## Keybindings

### Modes

AURA uses vim-inspired modal editing with additional modes for AI interaction:

| Mode | Description | Indicator |
|------|-------------|-----------|
| **Normal** | Navigation and commands | `NORMAL` (blue) |
| **Insert** | Text input | `INSERT` (green) |
| **Visual** | Character selection | `VISUAL` (magenta) |
| **VisualLine** | Line selection | `V-LINE` (magenta) |
| **Command** | Ex-style commands (`:`) | `COMMAND` (yellow) |
| **Intent** | Natural language AI input | `INTENT` (cyan) |
| **Review** | AI proposal review | `REVIEW` (red) |

### Normal Mode

| Key | Action |
|-----|--------|
| `i` | Enter Insert mode |
| `a` | Append after cursor (enter Insert) |
| `o` | Open line below (enter Insert) |
| `v` | Enter Visual mode |
| `V` | Enter Visual Line mode |
| `:` | Enter Command mode |
| `h` / `Left` | Move left |
| `j` / `Down` | Move down |
| `k` / `Up` | Move up |
| `l` / `Right` | Move right |
| `w` | Next word start |
| `b` | Previous word start |
| `e` | Word end |
| `0` | Line start |
| `$` | Line end |
| `gg` | Go to top of file |
| `G` | Go to end of file |
| `gd` | Go to definition (LSP) |
| `K` | Hover info (LSP) |
| `x` | Delete character |
| `d` | Delete line |
| `y` | Yank line |
| `p` | Paste |
| `u` | Undo |
| `Tab` | Accept ghost suggestion |
| `Esc` | Dismiss ghost suggestion |
| `Alt+]` / `Alt+[` | Cycle ghost suggestions |
| `]` / `[` | Next / previous diagnostic |
| `Ctrl+S` | Save |
| `Ctrl+N` | Toggle file tree sidebar |
| `Ctrl+J` / `` Ctrl+` `` | Toggle terminal pane |

### Leader Key (`Space`)

| Sequence | Action |
|----------|--------|
| `<Space>i` | Enter Intent mode (AI) |
| `<Space>e` | Explain selected code (AI) |
| `<Space>f` | Fix errors at cursor (AI) |
| `<Space>t` | Generate test (AI) |
| `<Space>u` | Undo AI edits only |
| `<Space>a` | Toggle authorship markers |
| `<Space>b` | Toggle inline blame |
| `<Space>c` | Show conversation history |
| `<Space>d` | Show recent decisions |
| `<Space>g` | Cycle AI aggressiveness |
| `<Space>s` | Show semantic info |
| `<Space>p` | Open fuzzy file picker |

### Insert Mode

| Key | Action |
|-----|--------|
| `Esc` | Return to Normal mode |
| `Ctrl+S` | Save |
| Characters | Insert text |
| `Enter` | New line |
| `Backspace` | Delete backwards |
| Arrow keys | Navigate |

### Visual / Visual Line Mode

| Key | Action |
|-----|--------|
| `Esc` | Return to Normal mode |
| `d` / `x` | Delete selection |
| `y` | Yank selection |
| Navigation | Extend selection |

### Command Mode

| Command | Action |
|---------|--------|
| `:w` | Save |
| `:q` | Quit (warns on unsaved changes) |
| `:q!` | Force quit |
| `:wq` | Save and quit |
| `:intent` | Enter Intent mode |
| `:search <query>` | Search conversation history |
| `:decisions` / `:dec` | Show recent decisions |
| `:undo-tree` / `:ut` | Show undo tree |
| `:commit` / `:gc` | AI-generated commit |
| `:commit <msg>` | Commit with message |
| `:branches` / `:br` | List branches |
| `:checkout <name>` | Switch branch |
| `:branch <name>` | Create branch |
| `:blame` | Toggle inline blame |
| `:log` / `:gl` | Show aura git log |
| `:experiment <name>` | Enter experimental mode |
| `:code-action` / `:ca` | LSP code actions |
| `:plugins` | List loaded plugins |
| `:files` / `:fp` | Open fuzzy file picker |
| `:term` / `:terminal` | Toggle terminal pane |
| `:tree` | Toggle file tree |
| `:term-height <N>` / `:th <N>` | Set terminal height |

### Review Mode (AI Proposals)

| Key | Action |
|-----|--------|
| `a` / `Enter` | Accept proposal |
| `r` / `Esc` | Reject proposal |
| `e` | Edit proposal in-place |
| `R` | Request revision |

### Terminal Pane (when focused)

| Key | Action |
|-----|--------|
| `Esc` / `Ctrl+J` / `` Ctrl+` `` | Return focus to editor |
| `Ctrl+Shift+Up/Down` | Resize terminal pane |
| `Ctrl+C` | Send interrupt |
| `Ctrl+D` | Send EOF |
| `Ctrl+L` | Clear screen |
| All other keys | Forwarded to PTY |

### File Tree (when focused)

| Key | Action |
|-----|--------|
| `j` / `Down` | Select next |
| `k` / `Up` | Select previous |
| `Enter` / `l` | Open file / expand directory |
| `h` | Collapse directory / go to parent |
| `Esc` | Return focus to editor |
| `Ctrl+N` | Close file tree |

<!-- ANCHOR: keybindings-end -->

<!-- ANCHOR: techstack-start -->

## Tech Stack

| Layer | Tool / Crate | Purpose |
|-------|-------------|---------|
| Language | Rust | Performance, safety, async |
| Async runtime | Tokio | Concurrent AI streams + user input |
| Text buffer | ropey | Efficient rope data structure |
| CRDT | automerge | Multi-author conflict-free editing |
| TUI framework | ratatui + crossterm | Terminal rendering |
| Syntax parsing | tree-sitter | Incremental syntax highlighting |
| Language server | LSP client | Diagnostics, go-to-def, hover, code actions |
| AI API | reqwest + tokio-stream | Anthropic API streaming client |
| Protocol | MCP (WebSocket) | AI agent-editor communication |
| Storage | rusqlite | Conversation + decision history |
| Git | gitoxide (gix) | Native Rust git operations |
| Terminal | portable-pty + vte | Embedded PTY with ANSI color |
| Config | serde + toml | Settings and theme files |
| Testing | proptest + criterion | Property-based + benchmark testing |

<!-- ANCHOR: techstack-end -->

## Documentation

- [User Guide & Documentation](https://odtorres.github.io/aura/) — mdBook documentation site
- [API Reference](https://odtorres.github.io/aura/api/) — Rustdoc for all crates
- [Contributing](CONTRIBUTING.md) — Development guide

## License

MIT
