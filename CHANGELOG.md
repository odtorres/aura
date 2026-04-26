# Changelog

All notable changes to AURA are documented here. Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

---

## [1.2.8] — 2026-04-25

### Changed
- **Diff view collapses empty pane** — when viewing a pure addition (new file)
  or pure deletion, the empty side (red for adds, green for deletes) is no
  longer rendered. The populated pane expands to fill the full width, so half
  the screen is no longer wasted on a blank gutter.

### Internal
- Cleared the last clippy warnings from the workspace: replaced
  `.get(&x).is_none()` with `!contains_key(&x)` in `git.rs` test helpers,
  reordered the `mod tests` block to follow trailing helpers in `render.rs`,
  and replaced `.expect(&format!(...))` with `.unwrap_or_else(|| panic!(...))`
  in `session.rs`. `cargo clippy --workspace --all-targets -- -W clippy::all`
  is now warning-free.

## [1.2.7] — 2026-04-17

### Performance
- **Bracket-depth cache** — rainbow bracket colouring no longer scans the whole file every frame; depths are cached on `EditorTab` and rebuilt only on buffer edits. Saves an O(total_lines) character scan per render event.
- **Removed duplicate sticky-scroll and breadcrumb rendering** in `draw_editor`. The per-pane layer already renders both from cached `foldable_ranges` and `app.breadcrumbs`; the inner duplicates were calling `enclosing_scopes()` with a full `rope().to_string()` clone every frame. Also fixes a latent UX bug where sticky scope headers could render twice.
- **Debounced syntax highlighting** — `refresh_highlights` now waits 40 ms after the last edit before re-parsing, so a continuous keystroke burst no longer triggers a full-file tree-sitter reparse per tick.
- **Bounded undo history** — `Buffer` caps its undo history at 1000 entries and drops the oldest when the cap is reached, so long editing sessions don't grow memory without bound.
- **Idle CRDT compaction** — after 30 s of no edits, each tab compacts its CRDT history without waiting for save. Previously long unsaved sessions would accumulate CRDT deltas indefinitely.

### Added
- New criterion benches: `render_frame_100k_lines` and `scroll_and_render_100k` guard the per-frame hot path on large files.

### Hardened
- Replaced fragile "check is_some then unwrap" pattern in diff-scroll and harpoon key handlers with `if let Some(...)` guards. No behavior change; removes six `.unwrap()` calls from the input hot path.

### Changed
- **Filesystem watching** — replaced the 2 s mtime polling loop with a native `notify` file watcher (kqueue on macOS, inotify on Linux, ReadDirectoryChangesW on Windows). External file edits and `aura.toml` hot-reload now take effect near-instantly instead of up to 2 s later. The mtime poll is kept as a fallback when the native backend fails to start.
- **Deferred LSP startup** — language servers (rust-analyzer, jdtls, etc.) now spawn on a background thread. Opening a file no longer blocks the UI for 1–2 s while the server initialises; the editor comes up immediately and LSP features activate as soon as the handshake completes.
- **Deferred RAG index build** — codebase RAG indexing (walks every file in the project) now runs on a background thread. On large repositories startup no longer blocks on the walk; the index becomes available to AI features on the first tick after the build completes.

### Internal
- First wedge of the `app.rs` monolith split: the file moves to `app/mod.rs`, with lifecycle helpers (filesystem watcher glue, RAG readiness poll, config hot-reload) extracted to `app/lifecycle.rs` and update-check plumbing extracted to `app/updates.rs`. No behavior change; establishes the sub-module pattern that future splits can follow.
- Added 13 unit tests covering the two remaining untested UI modules: `undo_tree` (5 tests for modal navigation, detail toggle, history-position mapping, and entry-building) and `ai_visor` (8 tests for panel visibility, tab cycling, selection clamping, and tab-scoped path lookups).

## [1.2.6] — 2026-04-15

### Fixed
- **Diff view not showing changes** — tab-based diff rendering silently returned early because scroll clamping and re-borrow only checked the old overlay field, not the tab's diff
- **AI commit message generation broken with no error shown** — ghost suggestion status in the command bar was hiding all error/progress messages; Claude Code stream-json event parsing missed top-level API events; git errors in staged diff were silently swallowed
- **Open buffers not detecting external file changes** — `file_mtimes` cache was not updated after manual or auto-saves, causing false positives and missed real external modifications

### Added
- **File tree auto-refresh** — sidebar now polls every 2 seconds when visible, so new files created by AI tools, git, or external processes appear automatically without restart
- **Status bar priority for source control** — error and progress messages from commit generation now take priority over ghost suggestion text when the source control panel is focused

## [1.2.5] — 2026-04-13

### Fixed
- All clippy warnings resolved (8 fixes across 5 files)
- 281 tests pass, zero warnings, clean formatting

### Session milestone
**Commit #100** of this development session. 100 commits, 46 releases, 439 roadmap items complete.

## [1.2.4] — 2026-04-13

### Added
- **Clickable file tree action bar** — visual icons (+ ◻ ✎ ✕ ⧉ ⟳ ⊙) for new file, new folder, rename, delete, copy, refresh, reveal

## [1.2.3] — 2026-04-10

### Added
- **Auto-reload externally modified files** — polls every 2s, reloads if no unsaved changes, warns if buffer is dirty

## [1.2.2] — 2026-04-10

### Fixed
- **O(n²) paste performance** — bulk `Vec::splice()` instead of loop `Vec::insert()` for line_authors

## [1.2.1] — 2026-04-10

### Added
- **`:w <filename>` save-as** — save buffer to a new file path with directory creation
- **`:e <filename>` open/create** — open or create files with directory creation

## [1.2.0] — 2026-04-08

### Added
- 15 medium-priority features: extract variable, move symbol, safe delete, format selection, multi-cursor regex, type hierarchy, git worktree, commit signing, terminal shell profiles, panel configuration
- All 439 roadmap items across 14 phases checked off

## [1.1.3] — 2026-04-08

### Added
- Harpoon-style file marks (`<Space>1-4`), Emmet expansion, clipboard ring, CI/CD status, secret detection, Python venv, spell checking, color picker, file path copy, PR comments, quick open prefixes

## [1.1.2] — 2026-04-08

### Added
- Flash/leap two-char jump, treesitter text objects (`daf`/`dif`), extract method, workspace symbols, file picker `filename:42`, go to implementations, AI auto-debug, AI doc generation, task auto-detection

## [1.1.1] — 2026-04-08

### Added
- Which-key popup, incremental selection expand/shrink, @terminal and @diff mentions, auto-organize imports, terminal link detection

## [1.1.0] — 2026-04-08

### Added
- **Navigate back/forward (jump list)** — `:back`/`:forward` with 100-entry history
- **Crash recovery** — swap files auto-saved every 30s in `~/.aura/swap/`

## [1.0.2] — 2026-04-08

### Fixed
- Chat panel tool approval auto-scrolls to prompt with prominent box-drawing border
- Status bar shows "Tool approval needed" when AI requests permission
- Chat panel scroll speed improved (Ctrl+Up/Down 3 lines, mouse 3 lines, PageUp/Down 10)

## [1.0.1] — 2026-04-08

### Fixed
- Chat panel Ctrl+Up/Down scrolling speed (3 lines instead of 1)

## [1.0.0] — 2026-04-08

### Milestone
**AURA v1.0** — all 325 roadmap items complete across 13 development phases.

### Phase 13: Next-Gen AI Infrastructure
- **Codebase RAG Indexing** — TF-IDF semantic search across all source files (`rag_index.rs`)
- **Apply Model** — structured search/replace block parsing for reliable AI edits (`apply_model.rs`)
- **AI PR Review** — `:pr-review <N>` reviews GitHub PRs via AI and `gh` CLI
- **AI Checkpoints** — automatic snapshots before AI edits with `:checkpoint rollback <id>`
- **Context Pinning** — `:pin` files/notes as persistent AI context across conversations
- **Inline AI Chat** — `Ctrl+K` for cursor-anchored AI conversation
- **Workspace Trust** — `:trust on/off` security sandbox for untrusted repos
- **Settings Sync** — `:sync export/import` for cross-machine portability
- **Token Usage Dashboard** — `:tokens` shows request count, token usage, estimated cost
- **Local File History** — auto-snapshots on save with `:history` and `:history restore`
- **TODO/FIXME Panel** — `:todos` scans workspace for TODO/FIXME/HACK/XXX tags
- **Vulnerability Scanning** — `:vuln` runs `cargo audit` or `npm audit`

## [0.9.0–0.9.4] — 2026-04-07

### Added
- Phase 13 roadmap: 29 next-gen features from 2025-2026 editor landscape gap analysis
- All 29 features implemented across versions 0.9.1–0.9.4

## [0.8.0–0.8.2] — 2026-04-07

### Added
- **Bookmarks** — `:bookmark add/list/jump/delete` persistent across sessions (`bookmarks.rs`)
- **Project Templates** — `:new rust/react/python/node/go <name>` scaffolding via CLI tools
- **AI Code Explanation** — `<Space>e` sends selected code to AI chat for explanation
- **Split Terminal** — `:term split` side-by-side terminal panes
- **AI Settings Persistence** — provider, model, commit_model saved to `aura.toml`

## [0.7.0] — 2026-04-07

### Added
- **Multi-Provider AI** — Anthropic, OpenAI, and Ollama support (`openai_client.rs`, `ollama_client.rs`)
- **Per-Feature Model Config** — different models for commit, chat, agent, speculative features
- **Settings Modal AI Section** — provider/model/commit model selectors with live switching
- **Auto-Detection** — detects provider from `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `OLLAMA_HOST`

## [0.6.0–0.6.2] — 2026-04-06

### Added
- **HTTP Client** — `:http send` executes requests from `.http` files with `{{variable}}` support
- **Notebook/REPL** — `:cell run` / `:cell run-all` for `# %%` cells (Python, JS, Ruby, Bash)
- **AI Pair Programming** — `:pair on/off` toggle
- **Image Preview** — Kitty graphics protocol for PNG/JPG/GIF/SVG/WebP
- **Comprehensive Documentation** — 5 new doc pages, all keybindings documented
- **Testing & Stability** — 24 new tests across 8 modules, all clippy warnings fixed

## [0.5.0–0.5.7] — 2026-04-05 to 2026-04-06

### Added
- **Zen Mode** — `:zen` hides all chrome for distraction-free editing
- **Breadcrumbs** — scope path above editor from tree-sitter (file > class > function)
- **Sticky Scroll** — enclosing scope headers pinned at viewport top (up to 3 levels)
- **Markdown Live Preview** — `:preview` split pane with rendered headers, code blocks, lists, tables
- **Improved Minimap** — Unicode half-block characters (▀▄█) for 2x vertical density
- **Inline AI Completions** — ghost text in Insert mode, Tab to accept
- **AI Refactoring** — `:refactor <instruction>` for cross-file changes via chat
- **AI Code Review** — `:review` sends staged git diff to AI
- **Conversation Export** — `:export [path]` saves chat history as markdown
- **Keybinding Profiles** — `:keymap vim/emacs/vscode`
- **Tab Drag-to-Reorder** — mouse drag in tab bar
- **WASM Plugin Support** — `.wasm` files alongside Lua plugins
- **Global Config** — `~/.aura/aura.toml` shared across all AURA instances

## [0.4.9–0.4.25] — 2026-04-05

### Added
- **Persistent Settings** — settings modal changes saved to `aura.toml`, survive restarts
- **Panel Toggle Fixes** — Ctrl+T (terminal), Ctrl+G (git), Ctrl+N (file tree) properly toggle on/off
- **Indent Guide Fix** — rainbow guides no longer render on top of code text
- **Agents Tab in AI Visor** — discover agents from `.claude/agents/` (project + global)
- **Update Notification Key** — press `u` to accept update notification
- **Interactive Rebase** — `:rebase [N]` visual modal with pick/reword/edit/squash/fixup/drop, Alt+j/k reorder
- **SSH Remote Editing** — `:ssh user@host:/path` opens and saves remote files
- **Plugin Marketplace** — `:plugin search/install/uninstall/update/list` with registry
- **File Tree Actions** — `r` rename, `d` delete, `a` new file, `A` new dir, `y` copy, `x` cut, `p` paste, `.` reveal
- **Diff View as Tabs** — diffs open as real tabs, switchable and closeable
- **10 Built-in Themes** — Dark, Light, Monokai, Dracula, Nord, One Dark, Catppuccin, Gruvbox, Tokyo Night, Solarized Dark
- **Theme Picker in Settings** — cycle with Left/Right arrows, live apply + persist
- **Homebrew Distribution** — `brew tap odtorres/aura && brew install aura` with auto-update workflow
- **File Tree Scroll Fix** — persistent scroll offset, no viewport jump on expand
- **Config in `.aura/`** — configuration moved to `.aura/aura.toml` with legacy fallback
- **Discard Staged Changes** — `d` on staged files in git panel with `y` confirmation
- **Light Theme Readability** — GitHub-inspired dark-on-white colors for all syntax elements

## [0.4.0–0.4.8] — 2026-03 to 2026-04

### Added
- Autonomous AI agent mode with subagents, planning, trust levels
- Per-feature AI model configuration (commit, speculative, agent, chat, summarize)
- LSP inlay hints, semantic highlighting (23 token types), code lens, call hierarchy, signature help
- Toggle comment (`gc`), move line (Alt+j/k), visual wrap, `:%s/old/new/g`
- Enhanced minimap (12-column code preview), rainbow indent guides, incremental search
- Settings hot-reload, EditorConfig support, auto-format on save, auto-save
- Terminal search (Ctrl+F), conditional breakpoints, watch expressions, debug variable tree
- Workspace multi-root, test runner (`:test-at`), collab peer permissions
- Git blame, branch picker (Ctrl+B), stash management, merge conflict editor

## [0.3.x] — 2026-02

### Added
- Tree-sitter syntax highlighting for 17+ languages (including React/Next.js)
- LSP integration with 10+ auto-detected language servers
- Git integration via gitoxide (gix) — native Rust, no shell-out
- Embedded PTY terminal with ANSI 256-color support
- MCP server/client protocol for AI agent communication
- Real-time collaborative editing with CRDT sync over TCP
- Lua plugin runtime with trait-based architecture

## [0.2.x] — 2026-01

### Added
- AI co-authoring via Anthropic API with streaming responses
- Intent → Propose → Review editing workflow
- CRDT multi-author tracking (human, AI, peer attribution)
- Interactive chat panel with @-mention context and tool execution
- Conversation history with SQLite persistence

## [0.1.x] — 2025-12

### Added
- Rope-based text buffer (ropey) with efficient large file handling
- Vim-like modal editing: Normal, Insert, Visual, VisualLine, Command modes
- Core vim motions: hjkl, w/b/e, 0/$, gg/G, f/F/t/T, text objects
- File open/save/close with unsaved changes protection
- Undo/redo with edit history
- Status bar, command bar, line numbers, viewport scrolling
- Property-based testing with proptest

---

[1.0.2]: https://github.com/odtorres/aura/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/odtorres/aura/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/odtorres/aura/compare/v0.9.4...v1.0.0
[0.8.0]: https://github.com/odtorres/aura/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/odtorres/aura/compare/v0.6.2...v0.7.0
[0.6.0]: https://github.com/odtorres/aura/compare/v0.5.7...v0.6.0
[0.5.0]: https://github.com/odtorres/aura/compare/v0.4.25...v0.5.0
