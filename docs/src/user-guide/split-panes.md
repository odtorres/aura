# Split Panes

AURA supports splitting the editor into two panes for side-by-side editing. You can view two different files, or the same file at two different positions.

## Opening a Split

### Vertical Split (side-by-side)

```
:vsplit
:vs
```

Splits the editor into left and right panes. Both initially show the current file.

### Horizontal Split (top/bottom)

```
:hsplit
:split
:sp
```

Splits the editor into top and bottom panes.

## Switching Focus

Press `Ctrl+W` to toggle focus between the primary and secondary panes. The focused pane has a **cyan border**, while the unfocused pane has a dark gray border.

All editing operations (typing, cursor movement, commands) affect the focused pane.

## Viewing Different Files

After opening a split, switch tabs in either pane to view different files:

1. Open a split: `:vsplit`
2. Switch to the other pane: `Ctrl+W`
3. Open a different file: `:e other_file.rs` or use the file picker (`<Space>p`)

## Closing a Split

```
:only
```

Closes the split and returns to a single-pane editor.

## Features

Each pane independently has:

- Its own **title bar** showing the filename and modification status
- Its own **minimap** scrollbar (if enabled)
- Its own **scroll position** — scroll one pane without affecting the other
- Its own **git gutter markers**
- **Peer cursors** displayed in the focused pane (collaborative editing)

## Layout

```
Vertical split (:vsplit)          Horizontal split (:hsplit)

+----------------+----------------+   +----------------------------------+
| file_a.rs      | file_b.rs      |   | file_a.rs                        |
|                |                |   |                                  |
| (primary)      | (secondary)    |   | (primary)                        |
|                |                |   |                                  |
+----------------+----------------+   +----------------------------------+
                                      | file_b.rs                        |
                                      |                                  |
                                      | (secondary)                      |
                                      |                                  |
                                      +----------------------------------+
```

## Configuration

Split panes respect all editor settings:

- **Minimap**: shown/hidden per the `show_minimap` setting in each pane
- **Line numbers**, **authorship markers**: displayed independently per pane
- **Theme colors**: cyan border = focused, dark gray = unfocused
