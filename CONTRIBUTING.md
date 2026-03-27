# Contributing to AURA

Thank you for your interest in contributing to AURA!

## Getting Started

1. Fork and clone the repository
2. Install Rust 1.75+ via [rustup](https://rustup.rs/)
3. Build: `cargo build --workspace`
4. Run tests: `cargo test --workspace`

## Development Guide

See the full [Development Guide](https://odtorres.github.io/aura/contributing/development.html) on the documentation site, which includes:

- Code conventions and naming standards
- Commit message format
- Testing requirements
- Performance targets

The same information is available in [CLAUDE.md](CLAUDE.md) in the repository root.

## Quick Reference

```bash
# Build
cargo build --workspace

# Test
cargo test --workspace

# Lint
cargo clippy --workspace -- -W clippy::all

# Format
cargo fmt --all

# Run the editor
cargo run -p aura -- <file>
```

## Project Structure

```
crates/
├── core/      # Buffer, cursor, authorship, CRDT, conversations
├── tui/       # Rendering, input, LSP, MCP, git, plugins
├── ai/        # Anthropic API client, context assembly
└── editor/    # Binary entry point
```

## Testing Collaborative Editing

To test real-time collaboration locally:

```bash
# Terminal 1: host a session
cargo run -p aura -- test_file.rs --host

# Terminal 2: join (port is shown in the host's status bar)
cargo run -p aura -- --join 127.0.0.1:<port>
```

Both instances will sync edits in real-time. The collab module (`crates/tui/src/collab.rs`) uses the same thread + mpsc channel pattern as the MCP server — see `mcp_server.rs` for reference.

## Submitting Changes

- Small, focused commits — one logical change per commit
- Commit message format: `<crate>: <description>` (e.g., `core: add word-jump cursor movement`)
- Run `cargo fmt` and `cargo clippy` before committing
- Run `cargo test --workspace` to ensure nothing is broken
- Every public function must have a doc comment
