# Session Persistence

AURA automatically saves your editor session when you quit and restores it the next time you open the same project. This means your open tabs, cursor positions, scroll offsets, and panel layout are preserved across restarts.

## How It Works

### Saving

When you exit AURA (`:q`, `:wq`, or `Ctrl+C`), the following state is saved to `.aura/session.json` in the project root:

- **Open tabs** — file paths for all open editor tabs, in order
- **Cursor positions** — row and column for each tab
- **Scroll offsets** — viewport scroll position for each tab
- **Active tab** — which tab was focused
- **UI layout** — visibility of the file tree, chat panel, terminal pane, and active sidebar view

### Restoring

When you launch AURA without a file argument:

```bash
aura
```

AURA checks for `.aura/session.json` in the current project directory. If found, it:

1. Reopens all files from the session (skipping any that no longer exist)
2. Restores cursor and scroll positions (clamped to current file bounds if the file changed)
3. Switches to the previously active tab
4. Restores the UI panel layout

A status message confirms the restore: `Session restored (3 tabs)`.

### When Session Restore is Skipped

Session restore only runs when AURA is launched **without an explicit file argument**. If you open a specific file:

```bash
aura src/main.rs
```

The session file is ignored and AURA opens just that file. The session is still saved on exit, so the next bare `aura` invocation will restore whatever tabs were open.

## Session File Location

The session file is stored at:

```
<project_root>/.aura/session.json
```

The project root is determined by:

1. The git repository working directory (if inside a git repo)
2. The current working directory (fallback)

This means each project maintains its own independent session.

## Session File Format

The session file is human-readable JSON:

```json
{
  "working_directory": "/path/to/project",
  "tabs": [
    {
      "file_path": "/path/to/project/src/main.rs",
      "cursor_row": 42,
      "cursor_col": 8,
      "scroll_row": 30,
      "scroll_col": 0
    },
    {
      "file_path": "/path/to/project/src/lib.rs",
      "cursor_row": 0,
      "cursor_col": 0,
      "scroll_row": 0,
      "scroll_col": 0
    }
  ],
  "active_tab": 0,
  "ui": {
    "file_tree_visible": true,
    "chat_panel_visible": false,
    "terminal_visible": false,
    "sidebar_view": "files"
  }
}
```

## .gitignore

The `.aura/` directory contains user-specific state (session, conversation database). It is typically listed in `.gitignore`:

```gitignore
.aura/
```

## Notes

- **Scratch buffers** (unsaved buffers with no file path) are not included in the session.
- If all session tabs refer to deleted files, the session is skipped and a blank scratch buffer opens.
- The session file is overwritten on every exit, so it always reflects the latest state.
