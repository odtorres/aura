# Architecture Overview

AURA is organized as a Cargo workspace with four crates, each with a clear responsibility boundary.

## Crate Dependency Graph

```
editor → tui → core
          ↓
          ai → core
```

- **`core`** has no dependency on any other workspace crate
- **`tui`** depends on `core` (and `ai` for AI integration)
- **`ai`** depends on `core` (for buffer/cursor types in context assembly)
- **`editor`** wires everything together (binary entry point)

## Crate Responsibilities

| Crate | Purpose |
|-------|---------|
| `aura-core` | Buffer (rope + CRDT), cursor, authorship, sync primitives, conversation store, semantic graph |
| `aura-tui` | App state machine, rendering (ratatui), input handling, LSP, MCP, git, collab, plugins |
| `aura-ai` | Anthropic API client, context assembly, response parsing |
| `aura` | Binary entry point, CLI argument parsing, terminal setup |

## Data Flow

```
User Input (crossterm)
    │
    ▼
Input Handler (input.rs) ──► Mode-specific handler
    │
    ▼
App State (app.rs) ──► Buffer Operations (core)
    │                         │
    │                         ▼
    │                   CRDT Layer (automerge)
    │                         │
    ▼                         ▼
Render (render.rs) ◄── Syntax Highlighting (tree-sitter)
    │
    ▼
Terminal Output (ratatui + crossterm)
```

### AI Data Flow

```
Intent Input ──► Context Assembly (ai/context.rs)
                      │
                      ▼
                Anthropic API (ai/client.rs)
                      │ (streaming)
                      ▼
                Proposal ──► Review Mode ──► Accept/Reject
                                                  │
                                                  ▼
                                          Buffer Edit (tagged with AI AuthorId)
```

### MCP Data Flow

```
External MCP Client ──► MCP Server (tui/mcp_server.rs)
                              │
                              ▼
                        Tool Dispatch ──► Buffer/Diagnostics/Context
                              │
                              ▼
                        Response ──► Client
```

### Collaborative Editing Data Flow

```
Host Instance                          Client Instance
    │                                       │
    ├── Local Edit                          ├── Local Edit
    │     │                                 │     │
    │     ▼                                 │     ▼
    │  Buffer + CRDT                        │  Buffer + CRDT
    │     │                                 │     │
    │     ▼                                 │     ▼
    │  generate_sync_message()              │  generate_sync_message()
    │     │                                 │     │
    │     ▼                                 │     ▼
    │  TCP (binary wire) ◄──────────────────┤  TCP (binary wire)
    │     │                                 │     │
    │     ▼                                 │     ▼
    │  receive_sync_message()               │  receive_sync_message()
    │     │                                 │     │
    │     ▼                                 │     ▼
    │  Incremental rope reconciliation      │  Incremental rope reconciliation
    │     │                                 │     │
    │     ▼                                 │     ▼
    └── Render (with peer cursors)          └── Render (with peer cursors)
```

## Design Philosophy

1. **Core never panics**: All buffer operations return `Result`. Arithmetic uses `saturating_sub`, `checked_add`, etc.
2. **Non-blocking main loop**: AI API calls, LSP communication, MCP messages, and collaborative sync are all handled on background threads. The UI thread never blocks on I/O.
3. **Authorship is first-class**: Every edit carries an `AuthorId` (Human, AI, or Peer). This is baked into the buffer, not bolted on.
4. **Single source of truth**: The rope buffer is the canonical text state. CRDT, syntax tree, and LSP all derive from it. For collaborative editing, the CRDT is the sync source of truth; the rope is reconciled incrementally after each sync.
5. **Modes are explicit**: The app state machine has well-defined modes with clear transition rules, avoiding ambiguous states.

## Key Types

| Type | Crate | Purpose |
|------|-------|---------|
| `Buffer` | core | Rope-based text buffer with authorship-tagged edit history |
| `Cursor` | core | Row/column position in the buffer |
| `AuthorId` | core | Identifies who made an edit (Human, AI agent, or remote Peer) |
| `CrdtDoc` | core | Automerge CRDT document with sync API for collaborative editing |
| `PeerSyncState` | core | Per-peer automerge sync state tracking |
| `ConversationStore` | core | SQLite-backed conversation and decision history |
| `SemanticGraph` | core | Lightweight dependency graph (function calls, test coverage) |
| `App` | tui | Main application state: buffer, mode, cursor, UI state |
| `Mode` | tui | Editing mode enum (Normal, Insert, Visual, Command, Intent, Review) |
| `AnthropicClient` | ai | Streaming API client for Claude |
| `EditorContext` | ai | Assembled context sent with AI requests |
| `CollabSession` | tui | Manages a TCP collaboration session (host or client) |
