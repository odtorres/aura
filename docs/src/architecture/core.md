# Core Crate (`aura-core`)

The core crate provides the fundamental data structures for AURA. It has no dependency on any other workspace crate.

## Modules

| Module | Purpose |
|--------|---------|
| `buffer` | Rope-based text buffer with authorship-tagged edit history |
| `cursor` | Cursor position (row/col) and selection management |
| `author` | `AuthorId`, `Author`, and `AuthorColor` types |
| `crdt` | Automerge CRDT document wrapper with sync API |
| `sync` | Collaborative sync primitives (`PeerSyncState`, re-exports) |
| `conversation` | SQLite-backed conversation and decision storage |
| `semantic` | Lightweight semantic dependency graph |

## Buffer

The `Buffer` type wraps a `ropey::Rope` for efficient text manipulation on large files. Key properties:

- **Authorship tracking**: Every edit is tagged with an `AuthorId` (Human, AI agent, or remote Peer)
- **Edit history**: Maintains an undo stack where each entry records the author, position, and content
- **Per-author undo**: Can undo edits by a specific author without affecting other authors' changes
- **Collaborative sync**: `apply_remote_sync()` receives automerge sync messages and incrementally patches the rope; `load_remote_snapshot()` loads an initial document from a host
- **File I/O**: Streams file reading for large files; saves atomically

### Operations

- `insert(pos, text, author)` — Insert text at a character position
- `delete(start, end, author)` — Delete a character range
- `insert_char(cursor, char, author)` — Insert at a cursor position
- `backspace(cursor, author)` — Delete character before cursor
- `delete_line(row, author)` — Delete an entire line
- `undo()` / `undo_by_author(author)` — Undo last edit (global or per-author)
- `save()` — Write buffer to the associated file path

### Cursor Utilities

- `cursor_to_char_idx(cursor)` — Convert row/col to a character offset
- `char_idx_to_cursor(idx)` — Convert character offset to row/col
- `next_word_start(pos)` / `prev_word_start(pos)` / `word_end(pos)` — Word motion

## CRDT Layer

`CrdtDoc` wraps Automerge for conflict-free concurrent editing:

- `new()`, `splice()`, `text()` all return `Result` — no panics in the CRDT layer
- Each author gets a unique actor ID in the CRDT
- Edits are applied to both the rope (for rendering) and the CRDT (for conflict resolution). CRDT errors are logged but don't crash — the rope is the source of truth
- When multiple agents edit simultaneously, the CRDT ensures convergence
- Sync API: `generate_sync_message()` / `receive_sync_message()` for real-time collaboration
- `save_bytes()` / `load_bytes()` for document snapshots; `fork()` for creating peer copies

## Sync Module

`sync.rs` provides collaborative sync primitives:

- `PeerSyncState` — tracks the automerge sync state for a single remote peer (peer_id, name, sync state, author ID)
- Re-exports `automerge::sync::State` as `SyncState` and `automerge::sync::Message` as `SyncMessage`

## Conversation Store

`ConversationStore` uses SQLite (via `rusqlite`) to persist:

- **Conversations**: Linked to file path + line range + git commit
- **Messages**: Human intents and AI responses within a conversation
- **Decisions**: Accept/reject records with full context
- **Full-text search**: Query across all conversation history
- **Compaction**: `compact()` deletes old messages, trims per-conversation history, removes excess conversations. Configured via `CompactConfig` (max age, max per conversation, max total, keep recent)
- **AI summarization**: `get_summary()` / `update_summary()` for AI-generated conversation summaries. `conversations_needing_summary()` finds eligible conversations. `delete_messages_except_recent()` thins old messages after summarization

## Semantic Graph

`SemanticGraph` builds a lightweight dependency graph from Tree-sitter and LSP data:

- Tracks which functions call which
- Tracks which tests cover which functions
- Used for impact analysis ("affected by this change: X, Y, Z")
- Fed into AI context ("this function is called by 3 other functions")

## API Reference

See the [rustdoc for `aura-core`](/api/aura_core/).
