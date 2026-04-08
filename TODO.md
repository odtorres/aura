# AURA — AI-native Universal Reactive Authoring editor

> A terminal text editor built from the ground up for human + AI co-authoring.
> The human steers. The AI proposes. The editor mediates.

---

## Philosophy

Current editors treat AI as a plugin — a guest in a house built for a single human cursor.
AURA treats human and AI as **co-authors**, with the editor as the mediator.
The editing loop is: **express intent → AI proposes → human reviews → accept/reject/refine**.

---

## Phase 0: Foundation — Minimal Viable Editor (Weeks 1–3)

The goal is a working terminal editor that can open, edit, and save files.
No AI yet. Just proving the core architecture works.

### 0.1 Project scaffold
- [x] Initialize Cargo workspace with crates: `core`, `tui`, `editor`
- [x] Set up CI with `cargo clippy`, `cargo test`, `cargo fmt`
- [x] Add `CLAUDE.md` with project conventions for Claude Code iteration
- [x] Add `.gitignore`, `LICENSE` (MIT), `README.md`

### 0.2 Text buffer with Rope
- [x] Integrate `ropey` crate for the text buffer
- [x] Implement basic operations: insert char, delete char, insert line, delete line
- [x] Implement cursor movement: up, down, left, right, word-jump, line start/end
- [x] Handle UTF-8 correctly (emojis, multi-byte characters)
- [x] Write unit tests for all buffer operations
- [x] Property-based tests with `proptest` for insert/delete sequences

### 0.3 TUI rendering
- [x] Integrate `ratatui` + `crossterm`
- [x] Render a single buffer with line numbers
- [x] Implement viewport scrolling (vertical + horizontal)
- [x] Status bar: filename, cursor position, mode indicator
- [x] Command bar at the bottom (for future commands)
- [x] Handle terminal resize events gracefully

### 0.4 File I/O
- [x] Open file from CLI argument
- [x] Save file (Ctrl+S or :w)
- [x] Detect unsaved changes, warn on quit
- [x] Handle large files without freezing (stream reading)

### 0.5 Basic modal editing
- [x] Normal mode: navigation, delete, yank, paste
- [x] Insert mode: typing, backspace, enter
- [x] Command mode: :w, :q, :wq, :q!
- [x] Visual mode: character and line selection
- [x] Mode transitions with clear visual indicator
- [x] Keep keybindings vim-like but don't try to clone vim — just the essentials

### Phase 0 Definition of Done
> Can open a Rust file, navigate it, make edits, and save. Feels responsive.
> All buffer operations have tests. No crashes on edge cases.

---

## Phase 1: CRDT + Authorship Tracking (Weeks 4–6)

This is where AURA diverges from every other editor.
Every edit carries metadata: who made it, when, and why.

### 1.1 CRDT integration
- [x] Integrate `automerge` crate
- [x] Replace or layer CRDT on top of rope buffer
- [x] Define author IDs: `human`, `ai-agent-1`, `ai-agent-n`
- [x] Every edit operation tagged with author + timestamp
- [x] Benchmark: ensure no perceptible latency from CRDT overhead on keystroke

### 1.2 Authorship-aware undo/redo
- [x] Implement per-author undo: roll back only AI changes, or only human changes
- [x] Global undo: roll back all changes in chronological order
- [x] Undo tree visualization (simple text-based, in a split pane)
- [x] Keybinding: `u` for global undo, `<leader>u` for AI-only undo

### 1.3 Change provenance UI
- [x] Gutter markers: color-coded by author (human = green, AI = blue)
- [x] Inline ghost-text for pending AI suggestions (dimmed/italic)
- [x] Toggle: show/hide authorship highlighting (`<leader>a`)
- [x] Status bar shows "last change by: human | 2s ago"

### Phase 1 Definition of Done
> Can distinguish human vs AI edits visually. Can undo AI changes independently.
> CRDT operations benchmarked under 1ms per edit on a 10K line file.

---

## Phase 2: AI Integration — The Core Loop (Weeks 7–10)

This is the heart of AURA: the intent → propose → review → accept cycle.

### 2.1 Anthropic API client
- [x] Create `ai` crate in workspace
- [x] Implement streaming API client for Claude (Anthropic API, reqwest + tokio)
- [x] Handle API key from env var (`ANTHROPIC_API_KEY`) or config file
- [x] Token counting and context window management
- [x] Retry logic with exponential backoff
- [x] Rate limiting awareness

### 2.2 Context assembly
- [x] Send current buffer content as context
- [x] Include cursor position and selection
- [x] Include file path, language, and project structure
- [x] Include Tree-sitter syntax node at cursor (see Phase 3)
- [x] Include recent edit history (last N changes with authorship)
- [x] Include relevant diagnostics from LSP (see Phase 3)
- [x] Truncation strategy: prioritize code near cursor, summarize distant code

### 2.3 Intent mode
- [x] New mode: Intent mode (triggered by `<leader>i` or `:intent`)
- [x] User types natural language intent: "handle errors in this function"
- [x] Intent is sent to AI with full context
- [x] AI response streams in as a structured diff/edit proposal
- [x] Parse AI response into concrete edit operations

### 2.4 Review interface
- [x] Split view: current code (top) vs proposed code (bottom)
- [x] Inline diff highlighting: proposed additions in green
- [x] Per-hunk accept/reject: `a` to accept, `r` to reject, `Esc` to cancel
- [x] Edit-in-place: modify the proposal before accepting
- [x] Keybinding: `<leader>rr` to request revision with follow-up intent
- [x] Animated streaming: show AI text appearing as it streams

### 2.5 Quick actions (no review needed)
- [x] Inline completion: Tab to accept ghost-text suggestion
- [x] `<leader>e` — Explain selected code
- [x] `<leader>f` — Fix diagnostic at cursor
- [x] `<leader>t` — Generate test for function at cursor
- [x] These use the same AI pipeline but with pre-built intents

### Phase 2 Definition of Done
> Can select code, express intent, review AI proposal in a diff view,
> and accept/reject per hunk. Streaming feels responsive. Edits are tracked as AI-authored.

---

## Phase 3: Semantic Awareness (Weeks 11–14)

The editor understands code structure, not just text.

### 3.1 Tree-sitter integration
- [x] Integrate `tree-sitter` crate with Rust bindings
- [x] Incremental parsing on every edit (must handle AI streaming edits)
- [x] Syntax highlighting using Tree-sitter queries
- [x] Expose syntax tree to AI context: current node, parent, scope
- [x] Language grammars: start with Rust, TypeScript, Python, Go
- [x] Highlight groups configurable via theme file (TOML)

### 3.2 LSP client
- [x] Implement LSP client (consider `tower-lsp` or custom over JSON-RPC)
- [x] Diagnostics: show errors/warnings in gutter + floating window
- [x] Go to definition, references, hover info
- [x] Code actions: integrate with AI (AI can trigger code actions or vice versa)
- [x] Feed LSP diagnostics into AI context automatically

### 3.3 Semantic graph
- [x] Build a lightweight dependency graph from Tree-sitter + LSP data
- [x] Track: which functions call which, which tests cover which functions
- [x] When AI proposes a change, show "affected by this change: X, Y, Z"
- [x] Feed graph info to AI: "this function is called by 3 other functions"

### Phase 3 Definition of Done
> Syntax highlighting via Tree-sitter. LSP diagnostics and navigation working.
> AI context includes structural info. User can see impact of proposed changes.

---

## Phase 4: Conversation Layer (Weeks 15–18)

Code and conversation are interleaved, not separate.

### 4.1 Persistent conversation storage
- [x] Integrate `rusqlite` for local SQLite database
- [x] Schema: conversations, messages, intents, edit_decisions
- [x] Link every conversation to file path + line range + git commit
- [x] Full-text search over conversation history

### 4.2 Inline conversation
- [x] Attach conversations to code ranges (like comments, but richer)
- [x] Toggle visibility: show/hide conversation annotations
- [x] Keybinding: `<leader>cc` to see conversation history for current line/function
- [x] "Why was this written this way?" → retrieves originating conversation

### 4.3 Decision log
- [x] Every accept/reject decision is logged with context
- [x] Queryable: "show me all rejected AI suggestions this week"
- [x] Pattern detection: "AI keeps suggesting X, you keep rejecting — adjust?"

### Phase 4 Definition of Done
> Can retrieve the conversation that led to any piece of code.
> Decision history is queryable. Conversation persists across sessions.

---

## Phase 5: MCP Protocol Layer (Weeks 19–22)

AURA becomes a platform, not just an editor.

### 5.1 MCP server
- [x] AURA exposes MCP server over TCP (localhost, auto-assigned port)
- [x] Tools exposed: `read_buffer`, `edit_buffer`, `get_diagnostics`,
      `get_selection`, `get_cursor_context`, `get_conversation_history`
- [x] Resources exposed: open files (buffer/current, buffer/info, diagnostics)
- [x] Any MCP client (Claude Code, custom agents) can connect

### 5.2 MCP client
- [x] AURA can connect to external MCP servers
- [x] Integrate with filesystem, git, and custom tool servers
- [x] Configuration: `aura.toml` defines MCP server connections

### 5.3 Multi-agent support
- [x] Multiple AI agents can connect simultaneously
- [x] Each agent gets its own author ID in the CRDT
- [x] Agent activity shown in status bar (agent count + MCP port)
- [x] Conflict resolution: if two agents edit the same region, human decides
- [x] Agent orchestration: "Agent A handles tests, Agent B handles implementation"

### Phase 5 Definition of Done
> Claude Code can connect to AURA via MCP and read/edit buffers.
> Multiple agents can work simultaneously with CRDT conflict resolution.

---

## Phase 6: Speculative Execution (Weeks 23–26)

The AI thinks ahead. The human reviews when ready.

### 6.1 Background analysis
- [x] On file open / cursor move, queue background AI analysis
- [x] Debounce: don't spam the API (3s idle threshold)
- [x] Priority queue: analyze code near cursor first (±15 lines)
- [x] Cache results: don't re-analyze unchanged code (FNV-1a content hash)

### 6.2 Ghost suggestions
- [x] Render AI improvement suggestions as ghost text overlay
- [x] Multiple suggestions per location (cycle with `<Alt+]>` / `<Alt+[>`)
- [x] Categories: fix, simplify, error handling, performance, refactor
- [x] Configurable aggressiveness: minimal / moderate / proactive (`<leader>g`)

### 6.3 Multi-file awareness
- [x] When a change is accepted, AI proactively checks related files
- [x] Cross-file changes proposed via semantic graph impact analysis
- [x] Proposed cross-file changes shown as a changeset
- [x] Accept/reject at changeset level or per-file

### Phase 6 Definition of Done
> AI proactively suggests improvements as ghost text.
> Cross-file changes proposed as atomic changesets. Background analysis doesn't block UI.

---

## Phase 7: Git Integration (Weeks 27–30)

### 7.1 Git awareness
- [x] Integrate `gitoxide` (`gix`) for native Rust git operations
- [x] Gutter: show git diff status (added/modified/deleted lines)
- [x] Inline blame with authorship (`<leader>b` or `:blame`)
- [x] Commit from within editor with AI-generated commit messages (`:commit`)

### 7.2 Conversation-linked commits
- [x] Attach conversation summaries to git commits as `Aura-Conversation` trailers
- [x] `git log --aura` (custom formatter) shows intent history
- [x] Link between git blame and conversation history

### 7.3 Branch management
- [x] Visual branch switcher (`:branches`, `:checkout <name>`)
- [x] AI can propose changes on a feature branch (`:branch <name>`)
- [x] "Experimental" mode: AI works on a branch, human reviews the PR

---

## Phase 8: Polish and Ecosystem (Weeks 31+)

### 8.1 Configuration
- [x] `aura.toml` for all settings: theme, keybindings, AI model, aggressiveness
- [x] Theme engine: color schemes in TOML (ship with at least 3 themes)
- [x] Keybinding customization layer

### 8.2 Performance
- [x] Profile with `flamegraph` crate
- [x] Target: <1ms keystroke-to-render latency
- [x] Target: <16ms frame time for streaming AI output
- [x] Target: handles 100K+ line files without lag
- [x] Memory profiling: ensure CRDT history doesn't grow unbounded (compact on save)

### 8.3 Plugin system (future)
- [x] Lua or WASM plugin API (evaluate tradeoffs)
- [x] Plugins can register new intents, modes, and UI panels
- [x] Ship with core plugins: file picker (fuzzy finder), file tree, terminal

### 8.4 Distribution
- [x] `cargo install --git` from GitHub
- [x] Shell installer (curl | sh)
- [x] Homebrew formula (template ready)
- [x] AUR package
- [x] AppImage for Linux
- [x] Release automation with `cargo-dist`
- [x] detect in the app a new version and show there is a new version available

### 8.5 Distribution — Remaining Manual Steps
- [x] Create `odtorres/homebrew-aura` repo on GitHub
- [x] Add `HOMEBREW_TAP_TOKEN` secret to the aura repo
- [x] Verify GitHub Release pipeline works on tag push
- ~~Publish to crates.io~~ — Not viable (include_str! paths outside crate dirs). Use `cargo install --git` instead.


## Phase 9: UX Improvements & Claude Code Integration (Post-launch)

Focused on making the editor feel polished and deeply integrated with Claude Code.

### 9.1 Interactive panel navigation
- [x] File tree focus mode: `Ctrl+n` opens tree with keyboard focus
- [x] Navigate with `j`/`k`, expand dirs with `Enter`/`l`, collapse with `h`
- [x] Open files by pressing `Enter` on a file entry
- [x] `Esc` returns focus to editor, `Ctrl+n` again closes tree
- [x] Visual indicator: focused panel shows yellow border with `[focused]` label

### 9.2 Real PTY terminal
- [x] Replace command runner with full PTY terminal (`portable-pty` + `vte`)
- [x] Real shell (inherits `$SHELL`) with colors, tab completion, history
- [x] ANSI 256-color parsing and rendering via VTE state machine
- [x] Streaming output — no more blocking on long-running commands
- [x] Scrollback buffer (5000 lines) with scroll navigation
- [x] Cursor rendered as reversed cell in the terminal pane
- [x] Auto-resize: PTY dimensions sync to actual pane size every frame
- [x] Full key forwarding: `Ctrl+C`, `Ctrl+D`, arrows, tab, etc.

### 9.3 Claude Code auto-discovery
- [x] `AURA_MCP_PORT` env var injected into the embedded terminal shell
- [x] Discovery file `~/.aura/mcp.json` written on startup (host, port, pid, file)
- [x] Discovery file cleaned up on editor exit
- [x] Manual override: users can set `AURA_MCP_PORT` externally to connect

### Phase 9 Definition of Done
> File tree and terminal panels are fully interactive with keyboard focus.
> PTY terminal runs a real shell with colors and streaming output.
> Claude Code running inside or outside AURA can auto-discover the MCP server.

---

## Phase 10: Real-Time Collaborative Editing

Multiple AURA instances can collaborate on the same file in real-time using the existing automerge CRDT for conflict-free merging.

### 10.1 CRDT sync foundation
- [x] Add `AuthorId::Peer { name, peer_id }` for remote human peers
- [x] Expose automerge sync API on `CrdtDoc` (generate/receive sync messages, save/load, fork)
- [x] Create `sync.rs` module with `PeerSyncState` and re-exports
- [x] Add `Buffer::apply_remote_sync()` for rope reconciliation after CRDT sync
- [x] Add `Buffer::load_remote_snapshot()` for initial document transfer
- [x] Add sync convergence unit tests (bidirectional, concurrent edits, fork, roundtrip)

### 10.2 Networking layer
- [x] Create `collab.rs` with `CollabSession`, TCP host/client, binary wire protocol
- [x] Add `poll_collab_events()` to main event loop
- [x] Add CLI flags: `--host`, `--join <addr:port>`, `--name <display_name>`
- [x] Add `:host` / `:join` / `:collab-stop` commands in command mode
- [x] Broadcast local edits to peers after every buffer mutation
- [x] Apply incoming sync messages to buffer on receive
- [x] Add `CollabConfig` to `aura.toml` (display_name, default_port)
- [x] Show collab status in status bar (COLLAB:port, peer count)

### 10.3 Remote peer awareness
- [x] Broadcast cursor position and selection to peers (throttled, max 50ms)
- [x] Render peer cursors as colored blocks with name labels
- [x] Highlight peer selections with colored backgrounds
- [x] Assign unique colors per peer (6-color rotating palette)

### 10.4 Reconnection & robustness
- [x] Client reconnect with exponential backoff (1s, 2s, 4s, ..., 30s max)
- [x] Host retains peer sync state for 5 minutes after disconnect
- [x] Show collab status in status bar (hosting, connected, reconnecting with attempt #)

### 10.5 Incremental rope reconciliation
- [x] Replace full rope rebuild with incremental diff (O(delta + scan) instead of O(document))
- [x] Update only affected line_authors for changed lines

### 10.6 Multi-file sessions
- [x] Wire protocol extended with `file_id` (u64 hash of canonical path) on all messages
- [x] New message types: `MSG_FILE_OPENED` (0x06), `MSG_FILE_CLOSED` (0x07)
- [x] Per-file sync state in `PeerInfo` (`HashMap<u64, SyncState>`)
- [x] Per-file awareness in `PeerInfo` (`HashMap<u64, AwarenessUpdate>`)
- [x] Host shares ALL open files on session start (multi-snapshot)
- [x] Sync messages routed to correct tab by `file_id`
- [x] Clients auto-open tabs for files received from host
- [x] `EditorTab::file_id()` helper for deterministic file identification
- [x] Peer cursors filtered to active tab's file only
- [x] Snapshot payload encoding with file_id + path + data
- [x] Backward compatible: `file_id=0` treated as legacy single-file mode

### Phase 10 Definition of Done
> Two or more AURA instances can connect, see each other's cursors, and edit multiple files in real-time with automatic conflict resolution via the CRDT.

---

## Tech Stack Summary

| Layer              | Tool / Crate            | Purpose                                    |
|--------------------|-------------------------|--------------------------------------------|
| Language           | Rust                    | Performance, safety, async                 |
| Async runtime      | Tokio                   | Concurrent AI streams + user input         |
| Text buffer        | ropey                   | Efficient rope data structure              |
| CRDT               | automerge               | Multi-author conflict-free editing         |
| TUI framework      | ratatui + crossterm     | Terminal rendering                         |
| Parsing            | tree-sitter             | Incremental syntax parsing                 |
| LSP                | tower-lsp / custom      | Language intelligence                      |
| AI API             | reqwest + tokio-stream  | Anthropic API streaming client             |
| Protocol           | MCP (WebSocket)         | AI agent ↔ editor communication            |
| Storage            | rusqlite                | Conversation + decision history            |
| Git                | gitoxide                | Native Rust git operations                 |
| Serialization      | serde + toml            | Config files, data persistence             |
| Testing            | proptest + insta        | Property-based + snapshot testing          |
| Terminal emulation | portable-pty + vte      | Real PTY shell with ANSI color parsing     |
| Profiling          | flamegraph              | Performance analysis                       |

---

## CLAUDE.md notes (for Claude Code iteration)

When working on this project with Claude Code:
- Always run `cargo clippy` before committing
- Always run `cargo test` after any change to `core` crate
- Buffer operations must never panic — use `Result` types
- Every public function needs a doc comment
- CRDT operations must be benchmarked when modified
- The TUI must remain responsive during AI API calls (never block the main loop)
- Prefer small, focused commits over large ones
- When adding a new crate dependency, justify it in the commit message

---

## Open Questions (Resolved & Remaining)

- [x] Should the CRDT layer sit between the rope and the renderer, or wrap the rope? → **Decided: CRDT wraps the rope** (`crdt.rs` CrdtDoc wraps Buffer)
- [x] Automerge vs Yrs (Yjs) — need to benchmark both for this use case → **Decided: Automerge** (in Cargo.toml)
- [x] Lua vs WASM for the future plugin system — Lua is simpler, WASM is more universal → **Decided: Rust trait-based plugin system** (`plugin.rs`)
- [x] How aggressive should the speculative execution be by default? → **Decided: Configurable via `<leader>g`** (minimal/moderate/proactive)
- [x] Should AURA support Vim emulation deeply, or define its own keybinding paradigm? → **Decided: Vim-inspired essentials**, not full emulation
- [x] How to handle very long conversations — auto-compact with configurable retention, AI summarization, context window capping

### 10.7 Completed
- [x] Visual block mode (Ctrl+V column editing)
- [x] Wire TLS into actual streams
- [x] More LSP features (rename, references panel)
- [x] Undo tree visualization
- [x] Multi-cursor editing
- [x] Snippet system
- [x] Integrated debugger (DAP protocol)
- [x] 3-panel merge conflict editor
- [x] AI Visor (Claude Code config browser)
- [x] Inline conflict resolution
- [x] Global panel-switching shortcuts

---

## Phase 11: Next-Generation Features

Based on competitive analysis vs Cursor, Zed, VS Code Copilot, Windsurf, Helix, and Neovim.

### 11.1 AI Features — High Priority
- [x] **@-mentions in chat** — Reference `@file.rs`, `@selection`, `@buffer`, `@errors` in the chat panel to give AI precise context.
- [x] **Autonomous agent mode** — `:agent <task>` lets AI loop autonomously with all tools auto-approved. Configurable iteration limit.
- [x] **Project rules / AI instructions** — `.aura/rules.md` or `.aura/rules/*.md` files automatically injected into AI system prompt.
- [x] **Next-edit prediction** — Predict WHERE the user will edit next, not just what. Cursor Tab and Copilot NES both do this. Show ghost cursors at predicted locations.

### 11.2 Editor UX — High Priority
- [x] **Code folding** — Fold/unfold code blocks using tree-sitter scope data. `zc` fold, `zo` open, `za` toggle, `zM` fold all, `zR` unfold all.
- [x] **Sticky scroll** — Pin current scope headers (function/class names) at the top of the editor.
- [x] **Indent guides** — Vertical lines showing scope depth at indent boundaries.
- [x] **Bracket pair colorization** — Color bracket pairs by nesting depth (rainbow brackets, 6-color palette).
- [x] **Project-wide search/replace** — Interactive search across all files with preview and batch replace. (`Ctrl+F` or `:search`)

### 11.3 Code Navigation — Medium Priority
- [x] **Document outline** — `Ctrl+O` or `:outline` — fuzzy-searchable symbol list for the current file.
- [x] **Breadcrumbs** — Scope path at top of editor: `file.rs > impl App > fn process_data`.
- [x] **Peek definition** — Inline popup showing definition without leaving current file. Like VS Code's Alt+F12.

### 11.4 Protocol & Integration — High Priority
- [x] **ACP (Agent Client Protocol) server** — JSON-RPC 2.0 over TCP. Exposes document read/edit, cursor, diagnostics, selection, file ops, terminal, project structure. Any ACP agent can drive AURA.
- [x] **@-docs indexing** — `@docs:<name>` mentions reference files from `.aura/docs/` directory.
- [x] **Task runner integration** — `:task <name>` runs tasks defined in `aura.toml [tasks]`. Auto-detects for Rust/Node/Go/Python/Make.

### 11.5 Terminal & Shell — Medium Priority
- [x] **Terminal shell integration** — Detect command boundaries, exit codes, and auto-suggest fixes for failed commands. VS Code has this natively.
- [x] **Multiple terminal tabs** — Split terminals or tabbed terminals. Currently only one terminal pane.
- [x] **Terminal inline AI suggestions** — AI suggests shell commands based on context. Copilot does this in VS Code terminal.

### 11.6 Git & Collaboration — Medium Priority
- [x] **Stash management** — View, push (`z`), pop (`p`), drop (`d`) stashes from source control panel.
- [x] **PR creation from editor** — `:pr` opens `gh pr create` interactively in the terminal.
- [x] **Follow mode in collab** — Follow a peer's viewport in real-time. Zed has this.
- [x] **Shared terminal in collab** — Share terminal output with collaborators.

### 11.7 Editor Polish — Nice to Have
- [x] **Auto-close brackets/quotes** — Automatically insert matching pair when typing `(`, `[`, `{`, `"`, `'`.
- [x] **Surround editing** — `cs"'` change surrounding quotes, `ds(` delete surrounding parens, `ysiw"` surround word with quotes.
- [x] **Word wrap / soft wrap** — Toggle with `:set wrap` / `:set nowrap`.
- [x] **Relative line numbers** — `:set rnu` / `:set nornu`. Current line shows absolute.
- [x] **Marks / bookmarks** — `m{a-z}` to set, `'{a-z}` to jump. `:marks` to list.
- [x] **Registers display** — `:registers` to show yank/delete register contents.
- [x] **Macro editing** — Edit macro contents before replaying.

### 11.8 Distribution
- [x] Create `aura-editor/homebrew-tap` repo on GitHub
- [x] Add `HOMEBREW_TAP_TOKEN` secret to the aura repo
- [x] Verify GitHub Release pipeline works on tag push
- ~~Publish to crates.io~~ — Not viable. Use `cargo install --git` instead.

---

## Phase 12: Competitive Feature Parity & Polish

Comprehensive audit comparing AURA against VS Code, Zed, Helix, Cursor, and Neovim.
Organized by priority: features to add, existing features to improve.

### 12.1 New Features — High Priority

- [x] **Minimap** — Visual code overview sidebar for quick navigation (like VS Code/Zed). Render a scaled-down view of the buffer on the right edge.
- [x] **Scrollbar** — Visual scroll position indicator in the editor gutter.
- [x] **Inlay hints** — Request inlay hints from LSP and render inline (type annotations, parameter names).
- [x] **Semantic highlighting** — Use LSP semantic tokens for meaning-based coloring on top of tree-sitter syntax.
- [x] **Incremental search** — Live-updating match highlights as the user types in `/` search mode.
- [x] **Multiple terminal tabs** — Support more than one terminal instance (`:term new`, `:term close`, `:term next/prev` — commands exist, need actual multi-instance backend).
- [x] **Auto-save** — Save files on configurable interval or focus-loss. Add `auto_save` option to `aura.toml`.
- [x] **Auto-format on save** — Run language formatter (rustfmt, prettier, black, gofmt) on save. Add `format_on_save` to `aura.toml`.
- [x] **Search in terminal buffer** — Find text in terminal scrollback with `/` or Ctrl+F while terminal focused.
- [x] **Settings hot-reload** — Watch `aura.toml` for changes and apply without restarting the editor.

### 12.2 New Features — Medium Priority

- [x] **Call hierarchy** — LSP `callHierarchy/incomingCalls` and `outgoingCalls` displayed in a tree panel.
- [x] **Rainbow indent guides** — Color indent guide lines by nesting depth (like bracket pair colorization).
- [x] **Workspace / multi-root** — Open multiple project folders as one session, with per-folder settings.
- [x] **Conditional breakpoints** — Break only when a user-specified expression evaluates to true.
- [x] **Watch expressions** — Monitor variable values during debug sessions, persisted across steps.
- [x] **Search history** — Recall previous `/` and `:search` queries with Up/Down arrows.
- [x] **File encoding detection** — Show current encoding in status bar, allow changing (UTF-8, Latin-1, etc.).
- [x] **EditorConfig support** — Read `.editorconfig` files for per-project indent style, line endings, trim whitespace.
- [x] **Test runner integration** — Discover tests via LSP/tree-sitter, run from UI, show pass/fail/duration.
- [x] **Code lens** — Render reference counts, test status, etc. above functions using LSP code lens.

### 12.3 New Features — Lower Priority

- [x] **Peek definition** — Inline definition preview popup without opening a new tab (like VS Code Alt+F12).
- [x] **Pinned / grouped tabs** — Pin important tabs so they can't be accidentally closed, group related files.
- [x] **Tab reordering** — Move tabs left/right via command or drag.
- [x] **Interactive rebase UI** — Visual `git rebase -i` with drag-to-reorder commits.
- [x] **Remote development (SSH)** — Edit files on remote machines via SSH connection.
- [x] **Multiple named sessions** — Save/switch between session profiles (e.g., "frontend", "backend").
- [x] **Plugin marketplace** — Discover, install, and update plugins from within the editor.
- [x] **Snippet variables** — Support `$TM_FILENAME`, `$TM_LINE_NUMBER`, `$CURRENT_DATE`, etc. in snippet expansions.

### 12.4 Existing Features — Critical Improvements

- [x] **Terminal tabs** — Wire actual multi-instance terminal backend (commands already parsed, need EmbeddedTerminal vec + tab state).
- [x] **Plugin buffer API** — Expose buffer read/write, cursor, diagnostics to Lua plugins so they can actually do useful work.
- [x] **Agent diff review** — Complete the stubbed `:agent diff` panel: show before/after file diffs when agent finishes, with accept/revert per file.
- [x] **Git graph** — Replace basic ASCII graph with interactive branch visualization (scrollable, clickable commits).
- [x] **Theme hot-reload** — Apply theme changes from `aura.toml` without restarting.

### 12.5 Existing Features — Important Improvements

- [x] **File watcher** — Replace polling with OS-native file watching (fsevents on macOS, inotify on Linux) for instant external change detection.
- [x] **Signature help formatting** — Rich formatting with highlighted active parameter, multi-line overloads.
- [x] **Collaboration permissions** — Add read-only mode for peers, per-file lock support.
- [x] **Debug variable inspection** — Wire `request_variables` for lazy-loading expandable variable tree in debug panel.
- [x] **Search result preview** — Syntax-highlighted preview with more context in project search results.
- [x] **Chat context window** — Replace hardcoded 40-message limit with sliding window + AI summarization of older messages.
- [x] **Command palette shortcuts** — Show keyboard shortcuts beside each command in Ctrl+Shift+P palette (already done for `:` commands).

### 12.6 Existing Features — Polish

- [x] **Hover info rendering** — Better markdown formatting, syntax-highlighted code blocks in hover popups.
- [x] **Completion doc preview** — Show documentation preview beside completion menu items.
- [x] **Block visual mode** — Fix edge cases in block insert/replace/delete operations.
- [x] **`ge` motion** — Add backward-to-word-end vim motion.
- [x] **Cross-file rename** — Use LSP workspace edit for rename refactoring across multiple files.
- [x] **Auto conversation compaction** — Automatically compact long AI conversations using summarization.
- [x] **Split pane sync** — Full scroll position synchronization between split panes in follow mode.
---

## Phase 13: Next-Gen AI & Editor Features (v0.9 → v1.0)

> Research-driven roadmap based on 2025-2026 editor landscape gap analysis.
> Benchmarked against: Cursor, Windsurf, Zed, VS Code, JetBrains, Continue.dev, Aider.

### 13.1 Critical — AI Infrastructure

- [ ] **Codebase RAG Indexing** — Embed entire codebase for AI context retrieval across all files; vector store with incremental updates on file save/git checkout.
- [ ] **Apply Model (Two-Model Pattern)** — Fast smaller model merges AI-proposed diffs into source files reliably; separate from the reasoning model.
- [ ] **AI PR Review Integration** — AI reviews pull requests inline with comments and suggestions; GitHub/GitLab API integration.
- [ ] **Incremental Background Indexing** — Instant workspace-symbol search at scale (100K+ file repos); powers go-to-symbol-in-workspace and AI retrieval.

### 13.2 High — Competitive Differentiators

- [ ] **AI Checkpoints + Rollback** — Automatic checkpoints before every AI edit; timeline UI to diff any two states and roll back.
- [ ] **Context Pinning** — Pin files, docs, or symbols as always-included AI context; persists across conversations.
- [ ] **Bring Your Own Model (20+ providers)** — Azure OpenAI, AWS Bedrock, Google Vertex, llama.cpp, vLLM, Together, Groq, Mistral API.
- [ ] **Structured Diff Edits** — AI returns search/replace blocks or unified diffs instead of full file rewrites; token-efficient.
- [ ] **Inline AI Chat (Ctrl+K)** — Start an AI conversation anchored to a selection inline, without opening the chat panel.
- [ ] **Workspace Trust / Security Sandbox** — Restrict plugin/terminal/file access in untrusted repositories.
- [ ] **Settings Sync Across Machines** — Cloud or git-backed sync for settings, keybindings, themes, plugins.
- [ ] **Multi-Location AI Edits** — AI edits multiple places simultaneously; accept/reject each location individually.
- [ ] **Dev Container Support** — Develop inside Docker/devcontainer environments with local editor + remote language servers.

### 13.3 Medium — Quality of Life

- [ ] **Token Usage Dashboard** — Real-time and historical token usage, estimated cost per session, budget limits.
- [ ] **Smart Paste** — AI transforms pasted code to match target language/context (e.g., Python → Rust).
- [ ] **Terminal Output Decorations** — Inline actions on command output: re-run, copy, open file from stack trace, explain error with AI.
- [ ] **Local File History (Timeline)** — Auto-snapshot on every save independent of git; diff and restore any prior version.
- [ ] **Linked Tag Editing** — HTML/JSX opening+closing tag sync editing via tree-sitter.
- [ ] **AI Test Generation** — Analyze code coverage, identify untested paths, generate targeted tests for uncovered branches.
- [ ] **TODO/FIXME Aggregation Panel** — Scan workspace for TODO/FIXME/HACK/XXX tags with navigable list.
- [ ] **FIM Completions (Fill-in-the-Middle)** — Inline suggestions aware of both prefix and suffix context.
- [ ] **Dependency Vulnerability Scanning** — Scan lock files (Cargo.lock, package-lock.json) for known CVEs with fix suggestions.
- [ ] **Voice Input** — Dictate intent or code via microphone for accessibility and rapid prototyping.
- [ ] **Database / SQL Client** — Connect to databases, run queries, view schemas with AI SQL assistance.

### 13.4 Low — Polish & Differentiation

- [ ] **AI Memory Across Sessions** — AI remembers user preferences, coding style, corrections without explicit rules files.
- [ ] **Refactoring Preview Panel** — Before executing refactoring (extract function, inline variable), show full preview of affected files.
- [ ] **Nerd Font / Icon Support** — Render file type icons in file tree and tabs using Nerd Font glyphs with fallback.
- [ ] **Predictive Command Palette** — Learn from usage frequency/recency, surface likely commands first with ML ranking.
- [ ] **Shared AI Chat in Collab** — In collaborative sessions, AI conversation visible to all peers.
