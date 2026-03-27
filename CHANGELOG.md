# Changelog

All notable changes to AURA will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.7] - 2026-03-27

### Added

- **Split panes** — Vertical (`:vsplit`) and horizontal (`:hsplit`) editor splits. View two files side-by-side with independent scroll, minimap, and git markers. `Ctrl+W` toggles focus. `:only` closes the split.

## [0.1.6] - 2026-03-27

### Added

- **Split panes** — Vertical (`:vsplit`) and horizontal (`:hsplit`) editor splits. View two files or the same file side-by-side. `Ctrl+W` toggles focus between panes. `:only` closes the split. Each pane has its own title, border highlighting, minimap, and scroll position.
- **Settings modal** — Interactive settings overlay (`Ctrl+,` or `:settings`). Toggle editor options live: minimap, line numbers, authorship markers, tab settings, scroll margin, conversation compaction, update checker. Changes apply immediately.
- **Optional minimap** — The scrollbar minimap can now be toggled on/off via the settings modal or `show_minimap` in `aura.toml`.

## [0.1.5] - 2026-03-27

### Changed

- **CrdtDoc API** — `new()`, `splice()`, and `text()` now return `Result` instead of panicking. Buffer operations gracefully handle CRDT failures.

### Fixed

- **Code quality** — Removed all `unwrap()` from library code. Replaced with proper error handling, `Result` propagation, or descriptive `expect()` for provably-safe operations across 8 files.

## [0.1.4] - 2026-03-27

### Added

- **Conversation compaction** — Configurable retention policies for the conversation database via `[conversations]` in `aura.toml`. Auto-compact on startup, `:compact` command for manual cleanup. Deletes old messages, trims per-conversation history, removes excess conversations.
- **AI conversation summarization** — Long conversations are automatically summarized by Claude in the background. Summaries replace old messages as context for future AI calls.
- **Context window management** — Chat panel caps context messages sent to AI per turn (default: 40), preventing unbounded memory and token growth.
- **AI commit message button** — Sparkle (`✨`) button on the "Commit Message" header in the git panel. Generates commit messages from staged diff with streaming preview.
- **Stage all button** — Green `+` button on the "Changes" header in the git panel. Also available via `Shift+S`.

### Changed

- **CrdtDoc API** — `new()`, `splice()`, and `text()` now return `Result` instead of panicking on error. All buffer operations gracefully handle CRDT failures.

### Fixed

- **Update checker** — `:update` command now forces a fresh GitHub API check instead of returning stale cached results.
- **Code quality** — Removed all `unwrap()` calls from library code (convention: unwrap only in tests). Replaced with proper error handling, `Result` propagation, or descriptive `expect()` for provably-safe operations. Affected: crdt.rs, buffer.rs, app.rs, collab.rs, mcp_server.rs, mcp_client.rs, chat_panel.rs, client.rs.

## [0.1.3] - 2026-03-27

### Added

- **Stage all button** — Green `+` button on the "Changes" header in the git source control panel. Click to stage all unstaged files at once. Also available via `Shift+S` keyboard shortcut.
- **AI commit message button** — Sparkle (`✨`) button on the "Commit Message" header in the git panel. Click to generate a commit message from staged changes using AI. The message streams into the commit message box in real-time for review before committing. Also available via `:commit` / `:gc` commands.
- **Conversation compaction** — Configurable retention policies for the conversation database via `[conversations]` in `aura.toml`. Auto-compact on startup deletes old messages, trims per-conversation history, and removes excess conversations. Manual compaction via `:compact` command.
- **AI conversation summarization** — Long conversations are automatically summarized by Claude in the background. Summaries replace old messages as context for future AI calls, keeping the database lean and API calls efficient.
- **Context window management** — Chat panel limits context messages sent to AI per turn (default: 40), preventing unbounded memory growth during long sessions.

### Fixed

- **Update checker**: `:update` command now forces a fresh GitHub API check instead of returning stale cached results. Shows status message for all check outcomes (available, up-to-date, error).

## [0.1.2] - 2026-03-27

### Added

- **Real-time collaborative editing** — Multiple AURA instances can edit the same file over TCP with automerge CRDT conflict-free merging. Colored peer cursors with name labels, selection highlighting, automatic reconnection with exponential backoff, and incremental rope reconciliation. Start with `--host` / `--join` CLI flags or `:host` / `:join` commands.
- **Multi-file collaborative sessions** — Host shares all open files in a single session. Clients auto-open tabs for each file. Sync messages routed by file identifier. Peer cursors filtered to the active tab.
- **Tab close buttons** — Clickable `×` on each tab with save/discard/cancel confirmation dialog for unsaved changes. Tab bar now always visible.
- **`AuthorId::Peer` variant** — Remote human peers tracked with unique colors (6-color rotating palette).
- **Collaborative editing documentation** — New user guide page, updated architecture docs, README, and CONTRIBUTING.

## [0.1.1] - 2026-03-27

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
- **In-app update checker** — background check against GitHub Releases API with floating notification toast, interactive update modal (Y/N), and `:update` / `:version` commands.
- **Mouse support** — click-to-position, click-and-drag visual selection, scroll wheel, clickable sidebar tabs (Files/Git), clickable git panel entries, clickable editor tab bar.
- **Quit-all commands** — `:qa`, `:qa!`, `:wqa` for closing all tabs at once.
- **Find and replace** with regex support.
- **Release automation** via cargo-dist — GitHub Actions, shell installer, Homebrew formula, cross-platform builds (macOS, Linux, Windows).
- **Real-time collaborative editing** — Multiple AURA instances can edit the same file over TCP. Automerge CRDT handles conflict-free merging. Colored peer cursors with name labels, selection highlighting, automatic reconnection with exponential backoff, and incremental rope reconciliation for performance. Start with `--host` / `--join` CLI flags or `:host` / `:join` commands.
- **Tab close buttons** — Clickable close button on each tab in the tab bar with save/discard confirmation dialog for unsaved changes.
- **Comprehensive documentation** — mdBook user guide, architecture docs, API reference, all deployed to GitHub Pages.

[0.1.1]: https://github.com/odtorres/aura/releases/tag/v0.1.1
