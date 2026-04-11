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
- **Interactive chat panel**: Conversational AI with tool execution and approval flow, right inside the editor.
- **Real-time collaborative editing**: Multiple AURA instances can edit the same file simultaneously over TCP, with automerge CRDT conflict resolution, colored peer cursors, and automatic reconnection.
- **Session persistence**: Open tabs, cursor positions, and UI layout are saved on exit and restored on reopen.

<!-- ANCHOR: overview-end -->

## Status — v1.0 Released

**AURA v1.0 is here.** 325 roadmap items complete across 13 development phases. 88 commits, 65+ modules, 281 tests, 10 themes, 3 AI providers.

### Highlights

| Category | Features |
|----------|----------|
| **AI** | Multi-provider (Claude, GPT, Ollama), inline completions, agent mode, RAG indexing, context pinning, checkpoints, PR review |
| **Editing** | Vim modal editing, multi-cursor, macros, text objects, surround, code folding, breadcrumbs, sticky scroll |
| **Intelligence** | Tree-sitter (17+ languages), LSP (10+ servers), semantic highlighting, inlay hints, code lens |
| **Git** | Source control panel, interactive rebase, diff view, graph, branches, stash, blame, AI commit messages |
| **Collaboration** | Real-time CRDT editing, peer cursors, follow mode, shared terminal |
| **Terminal** | Embedded PTY, tabs, split panes, shell integration, AI command suggestions |
| **Plugins** | Lua + WASM plugins, marketplace, plugin.toml metadata |
| **UI** | 10 themes, zen mode, markdown preview, minimap, command palette, settings modal |
| **Remote** | SSH editing, HTTP client, notebook/REPL, image preview |

See [CHANGELOG.md](CHANGELOG.md) for the full version history.

<!-- ANCHOR: quickstart-start -->

## Installation

### Homebrew (macOS & Linux)

```bash
brew tap odtorres/aura
brew install aura
```

### Shell installer (macOS / Linux)

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/odtorres/aura/releases/latest/download/aura-installer.sh | sh
```

### Cargo (from source)

```bash
cargo install --git https://github.com/odtorres/aura.git aura
```

### Download binaries

Pre-built binaries for macOS (Intel & ARM), Linux (x86-64 & ARM), and Windows are available on the [GitHub Releases](https://github.com/odtorres/aura/releases) page.

## Quick Start

```bash
# Open a file
aura path/to/file.rs

# Open scratch buffer
aura

# Collaborative editing: host a session
aura path/to/file.rs --host

# Collaborative editing: join a session
aura --join 127.0.0.1:12345

# Set your display name for collab
aura path/to/file.rs --host --name alice

# Or run from source
cargo run -p aura -- path/to/file.rs
```

<!-- ANCHOR: quickstart-end -->

## Language Server Setup

AURA auto-detects LSP servers when you open a file. Install a server for your language to get **inline error underlines**, diagnostics, go-to-definition, hover info, and more.

| Language | Install command |
|----------|---------------|
| Rust | `rustup component add rust-analyzer` |
| TypeScript/JS | `npm install -g typescript-language-server typescript` |
| Python | `npm install -g pyright` |
| Go | `go install golang.org/x/tools/gopls@latest` |
| PHP | `npm install -g intelephense` |
| C/C++ | Install `clangd` (LLVM/Clang toolchain) |
| Ruby | `gem install solargraph` |
| Bash/Shell | `npm install -g bash-language-server` |
| Elixir | [ElixirLS releases](https://github.com/elixir-lsp/elixir-ls/releases) |
| Lua | [LuaLS releases](https://github.com/LuaLS/lua-language-server/releases) |
| Dart | Included with Dart SDK |
| Swift | Included with Xcode |
| Kotlin | [KotlinLS releases](https://github.com/fwcd/kotlin-language-server/releases) |
| Zig | [ZLS releases](https://github.com/zigtools/zls/releases) |
| Scala | [Metals](https://scalameta.org/metals/) |
| Haskell | [HLS releases](https://github.com/haskell/haskell-language-server/releases) |
| Dockerfile | `npm install -g dockerfile-language-server-nodejs` |

Once installed and on your `$PATH`, open a file and AURA connects automatically. You'll see `LSP` in the status bar. Errors appear as **red underlines**, warnings as **yellow underlines** — in real-time as you type.

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
| **VisualBlock** | Column selection | `V-BLOCK` (magenta) |
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
| `Ctrl+V` | Enter Visual Block mode (column select) |
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
| `f{char}` / `F{char}` | Find char forward/backward on line |
| `t{char}` / `T{char}` | Till char forward/backward on line |
| `;` / `,` | Repeat / reverse last char search |
| `*` / `#` | Search word under cursor forward/backward |
| `x` | Delete character |
| `d{motion}` | Delete (operator + motion, e.g. `dw`, `d$`) |
| `c{motion}` | Change (delete + Insert, e.g. `cw`, `ci"`) |
| `y{motion}` | Yank (e.g. `yw`, `yy`) |
| `dd` | Delete line |
| `cc` | Change line |
| `yy` / `Y` | Yank line |
| `D` | Delete to end of line |
| `C` | Change to end of line |
| `s` | Substitute character |
| `S` | Substitute line |
| `r{char}` | Replace character |
| `J` | Join lines |
| `~` | Toggle case |
| `>>` / `<<` | Indent / dedent |
| `{count}{motion}` | Repeat motion (e.g. `3j`, `5dw`) |
| `.` | Repeat last edit |
| `q{a-z}` | Start/stop macro recording |
| `@{a-z}` | Play macro |
| `p` | Paste |
| `u` | Undo |
| `Tab` | Accept ghost suggestion |
| `Esc` | Dismiss ghost suggestion |
| `Alt+]` / `Alt+[` | Cycle ghost suggestions |
| `]` / `[` | Next / previous diagnostic |
| `Ctrl+S` | Save |
| `Ctrl+N` | Toggle file tree sidebar |
| `Ctrl+T` | Toggle terminal pane |
| `Ctrl+J` | Toggle chat panel |
| `Ctrl+H` | Toggle conversation history |
| `Ctrl+D` | Add cursor at next word match (multi-cursor) |
| `Ctrl+B` | Open branch picker |
| `Ctrl+P` | Open command palette |
| `Ctrl+H` | Toggle AI History panel |
| `Ctrl+W` | Toggle split pane focus |
| `Ctrl+,` | Open settings |

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
| `:chat` | Toggle chat panel |
| `:vsplit` / `:vs` | Vertical split pane |
| `:hsplit` / `:sp` | Horizontal split pane |
| `:only` | Close split pane |
| `:settings` / `:prefs` | Open settings modal |
| `:compact` | Compact conversation database |
| `:host` | Start hosting a collab session |
| `:join <addr:port>` | Join a collab session |
| `:graph` | Open git graph modal |
| `:branches` / `:br` | Open branch picker |
| `:collab-stop` | End the collab session |

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
| `Esc` / `Ctrl+T` | Return focus to editor |
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

### Git Panel (when focused)

| Key | Action |
|-----|--------|
| `j` / `Down` | Select next entry |
| `k` / `Up` | Select previous entry |
| `Tab` | Cycle to next section |
| `s` | Stage selected file |
| `S` | Stage all changed files |
| `u` | Unstage selected file |
| `d` | Discard changes (with confirmation) |
| `c` | Commit staged changes |
| `i` / `Enter` | Edit commit message (on Commit Message section) |
| `Enter` | Open diff view (on file entry) |
| `Esc` | Return focus to editor |
| Click `+` | Stage all changed files (on Changes header) |
| Click `✨` | AI-generate commit message (on Commit Message header) |

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
| Syntax parsing | tree-sitter | Incremental syntax highlighting (17+ languages, React/Next.js) |
| Language server | LSP client | Diagnostics, go-to-def, hover, code actions (10 servers) |
| AI API | reqwest + tokio-stream | Anthropic API streaming client |
| Protocol | MCP (WebSocket) | AI agent-editor communication |
| Storage | rusqlite | Conversation + decision history |
| Git | gitoxide (gix) | Native Rust git operations |
| Terminal | portable-pty + vte | Embedded PTY with ANSI color |
| Config | serde + toml | Settings and theme files |
| Testing | proptest + criterion | Property-based + benchmark testing |

<!-- ANCHOR: techstack-end -->


## Release History

See [CHANGELOG.md](CHANGELOG.md) for the full version history from v0.1.x through v1.0.2.

## Documentation

- [User Guide & Documentation](https://odtorres.github.io/aura/) — mdBook documentation site
- [API Reference](https://odtorres.github.io/aura/api/) — Rustdoc for all crates
- [Contributing](CONTRIBUTING.md) — Development guide

## License

MIT
