# Changelog

All notable changes to AURA will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.36] - 2026-03-30

### Added

- **Project-wide search/replace** (`Ctrl+F` or `:search`) — Interactive search across all project files with results grouped by file.
  - Full-screen overlay with search input, optional replace input, and results list.
  - Results show file path headers (cyan) with line:column + context for each match.
  - Navigate with `j`/`k`, `Enter` to jump to match location, `Esc` to close.
  - Replace mode (`Ctrl+R`): type replacement text, `R` to replace all across files.
  - Case sensitivity toggle (`Ctrl+C`), `Tab` to cycle between search/replace/results.
  - `:search <query>` and `:grep <query>` commands with argument support.
  - Skips `.git`, `target`, `node_modules`, binary files (>5MB).
  - Max 1000 results for responsiveness.
  - 3 unit tests for search and replace.

## [0.1.35] - 2026-03-30

### Fixed

- **Crash on multi-byte characters** — Fixed panic in Claude Code watcher when activity log contained multi-byte UTF-8 characters (e.g., em-dash `–`). The `truncate()` function now uses char-boundary-safe iteration.
- **Clippy `is_multiple_of` lint** — Fixed CI failure on Rust 1.94+ where `actual_col % indent_width == 0` triggers `clippy::manual_is_multiple_of`.

## [0.1.34] - 2026-03-30

### Added

- **Code folding** — Fold/unfold code blocks using tree-sitter AST data.
  - `za` toggle fold, `zc` close fold, `zo` open fold, `zM` close all, `zR` open all.
  - Foldable ranges auto-detected for functions, structs, classes, impl blocks, if/for/while, modules.
  - Gutter shows `▶` for folded lines, `▼` for foldable lines.
  - Folded lines show `··· (N lines)` indicator.
- **Sticky scroll** — Pinned scope headers at the top of the editor (up to 3 lines).
  - Shows enclosing function/class/impl/module names when they scroll out of view.
  - Dark gray background with thin separator line.
  - Uses tree-sitter to determine enclosing scopes.
- **Indent guides** — Vertical `│` lines at indent level boundaries.
  - Rendered in subtle gray (`Rgb(60,60,60)`) at each indent width boundary.
  - Respects the file's detected indent style (spaces or tabs).
- **Rainbow bracket colorization** — Brackets colored by nesting depth using a 6-color palette (Yellow, Magenta, Cyan, Green, Blue, Red).
  - Applies to `()`, `{}`, `[]` across all languages.
  - Depth calculated from file start for accuracy.
- **Auto-close brackets and quotes** — Typing `(`, `{`, `[`, `"`, `'` auto-inserts the matching closing character with cursor positioned between them.
  - Smart skip: if next character is already the closing pair, doesn't insert duplicate.

## [0.1.33] - 2026-03-29

### Added

- **AI Visor panel** (`Ctrl+I` or `:visor`) — Claude Code configuration browser showing the `.claude/` folder contents in a tabbed right-side panel. No other editor provides this feature.
  - **Overview tab**: Model, effort level, CLAUDE.md status, stats dashboard.
  - **Settings tab**: Merged settings cascade with scope indicators `[G]`lobal/`[P]`roject/`[L]`ocal, color-coded.
  - **Skills tab**: Lists skills from `.claude/skills/` and legacy `.claude/commands/` with descriptions.
  - **Hooks tab**: Shows configured hooks by event type (PreToolUse, PostToolUse, Stop, etc.).
  - **Plugins tab**: Installed Claude Code plugins from `~/.claude/plugins/`.
  - Navigate with `1`-`5` (tabs), `j`/`k` (items), `e`/`Enter` (open source file), `Tab` (cycle tabs).

## [0.1.32] - 2026-03-29

### Added

- **Undo tree visualization** (`:undo-tree` or `:ut`) — Full-screen modal showing the complete edit history with author colors, timestamps, and edit previews.
  - Two-panel layout: entry list (65%) + detail panel (35%).
  - Author-colored entries: Human (green), AI (cyan), Peer (magenta).
  - Current position marked with `→` in yellow, redo entries shown dimmed.
  - `j`/`k` navigate, `d`/`u` page, `Enter` restores to any history point, `t` toggles detail.
  - `Buffer::restore_to(pos)` — time-travel through undo history by undoing/redoing to reach target.

## [0.1.31] - 2026-03-29

### Added

- **Find All References** (`gr` or `:ref`) — Floating panel showing all references to a symbol, navigate with `j`/`k`, `Enter` to jump.
- **Rename Symbol** (`F2`, `gn`, or `:rename <name>`) — LSP-powered rename across all files. Edits current buffer directly and other files on disk.
- **LSP request ID tracking** — Replaced shape-based response dispatch with ID-based method tracking, enabling accurate disambiguation of References vs Definition responses.

## [0.1.30] - 2026-03-29

### Added

- **3-panel merge conflict editor** — VS Code-style merge resolution with Incoming (theirs) | Current (ours) | Result panels.
  - Conflict files shown with magenta **C** status in source control panel.
  - Press Enter on a conflict file to open the merge editor.
  - Resolve conflicts with `1` (current), `2` (incoming), `3`/`4` (both), `5` (ignore).
  - Navigate conflicts with `n`/`N`, cycle panels with `Tab`.
  - Result panel updates in real-time as you resolve conflicts.
  - Press `c` to complete merge — writes resolved file and stages it automatically.
  - Unit tests for conflict parsing, resolution, and result generation.
- **TLS encryption for collaborative editing** — Enable `use_tls = true` in `[collab]` config to encrypt all peer traffic with rustls.
  - Self-signed certificates generated automatically via rcgen.
  - WireReader/WireWriter transport abstraction supports both plaintext TCP and TLS.
  - Works with authentication tokens for defense in depth.
- **Global panel-switching shortcuts** — `Ctrl+T/G/N/J/H/,` now work from any focused panel without needing to press `Esc` first.

## [0.1.29] - 2026-03-29

### Added

- **Integrated debugger (DAP protocol)** — Full debug adapter protocol support for stepping through code, setting breakpoints, and inspecting variables directly in the editor.
  - **Breakpoints**: Toggle with F9 or `:bp` — red dots in the gutter, persist across sessions.
  - **Debug controls**: F5 (continue/start), F10 (step over), F11 (step in), Shift+F11 (step out), Shift+F5 (stop).
  - **Debug panel**: Bottom panel with three tabs — Call Stack, Variables (expandable tree), and Output.
  - **Auto-detection**: Automatically finds CodeLLDB (Rust/C/C++), debugpy (Python), dlv (Go), and Node.js debug adapters.
  - **Custom adapters**: Configure any DAP adapter in `aura.toml` under `[debuggers]`.
  - **Gutter indicators**: Red `●` for breakpoints, yellow `→` for current execution line, `⏸` for breakpoint+stopped.
  - **Commands**: `:debug`, `:debug <program>`, `:breakpoint`, `:continue`, `:step`, `:stepin`, `:stepout`, `:debug stop`, `:debug panel`.

## [0.1.28] - 2026-03-28

### Fixed

- **Subtler comment colors** — Comments now render in a softer gray (`Rgb(100,100,100)`) instead of bright `DarkGray`, making them clearly distinct from code text across all 17+ languages.

## [0.1.27] - 2026-03-28

### Summary — Session Highlights (v0.1.2 → v0.1.27)

This release consolidates 27 versions of feature development:

- **Collaborative editing** — Real-time multi-file collab with auth tokens and peer cursors
- **Vim power features** — Operator+motion, text objects, char search, visual block, multi-cursor, dot repeat, macros
- **17+ languages** — Tree-sitter highlighting + LSP for Rust, Python, TypeScript, Go, Java, C/C++, Ruby, HTML, CSS, JSON, YAML, TOML, Bash, Markdown, JSX/TSX
- **Git integration** — Visual graph modal, branch picker, stage-all button, AI commit messages, syntax-highlighted diff view, filename-first entries, dotfiles visible
- **AI features** — Conversation compaction with AI summarization, Claude Code activity observer, command palette, interactive chat with tool execution
- **Editor UX** — Split panes, settings modal, optional minimap, tab close buttons, Lua plugin runtime, code snippets (32 built-in)
- **Code quality** — No unwrap() in library code, LSP auto-restart, community standards

## [0.1.26] - 2026-03-28

### Added

- **Visual git graph modal** — `:graph` opens a full-screen modal showing commit history with ASCII branch graph lines, colored by branch. Left panel shows graph + commit hash + message + time. Right panel shows selected commit detail: full hash, author, date, refs, and changed files with status (M/A/D) and filename-first display. Navigate with j/k, page with d/u, Enter toggles detail panel, Esc closes.

## [0.1.25] - 2026-03-28

### Added

- **Documentation update** — Comprehensive docs for all features added in v0.1.18-v0.1.24: snippets (usage, built-in list, custom snippets), multi-cursor editing, visual block mode, branch picker, keybindings for all new shortcuts.

## [0.1.24] - 2026-03-28

### Added

- **Snippet system** — Tab-triggered code snippets with `${1:placeholder}` syntax. Type a trigger word (`fn`, `if`, `for`, `def`, `class`, etc.) and press Tab to expand. Tab moves between placeholders. Built-in snippets for Rust (10), Python (6), TypeScript/JS (8), Go (6), and generic (2). User-defined snippets from `~/.aura/snippets/*.json` in VS Code format.

## [0.1.23] - 2026-03-28

### Added

- **Multi-cursor editing** — `Ctrl+D` adds a cursor at the next occurrence of the word under cursor. Type to insert at all cursor positions simultaneously. Secondary cursors rendered as yellow blocks. `Esc` clears all secondary cursors.

## [0.1.22] - 2026-03-28

### Added

- **Syntax highlighting in diff view** — The side-by-side git diff view now applies tree-sitter syntax highlighting to all lines (unchanged, added, deleted). Supports all 17+ languages including JS/TS/JSX/TSX/React.
- **Branch picker shortcut** — `Ctrl+B` opens the branch picker modal from any mode.
- **Improved JS/TS/React/Next.js support** — `.jsx` uses TSX grammar for JSX highlighting. `.mjs`, `.cjs`, `.mts` extensions added. Full typescript-language-server LSP support.
- **Dotfiles visible** — File tree shows `.env`, `.gitignore`, `.eslintrc`, etc.
- **Filename-first git entries** — Git panel shows filename bold first, directory dimmed after.
- **Branch picker modal** — `:branches` / `:br` / `Ctrl+B` opens VS Code-style branch switcher with filter.

## [0.1.21] - 2026-03-28

### Added

- **Improved JS/TS/React/Next.js support** — `.jsx` now uses the TSX grammar for proper JSX syntax highlighting. Added `.mjs`, `.cjs`, `.mts` extensions for ES modules and CommonJS. All get typescript-language-server LSP support. Covers Next.js config files (`next.config.mjs`, etc.).

## [0.1.20] - 2026-03-28

### Added

- **Visual block mode** — `Ctrl+V` enters V-BLOCK mode for rectangular column selection. Block delete (`d`), yank (`y`), insert (`I`), and append (`A`).
- **Branch picker modal** — `:branches` / `:br` opens a VS Code-style branch picker. Filter by typing, Enter to switch, current branch highlighted in green. Git errors shown on failed checkout.
- **Filename-first git entries** — Git panel shows filename in bold white first, then directory in gray (like Cursor/VS Code). No more truncated filenames.
- **Dotfiles visible** — File tree now shows `.env`, `.gitignore`, `.eslintrc`, etc. Only `.git`, `.aura`, `target`, `node_modules` are hidden.

### Fixed

- **Diff view colors** — Dark green/red backgrounds instead of blinding bright colors.
- **TODO.md** — Fixed section 10.7 formatting.

## [0.1.19] - 2026-03-28

### Added

- **Visual block mode** — `Ctrl+V` enters column selection mode (V-BLOCK). Select rectangular regions, delete columns with `d`, yank blocks with `y`, insert at block start with `I`, append with `A`. Block selections highlighted per-cell in the editor.

### Fixed

- **Diff view colors** — Toned down additions (dark green bg) and deletions (dark red bg) for much better readability.
- **TODO.md formatting** — Fixed section 10.7 heading and checklist formatting.

## [0.1.18] - 2026-03-28

### Added

- **TLS infrastructure** — Self-signed certificate generation (rcgen), rustls ServerConfig/ClientConfig builders, NoCertVerifier for self-signed certs, and TLS relay thread for encrypted streams. Dependencies wired: rustls 0.23, rcgen 0.13. Stream encryption integration pending reader/writer architecture refactor.
- **Community standards** — Code of Conduct (Contributor Covenant v2.1), Security Policy, Issue Templates (bug report, feature request, question), Pull Request Template.

## [0.1.17] - 2026-03-28

### Added

- **Conversation detail modal** — Press Enter twice on a conversation in the AI History panel to open a full-screen modal with word-wrapped messages, file/branch/time header, acceptance rate, and scrollable content. Solves truncated text in the narrow side panel.
- **AI History panel improvements**: branch grouping, intent-based titles, relative timestamps, search (`/`), acceptance rate badges, smart truncation.
- **Claude Code activity observer** — Background watcher tails Claude Code's JSONL logs. Shows real-time activity in status bar.
- **MCP `report_activity` and `get_editor_state` tools** — Claude Code reports activity and queries editor state.
- **Fuzzy command palette** — `Ctrl+P` for unified search across commands, files, and settings.
- **LSP auto-restart** — Automatic recovery when LSP server disconnects.

## [0.1.16] - 2026-03-28

### Added

- **AI History panel improvements**:
  - **Branch grouping** — Conversations grouped by git branch with colored section headers
  - **Intent-based titles** — Shows the user's original request instead of generic "The developer and AI"
  - **Relative timestamps** — "2h ago", "3d ago" instead of raw ISO-8601
  - **Search/filter** — Press `/` to search conversations by title, file, or branch
  - **Acceptance rate badges** — Green/red `[2/3]` indicator showing accepted vs rejected proposals
  - **Smart truncation** — Text truncated at word boundaries, not mid-word
  - **Decision stats** — New `decision_stats()` query for per-conversation accept/reject counts

## [0.1.15] - 2026-03-28

### Added

- **Claude Code activity observer** — Background watcher that tails Claude Code's JSONL conversation logs (`~/.claude/projects/`) in real-time. Displays tool calls, responses, and progress events in AURA's status bar (e.g., "CC: Read: main.rs", "CC: Running cargo test...").
- **MCP `report_activity` tool** — Claude Code can proactively report what it's doing to AURA's agent registry (activity type, description, current task).
- **MCP `get_editor_state` tool** — Claude Code can query AURA's full state: current mode, open files, cursor position, diagnostics count, modification status.
- **Enhanced agent registry** — Agents now track `last_activity`, `current_task`, and `activity_count`.

### Fixed

- **LSP auto-restart** — When the LSP server disconnects (crash/OOM), AURA now automatically restarts it instead of leaving it dead.

## [0.1.14] - 2026-03-28

### Added

- **Command palette** — VS Code-style `Ctrl+P` fuzzy search across commands, files, and settings in one overlay. Type to filter, Enter to execute. Commands show `[cmd]` badge, files show `[file]`, settings show `[set]`. 24 commands, all workspace files, and 3 settings available.

## [0.1.13] - 2026-03-28

### Added

- **Dot repeat** — `.` replays the last edit (Insert-mode change sequences like `cw`, `s`, `o` + typed text). Press `.` to repeat the same change at a new cursor position.
- **Macro recording** — `q{a-z}` starts recording all keystrokes into a named register. `q` stops recording. `@{a-z}` plays back the macro. Record complex edit sequences and replay them instantly.

## [0.1.12] - 2026-03-28

### Added

- **Remote collaboration** — Collaboration sessions can now be hosted on all network interfaces (`bind_address = "0.0.0.0"` in `aura.toml`) for internet access. Token-based authentication prevents unauthorized access: host generates a token, clients must provide it to join.
- **Authentication tokens** — `:host` generates a token when `require_auth = true`. Clients join with `:join addr:port token` or `--join addr --token TOKEN`. Rejected connections get a clear error.
- **Configurable bind address** — `bind_address` in `[collab]` config controls whether the host listens on localhost only or all interfaces.

## [0.1.11] - 2026-03-27

### Added

- **Lua plugin runtime** — Dynamic plugin loading from `~/.aura/plugins/*.lua`. Each Lua script defines a `plugin` table with callbacks: `on_load()`, `on_key(mode, key)`, `on_save(path)`, `on_intent(intent)`. Plugins can return actions (`cmd:`, `insert:`, `status:`) to control the editor. Auto-discovered on startup.

## [0.1.10] - 2026-03-27

### Fixed

- **Update installer** — The in-app updater now always uses the shell installer (`curl ... | sh`) instead of `cargo install` which requires crates.io publishing. Works reliably for all installation methods.

## [0.1.9] - 2026-03-27

### Added

- **12 new language grammars** — Tree-sitter syntax highlighting for JavaScript, Java, C, C++, Ruby, HTML, CSS, JSON, Bash, TOML, YAML, and Markdown. Total: 17 languages supported.
- **6 new LSP servers** — Language server detection for Java (jdtls), C/C++ (clangd), Ruby (solargraph), and Bash (bash-language-server), in addition to existing Rust, Python, TypeScript, and Go servers.

## [0.1.8] - 2026-03-27

### Added

- **Operator-pending mode** — Full vim operator+motion system: `dw`, `d$`, `cw`, `ce`, `yw`, `yb`, etc. Operators (`d`, `c`, `y`, `>`, `<`) wait for a motion, then apply to the range.
- **Count prefix** — `3j`, `5dw`, `2dd`, etc. Numeric prefixes multiply motions and operations.
- **Text objects** — `ci"`, `da(`, `diw`, `yaw`, `ci{`, `di[`, `ca<`, etc. Inner (`i`) and around (`a`) variants for quotes, parentheses, braces, brackets, angle brackets, and words.
- **Character search** — `f{char}`, `F{char}`, `t{char}`, `T{char}` to jump to characters on the current line. `;` and `,` to repeat/reverse the search.
- **Essential vim commands** — `r{char}` (replace), `J` (join lines), `~` (toggle case), `s` (substitute), `S`/`cc` (substitute line), `C`/`c$` (change to EOL), `D`/`d$` (delete to EOL), `Y` (yank line), `*`/`#` (search word under cursor).
- **Indent/dedent operators** — `>>` and `<<` for indenting/dedenting lines, with count support (`3>>`).

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
