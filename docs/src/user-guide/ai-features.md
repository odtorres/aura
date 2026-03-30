# AI Features

AURA's AI integration centers on the **intent-propose-review** workflow: you express what you want, the AI proposes changes, and you review them in a structured diff view.

## Prerequisites

Set your Anthropic API key:

```bash
export ANTHROPIC_API_KEY=sk-ant-...
```

Configure AI settings in `aura.toml`:

```toml
[ai]
model = "claude-sonnet-4-20250514"
max_tokens = 4096
aggressiveness = "moderate"
idle_threshold_ms = 3000
```

## Intent Mode

Press `<Space>i` or `:intent` to enter Intent mode. Type a natural language description of what you want:

```
> Refactor this function to use iterators instead of manual loops
```

Press `Enter` to submit. AURA assembles context (buffer content, cursor position, file path, language, recent edits, LSP diagnostics, syntax tree info) and streams the request to Claude.

## Review Mode

When the AI returns a proposal, AURA enters Review mode automatically:

- **Split view**: Current code on top, proposed code on bottom
- **Diff highlighting**: Additions highlighted in green
- **Streaming**: AI text appears in real time as it generates

| Key | Action |
|-----|--------|
| `a` / `Enter` | Accept — applies the edit to the buffer |
| `r` / `Esc` | Reject — discards the proposal |
| `e` | Edit the proposal text before accepting |
| `R` | Request a revision with follow-up instructions |

Accepted edits are tagged with the AI's `AuthorId` in the CRDT, so they can be tracked and undone independently.

## Quick Actions

These bypass the intent input and use pre-built prompts:

| Shortcut | Action |
|----------|--------|
| `<Space>e` | Explain selected code |
| `<Space>f` | Fix errors at cursor |
| `<Space>t` | Generate test for function |

## Ghost Suggestions (Speculative Execution)

AURA's speculative engine analyzes code in the background and offers ghost text suggestions.

### How It Works

1. When you stop editing (idle for `idle_threshold_ms`), AURA queues analysis of code near your cursor (±15 lines)
2. Results are cached by content hash (FNV-1a) — unchanged code isn't re-analyzed
3. Suggestions appear as dimmed ghost text overlays

### Controls

| Key | Action |
|-----|--------|
| `Tab` | Accept current suggestion |
| `Esc` | Dismiss suggestions |
| `Alt+]` | Cycle to next suggestion |
| `Alt+[` | Cycle to previous suggestion |
| `<Space>g` | Cycle aggressiveness level |

### Aggressiveness Levels

- **Minimal**: Only fix clear bugs and errors
- **Moderate** (default): Bug fixes + simplifications + error handling
- **Proactive**: All of the above + performance improvements + refactoring suggestions

### Multi-File Awareness

When you accept a change, the speculative engine checks related files via the semantic graph. Cross-file changes are proposed as atomic changesets that can be accepted or rejected per-file.

## AI Commit Messages

AURA can generate commit messages from your staged changes using AI.

**From the git panel:** Click the `✨` button on the "Commit Message" header. The AI analyzes the staged diff (stat summary + patch content) and streams a conventional commit message into the message box.

**From command mode:** `:commit` or `:gc`

The generated message follows conventional commit format (`type: description`) and includes bullet points for multi-file changes. The message appears in the commit message box for you to review and edit before pressing `c` to commit.

See [Git Integration](git.md) for the full source control panel reference.

## Authorship Tracking

Every AI edit carries an `AuthorId` (e.g., `ai:claude`). This enables:

- **Visual markers**: Gutter shows color-coded authorship (human = green, AI = blue)
- **Selective undo**: `<Space>u` undoes only AI edits
- **Toggle**: `<Space>a` shows/hides authorship markers
- **Status bar**: Shows who made the last change and how recently

## Interactive Chat Panel

For free-form conversation with the AI (rather than targeted code edits), use the chat panel:

- Press `Ctrl+J` to open the chat panel on the right side
- Type messages and press `Enter` to send
- Responses stream in real time with full multi-turn context
- Select code in Visual mode to include it as context automatically
- The AI can execute tools (read/edit buffer, get diagnostics) with your approval

### @-Mentions

Type `@` in the chat panel to reference files and context:

- `@file.rs` — includes the file's full content in AI context
- `@selection` — includes the current editor selection
- `@buffer` — includes the current buffer content
- `@errors` — includes LSP diagnostics (errors/warnings)

An autocomplete dropdown appears as you type, fuzzy-filtered by filename. Navigate with Up/Down, Enter/Tab to select. Multiple @-mentions per message are supported.

See [Chat Panel](chat-panel.md) for the full reference.

## Autonomous Agent Mode

Agent mode lets the AI work autonomously — planning, editing files, running commands, checking results, and fixing errors — without requiring your approval at each step.

### Starting an Agent

```
:agent fix the compile error in main.rs
```

Or with a custom iteration limit:

```
:agent -n 100 add comprehensive tests for the parser module
```

### How It Works

1. The AI receives an enhanced system prompt instructing autonomous work
2. ALL tools are auto-approved (reads, edits, commands) — no Y/N prompts
3. The AI loops: analyze → edit → run → check → fix
4. Stops when: task is complete, iteration limit reached, or you press `Esc`

### Controls

| Action | Command |
|--------|---------|
| Start agent | `:agent <task>` |
| Custom limit | `:agent -n 100 <task>` |
| Stop agent | `Esc` or `:agent stop` |

### Status Bar

While the agent is running, the status bar shows:

```
AGENT [3/50] 2f 1c 12s
```

- `[3/50]` — iteration 3 of 50 max
- `2f` — 2 files changed
- `1c` — 1 command run
- `12s` — elapsed time

### Safety

- Default limit: 50 iterations (configurable with `-n`)
- All edits tracked in undo history — fully reversible with `u`
- Recommend using `:experiment <branch>` first to work on a separate branch
- Press `Esc` at any time to stop the agent immediately

## Conversation History

Every AI interaction is stored in a local SQLite database:

- `<Space>c` — View conversation history for the current line/function
- `:search <query>` — Full-text search over all conversations
- `<Space>d` / `:decisions` — View recent accept/reject decisions

Conversations are linked to file paths, line ranges, and git commits, so you can always trace "why was this written this way?"

### AI History Panel (`Ctrl+H`)

The right-side panel shows all conversations with:

- **Branch grouping** — Conversations organized by git branch with colored headers
- **Intent-based titles** — Shows what you asked instead of generic text
- **Relative timestamps** — "2h ago" instead of raw ISO dates
- **Acceptance rate** — Green/red badge showing accepted vs rejected proposals
- **Search** — Press `/` to filter by title, file, or branch

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate conversations |
| `Enter` | Expand conversation (show messages inline) |
| `Enter` (again) | Open **full-screen detail modal** with word-wrapped messages |
| `/` | Start search |
| `u` / `d` | Scroll messages |
| `Esc` | Close panel or modal |

### Detail Modal

The full-screen modal shows the complete conversation with proper formatting:
- Word-wrapped messages (no truncation)
- Color-coded roles: green (You), cyan (AI), gray (System)
- File path, branch, timestamp, and acceptance rate header
- Scroll with `j`/`k` or page with `d`/`u`

## Conversation Compaction

The conversation database grows over time. AURA provides automatic and manual compaction to keep it manageable.

### Auto-Compact on Startup

When `auto_compact = true` (default), AURA cleans up the database on every launch:

- Deletes messages older than `max_message_age_days` (default: 90 days)
- Trims conversations to `max_messages_per_conversation` (default: 200)
- Removes excess conversations beyond `max_conversations` (default: 500)
- Always preserves the `keep_recent_messages` most recent messages per conversation

### Manual Compaction

```
:compact
```

Runs the same cleanup immediately and reports how many messages and conversations were deleted.

### AI Summarization

When an AI client is configured, long conversations (those with more messages than `keep_recent_messages` and no existing summary) are automatically summarized in the background:

1. AURA sends the conversation transcript to Claude
2. Claude generates a 2-4 sentence summary
3. The summary is stored in the conversation's `summary` field
4. Old messages are thinned, keeping only the most recent ones plus the summary
5. When the conversation is loaded later, the summary is prepended as context so the AI remembers what was discussed

### Context Window Management

The chat panel limits how many messages are sent to the AI per turn (`max_context_messages`, default: 40). This prevents unbounded memory growth and API token waste during long sessions. The oldest messages are dropped first, but the initial context message is always preserved.

### Configuration

See [Configuration](../getting-started/configuration.md) for the full `[conversations]` settings reference.

## Code Snippets

AURA includes a Tab-triggered snippet system with VS Code-compatible `${1:placeholder}` syntax.

### Usage

1. Enter Insert mode (`i`)
2. Type a trigger word (e.g., `fn`, `if`, `for`)
3. Press `Tab` — the trigger expands into a template
4. Cursor lands on the first placeholder — type to replace
5. Press `Tab` to jump to the next placeholder
6. Final `Tab` goes to the `$0` (exit) position

### Built-in Snippets

**Rust** (10): `fn`, `pfn`, `test`, `impl`, `struct`, `enum`, `match`, `if`, `for`, `mod`

**Python** (6): `def`, `class`, `if`, `for`, `with`, `try`

**TypeScript/JS** (8): `fn`, `afn`, `class`, `if`, `for`, `import`, `export`, `const`

**Go** (6): `func`, `if`, `iferr`, `for`, `struct`, `switch`

**Generic** (2): `todo`, `fixme`

### Custom Snippets

Create JSON files in `~/.aura/snippets/` named by language:

```json
// ~/.aura/snippets/rust.json
{
  "Print Debug": {
    "prefix": "pd",
    "body": "println!(\"${1:var}: {:?}\", ${1:var});$0",
    "description": "Debug print"
  }
}
```

Format: VS Code snippet JSON with `prefix` (trigger), `body` (template), `description`.

## Multi-Cursor Editing

Edit at multiple positions simultaneously.

### Usage

1. Place cursor on a word in Normal mode
2. Press `Ctrl+D` — adds a yellow cursor at the next occurrence
3. Press `Ctrl+D` again — adds another (wraps around)
4. Enter Insert mode (`i`) — type at ALL cursor positions at once
5. Press `Esc` — clears all secondary cursors

Secondary cursors are rendered as yellow blocks. The primary cursor remains the terminal cursor.
