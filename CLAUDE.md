# CLAUDE.md — AURA Editor Project Conventions

## Project Overview

AURA (AI-native Universal Reactive Authoring) is a terminal text editor built from the ground up for human + AI co-authoring. Written in Rust, it uses a rope-based buffer with CRDT authorship tracking, a ratatui TUI, and will integrate with AI via the Anthropic API and MCP protocol.

## Architecture

```
crates/
├── core/      # Buffer (rope + CRDT), cursor, authorship, edit history
├── tui/       # Rendering (ratatui), input handling, app state machine
├── ai/        # Anthropic API client, context assembly, MCP (Phase 2+)
└── editor/    # Binary entry point, CLI arg parsing, terminal setup
```

### Crate dependency graph

```
editor → tui → core
           ↘ ai → core
```

`core` has no dependency on any other workspace crate. `tui` depends on `core`. `ai` depends on `core`. `editor` wires everything together.

## Current Phase

**Phase 0: Foundation — Minimal Viable Editor**

The immediate goal is a working terminal editor that can open, edit, and save files with vim-like modal editing. No AI integration yet.

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

The buffer uses `ropey` (rope data structure) for efficient text manipulation. Every edit is tagged with an `AuthorId` (human or AI agent). In Phase 1, `automerge` (CRDT) will be layered on top for conflict-free multi-author editing.

### Modal editing

Vim-inspired but not a vim clone. We implement the essential modes (Normal, Insert, Command, Visual, Intent) without trying to replicate vim's full command language. The goal is familiar ergonomics, not compatibility.

### AI integration pattern (Phase 2+)

The core editing loop will be: **express intent → AI proposes → human reviews → accept/reject/refine**. AI edits stream in via the Anthropic API and are rendered as ghost text or in a diff view. All AI changes are tracked separately in the CRDT so they can be undone independently.

## File Structure Reference

```
aura-editor/
├── Cargo.toml              # Workspace manifest
├── CLAUDE.md               # This file
├── TODO.md                 # Full project roadmap
├── README.md               # User-facing documentation
├── LICENSE                 # MIT
├── .gitignore
└── crates/
    ├── core/
    │   └── src/
    │       ├── lib.rs      # Re-exports
    │       ├── buffer.rs   # Rope-based buffer with authorship
    │       ├── cursor.rs   # Cursor and selection
    │       └── author.rs   # AuthorId and Author types
    ├── tui/
    │   └── src/
    │       ├── lib.rs      # Re-exports
    │       ├── app.rs      # App state, mode, event loop
    │       ├── render.rs   # TUI drawing (ratatui)
    │       └── input.rs    # Key handling per mode
    ├── ai/
    │   └── src/
    │       ├── lib.rs      # Config, re-exports
    │       ├── client.rs   # Anthropic API client (stub)
    │       └── context.rs  # Editor context assembly (stub)
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
| automerge (future) | CRDT for multi-author conflict-free editing               |
| tree-sitter (future)| Incremental syntax parsing                               |
| reqwest (future)   | HTTP client for Anthropic API                             |
| rusqlite (future)  | Embedded DB for conversation/decision history             |
| gitoxide (future)  | Pure Rust git operations                                  |
