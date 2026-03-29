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
- [x] `cargo install aura-editor`
- [x] Homebrew formula
- [x] AUR package
- [x] AppImage for Linux
- [x] Release automation with `cargo-dist`

### 8.5 Distribution — Remaining Manual Steps
- [ ] Create `aura-editor/homebrew-tap` repo on GitHub (needed for `brew install aura-editor/tap/aura`)
- [ ] Add `HOMEBREW_TAP_TOKEN` secret to the aura repo (GitHub Settings > Secrets) — a PAT with write access to the tap repo
- [ ] Cut a test release to verify the full pipeline: `git tag v0.1.0-rc.1 && git push --tags`
  - [ ] Verify GitHub Release is created with binaries for all 5 targets
  - [ ] Verify shell installer works: `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/odtorres/aura/releases/latest/download/aura-installer.sh | sh`
  - [ ] Verify `brew install aura-editor/tap/aura` works after tap repo is set up
- [ ] Publish to crates.io: `cargo publish -p aura`
- [ ] Verify `cargo install aura` works after crates.io publish
- [x] detect in the app a new version and show there is a new version available


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

### 10.7 Potential future work
- [x] Visual block mode (Ctrl+V column editing)
- [ ] Wire TLS into actual streams (needs reader/writer refactor)
- [ ] More LSP features (rename, references panel)
- [ ] Undo tree visualization
- [x] Multi-cursor editing
- [x] Snippet system
- [x] Integrated debugger (DAP protocol)