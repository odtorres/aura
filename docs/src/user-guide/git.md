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

## Committing

### AI-Generated Commit Messages

```
:commit
```

or `:gc` — AURA uses AI to generate a commit message based on the staged changes.

### Manual Commit

```
:commit Fix the off-by-one error in word jump
```

Commits the current changes with the provided message.

### Conversation-Linked Commits

When committing, AURA attaches conversation summaries as `Aura-Conversation` trailers in the commit message. This links the "why" (AI conversation) to the "what" (git diff).

## Branch Management

| Command | Action |
|---------|--------|
| `:branches` / `:br` | List all branches (current marked with `*`) |
| `:checkout <name>` | Switch to a branch |
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
