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

### Peek Definition

Press `gp` to preview a symbol's definition in an inline popup without leaving the current file:

- Syntax-highlighted code with line numbers
- Target line highlighted with a distinct background
- `j`/`k` to scroll within the popup
- `Enter` to navigate to the definition (same as `gd`)
- `Esc` or `q` to close the popup
- Any other key closes the popup and is processed normally

Peek works for both same-file and cross-file definitions. A scroll indicator appears when the definition exceeds the popup height.

### Hover Information

Press `K` in Normal mode to show hover info (type signatures, documentation) in a floating popup.

### Find All References

Press `gr` to find all references to the symbol under the cursor. A floating panel shows all locations:

```
References (5)
  main.rs:42:5
  lib.rs:128:12
  tests.rs:15:20
```

Navigate with `j`/`k`, press `Enter` to jump to that location, `Esc` to close.

Also available via `:references` or `:ref` command.

### Rename Symbol

Press `F2` or `gn` to rename the symbol under the cursor across all files:

1. A rename prompt appears in the command bar with the current symbol name
2. Edit the name and press `Enter`
3. All occurrences are renamed (in the current buffer and on disk for other files)

Also available via `:rename <new_name>` command.

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

### Syntax Highlighting (Tree-sitter)

AURA ships with tree-sitter grammars for **17 languages**:

Rust, Python, TypeScript, TSX, Go, JavaScript, JSX, Java, C, C++, Ruby, HTML, CSS, JSON, Bash/Shell, TOML, YAML, Markdown

**React/Next.js**: `.jsx` uses the TSX grammar for proper JSX highlighting. `.mjs`, `.cjs`, `.mts` supported for ES modules and CommonJS.

### LSP Server Auto-Detection

AURA auto-detects these language servers when they're installed:

| Language | Server | Install |
|----------|--------|---------|
| Rust | `rust-analyzer` | `rustup component add rust-analyzer` |
| Python | `pyright-langserver` | `npm install -g pyright` |
| TypeScript/JS/JSX/TSX | `typescript-language-server` | `npm install -g typescript-language-server` |
| Go | `gopls` | `go install golang.org/x/tools/gopls@latest` |
| Java | `jdtls` | Eclipse JDT Language Server |
| C/C++ | `clangd` | LLVM/Clang toolchain |
| Ruby | `solargraph` | `gem install solargraph` |
| Bash/Shell | `bash-language-server` | `npm install -g bash-language-server` |

The LSP client works with any language server that implements the Language Server Protocol.
