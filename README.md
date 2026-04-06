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

## Status

**All development phases complete.** AURA is a fully-featured AI-native editor with CRDT-based multi-author editing, Anthropic AI integration, Tree-sitter syntax highlighting, LSP support, MCP protocol, speculative execution, git integration, an embedded PTY terminal, real-time collaborative editing, and a plugin system.

See [TODO.md](TODO.md) for the full roadmap and phase history.

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

## New in v0.4 (Phase 12)

### AI & Agent System
- **Autonomous agent mode** with subagents, planning phases, trust levels (`:agent <task>`)
- **Per-feature AI model config** — use haiku for commits, sonnet for chat (`commit_model`, `agent_model`, etc.)
- **Agent diff review** — review all changes after agent completes (`:agent diff`)
- **Chat context sliding window** with automatic summarization

### LSP Intelligence
- **Inlay hints** — inline type annotations and parameter names
- **Semantic highlighting** — 23 token type colors from LSP semantic tokens
- **Code lens** — reference counts at end of function lines
- **Call hierarchy** — show incoming callers (`:calls`)
- **Signature help** — active parameter highlighting on `(` and `,`
- **Cross-file rename** — updates all open tab buffers

### Editing Features
- **Toggle comment** — `gc` or `:comment` (language-aware: `//` `#` `--` etc.)
- **Move line** — `Alt+j`/`Alt+k` moves current line
- **Visual wrap** — select text, type `(` `"` `[` to wrap in brackets/quotes
- **`:%s/old/new/g`** — vim-style search and replace
- **`:sort` / `:sort!`** — sort lines alphabetically or reverse
- **`:upper` / `:lower`** — convert selection case
- **`:duplicate`** — duplicate current line
- **`:trim`** — trim trailing whitespace
- **`:encoding lf/crlf`** — convert line endings
- **Block visual I/A** — insert/append text to all selected lines
- **`ge` / `gE`** — backward word/WORD end motions
- **`J` with count** — `3J` joins 3 lines
- **`:N`** — jump to line N

### UI & Navigation
- **Enhanced minimap** — 12-column code preview with scrollbar
- **Rainbow indent guides** — colored by nesting depth
- **Incremental search** — live match highlighting as you type
- **Search history** — Up/Down arrows, persisted across sessions
- **Command palette shortcuts** — keyboard shortcuts shown beside commands
- **File size + line count** in status bar
- **Selection word count** — `3L 12W 87C` in visual mode
- **Pinned tabs** — `:pin` / `:unpin`, protected from close
- **Tab reordering** — `:tabmove left/right`
- **Status bar click** — opens command palette
- **Diagnostic hints** — shows `<leader>f to fix` in popup

### Configuration & Workflow
- **Settings hot-reload** — `aura.toml` changes apply without restart
- **EditorConfig** — `.editorconfig` support for indent, line endings
- **Auto-format on save** — `format_on_save = true` (rustfmt, prettier, etc.)
- **Auto-save** — `auto_save_seconds = 30`
- **Auto clipboard sync** — yank copies to system clipboard
- **Named sessions** — `:session save/load/list/delete`
- **`:cd` / `:pwd`** — change working directory
- **Snippet variables** — `$TM_FILENAME`, `$CURRENT_DATE`, etc.

### Terminal & Debug
- **Terminal search** — `Ctrl+F` searches scrollback
- **Conditional breakpoints** — `:breakpoint if <condition>`
- **Watch expressions** — `:watch <expr>` / `:unwatch`
- **Debug variable expansion** — Enter to expand/collapse variable tree
- **Split scroll sync** — `:scrollsync`

### New in v0.4.4
- **`:run`** — run current file (cargo run, python, node, go, etc.)
- **`:test`** — run tests (cargo test, pytest, npm test, etc.)
- **Gutter click breakpoint** — click line number to toggle breakpoint
- **Word highlight** — all occurrences of word under cursor highlighted automatically
- **`:upper` / `:lower`** — convert selection case
- **`:trim`** — trim trailing whitespace
- **`:encoding lf/crlf`** — convert line endings
- **`:cd` / `:pwd`** — change working directory
- **`:run`** — smart run current file by language
- **`:test`** — smart test by language
- **`:recent`** — show recently opened files
- **`:set number/nonumber`** — toggle line numbers
- **`:set minimap/nominimap`** — toggle minimap
- **Ctrl+Z** — undo in any mode
- **Gutter click** — toggle breakpoint on line number
- **Word highlight** — auto-highlight all occurrences of word under cursor
- **`:count` / `:wc`** — document stats (lines, words, chars, size)
- **`:diff`** — diff unsaved changes vs file on disk
- **Ctrl+A** — select all (visual line mode)
- **`:open <folder>`** — open folder in file tree sidebar
- **Test runner** — auto-discover tests (Rust, Python, JS, Go), green ▶ gutter markers, `:test-at` runs test at cursor
- **`:collab-readonly <peer>`** — toggle read-only mode for collaboration peers
- **Workspace / multi-root** — `:workspace add/remove/list` for multi-folder projects, persisted in session
- **`:count` / `:wc`** — document statistics
- **`:diff`** — diff unsaved changes vs file on disk
- **Ctrl+A** — select all

### New in v0.4.9
- **Persistent settings** — settings modal changes (minimap, line numbers, tab width, etc.) are saved to `aura.toml` and survive restarts
- **Ctrl+T terminal toggle fix** — `Ctrl+T` / `` Ctrl+` `` now correctly toggles the terminal closed when focused (previously the keystroke was swallowed by the PTY)
- **Ctrl+G git panel toggle fix** — `Ctrl+G` now properly toggles the source control panel on/off (close branch was unreachable)
- **`g` shortcut in git panel** — press `g` in the source control panel to generate an AI commit message (like `c` for commit)
- **Indent guide rendering fix** — rainbow indent guides no longer render on top of code text for languages with mixed/tab indentation; guides use visual-column-aware counting and skip non-whitespace cells
- **Agents tab in AI Visor** — `:visor` now has a 6th tab showing discovered agents from `.claude/agents/` (project) and `~/.claude/agents/` (global); press `6` or Tab to navigate, Enter to open agent file

### New in v0.4.10
- **AI Visor documentation** — new docs page covering all 6 visor tabs (Overview, Settings, Skills, Hooks, Plugins, Agents)
- **Updated docs** — terminal, git, keybindings, and configuration pages updated with v0.4.9 features

### New in v0.4.11
- **Update notification `u` key** — press `u` to accept the update notification (click still works)
- **`.aura` folder visible** — removed from file tree skip list so all dotfolders are browsable

### New in v0.4.12
- **File tree scroll fix** — expanding a folder no longer jumps the viewport; scroll offset is now persistent and only adjusts when the selection goes off-screen

### New in v0.4.13
- **Config moved to `.aura/aura.toml`** — configuration now lives inside the `.aura/` folder; legacy `aura.toml` in project root is still supported
- **Crash fix** — opening `aura.toml` after changing settings no longer panics (ropey char-index-out-of-bounds); buffer is reloaded and cursor clamped after config save

### New in v0.4.14
- **Interactive rebase UI** — `:rebase` or `:rebase N` opens a visual `git rebase -i` modal with per-commit actions (pick/reword/edit/squash/fixup/drop), Alt+j/k to reorder, `w` to execute
- Color-coded actions: green=pick, cyan=reword, yellow=edit, magenta=squash, blue=fixup, red=drop

### New in v0.4.15
- **Remote SSH editing** — `:ssh user@host:/path/to/file` opens and edits files on remote machines via SSH; saves go back to the remote host automatically
- Uses your existing SSH config, keys, and known hosts — no extra setup needed
- `Buffer::from_text()` and `Buffer::clear_modified()` added to core for remote file support

### New in v0.4.16
- **Plugin marketplace** — `:plugin search` opens a modal to browse, install, and uninstall plugins from a git-based registry
- `:plugin install/uninstall/update/list` commands for CLI-style management
- Plugins use `plugin.toml` for metadata (name, version, description, author)
- Registry configurable in `aura.toml` via `[plugins] registry = "..."`
- Color-coded: green=installed, yellow=update available, white=new

### New in v0.4.17
- **Discard staged changes** — press `d` on staged files to unstage and discard with `y` confirmation (previously only worked on unstaged changes)

### New in v0.4.18
- **Homebrew distribution** — `brew tap odtorres/aura && brew install aura` now works with prebuilt binaries for macOS (ARM/Intel) and Linux (ARM/x86)

### New in v0.4.19
- **File tree actions** — full file management in the sidebar:
  - `r` rename, `D`/Delete delete (with `y` confirm), `a` new file, `A` new directory
  - `y` copy, `x` cut, `p` paste — vim-style clipboard for files
  - `.` reveal in Finder (macOS) / file manager (Linux) / Explorer (Windows)

### New in v0.4.20
- **Diff view as tabs** — opening a diff from the git panel now creates a real tab (`[diff] filename`), so you can switch between files and come back to the diff; close with `Esc`/`q` or the tab close button

### New in v0.4.21
- **Ctrl+T / Ctrl+N toggle fix** — panel toggle shortcuts now properly close panels on second press (state was cleared before checking, making the close branch unreachable)

## Documentation

- [User Guide & Documentation](https://odtorres.github.io/aura/) — mdBook documentation site
- [API Reference](https://odtorres.github.io/aura/api/) — Rustdoc for all crates
- [Contributing](CONTRIBUTING.md) — Development guide

## License

MIT
