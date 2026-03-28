# Git Integration

AURA integrates with git via gitoxide (`gix`), providing native Rust git operations without shelling out.

## Gutter Markers

When editing a file in a git repository, the gutter shows diff status:

- **Green `+`**: Added lines (not in HEAD)
- **Yellow `~`**: Modified lines (changed from HEAD)
- **Red `-`**: Deleted lines (present in HEAD but removed)

## Inline Blame

Toggle with `<Space>b` or `:blame`.

Shows the commit hash, author, and date for each line inline, similar to `git blame`. Blame information is fetched from the repository and rendered alongside the buffer.

## Source Control Panel

Open the git sidebar with `Ctrl+G` (or click the "Git" tab in the sidebar). The panel shows:

1. **Branch info** — current branch, ahead/behind counts
2. **Commit Message** — text box for composing commit messages
3. **Staged Changes** — files ready to commit
4. **Changes** — unstaged modified/added/deleted files

### Keyboard Shortcuts (when panel is focused)

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate entries |
| `Tab` / `BackTab` | Cycle sections |
| `s` | Stage selected file |
| `S` | Stage all changed files |
| `u` | Unstage selected file |
| `d` | Discard changes (with `y` to confirm) |
| `c` | Commit staged changes |
| `i` / `Enter` | Edit commit message |
| `Enter` (on file) | Open side-by-side diff view |

### Side-by-Side Diff View

Press `Enter` on a file entry to open a side-by-side diff with:

- **Full syntax highlighting** — tree-sitter colors for all 17+ languages (keywords, strings, types, etc.)
- **Dark-tinted backgrounds** — additions on dark green, deletions on dark red (readable)
- **Minimap** with colored markers for added/deleted lines
- Scroll with `j`/`k`, `Esc` to close

### Stage All Button

A green `+` button appears on the "Changes (N)" header when there are unstaged files. Click it to stage all files at once. Same as pressing `S`.

### AI Commit Message Button

A sparkle `✨` button appears on the "Commit Message" header when:
- There are staged files
- An AI client is configured (`ANTHROPIC_API_KEY` set)

Click it to generate a commit message from the staged diff. The AI response streams into the commit message box in real-time — the header shows "(AI...)" while generating. Once complete, review and edit the message, then press `c` to commit.

Also available via `:commit` or `:gc` commands.

## Committing

### AI-Generated Commit Messages

```
:commit
```

or `:gc` — opens the git panel and generates a commit message using AI. The message appears in the commit message box for review before committing.

### Manual Commit

```
:commit Fix the off-by-one error in word jump
```

Commits staged changes immediately with the provided message.

### Conversation-Linked Commits

When committing, AURA attaches conversation summaries as `Aura-Conversation` trailers in the commit message. This links the "why" (AI conversation) to the "what" (git diff).

## Branch Management

### Branch Picker

`:branches` or `:br` opens a modal branch picker (like VS Code/Cursor):

- All local branches listed with commit hash
- Current branch marked with `*` in green
- Type to filter branches by name
- `Enter` to switch, `Esc` to cancel
- Git errors shown in status bar if checkout fails (e.g., uncommitted changes)

### Commands

| Command | Action |
|---------|--------|
| `:branches` / `:br` | Open branch picker modal |
| `:checkout <name>` | Switch to a branch directly |
| `:branch <name>` | Create a new branch |

## Aura Git Log

```
:log
:gl
```

Shows recent commits with Aura-Conversation trailers, connecting code changes to the AI conversations that produced them.

```
:log 100
```

Shows the last 100 entries.

## Experimental Mode

```
:experiment feature-name
```

Creates a branch and enters a mode where AI suggestions are auto-accepted. Useful for letting the AI explore a larger change that you'll review as a whole (like a PR) rather than per-hunk.
