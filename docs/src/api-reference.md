# API Reference

The full Rust API documentation is built with `rustdoc` and deployed alongside this book.

## Crate Documentation

| Crate | Description | Link |
|-------|-------------|------|
| `aura-core` | Buffer, cursor, authorship, CRDT, conversation store, semantic graph | [aura_core](/api/aura_core/) |
| `aura-tui` | App state, rendering, input, LSP, MCP, git, plugins, speculative engine | [aura_tui](/api/aura_tui/) |
| `aura-ai` | Anthropic API client, context assembly, token estimation | [aura_ai](/api/aura_ai/) |

## Building Locally

```bash
# Build and open in browser
cargo doc --workspace --no-deps --open

# Build with warnings as errors (CI mode)
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

## Key Entry Points

### `aura-core`

- [`Buffer`](/api/aura_core/buffer/struct.Buffer.html) — rope-based text buffer
- [`Cursor`](/api/aura_core/cursor/struct.Cursor.html) — row/column position
- [`AuthorId`](/api/aura_core/author/struct.AuthorId.html) — edit authorship
- [`CrdtDoc`](/api/aura_core/crdt/struct.CrdtDoc.html) — Automerge CRDT wrapper
- [`ConversationStore`](/api/aura_core/conversation/struct.ConversationStore.html) — SQLite conversation history
- [`SemanticGraph`](/api/aura_core/semantic/struct.SemanticGraph.html) — dependency graph

### `aura-tui`

- [`App`](/api/aura_tui/app/struct.App.html) — main application state
- [`Mode`](/api/aura_tui/app/enum.Mode.html) — editing mode enum
- [`AuraConfig`](/api/aura_tui/config/struct.AuraConfig.html) — configuration
- [`McpServer`](/api/aura_tui/mcp_server/struct.McpServer.html) — MCP server
- [`PluginManager`](/api/aura_tui/plugin/struct.PluginManager.html) — plugin system

### `aura-ai`

- [`AnthropicClient`](/api/aura_ai/client/struct.AnthropicClient.html) — API client
- [`EditorContext`](/api/aura_ai/context/struct.EditorContext.html) — context assembly
- [`AiConfig`](/api/aura_ai/struct.AiConfig.html) — configuration
