# CLAUDE.md — AURA Editor Project Conventions

## Project Overview

AURA (AI-native Universal Reactive Authoring) is a terminal text editor built from the ground up for human + AI co-authoring. Written in Rust, it uses a rope-based buffer with CRDT authorship tracking, a ratatui TUI, and integrates with AI via the Anthropic API and MCP protocol.

## Architecture

```
crates/
├── core/      # Buffer (rope + CRDT), cursor, authorship, edit history
├── tui/       # Rendering (ratatui), input handling, app state machine
├── ai/        # Anthropic API client, context assembly
└── editor/    # Binary entry point, CLI arg parsing, terminal setup
```

### Crate dependency graph

```
editor → tui → core
          ↓
          ai → core
```

`core` has no dependency on any other workspace crate. `tui` depends on `core` and `ai`. `ai` depends on `core`. `editor` wires everything together.

## Current Phase

**Post-Phase 9: Polish & Distribution**

AURA is a fully-featured terminal editor with vim-like modal editing, AI co-authoring (Anthropic API), CRDT multi-author tracking, tree-sitter syntax highlighting, LSP integration, MCP server/client, embedded terminal, git integration, plugin system, and semantic indexing. Remaining work focuses on distribution packaging (Phase 8.5 unchecked items in `TODO.md`).

See `TODO.md` for the full roadmap.

## Code Conventions

### General

- **Every public function must have a doc comment** (`///`).
- Use `Result` types for anything that can fail. Buffer operations must never panic.
- Prefer `anyhow::Result` in application code, `thiserror` for library error types.
- Keep functions small and focused. If a function exceeds ~50 lines, consider splitting.
- Use `tracing` for logging, not `println!` or `eprintln!`.

### Naming

- Crate names: `aura-core`, `aura-tui`, `aura-ai` (kebab-case).
- Module names: `snake_case`.
- Types: `PascalCase`.
- Functions and variables: `snake_case`.
- Constants: `SCREAMING_SNAKE_CASE`.

### Rust Style

- Run `cargo fmt` before every commit.
- Run `cargo clippy -- -W clippy::all` and fix all warnings.
- No `unwrap()` in library code. `unwrap()` is acceptable only in tests.
- Use `saturating_sub`, `checked_add`, etc. for arithmetic on indices — never risk overflow.
- Prefer iterators over manual loops where it improves clarity.

### Testing

- All buffer operations in `core` must have unit tests.
- Use `proptest` for property-based testing on the buffer (random insert/delete sequences should never corrupt state).
- Use `insta` for snapshot testing of TUI output when we add it.
- Run `cargo test --workspace` before committing.

### Commits

- Small, focused commits. One logical change per commit.
- Commit message format: `<crate>: <description>` (e.g., `core: add word-jump cursor movement`).
- When adding a new dependency, justify it in the commit message body.

### Performance

- CRDT operations must be benchmarked when modified (target: <1ms per edit on 10K line file).
- Keystroke-to-render latency target: <1ms.
- Frame time target for streaming AI output: <16ms.
- Never block the main event loop with synchronous I/O or network calls.

## Key Design Decisions

### Buffer architecture

The buffer uses `ropey` (rope data structure) for efficient text manipulation. Every edit is tagged with an `AuthorId` (human or AI agent). Automerge (CRDT) is layered on top for conflict-free multi-author editing.

### Modal editing

Vim-inspired but not a vim clone. We implement the essential modes (Normal, Insert, Command, Visual, VisualLine, Intent, Review, Diff) without trying to replicate vim's full command language. The goal is familiar ergonomics, not compatibility.

### AI integration pattern

The core editing loop is: **express intent → AI proposes → human reviews → accept/reject/refine**. AI edits stream in via the Anthropic API and are rendered as ghost text or in a diff view. All AI changes are tracked separately in the CRDT so they can be undone independently.

## File Structure Reference

```
aura-editor/
├── Cargo.toml              # Workspace manifest
├── CLAUDE.md               # This file
├── TODO.md                 # Full project roadmap
├── README.md               # User-facing documentation
├── CONTRIBUTING.md         # Contributor guide
├── LICENSE                 # MIT
├── .gitignore
├── docs/                   # mdBook documentation site
└── crates/
    ├── core/
    │   └── src/
    │       ├── lib.rs          # Re-exports
    │       ├── buffer.rs       # Rope-based buffer with authorship
    │       ├── cursor.rs       # Cursor and selection
    │       ├── author.rs       # AuthorId and Author types
    │       ├── crdt.rs         # CrdtDoc wrapping automerge
    │       ├── conversation.rs # SQLite-backed conversation/decision history
    │       └── semantic.rs     # Lightweight dependency graph
    ├── tui/
    │   └── src/
    │       ├── lib.rs                  # Re-exports
    │       ├── app.rs                  # App state, mode, event loop
    │       ├── render.rs               # TUI drawing (ratatui)
    │       ├── input.rs                # Key handling per mode
    │       ├── config.rs               # aura.toml loading, theme engine
    │       ├── tab.rs                  # Tab/multi-buffer management
    │       ├── highlight.rs            # Tree-sitter syntax highlighting
    │       ├── file_tree.rs            # Directory sidebar
    │       ├── file_picker.rs          # Fuzzy file finder overlay
    │       ├── git.rs                  # Git integration (gitoxide)
    │       ├── source_control.rs       # Source control sidebar panel
    │       ├── diff_view.rs            # Side-by-side diff rendering
    │       ├── conversation_history.rs # Conversation history panel
    │       ├── lsp.rs                  # LSP client
    │       ├── mcp_server.rs           # MCP server (editor tools/resources)
    │       ├── mcp_client.rs           # MCP client (external servers)
    │       ├── speculative.rs          # Background AI analysis/ghost suggestions
    │       ├── semantic_index.rs       # Semantic indexer
    │       ├── plugin.rs               # Plugin trait and PluginManager
    │       └── embedded_terminal.rs    # PTY terminal (portable-pty + VTE)
    ├── ai/
    │   └── src/
    │       ├── lib.rs      # Config, re-exports
    │       ├── client.rs   # Anthropic API streaming client
    │       └── context.rs  # Editor context assembly
    └── editor/
        └── src/
            └── main.rs     # Entry point, terminal setup
```

## Useful Commands

```bash
# Build everything
cargo build --workspace

# Run the editor
cargo run -p aura -- <file>

# Run all tests
cargo test --workspace

# Lint
cargo clippy --workspace -- -W clippy::all

# Format
cargo fmt --all

# Check for unused dependencies
cargo +nightly udeps --workspace
```

## Dependencies Rationale

| Crate              | Why                                                       |
|--------------------|-----------------------------------------------------------|
| ropey              | Battle-tested rope for text buffers                       |
| ratatui            | Dominant Rust TUI framework, immediate-mode rendering     |
| crossterm          | Cross-platform terminal abstraction                       |
| tokio              | Async runtime for concurrent AI streams + user input      |
| automerge          | CRDT for multi-author conflict-free editing               |
| tree-sitter        | Incremental syntax parsing                                |
| tree-sitter-*      | Language grammars (Rust, Python, TypeScript, Go)          |
| reqwest            | HTTP client for Anthropic API (streaming)                 |
| tokio-stream       | Async stream utilities for AI response streaming          |
| rusqlite           | Embedded DB for conversation/decision history             |
| gix (gitoxide)     | Pure Rust git operations                                  |
| serde / serde_json | Serialization for config, API payloads, MCP messages      |
| toml               | Config file parsing (aura.toml)                           |
| uuid               | Unique IDs for authors, conversations                     |
| portable-pty       | PTY allocation for embedded terminal                      |
| vte                | VT terminal state machine for embedded terminal           |
| anyhow / thiserror | Error handling (application / library)                    |
| proptest           | Property-based testing for buffer operations              |
| criterion          | Benchmarking for render and CRDT performance              |
