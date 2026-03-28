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
