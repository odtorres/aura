# LSP (Language Server Protocol)

AURA includes a built-in LSP client that connects to language servers for intelligent code features.

## Features

### Diagnostics

Errors and warnings from the language server appear in:

- **Gutter markers**: Color-coded icons in the line number column
- **Floating window**: Detailed diagnostic text on hover
- **Navigation**: `]` / `[` jumps to the next/previous diagnostic

Diagnostics are also automatically fed into AI context, so when you use `<Space>f` (fix errors), the AI sees the exact error messages.

### Go to Definition

Press `gd` to jump to the definition of the symbol under the cursor.

### Hover Information

Press `K` in Normal mode to show hover info (type signatures, documentation) in a floating popup.

### References

Find all references to a symbol — integrated into the semantic graph for impact analysis.

### Code Actions

```
:code-action
:ca
```

Request code actions at the cursor position. The language server may offer quick fixes, refactorings, or other automated changes. These integrate with the AI pipeline — AI can trigger code actions and vice versa.

## AI Integration

LSP data enriches AI context:

- **Diagnostics** at the cursor are included in AI prompts, helping the AI understand what's broken
- **Syntax tree** information (via Tree-sitter) gives the AI structural context
- **Code actions** can be combined with AI suggestions for more targeted fixes

## Supported Languages

AURA ships with Tree-sitter grammars for:

- Rust
- TypeScript
- Python
- Go

The LSP client works with any language server that implements the Language Server Protocol. Configure the server command in your environment (AURA auto-detects common servers).
