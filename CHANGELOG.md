# Changelog

All notable changes to AURA will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-03-25

### Added

- **Modal editor** with Vim-inspired modes: Normal, Insert, Command, Visual, Visual Line, Intent, Review, Diff.
- **Rope-based buffer** (`ropey`) with CRDT authorship tracking (`automerge`) for conflict-free multi-author editing.
- **AI co-authoring** via Anthropic API — Intent mode for expressing edits in natural language, Review mode for accepting/rejecting AI proposals, ghost text suggestions from speculative background analysis.
- **Interactive chat panel** — multi-turn conversational AI with tool execution (read/edit buffer, diagnostics) and approval flow.
- **Tree-sitter syntax highlighting** for Rust, Python, TypeScript, and Go.
- **LSP client** — diagnostics, hover, go-to-definition, code actions, document symbols.
- **MCP protocol** — built-in MCP server exposing editor tools/resources, MCP client for connecting to external servers, Claude Code bridge integration.
- **Git integration** — diff markers, inline blame, staging/unstaging, committing, branch management, Aura-Conversation commit trailers.
- **Source control panel** — sidebar for reviewing and staging changes.
- **Embedded terminal** — PTY-backed terminal pane with full VT emulation.
- **File tree sidebar** with directory navigation.
- **Fuzzy file picker** overlay.
- **Tab management** — multi-buffer editing with tab bar.
- **Session persistence** — open tabs, cursor positions, scroll offsets, and UI layout saved on exit and restored on reopen.
- **Conversation history** — SQLite-backed storage of all AI interactions with searchable history panel.
- **Semantic indexer** — lightweight dependency graph for cross-file awareness.
- **Plugin system** — trait-based plugin architecture with manager.
- **Configuration** via `aura.toml` — themes (dark, light, monokai, custom), keybindings, AI settings, editor preferences.
- **In-app update checker** — background check against GitHub Releases API with status bar indicator and `:update` command.
- **Mouse support** — click-to-position, click-and-drag selection, scroll wheel.
- **Find and replace** with regex support.
- **Release automation** via cargo-dist — GitHub Actions, shell installer, Homebrew formula, cross-platform builds (macOS, Linux, Windows).

[0.1.0]: https://github.com/odtorres/aura/releases/tag/v0.1.0
