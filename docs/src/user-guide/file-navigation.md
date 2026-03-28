# File Navigation

AURA provides two ways to navigate files: a persistent file tree sidebar and a fuzzy file picker overlay.

## File Tree Sidebar

Toggle with `Ctrl+N` or `:tree`.

The file tree shows the directory structure of the current working directory. When opened with `Ctrl+N`, it receives keyboard focus — indicated by a yellow border with a `[focused]` label.

### Navigation

| Key | Action |
|-----|--------|
| `j` / `Down` | Move selection down |
| `k` / `Up` | Move selection up |
| `Enter` / `l` | Open file or expand directory |
| `h` | Collapse directory or jump to parent |
| `Esc` | Return focus to editor (tree stays visible) |
| `Ctrl+N` | Close tree and return focus |

When you press `Enter` on a file, it opens in the editor and focus returns to the buffer. Directories expand/collapse in place.

### Visible Files

The file tree shows all files including dotfiles (`.env`, `.gitignore`, `.eslintrc`, etc.). Only noise directories are hidden: `.git`, `.aura`, `target`, `node_modules`.

## Fuzzy File Picker

Open with `<Space>p` or `:files` (`:fp`).

The file picker is a floating overlay that fuzzy-matches filenames as you type. It searches all files in the working directory.

### Usage

1. Press `<Space>p` to open
2. Start typing to filter — matches are ranked by fuzzy score
3. Use `Ctrl+J`/`Ctrl+K` (or `Down`/`Up`) to move through results
4. Press `Enter` to open the selected file
5. Press `Esc` to close without opening

The picker is useful for quickly jumping to a file when you know part of its name, without navigating the tree.
