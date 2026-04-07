# Markdown Live Preview

Toggle with `:preview` or `:md`. Splits the editor 50/50 with a rendered markdown preview on the right side that updates live as you type.

## Supported Elements

- **Headers** (h1-h6) with colored block indicators and bold text
- **Code blocks** (fenced with ` ``` `) with dark background
- **Inline code** with `highlighted` styling
- **Bold** and *italic* text
- **Unordered lists** with bullet indicators (nested supported)
- **Ordered lists** with number prefixes
- **Blockquotes** with vertical bar indicator
- **Tables** with separator rows
- **Horizontal rules** (`---`, `***`, `___`)
- **Links** (displayed inline)

## Usage

```
:preview        " Toggle preview on/off
:md             " Alias for :preview
```

The preview pane scrolls in sync with the editor's scroll position. Close the preview with `:preview` again.

## Auto-detection

The preview works on any file, but is most useful with `.md` files. AURA does not auto-open the preview — use `:preview` to toggle it manually.
