# Zen Mode, Breadcrumbs & Sticky Scroll

## Zen Mode

Toggle distraction-free editing with `:zen`. All chrome is hidden:

- Tab bar
- Status bar and command bar
- File tree sidebar
- Terminal pane
- Debug panel
- Chat panel, history panel, AI Visor
- Minimap

Only the editor content remains. Toggle back with `:zen`.

## Breadcrumbs

When the editor pane is focused, the current scope path is shown above the content:

```
impl App > fn draw > if block
```

- Uses tree-sitter foldable ranges to determine enclosing scopes
- Updates automatically on cursor movement
- Hidden in zen mode
- Styled in italic with the theme's gutter color

## Sticky Scroll

When you scroll past a function, class, or block definition, its opening line is pinned at the top of the viewport:

```
pub fn draw_editor_pane(                    <- pinned
    // ... scrolled content below ...
```

- Up to 3 scope levels shown (outermost at top)
- Uses theme keyword color on status bar background
- Hidden in zen mode
- Automatically detected from tree-sitter foldable ranges
