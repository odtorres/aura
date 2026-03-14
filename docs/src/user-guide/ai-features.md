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

## Authorship Tracking

Every AI edit carries an `AuthorId` (e.g., `ai:claude`). This enables:

- **Visual markers**: Gutter shows color-coded authorship (human = green, AI = blue)
- **Selective undo**: `<Space>u` undoes only AI edits
- **Toggle**: `<Space>a` shows/hides authorship markers
- **Status bar**: Shows who made the last change and how recently

## Conversation History

Every AI interaction is stored in a local SQLite database:

- `<Space>c` — View conversation history for the current line/function
- `:search <query>` — Full-text search over all conversations
- `<Space>d` / `:decisions` — View recent accept/reject decisions

Conversations are linked to file paths, line ranges, and git commits, so you can always trace "why was this written this way?"
