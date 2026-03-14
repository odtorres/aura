# First Run

## Opening a File

```bash
aura path/to/file.rs
```

AURA opens in **Normal mode**. You'll see:

- The file content with syntax highlighting (via Tree-sitter)
- Line numbers in the gutter
- A status bar at the bottom showing the filename, cursor position, and mode indicator
- Git diff markers in the gutter (if inside a git repository)

## Scratch Buffer

Launch AURA without arguments to open an empty scratch buffer:

```bash
aura
```

## Basic Editing Workflow

1. **Navigate** using `h`/`j`/`k`/`l` (or arrow keys)
2. Press `i` to enter **Insert mode** and start typing
3. Press `Esc` to return to **Normal mode**
4. Press `Ctrl+S` or type `:w` to save
5. Type `:q` to quit (or `:wq` to save and quit)

## Enabling AI Features

Set your Anthropic API key to unlock AI features:

```bash
export ANTHROPIC_API_KEY=sk-ant-...
aura file.rs
```

With an API key configured:

- Press `<Space>i` to enter **Intent mode** and describe what you want the AI to do
- The AI proposes changes in a **Review** view where you can accept (`a`) or reject (`r`) per-hunk
- Ghost suggestions appear as you edit — press `Tab` to accept

## Exploring the UI

| Shortcut | What it opens |
|----------|---------------|
| `Ctrl+N` | File tree sidebar |
| `Ctrl+J` or `` Ctrl+` `` | Embedded terminal |
| `<Space>p` | Fuzzy file picker |
| `K` | LSP hover info |
| `<Space>b` | Inline git blame |

## Configuration

AURA looks for `aura.toml` in the current directory or `~/.config/aura/aura.toml`. See [Configuration](configuration.md) for details.
