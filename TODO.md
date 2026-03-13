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
- [ ] Undo tree visualization (simple text-based, in a split pane)
- [x] Keybinding: `u` for global undo, `<leader>u` for AI-only undo

### 1.3 Change provenance UI
- [x] Gutter markers: color-coded by author (human = green, AI = blue)
- [ ] Inline ghost-text for pending AI suggestions (dimmed/italic)
- [x] Toggle: show/hide authorship highlighting (`<leader>a`)
- [x] Status bar shows "last change by: human | 2s ago"

### Phase 1 Definition of Done
> Can distinguish human vs AI edits visually. Can undo AI changes independently.
> CRDT operations benchmarked under 1ms per edit on a 10K line file.

---

## Phase 2: AI Integration — The Core Loop (Weeks 7–10)

This is the heart of AURA: the intent → propose → review → accept cycle.

### 2.1 Anthropic API client
- [ ] Create `ai` crate in workspace
- [ ] Implement streaming API client for Claude (Anthropic API, reqwest + tokio)
- [ ] Handle API key from env var (`ANTHROPIC_API_KEY`) or config file
- [ ] Token counting and context window management
- [ ] Retry logic with exponential backoff
- [ ] Rate limiting awareness

### 2.2 Context assembly
- [ ] Send current buffer content as context
- [ ] Include cursor position and selection
- [ ] Include file path, language, and project structure
- [ ] Include Tree-sitter syntax node at cursor (see Phase 3)
- [ ] Include recent edit history (last N changes with authorship)
- [ ] Include relevant diagnostics from LSP (see Phase 3)
- [ ] Truncation strategy: prioritize code near cursor, summarize distant code

### 2.3 Intent mode
- [ ] New mode: Intent mode (triggered by `<leader>i` or `:intent`)
- [ ] User types natural language intent: "handle errors in this function"
- [ ] Intent is sent to AI with full context
- [ ] AI response streams in as a structured diff/edit proposal
- [ ] Parse AI response into concrete edit operations

### 2.4 Review interface
- [ ] Split view: current code (left) vs proposed code (right)
- [ ] Inline diff highlighting: additions (green), deletions (red), modifications (yellow)
- [ ] Per-hunk accept/reject: `a` to accept hunk, `r` to reject, `A` to accept all
- [ ] Edit-in-place: modify the proposal before accepting
- [ ] Keybinding: `<leader>rr` to request revision with follow-up intent
- [ ] Animated streaming: show AI edits appearing character by character

### 2.5 Quick actions (no review needed)
- [ ] Inline completion: Tab to accept ghost-text suggestion
- [ ] `<leader>e` — Explain selected code (shown in floating pane)
- [ ] `<leader>f` — Fix diagnostic at cursor
- [ ] `<leader>t` — Generate test for function at cursor
- [ ] These use the same AI pipeline but with pre-built intents

### Phase 2 Definition of Done
> Can select code, express intent, review AI proposal in a diff view,
> and accept/reject per hunk. Streaming feels responsive. Edits are tracked as AI-authored.

---

## Phase 3: Semantic Awareness (Weeks 11–14)

The editor understands code structure, not just text.

### 3.1 Tree-sitter integration
- [ ] Integrate `tree-sitter` crate with Rust bindings
- [ ] Incremental parsing on every edit (must handle AI streaming edits)
- [ ] Syntax highlighting using Tree-sitter queries
- [ ] Expose syntax tree to AI context: current node, parent, scope
- [ ] Language grammars: start with Rust, TypeScript, Python, Go
- [ ] Highlight groups configurable via theme file (TOML)

### 3.2 LSP client
- [ ] Implement LSP client (consider `tower-lsp` or custom over JSON-RPC)
- [ ] Diagnostics: show errors/warnings in gutter + floating window
- [ ] Go to definition, references, hover info
- [ ] Code actions: integrate with AI (AI can trigger code actions or vice versa)
- [ ] Feed LSP diagnostics into AI context automatically

### 3.3 Semantic graph
- [ ] Build a lightweight dependency graph from Tree-sitter + LSP data
- [ ] Track: which functions call which, which tests cover which functions
- [ ] When AI proposes a change, show "affected by this change: X, Y, Z"
- [ ] Feed graph info to AI: "this function is called by 3 other functions"

### Phase 3 Definition of Done
> Syntax highlighting via Tree-sitter. LSP diagnostics and navigation working.
> AI context includes structural info. User can see impact of proposed changes.

---

## Phase 4: Conversation Layer (Weeks 15–18)

Code and conversation are interleaved, not separate.

### 4.1 Persistent conversation storage
- [ ] Integrate `rusqlite` for local SQLite database
- [ ] Schema: conversations, messages, intents, edit_decisions
- [ ] Link every conversation to file path + line range + git commit
- [ ] Full-text search over conversation history

### 4.2 Inline conversation
- [ ] Attach conversations to code ranges (like comments, but richer)
- [ ] Toggle visibility: show/hide conversation annotations
- [ ] Keybinding: `<leader>cc` to see conversation history for current line/function
- [ ] "Why was this written this way?" → retrieves originating conversation

### 4.3 Decision log
- [ ] Every accept/reject decision is logged with context
- [ ] Queryable: "show me all rejected AI suggestions this week"
- [ ] Pattern detection: "AI keeps suggesting X, you keep rejecting — adjust?"

### Phase 4 Definition of Done
> Can retrieve the conversation that led to any piece of code.
> Decision history is queryable. Conversation persists across sessions.

---

## Phase 5: MCP Protocol Layer (Weeks 19–22)

AURA becomes a platform, not just an editor.

### 5.1 MCP server
- [ ] AURA exposes MCP server over WebSocket (localhost)
- [ ] Tools exposed: `read_buffer`, `edit_buffer`, `get_diagnostics`,
      `get_selection`, `get_cursor_context`, `get_conversation_history`
- [ ] Resources exposed: open files, project structure, git status
- [ ] Any MCP client (Claude Code, custom agents) can connect

### 5.2 MCP client
- [ ] AURA can connect to external MCP servers
- [ ] Integrate with filesystem, git, and custom tool servers
- [ ] Configuration: `aura.toml` defines MCP server connections

### 5.3 Multi-agent support
- [ ] Multiple AI agents can connect simultaneously
- [ ] Each agent gets its own author ID in the CRDT
- [ ] Agent activity shown in a "collaborators" panel
- [ ] Conflict resolution: if two agents edit the same region, human decides
- [ ] Agent orchestration: "Agent A handles tests, Agent B handles implementation"

### Phase 5 Definition of Done
> Claude Code can connect to AURA via MCP and read/edit buffers.
> Multiple agents can work simultaneously with CRDT conflict resolution.

---

## Phase 6: Speculative Execution (Weeks 23–26)

The AI thinks ahead. The human reviews when ready.

### 6.1 Background analysis
- [ ] On file open / cursor move, queue background AI analysis
- [ ] Debounce: don't spam the API, batch nearby changes
- [ ] Priority queue: analyze code near cursor first
- [ ] Cache results: don't re-analyze unchanged code

### 6.2 Ghost suggestions
- [ ] Render AI improvement suggestions as ghost text overlay
- [ ] Multiple suggestions per location (cycle with `<Alt+]>` / `<Alt+[>`)
- [ ] Categories: refactor, simplify, add error handling, performance
- [ ] Configurable aggressiveness: minimal / moderate / proactive

### 6.3 Multi-file awareness
- [ ] When a change is accepted, AI proactively checks related files
- [ ] "This change might require updating `tests/foo_test.rs` — want me to?"
- [ ] Proposed cross-file changes shown as a changeset (like a PR)
- [ ] Accept/reject at changeset level or per-file

### Phase 6 Definition of Done
> AI proactively suggests improvements as ghost text.
> Cross-file changes proposed as atomic changesets. Background analysis doesn't block UI.

---

## Phase 7: Git Integration (Weeks 27–30)

### 7.1 Git awareness
- [ ] Integrate `gitoxide` for native Rust git operations
- [ ] Gutter: show git diff status (added/modified/deleted lines)
- [ ] Inline blame with authorship (including AI authorship from CRDT)
- [ ] Commit from within editor with AI-generated commit messages

### 7.2 Conversation-linked commits
- [ ] Attach conversation summaries to git commits as trailers
- [ ] `git log --aura` (custom formatter) shows intent history
- [ ] Link between git blame and conversation history

### 7.3 Branch management
- [ ] Visual branch switcher
- [ ] AI can propose changes on a feature branch without touching main
- [ ] "Experimental" mode: AI works on a branch, human reviews the PR

---

## Phase 8: Polish and Ecosystem (Weeks 31+)

### 8.1 Configuration
- [ ] `aura.toml` for all settings: theme, keybindings, AI model, aggressiveness
- [ ] Theme engine: color schemes in TOML (ship with at least 3 themes)
- [ ] Keybinding customization layer

### 8.2 Performance
- [ ] Profile with `flamegraph` crate
- [ ] Target: <1ms keystroke-to-render latency
- [ ] Target: <16ms frame time for streaming AI output
- [ ] Target: handles 100K+ line files without lag
- [ ] Memory profiling: ensure CRDT history doesn't grow unbounded (compact on save)

### 8.3 Plugin system (future)
- [ ] Lua or WASM plugin API (evaluate tradeoffs)
- [ ] Plugins can register new intents, modes, and UI panels
- [ ] Ship with core plugins: file picker (fuzzy finder), file tree, terminal

### 8.4 Distribution
- [ ] `cargo install aura-editor`
- [ ] Homebrew formula
- [ ] AUR package
- [ ] AppImage for Linux
- [ ] Release automation with `cargo-dist`

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

## Open Questions

- [ ] Should the CRDT layer sit between the rope and the renderer, or wrap the rope?
- [ ] Automerge vs Yrs (Yjs) — need to benchmark both for this use case
- [ ] Lua vs WASM for the future plugin system — Lua is simpler, WASM is more universal
- [ ] How aggressive should the speculative execution be by default?
- [ ] Should AURA support Vim emulation deeply, or define its own keybinding paradigm?
- [ ] How to handle very long conversations — summarize and compact, or paginate?
